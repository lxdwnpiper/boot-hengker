mod chrome;
mod stages;
mod timeline;
mod tree;

use std::time::Duration;

use jiff::fmt::friendly::{Designator, FractionalUnit, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::Line,
    widgets::{Block, Borders, Widget},
};
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

pub use self::chrome::PasswordPrompt;
use self::{
    chrome::{FOOTER_HEIGHT, Footer, Header, PasswordPopup},
    stages::Stages,
    timeline::{RunningRun, Timeline},
    tree::UnitTree,
};
use crate::{
    colors::Colors,
    config::Timing,
    glyphs::Glyphs,
    model::{BootState, RowStatus, TreeRow, UnitKind, fit_to_height, fold_chains},
    origin::BootOrigin,
};

const DURATION_WIDTH: u16 = 8;
const MIN_BAR_WIDTH: u16 = 12;
const MIN_HEIGHT_FOR_TIMELINE: u16 = 12;
const MIN_TIMELINE_HEIGHT: u16 = timeline::CHROME_HEIGHT + 4;
const HEADER_HEIGHT: u16 = 1;
const BRANCH: &str = "├─ ";
const LAST_BRANCH: &str = "└─ ";
const PASSING_TRUNK: &str = "│  ";
const ENDED_TRUNK: &str = "   ";

const ELAPSED: SpanPrinter = SpanPrinter::new()
    .designator(Designator::Compact)
    .spacing(Spacing::None)
    .fractional(Some(FractionalUnit::Second))
    .precision(Some(1));

pub struct BootView<'a> {
    pub state: &'a BootState,
    pub colors: &'a Colors,
    pub timing: &'a Timing,
    pub glyphs: &'a Glyphs,
    pub now: Duration,
    pub origin: &'a BootOrigin,
    pub frame: u64,
    pub password_prompt: Option<PasswordPrompt<'a>>,
}

impl BootView<'_> {
    fn timeline<'a>(&'a self, rows: &'a [TreeRow]) -> Timeline<'a> {
        let mut running: Vec<RunningRun> = rows
            .iter()
            .filter(|row| row.kind != UnitKind::Target)
            .filter_map(|row| match row.status {
                RowStatus::Running { queued_at, since } => Some(RunningRun {
                    label: &row.label,
                    queued_at,
                    since,
                }),
                _ => None,
            })
            .collect();
        running.sort_by_key(|run| run.since);
        Timeline {
            origin: self.state.origin,
            now: self.now,
            root_switches: self
                .state
                .finished_stages
                .iter()
                .map(|stage| stage.reached_at)
                .collect(),
            milestones: self.state.milestones(),
            finished: self
                .state
                .finished_runs
                .iter()
                .filter(|run| {
                    run.finished_at.saturating_sub(run.started_at)
                        >= self.timing.hide_runs_shorter_than
                })
                .collect(),
            running,
            colors: &self.colors.timeline,
            glyphs: self.glyphs,
        }
    }
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let all_rows = self
            .state
            .tree_rows(self.now.saturating_sub(self.timing.keep_finished_units_for));
        let pending_count = all_rows.len();
        let header = Header {
            origin: self.origin,
            total: (!self.state.finished_stages.is_empty())
                .then(|| self.now.saturating_sub(self.state.origin)),
            colors: &self.colors.header,
        };
        let stages = Stages {
            state: self.state,
            now: self.now,
            colors: &self.colors.stages,
            glyphs: self.glyphs,
        };
        let timeline = self.timeline(&all_rows);
        let footer = Footer {
            tally: self.state.tally(),
            colors: &self.colors.footer,
            glyphs: self.glyphs,
        };
        let fits_timeline = area.width >= timeline::LABEL_WIDTH + 1 + MIN_BAR_WIDTH
            && area.height >= MIN_HEIGHT_FOR_TIMELINE;
        let free = area
            .height
            .saturating_sub(HEADER_HEIGHT + stages.height() + FOOTER_HEIGHT);
        let heights = Heights::share(
            free,
            UnitTree::height_of(pending_count, 0),
            if fits_timeline { timeline.height() } else { 0 },
        );
        let budget = usize::from(heights.tree.saturating_sub(1));
        let rows = [all_rows.clone(), fold_chains(all_rows.clone())]
            .into_iter()
            .find(|rows| rows.len() <= budget)
            .unwrap_or_else(|| fold_chains(fit_to_height(all_rows.clone(), budget)));
        let tree = UnitTree {
            phase: stages.current_phase(),
            rows: &rows,
            hidden: pending_count - shown_units(&rows),
            now: self.now,
            hide_runs_shorter_than: self.timing.hide_runs_shorter_than,
            frame: self.frame,
            colors: &self.colors.tree,
            glyphs: self.glyphs,
        };
        let tree_height = tree.height().min(heights.tree);

        let [
            header_area,
            stage_area,
            tree_area,
            _,
            timeline_area,
            footer_area,
        ] = Layout::vertical([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Length(stages.height()),
            Constraint::Length(tree_height),
            Constraint::Fill(1),
            Constraint::Length(heights.timeline),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .areas(area);

        header.render(header_area, buf);
        stages.render(stage_area, buf);
        tree.render(tree_area, buf);
        if heights.timeline > 0 {
            timeline.render(timeline_area, buf);
        }
        footer.render(footer_area, buf);
        if let Some(asking) = &self.password_prompt {
            PasswordPopup {
                asking,
                colors: &self.colors.password,
                glyphs: self.glyphs,
            }
            .render(area, buf);
        }
    }
}

struct Heights {
    tree: u16,
    timeline: u16,
}

impl Heights {
    fn share(free: u16, tree_need: u16, timeline_need: u16) -> Self {
        let leftover = free.saturating_sub(tree_need);
        let timeline = if leftover >= MIN_TIMELINE_HEIGHT {
            timeline_need.min(leftover)
        } else {
            0
        };
        Self {
            tree: free - timeline,
            timeline,
        }
    }
}

fn rule(color: Color, left: Line<'static>) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP)
        .border_style(color)
        .title(left)
}

fn shown_units(rows: &[TreeRow]) -> usize {
    rows.iter().map(|row| 1 + row.chain.len()).sum()
}

fn fit_width(text: &str, room: usize, ellipsis: &str) -> String {
    if text.width() <= room {
        return text.to_owned();
    }
    let (kept, _) = text.unicode_truncate(room.saturating_sub(ellipsis.width()));
    format!("{kept}{ellipsis}")
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
        screen_lines(&render_prompted(state, now, width, height, None))
    }

    fn render_prompted(
        state: &BootState,
        now: Duration,
        width: u16,
        height: u16,
        password_prompt: Option<PasswordPrompt>,
    ) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    BootView {
                        state,
                        colors: &Colors::default(),
                        timing: &Timing::default(),
                        glyphs: &Glyphs::default(),
                        now,
                        origin: &BootOrigin {
                            host: "ryzen".to_owned(),
                            system: "NixOS 26.11".to_owned(),
                            generation: "2771".to_owned(),
                            kernel: "Linux 7.2.9".to_owned(),
                        },
                        frame: 1,
                        password_prompt,
                    },
                    frame.area(),
                );
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn screen_lines(buffer: &Buffer) -> Vec<String> {
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                let line: String = (0..area.width)
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
        assert_eq!(
            lines[0], "ryzen · NixOS 26.11 · generation 2771 · Linux 7.2.9",
            "{screen}"
        );
        assert!(lines[1].starts_with("└─ Stage 2  12.4s"), "{screen}");
        assert!(lines[1].ends_with("Running"), "{screen}");
        assert!(
            lines[2].starts_with("   ├─ ◆ multi-user.target"),
            "{screen}"
        );
        assert!(
            lines[3].starts_with("   │  ├─ [/] systemd-rfkill"),
            "{screen}"
        );
        assert!(lines[4].starts_with("   │  ├─ [/] docker"), "{screen}");
        assert!(lines[5].starts_with("   │  └─ [ ] web"), "{screen}");
        assert!(lines[6].starts_with("   ├─ ◇ graphical.target"), "{screen}");
        assert!(lines[7].starts_with("   └─ [×] nvidia-powerd"), "{screen}");
        let timeline = lines
            .iter()
            .position(|line| line.contains(" Timeline "))
            .expect("timeline panel");
        assert!(
            lines[timeline + 1].starts_with(&format!("{:29}Kernel", "")),
            "{screen}"
        );
        assert!(
            lines[timeline + 2].starts_with("√ sysinit.target (2 units)"),
            "{screen}"
        );
        assert!(lines[timeline + 3].starts_with("√ udev-settle"), "{screen}");
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
    fn failures_stay_in_the_stage_that_wanted_them() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("multi-user.target".to_owned());
        [
            facts("multi-user.target", &["basic.target", "web.service"]),
            facts("basic.target", &["flaky.service", "quick.service"]),
            facts("flaky.service", &[]),
            facts("quick.service", &[]),
            facts("web.service", &[]),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|(unit, id)| {
            let name = unit.name.clone();
            state.track_unit(unit, Activity::default());
            state.queue_job(id, &name, JobKind::BringsUp, Duration::ZERO);
        });
        state.finish_job(3, JobResult::Failed, 1000 * MS);
        state.finish_job(4, JobResult::Done, 1000 * MS);
        state.finish_job(2, JobResult::Done, 1500 * MS);
        let lines = render(&state, 5000 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(lines[2].starts_with("   ├─ √ basic.target"), "{screen}");
        assert!(lines[3].starts_with("   │  └─ [×] flaky"), "{screen}");
        assert!(
            lines[4].starts_with("   └─ ◆ multi-user.target"),
            "{screen}"
        );
        assert!(lines[5].starts_with("      └─ [ ] web"), "{screen}");
        assert!(!screen.contains("quick"), "{screen}");
    }

    #[test]
    fn finished_units_linger_in_place_before_leaving() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("multi-user.target".to_owned());
        [
            facts("multi-user.target", &["early.service", "late.service"]),
            facts("early.service", &[]),
            facts("late.service", &[]),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|(unit, id)| {
            let name = unit.name.clone();
            let activity = if name.ends_with(".service") {
                running_since(Duration::ZERO)
            } else {
                Activity::default()
            };
            state.track_unit(unit, activity);
            state.queue_job(id, &name, JobKind::BringsUp, Duration::ZERO);
        });
        state.finish_job(2, JobResult::Done, 3000 * MS);
        let lingering = render(&state, 4000 * MS, 110, 24);
        let screen = lingering.join("\n");
        assert!(lingering[3].starts_with("      ├─ [√] early"), "{screen}");
        assert!(lingering[4].starts_with("      └─ [/] late"), "{screen}");
        let gone = render(&state, 5500 * MS, 110, 24);
        let tree: Vec<&String> = gone
            .iter()
            .take_while(|line| !line.contains(" Timeline "))
            .collect();
        assert!(
            tree.iter().all(|line| !line.contains("early")),
            "{}",
            gone.join("\n")
        );
    }

    #[test]
    fn only_running_units_carry_a_timer() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(lines[2].ends_with("multi-user.target"), "{screen}");
        assert!(lines[3].ends_with("6.4s"), "{screen}");
        assert!(lines[5].ends_with("web"), "{screen}");
    }

    const CRYPTSETUP_MESSAGE: &str =
        "Please enter passphrase for disk Samsung SSD 980 PRO 1TB (cryptroot):";

    #[test]
    fn password_prompt_floats_over_a_dimmed_view() {
        let buffer = render_prompted(
            &boot_in_progress(),
            12_400 * MS,
            110,
            24,
            Some(PasswordPrompt {
                message: CRYPTSETUP_MESSAGE,
                input: "***".to_owned(),
                sent: false,
            }),
        );
        let lines = screen_lines(&buffer);
        let screen = lines.join("\n");
        let title_row = lines
            .iter()
            .position(|line| line.contains("┌ Password "))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(lines[title_row].starts_with(' '), "{screen}");
        assert!(
            lines[title_row + 1].contains(&format!("│  {CRYPTSETUP_MESSAGE}")),
            "{screen}"
        );
        assert!(lines[title_row + 3].contains("│  > ***_"), "{screen}");
        assert!(lines[title_row + 4].contains("Enter unlock"), "{screen}");
        assert!(
            lines[23].starts_with("√ 2 done   2 running   3 waiting   × 1 failed"),
            "{screen}"
        );
        assert_eq!(
            buffer[(0, 0)].fg,
            Colors::default().password.backdrop,
            "{screen}"
        );
        let corner = buffer
            .content
            .iter()
            .find(|cell| cell.symbol() == "┌")
            .unwrap_or_else(|| panic!("{screen}"));
        assert_eq!(corner.fg, Colors::default().password.frame, "{screen}");
    }

    #[test]
    fn answered_password_prompt_shows_it_is_checking() {
        let lines = screen_lines(&render_prompted(
            &boot_in_progress(),
            12_400 * MS,
            110,
            24,
            Some(PasswordPrompt {
                message: CRYPTSETUP_MESSAGE,
                input: String::new(),
                sent: true,
            }),
        ));
        assert!(
            lines.iter().any(|line| line.contains("│  Checking…")),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn long_password_messages_wrap_inside_narrow_screens() {
        let lines = screen_lines(&render_prompted(
            &boot_in_progress(),
            12_400 * MS,
            50,
            24,
            Some(PasswordPrompt {
                message: CRYPTSETUP_MESSAGE,
                input: String::new(),
                sent: false,
            }),
        ));
        let screen = lines.join("\n");
        assert!(
            lines.iter().any(|line| line.contains("(cryptroot):")),
            "{screen}"
        );
        assert!(lines.iter().any(|line| line.contains("> _")), "{screen}");
    }

    #[test]
    fn sub_second_runs_stay_off_the_timeline() {
        let mut state = boot_in_progress();
        state.track_unit(facts("blink.service", &[]), running_since(4000 * MS));
        state.queue_job(20, "blink.service", JobKind::BringsUp, 4000 * MS);
        state.finish_job(20, JobResult::Done, 4400 * MS);
        let lines = render(&state, 12_400 * MS, 110, 24);
        assert!(
            lines.iter().all(|line| !line.contains("blink")),
            "{}",
            lines.join("\n")
        );
    }

    fn chain_boot() -> BootState {
        let mut state = BootState::new(Phase::Host);
        state.set_root("graphical.target".to_owned());
        [
            ("graphical.target", "network-wait.service"),
            ("network-wait.service", "network.target"),
            ("network.target", "network-manager.service"),
            ("network-manager.service", "dbus.service"),
            ("dbus.service", "udev-settle.service"),
            ("udev-settle.service", ""),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((name, waits_on), id)| {
            let after: Vec<&str> = [waits_on].into_iter().filter(|w| !w.is_empty()).collect();
            let activity = if name == "udev-settle.service" {
                running_since(1000 * MS)
            } else {
                Activity::default()
            };
            state.track_unit(facts(name, &after), activity);
            state.queue_job(id, name, JobKind::BringsUp, Duration::ZERO);
        });
        state
    }

    #[test]
    fn chains_stay_unfolded_while_they_fit() {
        let lines = render(&chain_boot(), 5000 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(
            lines[3].starts_with("      └─ [ ] network-wait"),
            "{screen}"
        );
        assert!(
            lines[4].starts_with("         └─ network.target"),
            "{screen}"
        );
        assert!(
            lines[7].starts_with("                  └─ [/] udev-settle"),
            "{screen}"
        );
    }

    #[test]
    fn chains_fold_when_short_and_shorten_to_their_ends_when_narrow() {
        let short = render(&chain_boot(), 5000 * MS, 110, 9);
        assert!(
            short[3]
                .starts_with("      └─ [ ] network-wait ─ network.target ─ network-manager ─ dbus"),
            "{}",
            short.join("\n")
        );
        assert!(
            short[4].starts_with("         └─ [/] udev-settle"),
            "{}",
            short.join("\n")
        );
        let narrow = render(&chain_boot(), 5000 * MS, 45, 8);
        assert!(
            narrow[3].starts_with("      └─ [ ] network-wait ─ … ─ dbus"),
            "{}",
            narrow.join("\n")
        );
    }

    fn described_boot() -> BootState {
        let mut state = BootState::new(Phase::Host);
        state.set_root("multi-user.target".to_owned());
        [
            (
                "multi-user.target",
                "Multi-User System",
                &["docker.service", "systemd-rfkill.service"][..],
            ),
            ("docker.service", "Docker Application Container Engine", &[]),
            (
                "systemd-rfkill.service",
                "Load/Save RF Kill Switch Status",
                &[],
            ),
        ]
        .into_iter()
        .zip(1..)
        .for_each(|((name, description, after), id)| {
            state.track_unit(
                UnitFacts {
                    description: description.to_owned(),
                    ..facts(name, after)
                },
                running_since(1000 * MS),
            );
            state.queue_job(id, name, JobKind::BringsUp, Duration::ZERO);
        });
        state
    }

    #[test]
    fn descriptions_line_up_in_a_column() {
        let lines = render(&described_boot(), 3000 * MS, 110, 24);
        let screen = lines.join("\n");
        let column_of = |text: &str| {
            lines
                .iter()
                .find_map(|line| line.find(text).map(|at| line[..at].width()))
                .unwrap_or_else(|| panic!("{text} missing\n{screen}"))
        };
        assert_eq!(column_of("Multi-User System"), column_of("Description"));
        assert_eq!(
            column_of("Multi-User System"),
            column_of("Docker Application")
        );
        assert_eq!(
            column_of("Multi-User System"),
            column_of("Load/Save RF Kill")
        );
        assert!(column_of("Multi-User System") > column_of("systemd-rfkill"));
    }

    #[test]
    fn non_service_units_carry_their_kind() {
        let mut state = described_boot();
        state.track_unit(
            UnitFacts {
                description: "Docker Socket for the API".to_owned(),
                ..facts("docker.socket", &[])
            },
            running_since(1000 * MS),
        );
        state.queue_job(9, "docker.socket", JobKind::BringsUp, Duration::ZERO);
        let lines = render(&state, 3000 * MS, 110, 24);
        let screen = lines.join("\n");
        let socket_row = lines
            .iter()
            .find(|line| line.contains("Docker Socket"))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(socket_row.contains("[/] docker "), "{screen}");
        assert!(socket_row.contains(" Socket "), "{screen}");
        assert!(
            render(&described_boot(), 3000 * MS, 110, 24)
                .iter()
                .all(|line| !line.contains(" Socket ")),
            "{screen}"
        );
    }

    #[test]
    fn narrow_screens_keep_names_and_drop_descriptions() {
        let lines = render(&described_boot(), 3000 * MS, 40, 24);
        let screen = lines.join("\n");
        assert!(screen.contains("systemd-rfkill"), "{screen}");
        assert!(!screen.contains("Docker"), "{screen}");
    }

    #[test]
    fn long_timeline_labels_end_in_an_ellipsis() {
        let mut state = boot_in_progress();
        state.track_unit(
            UnitFacts {
                description: "Kernel Configuration File System".to_owned(),
                ..facts("sys-kernel-config.mount", &[])
            },
            running_since(2000 * MS),
        );
        state.queue_job(30, "sys-kernel-config.mount", JobKind::BringsUp, 2000 * MS);
        state.finish_job(30, JobResult::Done, 4000 * MS);
        let lines = render(&state, 12_400 * MS, 110, 24);
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("√ Kernel Configuration File…")),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn narrow_screens_drop_the_timeline() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 40, 24);
        assert!(
            lines.iter().all(|line| !line.contains('■')),
            "{}",
            lines.join("\n")
        );
        assert!(lines[3].contains("systemd-rfkill"), "{}", lines.join("\n"));
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
            lines[1].starts_with("├─ Initrd   ■ done in 7.2s · 1 unit"),
            "{screen}"
        );
        assert!(lines[2].starts_with("└─ Stage 2  10.1s"), "{screen}");
        assert!(lines[3].starts_with("   └─ ◆ graphical.target"), "{screen}");
        let timeline = lines
            .iter()
            .position(|line| line.contains(" Timeline "))
            .expect("timeline panel");
        let initrd_bar = &lines[timeline + 2];
        assert!(initrd_bar.starts_with("√ Initrd (1 unit)"), "{screen}");
        assert!(initrd_bar.ends_with("■│"), "{screen}");
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
            lines[1].starts_with(
                "└─ ■ Boot finished in 9.5s · handing over to display-manager.service"
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
            lines[1].starts_with("└─ Shutdown 1.5s") && lines[1].ends_with("Running"),
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
            lines[1]
                .starts_with("└─ ■ Shutdown finished in 4.2s · handing over to systemd-shutdown"),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn the_tree_takes_the_room_and_the_timeline_only_what_is_left() {
        let heights = Heights::share(30, 40, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 50);
        assert_eq!((heights.tree, heights.timeline), (12, 18));
        let heights = Heights::share(30, 25, 50);
        assert_eq!((heights.tree, heights.timeline), (30, 0));
        let heights = Heights::share(30, 12, 8);
        assert_eq!((heights.tree, heights.timeline), (22, 8));
    }

    #[test]
    fn the_timeline_keeps_only_the_slowest_finished_runs() {
        let mut state = BootState::new(Phase::Host);
        state.set_root("multi-user.target".to_owned());
        let quick_units: Vec<String> = (0..10).map(|n| format!("run{n:02}.service")).collect();
        let after: Vec<&str> = quick_units
            .iter()
            .map(String::as_str)
            .chain(["slow.service"])
            .collect();
        state.track_unit(facts("multi-user.target", &after), Activity::default());
        state.queue_job(100, "multi-user.target", JobKind::BringsUp, Duration::ZERO);
        state.track_unit(facts("slow.service", &[]), running_since(Duration::ZERO));
        state.queue_job(101, "slow.service", JobKind::BringsUp, Duration::ZERO);
        quick_units.iter().zip(0..).for_each(|(name, id)| {
            state.track_unit(facts(name, &[]), running_since(Duration::ZERO));
            state.queue_job(id, name, JobKind::BringsUp, Duration::ZERO);
            state.finish_job(id, JobResult::Done, (1000 + 1000 * id) * MS);
        });
        let lines = render(&state, 20_000 * MS, 110, 40);
        let screen = lines.join("\n");
        let timeline = lines
            .iter()
            .position(|line| line.contains(" Timeline "))
            .expect("timeline panel");
        let (tree, panel) = lines.split_at(timeline);
        assert!(tree.iter().all(|line| !line.contains('…')), "{screen}");
        let shown: Vec<&str> = quick_units
            .iter()
            .map(|name| name.trim_end_matches(".service"))
            .filter(|label| panel.iter().any(|line| line.contains(label)))
            .collect();
        assert_eq!(
            shown,
            ["run05", "run06", "run07", "run08", "run09"],
            "{screen}"
        );
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
            lines.iter().any(|line| line.starts_with("   … ")),
            "{}",
            lines.join("\n")
        );
    }
}
