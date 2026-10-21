use std::{collections::HashSet, time::Duration};

use petgraph::{
    Direction::Incoming,
    algo::{DfsSpace, has_path_connecting},
    graphmap::DiGraphMap,
    visit::Visitable,
};

use super::{
    BootState, Outcome, Phase, Progress, UnitKind, UnitRecord,
    unit_facts::{display_label, summary},
};

type WaitingGraph<'a> = DiGraphMap<&'a str, ()>;
type WaitingSearch<'a> = DfsSpace<&'a str, <WaitingGraph<'a> as Visitable>::Map>;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SiblingRank {
    RunningSince(Duration),
    Failed,
    Waiting,
}

impl UnitRecord {
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
}

impl BootState {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot_test_fixtures::{
        nested_boot_sample, queued_boot, tree_outline, waiting, waiting_rows,
    };

    fn outline(state: &BootState) -> Vec<String> {
        tree_outline(&waiting_rows(state))
    }

    #[test]
    fn pending_units_nest_under_what_waits_on_them() {
        assert_eq!(
            outline(&nested_boot_sample()),
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
    fn devices_show_their_description() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [waiting("dev-nvme1n1.device", &[]).described("Lexar SSD NM790 2TB")],
        );
        assert_eq!(waiting_rows(&state)[0].label, "Lexar SSD NM790 2TB");
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
    fn ordering_cycles_do_not_repeat_units() {
        let state = queued_boot(
            Phase::Host,
            Duration::ZERO,
            [
                waiting("a.service", &["b.service"]),
                waiting("b.service", &["a.service"]),
            ],
        );
        assert_eq!(waiting_rows(&state).len(), 2);
    }
}
