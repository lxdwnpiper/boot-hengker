use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};

use super::{DURATION_WIDTH, ELAPSED};
use crate::{origin::BootOrigin, palette::Palette};

const SEPARATOR: &str = " · ";

pub struct Header<'a> {
    pub origin: &'a BootOrigin,
    pub total: Option<Duration>,
    pub palette: &'a Palette,
}

impl Header<'_> {
    fn origin(&self) -> Line<'static> {
        Line::from(
            self.origin
                .parts()
                .into_iter()
                .enumerate()
                .flat_map(|(index, part)| {
                    let separator = (index > 0).then(|| SEPARATOR.fg(self.palette.muted));
                    let part = Span::from(part).fg(self.palette.text);
                    let part = if index == 0 { part.bold() } else { part };
                    separator.into_iter().chain(once(part))
                })
                .collect::<Vec<_>>(),
        )
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let total = self
            .total
            .map(|total| ELAPSED.unsigned_duration_to_string(&total))
            .unwrap_or_default();
        Table::new(
            [Row::new([
                self.origin(),
                Line::from(total)
                    .fg(self.palette.text)
                    .bold()
                    .right_aligned(),
            ])],
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
        .render(area, buf);
    }
}
