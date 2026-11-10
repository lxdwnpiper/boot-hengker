use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{self, Fill, Length},
        Layout, Rect,
    },
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Widget, Wrap},
};

use super::{
    ELAPSED, rule,
    screen_spacing::{ANNOTATION_COLUMN_WIDTH, TEXT_GAP},
};
use crate::{
    boot_model::{Tally, boot_origin::BootOrigin},
    screen_settings::{
        color_scheme::{FooterColors, HeaderColors, PasswordColors},
        font_glyphs::Glyphs,
    },
};

const SEPARATOR: &str = " · ";
const REJECTED_PASSPHRASE: &str = "Wrong passphrase, try again";

pub struct Header<'a> {
    pub origin: &'a BootOrigin,
    pub operator: Option<&'a str>,
    pub total: Option<Duration>,
    pub colors: &'a HeaderColors,
}

impl Header<'_> {
    fn origin(&self) -> Line<'static> {
        let mut parts = self.origin.parts();
        if let (Some(operator), Some(host)) = (self.operator, parts.first_mut()) {
            *host = format!("{operator}@{host}");
        }
        Line::from(
            parts
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
        let [origin_area, total_area] = area.layout(&Layout::horizontal([
            Fill(1),
            Length(ANNOTATION_COLUMN_WIDTH),
        ]));
        self.origin().render(origin_area, buf);
        if let Some(total) = self.total {
            Line::from(ELAPSED.unsigned_duration_to_string(&total))
                .fg(self.colors.total_time)
                .bold()
                .right_aligned()
                .render(total_area, buf);
        }
    }
}

pub struct Footer<'a> {
    pub tally: Tally,
    pub hint: Option<&'a str>,
    pub colors: &'a FooterColors,
    pub glyphs: &'a Glyphs,
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let failed = format!(
            "{TEXT_GAP}{} {} failed",
            self.glyphs.failed, self.tally.failed
        );
        let failed = if self.tally.failed > 0 {
            failed.fg(self.colors.failed).bold()
        } else {
            failed.fg(self.colors.no_failures)
        };
        Paragraph::new(Line::from(vec![
            format!("{} {} done", self.glyphs.done, self.tally.done).fg(self.colors.done),
            format!("{TEXT_GAP}{} running", self.tally.running).fg(self.colors.running),
            format!("{TEXT_GAP}{} waiting", self.tally.waiting).fg(self.colors.waiting),
            failed,
        ]))
        .block(rule(self.colors.rule, Line::default()))
        .render(area, buf);
        if let Some(hint) = self.hint {
            Line::from(hint)
                .fg(self.colors.done)
                .bold()
                .right_aligned()
                .render(
                    Rect {
                        y: area.y.saturating_add(1),
                        height: 1,
                        ..area
                    },
                    buf,
                );
        }
    }
}

pub struct PasswordPrompt<'a> {
    pub message: &'a str,
    pub hint: Option<&'a str>,
    pub input: String,
    pub sent: bool,
    pub rejected: bool,
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
        let message = Line::from(self.asking.message.fg(self.colors.message).bold());
        let hint = self
            .asking
            .hint
            .map(|hint| Line::from(hint.fg(self.colors.hint)));
        let rejection = (self.asking.rejected && !self.asking.sent)
            .then(|| Line::from(REJECTED_PASSPHRASE.fg(self.colors.rejected).bold()));
        let lines: Vec<Line> = once(message)
            .chain(hint)
            .chain(once(Line::default()))
            .chain(rejection)
            .chain(once(self.input()))
            .collect();
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
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
