use std::{
    fs::{read_dir, read_link, read_to_string},
    path::{Path, PathBuf},
};

use rustix::system::uname;

const KERNEL_COMMAND_LINE: &str = "/proc/cmdline";
const SYSTEM_PROFILES: &str = "/nix/var/nix/profiles";
const OS_RELEASES: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];
const NIXOS_SYSTEM: &str = "nixos-system-";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BootOrigin {
    pub host: String,
    pub system: String,
    pub generation: String,
    pub kernel: String,
}

impl BootOrigin {
    #[must_use]
    pub fn read() -> Self {
        let names = uname();
        let host = names.nodename().to_string_lossy().into_owned();
        let command_line = read_to_string(KERNEL_COMMAND_LINE).unwrap_or_default();
        let system_path = init_system(&command_line);
        let system = system_path
            .and_then(|path| nixos_version(path, &host))
            .map(|version| format!("NixOS {version}"))
            .or_else(|| {
                OS_RELEASES
                    .iter()
                    .find_map(|path| read_to_string(path).ok())
                    .and_then(|release| pretty_name(&release))
            })
            .unwrap_or_default();
        let generation = system_path
            .map(|path| generation_of(Path::new(path), &system_links()))
            .unwrap_or_default();
        Self {
            kernel: format!("Linux {}", names.release().to_string_lossy()),
            host,
            system,
            generation,
        }
    }

    #[must_use]
    pub fn parts(&self) -> Vec<String> {
        [
            self.host.clone(),
            self.system.clone(),
            if self.generation.is_empty() {
                String::new()
            } else {
                format!("generation {}", self.generation)
            },
            self.kernel.clone(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect()
    }
}

fn init_system(command_line: &str) -> Option<&str> {
    command_line
        .split_whitespace()
        .find_map(|argument| argument.strip_prefix("init="))
        .and_then(|init| init.strip_suffix("/init"))
}

fn nixos_version<'a>(system_path: &'a str, host: &str) -> Option<&'a str> {
    let (_, name) = system_path.rsplit_once('/')?;
    let (_, rest) = name.split_once('-')?;
    rest.strip_prefix(NIXOS_SYSTEM)?
        .strip_prefix(host)?
        .strip_prefix('-')
}

fn pretty_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_owned())
}

struct SystemLink {
    generation: String,
    target: PathBuf,
}

fn system_links() -> Vec<SystemLink> {
    read_dir(SYSTEM_PROFILES)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let generation = name.strip_prefix("system-")?.strip_suffix("-link")?;
            Some(SystemLink {
                generation: generation.to_owned(),
                target: read_link(entry.path()).ok()?,
            })
        })
        .collect()
}

fn generation_of(system_path: &Path, links: &[SystemLink]) -> String {
    links
        .iter()
        .find(|link| link.target == system_path)
        .map(|link| link.generation.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND_LINE: &str = "initrd=\\EFI\\nixos\\x-initrd.efi \
                                init=/nix/store/\
                                av1146s7ayap8zgrpindi641v1fmdxsf-nixos-system-ryzen-26.11.\
                                20261007.39ad350/init panic=10 root=fstab";

    #[test]
    fn the_nixos_version_comes_from_the_init_on_the_command_line() {
        let system = init_system(COMMAND_LINE).expect("init system");
        assert_eq!(
            nixos_version(system, "ryzen"),
            Some("26.11.20261007.39ad350")
        );
        assert_eq!(nixos_version(system, "other-host"), None);
    }

    #[test]
    fn the_generation_is_the_profile_link_to_the_booted_system() {
        let system = init_system(COMMAND_LINE).expect("init system");
        let links = [
            SystemLink {
                generation: "2770".to_owned(),
                target: "/nix/store/x43q-nixos-system-ryzen-26.11".into(),
            },
            SystemLink {
                generation: "2771".to_owned(),
                target: system.into(),
            },
        ];
        assert_eq!(generation_of(Path::new(system), &links), "2771");
        assert_eq!(generation_of(Path::new(system), &links[..1]), "");
    }

    #[test]
    fn other_distributions_use_their_pretty_name() {
        assert_eq!(
            pretty_name("NAME=Arch\nPRETTY_NAME=\"Arch Linux\"\n"),
            Some("Arch Linux".to_owned())
        );
    }

    #[test]
    fn unknown_parts_are_left_out() {
        let origin = BootOrigin {
            host: "ryzen".to_owned(),
            system: "NixOS 26.11".to_owned(),
            generation: String::new(),
            kernel: "Linux 7.2.9".to_owned(),
        };
        assert_eq!(origin.parts(), ["ryzen", "NixOS 26.11", "Linux 7.2.9"]);
    }
}
