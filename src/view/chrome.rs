use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::{DURATION_WIDTH, ELAPSED, rule};
use crate::{glyphs::Glyphs, model::Tally, origin::BootOrigin, palette::Palette};

pub const FOOTER_HEIGHT: u16 = 2;
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
        self.origin().render(
            Rect {
                width: area.width.saturating_sub(DURATION_WIDTH),
                ..area
            },
            buf,
        );
        if let Some(total) = self.total {
            Line::from(ELAPSED.unsigned_duration_to_string(&total))
                .fg(self.palette.text)
                .bold()
                .right_aligned()
                .render(area, buf);
        }
    }
}

pub struct Footer<'a> {
    pub tally: Tally,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let failed = format!("   {} {} failed", self.glyphs.failed, self.tally.failed);
        let failed = if self.tally.failed > 0 {
            failed.fg(self.palette.failed).bold()
        } else {
            failed.fg(self.palette.waiting)
        };
        let block = Block::new()
            .borders(Borders::TOP)
            .border_style(self.palette.muted);
        let inner = block.inner(area);
        block.render(area, buf);
        Line::from(vec![
            format!("{} {} done", self.glyphs.done, self.tally.done).fg(self.palette.done),
            format!("   {} running", self.tally.running).fg(self.palette.running),
            format!("   {} waiting", self.tally.waiting).fg(self.palette.waiting),
            failed,
        ])
        .render(inner, buf);
    }
}

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
