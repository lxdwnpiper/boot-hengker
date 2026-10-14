use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::rule;
use crate::{glyphs::Glyphs, palette::Palette};

pub struct PasswordPrompt<'a> {
    pub message: &'a str,
    pub input: String,
    pub sent: bool,
}

pub struct PasswordBar<'a> {
    pub asking: &'a PasswordPrompt<'a>,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl Widget for PasswordBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status = if self.asking.sent {
            format!(" checking{}", self.glyphs.ellipsis).fg(self.palette.waiting)
        } else {
            format!(" {}_", self.asking.input).fg(self.palette.text)
        };
        Paragraph::new(Line::from(vec![
            self.asking.message.fg(self.palette.running).bold(),
            status,
        ]))
        .block(rule(
            self.palette,
            Line::from(" password ".fg(self.palette.running)),
        ))
        .render(area, buf);
    }
}
