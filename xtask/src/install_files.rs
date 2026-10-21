use std::{
    env,
    ffi::OsString,
    fs::{self, Permissions},
    io::ErrorKind,
    iter::once,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use clap::{Args, ValueEnum};
use rustix::process::geteuid;
use xshell::{Shell, cmd};

use crate::task_error::TaskError;

const UNIT_NAMES: [&str; 2] = ["bootvine.service", "bootvine-shutdown.service"];
const DRACUT_MODULE_DIR: &str = "/usr/lib/dracut/modules.d/90bootvine";
const DRACUT_CONF: &str = "/etc/dracut.conf.d/bootvine.conf";
const MKINITCPIO_HOOK: &str = "/usr/lib/initcpio/install/sd-bootvine";
const PACKAGED_BINARY_PATH: &str = "/usr/bin/bootvine";

#[derive(Args)]
pub struct InstallTarget {
    #[arg(
        long,
        default_value = "/usr",
        help = "Prefix for bin/bootvine and lib/systemd/system"
    )]
    prefix: PathBuf,
    #[arg(
        long,
        help = "Stage the files under this directory instead of /; skips sudo and systemctl"
    )]
    destdir: Option<PathBuf>,
}

#[derive(Args)]
pub struct InstallOptions {
    #[command(flatten)]
    target: InstallTarget,
    #[arg(long, value_enum, default_value_t = InitrdGenerator::None)]
    initrd: InitrdGenerator,
    #[arg(long, help = "Leave the units disabled")]
    no_enable: bool,
    #[arg(long, hide = true)]
    built_binary: Option<PathBuf>,
}

#[derive(Args)]
pub struct UninstallOptions {
    #[command(flatten)]
    target: InstallTarget,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InitrdGenerator {
    None,
    Dracut,
    Mkinitcpio,
}

struct InstalledFile {
    destination: PathBuf,
    contents: Vec<u8>,
    mode: u32,
}

impl InstallTarget {
    fn staged(&self, path: &Path) -> PathBuf {
        match &self.destdir {
            Some(root) => root.join(path.strip_prefix("/").unwrap_or(path)),
            None => path.to_owned(),
        }
    }

    fn changes_live_system(&self) -> bool {
        self.destdir.is_none()
    }

    fn needs_sudo(&self) -> bool {
        self.changes_live_system() && !geteuid().is_root()
    }

    fn binary_path(&self) -> PathBuf {
        self.prefix.join("bin/bootvine")
    }

    fn unit_path(&self, unit: &str) -> PathBuf {
        self.prefix.join("lib/systemd/system").join(unit)
    }

    fn every_installed_path(&self) -> Vec<PathBuf> {
        once(self.binary_path())
            .chain(UNIT_NAMES.iter().map(|unit| self.unit_path(unit)))
            .chain([
                Path::new(DRACUT_MODULE_DIR).join("module-setup.sh"),
                PathBuf::from(DRACUT_CONF),
                PathBuf::from(MKINITCPIO_HOOK),
            ])
            .collect()
    }
}

fn workspace_root() -> &'static Path {
    let xtask_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask_dir.parent().unwrap_or(xtask_dir)
}

fn release_binary() -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from)
        .join("release/bootvine")
}

fn read_file(path: &Path) -> Result<Vec<u8>, TaskError> {
    fs::read(path).map_err(|source| TaskError::File {
        path: path.to_owned(),
        source,
    })
}

fn read_system_file(relative: &str) -> Result<Vec<u8>, TaskError> {
    read_file(&workspace_root().join("system-files").join(relative))
}

fn unit_files(target: &InstallTarget) -> Result<Vec<InstalledFile>, TaskError> {
    UNIT_NAMES
        .iter()
        .map(|unit| {
            let packaged = read_system_file(&format!("systemd/{unit}"))?;
            let rewritten = String::from_utf8_lossy(&packaged).replace(
                PACKAGED_BINARY_PATH,
                &target.binary_path().to_string_lossy(),
            );
            Ok(InstalledFile {
                destination: target.unit_path(unit),
                contents: rewritten.into_bytes(),
                mode: 0o644,
            })
        })
        .collect()
}

fn initrd_hook_files(initrd: InitrdGenerator) -> Result<Vec<InstalledFile>, TaskError> {
    Ok(match initrd {
        InitrdGenerator::None => Vec::new(),
        InitrdGenerator::Dracut => vec![
            InstalledFile {
                destination: Path::new(DRACUT_MODULE_DIR).join("module-setup.sh"),
                contents: read_system_file("dracut/90bootvine/module-setup.sh")?,
                mode: 0o755,
            },
            InstalledFile {
                destination: PathBuf::from(DRACUT_CONF),
                contents: b"add_dracutmodules+=\" bootvine \"\n".to_vec(),
                mode: 0o644,
            },
        ],
        InitrdGenerator::Mkinitcpio => vec![InstalledFile {
            destination: PathBuf::from(MKINITCPIO_HOOK),
            contents: read_system_file("mkinitcpio/install/sd-bootvine")?,
            mode: 0o644,
        }],
    })
}

fn write_installed_file(target: &InstallTarget, file: &InstalledFile) -> Result<(), TaskError> {
    let path = target.staged(&file.destination);
    path.parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, &file.contents))
        .and_then(|()| fs::set_permissions(&path, Permissions::from_mode(file.mode)))
        .map_err(|source| TaskError::File { path, source })
}

fn remove_if_present(path: PathBuf) -> Result<(), TaskError> {
    match fs::remove_file(&path) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(TaskError::File {
            path,
            source: error,
        }),
        _ => Ok(()),
    }
}

fn rerun_with_sudo(shell: &Shell, extra_arguments: &[OsString]) -> Result<(), TaskError> {
    let this_task = env::current_exe().map_err(|source| TaskError::File {
        path: PathBuf::from("/proc/self/exe"),
        source,
    })?;
    let task_arguments = env::args_os().skip(1);
    cmd!(
        shell,
        "sudo {this_task} {task_arguments...} {extra_arguments...}"
    )
    .run()?;
    Ok(())
}

fn print_initramfs_hint(initrd: InitrdGenerator) {
    match initrd {
        InitrdGenerator::None => {}
        InitrdGenerator::Dracut => println!("rebuild the initramfs: sudo dracut --force"),
        InitrdGenerator::Mkinitcpio => println!(
            "add sd-bootvine after systemd in HOOKS in /etc/mkinitcpio.conf, then run: sudo \
             mkinitcpio -P"
        ),
    }
}

fn build_release_binary(shell: &Shell) -> Result<PathBuf, TaskError> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let _in_workspace = shell.push_dir(workspace_root());
    cmd!(shell, "{cargo} build --release --package bootvine").run()?;
    Ok(release_binary())
}

pub fn install_files(options: &InstallOptions) -> Result<(), TaskError> {
    let shell = Shell::new()?;
    let target = &options.target;
    let binary = match &options.built_binary {
        Some(built) => built.clone(),
        None => build_release_binary(&shell)?,
    };
    if target.needs_sudo() {
        return rerun_with_sudo(&shell, &["--built-binary".into(), binary.into_os_string()]);
    }
    let binary_file = InstalledFile {
        destination: target.binary_path(),
        contents: read_file(&binary)?,
        mode: 0o755,
    };
    once(binary_file)
        .chain(unit_files(target)?)
        .chain(initrd_hook_files(options.initrd)?)
        .try_for_each(|file| write_installed_file(target, &file))?;
    if target.changes_live_system() {
        cmd!(shell, "systemctl daemon-reload").run()?;
        if !options.no_enable {
            cmd!(shell, "systemctl enable {UNIT_NAMES...}").run()?;
        }
        print_initramfs_hint(options.initrd);
    }
    Ok(())
}

pub fn uninstall_files(options: &UninstallOptions) -> Result<(), TaskError> {
    let shell = Shell::new()?;
    let target = &options.target;
    if target.needs_sudo() {
        return rerun_with_sudo(&shell, &[]);
    }
    if target.changes_live_system() {
        let enabled_units: Vec<&str> = UNIT_NAMES
            .into_iter()
            .filter(|unit| target.unit_path(unit).exists())
            .collect();
        if !enabled_units.is_empty() {
            cmd!(shell, "systemctl disable {enabled_units...}").run()?;
        }
    }
    target
        .every_installed_path()
        .iter()
        .try_for_each(|path| remove_if_present(target.staged(path)))?;
    match fs::remove_dir(target.staged(Path::new(DRACUT_MODULE_DIR))) {
        Err(error) if error.kind() == ErrorKind::DirectoryNotEmpty => {
            return Err(TaskError::File {
                path: target.staged(Path::new(DRACUT_MODULE_DIR)),
                source: error,
            });
        }
        _ => {}
    }
    if target.changes_live_system() {
        cmd!(shell, "systemctl daemon-reload").run()?;
    }
    Ok(())
}
