use crate::boot_model::{RowStatus, TreeNode, TreeRow, UnitKind, flatten_forest};

const MIN_FOLDED_LINKS: usize = 2;

pub fn rows_within_height(forest: &[TreeNode], budget: usize) -> Vec<TreeRow> {
    [forest.to_vec(), fold_forest(forest.to_vec())]
        .into_iter()
        .map(flatten_forest)
        .find(|rows| rows.len() <= budget)
        .unwrap_or_else(|| {
            let essential = flatten_forest(prune_to_running(forest.to_vec(), 0)).len();
            let spare = budget.saturating_sub(essential + 1);
            flatten_forest(fold_forest(prune_to_running(forest.to_vec(), spare)))
        })
}

fn fold_forest(forest: Vec<TreeNode>) -> Vec<TreeNode> {
    forest.into_iter().map(|root| root.fold(false)).collect()
}

fn prune_to_running(forest: Vec<TreeNode>, mut spare: usize) -> Vec<TreeNode> {
    forest
        .into_iter()
        .filter_map(|root| {
            let anchors_the_tree = root.row.kind == UnitKind::Target;
            root.keep_on_paths_to_running(anchors_the_tree, &mut spare)
        })
        .collect()
}

impl TreeNode {
    fn leads_to_running(&self) -> bool {
        matches!(
            self.row.status,
            RowStatus::Running { .. } | RowStatus::Failed
        ) || self.children.iter().any(Self::leads_to_running)
    }

    fn keep_on_paths_to_running(self, anchored: bool, spare: &mut usize) -> Option<Self> {
        if !anchored && !self.leads_to_running() {
            *spare = spare.checked_sub(1)?;
        }
        Some(Self {
            children: self
                .children
                .into_iter()
                .filter_map(|child| child.keep_on_paths_to_running(false, spare))
                .collect(),
            row: self.row,
        })
    }

    fn is_chain_link(&self) -> bool {
        self.row.status == RowStatus::Waiting && self.children.len() == 1
    }

    fn foldable_links(&self) -> usize {
        let mut links = 0;
        let mut cursor = self;
        while cursor.is_chain_link() && cursor.children[0].is_chain_link() {
            links += 1;
            cursor = &cursor.children[0];
        }
        links
    }

    fn fold(mut self, may_absorb: bool) -> Self {
        let links = if may_absorb { self.foldable_links() } else { 0 };
        if links >= MIN_FOLDED_LINKS {
            let (chain, below) = (0..links).fold(
                (Vec::new(), self.children),
                |(mut chain, mut children), _| {
                    let only = children.remove(0);
                    chain.push(only.row.label);
                    (chain, only.children)
                },
            );
            self.row.chain = chain;
            self.children = below;
        }
        self.children = self
            .children
            .into_iter()
            .map(|child| child.fold(true))
            .collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        boot_model::{BootState, Phase},
        boot_test_fixtures::{
            activating, nested_boot_sample, queued_boot, running, tree_outline, waiting,
        },
    };

    fn chain_state(waits: &[(&str, &[&str])], running_unit: &str) -> BootState {
        queued_boot(
            Phase::Host,
            Duration::ZERO,
            waits.iter().map(|(name, after)| {
                if *name == running_unit {
                    running(name, after, Duration::ZERO)
                } else {
                    waiting(name, after)
                }
            }),
        )
    }

    fn fitted_outline(state: &BootState, budget: usize) -> Vec<String> {
        tree_outline(&rows_within_height(
            &state.waiting_forest(Duration::MAX),
            budget,
        ))
    }

    #[test]
    fn only_chains_leading_to_running_units_stay_in_view() {
        assert_eq!(
            fitted_outline(&nested_boot_sample(), 0),
            [
                "basic.target",
                "multi-user.target",
                "  docker",
                "  nm-wait",
                "    nm",
                "graphical.target",
            ]
        );
    }

    #[test]
    fn spare_rows_bring_back_waiting_units_under_kept_parents() {
        let state = chain_state(
            &[
                ("root.target", &["a.service", "b.service"]),
                ("a.service", &["a1.service"]),
                ("a1.service", &["a2.service"]),
                ("a2.service", &[]),
                ("b.service", &[]),
            ],
            "b.service",
        );
        assert_eq!(fitted_outline(&state, 4), ["root.target", "  b", "  a"]);
        assert_eq!(
            fitted_outline(&state, 5),
            ["root.target", "  b", "  a", "    a1", "      a2"]
        );
    }

    #[test]
    fn single_child_waiting_chains_fold_into_one_row() {
        let state = chain_state(
            &[
                ("root.target", &["a.service"]),
                ("a.service", &["b.target"]),
                ("b.target", &["c.service"]),
                ("c.service", &["d.service"]),
                ("d.service", &[]),
            ],
            "d.service",
        );
        assert_eq!(
            fitted_outline(&state, 0),
            ["root.target", "  a > b.target > c", "    d"]
        );
    }

    #[test]
    fn branches_and_short_chains_stay_unfolded() {
        let state = chain_state(
            &[
                ("root.target", &["a.service"]),
                ("a.service", &["b.service", "c.service"]),
                ("b.service", &["d.service"]),
                ("c.service", &[]),
                ("d.service", &[]),
            ],
            "d.service",
        );
        let mut both = chain_state(
            &[
                ("root.target", &["a.service"]),
                ("a.service", &["b.service", "c.service"]),
                ("b.service", &[]),
                ("c.service", &[]),
            ],
            "b.service",
        );
        both.update_activity("c.service", activating(Duration::ZERO));
        assert_eq!(
            fitted_outline(&state, 0),
            ["root.target", "  a", "    b", "      d"]
        );
        assert_eq!(
            fitted_outline(&both, 0),
            ["root.target", "  a", "    b", "    c"]
        );
    }

    #[test]
    fn with_nothing_running_only_the_root_target_stays_in_view() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [
                waiting("demo.target", &["idle.service"]),
                waiting("idle.service", &[]),
            ],
        );
        assert_eq!(fitted_outline(&state, 0), ["demo.target"]);
    }
}
