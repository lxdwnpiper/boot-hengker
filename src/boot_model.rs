pub mod boot_origin;
mod job_ingestion;
mod reached_milestones;
mod unit_facts;
mod waiting_tree;

use std::{collections::HashMap, time::Duration};

pub use self::{
    reached_milestones::MilestoneRow,
    unit_facts::{ActiveState, Activity, JobKind, JobResult, UnitFacts, UnitKind},
    waiting_tree::{RowStatus, TreeNode, TreeRow, flatten_forest},
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
            Self::Initrd => "Initrd",
            Self::Host => "Stage 2",
            Self::User => "User",
            Self::Shutdown => "Shutdown",
        }
    }
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
    activity: Activity,
    progress: Progress,
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
    pub milestone_targets: Vec<String>,
    finished_before_reconnect: usize,
    units: HashMap<String, UnitRecord>,
    jobs: HashMap<u32, String>,
}

const HOST_MILESTONES: [&str; 4] = [
    "sysinit.target",
    "basic.target",
    "multi-user.target",
    "graphical.target",
];

const SHUTDOWN_MILESTONES: [&str; 3] = ["shutdown.target", "umount.target", "final.target"];

pub const HANDOFF_UNIT: &str = "display-manager.service";
