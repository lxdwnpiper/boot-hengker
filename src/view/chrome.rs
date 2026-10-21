use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget, Wrap},
};

use super::{DURATION_WIDTH, ELAPSED};
use crate::{
    colors::{FooterColors, HeaderColors, PasswordColors},
    glyphs::Glyphs,
    model::Tally,
    origin::BootOrigin,
};

pub const FOOTER_HEIGHT: u16 = 2;
const SEPARATOR: &str = " · ";

pub struct Header<'a> {
    pub origin: &'a BootOrigin,
    pub total: Option<Duration>,
    pub colors: &'a HeaderColors,
}

impl Header<'_> {
    fn origin(&self) -> Line<'static> {
        Line::from(
            self.origin
                .parts()
                .into_iter()
                .enumerate()
                .flat_map(|(index, part)| {
                    let separator = (index > 0).then(|| SEPARATOR.fg(self.colors.separators));
                    let part = Span::from(part).fg(self.colors.origin);
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
                .fg(self.colors.total_time)
                .bold()
                .right_aligned()
                .render(area, buf);
        }
    }
}

pub struct Footer<'a> {
    pub tally: Tally,
    pub colors: &'a FooterColors,
    pub glyphs: &'a Glyphs,
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let failed = format!("   {} {} failed", self.glyphs.failed, self.tally.failed);
        let failed = if self.tally.failed > 0 {
            failed.fg(self.colors.failed).bold()
        } else {
            failed.fg(self.colors.no_failures)
        };
        let block = Block::new()
            .borders(Borders::TOP)
            .border_style(self.colors.rule);
        let inner = block.inner(area);
        block.render(area, buf);
        Line::from(vec![
            format!("{} {} done", self.glyphs.done, self.tally.done).fg(self.colors.done),
            format!("   {} running", self.tally.running).fg(self.colors.running),
            format!("   {} waiting", self.tally.waiting).fg(self.colors.waiting),
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

pub struct PasswordPopup<'a> {
    pub asking: &'a PasswordPrompt<'a>,
    pub colors: &'a PasswordColors,
    pub glyphs: &'a Glyphs,
}

impl PasswordPopup<'_> {
    const MIN_WIDTH: u16 = 40;

    fn input(&self) -> Line<'static> {
        if self.asking.sent {
            Line::from(format!("Checking{}", self.glyphs.ellipsis).fg(self.colors.checking))
        } else {
            Line::from(vec![
                "> ".fg(self.colors.frame),
                format!("{}_", self.asking.input).fg(self.colors.message),
            ])
        }
    }

    fn paragraph(&self) -> Paragraph<'_> {
        Paragraph::new(vec![
            Line::from(self.asking.message.fg(self.colors.message).bold()),
            Line::default(),
            self.input(),
        ])
        .wrap(Wrap { trim: false })
        .block(
            Block::bordered()
                .border_style(self.colors.frame)
                .padding(Padding::horizontal(2))
                .title(" Password ".fg(self.colors.frame).bold())
                .title_bottom(
                    Line::from(" Enter unlock · Ctrl+U clear ".fg(self.colors.hint))
                        .right_aligned(),
                ),
        )
    }
}

impl Widget for PasswordPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, Style::new().fg(self.colors.backdrop));
        let paragraph = self.paragraph();
        let width = u16::try_from(paragraph.line_width())
            .unwrap_or(u16::MAX)
            .max(Self::MIN_WIDTH)
            .min(area.width);
        let height = u16::try_from(paragraph.line_count(width)).unwrap_or(u16::MAX);
        let popup = area.centered(Constraint::Length(width), Constraint::Length(height));
        Clear.render(popup, buf);
        paragraph.render(popup, buf);
    }
}
