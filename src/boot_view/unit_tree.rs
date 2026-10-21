use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::{
    BRANCH, DURATION_WIDTH, ENDED_TRUNK, LAST_BRANCH, PASSING_TRUNK, RUNNING_ELAPSED, fit_width,
};
use crate::{
    boot_model::{RowStatus, TreeRow, UnitKind},
    screen_settings::{color_scheme::TreeColors, font_glyphs::Glyphs},
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
const SPINNER_STEP: Duration = Duration::from_millis(100);
const MIN_DESCRIPTION_WIDTH: u16 = 12;
const MIN_TAG_WIDTH: u16 = 6;
const MIN_LABEL_WIDTH: u16 = 24;
const HEADER_ROWS: u16 = 1;

pub struct UnitTree<'a> {
    pub phase: Line<'static>,
    pub rows: &'a [TreeRow],
    pub hidden: usize,
    pub now: Duration,
    pub hide_runs_shorter_than: Duration,
    pub colors: &'a TreeColors,
    pub glyphs: &'a Glyphs,
}

struct Columns {
    label: u16,
    tag: u16,
    description: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Reached,
    Current,
    Upcoming,
    Inside,
}

fn opens_a_stage(row: &TreeRow) -> bool {
    row.depth == 0 && row.kind == UnitKind::Target
}

fn spinner_at(now: Duration) -> char {
    let steps = now.as_millis() / SPINNER_STEP.as_millis();
    SPINNER[usize::try_from(steps % 4).unwrap_or_default()]
}

fn width_of(text: &str) -> u16 {
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}

impl UnitTree<'_> {
    pub fn height_of(rows: usize, hidden: usize) -> u16 {
        u16::try_from(rows + usize::from(hidden > 0))
            .unwrap_or(u16::MAX)
            .saturating_add(HEADER_ROWS)
    }

    pub fn height(&self) -> u16 {
        Self::height_of(self.rows.len(), self.hidden)
    }

    fn phase_fits_its_column(&self, columns: &Columns) -> bool {
        self.phase.width() <= usize::from(columns.label)
    }

    fn header(&self, columns: &Columns) -> Row<'static> {
        if !self.phase_fits_its_column(columns) {
            return Row::default().height(HEADER_ROWS);
        }
        let name = |text: &'static str| Line::from(text).fg(self.colors.columns);
        Row::new(columns.shown([
            self.phase.clone(),
            name("Kind"),
            name("Description"),
            name("Running").right_aligned(),
        ]))
    }

    fn stages(&self) -> impl Iterator<Item = Stage> + '_ {
        let current = self
            .rows
            .iter()
            .position(|row| opens_a_stage(row) && row.status != RowStatus::Done);
        self.rows.iter().enumerate().map(move |(index, row)| {
            if Some(index) == current {
                Stage::Current
            } else if opens_a_stage(row) && row.status == RowStatus::Done {
                Stage::Reached
            } else if opens_a_stage(row) {
                Stage::Upcoming
            } else {
                Stage::Inside
            }
        })
    }

    fn label_style(&self, row: &TreeRow, stage: Stage) -> Style {
        match stage {
            Stage::Current => Style::from(self.colors.current_stage).bold(),
            Stage::Reached => self.colors.reached_stage.into(),
            Stage::Upcoming | Stage::Inside => self.unit_label_style(row),
        }
    }

    fn unit_label_style(&self, row: &TreeRow) -> Style {
        match row.status {
            RowStatus::Failed => Style::from(self.colors.failed_unit).bold(),
            _ if row.kind == UnitKind::Target => self.colors.target.into(),
            RowStatus::Running { .. } => self.colors.running_unit.into(),
            RowStatus::Waiting => self.colors.waiting_unit.into(),
            RowStatus::Done => self.colors.finished_unit.into(),
        }
    }

    fn status_marker(&self, row: &TreeRow, stage: Stage) -> Span<'static> {
        match stage {
            Stage::Current => {
                format!("{} ", self.glyphs.stage_current).fg(self.colors.current_stage)
            }
            Stage::Upcoming => format!("{} ", self.glyphs.stage_upcoming).fg(self.colors.target),
            Stage::Reached => format!("{} ", self.glyphs.done).fg(self.colors.reached_stage),
            Stage::Inside => self.unit_marker(row),
        }
    }

    fn unit_marker(&self, row: &TreeRow) -> Span<'static> {
        match row.status {
            RowStatus::Failed => format!("[{}] ", self.glyphs.failed).fg(self.colors.failed_unit),
            _ if row.kind == UnitKind::Target => Span::default(),
            RowStatus::Running { .. } => {
                format!("[{}] ", spinner_at(self.now)).fg(self.colors.running_marker)
            }
            RowStatus::Waiting => "[ ] ".fg(self.colors.waiting_unit),
            RowStatus::Done => format!("[{}] ", self.glyphs.done).fg(self.colors.finished_marker),
        }
    }

    fn chained_label(&self, row: &TreeRow, room: usize) -> String {
        let link = format!(" {} ", self.glyphs.chain_link);
        let full: String = once(row.label.as_str())
            .chain(row.chain.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(&link);
        let label = match row.chain.as_slice() {
            [_, .., last] if full.width() > room => {
                format!("{}{link}{}{link}{last}", row.label, self.glyphs.ellipsis)
            }
            _ => full,
        };
        fit_width(&label, room, self.glyphs.ellipsis)
    }

    fn columns(&self, width: u16) -> Columns {
        let unannotated = width.saturating_sub(DURATION_WIDTH + 1);
        let annotated: Vec<(&TreeRow, Stage)> = self
            .rows
            .iter()
            .zip(self.stages())
            .filter(|(row, _)| {
                row.chain.is_empty() && !(row.description.is_empty() && row.kind.tag().is_empty())
            })
            .collect();
        let tag = annotated
            .iter()
            .map(|(row, _)| width_of(row.kind.tag()))
            .fold(MIN_TAG_WIDTH, u16::max);
        let content = unannotated.saturating_sub(2);
        let label = (content * 3 / 5).max(MIN_LABEL_WIDTH);
        let fits_annotations =
            !annotated.is_empty() && label + tag + MIN_DESCRIPTION_WIDTH <= content;
        fits_annotations.then_some(label).map_or(
            Columns {
                label: unannotated,
                tag: 0,
                description: 0,
            },
            |label| Columns {
                label,
                tag,
                description: content - label - tag,
            },
        )
    }

    fn row(&self, row: &TreeRow, stage: Stage, prefix: String, columns: &Columns) -> Row<'static> {
        let marker = self.status_marker(row, stage);
        let room = usize::from(columns.label).saturating_sub(prefix.width() + marker.width());
        let annotation = |text: &str, width: u16| {
            if row.chain.is_empty() {
                fit_width(text, usize::from(width), self.glyphs.ellipsis)
            } else {
                String::new()
            }
        };
        let timer = match row.status {
            RowStatus::Running { since, .. }
                if self.now.saturating_sub(since) >= self.hide_runs_shorter_than =>
            {
                RUNNING_ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(since))
            }
            _ => String::new(),
        };
        let style = self.label_style(row, stage);
        Row::new(columns.shown([
            Line::from(vec![
                prefix.fg(self.colors.branches),
                marker,
                Span::styled(self.chained_label(row, room), style),
            ]),
            Line::from(annotation(row.kind.tag(), columns.tag)).fg(self.colors.columns),
            Line::from(annotation(&row.description, columns.description)).fg(self.colors.columns),
            Line::from(timer).style(style).right_aligned(),
        ]))
    }
}

impl Columns {
    fn shown<T>(&self, cells: [T; 4]) -> impl Iterator<Item = T> {
        let present = [true, self.tag > 0, self.description > 0, true];
        cells
            .into_iter()
            .zip(present)
            .filter_map(|(cell, present)| present.then_some(cell))
    }

    fn widths(&self) -> impl Iterator<Item = Constraint> {
        self.shown([self.label, self.tag, self.description, DURATION_WIDTH])
            .map(Constraint::Length)
    }
}

impl Widget for UnitTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let capacity = usize::from(area.height.saturating_sub(HEADER_ROWS));
        let shown = if usize::from(self.height() - HEADER_ROWS) > capacity {
            capacity.saturating_sub(1).min(self.rows.len())
        } else {
            self.rows.len()
        };
        let hidden = self.rows.len() - shown + self.hidden;
        let overflow = (hidden > 0).then(|| {
            Row::new([
                format!("{ENDED_TRUNK}{} {hidden} more", self.glyphs.ellipsis)
                    .fg(self.colors.overflow),
            ])
        });
        let columns = self.columns(area.width);
        Table::new(
            self.rows
                .iter()
                .zip(self.stages())
                .zip(tree_prefixes(self.rows))
                .take(shown)
                .map(|((row, stage), prefix)| self.row(row, stage, prefix, &columns))
                .chain(overflow),
            columns.widths(),
        )
        .header(self.header(&columns))
        .render(area, buf);
        if !self.phase_fits_its_column(&columns) {
            self.phase.render(Rect { height: 1, ..area }, buf);
        }
    }
}

fn tree_prefixes(rows: &[TreeRow]) -> Vec<String> {
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
    let mut trunks: Vec<&str> = Vec::new();
    rows.iter()
        .zip(is_last)
        .map(|(row, last)| {
            trunks.truncate(row.depth);
            let connector = if last { LAST_BRANCH } else { BRANCH };
            let prefix = once(ENDED_TRUNK)
                .chain(trunks.iter().copied())
                .chain(once(connector))
                .collect();
            trunks.push(if last { ENDED_TRUNK } else { PASSING_TRUNK });
            prefix
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spinner_turns_once_per_step_whatever_the_frame_rate() {
        let turned: String = (0..5)
            .map(|step| spinner_at(SPINNER_STEP * step + Duration::from_millis(16)))
            .collect();
        assert_eq!(turned, "|/-\\|");
        assert_eq!(
            spinner_at(Duration::from_millis(99)),
            spinner_at(Duration::ZERO)
        );
    }

    fn at_depth(depth: usize) -> TreeRow {
        TreeRow {
            depth,
            label: String::new(),
            description: String::new(),
            kind: UnitKind::Service,
            status: RowStatus::Waiting,
            chain: Vec::new(),
        }
    }

    #[test]
    fn prefixes_close_each_branch_at_its_last_sibling() {
        let rows: Vec<TreeRow> = [0, 0, 1, 1, 2, 0].into_iter().map(at_depth).collect();
        assert_eq!(
            tree_prefixes(&rows),
            [
                "   ├─ ",
                "   ├─ ",
                "   │  ├─ ",
                "   │  └─ ",
                "   │     └─ ",
                "   └─ ",
            ]
        );
    }
}
