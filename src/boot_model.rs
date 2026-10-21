use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use petgraph::{
    Direction::Incoming,
    algo::{DfsSpace, has_path_connecting},
    graphmap::DiGraphMap,
    visit::Visitable,
};

type WaitingGraph<'a> = DiGraphMap<&'a str, ()>;
type WaitingSearch<'a> = DfsSpace<&'a str, <WaitingGraph<'a> as Visitable>::Map>;

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
            Self::Initrd => "Initrd",
            Self::Host => "Stage 2",
            Self::User => "User",
            Self::Shutdown => "Shutdown",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Progress {
    Untouched,
    Pending(PendingJob),
    Finished(Outcome),
}

#[derive(Clone, Debug)]
struct UnitRecord {
    facts: UnitFacts,
    state: ActiveState,
    inactive_exit: Option<Duration>,
    active_enter: Option<Duration>,
    active_exit: Option<Duration>,
    progress: Progress,
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
        let Progress::Pending(job) = self.progress else {
            return None;
        };
        let transitioning = matches!(
            self.state,
            ActiveState::Activating | ActiveState::Deactivating | ActiveState::Reloading
        );
        transitioning.then(|| self.transition_started(job.kind).unwrap_or(job.queued_at))
    }

    fn row_status(&self) -> RowStatus {
        match self.progress {
            Progress::Pending(job) => {
                self.running_since()
                    .map_or(RowStatus::Waiting, |since| RowStatus::Running {
                        queued_at: job.queued_at,
                        since,
                    })
            }
            Progress::Finished(Outcome::Failed) => RowStatus::Failed,
            Progress::Finished(_) | Progress::Untouched => RowStatus::Done,
        }
    }

    fn is_pending(&self) -> bool {
        matches!(self.progress, Progress::Pending(_))
    }

    fn is_finished(&self) -> bool {
        matches!(self.progress, Progress::Finished(_))
    }

    fn takes_down(&self) -> bool {
        matches!(
            self.progress,
            Progress::Pending(PendingJob {
                kind: JobKind::TakesDown,
                ..
            })
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Service,
    Target,
    Socket,
    Mount,
    Automount,
    Device,
    Swap,
    Path,
    Timer,
    Slice,
    Scope,
    Other,
}

impl UnitKind {
    #[must_use]
    pub fn of(name: &str) -> Self {
        match name.rsplit_once('.').map(|(_, suffix)| suffix) {
            Some("service") => Self::Service,
            Some("target") => Self::Target,
            Some("socket") => Self::Socket,
            Some("mount") => Self::Mount,
            Some("automount") => Self::Automount,
            Some("device") => Self::Device,
            Some("swap") => Self::Swap,
            Some("path") => Self::Path,
            Some("timer") => Self::Timer,
            Some("slice") => Self::Slice,
            Some("scope") => Self::Scope,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Service | Self::Target | Self::Other => "",
            Self::Socket => "Socket",
            Self::Mount => "Mount",
            Self::Automount => "Automount",
            Self::Device => "Device",
            Self::Swap => "Swap",
            Self::Path => "Path",
            Self::Timer => "Timer",
            Self::Slice => "Slice",
            Self::Scope => "Scope",
        }
    }

    fn labelled_by_description(self) -> bool {
        matches!(
            self,
            Self::Device | Self::Mount | Self::Swap | Self::Automount
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Waiting,
    Running {
        queued_at: Duration,
        since: Duration,
    },
    Failed,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub description: String,
    pub kind: UnitKind,
    pub status: RowStatus,
    pub chain: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MilestoneRow {
    pub label: String,
    pub finished_units: usize,
    pub started_at: Duration,
    pub reached_at: Duration,
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
    pub failed_units: usize,
    pub started_at: Duration,
    pub reached_at: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Root {
    Unresolved,
    Awaiting(String),
    Queued(String),
}

impl Root {
    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        match self {
            Self::Unresolved => None,
            Self::Awaiting(unit) | Self::Queued(unit) => Some(unit),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BootState {
    pub phase: Phase,
    pub origin: Duration,
    pub root: Root,
    pub failed: Vec<String>,
    pub finished_stages: Vec<FinishedStage>,
    pub finished_runs: Vec<FinishedRun>,
    pub handoff_unit: Option<String>,
    pub manager_exited_at: Option<Duration>,
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
            root: Root::Unresolved,
            failed: Vec::new(),
            finished_stages: Vec::new(),
            finished_runs: Vec::new(),
            handoff_unit,
            manager_exited_at: None,
            milestone_targets,
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
                progress: Progress::Untouched,
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
        if self
            .handoff_unit
            .as_deref()
            .and_then(|unit| self.units.get(unit))
            .is_some_and(|handoff| handoff.state != ActiveState::Inactive)
        {
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

    fn milestone_reached_at(&self, target: &str) -> Option<Duration> {
        let record = self.units.get(target)?;
        match record.progress {
            Progress::Finished(Outcome::Succeeded { at }) => Some(at),
            Progress::Untouched => record
                .active_enter
                .filter(|entered| record.state == ActiveState::Active && *entered >= self.origin),
            Progress::Pending(_) | Progress::Finished(Outcome::Failed | Outcome::Abandoned) => None,
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
        let start = MilestoneWalk {
            rows: self
                .finished_stages
                .iter()
                .map(|stage| MilestoneRow {
                    label: stage.label.to_owned(),
                    finished_units: stage.finished_units,
                    started_at: stage.started_at,
                    reached_at: stage.reached_at,
                })
                .collect(),
            previous: self
                .finished_stages
                .last()
                .map_or(self.origin, |stage| stage.reached_at),
            consumed: 0,
        };
        self.milestone_targets
            .iter()
            .filter_map(|target| {
                self.milestone_reached_at(target)
                    .map(|reached_at| ReachedMilestone { target, reached_at })
            })
            .fold(start, |walk, reached| walk.advance(&reached, &succeeded))
            .rows
    }

    #[must_use]
    pub fn waiting_forest(&self, finished_since: Duration) -> Vec<TreeNode> {
        let mut graph = WaitingGraph::new();
        self.units
            .iter()
            .filter(|(name, record)| match record.progress {
                Progress::Pending(_) | Progress::Finished(Outcome::Failed) => true,
                Progress::Finished(Outcome::Succeeded { at }) => {
                    at >= finished_since || self.is_milestone(name)
                }
                Progress::Finished(Outcome::Abandoned) => self.is_milestone(name),
                Progress::Untouched => false,
            })
            .for_each(|(name, _)| {
                graph.add_node(name.as_str());
            });
        let edges: Vec<(&str, &str)> = graph
            .nodes()
            .flat_map(|waiter| {
                self.waits_for(waiter, &graph)
                    .into_iter()
                    .map(move |dependency| (waiter, dependency))
            })
            .collect();
        graph.extend(edges);
        let root = self.root.unit().filter(|root| graph.contains_node(root));
        let stages: Vec<&str> = self
            .milestone_targets
            .iter()
            .map(String::as_str)
            .filter(|target| graph.contains_node(target))
            .chain(root.filter(|root| !self.is_milestone(root)))
            .collect();
        let mut unblocked: Vec<&str> = graph
            .nodes()
            .filter(|name| {
                graph.neighbors_directed(name, Incoming).next().is_none() && !stages.contains(name)
            })
            .collect();
        self.sort_siblings(&mut unblocked);
        let mut leftovers: Vec<&str> = graph.nodes().collect();
        self.sort_siblings(&mut leftovers);
        if self.phase == Phase::User {
            unblocked.clear();
            leftovers.clear();
        }
        let mut space = DfsSpace::new(&graph);
        let mut visited = HashSet::new();
        let mut forest = Vec::new();
        stages
            .iter()
            .copied()
            .chain(unblocked)
            .chain(leftovers)
            .for_each(|top| {
                if visited.contains(top) {
                    return;
                }
                let stage = self.grow(top, &graph, &mut space, &mut visited);
                if stage.row.status != RowStatus::Done || !stage.children.is_empty() {
                    forest.push(stage);
                }
            });
        forest
    }

    fn grow<'a>(
        &'a self,
        unit: &'a str,
        graph: &WaitingGraph<'a>,
        space: &mut WaitingSearch<'a>,
        visited: &mut HashSet<&'a str>,
    ) -> TreeNode {
        visited.insert(unit);
        let record = &self.units[unit];
        let row = TreeRow {
            depth: 0,
            label: display_label(&record.facts),
            description: summary(&record.facts),
            kind: UnitKind::of(&record.facts.name),
            status: record.row_status(),
            chain: Vec::new(),
        };
        let candidates: Vec<&str> = graph
            .neighbors(unit)
            .filter(|dependency| !visited.contains(dependency) && !self.is_milestone(dependency))
            .collect();
        let mut waits_on =
            |waiter, dependency| has_path_connecting(graph, waiter, dependency, Some(&mut *space));
        let mut children: Vec<&str> = candidates
            .iter()
            .copied()
            .filter(|child| {
                !candidates.iter().any(|sibling| {
                    sibling != child && waits_on(*sibling, *child) && !waits_on(*child, *sibling)
                })
            })
            .collect();
        self.sort_siblings(&mut children);
        visited.extend(children.iter().copied());
        TreeNode {
            row,
            children: children
                .into_iter()
                .map(|child| {
                    visited.remove(child);
                    self.grow(child, graph, space, visited)
                })
                .collect(),
        }
    }

    fn is_milestone(&self, unit: &str) -> bool {
        self.milestone_targets.iter().any(|target| target == unit)
    }

    fn waits_for<'a>(&'a self, waiter: &str, graph: &WaitingGraph) -> Vec<&'a str> {
        let Some(record) = self.units.get(waiter) else {
            return Vec::new();
        };
        let pending_job = |dependency: &&'a str| graph.contains_node(dependency);
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
        let rank = |name: &str| {
            let record = &self.units[name];
            match record.progress {
                Progress::Pending(_) => record
                    .running_since()
                    .map_or(SiblingRank::Waiting, SiblingRank::RunningSince),
                Progress::Finished(Outcome::Succeeded { at }) => {
                    SiblingRank::RunningSince(record.inactive_exit.unwrap_or(at))
                }
                Progress::Finished(Outcome::Failed) => SiblingRank::Failed,
                Progress::Finished(Outcome::Abandoned) | Progress::Untouched => {
                    SiblingRank::Waiting
                }
            }
        };
        names.sort_by(|left, right| rank(left).cmp(&rank(right)).then_with(|| left.cmp(right)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SiblingRank {
    RunningSince(Duration),
    Failed,
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
            reached_at: reached.reached_at,
        });
        self.consumed = self.consumed.max(upto);
        self.previous = reached.reached_at;
        self
    }
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub row: TreeRow,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn flatten_into(self, depth: usize, rows: &mut Vec<TreeRow>) {
        rows.push(TreeRow { depth, ..self.row });
        self.children
            .into_iter()
            .for_each(|child| child.flatten_into(depth + 1, rows));
    }
}

#[must_use]
pub fn flatten_forest(forest: Vec<TreeNode>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    forest
        .into_iter()
        .for_each(|root| root.flatten_into(0, &mut rows));
    rows
}

fn labelled_by_description(facts: &UnitFacts) -> bool {
    !facts.description.is_empty() && UnitKind::of(&facts.name).labelled_by_description()
}

fn display_label(facts: &UnitFacts) -> String {
    if labelled_by_description(facts) {
        return facts.description.clone();
    }
    match UnitKind::of(&facts.name) {
        UnitKind::Target | UnitKind::Other => facts.name.clone(),
        _ => facts
            .name
            .rsplit_once('.')
            .map_or_else(|| facts.name.clone(), |(stem, _)| stem.to_owned()),
    }
}

fn summary(facts: &UnitFacts) -> String {
    let repeats_the_name = facts.description == facts.name
        || facts
            .description
            .eq_ignore_ascii_case(&display_label(facts));
    if labelled_by_description(facts) || repeats_the_name {
        return String::new();
    }
    facts.description.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot_test_fixtures::{
        MS, activating, facts, nested_boot_sample as sample, queue_unit, queued_boot, tree_outline,
        waiting,
    };

    fn rows(state: &BootState) -> Vec<TreeRow> {
        flatten_forest(state.waiting_forest(Duration::MAX))
    }

    fn outline(state: &BootState) -> Vec<String> {
        tree_outline(&rows(state))
    }

    #[test]
    fn pending_units_nest_under_what_waits_on_them() {
        assert_eq!(
            outline(&sample()),
            [
                "basic.target",
                "multi-user.target",
                "  docker",
                "    containerd",
                "  nm-wait",
                "    nm",
                "graphical.target",
            ]
        );
    }

    #[test]
    fn a_unit_sits_in_the_earliest_stage_that_waits_on_it() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [
                waiting(
                    "multi-user.target",
                    &["basic.target", "dbus.service", "web.service"],
                ),
                waiting("basic.target", &["dbus.service"]),
                waiting("dbus.service", &[]),
                waiting("web.service", &[]),
            ],
        );
        assert_eq!(
            outline(&state),
            ["basic.target", "  dbus", "multi-user.target", "  web"]
        );
    }

    #[test]
    fn kinds_with_a_tag_drop_their_suffix() {
        let socket = facts("docker.socket", &[]);
        assert_eq!(UnitKind::of(&socket.name), UnitKind::Socket);
        assert_eq!(UnitKind::Socket.tag(), "Socket");
        assert_eq!(display_label(&socket), "docker");
        assert_eq!(display_label(&facts("sshd.service", &[])), "sshd");
        assert_eq!(display_label(&facts("basic.target", &[])), "basic.target");
        assert_eq!(UnitKind::Target.tag(), "");
    }

    #[test]
    fn descriptions_that_repeat_the_label_are_dropped() {
        let described = |name: &str, description: &str| {
            summary(&waiting(name, &[]).described(description).facts)
        };
        assert_eq!(
            described("multi-user.target", "Multi-User System"),
            "Multi-User System"
        );
        assert_eq!(described("greetd.service", "greetd.service"), "");
        assert_eq!(described("greetd.service", "Greetd"), "");
        assert_eq!(described("dev-sda.device", "Samsung SSD"), "");
        assert_eq!(described("demo.service", ""), "");
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
        assert!(
            rows(&state)
                .iter()
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
    fn display_manager_activation_hands_the_console_over() {
        let mut state = sample();
        queue_unit(&mut state, 20, waiting(HANDOFF_UNIT, &[]), 500 * MS);
        assert!(!state.is_complete());
        state.update_activity(HANDOFF_UNIT, activating(6000 * MS));
        assert!(state.is_complete());
    }

    #[test]
    fn display_manager_started_in_one_step_hands_the_console_over() {
        let mut state = sample();
        queue_unit(&mut state, 20, waiting(HANDOFF_UNIT, &[]), 500 * MS);
        state.finish_job(20, JobResult::Done, 6000 * MS);
        assert!(!state.is_complete());
        state.update_activity(
            HANDOFF_UNIT,
            Activity {
                state: Some(ActiveState::Active),
                inactive_exit: Some(6000 * MS),
                active_enter: Some(6000 * MS),
                active_exit: None,
            },
        );
        assert!(state.is_complete());
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
        assert!(rows(&state).is_empty());
    }

    #[test]
    fn devices_show_their_description() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [waiting("dev-nvme1n1.device", &[]).described("Lexar SSD NM790 2TB")],
        );
        assert_eq!(rows(&state)[0].label, "Lexar SSD NM790 2TB");
    }

    #[test]
    fn units_nest_under_the_deepest_pending_waiter() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [
                waiting(
                    "demo.target",
                    &["report.service", "web.service", "db.service"],
                ),
                waiting("report.service", &["web.service"]),
                waiting("web.service", &["db.service"]),
                waiting("db.service", &[]),
            ],
        );
        assert_eq!(
            outline(&state),
            ["demo.target", "  report", "    web", "      db"]
        );
    }

    #[test]
    fn stopping_units_wait_on_what_was_ordered_after_them() {
        let state = queued_boot(
            Phase::Shutdown,
            Duration::ZERO,
            [
                waiting("poweroff.target", &["shutdown.target"]),
                waiting("shutdown.target", &[]).ordered_before(&["poweroff.target"]),
                waiting("network.target", &[])
                    .ordered_before(&["web.service"])
                    .stopping(),
                waiting("web.service", &["network.target"]).stopping(),
            ],
        );
        assert_eq!(
            outline(&state),
            [
                "shutdown.target",
                "poweroff.target",
                "network.target",
                "  web",
            ]
        );
    }

    #[test]
    fn starting_units_wait_on_stops_in_either_direction() {
        let state = queued_boot(
            Phase::Shutdown,
            Duration::ZERO,
            [
                waiting("shutdown.target", &[]).ordered_before(&["db.service"]),
                waiting("db.service", &["shutdown.target"]).stopping(),
            ],
        );
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
        assert_eq!(
            rows(&state)[0].status,
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

    #[test]
    fn ordering_cycles_do_not_repeat_units() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [
                waiting("a.service", &["b.service"]),
                waiting("b.service", &["a.service"]),
            ],
        );
        assert_eq!(rows(&state).len(), 2);
    }
}
