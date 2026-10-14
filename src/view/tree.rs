use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::{BRANCH, DURATION_WIDTH, ELAPSED, ENDED_TRUNK, LAST_BRANCH, PASSING_TRUNK, fit_width};
use crate::{
    glyphs::Glyphs,
    model::{RowStatus, TreeRow, UnitKind},
    palette::Palette,
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
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
    pub frame: u64,
    pub palette: &'a Palette,
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

fn width_of(text: &str) -> u16 {
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}

impl UnitTree<'_> {
    pub fn height(&self) -> u16 {
        u16::try_from(self.rows.len() + usize::from(self.hidden > 0))
            .unwrap_or(u16::MAX)
            .saturating_add(HEADER_ROWS)
    }

    fn phase_fits_its_column(&self, columns: &Columns) -> bool {
        self.phase.width() <= usize::from(columns.label)
    }

    fn header(&self, columns: &Columns) -> Row<'static> {
        if !self.phase_fits_its_column(columns) {
            return Row::default().height(HEADER_ROWS);
        }
        let name = |text: &'static str| Line::from(text).fg(self.palette.muted);
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
        match (stage, row.kind, row.status) {
            (Stage::Current, ..) => Style::from(self.palette.running).bold(),
            (Stage::Reached, ..) => self.palette.done.into(),
            (_, _, RowStatus::Failed) => Style::from(self.palette.failed).bold(),
            (_, UnitKind::Target, _) => self.palette.target.into(),
            (_, _, RowStatus::Running) => self.palette.text.into(),
            (_, _, RowStatus::Waiting) => self.palette.waiting.into(),
            (_, _, RowStatus::Done) => self.palette.muted.into(),
        }
    }

    fn status_marker(&self, row: &TreeRow, stage: Stage) -> Span<'static> {
        match (stage, row.kind, row.status) {
            (Stage::Current, ..) => {
                format!("{} ", self.glyphs.stage_current).fg(self.palette.running)
            }
            (Stage::Upcoming, ..) => {
                format!("{} ", self.glyphs.stage_upcoming).fg(self.palette.target)
            }
            (Stage::Reached, ..) => format!("{} ", self.glyphs.done).fg(self.palette.done),
            (_, _, RowStatus::Failed) => {
                format!("[{}] ", self.glyphs.failed).fg(self.palette.failed)
            }
            (_, UnitKind::Target, _) => Span::default(),
            (_, _, RowStatus::Running) => {
                let spinner = SPINNER[usize::try_from(self.frame % 4).unwrap_or_default()];
                format!("[{spinner}] ").fg(self.palette.running)
            }
            (_, _, RowStatus::Waiting) => "[ ] ".fg(self.palette.waiting),
            (_, _, RowStatus::Done) => format!("[{}] ", self.glyphs.done).fg(self.palette.done),
        }
    }

    fn lead_width(&self, row: &TreeRow, stage: Stage) -> usize {
        tree_prefix(row).width() + self.status_marker(row, stage).width()
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

    fn row(&self, row: &TreeRow, stage: Stage, columns: &Columns) -> Row<'static> {
        let prefix = tree_prefix(row);
        let marker = self.status_marker(row, stage);
        let room = usize::from(columns.label).saturating_sub(self.lead_width(row, stage));
        let annotation = |text: &str, width: u16| {
            if row.chain.is_empty() {
                fit_width(text, usize::from(width), self.glyphs.ellipsis)
            } else {
                String::new()
            }
        };
        let timer = row
            .running_since
            .map(|since| self.now.saturating_sub(since))
            .filter(|running_for| *running_for >= self.hide_runs_shorter_than)
            .map(|running_for| ELAPSED.unsigned_duration_to_string(&running_for))
            .unwrap_or_default();
        let style = self.label_style(row, stage);
        Row::new(columns.shown([
            Line::from(vec![
                prefix.fg(self.palette.muted),
                marker,
                Span::styled(self.chained_label(row, room), style),
            ]),
            Line::from(annotation(row.kind.tag(), columns.tag)).fg(self.palette.muted),
            Line::from(annotation(&row.description, columns.description)).fg(self.palette.muted),
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
                    .fg(self.palette.waiting),
            ])
        });
        let columns = self.columns(area.width);
        Table::new(
            self.rows
                .iter()
                .zip(self.stages())
                .take(shown)
                .map(|(row, stage)| self.row(row, stage, &columns))
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

fn tree_prefix(row: &TreeRow) -> String {
    let ancestors = row
        .is_last_sibling
        .iter()
        .take(row.depth)
        .map(|ancestor_is_last| {
            if *ancestor_is_last {
                ENDED_TRUNK
            } else {
                PASSING_TRUNK
            }
        });
    let connector = match row.is_last_sibling.last() {
        Some(false) => BRANCH,
        _ => LAST_BRANCH,
    };
    once(ENDED_TRUNK)
        .chain(ancestors)
        .chain(once(connector))
        .collect()
}
