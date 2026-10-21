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
    colors::Colors,
    config::Timing,
    glyphs::Glyphs,
    model::{ActiveState, Activity, BootState, JobKind, JobResult, Phase, UnitFacts},
    origin::BootOrigin,
    passphrase,
    view::{BootView, PasswordPrompt},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FINAL_FRAME: Duration = Duration::from_secs(3);

pub const DEMO_PASSPHRASE: &str = "demo";

pub struct Step {
    unit: &'static str,
    after: &'static [&'static str],
    work: Duration,
    fails: bool,
    stops: bool,
    asks: Option<&'static str>,
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

    fn prompt(&self, typed: &str, now: Duration) -> Option<PasswordPrompt<'static>> {
        Some(PasswordPrompt {
            message: self.asking()?.asks?,
            input: passphrase::masked(typed),
            sent: self.is_checking(now),
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

fn next_event() -> io::Result<Option<Event>> {
    if event::poll(FRAME_INTERVAL)? {
        event::read().map(Some)
    } else {
        Ok(None)
    }
}

fn quits(event: &Event, typing: bool) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Esc || (!typing && key.code == KeyCode::Char('q'))
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
    let mut typed = passphrase::empty();
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
                    colors: &Colors::default(),
                    timing: &Timing::default(),
                    glyphs: &Glyphs::default(),
                    now: now(),
                    origin: &origin,
                    frame,
                    password_prompt: script.prompt(&typed, now()),
                },
                canvas.area(),
            );
        })?;
        if script.is_done() && settled_at.get_or_insert_with(Instant::now).elapsed() >= FINAL_FRAME
        {
            return Ok(());
        }
        if let Some(event) = next_event()? {
            let asking = script.asking().is_some();
            if quits(&event, asking) {
                return Ok(());
            }
            if asking
                && !script.is_checking(now())
                && let Some(submitted) = passphrase::type_key(&mut typed, &event)
            {
                script.answer(&submitted, now());
            }
        }
        frame += 1;
    }
}
