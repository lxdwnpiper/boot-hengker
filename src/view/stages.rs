use std::{iter::once, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::Line,
    widgets::{Block, Widget},
};

use super::{ELAPSED, RULE_STUB, rule, unit_count};
use crate::{glyphs::Glyphs, model::BootState, palette::Palette};

const SHUTDOWN_SUCCESSOR: &str = "systemd-shutdown";

pub struct Stages<'a> {
    pub state: &'a BootState,
    pub now: Duration,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

struct Conclusion<'a> {
    finished: &'static str,
    took: Duration,
    handed_to: &'a str,
}

impl Stages<'_> {
    pub fn height(&self) -> u16 {
        u16::try_from(self.state.finished_stages.len() + 1).unwrap_or(u16::MAX)
    }

    fn conclusion(&self) -> Option<Conclusion<'_>> {
        if let Some(exited_at) = self.state.manager_exited_at {
            return Some(Conclusion {
                finished: "shutdown",
                took: exited_at.saturating_sub(self.state.origin),
                handed_to: SHUTDOWN_SUCCESSOR,
            });
        }
        self.state.is_complete().then(|| Conclusion {
            finished: "boot",
            took: self.now.saturating_sub(self.state.origin),
            handed_to: self
                .state
                .handoff_unit
                .as_deref()
                .or(self.state.root.as_deref())
                .unwrap_or("the console"),
        })
    }

    fn finished_rules(&self) -> impl Iterator<Item = Block<'static>> + '_ {
        self.state.finished_stages.iter().map(|stage| {
            rule(
                self.palette,
                Line::from(vec![
                    format!("{RULE_STUB} {} {RULE_STUB} ", stage.label).fg(self.palette.muted),
                    format!(
                        "{} done in {}",
                        self.glyphs.bead,
                        ELAPSED.unsigned_duration_to_string(
                            &stage.reached_at.saturating_sub(stage.started_at)
                        )
                    )
                    .fg(self.palette.done),
                    format!(" · {} ", unit_count(stage.finished_units)).fg(self.palette.waiting),
                ]),
            )
        })
    }

    fn concluded_rule(&self, conclusion: &Conclusion) -> Block<'static> {
        rule(
            self.palette,
            Line::from(vec![
                format!("{RULE_STUB} ").fg(self.palette.muted),
                format!(
                    "{} {} finished in {}",
                    self.glyphs.bead,
                    conclusion.finished,
                    ELAPSED.unsigned_duration_to_string(&conclusion.took)
                )
                .fg(self.palette.done)
                .bold(),
                format!(" · handing over to {} ", conclusion.handed_to).fg(self.palette.text),
            ]),
        )
    }

    fn current_rule(&self) -> Block<'static> {
        let started_at = self
            .state
            .finished_stages
            .last()
            .map_or(self.state.origin, |stage| stage.reached_at);
        let elapsed = ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(started_at));
        rule(
            self.palette,
            Line::from(vec![
                format!("{RULE_STUB} ").fg(self.palette.muted),
                self.state.phase.label().fg(self.palette.text).bold(),
                " ".into(),
            ]),
        )
        .title(
            Line::from(vec![
                " ".into(),
                elapsed.fg(self.palette.text).bold(),
                format!(" {RULE_STUB}").fg(self.palette.muted),
            ])
            .right_aligned(),
        )
    }
}

impl Widget for Stages<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let last = self.conclusion().map_or_else(
            || self.current_rule(),
            |conclusion| self.concluded_rule(&conclusion),
        );
        area.rows()
            .zip(self.finished_rules().chain(once(last)))
            .for_each(|(row, rule)| rule.render(row, buf));
    }
}
