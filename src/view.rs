mod footer;
mod header;
mod stages;
mod timeline;
mod tree;

use std::time::Duration;

use jiff::fmt::friendly::{Designator, FractionalUnit, Spacing, SpanPrinter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Widget},
};
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

use self::{footer::Footer, header::Header, stages::Stages, timeline::Timeline, tree::UnitTree};
use crate::{
    glyphs::Glyphs,
    model::{BootState, RowStatus, TreeRow, UnitKind, fit_to_height, fold_chains},
    origin::BootOrigin,
    palette::Palette,
};

const DURATION_WIDTH: u16 = 8;
const MIN_BAR_WIDTH: u16 = 12;
const MIN_HEIGHT_FOR_TIMELINE: u16 = 12;
const MIN_TIMELINE_HEIGHT: u16 = timeline::CHROME_HEIGHT + 4;
const NOTEWORTHY_DURATION: Duration = Duration::from_secs(1);
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
    pub palette: &'a Palette,
    pub glyphs: &'a Glyphs,
    pub now: Duration,
    pub origin: &'a BootOrigin,
    pub frame: u64,
}

impl BootView<'_> {
    fn timeline<'a>(&'a self, rows: &'a [TreeRow]) -> Timeline<'a> {
        let mut running: Vec<&TreeRow> = rows
            .iter()
            .filter(|row| row.kind != UnitKind::Target && row.status == RowStatus::Running)
            .collect();
        running.sort_by_key(|row| row.running_since);
        Timeline {
            origin: self.state.origin,
            now: self.now,
            root_switches: self
                .state
                .finished_stages
                .iter()
                .map(|stage| stage.reached_at)
                .collect(),
            milestones: self
                .state
                .milestones()
                .into_iter()
                .filter(|row| row.reached_at.is_some())
                .collect(),
            finished: self
                .state
                .finished_runs
                .iter()
                .filter(|run| run.finished_at.saturating_sub(run.started_at) >= NOTEWORTHY_DURATION)
                .collect(),
            running,
            palette: self.palette,
            glyphs: self.glyphs,
        }
    }
}

impl Widget for BootView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let all_rows = self.state.tree_rows();
        let pending_count = all_rows.len();
        let pruned_rows = fold_chains(fit_to_height(all_rows.clone(), 0));
        let header = Header {
            origin: self.origin,
            total: (!self.state.finished_stages.is_empty())
                .then(|| self.now.saturating_sub(self.state.origin)),
            palette: self.palette,
        };
        let stages = Stages {
            state: self.state,
            now: self.now,
            palette: self.palette,
            glyphs: self.glyphs,
        };
        let pruned = UnitTree {
            phase: stages.current_phase(),
            rows: &pruned_rows,
            hidden: pending_count - shown_units(&pruned_rows),
            now: self.now,
            frame: self.frame,
            palette: self.palette,
            glyphs: self.glyphs,
        };
        let timeline = self.timeline(&pruned_rows);
        let footer = Footer {
            tally: self.state.tally(),
            shows_root_switch: !self.state.finished_stages.is_empty(),
            palette: self.palette,
            glyphs: self.glyphs,
        };
        let fits_timeline = area.width >= timeline::LABEL_WIDTH + 1 + MIN_BAR_WIDTH
            && area.height >= MIN_HEIGHT_FOR_TIMELINE;
        let free = area
            .height
            .saturating_sub(HEADER_HEIGHT + stages.height() + footer::HEIGHT);
        let heights = Heights::share(
            free,
            pruned.height(),
            if fits_timeline { timeline.height() } else { 0 },
        );
        let budget = usize::from(heights.tree.saturating_sub(1));
        let whole_tree = fit_to_height(all_rows.clone(), usize::MAX);
        let rows = [whole_tree.clone(), fold_chains(whole_tree)]
            .into_iter()
            .find(|rows| rows.len() <= budget)
            .unwrap_or_else(|| fold_chains(fit_to_height(all_rows, budget)));
        let tree = UnitTree {
            rows: &rows,
            hidden: pending_count - shown_units(&rows),
            ..pruned
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
            Constraint::Length(footer::HEIGHT),
        ])
        .areas(area);

        header.render(header_area, buf);
        stages.render(stage_area, buf);
        tree.render(tree_area, buf);
        if heights.timeline > 0 {
            timeline.render(timeline_area, buf);
        }
        footer.render(footer_area, buf);
    }
}

struct Heights {
    tree: u16,
    timeline: u16,
}

impl Heights {
    fn share(free: u16, tree_need: u16, timeline_need: u16) -> Self {
        let timeline = timeline_need
            .min(free.saturating_sub(tree_need).max(MIN_TIMELINE_HEIGHT))
            .min(free);
        Self {
            tree: free - timeline,
            timeline,
        }
    }
}

fn rule(palette: &Palette, left: Line<'static>) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP)
        .border_style(palette.muted)
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
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    BootView {
                        state,
                        palette: &Palette::default(),
                        glyphs: &Glyphs::default(),
                        now,
                        origin: &BootOrigin {
                            host: "ryzen".to_owned(),
                            system: "NixOS 26.11".to_owned(),
                            generation: "2771".to_owned(),
                            kernel: "Linux 7.2.9".to_owned(),
                        },
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
        assert_eq!(
            lines[0], "ryzen · NixOS 26.11 · generation 2771 · Linux 7.2.9",
            "{screen}"
        );
        assert!(lines[1].starts_with("└─ stage 2  12.4s"), "{screen}");
        assert!(lines[1].ends_with("running"), "{screen}");
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
        let lines = render(&state, 3000 * MS, 110, 24);
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
    fn only_running_units_carry_a_timer() {
        let lines = render(&boot_in_progress(), 12_400 * MS, 110, 24);
        let screen = lines.join("\n");
        assert!(lines[2].ends_with("multi-user.target"), "{screen}");
        assert!(lines[3].ends_with("6.4s"), "{screen}");
        assert!(lines[5].ends_with("web"), "{screen}");
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
        let short = render(&chain_boot(), 5000 * MS, 110, 13);
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
        assert_eq!(column_of("Multi-User System"), column_of("description"));
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
        assert!(socket_row.contains(" socket "), "{screen}");
        assert!(
            render(&described_boot(), 3000 * MS, 110, 24)
                .iter()
                .all(|line| !line.contains(" socket ")),
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
            lines[1].starts_with("├─ initrd   ■ done in 7.2s · 1 unit"),
            "{screen}"
        );
        assert!(lines[2].starts_with("└─ stage 2  10.1s"), "{screen}");
        assert!(lines[3].starts_with("   └─ ◆ graphical.target"), "{screen}");
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
            lines[1].starts_with(
                "└─ ■ boot finished in 9.5s · handing over to display-manager.service"
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
            lines[1].starts_with("└─ shutdown 1.5s") && lines[1].ends_with("running"),
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
                .starts_with("└─ ■ shutdown finished in 4.2s · handing over to systemd-shutdown"),
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
            lines.iter().any(|line| line.starts_with("   … ")),
            "{}",
            lines.join("\n")
        );
    }
}
