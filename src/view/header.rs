use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Stylize,
    text::Line,
    widgets::{Row, Table, Widget},
};

use super::{DURATION_WIDTH, ELAPSED};
use crate::palette::Palette;

pub struct Header<'a> {
    pub host: &'a str,
    pub total: Option<Duration>,
    pub palette: &'a Palette,
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let total = self
            .total
            .map(|total| ELAPSED.unsigned_duration_to_string(&total))
            .unwrap_or_default();
        Table::new(
            [Row::new([
                Line::from(format!("bootvine  {}", self.host)),
                Line::from(total).right_aligned(),
            ])],
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
        .fg(self.palette.text)
        .bold()
        .render(area, buf);
    }
}
