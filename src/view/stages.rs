use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{BRANCH, ELAPSED, LAST_BRANCH, unit_count};
use crate::{glyphs::Glyphs, model::BootState, palette::Palette};

const SHUTDOWN_SUCCESSOR: &str = "systemd-shutdown";
const PHASE_WIDTH: usize = 9;

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
        u16::try_from(self.lines().len()).unwrap_or(u16::MAX)
    }

    fn conclusion(&self) -> Option<Conclusion<'_>> {
        if let Some(exited_at) = self.state.manager_exited_at {
            return Some(Conclusion {
                finished: "Shutdown",
                took: exited_at.saturating_sub(self.state.origin),
                handed_to: SHUTDOWN_SUCCESSOR,
            });
        }
        self.state.is_complete().then(|| Conclusion {
            finished: "Boot",
            took: self.now.saturating_sub(self.state.origin),
            handed_to: self
                .state
                .handoff_unit
                .as_deref()
                .or(self.state.root.as_deref())
                .unwrap_or("the console"),
        })
    }

    fn finished_phases(&self) -> impl Iterator<Item = Line<'static>> + '_ {
        self.state.finished_stages.iter().map(|stage| {
            Line::from(vec![
                BRANCH.fg(self.palette.muted),
                format!("{:PHASE_WIDTH$}", stage.label).fg(self.palette.waiting),
                format!(
                    "{} done in {}",
                    self.glyphs.bead,
                    ELAPSED.unsigned_duration_to_string(
                        &stage.reached_at.saturating_sub(stage.started_at)
                    )
                )
                .fg(self.palette.done),
                format!(" · {}", unit_count(stage.finished_units)).fg(self.palette.waiting),
                if stage.failed_units > 0 {
                    format!(" · {} {} failed", self.glyphs.failed, stage.failed_units)
                        .fg(self.palette.failed)
                        .bold()
                } else {
                    Span::default()
                },
            ])
        })
    }

    fn concluded_phase(&self, conclusion: &Conclusion) -> Line<'static> {
        Line::from(vec![
            LAST_BRANCH.fg(self.palette.muted),
            format!(
                "{} {} finished in {}",
                self.glyphs.bead,
                conclusion.finished,
                ELAPSED.unsigned_duration_to_string(&conclusion.took)
            )
            .fg(self.palette.done)
            .bold(),
            format!(" · handing over to {}", conclusion.handed_to).fg(self.palette.text),
        ])
    }

    pub fn current_phase(&self) -> Line<'static> {
        if let Some(conclusion) = self.conclusion() {
            return self.concluded_phase(&conclusion);
        }
        let started_at = self
            .state
            .finished_stages
            .last()
            .map_or(self.state.origin, |stage| stage.reached_at);
        let elapsed = ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(started_at));
        Line::from(vec![
            LAST_BRANCH.fg(self.palette.muted),
            format!("{:PHASE_WIDTH$}", self.state.phase.label())
                .fg(self.palette.text)
                .bold(),
            elapsed.fg(self.palette.text),
        ])
    }

    fn lines(&self) -> Vec<Line<'static>> {
        self.finished_phases().collect()
    }
}

impl Widget for Stages<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines()).render(area, buf);
    }
}
