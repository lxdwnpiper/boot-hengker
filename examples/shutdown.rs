use std::{io, time::Duration};

use bootvine::{
    boot_model::Phase,
    demo_script::scripted_boot::{Scenario, Step, play, step, stopping},
};

const SHUTDOWN: &[Step] = &[
    step(
        "poweroff.target",
        &["systemd-poweroff.service"],
        Duration::ZERO,
    ),
    step(
        "systemd-poweroff.service",
        &["final.target"],
        Duration::from_millis(300),
    ),
    step("final.target", &["umount.target"], Duration::ZERO),
    step(
        "umount.target",
        &["shutdown.target", "var-lib-docker.mount", "home.mount"],
        Duration::ZERO,
    ),
    step(
        "shutdown.target",
        &[
            "display-manager.service",
            "user@1000.service",
            "docker.service",
            "containerd.service",
            "tailscaled.service",
            "NetworkManager.service",
        ],
        Duration::ZERO,
    ),
    stopping("display-manager.service", &[], Duration::from_millis(900)),
    stopping(
        "user@1000.service",
        &["display-manager.service"],
        Duration::from_millis(1600),
    ),
    stopping("docker.service", &[], Duration::from_millis(3200)),
    stopping(
        "containerd.service",
        &["docker.service"],
        Duration::from_millis(700),
    ),
    stopping("tailscaled.service", &[], Duration::from_millis(1100)),
    stopping(
        "NetworkManager.service",
        &["tailscaled.service", "docker.service"],
        Duration::from_millis(600),
    ),
    stopping(
        "var-lib-docker.mount",
        &["docker.service", "containerd.service"],
        Duration::from_millis(400),
    ),
    stopping(
        "home.mount",
        &["user@1000.service"],
        Duration::from_millis(500),
    ),
];

fn description(unit: &str) -> Option<&'static str> {
    let description = match unit {
        "poweroff.target" | "systemd-poweroff.service" => "System Power Off",
        "final.target" => "Late Shutdown Services",
        "umount.target" => "Unmount All Filesystems",
        "shutdown.target" => "System Shutdown",
        "display-manager.service" => "Display Manager",
        "user@1000.service" => "User Manager for UID 1000",
        "docker.service" => "Docker Application Container Engine",
        "containerd.service" => "containerd container runtime",
        "tailscaled.service" => "Tailscale node agent",
        "NetworkManager.service" => "Network Manager",
        "var-lib-docker.mount" => "/var/lib/docker",
        "home.mount" => "/home",
        _ => return None,
    };
    Some(description)
}

fn main() -> io::Result<()> {
    ratatui::run(|terminal| {
        play(
            terminal,
            &Scenario {
                phase: Phase::Shutdown,
                steps: SHUTDOWN,
                after_switch_root: &[],
                describe: description,
            },
        )
    })
}
