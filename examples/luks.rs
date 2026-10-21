use std::{io, time::Duration};

use bootvine::{
    boot_model::Phase,
    scripted_boot::{Scenario, Step, play, step, unlocking},
};

const ROOT_PARTITION: &str =
    "dev-disk-by\\x2duuid-7c2e9f4a\\x2d1b3d\\x2d4e8f\\x2d9a6c\\x2d2d5b8e1f0c3a.device";

const INITRD: &[Step] = &[
    step(
        "initrd.target",
        &["initrd-fs.target", "initrd-root-fs.target"],
        Duration::ZERO,
    ),
    step(
        "initrd-fs.target",
        &["initrd-root-fs.target"],
        Duration::ZERO,
    ),
    step("initrd-root-fs.target", &["sysroot.mount"], Duration::ZERO),
    step(
        "sysroot.mount",
        &["systemd-fsck-root.service"],
        Duration::from_millis(400),
    ),
    step(
        "systemd-fsck-root.service",
        &["initrd-root-device.target"],
        Duration::from_millis(600),
    ),
    step(
        "initrd-root-device.target",
        &["dev-mapper-cryptroot.device"],
        Duration::ZERO,
    ),
    step(
        "dev-mapper-cryptroot.device",
        &["cryptsetup.target"],
        Duration::from_millis(100),
    ),
    step(
        "cryptsetup.target",
        &["systemd-cryptsetup@cryptroot.service"],
        Duration::ZERO,
    ),
    unlocking(
        "systemd-cryptsetup@cryptroot.service",
        &["cryptsetup-pre.target", ROOT_PARTITION],
        "Please enter passphrase for disk Samsung SSD 980 PRO 1TB (cryptroot):",
        Duration::from_millis(2000),
    ),
    step(
        "cryptsetup-pre.target",
        &["systemd-udevd.service"],
        Duration::ZERO,
    ),
    step(
        ROOT_PARTITION,
        &["systemd-udev-trigger.service"],
        Duration::from_millis(1100),
    ),
    step(
        "systemd-udev-trigger.service",
        &["systemd-udevd.service"],
        Duration::from_millis(500),
    ),
    step(
        "systemd-udevd.service",
        &["systemd-tmpfiles-setup-dev.service"],
        Duration::from_millis(200),
    ),
    step(
        "systemd-tmpfiles-setup-dev.service",
        &["kmod-static-nodes.service"],
        Duration::from_millis(100),
    ),
    step("kmod-static-nodes.service", &[], Duration::from_millis(100)),
    step("systemd-journald.service", &[], Duration::from_millis(300)),
    step(
        "systemd-modules-load.service",
        &[],
        Duration::from_millis(900),
    ),
    step(
        "systemd-vconsole-setup.service",
        &["systemd-modules-load.service"],
        Duration::from_millis(200),
    ),
    step(
        "systemd-sysctl.service",
        &["systemd-modules-load.service"],
        Duration::from_millis(100),
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
        &["systemd-logind.service"],
        Duration::from_millis(600),
    ),
    step(
        "multi-user.target",
        &[
            "basic.target",
            "NetworkManager.service",
            "systemd-logind.service",
            "systemd-timesyncd.service",
        ],
        Duration::ZERO,
    ),
    step("basic.target", &["sysinit.target"], Duration::ZERO),
    step(
        "sysinit.target",
        &[
            "systemd-tmpfiles-setup.service",
            "systemd-udev-trigger.service",
            "home.mount",
        ],
        Duration::ZERO,
    ),
    step(
        "systemd-tmpfiles-setup.service",
        &[],
        Duration::from_millis(400),
    ),
    step(
        "systemd-udev-trigger.service",
        &[],
        Duration::from_millis(800),
    ),
    step("home.mount", &[], Duration::from_millis(300)),
    step(
        "systemd-logind.service",
        &["basic.target"],
        Duration::from_millis(300),
    ),
    step(
        "NetworkManager.service",
        &["basic.target"],
        Duration::from_millis(1400),
    ),
    step(
        "systemd-timesyncd.service",
        &["basic.target"],
        Duration::from_millis(500),
    ),
];

fn description(unit: &str) -> &'static str {
    match unit {
        "initrd.target" => "Initrd Default Target",
        "initrd-fs.target" => "Initrd File Systems",
        "initrd-root-fs.target" => "Initrd Root File System",
        "initrd-root-device.target" => "Initrd Root Device",
        "sysroot.mount" => "/sysroot",
        "systemd-fsck-root.service" => "File System Check on /dev/mapper/cryptroot",
        "dev-mapper-cryptroot.device" => "/dev/mapper/cryptroot",
        "cryptsetup.target" => "Local Encrypted Volumes",
        "cryptsetup-pre.target" => "Local Encrypted Volumes (Pre)",
        "systemd-cryptsetup@cryptroot.service" => "Cryptography Setup for cryptroot",
        ROOT_PARTITION => "Samsung SSD 980 PRO 1TB cryptroot",
        "systemd-udev-trigger.service" => "Coldplug All udev Devices",
        "systemd-udevd.service" => "Rule-based Manager for Device Events and Files",
        "systemd-tmpfiles-setup-dev.service" => "Create Static Device Nodes in /dev",
        "kmod-static-nodes.service" => "Create List of Static Device Nodes",
        "systemd-journald.service" => "Journal Service",
        "systemd-modules-load.service" => "Load Kernel Modules",
        "systemd-vconsole-setup.service" => "Virtual Console Setup",
        "systemd-sysctl.service" => "Apply Kernel Variables",
        "graphical.target" => "Graphical Interface",
        "display-manager.service" => "Display Manager",
        "multi-user.target" => "Multi-User System",
        "basic.target" => "Basic System",
        "sysinit.target" => "System Initialization",
        "systemd-tmpfiles-setup.service" => "Create System Files and Directories",
        "home.mount" => "/home",
        "systemd-logind.service" => "User Login Management",
        "NetworkManager.service" => "Network Manager",
        "systemd-timesyncd.service" => "Network Time Synchronization",
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
