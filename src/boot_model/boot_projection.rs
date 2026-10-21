use std::time::Duration;

use super::{BootState, MilestoneRow, Tally, TreeNode};

#[derive(Clone, Debug)]
pub struct BootProjection {
    pub forest: Vec<TreeNode>,
    pub milestones: Vec<MilestoneRow>,
    pub tally: Tally,
}

impl BootState {
    #[must_use]
    pub fn project(&self, finished_since: Duration) -> BootProjection {
        BootProjection {
            forest: self.waiting_forest(finished_since),
            milestones: self.milestones(),
            tally: self.tally(),
        }
    }
}
