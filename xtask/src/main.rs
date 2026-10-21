mod install_files;
mod record_screencasts;
mod task_error;

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};

use crate::install_files::{InstallOptions, UninstallOptions, install_files, uninstall_files};

#[derive(Parser)]
#[command(about = "Repository tasks for bootvine")]
struct RepoTasks {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    #[command(
        about = "Record the README screencasts with vhs and copy the passphrase screenshot into \
                 recordings/"
    )]
    RecordScreencasts {
        #[arg(long, help = "passphrase.png produced by the vm-luks-passphrase check")]
        passphrase_png: PathBuf,
    },
    #[command(about = "Build bootvine and install the binary, units and initrd hook")]
    Install(InstallOptions),
    #[command(about = "Remove what install put in place")]
    Uninstall(UninstallOptions),
}

fn main() -> ExitCode {
    let outcome = match RepoTasks::parse().task {
        Task::RecordScreencasts { passphrase_png } => {
            record_screencasts::record_screencasts(&passphrase_png)
        }
        Task::Install(options) => install_files(&options),
        Task::Uninstall(options) => uninstall_files(&options),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
