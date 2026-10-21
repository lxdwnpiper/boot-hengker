use std::time::Duration;

use unicode_width::UnicodeWidthStr;

use super::screen_render_fixtures::{boot_in_progress, chain_boot, described_boot, render};
use crate::{
    boot_model::{JobResult, Phase},
    demo_script::boot_test_fixtures::{MS, queue_unit, queued_boot, running, waiting},
};

#[test]
fn failures_stay_in_the_stage_that_wanted_them() {
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        [
            waiting("multi-user.target", &["basic.target", "web.service"]),
            waiting("basic.target", &["flaky.service", "quick.service"]),
            waiting("flaky.service", &[]),
            waiting("quick.service", &[]),
            waiting("web.service", &[]),
        ],
    );
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
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        [
            waiting("multi-user.target", &["early.service", "late.service"]),
            running("early.service", &[], Duration::ZERO),
            running("late.service", &[], Duration::ZERO),
        ],
    );
    state.finish_job(2, JobResult::Done, 3000 * MS);
    let lingering = render(&state, 4000 * MS, 110, 24);
    let screen = lingering.join("\n");
    assert!(lingering[3].starts_with("      ├─ [√] early"), "{screen}");
    assert!(lingering[4].starts_with("      └─ [|] late"), "{screen}");
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
        lines[7].starts_with("                  └─ [-] udev-settle"),
        "{screen}"
    );
}

#[test]
fn chains_fold_when_short_and_shorten_to_their_ends_when_narrow() {
    let short = render(&chain_boot(), 5000 * MS, 110, 9);
    assert!(
        short[3].starts_with("      └─ [ ] network-wait ─ network.target ─ network-manager ─ dbus"),
        "{}",
        short.join("\n")
    );
    assert!(
        short[4].starts_with("         └─ [-] udev-settle"),
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
    queue_unit(
        &mut state,
        9,
        running("docker.socket", &[], 1000 * MS).described("Docker Socket for the API"),
        Duration::ZERO,
    );
    let lines = render(&state, 3000 * MS, 110, 24);
    let screen = lines.join("\n");
    let socket_row = lines
        .iter()
        .find(|line| line.contains("Docker Socket"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(socket_row.contains("[-] docker "), "{screen}");
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
fn overflowing_trees_are_cut_with_a_count() {
    let names: Vec<String> = (0..30).map(|id| format!("unit{id:02}.service")).collect();
    let state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        names.iter().map(|name| waiting(name, &[])),
    );
    let lines = render(&state, 1000 * MS, 110, 20);
    assert!(
        lines.iter().any(|line| line.starts_with("   … ")),
        "{}",
        lines.join("\n")
    );
}
