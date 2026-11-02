use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{BRANCH, ELAPSED, LAST_BRANCH, screen_spacing::ANNOTATION_COLUMN_WIDTH, unit_count};
use crate::{
    boot_model::BootState,
    screen_settings::{color_scheme::StageColors, font_glyphs::Glyphs},
};

const SHUTDOWN_SUCCESSOR: &str = "systemd-shutdown";

fn padded_phase_label(label: &str) -> String {
    format!(
        "{label:phase_width$}",
        phase_width = usize::from(ANNOTATION_COLUMN_WIDTH)
    )
}

pub struct Stages<'a> {
    pub state: &'a BootState,
    pub now: Duration,
    pub colors: &'a StageColors,
    pub glyphs: &'a Glyphs,
}

struct Conclusion<'a> {
    finished: &'static str,
    took: Duration,
    handed_to: &'a str,
}

impl Stages<'_> {
    pub fn height(&self) -> u16 {
        u16::try_from(self.state.finished_stages.len()).unwrap_or(u16::MAX)
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
                .started_display_manager()
                .unwrap_or("the console"),
        })
    }

    fn finished_phases(&self) -> impl Iterator<Item = Line<'static>> + '_ {
        self.state.finished_stages.iter().map(|stage| {
            Line::from(vec![
                BRANCH.fg(self.colors.branches),
                padded_phase_label(stage.label).fg(self.colors.finished_label),
                format!(
                    "{} done in {}",
                    self.glyphs.bead,
                    ELAPSED.unsigned_duration_to_string(
                        &stage.reached_at.saturating_sub(stage.started_at)
                    )
                )
                .fg(self.colors.finished_time),
                format!(" · {}", unit_count(stage.finished_units)).fg(self.colors.unit_count),
                if stage.failed_units > 0 {
                    format!(" · {} {} failed", self.glyphs.failed, stage.failed_units)
                        .fg(self.colors.failed_count)
                        .bold()
                } else {
                    Span::default()
                },
            ])
        })
    }

    fn concluded_phase(&self, conclusion: &Conclusion) -> Line<'static> {
        Line::from(vec![
            LAST_BRANCH.fg(self.colors.branches),
            format!(
                "{} {} finished in {}",
                self.glyphs.bead,
                conclusion.finished,
                ELAPSED.unsigned_duration_to_string(&conclusion.took)
            )
            .fg(self.colors.finished_time)
            .bold(),
            format!(" · handing over to {}", conclusion.handed_to).fg(self.colors.current),
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
            LAST_BRANCH.fg(self.colors.branches),
            padded_phase_label(self.state.phase.label())
                .fg(self.colors.current)
                .bold(),
            elapsed.fg(self.colors.current),
        ])
    }
}

impl Widget for Stages<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.finished_phases().collect::<Vec<_>>()).render(area, buf);
    }
}
