use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Initrd,
    Host,
    User,
    Shutdown,
}

impl Phase {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Initrd => "initrd",
            Self::Host => "stage 2",
            Self::User => "user",
            Self::Shutdown => "shutdown",
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
    pub before: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub state: Option<ActiveState>,
    pub inactive_exit: Option<Duration>,
    pub active_enter: Option<Duration>,
    pub active_exit: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    BringsUp,
    TakesDown,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingJob {
    id: u32,
    queued_at: Duration,
    kind: JobKind,
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
    active_exit: Option<Duration>,
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
        if activity.active_exit.is_some() {
            self.active_exit = activity.active_exit;
        }
    }

    fn transition_started(&self, kind: JobKind) -> Option<Duration> {
        match kind {
            JobKind::TakesDown => self.active_exit,
            JobKind::BringsUp | JobKind::Unknown => self.inactive_exit,
        }
    }

    fn running_since(&self) -> Option<Duration> {
        let job = self.job?;
        let transitioning = matches!(
            self.state,
            ActiveState::Activating | ActiveState::Deactivating | ActiveState::Reloading
        );
        transitioning.then(|| self.transition_started(job.kind).unwrap_or(job.queued_at))
    }

    fn takes_down(&self) -> bool {
        self.job.is_some_and(|job| job.kind == JobKind::TakesDown)
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
    pub manager_exited_at: Option<Duration>,
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

pub const SHUTDOWN_MILESTONES: [&str; 3] = ["shutdown.target", "umount.target", "final.target"];

pub const HANDOFF_UNIT: &str = "display-manager.service";

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
            root: None,
            failed: Vec::new(),
            finished_stages: Vec::new(),
            finished_runs: Vec::new(),
            root_job_seen: false,
            handoff_unit,
            manager_exited_at: None,
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
                active_exit: None,
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

    pub fn queue_job(&mut self, id: u32, unit: &str, kind: JobKind, at: Duration) {
        self.enqueue(id, unit, kind, at);
        if kind == JobKind::BringsUp
            && self.jobs.get(&id).is_some_and(|tracked| tracked == unit)
            && self.root.as_deref() == Some(unit)
        {
            self.root_job_seen = true;
        }
    }

    fn enqueue(&mut self, id: u32, unit: &str, kind: JobKind, at: Duration) {
        if self.jobs.contains_key(&id) {
            return;
        }
        let Some(record) = self.units.get_mut(unit) else {
            return;
        };
        record.job = Some(PendingJob {
            id,
            queued_at: at,
            kind,
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
            .job
            .filter(|job| job.id == id && job.kind != JobKind::Unknown)
            .and_then(|job| record.transition_started(job.kind))
            .filter(|started| *started >= self.origin && *started <= at);
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
        if matches!(self.phase, Phase::Initrd | Phase::Shutdown) {
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
            .flat_map(|name| self.waits_for(name, &pending))
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
        let candidates: Vec<&str> = self
            .waits_for(unit, pending)
            .into_iter()
            .filter(|dependency| !visited.contains(dependency))
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
            let next = self.waits_for(unit, pending);
            if next.contains(&dependency) {
                return true;
            }
            frontier.extend(next);
        }
        false
    }

    fn waits_for<'a>(&'a self, waiter: &str, pending: &HashSet<&str>) -> Vec<&'a str> {
        let Some(record) = self.units.get(waiter) else {
            return Vec::new();
        };
        let pending_job = |dependency: &&'a str| pending.contains(dependency);
        let taking_down = |dependency: &&'a str| self.units[*dependency].takes_down();
        let after = record.facts.after.iter().map(String::as_str);
        let before = record.facts.before.iter().map(String::as_str);
        if record.takes_down() {
            before.filter(pending_job).filter(taking_down).collect()
        } else {
            after
                .filter(pending_job)
                .chain(before.filter(pending_job).filter(taking_down))
                .collect()
        }
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

#[must_use]
pub fn paths_to_running(rows: Vec<PendingRow>) -> Vec<PendingRow> {
    let mut kept_below: Vec<bool> = Vec::new();
    let mut kept: Vec<bool> = rows
        .iter()
        .rev()
        .map(|row| {
            let descendant_kept = kept_below.iter().skip(row.depth + 1).any(|kept| *kept);
            kept_below.resize(row.depth + 1, false);
            let anchors_the_tree = row.depth == 0 && row.is_target;
            let keep = row.status == RowStatus::Running || descendant_kept || anchors_the_tree;
            kept_below[row.depth] |= keep;
            keep
        })
        .collect();
    kept.reverse();
    let survivors: Vec<PendingRow> = rows
        .into_iter()
        .zip(kept)
        .filter_map(|(row, keep)| keep.then_some(row))
        .collect();
    relink_siblings(survivors)
}

fn relink_siblings(rows: Vec<PendingRow>) -> Vec<PendingRow> {
    let mut later_sibling: Vec<bool> = Vec::new();
    let mut is_last: Vec<bool> = rows
        .iter()
        .rev()
        .map(|row| {
            let last = !later_sibling.get(row.depth).copied().unwrap_or(false);
            later_sibling.resize(row.depth + 1, false);
            later_sibling[row.depth] = true;
            last
        })
        .collect();
    is_last.reverse();
    let mut ancestry: Vec<bool> = Vec::new();
    rows.into_iter()
        .zip(is_last)
        .map(|(row, last)| {
            ancestry.truncate(row.depth);
            ancestry.push(last);
            PendingRow {
                is_last_sibling: ancestry.clone(),
                ..row
            }
        })
        .collect()
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
            before: Vec::new(),
        }
    }

    fn activating(since: Duration) -> Activity {
        Activity {
            state: Some(ActiveState::Activating),
            inactive_exit: Some(since),
            active_enter: None,
            active_exit: None,
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
            state.queue_job(id, &name, JobKind::BringsUp, 500 * MS);
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
    fn only_chains_leading_to_running_units_stay_in_view() {
        let rows = paths_to_running(sample().pending_rows());
        let outline: Vec<String> = rows
            .iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
            .collect();
        assert_eq!(
            outline,
            [
                "graphical.target",
                "  multi-user.target",
                "    docker",
                "    nm-wait",
                "      nm",
            ]
        );
        let lasts: Vec<bool> = rows
            .iter()
            .map(|row| *row.is_last_sibling.last().unwrap())
            .collect();
        assert_eq!(lasts, [true, true, false, true, true]);
    }

    #[test]
    fn with_nothing_running_only_the_root_target_stays_in_view() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("demo.target".to_owned());
        state.track_unit(facts("demo.target", &["idle.service"]), Activity::default());
        state.track_unit(facts("idle.service", &[]), Activity::default());
        state.queue_job(1, "demo.target", JobKind::BringsUp, Duration::ZERO);
        state.queue_job(2, "idle.service", JobKind::BringsUp, Duration::ZERO);
        let rows = paths_to_running(state.pending_rows());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "demo.target");
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
                active_exit: None,
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
        state.queue_job(20, HANDOFF_UNIT, JobKind::BringsUp, 500 * MS);
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
        state.queue_job(1, "initrd.target", JobKind::BringsUp, Duration::ZERO);
        state.queue_job(2, "sysroot.mount", JobKind::BringsUp, Duration::ZERO);
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
                before: Vec::new(),
            },
            Activity::default(),
        );
        state.queue_job(1, "dev-nvme1n1.device", JobKind::BringsUp, Duration::ZERO);
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
            state.queue_job(id, &name, JobKind::BringsUp, Duration::ZERO);
        });
        let outline: Vec<String> = state
            .pending_rows()
            .into_iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
            .collect();
        assert_eq!(outline, ["demo.target", "  report", "    web", "      db"]);
    }

    fn ordered(name: &str, after: &[&str], before: &[&str]) -> UnitFacts {
        UnitFacts {
            before: before.iter().map(ToString::to_string).collect(),
            ..facts(name, after)
        }
    }

    fn outline(state: &BootState) -> Vec<String> {
        state
            .pending_rows()
            .into_iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.label))
            .collect()
    }

    #[test]
    fn stopping_units_wait_on_what_was_ordered_after_them() {
        let mut state = BootState::new(Phase::Shutdown);
        state.set_root("poweroff.target".to_owned());
        [
            (
                ordered("poweroff.target", &["shutdown.target"], &[]),
                JobKind::BringsUp,
            ),
            (
                ordered("shutdown.target", &[], &["poweroff.target"]),
                JobKind::BringsUp,
            ),
            (
                ordered("network.target", &[], &["web.service"]),
                JobKind::TakesDown,
            ),
            (
                ordered("web.service", &["network.target"], &[]),
                JobKind::TakesDown,
            ),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((unit, kind), id)| {
            let name = unit.name.clone();
            state.track_unit(unit, Activity::default());
            state.queue_job(id, &name, kind, Duration::ZERO);
        });
        assert_eq!(
            outline(&state),
            [
                "poweroff.target",
                "  shutdown.target",
                "network.target",
                "  web",
            ]
        );
    }

    #[test]
    fn starting_units_wait_on_stops_in_either_direction() {
        let mut state = BootState::new(Phase::Shutdown);
        state.set_root("shutdown.target".to_owned());
        [
            (
                ordered("shutdown.target", &[], &["db.service"]),
                JobKind::BringsUp,
            ),
            (
                ordered("db.service", &["shutdown.target"], &[]),
                JobKind::TakesDown,
            ),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((unit, kind), id)| {
            let name = unit.name.clone();
            state.track_unit(unit, Activity::default());
            state.queue_job(id, &name, kind, Duration::ZERO);
        });
        assert_eq!(outline(&state), ["shutdown.target", "  db"]);
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
        assert_eq!(state.pending_rows()[0].running_since, Some(1200 * MS));
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

    #[test]
    fn shutdown_counts_toward_its_own_milestones_and_never_completes() {
        let mut state = BootState::new(Phase::Shutdown);
        state.set_root("poweroff.target".to_owned());
        ["poweroff.target", "shutdown.target", "db.service"]
            .into_iter()
            .for_each(|name| state.track_unit(facts(name, &[]), Activity::default()));
        state.queue_job(1, "poweroff.target", JobKind::BringsUp, Duration::ZERO);
        state.queue_job(2, "shutdown.target", JobKind::BringsUp, Duration::ZERO);
        state.queue_job(3, "db.service", JobKind::TakesDown, Duration::ZERO);
        state.finish_job(3, JobResult::Done, 500 * MS);
        state.finish_job(2, JobResult::Done, 600 * MS);
        state.finish_job(1, JobResult::Done, 700 * MS);
        let milestones = state.milestones();
        assert_eq!(milestones[0].label, "shutdown.target");
        assert_eq!(milestones[0].finished_units, 2);
        assert!(!state.is_complete());
    }

    #[test]
    fn ordering_cycles_do_not_repeat_units() {
        let mut state = BootState::new(Phase::Host);
        state.track_unit(facts("a.service", &["b.service"]), Activity::default());
        state.track_unit(facts("b.service", &["a.service"]), Activity::default());
        state.queue_job(1, "a.service", JobKind::BringsUp, Duration::ZERO);
        state.queue_job(2, "b.service", JobKind::BringsUp, Duration::ZERO);
        assert_eq!(state.pending_rows().len(), 2);
    }
}
