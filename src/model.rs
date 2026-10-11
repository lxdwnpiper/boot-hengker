use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Initrd,
    Host,
    User,
}

impl Phase {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Initrd => "initrd",
            Self::Host => "stage 2",
            Self::User => "user",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveState {
    Inactive,
    Activating,
    Active,
    Deactivating,
    Reloading,
    Failed,
}

impl ActiveState {
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "activating" => Self::Activating,
            "active" => Self::Active,
            "deactivating" => Self::Deactivating,
            "reloading" => Self::Reloading,
            "failed" => Self::Failed,
            _ => Self::Inactive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobResult {
    Done,
    Skipped,
    Canceled,
    Failed,
    Timeout,
    Dependency,
}

impl JobResult {
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "done" => Self::Done,
            "skipped" => Self::Skipped,
            "canceled" => Self::Canceled,
            "timeout" => Self::Timeout,
            "dependency" => Self::Dependency,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitFacts {
    pub name: String,
    pub description: String,
    pub after: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub state: Option<ActiveState>,
    pub inactive_exit: Option<Duration>,
    pub active_enter: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingJob {
    id: u32,
    queued_at: Duration,
    brings_up: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Succeeded { at: Duration },
    Failed,
    Abandoned,
}

#[derive(Clone, Debug)]
struct UnitRecord {
    facts: UnitFacts,
    state: ActiveState,
    inactive_exit: Option<Duration>,
    active_enter: Option<Duration>,
    job: Option<PendingJob>,
    outcome: Option<Outcome>,
}

impl UnitRecord {
    fn apply(&mut self, activity: Activity) {
        if let Some(state) = activity.state {
            self.state = state;
        }
        if activity.inactive_exit.is_some() {
            self.inactive_exit = activity.inactive_exit;
        }
        if activity.active_enter.is_some() {
            self.active_enter = activity.active_enter;
        }
    }

    fn running_since(&self) -> Option<Duration> {
        let job = self.job?;
        let transitioning = matches!(
            self.state,
            ActiveState::Activating | ActiveState::Deactivating | ActiveState::Reloading
        );
        transitioning.then(|| self.inactive_exit.unwrap_or(job.queued_at))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Waiting,
    Running,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRow {
    pub depth: usize,
    pub is_last_sibling: Vec<bool>,
    pub label: String,
    pub is_target: bool,
    pub status: RowStatus,
    pub queued_at: Duration,
    pub running_since: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MilestoneRow {
    pub label: String,
    pub finished_units: usize,
    pub started_at: Duration,
    pub reached_at: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub done: usize,
    pub running: usize,
    pub waiting: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedRun {
    pub label: String,
    pub started_at: Duration,
    pub finished_at: Duration,
    pub failed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedStage {
    pub label: &'static str,
    pub finished_units: usize,
    pub started_at: Duration,
    pub reached_at: Duration,
}

#[derive(Clone, Debug)]
pub struct BootState {
    pub phase: Phase,
    pub origin: Duration,
    pub root: Option<String>,
    pub failed: Vec<String>,
    pub finished_stages: Vec<FinishedStage>,
    pub finished_runs: Vec<FinishedRun>,
    pub handoff_unit: Option<String>,
    root_job_seen: bool,
    milestone_targets: Vec<String>,
    finished_before_reconnect: usize,
    units: HashMap<String, UnitRecord>,
    jobs: HashMap<u32, String>,
}

pub const HOST_MILESTONES: [&str; 4] = [
    "sysinit.target",
    "basic.target",
    "multi-user.target",
    "graphical.target",
];

pub const HANDOFF_UNIT: &str = "display-manager.service";

impl BootState {
    #[must_use]
    pub fn new(phase: Phase) -> Self {
        let (milestone_targets, handoff_unit) = match phase {
            Phase::Initrd | Phase::Host => (
                HOST_MILESTONES.iter().map(ToString::to_string).collect(),
                Some(HANDOFF_UNIT.to_owned()),
            ),
            Phase::User => (Vec::new(), None),
        };
        Self {
            phase,
            origin: Duration::ZERO,
            root: None,
            failed: Vec::new(),
            finished_stages: Vec::new(),
            finished_runs: Vec::new(),
            root_job_seen: false,
            handoff_unit,
            milestone_targets,
            finished_before_reconnect: 0,
            units: HashMap::new(),
            jobs: HashMap::new(),
        }
    }

    pub fn set_root(&mut self, root: String) {
        if self.root.as_ref() != Some(&root) {
            self.root_job_seen = false;
        }
        if self.phase == Phase::User && !self.milestone_targets.contains(&root) {
            self.milestone_targets.push(root.clone());
        }
        self.root = Some(root);
    }

    pub fn set_handoff(&mut self, unit: String) {
        if self.handoff_unit.is_some() {
            self.handoff_unit = Some(unit);
        }
    }

    pub fn track_unit(&mut self, facts: UnitFacts, activity: Activity) {
        let record = self
            .units
            .entry(facts.name.clone())
            .or_insert_with(|| UnitRecord {
                facts: facts.clone(),
                state: ActiveState::Inactive,
                inactive_exit: None,
                active_enter: None,
                job: None,
                outcome: None,
            });
        record.facts = facts;
        record.apply(activity);
    }

    pub fn update_activity(&mut self, unit: &str, activity: Activity) {
        if let Some(record) = self.units.get_mut(unit) {
            record.apply(activity);
        }
    }

    pub fn queue_job(&mut self, id: u32, unit: &str, at: Duration) {
        self.enqueue(id, unit, at, true);
        if self.jobs.get(&id).is_some_and(|tracked| tracked == unit)
            && self.root.as_deref() == Some(unit)
        {
            self.root_job_seen = true;
        }
    }

    pub fn queue_job_of_unknown_kind(&mut self, id: u32, unit: &str, at: Duration) {
        self.enqueue(id, unit, at, false);
    }

    fn enqueue(&mut self, id: u32, unit: &str, at: Duration, brings_up: bool) {
        if self.jobs.contains_key(&id) {
            return;
        }
        let Some(record) = self.units.get_mut(unit) else {
            return;
        };
        record.job = Some(PendingJob {
            id,
            queued_at: at,
            brings_up,
        });
        record.outcome = None;
        self.jobs.insert(id, unit.to_owned());
    }

    pub fn finish_job(&mut self, id: u32, result: JobResult, at: Duration) {
        let Some(unit) = self.jobs.remove(&id) else {
            return;
        };
        let Some(record) = self.units.get_mut(&unit) else {
            return;
        };
        let started_at = record
            .inactive_exit
            .filter(|started| *started >= self.origin && *started <= at)
            .filter(|_| record.job.is_some_and(|job| job.id == id && job.brings_up));
        if record.job.is_some_and(|job| job.id == id) {
            record.job = None;
        }
        if let Some(started_at) = started_at
            && !record.facts.name.ends_with(".target")
        {
            self.finished_runs.push(FinishedRun {
                label: display_label(&record.facts),
                started_at,
                finished_at: at,
                failed: matches!(
                    result,
                    JobResult::Failed | JobResult::Timeout | JobResult::Dependency
                ),
            });
        }
        let outcome = match result {
            JobResult::Done => Outcome::Succeeded { at },
            JobResult::Skipped | JobResult::Canceled => Outcome::Abandoned,
            JobResult::Failed | JobResult::Timeout | JobResult::Dependency => Outcome::Failed,
        };
        record.outcome = Some(outcome);
        if outcome == Outcome::Failed && !self.failed.contains(&unit) {
            self.failed.push(unit);
        }
    }

    pub fn switch_root(&mut self, at: Duration) {
        let finished_here = self
            .units
            .values()
            .filter(|record| record.outcome.is_some())
            .count();
        self.finished_stages.push(FinishedStage {
            label: Phase::Initrd.label(),
            finished_units: finished_here + self.finished_before_reconnect,
            started_at: self.origin,
            reached_at: at,
        });
        self.phase = Phase::Host;
        self.root = None;
        self.root_job_seen = false;
        self.finished_before_reconnect = 0;
        self.units.clear();
        self.jobs.clear();
    }

    pub fn forget_jobs(&mut self) {
        self.finished_before_reconnect += self
            .units
            .values()
            .filter(|record| record.outcome.is_some())
            .count();
        self.jobs.clear();
        self.units.values_mut().for_each(|record| {
            record.job = None;
            record.outcome = None;
        });
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        if self.phase == Phase::Initrd {
            return false;
        }
        if let Some(handoff) = self
            .handoff_unit
            .as_deref()
            .and_then(|unit| self.units.get(unit))
            && handoff.job.is_some()
            && handoff.state != ActiveState::Inactive
        {
            return true;
        }
        let Some(root) = self.root.as_deref().and_then(|root| self.units.get(root)) else {
            return false;
        };
        self.root_job_seen && root.job.is_none()
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
            match (record.job, record.running_since(), record.outcome) {
                (Some(_), Some(_), _) => tally.running += 1,
                (Some(_), None, _) => tally.waiting += 1,
                (None, _, Some(Outcome::Succeeded { .. } | Outcome::Abandoned)) => tally.done += 1,
                _ => {}
            }
            tally
        })
    }

    fn milestone_reached_at(&self, target: &str) -> Option<Duration> {
        let record = self.units.get(target)?;
        if record.job.is_some() {
            return None;
        }
        match record.outcome {
            Some(Outcome::Succeeded { at }) => Some(at),
            Some(_) => None,
            None => record
                .active_enter
                .filter(|entered| record.state == ActiveState::Active && *entered >= self.origin),
        }
    }

    #[must_use]
    pub fn milestones(&self) -> Vec<MilestoneRow> {
        let mut succeeded: Vec<Duration> = self
            .units
            .values()
            .filter_map(|record| match record.outcome {
                Some(Outcome::Succeeded { at }) => Some(at),
                _ => None,
            })
            .collect();
        succeeded.sort_unstable();
        let start = MilestoneWalk {
            rows: self
                .finished_stages
                .iter()
                .map(|stage| MilestoneRow {
                    label: stage.label.to_owned(),
                    finished_units: stage.finished_units,
                    started_at: stage.started_at,
                    reached_at: Some(stage.reached_at),
                })
                .collect(),
            previous: self
                .finished_stages
                .last()
                .map_or(self.origin, |stage| stage.reached_at),
            consumed: 0,
        };
        let mut walk = self
            .milestone_targets
            .iter()
            .filter_map(|target| {
                self.milestone_reached_at(target)
                    .map(|reached_at| ReachedMilestone { target, reached_at })
            })
            .fold(start, |walk, reached| walk.advance(&reached, &succeeded));
        let remaining = succeeded.len() - walk.consumed + self.finished_before_reconnect;
        if remaining > 0 {
            walk.rows.push(MilestoneRow {
                label: "finished".to_owned(),
                finished_units: remaining,
                started_at: walk.previous,
                reached_at: None,
            });
        }
        walk.rows
    }

    #[must_use]
    pub fn pending_rows(&self) -> Vec<PendingRow> {
        let pending: HashSet<&str> = self
            .units
            .iter()
            .filter(|(_, record)| record.job.is_some())
            .map(|(name, _)| name.as_str())
            .collect();
        let root = self.root.as_deref().filter(|root| pending.contains(root));
        let blocked_by_pending: HashSet<&str> = pending
            .iter()
            .flat_map(|name| self.units[*name].facts.after.iter())
            .map(String::as_str)
            .filter(|dependency| pending.contains(dependency))
            .collect();
        let mut unblocked: Vec<&str> = pending
            .iter()
            .copied()
            .filter(|name| !blocked_by_pending.contains(name) && Some(*name) != root)
            .collect();
        self.sort_siblings(&mut unblocked);
        let mut leftovers: Vec<&str> = pending.iter().copied().collect();
        self.sort_siblings(&mut leftovers);
        if self.phase == Phase::User {
            unblocked.clear();
            leftovers.clear();
        }
        let mut rows = Vec::new();
        let mut visited = HashSet::new();
        root.into_iter()
            .chain(unblocked)
            .chain(leftovers)
            .for_each(|top| {
                if !visited.contains(top) {
                    self.emit(
                        top,
                        &pending,
                        &mut visited,
                        &mut Vec::new(),
                        true,
                        &mut rows,
                    );
                }
            });
        rows
    }

    fn emit<'a>(
        &'a self,
        unit: &'a str,
        pending: &HashSet<&'a str>,
        visited: &mut HashSet<&'a str>,
        ancestry: &mut Vec<bool>,
        is_last: bool,
        rows: &mut Vec<PendingRow>,
    ) {
        visited.insert(unit);
        let record = &self.units[unit];
        let job = record.job.expect("pending rows only hold units with a job");
        let running_since = record.running_since();
        let mut is_last_sibling = ancestry.clone();
        is_last_sibling.push(is_last);
        rows.push(PendingRow {
            depth: ancestry.len(),
            is_last_sibling,
            label: display_label(&record.facts),
            is_target: record.facts.name.ends_with(".target"),
            status: if running_since.is_some() {
                RowStatus::Running
            } else {
                RowStatus::Waiting
            },
            queued_at: job.queued_at,
            running_since,
        });
        let candidates: Vec<&str> = record
            .facts
            .after
            .iter()
            .map(String::as_str)
            .filter(|dependency| pending.contains(dependency) && !visited.contains(dependency))
            .collect();
        let mut children: Vec<&str> = candidates
            .iter()
            .copied()
            .filter(|child| {
                !candidates.iter().any(|sibling| {
                    sibling != child
                        && self.waits_on(sibling, child, pending)
                        && !self.waits_on(child, sibling, pending)
                })
            })
            .collect();
        self.sort_siblings(&mut children);
        children.dedup();
        visited.extend(children.iter().copied());
        ancestry.push(is_last);
        let count = children.len();
        children.into_iter().enumerate().for_each(|(index, child)| {
            visited.remove(child);
            self.emit(child, pending, visited, ancestry, index + 1 == count, rows);
        });
        ancestry.pop();
    }

    fn waits_on(&self, waiter: &str, dependency: &str, pending: &HashSet<&str>) -> bool {
        let mut seen = HashSet::new();
        let mut frontier = vec![waiter];
        while let Some(unit) = frontier.pop() {
            if !seen.insert(unit) {
                continue;
            }
            let Some(record) = self.units.get(unit) else {
                continue;
            };
            let mut next = record
                .facts
                .after
                .iter()
                .map(String::as_str)
                .filter(|after| pending.contains(after));
            if next.clone().any(|after| after == dependency) {
                return true;
            }
            frontier.extend(next.by_ref());
        }
        false
    }

    fn sort_siblings(&self, names: &mut [&str]) {
        let rank = |name: &str| match self.units[name].running_since() {
            Some(since) => SiblingRank::RunningSince(since),
            None => SiblingRank::Waiting,
        };
        names.sort_by(|left, right| rank(left).cmp(&rank(right)).then_with(|| left.cmp(right)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SiblingRank {
    RunningSince(Duration),
    Waiting,
}

struct ReachedMilestone<'a> {
    target: &'a str,
    reached_at: Duration,
}

struct MilestoneWalk {
    rows: Vec<MilestoneRow>,
    previous: Duration,
    consumed: usize,
}

impl MilestoneWalk {
    fn advance(mut self, reached: &ReachedMilestone, succeeded_sorted: &[Duration]) -> Self {
        let upto = succeeded_sorted.partition_point(|at| *at <= reached.reached_at);
        self.rows.push(MilestoneRow {
            label: reached.target.to_owned(),
            finished_units: upto.saturating_sub(self.consumed),
            started_at: self.previous.min(reached.reached_at),
            reached_at: Some(reached.reached_at),
        });
        self.consumed = self.consumed.max(upto);
        self.previous = reached.reached_at;
        self
    }
}

fn display_label(facts: &UnitFacts) -> String {
    if let Some(stem) = facts.name.strip_suffix(".service") {
        return stem.to_owned();
    }
    let described_by_path = [".device", ".mount", ".swap", ".automount"]
        .iter()
        .any(|suffix| facts.name.ends_with(suffix));
    if described_by_path && !facts.description.is_empty() {
        return facts.description.clone();
    }
    facts.name.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn facts(name: &str, after: &[&str]) -> UnitFacts {
        UnitFacts {
            name: name.to_owned(),
            description: format!("{name} description"),
            after: after.iter().map(ToString::to_string).collect(),
        }
    }

    fn activating(since: Duration) -> Activity {
        Activity {
            state: Some(ActiveState::Activating),
            inactive_exit: Some(since),
            active_enter: None,
        }
    }

    fn sample() -> BootState {
        let mut state = BootState::new(Phase::Host);
        state.set_root("graphical.target".to_owned());
        [
            (
                facts("graphical.target", &["multi-user.target"]),
                Activity::default(),
            ),
            (
                facts(
                    "multi-user.target",
                    &["docker.service", "nm-wait.service", "basic.target"],
                ),
                Activity::default(),
            ),
            (
                facts("nm-wait.service", &["nm.service"]),
                Activity::default(),
            ),
            (facts("nm.service", &[]), activating(1000 * MS)),
            (
                facts("docker.service", &["containerd.service"]),
                activating(3000 * MS),
            ),
            (facts("containerd.service", &[]), Activity::default()),
            (facts("basic.target", &[]), Activity::default()),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((unit, activity), id)| {
            let name = unit.name.clone();
            state.track_unit(unit, activity);
            state.queue_job(id, &name, 500 * MS);
        });
        state
    }

    #[test]
    fn pending_units_nest_under_what_waits_on_them() {
        let outline: Vec<String> = sample()
            .pending_rows()
            .into_iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
            .collect();
        assert_eq!(
            outline,
            [
                "graphical.target",
                "  multi-user.target",
                "    docker",
                "      containerd",
                "    basic.target",
                "    nm-wait",
                "      nm",
            ]
        );
    }

    #[test]
    fn finished_jobs_leave_the_tree_and_count_toward_their_milestone() {
        let mut state = sample();
        state.finish_job(6, JobResult::Done, 3500 * MS);
        state.finish_job(7, JobResult::Done, 4000 * MS);
        state.update_activity(
            "basic.target",
            Activity {
                state: Some(ActiveState::Active),
                inactive_exit: None,
                active_enter: Some(4000 * MS),
            },
        );
        let rows = state.pending_rows();
        assert!(
            rows.iter()
                .all(|row| row.label != "containerd" && row.label != "basic.target")
        );
        let milestones = state.milestones();
        assert_eq!(milestones.len(), 1);
        assert_eq!(milestones[0].label, "basic.target");
        assert_eq!(milestones[0].finished_units, 2);
    }

    #[test]
    fn failures_are_listed_and_not_counted_as_done() {
        let mut state = sample();
        state.finish_job(4, JobResult::Failed, 2000 * MS);
        assert_eq!(state.failed, ["nm.service"]);
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
    fn display_manager_activation_hands_the_console_over() {
        let mut state = sample();
        state.track_unit(facts(HANDOFF_UNIT, &[]), Activity::default());
        state.queue_job(20, HANDOFF_UNIT, 500 * MS);
        assert!(!state.is_complete());
        state.update_activity(HANDOFF_UNIT, activating(6000 * MS));
        assert!(state.is_complete());
    }

    #[test]
    fn switching_root_freezes_the_initrd_summary() {
        let mut state = BootState::new(Phase::Initrd);
        state.set_root("initrd.target".to_owned());
        state.track_unit(facts("initrd.target", &[]), Activity::default());
        state.track_unit(facts("sysroot.mount", &[]), Activity::default());
        state.queue_job(1, "initrd.target", Duration::ZERO);
        state.queue_job(2, "sysroot.mount", Duration::ZERO);
        state.finish_job(2, JobResult::Done, 1000 * MS);
        state.finish_job(1, JobResult::Done, 1500 * MS);
        assert!(!state.is_complete());
        state.switch_root(2000 * MS);
        assert_eq!(state.phase, Phase::Host);
        assert_eq!(
            state.finished_stages,
            [FinishedStage {
                label: "initrd",
                finished_units: 2,
                started_at: Duration::ZERO,
                reached_at: 2000 * MS,
            }]
        );
        assert_eq!(state.milestones()[0].reached_at, Some(2000 * MS));
        assert_eq!(state.tally().done, 2);
        assert!(state.pending_rows().is_empty());
    }

    #[test]
    fn devices_show_their_description() {
        let mut state = BootState::new(Phase::Host);
        state.track_unit(
            UnitFacts {
                name: "dev-nvme1n1.device".to_owned(),
                description: "Lexar SSD NM790 2TB".to_owned(),
                after: Vec::new(),
            },
            Activity::default(),
        );
        state.queue_job(1, "dev-nvme1n1.device", Duration::ZERO);
        assert_eq!(state.pending_rows()[0].label, "Lexar SSD NM790 2TB");
    }

    #[test]
    fn units_nest_under_the_deepest_pending_waiter() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("demo.target".to_owned());
        [
            facts(
                "demo.target",
                &["report.service", "web.service", "db.service"],
            ),
            facts("report.service", &["web.service"]),
            facts("web.service", &["db.service"]),
            facts("db.service", &[]),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|(unit, id)| {
            let name = unit.name.clone();
            state.track_unit(unit, Activity::default());
            state.queue_job(id, &name, Duration::ZERO);
        });
        let outline: Vec<String> = state
            .pending_rows()
            .into_iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
            .collect();
        assert_eq!(outline, ["demo.target", "  report", "    web", "      db"]);
    }

    #[test]
    fn ordering_cycles_do_not_repeat_units() {
        let mut state = BootState::new(Phase::Host);
        state.track_unit(facts("a.service", &["b.service"]), Activity::default());
        state.track_unit(facts("b.service", &["a.service"]), Activity::default());
        state.queue_job(1, "a.service", Duration::ZERO);
        state.queue_job(2, "b.service", Duration::ZERO);
        assert_eq!(state.pending_rows().len(), 2);
    }
}
