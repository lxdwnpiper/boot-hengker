use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    thread,
};

use crossterm::{
    Command,
    cursor::MoveTo,
    queue,
    style::{Attribute, Color as ConsoleColor, SetAttribute, SetForegroundColor},
};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use rustix::termios::tcgetwinsize;

use crate::console_session::{stream_sources::FRAME_INTERVAL, watch_screen::overwrite_every_cell};

pub const RESUME_CONSOLE_MARKER: &str = "/run/bootvine/resume-console-open";
const BOOTVINE_TERMINAL: &str = "/dev/tty12";
const KERNEL_PRINTK: &str = "/proc/sys/kernel/printk";
const KERNEL_TIMESTAMPS: &str = "/sys/module/printk/parameters/time";
const KERNEL_INFO_VISIBLE: &str = "7";
const TIMESTAMPS_HIDDEN: &str = "N";
const FRAMES_TO_SETTLE: u32 = 2;
const BAND_HEIGHT: u16 = 4;
const BAND_TITLE: &str = " Looking for a hibernated session ";

struct ConfineScrolling(Rect);

impl Command for ConfineScrolling {
    fn write_ansi(&self, sequence: &mut impl fmt::Write) -> fmt::Result {
        write!(sequence, "\x1b[{};{}r", self.0.top() + 1, self.0.bottom())
    }
}

struct ReleaseScrolling;

impl Command for ReleaseScrolling {
    fn write_ansi(&self, sequence: &mut impl fmt::Write) -> fmt::Result {
        sequence.write_str("\x1b[r")
    }
}

#[must_use]
pub fn resume_console_is_open(marker: &Path) -> bool {
    marker.exists()
}

fn band_area(screen: Rect) -> Rect {
    let [band] = Layout::vertical([Constraint::Length(BAND_HEIGHT)])
        .flex(Flex::Center)
        .areas(screen);
    band
}

fn band_frame() -> Block<'static> {
    Block::new()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::new().fg(Color::Yellow))
        .title(BAND_TITLE)
}

fn open_bootvine_terminal() -> io::Result<File> {
    OpenOptions::new().write(true).open(BOOTVINE_TERMINAL)
}

fn draw_progress_band() -> io::Result<()> {
    let terminal = open_bootvine_terminal()?;
    let window = tcgetwinsize(&terminal)?;
    let band = band_area(Rect::new(0, 0, window.ws_col, window.ws_row));
    let progress_rows = band_frame().inner(band);
    if progress_rows.is_empty() {
        return Ok(());
    }
    let mut painted = Buffer::empty(band);
    band_frame().render(band, &mut painted);
    let mut backend = CrosstermBackend::new(terminal);
    overwrite_every_cell(&mut backend, &painted)?;
    queue!(
        backend,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(ConsoleColor::DarkYellow),
        ConfineScrolling(progress_rows),
        MoveTo(progress_rows.x, progress_rows.y),
    )?;
    Write::flush(&mut backend)
}

fn restore_bootvine_terminal() -> io::Result<()> {
    let mut terminal = open_bootvine_terminal()?;
    queue!(terminal, SetAttribute(Attribute::Reset), ReleaseScrolling)?;
    terminal.flush()
}

pub fn open_resume_console() -> io::Result<()> {
    let printk_levels = fs::read_to_string(KERNEL_PRINTK)?;
    let previous_level = printk_levels.split_whitespace().next().unwrap_or_default();
    let previous_timestamps = fs::read_to_string(KERNEL_TIMESTAMPS).unwrap_or_default();
    let marker = Path::new(RESUME_CONSOLE_MARKER);
    if let Some(directory) = marker.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(
        marker,
        format!("{previous_level}\n{}\n", previous_timestamps.trim()),
    )?;
    thread::sleep(FRAME_INTERVAL * FRAMES_TO_SETTLE);
    let _ = fs::write(KERNEL_TIMESTAMPS, TIMESTAMPS_HIDDEN);
    fs::write(KERNEL_PRINTK, KERNEL_INFO_VISIBLE)?;
    draw_progress_band()
}

pub fn close_resume_console() -> io::Result<()> {
    let marker = Path::new(RESUME_CONSOLE_MARKER);
    let recorded = fs::read_to_string(marker)?;
    let mut recorded_lines = recorded.lines();
    let previous_level = recorded_lines.next().unwrap_or_default();
    let previous_timestamps = recorded_lines.next().unwrap_or_default();
    let terminal_restored = restore_bootvine_terminal();
    if !previous_timestamps.is_empty() {
        let _ = fs::write(KERNEL_TIMESTAMPS, previous_timestamps);
    }
    let level_restored = fs::write(KERNEL_PRINTK, previous_level);
    fs::remove_file(marker)?;
    terminal_restored.and(level_restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_with_rows(rows: u16) -> Rect {
        Rect::new(0, 0, 80, rows)
    }

    #[test]
    fn the_band_sits_in_the_middle_with_two_rows_for_the_kernel() {
        let band = band_area(screen_with_rows(50));
        assert_eq!(band, Rect::new(0, 23, 80, BAND_HEIGHT));
        assert_eq!(band_frame().inner(band), Rect::new(0, 24, 80, 2));
    }

    #[test]
    fn the_scroll_region_counts_rows_from_one() {
        let mut sequence = String::new();
        ConfineScrolling(Rect::new(0, 24, 80, 2))
            .write_ansi(&mut sequence)
            .expect("writing to a string");
        assert_eq!(sequence, "\x1b[25;26r");
    }

    #[test]
    fn a_screen_without_room_for_the_band_leaves_the_kernel_no_rows() {
        let band = band_area(screen_with_rows(2));
        assert!(band_frame().inner(band).is_empty());
    }
}
