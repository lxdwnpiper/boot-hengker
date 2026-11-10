use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Widget},
};

use crate::{
    boot_model::boot_origin::BootOrigin,
    screen_settings::{color_scheme::Colors, font_glyphs::Glyphs},
};

const FRAME: Duration = Duration::from_millis(100);
const TRAIL: i64 = 16;
const RAIN_GLYPHS: &[u8] = b"0123456789ABCDEF/\\|<>[]{}#$%&*+=~^";
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
const TITLE: &str = " boot sequence ";
const WATCHWORD: &str = "BOOTVINE // SYSTEM BOOT";
const BAR_WIDTH: usize = 26;
const PANEL_HEIGHT: u16 = 6;
const PANEL_PADDING: u16 = 4;
const CURSOR_ON_AFTER: u128 = 500;
const SPINNER_STEP: u128 = 160;

pub struct IntroView<'a> {
    pub now: Duration,
    pub intro_for: Duration,
    pub operator: &'a str,
    pub origin: &'a BootOrigin,
    pub colors: &'a Colors,
    pub glyphs: &'a Glyphs,
}

impl IntroView<'_> {
    fn heading(&self) -> String {
        match (self.operator, self.origin.host.as_str()) {
            ("", host) => host.to_owned(),
            (operator, host) => format!("{operator}@{host}"),
        }
    }

    fn status(&self) -> String {
        let millis = self.now.as_millis();
        let step = usize::try_from(millis / SPINNER_STEP).unwrap_or_default();
        let spinner = SPINNER[step % SPINNER.len()];
        let cursor = if (millis / CURSOR_ON_AFTER).is_multiple_of(2) {
            '_'
        } else {
            ' '
        };
        format!("[ {spinner} ] setting up the console{cursor}")
    }

    fn progress(&self, glyphs: &Glyphs) -> String {
        let total = self.intro_for.as_millis().max(1);
        let elapsed = self.now.as_millis().min(total);
        let width = u128::try_from(BAR_WIDTH).unwrap_or_default();
        let filled = usize::try_from(width * elapsed / total)
            .unwrap_or_default()
            .min(BAR_WIDTH);
        let percent = usize::try_from(elapsed * 100 / total).unwrap_or_default();
        let full = glyphs.growing_tip.map_or('#', |tips| tips[7]);
        let empty = glyphs.waiting_bead;
        format!(
            "[{}{}] {percent:>3}%",
            full.to_string().repeat(filled),
            empty.to_string().repeat(BAR_WIDTH - filled)
        )
    }

    fn panel(&self, heading: &str, status: &str, bar: &str) -> Paragraph<'static> {
        Paragraph::new(vec![
            Line::from(heading.to_owned())
                .style(Style::new().fg(self.colors.tree.reached_stage).bold()),
            Line::from(WATCHWORD).style(Style::new().fg(self.colors.tree.target).bold()),
            Line::from(vec![
                Span::styled("  ", Style::new()),
                Span::styled(
                    status.to_owned(),
                    Style::new().fg(self.colors.tree.current_stage),
                ),
            ]),
            Line::from(vec![
                Span::styled("  ", Style::new()),
                Span::styled(
                    bar.to_owned(),
                    Style::new().fg(self.colors.tree.finished_marker),
                ),
            ]),
        ])
        .block(
            Block::bordered()
                .border_style(Style::new().fg(self.colors.tree.reached_stage))
                .padding(Padding::horizontal(1))
                .title(Line::from(TITLE).style(Style::new().fg(self.colors.tree.current_stage))),
        )
    }
}

impl Widget for IntroView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        rain(area, self.now, self.colors, buf);
        let heading = self.heading();
        let status = self.status();
        let bar = self.progress(self.glyphs);
        let widest = heading
            .chars()
            .count()
            .max(WATCHWORD.chars().count())
            .max(status.chars().count() + 2)
            .max(bar.chars().count() + 2);
        let width = u16::try_from(widest)
            .unwrap_or(u16::MAX)
            .saturating_add(PANEL_PADDING);
        let panel = area.centered(
            Constraint::Length(width.min(area.width)),
            Constraint::Length(PANEL_HEIGHT.min(area.height)),
        );
        if panel.is_empty() {
            return;
        }
        Clear.render(panel, buf);
        self.panel(&heading, &status, &bar).render(panel, buf);
    }
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    let scaled = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let folded = scaled ^ (scaled >> 27);
    let carried = folded.wrapping_mul(0x94D0_49BB_1331_11EB);
    carried ^ (carried >> 31)
}

fn rain(area: Rect, now: Duration, colors: &Colors, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let frame = u64::try_from(FRAME.as_millis()).unwrap_or(1);
    let tick = u64::try_from(now.as_millis()).unwrap_or(u64::MAX) / frame;
    let rows = i64::from(area.height);
    let span = u64::try_from(rows + TRAIL + 2).unwrap_or(1);
    let glyphs = u64::try_from(RAIN_GLYPHS.len()).unwrap_or(1);
    for column in 0..area.width {
        let seed = mix(u64::from(column).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let pace = 1 + seed % 2;
        let offset = (seed >> 21) % span;
        let head = i64::try_from(tick.wrapping_mul(pace).wrapping_add(offset) % span)
            .unwrap_or_default()
            - TRAIL;
        for depth in 0..TRAIL {
            let row = head - depth;
            if !(0..rows).contains(&row) {
                continue;
            }
            let depth_seed = u64::try_from(depth).unwrap_or_default();
            let pick = mix(seed ^ depth_seed.wrapping_mul(0x9E37) ^ tick);
            let glyph = char::from(RAIN_GLYPHS[usize::try_from(pick % glyphs).unwrap_or_default()]);
            let style = match depth {
                0 => Style::new().fg(colors.tree.running_marker).bold(),
                1..=4 => Style::new().fg(colors.tree.reached_stage),
                _ => Style::new().fg(colors.tree.branches),
            };
            if let Some(cell) =
                buf.cell_mut((area.x + column, area.y + u16::try_from(row).unwrap_or(0)))
            {
                cell.set_char(glyph);
                cell.set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn origin() -> BootOrigin {
        BootOrigin {
            host: "localarch".to_owned(),
            system: None,
            generation: None,
            kernel: "Linux".to_owned(),
        }
    }

    fn screen(operator: &str, now: Duration) -> Vec<String> {
        let origin = origin();
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    IntroView {
                        now,
                        intro_for: Duration::from_secs(3),
                        operator,
                        origin: &origin,
                        colors: &Colors::default(),
                        glyphs: &Glyphs::default(),
                    },
                    frame.area(),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let area = buffer.area;
        (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|column| buffer[(column, row)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn the_intro_names_the_operator_on_the_host() {
        let lines = screen("piperCs", Duration::ZERO).join("\n");
        assert!(lines.contains("piperCs@localarch"), "{lines}");
        assert!(lines.contains(WATCHWORD), "{lines}");
    }

    #[test]
    fn an_empty_operator_leaves_the_bare_host() {
        let lines = screen("", Duration::ZERO).join("\n");
        assert!(lines.contains(" localarch "), "{lines}");
        assert!(!lines.contains('@'), "{lines}");
    }

    #[test]
    fn the_progress_bar_fills_and_the_rain_moves_between_frames() {
        let first = screen("piperCs", Duration::ZERO);
        let later = screen("piperCs", Duration::from_millis(2000));
        let filled = |lines: &[String]| {
            lines
                .iter()
                .flat_map(|line| line.chars())
                .filter(|symbol| *symbol == '█')
                .count()
        };
        assert!(filled(&later) > filled(&first), "{}", later.join("\n"));
        assert_ne!(first, later);
    }
}
