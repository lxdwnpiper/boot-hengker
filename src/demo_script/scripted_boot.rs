use std::{
    collections::{HashMap, HashSet},
    io,
    ops::ControlFlow,
    time::Duration,
};

use futures::stream;
use ratatui::{DefaultTerminal, crossterm::event::Event};
use tokio::runtime::Builder;
use zeroize::Zeroizing;

use crate::{
    boot_model::{
        ActiveState, Activity, BootState, JobKind, JobResult, Phase, UnitFacts, UnitTimeouts,
        boot_origin::BootOrigin,
    },
    boot_view::PasswordPrompt,
    console_session::{
        console_palette::ConsolePalette,
        passphrase_input,
        stream_sources::{frame_inputs, key_inputs},
        watch_screen::{ConsoleDesk, WatchEnding, WatchScreen, watch_until_ending},
    },
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
    systemd_link::monotonic_clock::monotonic_now,
};

const FINAL_FRAME: Duration = Duration::from_secs(3);

pub const DEMO_PASSPHRASE: &str = "demo";
const DEMO_PASSPHRASE_HINT: &str = "This demo unlocks with the passphrase \"demo\"";

pub struct Step {
    unit: &'static str,
    after: &'static [&'static str],
    work: Duration,
    fails: bool,
    stops: bool,
    asks: Option<&'static str>,
    timeout: Option<Duration>,
}

impl Step {
    #[must_use]
    pub const fn timing_out_after(self, limit: Duration) -> Self {
        Self {
            timeout: Some(limit),
            ..self
        }
    }
}

#[must_use]
pub const fn step(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        unit,
        after,
        work,
        fails: false,
        stops: false,
        asks: None,
        timeout: None,
    }
}

#[must_use]
pub const fn failing(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        fails: true,
        ..step(unit, after, work)
    }
}

#[must_use]
pub const fn stopping(unit: &'static str, after: &'static [&'static str], work: Duration) -> Step {
    Step {
        stops: true,
        ..step(unit, after, work)
    }
}

#[must_use]
pub const fn unlocking(
    unit: &'static str,
    after: &'static [&'static str],
    message: &'static str,
    key_derivation: Duration,
) -> Step {
    Step {
        asks: Some(message),
        ..step(unit, after, key_derivation)
    }
}

pub struct Scenario {
    pub phase: Phase,
    pub steps: &'static [Step],
    pub after_switch_root: &'static [Step],
    pub describe: fn(&str) -> Option<&'static str>,
}

struct ScriptedJob {
    id: u32,
    step: &'static Step,
}

impl ScriptedJob {
    fn facts(&self, steps: &[Step], describe: fn(&str) -> Option<&'static str>) -> UnitFacts {
        let is_stopping = |unit: &&str| steps.iter().any(|step| step.unit == *unit && step.stops);
        let (before, after): (Vec<&str>, Vec<&str>) =
            self.step.after.iter().copied().partition(is_stopping);
        UnitFacts {
            name: self.step.unit.to_owned(),
            description: describe(self.step.unit).map(str::to_owned),
            after: after.into_iter().map(ToString::to_string).collect(),
            before: before.into_iter().map(ToString::to_string).collect(),
            timeouts: UnitTimeouts {
                job_running: self.step.timeout,
                ..UnitTimeouts::default()
            },
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
        let state = if self.step.fails {
            ActiveState::Failed
        } else if self.step.stops {
            ActiveState::Inactive
        } else {
            ActiveState::Active
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
    answer: Option<Answer>,
}

#[derive(Clone, Copy)]
struct Answer {
    checked_at: Duration,
    unlocks: bool,
}

impl Script {
    fn begin(
        steps: &'static [Step],
        describe: fn(&str) -> Option<&'static str>,
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
            answer: None,
        }
    }

    fn asking(&self) -> Option<&'static Step> {
        self.jobs.iter().map(|job| job.step).find(|step| {
            step.asks.is_some()
                && self.started.contains_key(step.unit)
                && !self.finished.contains(step.unit)
        })
    }

    fn is_checking(&self, now: Duration) -> bool {
        self.answer.is_some_and(|answer| now < answer.checked_at)
    }

    fn was_rejected(&self, now: Duration) -> bool {
        self.answer
            .is_some_and(|answer| !answer.unlocks && now >= answer.checked_at)
    }

    fn prompt(&self, typed: &str, now: Duration) -> Option<PasswordPrompt<'static>> {
        Some(PasswordPrompt {
            message: self.asking()?.asks?,
            hint: Some(DEMO_PASSPHRASE_HINT),
            input: passphrase_input::masked(typed),
            sent: self.is_checking(now),
            rejected: self.was_rejected(now),
        })
    }

    fn answer(&mut self, submitted: &str, now: Duration) {
        let Some(step) = self.asking() else {
            return;
        };
        self.answer = Some(Answer {
            checked_at: now + step.work,
            unlocks: submitted == DEMO_PASSPHRASE,
        });
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
        let done_at = match job.step.asks {
            Some(_) => self
                .answer
                .filter(|answer| answer.unlocks)
                .map(|answer| answer.checked_at),
            None => self
                .started
                .get(job.step.unit)
                .map(|started| *started + job.step.work),
        };
        !self.finished.contains(job.step.unit) && done_at.is_some_and(|done_at| now >= done_at)
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

struct DemoDesk {
    script: Script,
    after_switch_root: Option<&'static [Step]>,
    describe: fn(&str) -> Option<&'static str>,
    settled_at: Option<Duration>,
    typed: Zeroizing<String>,
    glyphs: Glyphs,
}

impl ConsoleDesk for DemoDesk {
    fn frame_passed(&mut self, state: &mut BootState, now: Duration) -> ControlFlow<WatchEnding> {
        self.script.advance(state, now);
        if self.script.is_done()
            && let Some(steps) = self.after_switch_root.take()
        {
            state.switch_root(now);
            self.script = Script::begin(steps, self.describe, 1000, state, now);
        }
        if !self.script.is_done() {
            return ControlFlow::Continue(());
        }
        let settled_at = *self.settled_at.get_or_insert(now);
        if now.saturating_sub(settled_at) >= FINAL_FRAME {
            ControlFlow::Break(WatchEnding::BootCompleted)
        } else {
            ControlFlow::Continue(())
        }
    }

    fn glyphs(&self) -> &Glyphs {
        &self.glyphs
    }

    fn palette(&self) -> ConsolePalette {
        ConsolePalette::Terminal
    }

    fn prompt(&self, now: Duration) -> Option<PasswordPrompt<'_>> {
        self.script.prompt(&self.typed, now)
    }

    fn press_key(&mut self, event: &Event, now: Duration) -> ControlFlow<WatchEnding> {
        let asking = self.script.asking().is_some();
        if passphrase_input::asks_to_quit(event, asking) {
            return ControlFlow::Break(WatchEnding::ViewerLeft);
        }
        if asking
            && !self.script.is_checking(now)
            && let Some(submitted) = passphrase_input::type_key(&mut self.typed, event)
        {
            self.script.answer(&submitted, now);
        }
        ControlFlow::Continue(())
    }
}

fn demo_origin() -> BootOrigin {
    BootOrigin {
        host: "demo".to_owned(),
        system: Some("NixOS 26.11.20261007.39ad350".to_owned()),
        generation: Some("42".to_owned()),
        kernel: "Linux 7.2.9".to_owned(),
    }
}

async fn play_scenario(terminal: &mut DefaultTerminal, scenario: &Scenario) -> io::Result<()> {
    let mut state = BootState::new(scenario.phase);
    let script = Script::begin(
        scenario.steps,
        scenario.describe,
        1,
        &mut state,
        Duration::ZERO,
    );
    let desk = DemoDesk {
        script,
        after_switch_root: (!scenario.after_switch_root.is_empty())
            .then_some(scenario.after_switch_root),
        describe: scenario.describe,
        settled_at: None,
        typed: passphrase_input::empty(),
        glyphs: Glyphs::default(),
    };
    let mut screen = WatchScreen::new(
        state,
        desk,
        demo_origin(),
        Colors::default(),
        Timing::default(),
        monotonic_now(),
    );
    let inputs = stream::select(frame_inputs(), key_inputs()?);
    match watch_until_ending(&mut screen, terminal, inputs).await {
        WatchEnding::Failed(error) => Err(io::Error::other(error)),
        WatchEnding::BootCompleted | WatchEnding::ViewerLeft => Ok(()),
    }
}

pub fn play(terminal: &mut DefaultTerminal, scenario: &Scenario) -> io::Result<()> {
    Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?
        .block_on(play_scenario(terminal, scenario))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_DERIVATION: Duration = Duration::from_millis(500);
    const UNLOCK_ONLY: &[Step] = &[unlocking(
        "systemd-cryptsetup@root.service",
        &[],
        "Please enter passphrase for disk root:",
        KEY_DERIVATION,
    )];

    struct AskingDemo {
        script: Script,
        state: BootState,
    }

    fn asking_demo() -> AskingDemo {
        let mut state = BootState::new(Phase::Initrd);
        let mut script = Script::begin(UNLOCK_ONLY, |_| None, 1, &mut state, Duration::ZERO);
        script.advance(&mut state, Duration::ZERO);
        AskingDemo { script, state }
    }

    #[test]
    fn the_hint_names_the_demo_passphrase() {
        assert!(DEMO_PASSPHRASE_HINT.contains(&format!("\"{DEMO_PASSPHRASE}\"")));
    }

    #[test]
    fn a_wrong_answer_is_checked_then_called_out() {
        let AskingDemo { mut script, .. } = asking_demo();
        let asked = script.prompt("", Duration::ZERO).expect("asking");
        assert_eq!(asked.hint, Some(DEMO_PASSPHRASE_HINT));
        assert!(!asked.rejected);
        script.answer("wrong", Duration::ZERO);
        let checking = script.prompt("", KEY_DERIVATION / 2).expect("asking");
        assert!(checking.sent && !checking.rejected);
        let rejected = script.prompt("", KEY_DERIVATION).expect("asking again");
        assert!(rejected.rejected && !rejected.sent);
    }

    #[test]
    fn the_right_answer_unlocks_and_closes_the_prompt() {
        let AskingDemo {
            mut script,
            mut state,
        } = asking_demo();
        script.answer(DEMO_PASSPHRASE, Duration::ZERO);
        script.advance(&mut state, KEY_DERIVATION);
        assert!(script.is_done());
        assert!(script.prompt("", KEY_DERIVATION).is_none());
    }
}
