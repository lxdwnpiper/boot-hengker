use std::{io, time::Duration};

use bootvine::{
    boot_model::Phase,
    scripted_boot::{Scenario, Step, failing, play, step},
};

const INITRD: &[Step] = &[
    step(
        "initrd.target",
        &["initrd-fs.target", "initrd-root-device.target"],
        Duration::ZERO,
    ),
    step("initrd-fs.target", &["sysroot.mount"], Duration::ZERO),
    step(
        "initrd-root-device.target",
        &["dev-disk-by\\x2dlabel-nixos.device"],
        Duration::ZERO,
    ),
    step(
        "sysroot.mount",
        &["systemd-fsck-root.service"],
        Duration::from_millis(300),
    ),
    step(
        "systemd-fsck-root.service",
        &["dev-disk-by\\x2dlabel-nixos.device"],
        Duration::from_millis(500),
    ),
    step(
        "dev-disk-by\\x2dlabel-nixos.device",
        &[],
        Duration::from_millis(1200),
    ),
    step("systemd-udevd.service", &[], Duration::from_millis(400)),
    step(
        "demo-initrd-probe.service",
        &[],
        Duration::from_millis(2500),
    ),
];

const HOST: &[Step] = &[
    step(
        "graphical.target",
        &["multi-user.target", "display-manager.service"],
        Duration::ZERO,
    ),
    step(
        "display-manager.service",
        &["multi-user.target"],
        Duration::from_millis(400),
    ),
    step(
        "multi-user.target",
        &[
            "basic.target",
            "NetworkManager-wait-online.service",
            "docker.service",
            "tailscaled.service",
            "nvidia-powerd.service",
            "systemd-rfkill.service",
        ],
        Duration::ZERO,
    ),
    step("basic.target", &["sysinit.target"], Duration::ZERO),
    step(
        "sysinit.target",
        &[
            "systemd-tmpfiles-setup.service",
            "systemd-udev-trigger.service",
        ],
        Duration::ZERO,
    ),
    step(
        "systemd-tmpfiles-setup.service",
        &[],
        Duration::from_millis(600),
    ),
    step(
        "systemd-udev-trigger.service",
        &[],
        Duration::from_millis(900),
    ),
    step(
        "NetworkManager.service",
        &["basic.target"],
        Duration::from_millis(1500),
    ),
    step(
        "NetworkManager-wait-online.service",
        &["NetworkManager.service"],
        Duration::from_millis(3500),
    ),
    step(
        "network-online.target",
        &["NetworkManager-wait-online.service"],
        Duration::ZERO,
    ),
    step(
        "tailscaled.service",
        &["network-online.target"],
        Duration::from_millis(1200),
    ),
    step(
        "containerd.service",
        &["basic.target"],
        Duration::from_millis(800),
    ),
    step(
        "docker.socket",
        &["basic.target"],
        Duration::from_millis(200),
    ),
    step("var-lib-docker.mount", &[], Duration::from_millis(1800)),
    step(
        "docker.service",
        &[
            "containerd.service",
            "docker.socket",
            "var-lib-docker.mount",
            "network-online.target",
        ],
        Duration::from_millis(1500),
    ),
    failing(
        "nvidia-powerd.service",
        &["basic.target"],
        Duration::from_millis(700),
    ),
    step(
        "sys-devices-virtual-misc-rfkill.device",
        &[],
        Duration::from_millis(4000),
    ),
    step(
        "systemd-rfkill.service",
        &["sys-devices-virtual-misc-rfkill.device"],
        Duration::from_millis(300),
    ),
];

fn description(unit: &str) -> &'static str {
    match unit {
        "initrd.target" => "Initrd Default Target",
        "initrd-fs.target" => "Initrd File Systems",
        "initrd-root-device.target" => "Initrd Root Device",
        "sysroot.mount" => "/sysroot",
        "systemd-fsck-root.service" => "File System Check on Root Device",
        "dev-disk-by\\x2dlabel-nixos.device" => "/dev/disk/by-label/nixos",
        "systemd-udevd.service" => "Rule-based Manager for Device Events and Files",
        "demo-initrd-probe.service" => "Slow hardware probe",
        "graphical.target" => "Graphical Interface",
        "display-manager.service" => "Display Manager",
        "multi-user.target" => "Multi-User System",
        "basic.target" => "Basic System",
        "sysinit.target" => "System Initialization",
        "systemd-tmpfiles-setup.service" => "Create System Files and Directories",
        "systemd-udev-trigger.service" => "Coldplug All udev Devices",
        "NetworkManager.service" => "Network Manager",
        "NetworkManager-wait-online.service" => "Network Manager Wait Online",
        "network-online.target" => "Network is Online",
        "tailscaled.service" => "Tailscale node agent",
        "containerd.service" => "containerd container runtime",
        "docker.socket" => "Docker Socket for the API",
        "docker.service" => "Docker Application Container Engine",
        "var-lib-docker.mount" => "/var/lib/docker",
        "nvidia-powerd.service" => "NVIDIA Dynamic Boost",
        "sys-devices-virtual-misc-rfkill.device" => "/sys/devices/virtual/misc/rfkill",
        "systemd-rfkill.service" => "Load/Save RF Kill Switch Status",
        _ => "",
    }
}

fn main() -> io::Result<()> {
    ratatui::run(|terminal| {
        play(
            terminal,
            &Scenario {
                phase: Phase::Initrd,
                steps: INITRD,
                after_switch_root: HOST,
                describe: description,
            },
        )
    })
}
