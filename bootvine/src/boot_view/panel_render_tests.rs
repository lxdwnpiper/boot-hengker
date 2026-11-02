use std::time::Duration;

use super::{
    PasswordPrompt,
    screen_render_fixtures::{boot_in_progress, render, render_prompted, screen_lines},
};
use crate::{
    boot_model::{ActiveState, Activity, BootState, FailureCause, HANDOFF_UNIT, JobResult, Phase},
    boot_test_fixtures::{MS, queue_unit, queued_boot, running, waiting},
    screen_settings::color_scheme::Colors,
};

const CRYPTSETUP_MESSAGE: &str =
    "Please enter passphrase for disk Samsung SSD 980 PRO 1TB (cryptroot):";

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
        lines[3].starts_with("   │  ├─ [|] systemd-rfkill"),
        "{screen}"
    );
    assert!(lines[4].starts_with("   │  ├─ [|] docker"), "{screen}");
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
        lines[timeline + 4].contains("·█")
            && lines[timeline + 4].ends_with(['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█']),
        "{screen}"
    );
    assert!(lines[timeline + 5].starts_with("► docker"), "{screen}");
    assert!(
        lines[timeline..].iter().all(|line| !line.contains(" web")),
        "{screen}"
    );
    assert!(!screen.contains("root switch"), "{screen}");
    assert!(
        lines[23].starts_with("√ 2 done  2 running  3 waiting  × 1 failed"),
        "{screen}"
    );
}

#[test]
fn password_prompt_floats_over_a_dimmed_view() {
    let buffer = render_prompted(
        &boot_in_progress(),
        12_400 * MS,
        110,
        24,
        Some(PasswordPrompt {
            message: CRYPTSETUP_MESSAGE,
            hint: None,
            input: "***".to_owned(),
            sent: false,
            rejected: false,
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
        lines[23].starts_with("√ 2 done  2 running  3 waiting  × 1 failed"),
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
            hint: None,
            input: String::new(),
            sent: true,
            rejected: false,
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
            hint: None,
            input: String::new(),
            sent: false,
            rejected: false,
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
fn a_rejected_passphrase_is_called_out_under_the_hint() {
    let buffer = render_prompted(
        &boot_in_progress(),
        12_400 * MS,
        110,
        24,
        Some(PasswordPrompt {
            message: CRYPTSETUP_MESSAGE,
            hint: Some("This demo unlocks with the passphrase \"demo\""),
            input: String::new(),
            sent: false,
            rejected: true,
        }),
    );
    let lines = screen_lines(&buffer);
    let screen = lines.join("\n");
    let message_row = lines
        .iter()
        .position(|line| line.contains(CRYPTSETUP_MESSAGE))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(
        lines[message_row + 1].contains("│  This demo unlocks with the passphrase \"demo\""),
        "{screen}"
    );
    assert!(
        lines[message_row + 3].contains("│  Wrong passphrase, try again"),
        "{screen}"
    );
    assert!(lines[message_row + 4].contains("│  > _"), "{screen}");
    let rejection_row = u16::try_from(message_row + 3).expect("row fits the screen");
    let warning = (0..buffer.area.width)
        .map(|column| &buffer[(column, rejection_row)])
        .find(|cell| cell.symbol() == "W")
        .unwrap_or_else(|| panic!("{screen}"));
    assert_eq!(warning.fg, Colors::default().password.rejected, "{screen}");
}

#[test]
fn sub_second_runs_stay_off_the_timeline() {
    let mut state = boot_in_progress();
    queue_unit(
        &mut state,
        20,
        running("blink.service", &[], 4000 * MS),
        4000 * MS,
    );
    state.finish_job(20, JobResult::Done, 4400 * MS);
    let lines = render(&state, 12_400 * MS, 110, 24);
    assert!(
        lines.iter().all(|line| !line.contains("blink")),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn long_timeline_labels_end_in_an_ellipsis() {
    let mut state = boot_in_progress();
    queue_unit(
        &mut state,
        30,
        running("sys-kernel-config.mount", &[], 2000 * MS)
            .described("Kernel Configuration File System"),
        2000 * MS,
    );
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
    let lines = render(&boot_in_progress(), 12_400 * MS, 37, 24);
    assert!(
        lines.iter().all(|line| !line.contains('■')),
        "{}",
        lines.join("\n")
    );
    assert!(lines[3].contains("systemd-rfkill"), "{}", lines.join("\n"));
}

#[test]
fn finished_stages_collapse_into_a_summary_rule() {
    let mut state = queued_boot(
        Phase::Initrd,
        Duration::ZERO,
        [waiting("initrd.target", &[])],
    );
    state.finish_job(1, JobResult::Done, 6000 * MS);
    state.switch_root(7200 * MS);
    state.set_root("graphical.target".to_owned());
    queue_unit(&mut state, 2, waiting("graphical.target", &[]), 7300 * MS);
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
fn completion_without_a_display_manager_hands_back_the_console() {
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        [waiting("graphical.target", &[])],
    );
    state.finish_job(1, JobResult::Done, 9000 * MS);
    let lines = render(&state, 9500 * MS, 110, 24);
    assert!(
        lines[1].starts_with("└─ ■ Boot finished in 9.5s · handing over to the console"),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn completion_names_the_display_manager_that_started() {
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        [waiting("graphical.target", &[])],
    );
    queue_unit(&mut state, 2, waiting(HANDOFF_UNIT, &[]), 500 * MS);
    state.update_activity(
        HANDOFF_UNIT,
        Activity {
            state: Some(ActiveState::Active),
            ..Activity::default()
        },
    );
    state.finish_job(1, JobResult::Done, 9000 * MS);
    let lines = render(&state, 9500 * MS, 110, 24);
    assert!(
        lines[1]
            .starts_with("└─ ■ Boot finished in 9.5s · handing over to display-manager.service"),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn shutdown_in_progress_names_its_stage() {
    let mut state = queued_boot(
        Phase::Shutdown,
        20_000 * MS,
        [waiting("poweroff.target", &[])],
    );
    state.origin = 20_000 * MS;
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
        lines[1].starts_with("└─ ■ Shutdown finished in 4.2s · handing over to systemd-shutdown"),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn the_timeline_keeps_only_the_slowest_finished_runs() {
    let quick_units: Vec<String> = (0..10).map(|n| format!("run{n:02}.service")).collect();
    let after: Vec<&str> = quick_units
        .iter()
        .map(String::as_str)
        .chain(["slow.service"])
        .collect();
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        [
            waiting("multi-user.target", &after),
            running("slow.service", &[], Duration::ZERO),
        ],
    );
    quick_units.iter().zip(10..).for_each(|(name, id)| {
        queue_unit(
            &mut state,
            id,
            running(name, &[], Duration::ZERO),
            Duration::ZERO,
        );
        state.finish_job(id, JobResult::Done, (1000 * id - 9000) * MS);
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

fn explained_nvidia_failure() -> BootState {
    let mut state = boot_in_progress();
    state.explain_failure(
        "nvidia-powerd.service",
        FailureCause {
            reason: "exited with status 1".to_owned(),
            log_lines: vec![
                "ERROR! Failed to get system power limits".to_owned(),
                "giving up".to_owned(),
            ],
        },
    );
    state
}

#[test]
fn failures_show_their_reason_and_journal_lines_above_the_timeline() {
    let lines = render(&explained_nvidia_failure(), 12_400 * MS, 110, 30);
    let screen = lines.join("\n");
    let failures = lines
        .iter()
        .position(|line| line.contains(" Failures "))
        .expect("failure panel");
    let timeline = lines
        .iter()
        .position(|line| line.contains(" Timeline "))
        .expect("timeline panel");
    assert!(failures < timeline, "{screen}");
    assert_eq!(
        lines[failures + 1],
        "× nvidia-powerd  exited with status 1",
        "{screen}"
    );
    assert_eq!(
        lines[failures + 2],
        "   ERROR! Failed to get system power limits",
        "{screen}"
    );
    assert_eq!(lines[failures + 3], "   giving up", "{screen}");
}

#[test]
fn failure_lines_are_cut_to_a_narrow_screen() {
    let lines = render(&explained_nvidia_failure(), 12_400 * MS, 30, 30);
    let screen = lines.join("\n");
    let failures = lines
        .iter()
        .position(|line| line.contains(" Failures "))
        .expect("failure panel");
    assert_eq!(
        lines[failures + 1],
        "× nvidia-powerd  exited with …",
        "{screen}"
    );
    assert_eq!(
        lines[failures + 2],
        "   ERROR! Failed to get syste…",
        "{screen}"
    );
}

#[test]
fn dependency_failures_stay_out_of_the_failure_panel() {
    let mut state = queued_boot(
        Phase::Host,
        1000 * MS,
        [
            waiting("multi-user.target", &["web.service"]),
            waiting("web.service", &[]),
        ],
    );
    state.finish_job(2, JobResult::Dependency, 2000 * MS);
    let lines = render(&state, 3000 * MS, 110, 24);
    let screen = lines.join("\n");
    assert!(!screen.contains(" Failures "), "{screen}");
    assert!(screen.contains("[×] web"), "{screen}");
    assert!(screen.contains("× 1 failed"), "{screen}");
}
