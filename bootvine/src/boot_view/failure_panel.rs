use std::iter::once;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::{fit_width, rule};
use crate::{
    boot_model::{FailedUnit, FailureExplanation},
    screen_settings::{color_scheme::FailureColors, font_glyphs::Glyphs},
};

pub const RULE_HEIGHT: u16 = 1;
const LOG_INDENT: &str = "    ";
const REASON_GAP: &str = "  ";

pub struct FailurePanel<'a> {
    pub newest_first: Vec<&'a FailedUnit>,
    pub colors: &'a FailureColors,
    pub glyphs: &'a Glyphs,
}

fn log_lines(failed: &FailedUnit) -> &[String] {
    match &failed.explanation {
        FailureExplanation::Explained(cause) => &cause.log_lines,
        FailureExplanation::Unexplained => &[],
    }
}

impl FailurePanel<'_> {
    pub fn height(&self) -> u16 {
        if self.newest_first.is_empty() {
            return 0;
        }
        let rows: usize = self
            .newest_first
            .iter()
            .map(|failed| 1 + log_lines(failed).len())
            .sum();
        RULE_HEIGHT.saturating_add(u16::try_from(rows).unwrap_or(u16::MAX))
    }

    fn headline(&self, failed: &FailedUnit, width: usize) -> Line<'static> {
        let marker = format!("{} ", self.glyphs.failed);
        let label_room = width.saturating_sub(marker.width());
        let label = fit_width(&failed.label, label_room, self.glyphs.ellipsis);
        let reason_room = label_room.saturating_sub(label.width() + REASON_GAP.width());
        let reason = match &failed.explanation {
            FailureExplanation::Explained(cause) if reason_room > 0 => {
                fit_width(&cause.reason, reason_room, self.glyphs.ellipsis)
            }
            _ => String::new(),
        };
        let reason_span =
            (!reason.is_empty()).then(|| format!("{REASON_GAP}{reason}").fg(self.colors.reason));
        Line::from(
            [
                marker.fg(self.colors.unit),
                label.fg(self.colors.unit).bold(),
            ]
            .into_iter()
            .chain(reason_span)
            .collect::<Vec<Span>>(),
        )
    }

    fn block(&self, failed: &FailedUnit, width: usize) -> Vec<Line<'static>> {
        let log_room = width.saturating_sub(LOG_INDENT.width());
        once(self.headline(failed, width))
            .chain(log_lines(failed).iter().map(|line| {
                format!(
                    "{LOG_INDENT}{}",
                    fit_width(line, log_room, self.glyphs.ellipsis)
                )
                .fg(self.colors.log)
                .into()
            }))
            .collect()
    }

    fn lines_within(&self, rows: usize, width: usize) -> Vec<Line<'static>> {
        if self.newest_first.len() > rows {
            return self.headlines_with_overflow(rows, width);
        }
        let mut spare_rows = rows - self.newest_first.len();
        self.newest_first
            .iter()
            .flat_map(|failed| {
                let mut block = self.block(failed, width);
                let kept_log_lines = (block.len() - 1).min(spare_rows);
                spare_rows -= kept_log_lines;
                block.truncate(1 + kept_log_lines);
                block
            })
            .collect()
    }

    fn headlines_with_overflow(&self, rows: usize, width: usize) -> Vec<Line<'static>> {
        let Some(headline_rows) = rows.checked_sub(1) else {
            return Vec::new();
        };
        let hidden = self.newest_first.len() - headline_rows;
        self.newest_first
            .iter()
            .take(headline_rows)
            .map(|failed| self.headline(failed, width))
            .chain(once(
                Line::from(format!("{} {hidden} more failed", self.glyphs.ellipsis))
                    .fg(self.colors.overflow),
            ))
            .collect()
    }
}

impl Widget for FailurePanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = rule(
            self.colors.rule,
            Line::from(" Failures ".fg(self.colors.title)),
        );
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.lines_within(usize::from(inner.height), usize::from(inner.width)))
            .render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot_model::{FailureCause, FailureKind};

    fn explained(label: &str, log_lines: &[&str]) -> FailedUnit {
        FailedUnit {
            unit: format!("{label}.service"),
            label: label.to_owned(),
            kind: FailureKind::Broke,
            explanation: FailureExplanation::Explained(FailureCause {
                reason: "exited with status 1".to_owned(),
                log_lines: log_lines.iter().map(ToString::to_string).collect(),
            }),
        }
    }

    fn panel_text(failures: &[FailedUnit], rows: usize) -> Vec<String> {
        FailurePanel {
            newest_first: failures.iter().collect(),
            colors: &FailureColors::default(),
            glyphs: &Glyphs::fitting(|_| true),
        }
        .lines_within(rows, 60)
        .iter()
        .map(ToString::to_string)
        .collect()
    }

    #[test]
    fn the_newest_failure_keeps_its_lines_and_older_ones_shrink_to_a_headline() {
        let failures = [
            explained("web", &["bind: address in use", "giving up"]),
            explained("db", &["disk full"]),
            explained("cache", &["no memory"]),
        ];
        assert_eq!(
            panel_text(&failures, 5),
            [
                "× web  exited with status 1",
                "    bind: address in use",
                "    giving up",
                "× db  exited with status 1",
                "× cache  exited with status 1",
            ]
        );
    }

    #[test]
    fn room_left_by_the_newest_failure_goes_to_older_ones() {
        let failures = [
            explained("web", &["giving up"]),
            explained("db", &["disk full", "read-only file system"]),
            explained("cache", &["no memory"]),
        ];
        assert_eq!(
            panel_text(&failures, 6),
            [
                "× web  exited with status 1",
                "    giving up",
                "× db  exited with status 1",
                "    disk full",
                "    read-only file system",
                "× cache  exited with status 1",
            ]
        );
    }

    #[test]
    fn failures_beyond_the_room_are_counted() {
        let failures = [
            explained("web", &["giving up"]),
            explained("db", &[]),
            explained("cache", &[]),
            explained("queue", &[]),
        ];
        assert_eq!(
            panel_text(&failures, 3),
            [
                "× web  exited with status 1",
                "× db  exited with status 1",
                "… 2 more failed"
            ]
        );
    }

    #[test]
    fn an_unexplained_failure_shows_only_its_name() {
        let failures = [FailedUnit {
            explanation: FailureExplanation::Unexplained,
            ..explained("web", &[])
        }];
        assert_eq!(panel_text(&failures, 4), ["× web"]);
    }
}
