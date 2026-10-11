use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, Row, Table, Widget},
};

use crate::{glyphs::Glyphs, model::Tally, palette::Palette};

pub const HEIGHT: u16 = 2;
const LEGEND_GAP: usize = 2;

pub struct Footer<'a> {
    pub tally: Tally,
    pub shows_root_switch: bool,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl Footer<'_> {
    fn counts(&self) -> Line<'static> {
        let failed = format!("   {} {} failed", self.glyphs.failed, self.tally.failed);
        let failed = if self.tally.failed > 0 {
            failed.fg(self.palette.failed).bold()
        } else {
            failed.fg(self.palette.waiting)
        };
        Line::from(vec![
            format!("{} {} done", self.glyphs.done, self.tally.done).fg(self.palette.done),
            format!("   {} running", self.tally.running).fg(self.palette.running),
            format!("   {} waiting", self.tally.waiting).fg(self.palette.waiting),
            failed,
        ])
    }

    fn legend(&self) -> Line<'static> {
        let root_switch = if self.shows_root_switch {
            format!("  {} root switch", self.glyphs.root_switch_mark)
        } else {
            String::new()
        };
        Line::from(format!(
            "{} running  {} waiting{root_switch}",
            self.glyphs.bead, self.glyphs.waiting_bead
        ))
        .fg(self.palette.waiting)
        .right_aligned()
    }
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let counts = self.counts();
        let legend = self.legend();
        let legend_fits = counts.width() + legend.width() + LEGEND_GAP <= usize::from(area.width);
        let legend_width = if legend_fits {
            u16::try_from(legend.width()).unwrap_or(0)
        } else {
            0
        };
        Table::new(
            [Row::new([counts, legend])],
            [Constraint::Fill(1), Constraint::Length(legend_width)],
        )
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(self.palette.muted),
        )
        .render(area, buf);
    }
}
