mod failure_panel;
#[cfg(test)]
mod frame_cost_bench;
#[cfg(test)]
mod panel_render_tests;
mod run_timeline;
mod screen_chrome;
#[cfg(test)]
mod screen_render_fixtures;
mod screen_spacing;
mod stage_summary;
mod tree_height_fitting;
#[cfg(test)]
mod tree_render_tests;
mod unit_tree;

use std::time::Duration;

use jiff::fmt::friendly::{Designator, FractionalUnit, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    style::Color,
    text::Line,
    widgets::{Block, Borders, Widget},
};
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

pub use self::screen_chrome::PasswordPrompt;
use self::{
    failure_panel::FailurePanel,
    run_timeline::{RunningRun, Timeline, timeline_fits},
    screen_chrome::{Footer, Header, PasswordPopup},
    screen_spacing::{PANEL_MINIMUM_HEIGHT, RULED_ROW_HEIGHT, TEXT_ROW_HEIGHT},
    stage_summary::Stages,
    tree_height_fitting::rows_within_height,
    unit_tree::UnitTree,
};
use crate::{
    boot_model::{
        BootState, MilestoneRow, RowStatus, TreeNode, TreeRow, UnitKind, boot_origin::BootOrigin,
        flatten_forest,
    },
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
};

const MIN_TIMELINE_HEIGHT: u16 = RULED_ROW_HEIGHT + PANEL_MINIMUM_HEIGHT;
const BRANCH: &str = "├─ ";
const LAST_BRANCH: &str = "└─ ";
const PASSING_TRUNK: &str = "│  ";
const ENDED_TRUNK: &str = "   ";

const ELAPSED: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None)
    .fractional(Some(FractionalUnit::Second))
    .precision(Some(1));

const RUNNING_ELAPSED: SpanPrinter = ELAPSED.precision(Some(2));

const WHOLE_UNITS: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None);

pub struct BootView<'a> {
    pub state: &'a BootState,
    pub projection: &'a BootProjection,
    pub colors: &'a Colors,
    pub timing: &'a Timing,
    pub glyphs: &'a Glyphs,
    pub now: Duration,
    pub origin: &'a BootOrigin,
    pub password_prompt: Option<PasswordPrompt<'a>>,
}

impl BootView<'_> {
    fn failure_panel(&self) -> FailurePanel<'_> {
        FailurePanel {
            newest_first: self
                .state
                .failed
                .iter()
                .rev()
                .filter(|failed| failed.kind.has_own_cause())
                .collect(),
            colors: &self.colors.failures,
            glyphs: self.glyphs,
        }
    }

    fn timeline<'a>(&'a self, rows: &'a [TreeRow]) -> Timeline<'a> {
        let mut running: Vec<RunningRun> = rows
            .iter()
            .filter(|row| row.kind != UnitKind::Target)
            .filter_map(|row| match row.status {
                RowStatus::Running {
                    queued_at, since, ..
                } => Some(RunningRun {
                    label: &row.label,
                    queued_at,
                    since,
                }),
                _ => None,
            })
            .collect();
        running.sort_by_key(|run| run.since);
        Timeline {
            origin: self.state.origin,
            now: self.now,
            root_switches: self
                .state
                .finished_stages
                .iter()
                .map(|stage| stage.reached_at)
                .collect(),
            milestones: &self.projection.milestones,
            finished: self
                .state
                .finished_runs
                .iter()
                .filter(|run| {
                    run.finished_at.saturating_sub(run.started_at)
                        >= self.timing.hide_runs_shorter_than
                })
                .collect(),
            running,
            colors: &self.colors.timeline,
            glyphs: self.glyphs,
        }
    }
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let forest = &self.projection.forest;
        let all_rows = flatten_forest(forest.clone());
        let pending_count = all_rows.len();
        let header = Header {
            origin: self.origin,
            total: (!self.state.finished_stages.is_empty())
                .then(|| self.now.saturating_sub(self.state.origin)),
            colors: &self.colors.header,
        };
        let stages = Stages {
            state: self.state,
            now: self.now,
            colors: &self.colors.stages,
            glyphs: self.glyphs,
        };
        let timeline = self.timeline(&all_rows);
        let failure_panel = self.failure_panel();
        let footer = Footer {
            tally: self.state.tally(),
            colors: &self.colors.footer,
            glyphs: self.glyphs,
        };
        let [header_area, stage_area, body_area, footer_area] = area.layout(&Layout::vertical([
            Length(TEXT_ROW_HEIGHT),
            Length(stages.height()),
            Fill(1),
            Length(RULED_ROW_HEIGHT),
        ]));
        let heights = Heights::share(
            body_area.height,
            UnitTree::height_of(pending_count, 0),
            failure_panel.height(),
            if timeline_fits(body_area) {
                timeline.height()
            } else {
                0
            },
        );
        let budget = usize::from(heights.tree.saturating_sub(TEXT_ROW_HEIGHT));
        let rows = rows_within_height(forest, budget);
        let tree = UnitTree {
            phase: stages.current_phase(),
            rows: &rows,
            hidden: pending_count - shown_units(&rows),
            now: self.now,
            hide_runs_shorter_than: self.timing.hide_runs_shorter_than,
            colors: &self.colors.tree,
            glyphs: self.glyphs,
        };
        let tree_height = tree.height().min(heights.tree);

        let [tree_area, _, failure_area, timeline_area] = body_area.layout(&Layout::vertical([
            Length(tree_height),
            Fill(1),
            Length(heights.failures),
            Length(heights.timeline),
        ]));

        header.render(header_area, buf);
        stages.render(stage_area, buf);
        tree.render(tree_area, buf);
        if heights.failures > 0 {
            failure_panel.render(failure_area, buf);
        }
        if heights.timeline > 0 {
            timeline.render(timeline_area, buf);
        }
        footer.render(footer_area, buf);
        if let Some(asking) = &self.password_prompt {
            PasswordPopup {
                asking,
                colors: &self.colors.password,
                glyphs: self.glyphs,
            }
            .render(area, buf);
        }
    }
}

struct Heights {
    tree: u16,
    failures: u16,
    timeline: u16,
}

impl Heights {
    fn share(free: u16, tree_need: u16, failures_need: u16, timeline_need: u16) -> Self {
        let reserved = PANEL_MINIMUM_HEIGHT
            .min(failures_need)
            .min(free.saturating_sub(PANEL_MINIMUM_HEIGHT));
        let failures = failures_need
            .min(free.saturating_sub(tree_need))
            .max(reserved);
        let failures = if failures >= RULED_ROW_HEIGHT {
            failures
        } else {
            0
        };
        let leftover = free.saturating_sub(tree_need).saturating_sub(failures);
        let timeline = if leftover >= MIN_TIMELINE_HEIGHT {
            timeline_need.min(leftover)
        } else {
            0
        };
        Self {
            tree: free - failures - timeline,
            failures,
            timeline,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BootProjection {
    pub forest: Vec<TreeNode>,
    pub milestones: Vec<MilestoneRow>,
}

#[must_use]
pub fn project_boot(state: &BootState, timing: &Timing, now: Duration) -> BootProjection {
    BootProjection {
        forest: state.waiting_forest(now.saturating_sub(timing.keep_finished_units_for)),
        milestones: state.milestones(),
    }
}

fn rule(color: Color, left: Line<'static>) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP)
        .border_style(color)
        .title(left)
}

fn shown_units(rows: &[TreeRow]) -> usize {
    rows.iter().map(|row| 1 + row.chain.len()).sum()
}

fn fit_width(text: &str, room: usize, ellipsis: &str) -> String {
    if text.width() <= room {
        return text.to_owned();
    }
    let (kept, _) = text.unicode_truncate(room.saturating_sub(ellipsis.width()));
    format!("{kept}{ellipsis}")
}

fn unit_count(count: usize) -> String {
    match count {
        1 => "1 unit".to_owned(),
        count => format!("{count} units"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_takes_the_room_and_the_timeline_only_what_is_left() {
        let heights = Heights::share(30, 40, 0, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 0, 50);
        assert_eq!((heights.tree, heights.timeline), (12, 18));
        let heights = Heights::share(30, 25, 0, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 0, 8);
        assert_eq!((heights.tree, heights.timeline), (22, 8));
    }

    #[test]
    fn failures_come_before_the_timeline_and_keep_a_few_rows_beside_a_full_tree() {
        let heights = Heights::share(30, 12, 6, 50);
        assert_eq!(
            (heights.tree, heights.failures, heights.timeline),
            (12, 6, 12)
        );
        let heights = Heights::share(30, 40, 9, 50);
        assert_eq!(
            (heights.tree, heights.failures, heights.timeline),
            (26, 4, 0)
        );
        let heights = Heights::share(5, 40, 9, 50);
        assert_eq!(
            (heights.tree, heights.failures, heights.timeline),
            (5, 0, 0)
        );
    }
}
