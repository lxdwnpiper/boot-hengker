use std::time::Duration;

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use super::{BootView, PasswordPrompt, project_boot};
use crate::{
    boot_model::{BootState, JobResult, Phase, boot_origin::BootOrigin},
    boot_test_fixtures::{MS, queued_boot, running, waiting},
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
};

pub fn render(state: &BootState, now: Duration, width: u16, height: u16) -> Vec<String> {
    screen_lines(&render_prompted(state, now, width, height, None))
}

pub fn render_prompted(
    state: &BootState,
    now: Duration,
    width: u16,
    height: u16,
    password_prompt: Option<PasswordPrompt>,
) -> Buffer {
    draw(state, now, width, height, password_prompt, None)
}

pub fn render_holding(state: &BootState, now: Duration, width: u16, height: u16) -> Vec<String> {
    screen_lines(&draw(
        state,
        now,
        width,
        height,
        None,
        Some("Press Enter to continue"),
    ))
}

fn draw(
    state: &BootState,
    now: Duration,
    width: u16,
    height: u16,
    password_prompt: Option<PasswordPrompt>,
    hold_hint: Option<&str>,
) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
    let timing = Timing::default();
    let projection = project_boot(state, &timing, now);
    terminal
        .draw(|frame| {
            frame.render_widget(
                BootView {
                    state,
                    projection: &projection,
                    colors: &Colors::default(),
                    timing: &timing,
                    glyphs: &Glyphs::default(),
                    now,
                    origin: &ryzen_origin(),
                    password_prompt,
                    hold_hint,
                    operator: None,
                },
                frame.area(),
            );
        })
        .expect("draw");
    terminal.backend().buffer().clone()
}

pub fn ryzen_origin() -> BootOrigin {
    BootOrigin {
        host: "ryzen".to_owned(),
        system: Some("NixOS 26.11".to_owned()),
        generation: Some("2771".to_owned()),
        kernel: "Linux 7.2.9".to_owned(),
    }
}

pub fn screen_lines(buffer: &Buffer) -> Vec<String> {
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

pub fn boot_in_progress() -> BootState {
    let mut state = queued_boot(
        Phase::Host,
        1000 * MS,
        [
            waiting("graphical.target", &["multi-user.target"]),
            waiting(
                "multi-user.target",
                &["docker.service", "systemd-rfkill.service", "web.service"],
            ),
            running("docker.service", &[], 9000 * MS),
            running("systemd-rfkill.service", &[], 6000 * MS),
            waiting("web.service", &[]),
            waiting("sysinit.target", &[]),
            waiting("nvidia-powerd.service", &[]),
            running("udev-settle.service", &[], 2000 * MS),
        ],
    );
    state.finish_job(6, JobResult::Done, 4000 * MS);
    state.finish_job(7, JobResult::Failed, 5000 * MS);
    state.finish_job(8, JobResult::Done, 3500 * MS);
    state
}

pub fn chain_boot() -> BootState {
    queued_boot(
        Phase::Host,
        Duration::ZERO,
        [
            waiting("graphical.target", &["network-wait.service"]),
            waiting("network-wait.service", &["network.target"]),
            waiting("network.target", &["network-manager.service"]),
            waiting("network-manager.service", &["dbus.service"]),
            waiting("dbus.service", &["udev-settle.service"]),
            running("udev-settle.service", &[], 1000 * MS),
        ],
    )
}

pub fn described_boot() -> BootState {
    queued_boot(
        Phase::Host,
        Duration::ZERO,
        [
            running(
                "multi-user.target",
                &["docker.service", "systemd-rfkill.service"],
                1000 * MS,
            )
            .described("Multi-User System"),
            running("docker.service", &[], 1000 * MS)
                .described("Docker Application Container Engine"),
            running("systemd-rfkill.service", &[], 1000 * MS)
                .described("Load/Save RF Kill Switch Status"),
        ],
    )
}
