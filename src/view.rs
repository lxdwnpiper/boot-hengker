use std::iter::repeat_n;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::model::{BootState, Micros, MilestoneRow, PendingRow, RowStatus, Tally};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
const DONE: char = '√';
const FAILED: char = '×';
const RUNNING_BAR: char = '█';
const WAITING_BAR: char = '░';
const STILL_GOING: char = '▶';
const ELLIPSIS: char = '…';
const PHASE_ARROW: char = '→';

const MIN_BAR_WIDTH: u16 = 12;
const DURATION_WIDTH: usize = 7;
const MAX_FAILED_ROWS: usize = 3;
const AXIS_STEPS_SECONDS: [u64; 12] = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];
const AXIS_LABEL_SPACING: u64 = 8;

pub struct BootView<'a> {
    pub state: &'a BootState,
    pub now: Micros,
    pub host: &'a str,
    pub frame: u64,
}

struct Columns {
    left: u16,
    bar_x: u16,
    bar_width: u16,
}

impl Columns {
    fn for_width(width: u16) -> Self {
        let left = (width * 11 / 20).clamp(40.min(width), 72);
        let bar_x = left + 1;
        let bar_width = width.saturating_sub(bar_x);
        if bar_width < MIN_BAR_WIDTH {
            return Self {
                left: width,
                bar_x: width,
                bar_width: 0,
            };
        }
        Self {
            left,
            bar_x,
            bar_width,
        }
    }
}

struct TimeAxis {
    origin: Micros,
    end: Micros,
    step: Micros,
    width: u16,
}

impl TimeAxis {
    fn new(origin: Micros, now: Micros, width: u16) -> Self {
        let span_seconds = now.saturating_sub(origin).div_ceil(1_000_000).max(1);
        let max_labels = (u64::from(width) / AXIS_LABEL_SPACING).max(1);
        let step_seconds = AXIS_STEPS_SECONDS
            .into_iter()
            .find(|step| span_seconds.div_ceil(*step) <= max_labels)
            .unwrap_or(*AXIS_STEPS_SECONDS.last().expect("axis steps are not empty"));
        let end_seconds = span_seconds.div_ceil(step_seconds).max(1) * step_seconds;
        Self {
            origin,
            end: origin + end_seconds * 1_000_000,
            step: step_seconds * 1_000_000,
            width,
        }
    }

    fn column(&self, at: Micros) -> u16 {
        let clamped = at.clamp(self.origin, self.end);
        let fraction = (clamped - self.origin) as f64 / (self.end - self.origin) as f64;
        ((fraction * f64::from(self.width)) as u16).min(self.width.saturating_sub(1))
    }

    fn ticks(&self) -> impl Iterator<Item = AxisTick> + '_ {
        (0..)
            .map(|index| index * self.step)
            .take_while(|offset| self.origin + offset <= self.end)
            .map(|offset| AxisTick {
                column: self.column(self.origin + offset),
                offset,
            })
    }
}

struct AxisTick {
    column: u16,
    offset: Micros,
}

fn format_duration(micros: Micros) -> String {
    let tenths = micros / 100_000;
    if tenths < 600 {
        return format!("{}.{}s", tenths / 10, tenths % 10);
    }
    let seconds = micros / 1_000_000;
    if seconds < 3600 {
        return format!("{}m{:02}s", seconds / 60, seconds % 60);
    }
    format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60)
}

fn format_axis_label(offset: Micros) -> String {
    let seconds = offset / 1_000_000;
    if seconds < 60 || !seconds.is_multiple_of(60) {
        format!("{seconds}s")
    } else {
        format!("{}m", seconds / 60)
    }
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut kept: String = text.chars().take(width - 1).collect();
    kept.push(ELLIPSIS);
    kept
}

fn tree_prefix(row: &PendingRow) -> String {
    let mut prefix: String = row
        .is_last_sibling
        .iter()
        .skip(1)
        .take(row.depth.saturating_sub(1))
        .map(|ancestor_is_last| if *ancestor_is_last { "   " } else { "│  " })
        .collect();
    if row.depth > 0 {
        let is_last = row.is_last_sibling.last().copied().unwrap_or(true);
        prefix.push_str(if is_last { "└─ " } else { "├─ " });
    }
    prefix
}

struct Palette;

impl Palette {
    fn header() -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    fn dim() -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn running() -> Style {
        Style::new().fg(Color::Yellow)
    }

    fn waiting() -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn done() -> Style {
        Style::new().fg(Color::Green)
    }

    fn failed() -> Style {
        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    fn target() -> Style {
        Style::new().fg(Color::Cyan)
    }
}

struct Painter<'b> {
    buf: &'b mut Buffer,
    area: Rect,
}

impl Painter<'_> {
    fn text(&mut self, x: u16, y: u16, text: &str, style: Style) {
        if y >= self.area.height || x >= self.area.width {
            return;
        }
        let room = usize::from(self.area.width - x);
        self.buf
            .set_stringn(self.area.x + x, self.area.y + y, text, room, style);
    }

    fn right_aligned(&mut self, right_edge: u16, y: u16, text: &str, style: Style) {
        let width = text.chars().count() as u16;
        self.text(right_edge.saturating_sub(width), y, text, style);
    }

    fn fill(&mut self, x: u16, y: u16, length: u16, symbol: char, style: Style) {
        let run: String = repeat_n(symbol, usize::from(length)).collect();
        self.text(x, y, &run, style);
    }
}

impl BootView<'_> {
    fn draw_header(&self, painter: &mut Painter, columns: &Columns, axis: Option<&TimeAxis>) {
        let title = format!(
            "boot-tree  {} {PHASE_ARROW} {}",
            self.host,
            self.state.phase().label()
        );
        painter.text(0, 0, &title, Palette::header());
        let elapsed = format_duration(self.now.saturating_sub(self.state.origin()));
        painter.right_aligned(columns.left, 0, &elapsed, Palette::header());
        let Some(axis) = axis else {
            return;
        };
        axis.ticks().for_each(|tick| {
            let label = format_axis_label(tick.offset);
            let label_width = label.chars().count() as u16;
            let x = columns.bar_x + tick.column.min(axis.width.saturating_sub(label_width));
            painter.text(x, 0, &label, Palette::dim());
            painter.text(columns.bar_x + tick.column, 1, "|", Palette::dim());
        });
    }

    fn draw_pending(
        &self,
        painter: &mut Painter,
        columns: &Columns,
        axis: Option<&TimeAxis>,
        y: u16,
        row: &PendingRow,
    ) {
        let is_target = row.label.ends_with(".target");
        let prefix = tree_prefix(row);
        let marker = match (is_target, row.status) {
            (true, _) => String::new(),
            (false, RowStatus::Running) => format!("[{}] ", SPINNER[(self.frame % 4) as usize]),
            (false, RowStatus::Waiting) => "[ ] ".to_owned(),
        };
        let label_style = match (is_target, row.status) {
            (true, _) => Palette::target(),
            (false, RowStatus::Running) => Style::new(),
            (false, RowStatus::Waiting) => Palette::dim(),
        };
        let since = row.running_since.unwrap_or(row.queued_at);
        let duration = format_duration(self.now.saturating_sub(since));
        let fixed = prefix.chars().count() + marker.chars().count() + DURATION_WIDTH + 1;
        let label_room = usize::from(columns.left).saturating_sub(fixed);
        let mut x = 0;
        painter.text(x, y, &prefix, Palette::dim());
        x += prefix.chars().count() as u16;
        let marker_style = if row.status == RowStatus::Running {
            Palette::running()
        } else {
            Palette::dim()
        };
        painter.text(x, y, &marker, marker_style);
        x += marker.chars().count() as u16;
        painter.text(x, y, &truncate(&row.label, label_room), label_style);
        painter.right_aligned(columns.left, y, &duration, label_style);
        let Some(axis) = axis else {
            return;
        };
        let queued = axis.column(row.queued_at);
        let now = axis.column(self.now);
        match row.running_since {
            Some(running_since) => {
                let started = axis.column(running_since);
                if started > queued {
                    painter.fill(
                        columns.bar_x + queued,
                        y,
                        started - queued,
                        WAITING_BAR,
                        Palette::waiting(),
                    );
                }
                painter.fill(
                    columns.bar_x + started,
                    y,
                    now.saturating_sub(started).max(1),
                    RUNNING_BAR,
                    Palette::running(),
                );
            }
            None => {
                painter.fill(
                    columns.bar_x + queued,
                    y,
                    now.saturating_sub(queued).max(1),
                    WAITING_BAR,
                    Palette::waiting(),
                );
            }
        }
        let arrow_x = (now + 1).min(axis.width.saturating_sub(1));
        painter.text(
            columns.bar_x + arrow_x,
            y,
            &STILL_GOING.to_string(),
            Palette::dim(),
        );
    }
}

impl Painter<'_> {
    fn milestone(
        &mut self,
        columns: &Columns,
        axis: Option<&TimeAxis>,
        y: u16,
        row: &MilestoneRow,
    ) {
        let units = match row.finished_units {
            1 => "1 unit".to_owned(),
            count => format!("{count} units"),
        };
        let label = truncate(&row.label, 18);
        let Some(reached_at) = row.reached_at else {
            self.text(0, y, &format!("  {label:<18} {units:>10}"), Palette::dim());
            return;
        };
        self.text(
            0,
            y,
            &format!("{DONE} {label:<18} {units:>10}"),
            Palette::done(),
        );
        let duration = format_duration(reached_at.saturating_sub(row.started_at));
        self.right_aligned(columns.left, y, &duration, Palette::done());
        if let Some(axis) = axis {
            let start = axis.column(row.started_at);
            let end = axis.column(reached_at);
            self.fill(
                columns.bar_x + start,
                y,
                end.saturating_sub(start).max(1),
                RUNNING_BAR,
                Palette::done(),
            );
        }
    }

    fn footer(&mut self, tally: Tally, y: u16) {
        self.fill(0, y, self.area.width, '─', Palette::dim());
        let counts = format!(
            "{DONE} {} done   {} running   {} waiting   {FAILED} {} failed",
            tally.done, tally.running, tally.waiting, tally.failed
        );
        let style = if tally.failed > 0 {
            Palette::failed()
        } else {
            Style::new()
        };
        self.text(0, y + 1, &counts, style);
        let legend = format!("{RUNNING_BAR} running  {WAITING_BAR} waiting");
        let room_left = usize::from(self.area.width).saturating_sub(counts.chars().count());
        if room_left > legend.chars().count() + 2 {
            self.right_aligned(self.area.width, y + 1, &legend, Palette::dim());
        }
    }
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut painter = Painter { buf, area };
        let columns = Columns::for_width(area.width);
        let axis = (columns.bar_width > 0)
            .then(|| TimeAxis::new(self.state.origin(), self.now, columns.bar_width));
        self.draw_header(&mut painter, &columns, axis.as_ref());

        let milestones = self.state.milestones();
        let failed = self.state.failed_units();
        let shown_failed = failed.len().min(MAX_FAILED_ROWS);
        let failed_lines = shown_failed + usize::from(failed.len() > MAX_FAILED_ROWS);
        let footer_height = 2;
        let bottom_height = (milestones.len() + failed_lines + footer_height) as u16;
        let tree_top = 2;
        let tree_bottom = area.height.saturating_sub(bottom_height + 1).max(tree_top);

        let rows = self.state.pending_rows();
        let capacity = usize::from(tree_bottom - tree_top);
        let overflow = rows.len() > capacity;
        let visible = if overflow {
            capacity.saturating_sub(1)
        } else {
            rows.len()
        };
        rows.iter()
            .take(visible)
            .zip(tree_top..)
            .for_each(|(row, y)| self.draw_pending(&mut painter, &columns, axis.as_ref(), y, row));
        if overflow {
            let hidden = rows.len() - visible;
            painter.text(
                0,
                tree_top + visible as u16,
                &format!("{ELLIPSIS} {hidden} more"),
                Palette::dim(),
            );
        }

        let milestones_top = area.height.saturating_sub(bottom_height);
        milestones
            .iter()
            .zip(milestones_top..)
            .for_each(|(milestone, y)| painter.milestone(&columns, axis.as_ref(), y, milestone));
        let failed_top = milestones_top + milestones.len() as u16;
        let hidden_failures = failed.len().saturating_sub(MAX_FAILED_ROWS);
        failed
            .iter()
            .take(shown_failed)
            .map(|unit| format!("{FAILED} {unit} failed"))
            .chain((hidden_failures > 0).then(|| format!("{FAILED} {hidden_failures} more failed")))
            .zip(failed_top..)
            .for_each(|(line, y)| painter.text(0, y, &line, Palette::failed()));
        painter.footer(self.state.tally(), failed_top + failed_lines as u16);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::model::{ActiveState, Activity, JobResult, Phase, UnitFacts};

    fn facts(name: &str, after: &[&str]) -> UnitFacts {
        UnitFacts {
            name: name.to_owned(),
            description: String::new(),
            after: after.iter().map(ToString::to_string).collect(),
        }
    }

    fn render(state: &BootState, now: Micros, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    BootView {
                        state,
                        now,
                        host: "ryzen",
                        frame: 1,
                    },
                    frame.area(),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                let line: String = (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect();
                line.trim_end().to_owned()
            })
            .collect()
    }

    fn boot_in_progress() -> BootState {
        let mut state = BootState::new(Phase::Host);
        state.set_root("graphical.target".to_owned());
        state.track_unit(
            facts("graphical.target", &["multi-user.target"]),
            Activity::default(),
        );
        state.track_unit(
            facts(
                "multi-user.target",
                &["docker.service", "systemd-rfkill.service"],
            ),
            Activity::default(),
        );
        state.track_unit(
            facts("docker.service", &[]),
            Activity {
                state: Some(ActiveState::Activating),
                inactive_exit: Some(9_000_000),
                active_enter: None,
            },
        );
        state.track_unit(
            facts("systemd-rfkill.service", &[]),
            Activity {
                state: Some(ActiveState::Activating),
                inactive_exit: Some(6_000_000),
                active_enter: None,
            },
        );
        state.track_unit(facts("sysinit.target", &[]), Activity::default());
        state.track_unit(facts("nvidia-powerd.service", &[]), Activity::default());
        [
            "graphical.target",
            "multi-user.target",
            "docker.service",
            "systemd-rfkill.service",
            "sysinit.target",
            "nvidia-powerd.service",
        ]
        .into_iter()
        .zip(1..)
        .for_each(|(unit, id)| state.queue_job(id, unit, 1_000_000));
        state.finish_job(5, JobResult::Done, 4_000_000);
        state.finish_job(6, JobResult::Failed, 5_000_000);
        state
    }

    #[test]
    fn renders_tree_bars_milestones_and_footer() {
        let lines = render(&boot_in_progress(), 12_400_000, 110, 14);
        let screen = lines.join("\n");
        assert!(
            lines[0].starts_with("boot-tree  ryzen → stage 2"),
            "{screen}"
        );
        assert!(lines[0].contains("12.4s"), "{screen}");
        assert!(lines[2].starts_with("graphical.target"), "{screen}");
        assert!(lines[3].starts_with("└─ multi-user.target"), "{screen}");
        assert!(lines[4].starts_with("   ├─ [/] systemd-rfkill"), "{screen}");
        assert!(lines[5].starts_with("   └─ [/] docker"), "{screen}");
        assert!(lines[4].contains('█') && lines[4].contains('▶'), "{screen}");
        assert!(screen.contains("√ sysinit.target"), "{screen}");
        assert!(
            screen.contains("× nvidia-powerd.service failed"),
            "{screen}"
        );
        assert!(
            screen.contains("√ 1 done   2 running   2 waiting   × 1 failed"),
            "{screen}"
        );
    }

    #[test]
    fn narrow_screens_drop_the_timeline() {
        let lines = render(&boot_in_progress(), 12_400_000, 50, 14);
        assert!(
            lines.iter().all(|line| !line.contains('█')),
            "{}",
            lines.join("\n")
        );
        assert!(lines[4].contains("systemd-rfkill"));
    }

    #[test]
    fn overflowing_trees_are_cut_with_a_count() {
        let mut state = BootState::new(Phase::Host);
        (0..30).for_each(|id| {
            let name = format!("unit{id:02}.service");
            state.track_unit(facts(&name, &[]), Activity::default());
            state.queue_job(id, &name, 0);
        });
        let lines = render(&state, 1_000_000, 110, 12);
        assert!(
            lines.iter().any(|line| line.starts_with("… ")),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn durations_switch_units_past_a_minute() {
        assert_eq!(format_duration(12_400_000), "12.4s");
        assert_eq!(format_duration(65_000_000), "1m05s");
        assert_eq!(format_duration(3_720_000_000), "1h02m");
    }
}
