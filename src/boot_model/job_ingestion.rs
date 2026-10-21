use std::{collections::HashMap, time::Duration};

use super::{
    ActiveState, Activity, BootState, FinishedRun, FinishedStage, HANDOFF_UNIT, HOST_MILESTONES,
    JobKind, JobResult, Outcome, PendingJob, Phase, Progress, Root, SHUTDOWN_MILESTONES, Tally,
    UnitFacts, UnitKind, UnitRecord, unit_facts::display_label,
};

impl UnitRecord {
    fn transition_started(&self, kind: JobKind) -> Option<Duration> {
        match kind {
            JobKind::TakesDown => self.activity.active_exit,
            JobKind::BringsUp | JobKind::Unknown => self.activity.inactive_exit,
        }
    }

    pub(super) fn running_since(&self) -> Option<Duration> {
        let Progress::Pending(job) = self.progress else {
            return None;
        };
        let transitioning = matches!(
            self.activity.state,
            Some(ActiveState::Activating | ActiveState::Deactivating | ActiveState::Reloading)
        );
        transitioning.then(|| self.transition_started(job.kind).unwrap_or(job.queued_at))
    }

    fn is_pending(&self) -> bool {
        matches!(self.progress, Progress::Pending(_))
    }

    fn is_finished(&self) -> bool {
        matches!(self.progress, Progress::Finished(_))
    }

    pub(super) fn takes_down(&self) -> bool {
        matches!(
            self.progress,
            Progress::Pending(PendingJob {
                kind: JobKind::TakesDown,
                ..
            })
        )
    }
}

impl BootState {
    #[must_use]
    pub fn new(phase: Phase) -> Self {
        let (milestone_targets, handoff_unit) = match phase {
            Phase::Initrd | Phase::Host => (
                HOST_MILESTONES.iter().map(ToString::to_string).collect(),
                Some(HANDOFF_UNIT.to_owned()),
            ),
            Phase::Shutdown => (
                SHUTDOWN_MILESTONES
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                None,
            ),
            Phase::User => (Vec::new(), None),
        };
        Self {
            phase,
            origin: Duration::ZERO,
            root: Root::Unresolved,
            failed: Vec::new(),
            finished_stages: Vec::new(),
            finished_runs: Vec::new(),
            handoff_unit,
            manager_exited_at: None,
            milestone_targets,
            boot_finished: false,
            finished_before_reconnect: 0,
            units: HashMap::new(),
            jobs: HashMap::new(),
        }
    }

    pub fn set_root(&mut self, root: String) {
        if self.phase == Phase::User && !self.milestone_targets.contains(&root) {
            self.milestone_targets.push(root.clone());
        }
        if self.root.unit() != Some(&root) {
            self.root = Root::Awaiting(root);
        }
    }

    pub fn set_handoff(&mut self, unit: String) {
        if self.handoff_unit.is_some() {
            self.handoff_unit = Some(unit);
        }
    }

    pub fn mark_boot_finished(&mut self) {
        self.boot_finished = true;
    }

    pub fn track_unit(&mut self, facts: UnitFacts, activity: Activity) {
        let record = self
            .units
            .entry(facts.name.clone())
            .or_insert_with(|| UnitRecord {
                facts: facts.clone(),
                activity: Activity::default(),
                progress: Progress::Untouched,
            });
        record.facts = facts;
        record.activity = record.activity.merge(activity);
    }

    pub fn update_activity(&mut self, unit: &str, activity: Activity) {
        if let Some(record) = self.units.get_mut(unit) {
            record.activity = record.activity.merge(activity);
        }
    }

    pub fn queue_job(&mut self, id: u32, unit: &str, kind: JobKind, at: Duration) {
        self.enqueue(id, unit, kind, at);
        if kind == JobKind::BringsUp
            && self.jobs.get(&id).is_some_and(|tracked| tracked == unit)
            && self.root == Root::Awaiting(unit.to_owned())
        {
            self.root = Root::Queued(unit.to_owned());
        }
    }

    fn enqueue(&mut self, id: u32, unit: &str, kind: JobKind, at: Duration) {
        if self.jobs.contains_key(&id) {
            return;
        }
        let Some(record) = self.units.get_mut(unit) else {
            return;
        };
        record.progress = Progress::Pending(PendingJob {
            id,
            queued_at: at,
            kind,
        });
        self.jobs.insert(id, unit.to_owned());
    }

    pub fn finish_job(&mut self, id: u32, result: JobResult, at: Duration) {
        let Some(unit) = self.jobs.remove(&id) else {
            return;
        };
        let Some(record) = self.units.get_mut(&unit) else {
            return;
        };
        let outcome = match result {
            JobResult::Done => Outcome::Succeeded { at },
            JobResult::Skipped | JobResult::Canceled => Outcome::Abandoned,
            JobResult::Failed | JobResult::Timeout | JobResult::Dependency => Outcome::Failed,
        };
        let label = display_label(&record.facts);
        if outcome == Outcome::Failed && !self.failed.contains(&label) {
            self.failed.push(label.clone());
        }
        let job = match record.progress {
            Progress::Pending(job) if job.id == id => job,
            _ => return,
        };
        record.progress = Progress::Finished(outcome);
        if let Some(started_at) = record
            .transition_started(job.kind)
            .filter(|started| job.kind != JobKind::Unknown && (self.origin..=at).contains(started))
            && UnitKind::of(&record.facts.name) != UnitKind::Target
        {
            self.finished_runs.push(FinishedRun {
                label,
                started_at,
                finished_at: at,
                failed: outcome == Outcome::Failed,
            });
        }
    }

    pub fn switch_root(&mut self, at: Duration) {
        let finished_here = self
            .units
            .values()
            .filter(|record| record.is_finished())
            .count();
        self.finished_stages.push(FinishedStage {
            label: Phase::Initrd.label(),
            finished_units: finished_here + self.finished_before_reconnect,
            failed_units: self
                .units
                .values()
                .filter(|record| record.progress == Progress::Finished(Outcome::Failed))
                .count(),
            started_at: self.origin,
            reached_at: at,
        });
        self.phase = Phase::Host;
        self.root = Root::Unresolved;
        self.finished_before_reconnect = 0;
        self.units.clear();
        self.jobs.clear();
    }

    pub fn forget_jobs(&mut self) {
        self.finished_before_reconnect += self
            .units
            .values()
            .filter(|record| record.is_finished())
            .count();
        self.jobs.clear();
        self.units
            .values_mut()
            .for_each(|record| record.progress = Progress::Untouched);
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        if matches!(self.phase, Phase::Initrd | Phase::Shutdown) {
            return false;
        }
        if self.boot_finished {
            return true;
        }
        let Root::Queued(root) = &self.root else {
            return false;
        };
        self.units.get(root).is_some_and(|root| !root.is_pending())
    }

    #[must_use]
    pub fn tally(&self) -> Tally {
        let carried = Tally {
            done: self
                .finished_stages
                .iter()
                .map(|stage| stage.finished_units)
                .sum::<usize>()
                + self.finished_before_reconnect,
            failed: self.failed.len(),
            ..Tally::default()
        };
        self.units.values().fold(carried, |mut tally, record| {
            match record.progress {
                Progress::Pending(_) if record.running_since().is_some() => tally.running += 1,
                Progress::Pending(_) => tally.waiting += 1,
                Progress::Finished(Outcome::Succeeded { .. } | Outcome::Abandoned) => {
                    tally.done += 1;
                }
                Progress::Finished(Outcome::Failed) | Progress::Untouched => {}
            }
            tally
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        boot_model::RowStatus,
        demo_script::boot_test_fixtures::{
            MS, activating, facts, nested_boot_sample as sample, queue_unit, queued_boot, waiting,
            waiting_rows,
        },
    };

    #[test]
    fn failures_are_listed_and_not_counted_as_done() {
        let mut state = sample();
        state.finish_job(4, JobResult::Failed, 2000 * MS);
        assert_eq!(state.failed, ["nm"]);
        let tally = state.tally();
        assert_eq!(tally.failed, 1);
        assert_eq!(tally.done, 0);
        assert_eq!(tally.running, 1);
        assert_eq!(tally.waiting, 5);
    }

    #[test]
    fn boot_completes_when_the_root_job_is_gone() {
        let mut state = sample();
        assert!(!state.is_complete());
        (1..=7).for_each(|id| state.finish_job(id, JobResult::Done, 5000 * MS));
        assert!(state.is_complete());
    }

    #[test]
    fn display_manager_activation_does_not_hand_the_console_over() {
        let mut state = sample();
        queue_unit(&mut state, 20, waiting(HANDOFF_UNIT, &[]), 500 * MS);
        state.update_activity(HANDOFF_UNIT, activating(6000 * MS));
        assert!(!state.is_complete());
    }

    #[test]
    fn boot_finished_hands_the_console_over() {
        let mut state = sample();
        queue_unit(&mut state, 20, waiting(HANDOFF_UNIT, &[]), 500 * MS);
        state.mark_boot_finished();
        assert!(state.is_complete());
    }

    #[test]
    fn boot_finished_is_ignored_in_the_initrd() {
        let mut state = BootState::new(Phase::Initrd);
        state.mark_boot_finished();
        assert!(!state.is_complete());
    }

    #[test]
    fn switching_root_freezes_the_initrd_summary() {
        let mut state = queued_boot(
            Phase::Initrd,
            Duration::ZERO,
            [waiting("initrd.target", &[]), waiting("sysroot.mount", &[])],
        );
        state.finish_job(2, JobResult::Done, 1000 * MS);
        state.finish_job(1, JobResult::Done, 1500 * MS);
        assert!(!state.is_complete());
        state.switch_root(2000 * MS);
        assert_eq!(state.phase, Phase::Host);
        assert_eq!(
            state.finished_stages,
            [FinishedStage {
                label: "Initrd",
                finished_units: 2,
                failed_units: 0,
                started_at: Duration::ZERO,
                reached_at: 2000 * MS,
            }]
        );
        assert_eq!(state.milestones()[0].reached_at, 2000 * MS);
        assert_eq!(state.tally().done, 2);
        assert!(waiting_rows(&state).is_empty());
    }

    #[test]
    fn stopped_units_join_the_timeline_from_when_they_began_stopping() {
        let mut state = BootState::new(Phase::Shutdown);
        state.origin = 1000 * MS;
        state.track_unit(facts("db.service", &[]), Activity::default());
        state.queue_job(1, "db.service", JobKind::TakesDown, 1000 * MS);
        state.update_activity(
            "db.service",
            Activity {
                state: Some(ActiveState::Deactivating),
                active_exit: Some(1200 * MS),
                ..Activity::default()
            },
        );
        assert_eq!(
            waiting_rows(&state)[0].status,
            RowStatus::Running {
                queued_at: 1000 * MS,
                since: 1200 * MS
            }
        );
        state.finish_job(1, JobResult::Done, 1700 * MS);
        assert_eq!(
            state.finished_runs,
            [FinishedRun {
                label: "db".to_owned(),
                started_at: 1200 * MS,
                finished_at: 1700 * MS,
                failed: false,
            }]
        );
    }
}
