use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{glyphs::Glyphs, palette::Palette};

const MAX_ROWS: usize = 3;

pub struct Failures<'a> {
    pub units: &'a [String],
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl Failures<'_> {
    pub fn height(&self) -> u16 {
        u16::try_from(self.lines().len()).unwrap_or(u16::MAX)
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let hidden = self.units.len().saturating_sub(MAX_ROWS);
        self.units
            .iter()
            .take(MAX_ROWS)
            .map(|unit| format!("{} {unit} failed", self.glyphs.failed))
            .chain((hidden > 0).then(|| format!("{} {hidden} more failed", self.glyphs.failed)))
            .map(|text| Line::from(text).fg(self.palette.failed).bold())
            .collect()
    }
}

impl Widget for Failures<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines()).render(area, buf);
    }
}
