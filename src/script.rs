use std::{
    collections::{HashMap, HashSet},
    io,
    time::{Duration, Instant},
};

use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode},
};

use crate::{
    config::Timing,
    glyphs::Glyphs,
    model::{ActiveState, Activity, BootState, JobKind, JobResult, Phase, UnitFacts},
    origin::BootOrigin,
    palette::Palette,
    view::BootView,
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FINAL_FRAME: Duration = Duration::from_secs(3);

pub struct Step {
    unit: &'static str,
    after: &'static [&'static str],
    work: Duration,
    fails: bool,
    stops: bool,
}

#[must_use]
pub const fn step(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: false,
        stops: false,
    }
}

#[must_use]
pub const fn failing(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: true,
        stops: false,
    }
}

#[must_use]
pub const fn stopping(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: false,
        stops: true,
    }
}

pub struct Scenario {
    pub phase: Phase,
    pub steps: &'static [Step],
    pub after_switch_root: &'static [Step],
    pub describe: fn(&str) -> &'static str,
}

struct ScriptedJob {
    id: u32,
    step: &'static Step,
}

impl ScriptedJob {
    fn facts(&self, steps: &[Step], describe: fn(&str) -> &'static str) -> UnitFacts {
        let is_stopping = |unit: &&str| steps.iter().any(|step| step.unit == *unit && step.stops);
        let (before, after): (Vec<&str>, Vec<&str>) =
            self.step.after.iter().copied().partition(is_stopping);
        UnitFacts {
            name: self.step.unit.to_owned(),
            description: describe(self.step.unit).to_owned(),
            after: after.into_iter().map(ToString::to_string).collect(),
            before: before.into_iter().map(ToString::to_string).collect(),
        }
    }

    fn kind(&self) -> JobKind {
        if self.step.stops {
            JobKind::TakesDown
        } else {
            JobKind::BringsUp
        }
    }

    fn outcome(&self) -> JobResult {
        if self.step.fails {
            JobResult::Failed
        } else {
            JobResult::Done
        }
    }

    fn initial_activity(&self) -> Activity {
        Activity {
            state: self.step.stops.then_some(ActiveState::Active),
            ..Activity::default()
        }
    }

    fn started_activity(&self, now: Duration) -> Activity {
        if self.step.stops {
            Activity {
                state: Some(ActiveState::Deactivating),
                active_exit: Some(now),
                ..Activity::default()
            }
        } else {
            Activity {
                state: Some(ActiveState::Activating),
                inactive_exit: Some(now),
                ..Activity::default()
            }
        }
    }

    fn settled_activity(&self, now: Duration) -> Activity {
        let state = match (self.step.fails, self.step.stops) {
            (true, _) => ActiveState::Failed,
            (false, true) => ActiveState::Inactive,
            (false, false) => ActiveState::Active,
        };
        Activity {
            state: Some(state),
            active_enter: (!self.step.stops).then_some(now),
            ..Activity::default()
        }
    }
}

struct Script {
    jobs: Vec<ScriptedJob>,
    started: HashMap<&'static str, Duration>,
    finished: HashSet<&'static str>,
}

impl Script {
    fn begin(
        steps: &'static [Step],
        describe: fn(&str) -> &'static str,
        first_job: u32,
        state: &mut BootState,
        now: Duration,
    ) -> Self {
        let jobs: Vec<ScriptedJob> = steps
            .iter()
            .zip(first_job..)
            .map(|(step, id)| ScriptedJob { id, step })
            .collect();
        state.set_root(steps[0].unit.to_owned());
        jobs.iter().for_each(|job| {
            state.track_unit(job.facts(steps, describe), job.initial_activity());
        });
        jobs.iter()
            .for_each(|job| state.queue_job(job.id, job.step.unit, job.kind(), now));
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

    fn is_done(&self) -> bool {
        self.finished.len() == self.jobs.len()
    }

    fn is_due(&self, job: &ScriptedJob, now: Duration) -> bool {
        !self.finished.contains(job.step.unit)
            && self
                .started
                .get(job.step.unit)
                .is_some_and(|started| now >= *started + job.step.work)
    }

    fn advance(&mut self, state: &mut BootState, now: Duration) {
        let starting: HashSet<&'static str> = self
            .jobs
            .iter()
            .filter(|job| self.is_ready(job))
            .map(|job| job.step.unit)
            .collect();
        self.jobs
            .iter()
            .filter(|job| starting.contains(job.step.unit))
            .for_each(|job| {
                self.started.insert(job.step.unit, now);
                state.update_activity(job.step.unit, job.started_activity(now));
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
                state.update_activity(job.step.unit, job.settled_activity(now));
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

pub fn play(terminal: &mut DefaultTerminal, scenario: &Scenario) -> io::Result<()> {
    let clock = Instant::now();
    let now = || clock.elapsed();
    let origin = BootOrigin {
        host: "demo".to_owned(),
        system: "NixOS 26.11.20261007.39ad350".to_owned(),
        generation: "42".to_owned(),
        kernel: "Linux 7.2.9".to_owned(),
    };
    let mut state = BootState::new(scenario.phase);
    let mut script = Script::begin(scenario.steps, scenario.describe, 1, &mut state, now());
    let mut switched_root = scenario.after_switch_root.is_empty();
    let mut settled_at = None;
    let mut frame = 0;
    loop {
        script.advance(&mut state, now());
        if script.is_done() && !switched_root {
            state.switch_root(now());
            script = Script::begin(
                scenario.after_switch_root,
                scenario.describe,
                1000,
                &mut state,
                now(),
            );
            switched_root = true;
        }
        terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state: &state,
                    palette: &Palette::default(),
                    timing: &Timing::default(),
                    glyphs: &Glyphs::default(),
                    now: now(),
                    origin: &origin,
                    frame,
                    password_prompt: None,
                },
                canvas.area(),
            );
        })?;
        if script.is_done() && settled_at.get_or_insert_with(Instant::now).elapsed() >= FINAL_FRAME
        {
            return Ok(());
        }
        if wants_to_quit()? {
            return Ok(());
        }
        frame += 1;
    }
}
