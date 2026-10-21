use std::time::Duration;

use super::{Activity, BootState, JobKind, JobResult, UnitFacts};

pub enum BootChange {
    TrackUnit {
        facts: UnitFacts,
        activity: Activity,
    },
    UpdateActivity {
        unit: String,
        activity: Activity,
    },
    QueueJob {
        id: u32,
        unit: String,
        kind: JobKind,
        at: Duration,
    },
    FinishJob {
        id: u32,
        result: JobResult,
        at: Duration,
    },
    SetRoot(String),
    SetHandoff(String),
    SetOrigin(Duration),
    SwitchRoot {
        at: Duration,
    },
    ForgetJobs,
    ManagerExited {
        at: Duration,
    },
}

impl BootState {
    pub fn apply(&mut self, change: BootChange) {
        match change {
            BootChange::TrackUnit { facts, activity } => self.track_unit(facts, activity),
            BootChange::UpdateActivity { unit, activity } => self.update_activity(&unit, activity),
            BootChange::QueueJob { id, unit, kind, at } => self.queue_job(id, &unit, kind, at),
            BootChange::FinishJob { id, result, at } => self.finish_job(id, result, at),
            BootChange::SetRoot(root) => self.set_root(root),
            BootChange::SetHandoff(unit) => self.set_handoff(unit),
            BootChange::SetOrigin(origin) => self.origin = origin,
            BootChange::SwitchRoot { at } => self.switch_root(at),
            BootChange::ForgetJobs => self.forget_jobs(),
            BootChange::ManagerExited { at } => self.manager_exited_at = Some(at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        boot_model::Phase,
        demo_script::boot_test_fixtures::{MS, facts, waiting_rows},
    };

    #[test]
    fn applied_changes_match_the_direct_calls() {
        let mut applied = BootState::new(Phase::Host);
        [
            BootChange::SetOrigin(100 * MS),
            BootChange::TrackUnit {
                facts: facts("web.service", &[]),
                activity: Activity::default(),
            },
            BootChange::SetRoot("web.service".to_owned()),
            BootChange::QueueJob {
                id: 1,
                unit: "web.service".to_owned(),
                kind: JobKind::BringsUp,
                at: 200 * MS,
            },
            BootChange::FinishJob {
                id: 1,
                result: JobResult::Done,
                at: 900 * MS,
            },
        ]
        .into_iter()
        .for_each(|change| applied.apply(change));
        let mut called = BootState::new(Phase::Host);
        called.origin = 100 * MS;
        called.track_unit(facts("web.service", &[]), Activity::default());
        called.set_root("web.service".to_owned());
        called.queue_job(1, "web.service", JobKind::BringsUp, 200 * MS);
        called.finish_job(1, JobResult::Done, 900 * MS);
        assert_eq!(applied.tally(), called.tally());
        assert_eq!(applied.is_complete(), called.is_complete());
        assert_eq!(waiting_rows(&applied), waiting_rows(&called));
    }
}
