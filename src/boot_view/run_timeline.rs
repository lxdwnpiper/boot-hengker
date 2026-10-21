use std::{cmp::Reverse, iter::once, time::Duration};

use jiff::fmt::friendly::{Designator, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Stylize},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Paragraph, Widget,
        canvas::{Canvas, Line as Stroke, Points},
    },
};

use super::{fit_width, rule, unit_count};
use crate::{
    boot_model::{FinishedRun, MilestoneRow},
    screen_settings::{color_scheme::TimelineColors, font_glyphs::Glyphs},
};

pub const LABEL_WIDTH: u16 = 28;
pub const CHROME_HEIGHT: u16 = 2;
const MARK_WIDTH: u16 = 2;
const SLOWEST_RUNS_SHOWN: usize = 5;
const KERNEL_ORIGIN_LABEL: &str = "Kernel";
const AXIS_LABEL_SPACING: u16 = 8;
const AXIS_STEPS: [Duration; 12] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_mins(1),
    Duration::from_mins(2),
    Duration::from_mins(5),
    Duration::from_mins(10),
    Duration::from_mins(30),
    Duration::from_hours(1),
];
const AXIS_LABEL: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None);

pub struct Timeline<'a> {
    pub origin: Duration,
    pub now: Duration,
    pub root_switches: Vec<Duration>,
    pub milestones: &'a [MilestoneRow],
    pub finished: Vec<&'a FinishedRun>,
    pub running: Vec<RunningRun<'a>>,
    pub colors: &'a TimelineColors,
    pub glyphs: &'a Glyphs,
}

pub struct RunningRun<'a> {
    pub label: &'a str,
    pub queued_at: Duration,
    pub since: Duration,
}

impl Timeline<'_> {
    pub fn height(&self) -> u16 {
        let entries = self.milestones.len()
            + self.finished.len().min(SLOWEST_RUNS_SHOWN)
            + self.running.len();
        if entries == 0 {
            return 0;
        }
        CHROME_HEIGHT.saturating_add(u16::try_from(entries).unwrap_or(u16::MAX))
    }

    fn finished_that_fit(&self, height: u16) -> Vec<&FinishedRun> {
        let bar_rows = usize::from(height.saturating_sub(CHROME_HEIGHT));
        let room = bar_rows.saturating_sub(self.milestones.len() + self.running.len());
        let mut longest_first = self.finished.clone();
        longest_first
            .sort_unstable_by_key(|run| Reverse(run.finished_at.saturating_sub(run.started_at)));
        longest_first.truncate(room.min(SLOWEST_RUNS_SHOWN));
        longest_first.sort_unstable_by_key(|run| run.started_at);
        longest_first
    }

    fn milestone_entry(&self, row: &MilestoneRow) -> TimelineEntry<'static> {
        TimelineEntry {
            label: Line::styled(
                format!(
                    "{} {} ({})",
                    self.glyphs.done,
                    row.label,
                    unit_count(row.finished_units)
                ),
                self.colors.milestone,
            ),
            segments: vec![Segment {
                from: row.started_at,
                to: row.reached_at,
                symbol: self.glyphs.bead,
                color: self.colors.milestone,
            }],
            still_going: false,
        }
    }

    fn finished_entry<'a>(&self, run: &'a FinishedRun) -> TimelineEntry<'a> {
        let mark = if run.failed {
            self.glyphs.failed
        } else {
            self.glyphs.done
        };
        let outcome = if run.failed {
            self.colors.failed
        } else {
            self.colors.finished
        };
        TimelineEntry {
            label: Line::from(vec![
                Span::styled(format!("{mark} "), outcome),
                self.fitted(&run.label).fg(self.colors.finished_label),
            ]),
            segments: vec![Segment {
                from: run.started_at,
                to: run.finished_at,
                symbol: self.glyphs.bead,
                color: outcome,
            }],
            still_going: false,
        }
    }

    fn fitted(&self, label: &str) -> String {
        fit_width(
            label,
            usize::from(LABEL_WIDTH - MARK_WIDTH),
            self.glyphs.ellipsis,
        )
    }

    fn running_entry(&self, run: &RunningRun) -> TimelineEntry<'static> {
        let waited = (run.since > run.queued_at).then_some(Segment {
            from: run.queued_at,
            to: run.since,
            symbol: self.glyphs.waiting_bead,
            color: self.colors.queued_bar,
        });
        let running_bar = self
            .glyphs
            .growing_tip
            .map_or(self.glyphs.bead, |tips| tips[7]);
        let ran = Segment {
            from: run.since,
            to: self.now,
            symbol: running_bar,
            color: self.colors.running,
        };
        TimelineEntry {
            label: Line::from(vec![
                format!("{} ", self.glyphs.running_mark).fg(self.colors.running),
                self.fitted(run.label).fg(self.colors.running_label),
            ]),
            segments: waited.into_iter().chain(once(ran)).collect(),
            still_going: true,
        }
    }
}

impl Widget for Timeline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = rule(
            self.colors.rule,
            Line::from(" Timeline ".fg(self.colors.title)),
        );
        let inner = block.inner(area);
        block.render(area, buf);
        let [label_area, bar_area] =
            Layout::horizontal([Constraint::Length(LABEL_WIDTH), Constraint::Fill(1)])
                .spacing(1)
                .areas(inner);
        let entries: Vec<TimelineEntry> = self
            .milestones
            .iter()
            .map(|row| self.milestone_entry(row))
            .chain(
                self.finished_that_fit(area.height)
                    .into_iter()
                    .map(|run| self.finished_entry(run)),
            )
            .chain(self.running.iter().map(|row| self.running_entry(row)))
            .collect();
        Paragraph::new(
            once(Line::default())
                .chain(entries.iter().map(|entry| entry.label.clone()))
                .collect::<Vec<_>>(),
        )
        .render(label_area, buf);
        let axis = TimeAxis::new(self.origin, self.now, bar_area.width);
        let label_row = f64::from(bar_area.height.saturating_sub(1).max(1));
        let axis_color = self.colors.axis;
        Canvas::default()
            .x_bounds(axis.bounds())
            .y_bounds([0.0, label_row])
            .paint(|ctx| {
                entries.iter().zip(1_u16..).for_each(|(entry, row)| {
                    let y = label_row - f64::from(row);
                    entry.segments.iter().for_each(|segment| {
                        ctx.marker(Marker::Custom(segment.symbol));
                        ctx.draw(&Stroke::new(
                            segment.from.as_secs_f64(),
                            y,
                            segment.to.as_secs_f64(),
                            y,
                            segment.color,
                        ));
                    });
                    if entry.still_going {
                        let tip = self
                            .glyphs
                            .growing_tip
                            .map_or(self.glyphs.still_going, |tips| {
                                tips[axis.eighths_into_column(self.now)]
                            });
                        let tip_color = if self.glyphs.growing_tip.is_some() {
                            self.colors.running
                        } else {
                            axis_color
                        };
                        ctx.marker(Marker::Custom(tip));
                        ctx.draw(&Points::new(&[(self.now.as_secs_f64(), y)], tip_color));
                    }
                });
                ctx.marker(Marker::Custom(self.glyphs.root_switch_mark));
                self.root_switches.iter().for_each(|at| {
                    let x = at.as_secs_f64();
                    ctx.draw(&Stroke::new(x, 0.0, x, label_row - 1.0, axis_color));
                });
                axis.tick_offsets().for_each(|offset| {
                    let label = axis.label(offset);
                    let x = (axis.origin + offset)
                        .as_secs_f64()
                        .min(axis.bounds()[1] - axis.columns_to_seconds(label.len() - 1));
                    ctx.print(x, label_row, label.fg(axis_color));
                });
            })
            .render(bar_area, buf);
    }
}

struct TimelineEntry<'a> {
    label: Line<'a>,
    segments: Vec<Segment>,
    still_going: bool,
}

struct Segment {
    from: Duration,
    to: Duration,
    symbol: char,
    color: Color,
}

struct TimeAxis {
    origin: Duration,
    span: Duration,
    step: Duration,
    width: u16,
}

impl TimeAxis {
    fn new(origin: Duration, now: Duration, width: u16) -> Self {
        let elapsed = now.saturating_sub(origin).max(AXIS_STEPS[0]);
        let max_labels = u128::from((width / AXIS_LABEL_SPACING).max(1));
        let step = AXIS_STEPS
            .into_iter()
            .find(|step| elapsed.as_nanos().div_ceil(step.as_nanos()) <= max_labels)
            .unwrap_or(AXIS_STEPS[AXIS_STEPS.len() - 1]);
        let steps = u32::try_from(elapsed.as_nanos().div_ceil(step.as_nanos())).unwrap_or(u32::MAX);
        Self {
            origin,
            span: step * steps.max(1),
            step,
            width,
        }
    }

    fn bounds(&self) -> [f64; 2] {
        [
            self.origin.as_secs_f64(),
            (self.origin + self.span).as_secs_f64(),
        ]
    }

    fn eighths_into_column(&self, at: Duration) -> usize {
        let last_column = u128::from(self.width.saturating_sub(1).max(1));
        let span = self.span.as_nanos().max(1);
        let scaled = at.saturating_sub(self.origin).as_nanos() * last_column;
        usize::try_from(scaled % span * 8 / span).unwrap_or_default()
    }

    fn columns_to_seconds(&self, columns: usize) -> f64 {
        let last_column = self.width.saturating_sub(1).max(1);
        let columns = u16::try_from(columns).unwrap_or(last_column);
        self.span.as_secs_f64() * f64::from(columns) / f64::from(last_column)
    }

    fn tick_offsets(&self) -> impl Iterator<Item = Duration> + '_ {
        (0..)
            .map(|index: u32| self.step * index)
            .take_while(|offset| *offset <= self.span)
    }

    fn label(&self, offset: Duration) -> String {
        if offset.is_zero() && self.origin.is_zero() {
            KERNEL_ORIGIN_LABEL.to_owned()
        } else {
            AXIS_LABEL.unsigned_duration_to_string(&offset)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tip_fills_its_column_in_eighths() {
        let axis = TimeAxis {
            origin: Duration::ZERO,
            span: Duration::from_secs(10),
            step: Duration::from_secs(1),
            width: 11,
        };
        let eighths = |millis: u64| axis.eighths_into_column(Duration::from_millis(millis));
        assert_eq!(eighths(2000), 0);
        assert_eq!(eighths(2125), 1);
        assert_eq!(eighths(2500), 4);
        assert_eq!(eighths(2999), 7);
        assert_eq!(eighths(3000), 0);
    }
}
