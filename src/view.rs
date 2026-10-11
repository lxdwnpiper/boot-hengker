use std::{
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

use crate::model::{BootState, MilestoneRow, PendingRow, RowStatus, Tally};

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

const DURATION_WIDTH: u16 = 8;
const TIMELINE_LABEL_WIDTH: u16 = 28;
const MIN_BAR_WIDTH: u16 = 12;
const MIN_HEIGHT_FOR_TIMELINE: u16 = 12;
const TIMELINE_CHROME_HEIGHT: u16 = 3;
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
    pub now: Duration,
    pub host: &'a str,
    pub frame: u64,
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = self.state.pending_rows();
        let milestones: Vec<MilestoneRow> = self
            .state
            .milestones()
            .into_iter()
            .filter(|row| row.reached_at.is_some())
            .collect();
        let mut units: Vec<&PendingRow> = rows.iter().filter(|row| !row.is_target).collect();
        units.sort_by_key(|row| row.running_since.unwrap_or(Duration::MAX));
        let failures = failure_lines(&self.state.failed);
        let stages = self.stage_lines(area.width);
        let stages_height = u16::try_from(stages.len()).unwrap_or(u16::MAX);
        let failures_height = u16::try_from(failures.len()).unwrap_or(u16::MAX);
        let heights = Heights::share(
            area.height
                .saturating_sub(HEADER_HEIGHT + stages_height + failures_height + FOOTER_HEIGHT),
            u16::try_from(rows.len()).unwrap_or(u16::MAX),
            timeline_need(area, milestones.len() + units.len()),
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
        self.tree(&rows, tree.height).render(tree, buf);
        Paragraph::new(failures).render(failed, buf);
        if heights.timeline > 0 {
            self.timeline(&milestones, &units, timeline.width)
                .render(timeline, buf);
        }
        footer_paragraph(
            self.state.tally(),
            footer.width,
            !self.state.finished_stages.is_empty(),
        )
        .render(footer, buf);
    }
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
        let timeline = timeline_need.min((free / 2).max(free.saturating_sub(tree_need)));
        Self {
            tree: free - timeline,
            timeline,
        }
    }
}

fn rule_line(width: u16, left: Vec<Span<'static>>, right: Vec<Span<'static>>) -> Line<'static> {
    let used = Line::from(left.clone()).width() + Line::from(right.clone()).width();
    let fill: String = repeat_n(RULE, usize::from(width).saturating_sub(used)).collect();
    Line::from(
        left.into_iter()
            .chain(once(Span::from(fill).dark_gray()))
            .chain(right)
            .collect::<Vec<_>>(),
    )
}

impl BootView<'_> {
    fn header(&self) -> Table<'static> {
        let elapsed =
            ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(self.state.origin));
        Table::new(
            [Row::new([
                Line::from(format!("boot-tree  {}", self.host)),
                Line::from(elapsed).right_aligned(),
            ])],
            [Constraint::Fill(1), Constraint::Length(DURATION_WIDTH)],
        )
        .bold()
    }

    fn stage_lines(&self, width: u16) -> Vec<Line<'static>> {
        let finished = self.state.finished_stages.iter().map(|stage| {
            rule_line(
                width,
                vec![
                    Span::from(format!("{RULE_STUB} {} {RULE_STUB} ", stage.label)).dark_gray(),
                    Span::from(format!(
                        "{BEAD} done in {}",
                        ELAPSED.unsigned_duration_to_string(
                            &stage.reached_at.saturating_sub(stage.started_at)
                        )
                    ))
                    .green(),
                    Span::from(format!(" · {} ", unit_count(stage.finished_units))).dark_gray(),
                ],
                Vec::new(),
            )
        });
        let current_started_at = self
            .state
            .finished_stages
            .last()
            .map_or(self.state.origin, |stage| stage.reached_at);
        let current = rule_line(
            width,
            vec![
                Span::from(format!("{RULE_STUB} ")).dark_gray(),
                Span::from(self.state.phase.label()).bold(),
                Span::raw(" "),
            ],
            vec![
                Span::raw(" "),
                Span::from(
                    ELAPSED
                        .unsigned_duration_to_string(&self.now.saturating_sub(current_started_at)),
                )
                .bold(),
                Span::from(format!(" {RULE_STUB}")).dark_gray(),
            ],
        );
        finished.chain(once(current)).collect()
    }

    fn tree<'a>(&self, rows: &'a [PendingRow], height: u16) -> Table<'a> {
        let capacity = usize::from(height);
        let shown = if rows.len() > capacity {
            capacity.saturating_sub(1)
        } else {
            rows.len()
        };
        let hidden = rows.len() - shown;
        let overflow =
            (hidden > 0).then(|| Row::new([Line::from(format!("… {hidden} more")).dark_gray()]));
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
                Span::from(tree_prefix(row)).dark_gray(),
                status_marker(row, self.frame),
                Span::styled(row.label.as_str(), label_style(row)),
            ]),
            Line::from(ELAPSED.unsigned_duration_to_string(&self.now.saturating_sub(since)))
                .style(label_style(row))
                .right_aligned(),
        ])
    }

    fn timeline<'a>(
        &self,
        milestones: &'a [MilestoneRow],
        units: &[&'a PendingRow],
        width: u16,
    ) -> Table<'a> {
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
        let axis_rows = [
            Row::new([Line::default(), axis.labels()]),
            Row::new([Line::default(), axis.ticks_line(&root_switches)]),
        ];
        let milestone_rows = milestones.iter().map(|row| {
            Row::new([
                Line::from(format!(
                    "{DONE} {} ({})",
                    row.label,
                    unit_count(row.finished_units)
                ))
                .green(),
                axis.bar(&milestone_segments(row), None, &root_switches),
            ])
        });
        let unit_rows = units.iter().map(|row| {
            Row::new([
                Line::from(vec![
                    timeline_marker(row),
                    Span::styled(row.label.as_str(), label_style(row)),
                ]),
                axis.bar(
                    &unit_segments(row, self.now),
                    Some(self.now),
                    &root_switches,
                ),
            ])
        });
        Table::new(
            axis_rows.into_iter().chain(milestone_rows).chain(unit_rows),
            [
                Constraint::Length(TIMELINE_LABEL_WIDTH),
                Constraint::Fill(1),
            ],
        )
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().dark_gray())
                .title(Line::from(" timeline ").dark_gray()),
        )
    }
}

fn timeline_need(screen: Rect, entries: usize) -> u16 {
    let fits = screen.width >= TIMELINE_LABEL_WIDTH + 1 + MIN_BAR_WIDTH
        && screen.height >= MIN_HEIGHT_FOR_TIMELINE
        && entries > 0;
    if fits {
        TIMELINE_CHROME_HEIGHT.saturating_add(u16::try_from(entries).unwrap_or(u16::MAX))
    } else {
        0
    }
}

fn failure_lines(units: &[String]) -> Vec<Line<'static>> {
    let hidden = units.len().saturating_sub(MAX_FAILED_ROWS);
    units
        .iter()
        .take(MAX_FAILED_ROWS)
        .map(|unit| format!("{FAILED} {unit} failed"))
        .chain((hidden > 0).then(|| format!("{FAILED} {hidden} more failed")))
        .map(|text| Line::from(text).red().bold())
        .collect()
}

fn footer_paragraph(tally: Tally, width: u16, root_switched: bool) -> Paragraph<'static> {
    let counts = format!(
        "{DONE} {} done   {} running   {} waiting   {FAILED} {} failed",
        tally.done, tally.running, tally.waiting, tally.failed
    );
    let switch_legend = if root_switched {
        format!("  {ROOT_SWITCH_MARK} root switch")
    } else {
        String::new()
    };
    let legend = format!("{BEAD} running  {WAITING_BEAD} waiting{switch_legend}");
    let gap = usize::from(width).saturating_sub(counts.chars().count() + legend.chars().count());
    let counts_style = if tally.failed > 0 {
        Style::new().red().bold()
    } else {
        Style::new()
    };
    let line = if gap >= 2 {
        Line::from(vec![
            Span::styled(counts, counts_style),
            Span::raw(" ".repeat(gap)),
            Span::from(legend).dark_gray(),
        ])
    } else {
        Line::from(Span::styled(counts, counts_style))
    };
    Paragraph::new(line).block(
        Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().dark_gray()),
    )
}

struct Segment {
    from: Duration,
    to: Duration,
    symbol: char,
    style: Style,
}

fn milestone_segments(row: &MilestoneRow) -> Vec<Segment> {
    row.reached_at
        .map(|reached_at| Segment {
            from: row.started_at,
            to: reached_at,
            symbol: BEAD,
            style: Style::new().green(),
        })
        .into_iter()
        .collect()
}

fn unit_segments(row: &PendingRow, now: Duration) -> Vec<Segment> {
    let waiting_until = row.running_since.unwrap_or(now);
    let waiting = (waiting_until > row.queued_at).then(|| Segment {
        from: row.queued_at,
        to: waiting_until,
        symbol: WAITING_BEAD,
        style: Style::new().dark_gray(),
    });
    let running = row.running_since.map(|since| Segment {
        from: since,
        to: now,
        symbol: BEAD,
        style: Style::new().yellow(),
    });
    waiting.into_iter().chain(running).collect()
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

    fn root_switch_strokes(&self, root_switches: &[Duration]) -> Vec<Stroke> {
        root_switches
            .iter()
            .map(|at| {
                Stroke::run(
                    self.column(*at),
                    1,
                    ROOT_SWITCH_MARK,
                    Style::new().dark_gray(),
                )
            })
            .collect()
    }

    fn labels(&self) -> Line<'static> {
        canvas_line(
            self.width,
            self.tick_offsets().map(|offset| {
                let glyphs = AXIS_LABEL.unsigned_duration_to_string(&offset);
                let label_width = u16::try_from(glyphs.len()).unwrap_or(self.width);
                Stroke {
                    column: self
                        .column(self.origin + offset)
                        .min(self.width.saturating_sub(label_width)),
                    glyphs,
                    style: Style::new().dark_gray(),
                }
            }),
        )
    }

    fn ticks_line(&self, root_switches: &[Duration]) -> Line<'static> {
        canvas_line(
            self.width,
            self.root_switch_strokes(root_switches)
                .into_iter()
                .chain(self.tick_offsets().map(|offset| {
                    Stroke::run(
                        self.column(self.origin + offset),
                        1,
                        '|',
                        Style::new().dark_gray(),
                    )
                })),
        )
    }

    fn bar(
        &self,
        segments: &[Segment],
        still_going_at: Option<Duration>,
        root_switches: &[Duration],
    ) -> Line<'static> {
        let bars = segments.iter().map(|segment| {
            let start = self.column(segment.from);
            let end = self.column(segment.to).max(start + 1);
            Stroke::run(start, end - start, segment.symbol, segment.style)
        });
        let still_going = still_going_at
            .map(|now| Stroke::run(self.column(now), 1, STILL_GOING, Style::new().dark_gray()));
        canvas_line(
            self.width,
            self.root_switch_strokes(root_switches)
                .into_iter()
                .chain(bars)
                .chain(still_going),
        )
    }
}

fn status_marker(row: &PendingRow, frame: u64) -> Span<'static> {
    match (row.is_target, row.status) {
        (true, _) => Span::raw(""),
        (false, RowStatus::Running) => {
            let spinner = SPINNER[usize::try_from(frame % 4).unwrap_or_default()];
            Span::from(format!("[{spinner}] ")).yellow()
        }
        (false, RowStatus::Waiting) => Span::from("[ ] ").dark_gray(),
    }
}

fn timeline_marker(row: &PendingRow) -> Span<'static> {
    match row.status {
        RowStatus::Running => Span::from(format!("{RUNNING_MARK} ")).yellow(),
        RowStatus::Waiting => Span::raw("  "),
    }
}

fn label_style(row: &PendingRow) -> Style {
    match (row.is_target, row.status) {
        (true, _) => Style::new().cyan(),
        (false, RowStatus::Running) => Style::new(),
        (false, RowStatus::Waiting) => Style::new().dark_gray(),
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
    use crate::model::{ActiveState, Activity, JobResult, Phase, UnitFacts};

    const MS: Duration = Duration::from_millis(1);

    fn facts(name: &str, after: &[&str]) -> UnitFacts {
        UnitFacts {
            name: name.to_owned(),
            description: String::new(),
            after: after.iter().map(ToString::to_string).collect(),
        }
    }

    fn render(state: &BootState, now: Duration, width: u16, height: u16) -> Vec<String> {
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

    fn running_since(since: Duration) -> Activity {
        Activity {
            state: Some(ActiveState::Activating),
            inactive_exit: Some(since),
            active_enter: None,
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
                    &["docker.service", "systemd-rfkill.service"],
                ),
                Activity::default(),
            ),
            (facts("docker.service", &[]), running_since(9000 * MS)),
            (
                facts("systemd-rfkill.service", &[]),
                running_since(6000 * MS),
            ),
            (facts("sysinit.target", &[]), Activity::default()),
            (facts("nvidia-powerd.service", &[]), Activity::default()),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((unit, activity), id)| {
            let name = unit.name.clone();
            state.track_unit(unit, activity);
            state.queue_job(id, &name, 1000 * MS);
        });
        state.finish_job(5, JobResult::Done, 4000 * MS);
        state.finish_job(6, JobResult::Failed, 5000 * MS);
        state
    }

    #[test]
    fn tree_on_top_timeline_below() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(lines[0].starts_with("boot-tree  ryzen"), "{screen}");
        assert!(lines[0].ends_with("12.4s"), "{screen}");
        assert!(lines[2].starts_with("── stage 2 ──"), "{screen}");
        assert!(lines[2].ends_with("12.4s ──"), "{screen}");
        assert!(lines[3].starts_with("graphical.target"), "{screen}");
        assert!(lines[4].starts_with("└─ multi-user.target"), "{screen}");
        assert!(lines[5].starts_with("   ├─ [/] systemd-rfkill"), "{screen}");
        assert!(lines[6].starts_with("   └─ [/] docker"), "{screen}");
        assert!(
            lines[7].starts_with("× nvidia-powerd.service failed"),
            "{screen}"
        );
        let timeline = lines
            .iter()
            .position(|line| line.contains(" timeline "))
            .expect("timeline panel");
        assert_eq!(timeline, 24 - 2 - 3 - 3, "{screen}");
        assert!(lines[timeline + 1].contains("0s"), "{screen}");
        assert!(
            lines[timeline + 3].starts_with("√ sysinit.target (1 unit)"),
            "{screen}"
        );
        assert!(lines[timeline + 3].contains('■'), "{screen}");
        assert!(
            lines[timeline + 4].starts_with("► systemd-rfkill"),
            "{screen}"
        );
        assert!(
            lines[timeline + 4].contains("·■") && lines[timeline + 4].ends_with('▶'),
            "{screen}"
        );
        assert!(lines[timeline + 5].starts_with("► docker"), "{screen}");
        assert!(!screen.contains("root switch"), "{screen}");
        assert!(
            lines[23].starts_with("√ 1 done   2 running   2 waiting   × 1 failed"),
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
        state.queue_job(1, "initrd.target", Duration::ZERO);
        state.finish_job(1, JobResult::Done, 6000 * MS);
        state.switch_root(7200 * MS);
        state.set_root("graphical.target".to_owned());
        state.track_unit(facts("graphical.target", &[]), Activity::default());
        state.queue_job(2, "graphical.target", 7300 * MS);
        let lines = render(&state, 17_300 * MS, 110, 24);
        let screen = lines.join("\n");
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
        assert!(lines[timeline + 3].contains("■│"), "{screen}");
        assert!(screen.contains("│ root switch"), "{screen}");
    }

    #[test]
    fn overflowing_trees_are_cut_with_a_count() {
        let mut state = BootState::new(Phase::Host);
        (0..30).for_each(|id| {
            let name = format!("unit{id:02}.service");
            state.track_unit(facts(&name, &[]), Activity::default());
            state.queue_job(id, &name, Duration::ZERO);
        });
        let lines = render(&state, 1000 * MS, 110, 20);
        assert!(
            lines.iter().any(|line| line.starts_with("… ")),
            "{}",
            lines.join("\n")
        );
    }
}
