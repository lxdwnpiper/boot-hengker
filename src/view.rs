use std::{
    cmp::Reverse,
    iter::{once, repeat_n},
    time::Duration,
};

use jiff::fmt::friendly::{Designator, FractionalUnit, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Widget},
};

use crate::{
    model::{BootState, FinishedRun, MilestoneRow, PendingRow, RowStatus, Tally, paths_to_running},
    palette::Palette,
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
const DONE: char = '√';
const FAILED: char = '×';
const BEAD: char = '■';
const WAITING_BEAD: char = '·';
const STILL_GOING: char = '▶';
const RUNNING_MARK: char = '►';
const ROOT_SWITCH_MARK: char = '│';
const RULE: char = '─';
const RULE_STUB: &str = "──";
const KERNEL_ORIGIN_LABEL: &str = "kernel";
const SHUTDOWN_SUCCESSOR: &str = "systemd-shutdown";

const DURATION_WIDTH: u16 = 8;
const TIMELINE_LABEL_WIDTH: u16 = 28;
const MIN_BAR_WIDTH: u16 = 12;
const MIN_HEIGHT_FOR_TIMELINE: u16 = 12;
const TIMELINE_CHROME_HEIGHT: u16 = 3;
const MIN_TIMELINE_HEIGHT: u16 = TIMELINE_CHROME_HEIGHT + 4;
const HEADER_HEIGHT: u16 = 2;
const FOOTER_HEIGHT: u16 = 2;
const MAX_FAILED_ROWS: usize = 3;
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

const ELAPSED: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None)
    .fractional(Some(FractionalUnit::Second))
    .precision(Some(1));
const AXIS_LABEL: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None);

pub struct BootView<'a> {
    pub state: &'a BootState,
    pub palette: &'a Palette,
    pub now: Duration,
    pub host: &'a str,
    pub frame: u64,
}

struct TimelineContent<'a> {
    milestones: Vec<MilestoneRow>,
    finished: Vec<&'a FinishedRun>,
    running: Vec<&'a PendingRow>,
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let all_rows = self.state.pending_rows();
        let pending_count = all_rows.len();
        let rows = paths_to_running(all_rows);
        let folded = pending_count - rows.len();
        let mut running: Vec<&PendingRow> = rows
            .iter()
            .filter(|row| !row.is_target && row.status == RowStatus::Running)
            .collect();
        running.sort_by_key(|row| row.running_since);
        let milestones: Vec<MilestoneRow> = self
            .state
            .milestones()
            .into_iter()
            .filter(|row| row.reached_at.is_some())
            .collect();
        let failures = self.failure_lines();
        let stages = self.stage_lines(area.width);
        let stages_height = u16::try_from(stages.len()).unwrap_or(u16::MAX);
        let failures_height = u16::try_from(failures.len()).unwrap_or(u16::MAX);
        let fits_timeline = area.width >= TIMELINE_LABEL_WIDTH + 1 + MIN_BAR_WIDTH
            && area.height >= MIN_HEIGHT_FOR_TIMELINE;
        let timeline_entries = milestones.len() + self.state.finished_runs.len() + running.len();
        let timeline_need = if fits_timeline && timeline_entries > 0 {
            TIMELINE_CHROME_HEIGHT
                .saturating_add(u16::try_from(timeline_entries).unwrap_or(u16::MAX))
        } else {
            0
        };
        let heights = Heights::share(
            area.height
                .saturating_sub(HEADER_HEIGHT + stages_height + failures_height + FOOTER_HEIGHT),
            u16::try_from(rows.len() + usize::from(folded > 0)).unwrap_or(u16::MAX),
            timeline_need,
        );

        let [header, _, stage_area, tree, failed, _, timeline, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(HEADER_HEIGHT - 1),
            Constraint::Length(stages_height),
            Constraint::Length(heights.tree),
            Constraint::Length(failures_height),
            Constraint::Fill(1),
            Constraint::Length(heights.timeline),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .areas(area);

        self.header().render(header, buf);
        Paragraph::new(stages).render(stage_area, buf);
        self.tree(&rows, folded, tree.height).render(tree, buf);
        Paragraph::new(failures).render(failed, buf);
        if heights.timeline > 0 {
            let bar_rows = usize::from(heights.timeline.saturating_sub(TIMELINE_CHROME_HEIGHT));
            let finished_room = bar_rows.saturating_sub(milestones.len() + running.len());
            let mut finished: Vec<&FinishedRun> = self.state.finished_runs.iter().collect();
            finished.sort_by_key(|run| Reverse(run.finished_at.saturating_sub(run.started_at)));
            finished.truncate(finished_room);
            finished.sort_by_key(|run| run.started_at);
            self.timeline(
                &TimelineContent {
                    milestones,
                    finished,
                    running,
                },
                timeline.width,
            )
            .render(timeline, buf);
        }
        self.footer(self.state.tally(), footer.width)
            .render(footer, buf);
    }
}

struct Conclusion<'a> {
    finished: &'static str,
    took: Duration,
    handed_to: &'a str,
}

struct Heights {
    tree: u16,
    timeline: u16,
}

impl Heights {
    fn share(free: u16, tree_need: u16, timeline_need: u16) -> Self {
        if tree_need.saturating_add(timeline_need) <= free {
            return Self {
                tree: tree_need,
                timeline: timeline_need,
            };
        }
        let timeline = timeline_need
            .min(free.saturating_sub(tree_need).max(MIN_TIMELINE_HEIGHT))
            .min(free);
        Self {
            tree: free - timeline,
            timeline,
        }
    }
}

impl BootView<'_> {
    fn rule_line(
        &self,
        width: u16,
        left: Vec<Span<'static>>,
        right: Vec<Span<'static>>,
    ) -> Line<'static> {
        let used = Line::from(left.clone()).width() + Line::from(right.clone()).width();
        let fill: String = repeat_n(RULE, usize::from(width).saturating_sub(used)).collect();
        Line::from(
            left.into_iter()
                .chain(once(Span::styled(
                    fill,
                    Style::new().fg(self.palette.muted),
                )))
                .chain(right)
                .collect::<Vec<_>>(),
        )
    }

    fn header(&self) -> Table<'static> {
        let total = (!self.state.finished_stages.is_empty()).then(|| {
            ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(self.state.origin))
        });
        Table::new(
            [Row::new([
                Line::from(format!("bootvine  {}", self.host)),
                Line::from(total.unwrap_or_default()).right_aligned(),
            ])],
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
        .style(Style::new().fg(self.palette.text))
        .bold()
    }

    fn stage_lines(&self, width: u16) -> Vec<Line<'static>> {
        let finished = self.state.finished_stages.iter().map(|stage| {
            self.rule_line(
                width,
                vec![
                    Span::styled(
                        format!("{RULE_STUB} {} {RULE_STUB} ", stage.label),
                        Style::new().fg(self.palette.muted),
                    ),
                    Span::styled(
                        format!(
                            "{BEAD} done in {}",
                            ELAPSED.unsigned_duration_to_string(
                                &stage.reached_at.saturating_sub(stage.started_at)
                            )
                        ),
                        Style::new().fg(self.palette.done),
                    ),
                    Span::styled(
                        format!(" · {} ", unit_count(stage.finished_units)),
                        Style::new().fg(self.palette.waiting),
                    ),
                ],
                Vec::new(),
            )
        });
        let current_started_at = self
            .state
            .finished_stages
            .last()
            .map_or(self.state.origin, |stage| stage.reached_at);
        let conclusion = match self.state.manager_exited_at {
            Some(exited_at) => Some(Conclusion {
                finished: "shutdown",
                took: exited_at.saturating_sub(self.state.origin),
                handed_to: SHUTDOWN_SUCCESSOR,
            }),
            None => self.state.is_complete().then(|| Conclusion {
                finished: "boot",
                took: self.now.saturating_sub(self.state.origin),
                handed_to: self
                    .state
                    .handoff_unit
                    .as_deref()
                    .or(self.state.root.as_deref())
                    .unwrap_or("the console"),
            }),
        };
        let current = if let Some(conclusion) = conclusion {
            self.rule_line(
                width,
                vec![
                    Span::styled(format!("{RULE_STUB} "), Style::new().fg(self.palette.muted)),
                    Span::styled(
                        format!(
                            "{BEAD} {} finished in {}",
                            conclusion.finished,
                            ELAPSED.unsigned_duration_to_string(&conclusion.took)
                        ),
                        Style::new().fg(self.palette.done).bold(),
                    ),
                    Span::styled(
                        format!(" · handing over to {} ", conclusion.handed_to),
                        Style::new().fg(self.palette.text),
                    ),
                ],
                Vec::new(),
            )
        } else {
            self.rule_line(
                width,
                vec![
                    Span::styled(format!("{RULE_STUB} "), Style::new().fg(self.palette.muted)),
                    Span::styled(
                        self.state.phase.label(),
                        Style::new().fg(self.palette.text).bold(),
                    ),
                    Span::raw(" "),
                ],
                vec![
                    Span::raw(" "),
                    Span::styled(
                        ELAPSED.unsigned_duration_to_string(
                            &self.now.saturating_sub(current_started_at),
                        ),
                        Style::new().fg(self.palette.text).bold(),
                    ),
                    Span::styled(format!(" {RULE_STUB}"), Style::new().fg(self.palette.muted)),
                ],
            )
        };
        finished.chain(once(current)).collect()
    }

    fn failure_lines(&self) -> Vec<Line<'static>> {
        let units = &self.state.failed;
        let hidden = units.len().saturating_sub(MAX_FAILED_ROWS);
        units
            .iter()
            .take(MAX_FAILED_ROWS)
            .map(|unit| format!("{FAILED} {unit} failed"))
            .chain((hidden > 0).then(|| format!("{FAILED} {hidden} more failed")))
            .map(|text| Line::styled(text, Style::new().fg(self.palette.failed).bold()))
            .collect()
    }

    fn label_style(&self, row: &PendingRow) -> Style {
        match (row.is_target, row.status) {
            (true, _) => Style::new().fg(self.palette.target),
            (false, RowStatus::Running) => Style::new().fg(self.palette.text),
            (false, RowStatus::Waiting) => Style::new().fg(self.palette.waiting),
        }
    }

    fn status_marker(&self, row: &PendingRow) -> Span<'static> {
        match (row.is_target, row.status) {
            (true, _) => Span::raw(""),
            (false, RowStatus::Running) => {
                let spinner = SPINNER[usize::try_from(self.frame % 4).unwrap_or_default()];
                Span::styled(
                    format!("[{spinner}] "),
                    Style::new().fg(self.palette.running),
                )
            }
            (false, RowStatus::Waiting) => {
                Span::styled("[ ] ", Style::new().fg(self.palette.waiting))
            }
        }
    }

    fn tree<'a>(&self, rows: &'a [PendingRow], folded: usize, height: u16) -> Table<'a> {
        let capacity = usize::from(height);
        let shown = if rows.len() + usize::from(folded > 0) > capacity {
            capacity.saturating_sub(1).min(rows.len())
        } else {
            rows.len()
        };
        let hidden = rows.len() - shown + folded;
        let overflow = (hidden > 0).then(|| {
            Row::new([Line::styled(
                format!("… {hidden} more"),
                Style::new().fg(self.palette.waiting),
            )])
        });
        Table::new(
            rows.iter()
                .take(shown)
                .map(|row| self.tree_row(row))
                .chain(overflow),
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
    }

    fn tree_row<'a>(&self, row: &'a PendingRow) -> Row<'a> {
        let since = row.running_since.unwrap_or(row.queued_at);
        Row::new([
            Line::from(vec![
                Span::styled(tree_prefix(row), Style::new().fg(self.palette.muted)),
                self.status_marker(row),
                Span::styled(row.label.as_str(), self.label_style(row)),
            ]),
            Line::from(ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(since)))
                .style(self.label_style(row))
                .right_aligned(),
        ])
    }

    fn timeline<'a>(&self, content: &TimelineContent<'a>, width: u16) -> Table<'a> {
        let axis = TimeAxis::new(
            self.state.origin,
            self.now,
            width.saturating_sub(TIMELINE_LABEL_WIDTH + 1),
        );
        let root_switches: Vec<Duration> = self
            .state
            .finished_stages
            .iter()
            .map(|stage| stage.reached_at)
            .collect();
        let muted = Style::new().fg(self.palette.muted);
        let axis_rows = [
            Row::new([Line::default(), axis.labels(muted)]),
            Row::new([Line::default(), axis.ticks_line(&root_switches, muted)]),
        ];
        let entries = content
            .milestones
            .iter()
            .map(|row| self.milestone_entry(row))
            .chain(content.finished.iter().map(|run| self.finished_entry(run)))
            .chain(content.running.iter().map(|row| self.running_entry(row)));
        let bar_rows = entries.map(|entry| {
            Row::new([
                entry.label,
                axis.bar(&entry.segments, entry.still_going_at, &root_switches, muted),
            ])
        });
        Table::new(
            axis_rows.into_iter().chain(bar_rows),
            [
                Constraint::Length(TIMELINE_LABEL_WIDTH),
                Constraint::Fill(1),
            ],
        )
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().fg(self.palette.muted))
                .title(Line::styled(
                    " timeline ",
                    Style::new().fg(self.palette.waiting),
                )),
        )
    }

    fn milestone_entry(&self, row: &MilestoneRow) -> TimelineEntry<'static> {
        let done = Style::new().fg(self.palette.done);
        TimelineEntry {
            label: Line::styled(
                format!("{DONE} {} ({})", row.label, unit_count(row.finished_units)),
                done,
            ),
            segments: row
                .reached_at
                .map(|reached_at| Segment {
                    from: row.started_at,
                    to: reached_at,
                    symbol: BEAD,
                    style: done,
                })
                .into_iter()
                .collect(),
            still_going_at: None,
        }
    }

    fn finished_entry<'a>(&self, run: &'a FinishedRun) -> TimelineEntry<'a> {
        let (mark, outcome) = if run.failed {
            (FAILED, Style::new().fg(self.palette.failed))
        } else {
            (DONE, Style::new().fg(self.palette.done))
        };
        TimelineEntry {
            label: Line::from(vec![
                Span::styled(format!("{mark} "), outcome),
                Span::styled(run.label.as_str(), Style::new().fg(self.palette.waiting)),
            ]),
            segments: vec![Segment {
                from: run.started_at,
                to: run.finished_at,
                symbol: BEAD,
                style: outcome,
            }],
            still_going_at: None,
        }
    }

    fn running_entry<'a>(&self, row: &'a PendingRow) -> TimelineEntry<'a> {
        let since = row.running_since.unwrap_or(row.queued_at);
        let waited = (since > row.queued_at).then(|| Segment {
            from: row.queued_at,
            to: since,
            symbol: WAITING_BEAD,
            style: Style::new().fg(self.palette.waiting),
        });
        let ran = Segment {
            from: since,
            to: self.now,
            symbol: BEAD,
            style: Style::new().fg(self.palette.running),
        };
        TimelineEntry {
            label: Line::from(vec![
                Span::styled(
                    format!("{RUNNING_MARK} "),
                    Style::new().fg(self.palette.running),
                ),
                Span::styled(row.label.as_str(), Style::new().fg(self.palette.text)),
            ]),
            segments: waited.into_iter().chain(once(ran)).collect(),
            still_going_at: Some(self.now),
        }
    }

    fn footer(&self, tally: Tally, width: u16) -> Paragraph<'static> {
        let failed_style = if tally.failed > 0 {
            Style::new().fg(self.palette.failed).bold()
        } else {
            Style::new().fg(self.palette.waiting)
        };
        let counts = vec![
            Span::styled(
                format!("{DONE} {} done", tally.done),
                Style::new().fg(self.palette.done),
            ),
            Span::styled(
                format!("   {} running", tally.running),
                Style::new().fg(self.palette.running),
            ),
            Span::styled(
                format!("   {} waiting", tally.waiting),
                Style::new().fg(self.palette.waiting),
            ),
            Span::styled(format!("   {FAILED} {} failed", tally.failed), failed_style),
        ];
        let switch_legend = if self.state.finished_stages.is_empty() {
            String::new()
        } else {
            format!("  {ROOT_SWITCH_MARK} root switch")
        };
        let legend = format!("{BEAD} running  {WAITING_BEAD} waiting{switch_legend}");
        let used = Line::from(counts.clone()).width() + legend.chars().count();
        let gap = usize::from(width).saturating_sub(used);
        let spans = if gap >= 2 {
            counts
                .into_iter()
                .chain([
                    Span::raw(" ".repeat(gap)),
                    Span::styled(legend, Style::new().fg(self.palette.waiting)),
                ])
                .collect()
        } else {
            counts
        };
        Paragraph::new(Line::from(spans)).block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().fg(self.palette.muted)),
        )
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
                .map(|at| Stroke::run(self.column(*at), 1, ROOT_SWITCH_MARK, style))
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
        let switches = root_switches
            .iter()
            .map(|at| Stroke::run(self.column(*at), 1, ROOT_SWITCH_MARK, switch_style));
        let bars = segments.iter().map(|segment| {
            let start = self.column(segment.from);
            let end = self.column(segment.to).max(start + 1);
            Stroke::run(start, end - start, segment.symbol, segment.style)
        });
        let still_going =
            still_going_at.map(|now| Stroke::run(self.column(now), 1, STILL_GOING, switch_style));
        canvas_line(self.width, switches.chain(bars).chain(still_going))
    }
}

fn tree_prefix(row: &PendingRow) -> String {
    let ancestors = row
        .is_last_sibling
        .iter()
        .skip(1)
        .take(row.depth.saturating_sub(1))
        .map(|ancestor_is_last| if *ancestor_is_last { "   " } else { "│  " });
    let connector = (row.depth > 0).then(|| match row.is_last_sibling.last() {
        Some(false) => "├─ ",
        _ => "└─ ",
    });
    ancestors.chain(connector).chain(once("")).collect()
}

fn unit_count(count: usize) -> String {
    match count {
        1 => "1 unit".to_owned(),
        count => format!("{count} units"),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::model::{ActiveState, Activity, JobKind, JobResult, Phase, UnitFacts};

    const MS: Duration = Duration::from_millis(1);

    fn facts(name: &str, after: &[&str]) -> UnitFacts {
        UnitFacts {
            name: name.to_owned(),
            description: String::new(),
            after: after.iter().map(ToString::to_string).collect(),
            before: Vec::new(),
        }
    }

    fn render(state: &BootState, now: Duration, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    BootView {
                        state,
                        palette: &Palette::default(),
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

    fn running_since(since: Duration) -> Activity {
        Activity {
            state: Some(ActiveState::Activating),
            inactive_exit: Some(since),
            active_enter: None,
            active_exit: None,
        }
    }

    fn boot_in_progress() -> BootState {
        let mut state = BootState::new(Phase::Host);
        state.set_root("graphical.target".to_owned());
        [
            (
                facts("graphical.target", &["multi-user.target"]),
                Activity::default(),
            ),
            (
                facts(
                    "multi-user.target",
                    &["docker.service", "systemd-rfkill.service", "web.service"],
                ),
                Activity::default(),
            ),
            (facts("docker.service", &[]), running_since(9000 * MS)),
            (
                facts("systemd-rfkill.service", &[]),
                running_since(6000 * MS),
            ),
            (facts("web.service", &[]), Activity::default()),
            (facts("sysinit.target", &[]), Activity::default()),
            (facts("nvidia-powerd.service", &[]), Activity::default()),
            (facts("udev-settle.service", &[]), running_since(2000 * MS)),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((unit, activity), id)| {
            let name = unit.name.clone();
            state.track_unit(unit, activity);
            state.queue_job(id, &name, JobKind::BringsUp, 1000 * MS);
        });
        state.finish_job(6, JobResult::Done, 4000 * MS);
        state.finish_job(7, JobResult::Failed, 5000 * MS);
        state.finish_job(8, JobResult::Done, 3500 * MS);
        state
    }

    #[test]
    fn tree_on_top_timeline_below() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 110, 24);
        let screen = lines.join("\n");
        assert_eq!(lines[0], "bootvine  ryzen", "{screen}");
        assert!(lines[2].starts_with("── stage 2 ──"), "{screen}");
        assert!(lines[2].ends_with("12.4s ──"), "{screen}");
        assert!(lines[3].starts_with("graphical.target"), "{screen}");
        assert!(lines[4].starts_with("└─ multi-user.target"), "{screen}");
        assert!(lines[5].starts_with("   ├─ [/] systemd-rfkill"), "{screen}");
        assert!(lines[6].starts_with("   └─ [/] docker"), "{screen}");
        assert!(lines[7].starts_with("… 1 more"), "{screen}");
        assert!(
            lines[8].starts_with("× nvidia-powerd.service failed"),
            "{screen}"
        );
        let timeline = lines
            .iter()
            .position(|line| line.contains(" timeline "))
            .expect("timeline panel");
        assert!(
            lines[timeline + 1].starts_with(&format!("{:29}kernel", "")),
            "{screen}"
        );
        assert!(
            lines[timeline + 3].starts_with("√ sysinit.target (2 units)"),
            "{screen}"
        );
        assert!(lines[timeline + 4].starts_with("√ udev-settle"), "{screen}");
        assert!(lines[timeline + 4].contains('■'), "{screen}");
        assert!(
            lines[timeline + 5].starts_with("► systemd-rfkill"),
            "{screen}"
        );
        assert!(
            lines[timeline + 5].contains("·■") && lines[timeline + 5].ends_with('▶'),
            "{screen}"
        );
        assert!(lines[timeline + 6].starts_with("► docker"), "{screen}");
        assert!(
            lines[timeline..].iter().all(|line| !line.contains(" web")),
            "{screen}"
        );
        assert!(!screen.contains("root switch"), "{screen}");
        assert!(
            lines[23].starts_with("√ 2 done   2 running   3 waiting   × 1 failed"),
            "{screen}"
        );
    }

    #[test]
    fn narrow_screens_drop_the_timeline() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 36, 24);
        assert!(
            lines.iter().all(|line| !line.contains('■')),
            "{}",
            lines.join("\n")
        );
        assert!(lines[5].contains("systemd-rfkill"));
    }

    #[test]
    fn finished_stages_collapse_into_a_summary_rule() {
        let mut state = BootState::new(Phase::Initrd);
        state.set_root("initrd.target".to_owned());
        state.track_unit(facts("initrd.target", &[]), Activity::default());
        state.queue_job(1, "initrd.target", JobKind::BringsUp, Duration::ZERO);
        state.finish_job(1, JobResult::Done, 6000 * MS);
        state.switch_root(7200 * MS);
        state.set_root("graphical.target".to_owned());
        state.track_unit(facts("graphical.target", &[]), Activity::default());
        state.queue_job(2, "graphical.target", JobKind::BringsUp, 7300 * MS);
        let lines = render(&state, 17_300 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(lines[0].ends_with("17.3s"), "{screen}");
        assert!(
            lines[2].starts_with("── initrd ── ■ done in 7.2s · 1 unit ──"),
            "{screen}"
        );
        assert!(lines[3].starts_with("── stage 2 ──"), "{screen}");
        assert!(lines[3].ends_with("10.1s ──"), "{screen}");
        assert!(lines[4].starts_with("graphical.target"), "{screen}");
        let timeline = lines
            .iter()
            .position(|line| line.contains(" timeline "))
            .expect("timeline panel");
        let marker_column = |line: &str| line.chars().position(|symbol| symbol == '│');
        let switch_column = marker_column(&lines[timeline + 2]).expect("root switch marker");
        assert!(
            lines[timeline + 3].starts_with("√ initrd (1 unit)"),
            "{screen}"
        );
        assert_eq!(
            marker_column(&lines[timeline + 3]),
            Some(switch_column),
            "{screen}"
        );
        assert!(screen.contains("│ root switch"), "{screen}");
    }

    #[test]
    fn completion_announces_the_handover() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("graphical.target".to_owned());
        state.track_unit(facts("graphical.target", &[]), Activity::default());
        state.queue_job(1, "graphical.target", JobKind::BringsUp, Duration::ZERO);
        state.finish_job(1, JobResult::Done, 9000 * MS);
        let lines = render(&state, 9500 * MS, 110, 24);
        assert!(
            lines[2].starts_with(
                "── ■ boot finished in 9.5s · handing over to display-manager.service ──"
            ),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn shutdown_in_progress_names_its_stage() {
        let mut state = BootState::new(Phase::Shutdown);
        state.origin = 20_000 * MS;
        state.set_root("poweroff.target".to_owned());
        state.track_unit(facts("poweroff.target", &[]), Activity::default());
        state.queue_job(1, "poweroff.target", JobKind::BringsUp, 20_000 * MS);
        let lines = render(&state, 21_500 * MS, 110, 24);
        assert!(
            lines[2].starts_with("── shutdown ─") && lines[2].ends_with(" 1.5s ──"),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn shutdown_announces_the_handover_once_the_manager_exits() {
        let mut state = BootState::new(Phase::Shutdown);
        state.origin = 20_000 * MS;
        state.manager_exited_at = Some(24_200 * MS);
        let lines = render(&state, 30_000 * MS, 110, 24);
        assert!(
            lines[2].starts_with(
                "── ■ shutdown finished in 4.2s · handing over to systemd-shutdown ──"
            ),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn a_long_tree_keeps_the_room_and_leaves_the_timeline_a_strip() {
        let heights = Heights::share(30, 40, 50);
        assert_eq!(heights.timeline, MIN_TIMELINE_HEIGHT);
        assert_eq!(heights.tree, 30 - MIN_TIMELINE_HEIGHT);
        let heights = Heights::share(30, 12, 50);
        assert_eq!((heights.tree, heights.timeline), (12, 18));
    }

    #[test]
    fn overflowing_trees_are_cut_with_a_count() {
        let mut state = BootState::new(Phase::Host);
        (0..30).for_each(|id| {
            let name = format!("unit{id:02}.service");
            state.track_unit(facts(&name, &[]), Activity::default());
            state.queue_job(id, &name, JobKind::BringsUp, Duration::ZERO);
        });
        let lines = render(&state, 1000 * MS, 110, 20);
        assert!(
            lines.iter().any(|line| line.starts_with("… ")),
            "{}",
            lines.join("\n")
        );
    }
}
