use std::{
    cmp::Reverse,
    iter::{once, repeat_n},
    time::Duration,
};

use jiff::fmt::friendly::{Designator, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Row, Table, Widget},
};

use super::{fit_width, rule, unit_count};
use crate::{
    glyphs::Glyphs,
    model::{FinishedRun, MilestoneRow, TreeRow},
    palette::Palette,
};

pub const LABEL_WIDTH: u16 = 28;
pub const CHROME_HEIGHT: u16 = 3;
const MARK_WIDTH: u16 = 2;
const KERNEL_ORIGIN_LABEL: &str = "kernel";
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
    pub milestones: Vec<MilestoneRow>,
    pub finished: Vec<&'a FinishedRun>,
    pub running: Vec<&'a TreeRow>,
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
}

impl Timeline<'_> {
    pub fn height(&self) -> u16 {
        let entries = self.milestones.len() + self.finished.len() + self.running.len();
        if entries == 0 {
            return 0;
        }
        CHROME_HEIGHT.saturating_add(u16::try_from(entries).unwrap_or(u16::MAX))
    }

    fn finished_that_fit(&self, height: u16) -> Vec<&FinishedRun> {
        let bar_rows = usize::from(height.saturating_sub(CHROME_HEIGHT));
        let room = bar_rows.saturating_sub(self.milestones.len() + self.running.len());
        let mut longest_first = self.finished.clone();
        longest_first.sort_by_key(|run| Reverse(run.finished_at.saturating_sub(run.started_at)));
        longest_first.truncate(room);
        longest_first.sort_by_key(|run| run.started_at);
        longest_first
    }

    fn milestone_entry(&self, row: &MilestoneRow) -> TimelineEntry<'static> {
        let done = Style::from(self.palette.done);
        TimelineEntry {
            label: Line::styled(
                format!(
                    "{} {} ({})",
                    self.glyphs.done,
                    row.label,
                    unit_count(row.finished_units)
                ),
                done,
            ),
            segments: row
                .reached_at
                .map(|reached_at| Segment {
                    from: row.started_at,
                    to: reached_at,
                    symbol: self.glyphs.bead,
                    style: done,
                })
                .into_iter()
                .collect(),
            still_going_at: None,
        }
    }

    fn finished_entry<'a>(&self, run: &'a FinishedRun) -> TimelineEntry<'a> {
        let mark = if run.failed {
            self.glyphs.failed
        } else {
            self.glyphs.done
        };
        let outcome = Style::from(if run.failed {
            self.palette.failed
        } else {
            self.palette.done
        });
        TimelineEntry {
            label: Line::from(vec![
                Span::styled(format!("{mark} "), outcome),
                self.fitted(&run.label).fg(self.palette.waiting),
            ]),
            segments: vec![Segment {
                from: run.started_at,
                to: run.finished_at,
                symbol: self.glyphs.bead,
                style: outcome,
            }],
            still_going_at: None,
        }
    }

    fn fitted(&self, label: &str) -> String {
        fit_width(
            label,
            usize::from(LABEL_WIDTH - MARK_WIDTH),
            self.glyphs.ellipsis,
        )
    }

    fn running_entry<'a>(&self, row: &'a TreeRow) -> TimelineEntry<'a> {
        let since = row.running_since.unwrap_or(row.queued_at);
        let waited = (since > row.queued_at).then(|| Segment {
            from: row.queued_at,
            to: since,
            symbol: self.glyphs.waiting_bead,
            style: self.palette.waiting.into(),
        });
        let ran = Segment {
            from: since,
            to: self.now,
            symbol: self.glyphs.bead,
            style: self.palette.running.into(),
        };
        TimelineEntry {
            label: Line::from(vec![
                format!("{} ", self.glyphs.running_mark).fg(self.palette.running),
                self.fitted(&row.label).fg(self.palette.text),
            ]),
            segments: waited.into_iter().chain(once(ran)).collect(),
            still_going_at: Some(self.now),
        }
    }
}

impl Widget for Timeline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let axis = TimeAxis::new(
            self.origin,
            self.now,
            area.width.saturating_sub(LABEL_WIDTH + 1),
            *self.glyphs,
        );
        let muted = Style::from(self.palette.muted);
        let axis_rows = [
            Row::new([Line::default(), axis.labels(muted)]),
            Row::new([Line::default(), axis.ticks_line(&self.root_switches, muted)]),
        ];
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
        let bar_rows = entries.into_iter().map(|entry| {
            Row::new([
                entry.label,
                axis.bar(
                    &entry.segments,
                    entry.still_going_at,
                    &self.root_switches,
                    muted,
                ),
            ])
        });
        Table::new(
            axis_rows.into_iter().chain(bar_rows),
            [Constraint::Length(LABEL_WIDTH), Constraint::Fill(1)],
        )
        .block(rule(
            self.palette,
            Line::from(" timeline ".fg(self.palette.waiting)),
        ))
        .render(area, buf);
    }
}

struct TimelineEntry<'a> {
    label: Line<'a>,
    segments: Vec<Segment>,
    still_going_at: Option<Duration>,
}

struct Segment {
    from: Duration,
    to: Duration,
    symbol: char,
    style: Style,
}

struct Stroke {
    column: u16,
    glyphs: String,
    style: Style,
}

impl Stroke {
    fn run(column: u16, length: u16, symbol: char, style: Style) -> Self {
        Self {
            column,
            glyphs: repeat_n(symbol, usize::from(length)).collect(),
            style,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Glyph {
    symbol: char,
    style: Style,
}

const BLANK: Glyph = Glyph {
    symbol: ' ',
    style: Style::new(),
};

fn canvas_line(width: u16, strokes: impl IntoIterator<Item = Stroke>) -> Line<'static> {
    let cells = strokes
        .into_iter()
        .fold(vec![BLANK; usize::from(width)], |mut cells, stroke| {
            cells
                .iter_mut()
                .skip(usize::from(stroke.column))
                .zip(stroke.glyphs.chars())
                .for_each(|(cell, symbol)| {
                    *cell = Glyph {
                        symbol,
                        style: stroke.style,
                    };
                });
            cells
        });
    Line::from(
        cells
            .chunk_by(|left, right| left.style == right.style)
            .map(|run| {
                Span::styled(
                    run.iter().map(|glyph| glyph.symbol).collect::<String>(),
                    run[0].style,
                )
            })
            .collect::<Vec<_>>(),
    )
}

struct TimeAxis {
    origin: Duration,
    span: Duration,
    step: Duration,
    width: u16,
    glyphs: Glyphs,
}

impl TimeAxis {
    fn new(origin: Duration, now: Duration, width: u16, glyphs: Glyphs) -> Self {
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
            glyphs,
        }
    }

    fn column(&self, at: Duration) -> u16 {
        let offset = at.saturating_sub(self.origin).min(self.span);
        let column = offset.as_nanos() * u128::from(self.width) / self.span.as_nanos();
        u16::try_from(column)
            .unwrap_or(u16::MAX)
            .min(self.width.saturating_sub(1))
    }

    fn tick_offsets(&self) -> impl Iterator<Item = Duration> + '_ {
        (0..)
            .map(|index: u32| self.step * index)
            .take_while(|offset| *offset <= self.span)
    }

    fn labels(&self, style: Style) -> Line<'static> {
        canvas_line(
            self.width,
            self.tick_offsets().map(|offset| {
                let glyphs = if offset.is_zero() && self.origin.is_zero() {
                    KERNEL_ORIGIN_LABEL.to_owned()
                } else {
                    AXIS_LABEL.unsigned_duration_to_string(&offset)
                };
                let label_width = u16::try_from(glyphs.len()).unwrap_or(self.width);
                Stroke {
                    column: self
                        .column(self.origin + offset)
                        .min(self.width.saturating_sub(label_width)),
                    glyphs,
                    style,
                }
            }),
        )
    }

    fn ticks_line(&self, root_switches: &[Duration], style: Style) -> Line<'static> {
        canvas_line(
            self.width,
            root_switches
                .iter()
                .map(|at| Stroke::run(self.column(*at), 1, self.glyphs.root_switch_mark, style))
                .chain(
                    self.tick_offsets().map(|offset| {
                        Stroke::run(self.column(self.origin + offset), 1, '|', style)
                    }),
                ),
        )
    }

    fn bar(
        &self,
        segments: &[Segment],
        still_going_at: Option<Duration>,
        root_switches: &[Duration],
        switch_style: Style,
    ) -> Line<'static> {
        let switches = root_switches.iter().map(|at| {
            Stroke::run(
                self.column(*at),
                1,
                self.glyphs.root_switch_mark,
                switch_style,
            )
        });
        let bars = segments.iter().map(|segment| {
            let start = self.column(segment.from);
            let end = self.column(segment.to).max(start + 1);
            Stroke::run(start, end - start, segment.symbol, segment.style)
        });
        let still_going = still_going_at
            .map(|now| Stroke::run(self.column(now), 1, self.glyphs.still_going, switch_style));
        canvas_line(self.width, switches.chain(bars).chain(still_going))
    }
}
