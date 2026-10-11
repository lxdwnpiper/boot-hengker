use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};

use super::{DURATION_WIDTH, ELAPSED, NOTEWORTHY_DURATION};
use crate::{
    glyphs::Glyphs,
    model::{PendingRow, RowStatus},
    palette::Palette,
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub struct PendingTree<'a> {
    pub rows: &'a [PendingRow],
    pub folded: usize,
    pub now: Duration,
    pub frame: u64,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl PendingTree<'_> {
    pub fn height(&self) -> u16 {
        u16::try_from(self.rows.len() + usize::from(self.folded > 0)).unwrap_or(u16::MAX)
    }

    fn label_color(&self, row: &PendingRow) -> Color {
        if row.is_target {
            return self.palette.target;
        }
        match row.status {
            RowStatus::Running => self.palette.text,
            RowStatus::Waiting => self.palette.waiting,
        }
    }

    fn status_marker(&self, row: &PendingRow) -> Span<'static> {
        if row.is_target {
            return Span::default();
        }
        match row.status {
            RowStatus::Running => {
                let spinner = SPINNER[usize::try_from(self.frame % 4).unwrap_or_default()];
                format!("[{spinner}] ").fg(self.palette.running)
            }
            RowStatus::Waiting => "[ ] ".fg(self.palette.waiting),
        }
    }

    fn chained_label(&self, row: &PendingRow, room: usize) -> String {
        let link = format!(" {} ", self.glyphs.chain_link);
        let Some(last) = row.chain.last() else {
            return row.label.clone();
        };
        let full: String = once(row.label.as_str())
            .chain(row.chain.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(&link);
        if full.chars().count() <= room || row.chain.len() == 1 {
            return full;
        }
        format!("{}{link}{}{link}{last}", row.label, self.glyphs.ellipsis)
    }

    fn row(&self, row: &PendingRow, label_room: usize) -> Row<'static> {
        let prefix = tree_prefix(row);
        let marker = self.status_marker(row);
        let room = label_room.saturating_sub(prefix.chars().count() + marker.width());
        let timer = row
            .running_since
            .map(|since| self.now.saturating_sub(since))
            .filter(|running_for| *running_for >= NOTEWORTHY_DURATION)
            .map(|running_for| ELAPSED.unsigned_duration_to_string(&running_for))
            .unwrap_or_default();
        let color = self.label_color(row);
        Row::new([
            Line::from(vec![
                prefix.fg(self.palette.muted),
                marker,
                self.chained_label(row, room).fg(color),
            ]),
            Line::from(timer).fg(color).right_aligned(),
        ])
    }
}

impl Widget for PendingTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let capacity = usize::from(area.height);
        let label_room = usize::from(area.width.saturating_sub(DURATION_WIDTH + 1));
        let shown = if usize::from(self.height()) > capacity {
            capacity.saturating_sub(1).min(self.rows.len())
        } else {
            self.rows.len()
        };
        let hidden = self.rows.len() - shown + self.folded;
        let overflow = (hidden > 0).then(|| {
            Row::new([format!("{} {hidden} more", self.glyphs.ellipsis).fg(self.palette.waiting)])
        });
        Table::new(
            self.rows
                .iter()
                .take(shown)
                .map(|row| self.row(row, label_room))
                .chain(overflow),
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
        .render(area, buf);
    }
}

fn tree_prefix(row: &PendingRow) -> String {
    let ancestors = row
        .is_last_sibling
        .iter()
        .skip(1)
        .take(row.depth.saturating_sub(1))
        .map(|ancestor_is_last| if *ancestor_is_last { "   " } else { "│  " });
    let connector = (row.depth > 0).then(|| match row.is_last_sibling.last() {
        Some(false) => "├─ ",
        _ => "└─ ",
    });
    ancestors.chain(connector).chain(once("")).collect()
}
