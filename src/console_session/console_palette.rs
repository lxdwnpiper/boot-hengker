use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsolePalette {
    Terminal,
    SixteenColorVt,
    EightColorVt,
}

impl ConsolePalette {
    #[must_use]
    pub fn of_virtual_terminal(font_uses_high_glyphs: bool) -> Self {
        if font_uses_high_glyphs {
            Self::EightColorVt
        } else {
            Self::SixteenColorVt
        }
    }

    pub fn fit_frame(self, frame: &mut Buffer) {
        if self == Self::Terminal {
            return;
        }
        frame.content.iter_mut().for_each(|cell| {
            cell.modifier.remove(Modifier::BOLD);
            if self == Self::EightColorVt {
                cell.fg = without_brightness(cell.fg);
                cell.bg = without_brightness(cell.bg);
            }
        });
    }
}

fn without_brightness(color: Color) -> Color {
    match color {
        Color::DarkGray | Color::White => Color::Gray,
        Color::LightRed => Color::Red,
        Color::LightGreen => Color::Green,
        Color::LightYellow => Color::Yellow,
        Color::LightBlue => Color::Blue,
        Color::LightMagenta => Color::Magenta,
        Color::LightCyan => Color::Cyan,
        Color::Indexed(bright @ 8..=15) => Color::Indexed(bright - 8),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Rect, style::Style};

    use super::*;

    fn frame_painted(style: Style) -> Buffer {
        let mut frame = Buffer::empty(Rect::new(0, 0, 1, 1));
        frame.set_style(frame.area, style);
        frame
    }

    #[test]
    fn eight_color_consoles_keep_dark_gray_visible() {
        let mut frame = frame_painted(Style::new().fg(Color::DarkGray).bold());
        ConsolePalette::EightColorVt.fit_frame(&mut frame);
        assert_eq!(frame.content[0].fg, Color::Gray);
        assert!(!frame.content[0].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn eight_color_consoles_fold_bright_colors_onto_normal_ones() {
        let mut frame = frame_painted(Style::new().fg(Color::LightYellow).bg(Color::Indexed(12)));
        ConsolePalette::EightColorVt.fit_frame(&mut frame);
        assert_eq!(frame.content[0].fg, Color::Yellow);
        assert_eq!(frame.content[0].bg, Color::Indexed(4));
    }

    #[test]
    fn sixteen_color_consoles_only_lose_bold() {
        let mut frame = frame_painted(Style::new().fg(Color::DarkGray).bold());
        ConsolePalette::SixteenColorVt.fit_frame(&mut frame);
        assert_eq!(frame.content[0].fg, Color::DarkGray);
        assert!(!frame.content[0].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn terminals_keep_the_frame_as_drawn() {
        let mut frame = frame_painted(Style::new().fg(Color::DarkGray).bold());
        ConsolePalette::Terminal.fit_frame(&mut frame);
        assert_eq!(frame.content[0].fg, Color::DarkGray);
        assert!(frame.content[0].modifier.contains(Modifier::BOLD));
    }
}
