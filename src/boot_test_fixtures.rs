use std::{iter::once, time::Duration};

use crate::boot_model::{ActiveState, Activity, BootState, JobKind, Phase, TreeRow, UnitFacts};

pub const MS: Duration = Duration::from_millis(1);

pub struct QueuedUnit {
    pub facts: UnitFacts,
    pub activity: Activity,
    pub kind: JobKind,
}

pub fn facts(name: &str, after: &[&str]) -> UnitFacts {
    UnitFacts {
        name: name.to_owned(),
        description: String::new(),
        after: after.iter().map(ToString::to_string).collect(),
        before: Vec::new(),
    }
}

pub fn activating(since: Duration) -> Activity {
    Activity {
        state: Some(ActiveState::Activating),
        inactive_exit: Some(since),
        ..Activity::default()
    }
}

pub fn waiting(name: &str, after: &[&str]) -> QueuedUnit {
    QueuedUnit {
        facts: facts(name, after),
        activity: Activity::default(),
        kind: JobKind::BringsUp,
    }
}

pub fn running(name: &str, after: &[&str], since: Duration) -> QueuedUnit {
    QueuedUnit {
        activity: activating(since),
        ..waiting(name, after)
    }
}

impl QueuedUnit {
    pub fn stopping(self) -> Self {
        Self {
            kind: JobKind::TakesDown,
            ..self
        }
    }

    pub fn ordered_before(mut self, before: &[&str]) -> Self {
        self.facts.before = before.iter().map(ToString::to_string).collect();
        self
    }

    pub fn described(mut self, description: &str) -> Self {
        description.clone_into(&mut self.facts.description);
        self
    }
}

pub fn queued_boot(
    phase: Phase,
    queued_at: Duration,
    units: impl IntoIterator<Item = QueuedUnit>,
) -> BootState {
    let mut state = BootState::new(phase);
    units.into_iter().zip(1..).for_each(|(unit, id)| {
        if id == 1 {
            state.set_root(unit.facts.name.clone());
        }
        queue_unit(&mut state, id, unit, queued_at);
    });
    state
}

pub fn nested_boot_sample() -> BootState {
    queued_boot(
        Phase::Host,
        500 * MS,
        [
            waiting("graphical.target", &["multi-user.target"]),
            waiting(
                "multi-user.target",
                &["docker.service", "nm-wait.service", "basic.target"],
            ),
            waiting("nm-wait.service", &["nm.service"]),
            running("nm.service", &[], 1000 * MS),
            running("docker.service", &["containerd.service"], 3000 * MS),
            waiting("containerd.service", &[]),
            waiting("basic.target", &[]),
        ],
    )
}

pub fn queue_unit(state: &mut BootState, id: u32, unit: QueuedUnit, queued_at: Duration) {
    let name = unit.facts.name.clone();
    state.track_unit(unit.facts, unit.activity);
    state.queue_job(id, &name, unit.kind, queued_at);
}

pub fn tree_outline(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let chain = once(row.label.as_str())
                .chain(row.chain.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" > ");
            format!("{}{chain}", "  ".repeat(row.depth))
        })
        .collect()
}
