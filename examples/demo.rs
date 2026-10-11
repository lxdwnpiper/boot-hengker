use std::{
    collections::{HashMap, HashSet},
    io,
    time::{Duration, Instant},
};

use boot_tree::{
    model::{ActiveState, Activity, BootState, JobResult, Phase, UnitFacts},
    palette::Palette,
    view::BootView,
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FINAL_FRAME: Duration = Duration::from_secs(3);

struct Step {
    unit: &'static str,
    after: &'static [&'static str],
    work: Duration,
    fails: bool,
}

const fn step(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: false,
    }
}

const fn failing(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: true,
    }
}

const INITRD: &[Step] = &[
    step(
        "initrd.target",
        &["initrd-fs.target", "initrd-root-device.target"],
        Duration::ZERO,
    ),
    step("initrd-fs.target", &["sysroot.mount"], Duration::ZERO),
    step(
        "initrd-root-device.target",
        &["dev-disk-by\\x2dlabel-nixos.device"],
        Duration::ZERO,
    ),
    step(
        "sysroot.mount",
        &["systemd-fsck-root.service"],
        Duration::from_millis(300),
    ),
    step(
        "systemd-fsck-root.service",
        &["dev-disk-by\\x2dlabel-nixos.device"],
        Duration::from_millis(500),
    ),
    step(
        "dev-disk-by\\x2dlabel-nixos.device",
        &[],
        Duration::from_millis(1200),
    ),
    step("systemd-udevd.service", &[], Duration::from_millis(400)),
    step(
        "demo-initrd-probe.service",
        &[],
        Duration::from_millis(2500),
    ),
];

const HOST: &[Step] = &[
    step(
        "graphical.target",
        &["multi-user.target", "display-manager.service"],
        Duration::ZERO,
    ),
    step(
        "display-manager.service",
        &["multi-user.target"],
        Duration::from_millis(400),
    ),
    step(
        "multi-user.target",
        &[
            "basic.target",
            "NetworkManager-wait-online.service",
            "docker.service",
            "tailscaled.service",
            "nvidia-powerd.service",
            "systemd-rfkill.service",
        ],
        Duration::ZERO,
    ),
    step("basic.target", &["sysinit.target"], Duration::ZERO),
    step(
        "sysinit.target",
        &[
            "systemd-tmpfiles-setup.service",
            "systemd-udev-trigger.service",
        ],
        Duration::ZERO,
    ),
    step(
        "systemd-tmpfiles-setup.service",
        &[],
        Duration::from_millis(600),
    ),
    step(
        "systemd-udev-trigger.service",
        &[],
        Duration::from_millis(900),
    ),
    step(
        "NetworkManager.service",
        &["basic.target"],
        Duration::from_millis(1500),
    ),
    step(
        "NetworkManager-wait-online.service",
        &["NetworkManager.service"],
        Duration::from_millis(3500),
    ),
    step(
        "network-online.target",
        &["NetworkManager-wait-online.service"],
        Duration::ZERO,
    ),
    step(
        "tailscaled.service",
        &["network-online.target"],
        Duration::from_millis(1200),
    ),
    step(
        "containerd.service",
        &["basic.target"],
        Duration::from_millis(800),
    ),
    step(
        "docker.service",
        &["containerd.service", "network-online.target"],
        Duration::from_millis(1500),
    ),
    failing(
        "nvidia-powerd.service",
        &["basic.target"],
        Duration::from_millis(700),
    ),
    step(
        "sys-devices-virtual-misc-rfkill.device",
        &[],
        Duration::from_millis(4000),
    ),
    step(
        "systemd-rfkill.service",
        &["sys-devices-virtual-misc-rfkill.device"],
        Duration::from_millis(300),
    ),
];

struct ScriptedJob {
    id: u32,
    step: &'static Step,
}

impl ScriptedJob {
    fn facts(&self) -> UnitFacts {
        UnitFacts {
            name: self.step.unit.to_owned(),
            description: self.step.unit.to_owned(),
            after: self.step.after.iter().map(ToString::to_string).collect(),
        }
    }

    fn outcome(&self) -> JobResult {
        if self.step.fails {
            JobResult::Failed
        } else {
            JobResult::Done
        }
    }

    fn settled_state(&self) -> ActiveState {
        if self.step.fails {
            ActiveState::Failed
        } else {
            ActiveState::Active
        }
    }
}

struct Script {
    jobs: Vec<ScriptedJob>,
    started: HashMap<&'static str, Duration>,
    finished: HashSet<&'static str>,
}

impl Script {
    fn begin(steps: &'static [Step], first_job: u32, state: &mut BootState, now: Duration) -> Self {
        let jobs: Vec<ScriptedJob> = steps
            .iter()
            .zip(first_job..)
            .map(|(step, id)| ScriptedJob { id, step })
            .collect();
        state.set_root(steps[0].unit.to_owned());
        jobs.iter()
            .for_each(|job| state.track_unit(job.facts(), Activity::default()));
        jobs.iter()
            .for_each(|job| state.queue_job(job.id, job.step.unit, now));
        Self {
            jobs,
            started: HashMap::new(),
            finished: HashSet::new(),
        }
    }

    fn is_ready(&self, job: &ScriptedJob) -> bool {
        !self.started.contains_key(job.step.unit)
            && job
                .step
                .after
                .iter()
                .all(|dependency| self.finished.contains(dependency))
    }

    fn is_due(&self, job: &ScriptedJob, now: Duration) -> bool {
        !self.finished.contains(job.step.unit)
            && self
                .started
                .get(job.step.unit)
                .is_some_and(|started| now >= *started + job.step.work)
    }

    fn advance(&mut self, state: &mut BootState, now: Duration) {
        let starting: Vec<&'static str> = self
            .jobs
            .iter()
            .filter(|job| self.is_ready(job))
            .map(|job| job.step.unit)
            .collect();
        starting.iter().for_each(|unit| {
            self.started.insert(unit, now);
            state.update_activity(
                unit,
                Activity {
                    state: Some(ActiveState::Activating),
                    inactive_exit: Some(now),
                    active_enter: None,
                },
            );
        });
        let due: HashSet<&'static str> = self
            .jobs
            .iter()
            .filter(|job| self.is_due(job, now))
            .map(|job| job.step.unit)
            .collect();
        self.jobs
            .iter()
            .filter(|job| due.contains(job.step.unit))
            .for_each(|job| {
                state.update_activity(
                    job.step.unit,
                    Activity {
                        state: Some(job.settled_state()),
                        inactive_exit: None,
                        active_enter: Some(now),
                    },
                );
                state.finish_job(job.id, job.outcome(), now);
            });
        self.finished.extend(due);
    }
}

fn wants_to_quit() -> io::Result<bool> {
    if !event::poll(FRAME_INTERVAL)? {
        return Ok(false);
    }
    Ok(
        matches!(event::read()?, Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)),
    )
}

fn play(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let clock = Instant::now();
    let now = || clock.elapsed();
    let mut state = BootState::new(Phase::Initrd);
    let mut script = Script::begin(INITRD, 1, &mut state, now());
    let mut frame = 0;
    let mut completed_at = None;
    loop {
        script.advance(&mut state, now());
        if state.phase == Phase::Initrd && script.finished.len() == script.jobs.len() {
            state.switch_root(now());
            script = Script::begin(HOST, 1000, &mut state, now());
        }
        terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state: &state,
                    palette: &Palette::default(),
                    now: now(),
                    host: "demo",
                    frame,
                },
                canvas.area(),
            );
        })?;
        if state.is_complete() {
            let completed = *completed_at.get_or_insert_with(Instant::now);
            if completed.elapsed() >= FINAL_FRAME {
                return Ok(());
            }
        }
        if wants_to_quit()? {
            return Ok(());
        }
        frame += 1;
    }
}

fn main() -> io::Result<()> {
    ratatui::run(play)
}
