use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{self, Fill, Length},
        Layout, Rect,
    },
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::{
    BRANCH, ENDED_TRUNK, LAST_BRANCH, PASSING_TRUNK, RUNNING_ELAPSED, WHOLE_UNITS, fit_width,
    screen_spacing::{ANNOTATION_COLUMN_WIDTH, COLUMN_GAP_WIDTH, TEXT_ROW_HEIGHT},
};
use crate::{
    boot_model::{RowStatus, TreeRow, UnitKind},
    screen_settings::{color_scheme::TreeColors, font_glyphs::Glyphs},
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
const SPINNER_STEP: Duration = Duration::from_millis(100);
const LABEL_SHARE: u16 = 3;
const DESCRIPTION_SHARE: u16 = 2;
const SHOW_TIMEOUT_AFTER: Duration = Duration::from_secs(5);
const TIMEOUT_HEADER: &str = "Timeout";

struct RunTimeout {
    limit: Duration,
    exceeded: bool,
}

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
    constraints: Vec<Constraint>,
    label: u16,
    description: u16,
    shows_annotations: bool,
    shows_timeout: bool,
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

fn solved_columns(width: u16, shows_annotations: bool, shows_timeout: bool) -> Columns {
    let leading = if shows_annotations {
        vec![
            Fill(LABEL_SHARE),
            Length(ANNOTATION_COLUMN_WIDTH),
            Fill(DESCRIPTION_SHARE),
        ]
    } else {
        vec![Fill(1)]
    };
    let constraints: Vec<Constraint> = leading
        .into_iter()
        .chain(once(Length(ANNOTATION_COLUMN_WIDTH)))
        .chain(shows_timeout.then_some(Length(ANNOTATION_COLUMN_WIDTH)))
        .collect();
    let solved = Layout::horizontal(constraints.clone())
        .spacing(COLUMN_GAP_WIDTH)
        .split(Rect::new(0, 0, width, TEXT_ROW_HEIGHT));
    Columns {
        label: solved[0].width,
        description: if shows_annotations {
            solved[2].width
        } else {
            0
        },
        constraints,
        shows_annotations,
        shows_timeout,
    }
}

impl UnitTree<'_> {
    pub fn height_of(rows: usize, hidden: usize) -> u16 {
        u16::try_from(rows + usize::from(hidden > 0))
            .unwrap_or(u16::MAX)
            .saturating_add(TEXT_ROW_HEIGHT)
    }

    pub fn height(&self) -> u16 {
        Self::height_of(self.rows.len(), self.hidden)
    }

    fn phase_fits_its_column(&self, columns: &Columns) -> bool {
        self.phase.width() <= usize::from(columns.label)
    }

    fn header(&self, columns: &Columns) -> Row<'static> {
        if !self.phase_fits_its_column(columns) {
            return Row::default().height(TEXT_ROW_HEIGHT);
        }
        let name = |text: &'static str| Line::from(text).fg(self.colors.columns);
        Row::new(columns.shown([
            self.phase.clone(),
            name("Kind"),
            name("Description"),
            name("Running").right_aligned(),
            name(TIMEOUT_HEADER).right_aligned(),
        ]))
    }

    fn run_timeout(&self, row: &TreeRow) -> Option<RunTimeout> {
        let RowStatus::Running {
            since,
            deadline: Some(deadline),
            ..
        } = row.status
        else {
            return None;
        };
        let elapsed = self.now.saturating_sub(since);
        let limit = deadline.saturating_sub(since);
        (elapsed >= SHOW_TIMEOUT_AFTER).then_some(RunTimeout {
            limit,
            exceeded: elapsed >= limit,
        })
    }

    fn timeout_cell(&self, row: &TreeRow) -> Line<'static> {
        self.run_timeout(row)
            .map_or_else(Line::default, |timeout| {
                let color = if timeout.exceeded {
                    self.colors.failed_unit
                } else {
                    self.colors.columns
                };
                Line::from(WHOLE_UNITS.unsigned_duration_to_string(&timeout.limit)).fg(color)
            })
            .right_aligned()
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
            RowStatus::Failed(kind) if kind.has_own_cause() => {
                Style::from(self.colors.failed_unit).bold()
            }
            RowStatus::Failed(_) => Style::from(self.colors.failed_unit).dim(),
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
            RowStatus::Failed(_) => {
                format!("[{}] ", self.glyphs.failed).fg(self.colors.failed_unit)
            }
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
        let shows_timeout = self.rows.iter().any(|row| self.run_timeout(row).is_some());
        let has_annotated_rows = self.rows.iter().any(|row| {
            row.chain.is_empty() && !(row.description.is_none() && row.kind.tag().is_none())
        });
        let annotated = solved_columns(width, true, shows_timeout);
        if has_annotated_rows && annotated.description >= ANNOTATION_COLUMN_WIDTH {
            annotated
        } else {
            solved_columns(width, false, shows_timeout)
        }
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
        Row::new(
            columns.shown([
                Line::from(vec![
                    prefix.fg(self.colors.branches),
                    marker,
                    Span::styled(self.chained_label(row, room), style),
                ]),
                Line::from(annotation(
                    row.kind.tag().unwrap_or_default(),
                    ANNOTATION_COLUMN_WIDTH,
                ))
                .fg(self.colors.columns),
                Line::from(annotation(
                    row.description.as_deref().unwrap_or_default(),
                    columns.description,
                ))
                .fg(self.colors.columns),
                Line::from(timer).style(style).right_aligned(),
                self.timeout_cell(row),
            ]),
        )
    }
}

impl Columns {
    fn shown<T>(&self, cells: [T; 5]) -> impl Iterator<Item = T> {
        let present = [
            true,
            self.shows_annotations,
            self.shows_annotations,
            true,
            self.shows_timeout,
        ];
        cells
            .into_iter()
            .zip(present)
            .filter_map(|(cell, present)| present.then_some(cell))
    }
}

impl Widget for UnitTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let capacity = usize::from(area.height.saturating_sub(TEXT_ROW_HEIGHT));
        let shown = if usize::from(self.height() - TEXT_ROW_HEIGHT) > capacity {
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
        let table = Table::new(
            self.rows
                .iter()
                .zip(self.stages())
                .zip(tree_prefixes(self.rows))
                .take(shown)
                .map(|((row, stage), prefix)| self.row(row, stage, prefix, &columns)),
            columns.constraints.clone(),
        )
        .column_spacing(COLUMN_GAP_WIDTH)
        .header(self.header(&columns));
        match overflow {
            Some(overflow_row) => table.footer(overflow_row),
            None => table,
        }
        .render(area, buf);
        if !self.phase_fits_its_column(&columns) {
            let [phase_row, _] = area.layout(&Layout::vertical([Length(TEXT_ROW_HEIGHT), Fill(1)]));
            self.phase.render(phase_row, buf);
        }
    }
}

fn tree_prefixes(rows: &[TreeRow]) -> Vec<String> {
    let is_last = rows
        .iter()
        .rev()
        .scan(Vec::<bool>::new(), |later_sibling, row| {
            let last = !later_sibling.get(row.depth).copied().unwrap_or(false);
            later_sibling.resize(row.depth + 1, false);
            later_sibling[row.depth] = true;
            Some(last)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev();
    rows.iter()
        .zip(is_last)
        .scan(Vec::<&str>::new(), |trunks, (row, last)| {
            trunks.truncate(row.depth);
            let connector = if last { LAST_BRANCH } else { BRANCH };
            let prefix = once(ENDED_TRUNK)
                .chain(trunks.iter().copied())
                .chain(once(connector))
                .collect();
            trunks.push(if last { ENDED_TRUNK } else { PASSING_TRUNK });
            Some(prefix)
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
            description: None,
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
