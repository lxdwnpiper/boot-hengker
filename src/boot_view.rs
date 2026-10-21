#[cfg(test)]
mod panel_render_tests;
mod run_timeline;
mod screen_chrome;
#[cfg(test)]
mod screen_render_fixtures;
mod stage_summary;
mod tree_height_fitting;
#[cfg(test)]
mod tree_render_tests;
mod unit_tree;

use std::time::Duration;

use jiff::fmt::friendly::{Designator, FractionalUnit, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::Line,
    widgets::{Block, Borders, Widget},
};
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

pub use self::screen_chrome::PasswordPrompt;
use self::{
    run_timeline::{RunningRun, Timeline},
    screen_chrome::{FOOTER_HEIGHT, Footer, Header, PasswordPopup},
    stage_summary::Stages,
    tree_height_fitting::rows_within_height,
    unit_tree::UnitTree,
};
use crate::{
    boot_model::{
        BootState, RowStatus, TreeRow, UnitKind, boot_origin::BootOrigin, flatten_forest,
    },
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
};

const DURATION_WIDTH: u16 = 8;
const MIN_BAR_WIDTH: u16 = 12;
const MIN_HEIGHT_FOR_TIMELINE: u16 = 12;
const MIN_TIMELINE_HEIGHT: u16 = run_timeline::CHROME_HEIGHT + 4;
const HEADER_HEIGHT: u16 = 1;
const BRANCH: &str = "├─ ";
const LAST_BRANCH: &str = "└─ ";
const PASSING_TRUNK: &str = "│  ";
const ENDED_TRUNK: &str = "   ";

const ELAPSED: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None)
    .fractional(Some(FractionalUnit::Second))
    .precision(Some(1));

pub struct BootView<'a> {
    pub state: &'a BootState,
    pub colors: &'a Colors,
    pub timing: &'a Timing,
    pub glyphs: &'a Glyphs,
    pub now: Duration,
    pub origin: &'a BootOrigin,
    pub password_prompt: Option<PasswordPrompt<'a>>,
}

impl BootView<'_> {
    fn timeline<'a>(&'a self, rows: &'a [TreeRow]) -> Timeline<'a> {
        let mut running: Vec<RunningRun> = rows
            .iter()
            .filter(|row| row.kind != UnitKind::Target)
            .filter_map(|row| match row.status {
                RowStatus::Running { queued_at, since } => Some(RunningRun {
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
            milestones: self.state.milestones(),
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
        let forest = self
            .state
            .waiting_forest(self.now.saturating_sub(self.timing.keep_finished_units_for));
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
        let footer = Footer {
            tally: self.state.tally(),
            colors: &self.colors.footer,
            glyphs: self.glyphs,
        };
        let fits_timeline = area.width >= run_timeline::LABEL_WIDTH + 1 + MIN_BAR_WIDTH
            && area.height >= MIN_HEIGHT_FOR_TIMELINE;
        let free = area
            .height
            .saturating_sub(HEADER_HEIGHT + stages.height() + FOOTER_HEIGHT);
        let heights = Heights::share(
            free,
            UnitTree::height_of(pending_count, 0),
            if fits_timeline { timeline.height() } else { 0 },
        );
        let budget = usize::from(heights.tree.saturating_sub(1));
        let rows = rows_within_height(&forest, budget);
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

        let [
            header_area,
            stage_area,
            tree_area,
            _,
            timeline_area,
            footer_area,
        ] = Layout::vertical([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Length(stages.height()),
            Constraint::Length(tree_height),
            Constraint::Fill(1),
            Constraint::Length(heights.timeline),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .areas(area);

        header.render(header_area, buf);
        stages.render(stage_area, buf);
        tree.render(tree_area, buf);
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
    timeline: u16,
}

impl Heights {
    fn share(free: u16, tree_need: u16, timeline_need: u16) -> Self {
        let leftover = free.saturating_sub(tree_need);
        let timeline = if leftover >= MIN_TIMELINE_HEIGHT {
            timeline_need.min(leftover)
        } else {
            0
        };
        Self {
            tree: free - timeline,
            timeline,
        }
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
        let heights = Heights::share(30, 40, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 50);
        assert_eq!((heights.tree, heights.timeline), (12, 18));
        let heights = Heights::share(30, 25, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 8);
        assert_eq!((heights.tree, heights.timeline), (22, 8));
    }
}
