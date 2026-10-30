use std::time::Duration;

use super::{ActiveState, BootState, Outcome, Progress};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MilestoneRow {
    pub label: String,
    pub finished_units: usize,
    pub started_at: Duration,
    pub reached_at: Duration,
}

struct MilestoneCursor {
    previous: Duration,
    consumed: usize,
}

impl BootState {
    fn milestone_reached_at(&self, target: &str) -> Option<Duration> {
        let record = self.units.get(target)?;
        match record.progress {
            Progress::Finished(Outcome::Succeeded { at }) => Some(at),
            Progress::Untouched => record.activity.active_enter.filter(|entered| {
                record.activity.state == Some(ActiveState::Active) && *entered >= self.origin
            }),
            Progress::Pending(_) | Progress::Finished(Outcome::Failed(_) | Outcome::Abandoned) => {
                None
            }
        }
    }

    #[must_use]
    pub fn milestones(&self) -> Vec<MilestoneRow> {
        let mut succeeded: Vec<Duration> = self
            .units
            .values()
            .filter_map(|record| match record.progress {
                Progress::Finished(Outcome::Succeeded { at }) => Some(at),
                _ => None,
            })
            .collect();
        succeeded.sort_unstable();
        let start = MilestoneCursor {
            previous: self
                .finished_stages
                .last()
                .map_or(self.origin, |stage| stage.reached_at),
            consumed: 0,
        };
        let reached_rows = self
            .milestone_targets
            .iter()
            .scan(start, |cursor, target| {
                Some(self.milestone_reached_at(target).map(|reached_at| {
                    let upto = succeeded.partition_point(|at| *at <= reached_at);
                    let row = MilestoneRow {
                        label: target.clone(),
                        finished_units: upto.saturating_sub(cursor.consumed),
                        started_at: cursor.previous.min(reached_at),
                        reached_at,
                    };
                    cursor.consumed = cursor.consumed.max(upto);
                    cursor.previous = reached_at;
                    row
                }))
            })
            .flatten();
        self.finished_stages
            .iter()
            .map(|stage| MilestoneRow {
                label: stage.label.to_owned(),
                finished_units: stage.finished_units,
                started_at: stage.started_at,
                reached_at: stage.reached_at,
            })
            .chain(reached_rows)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        boot_model::{Activity, JobResult, Phase},
        boot_test_fixtures::{MS, nested_boot_sample, queued_boot, waiting, waiting_rows},
    };

    #[test]
    fn finished_jobs_leave_the_tree_and_count_toward_their_milestone() {
        let mut state = nested_boot_sample();
        state.finish_job(6, JobResult::Done, 3500 * MS);
        state.finish_job(7, JobResult::Done, 4000 * MS);
        state.update_activity(
            "basic.target",
            Activity {
                state: Some(ActiveState::Active),
                inactive_exit: None,
                active_enter: Some(4000 * MS),
                active_exit: None,
            },
        );
        assert!(
            waiting_rows(&state)
                .iter()
                .all(|row| row.label != "containerd" && row.label != "basic.target")
        );
        let milestones = state.milestones();
        assert_eq!(milestones.len(), 1);
        assert_eq!(milestones[0].label, "basic.target");
        assert_eq!(milestones[0].finished_units, 2);
    }

    #[test]
    fn shutdown_counts_toward_its_own_milestones_and_never_completes() {
        let mut state = queued_boot(
            Phase::Shutdown,
            Duration::ZERO,
            [
                waiting("poweroff.target", &[]),
                waiting("shutdown.target", &[]),
                waiting("db.service", &[]).stopping(),
            ],
        );
        state.finish_job(3, JobResult::Done, 500 * MS);
        state.finish_job(2, JobResult::Done, 600 * MS);
        state.finish_job(1, JobResult::Done, 700 * MS);
        let milestones = state.milestones();
        assert_eq!(milestones[0].label, "shutdown.target");
        assert_eq!(milestones[0].finished_units, 2);
        assert!(!state.is_complete());
    }
}
