use std::time::Duration;

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use super::{BootView, PasswordPrompt};
use crate::{
    boot_model::{BootState, JobResult, Phase},
    boot_origin::BootOrigin,
    boot_test_fixtures::{MS, queued_boot, running, waiting},
    color_scheme::Colors,
    config_file::Timing,
    font_glyphs::Glyphs,
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
