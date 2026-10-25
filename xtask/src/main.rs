mod record_screencasts;
mod sync_default_config;
mod task_error;

use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};

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
    RecordScreencasts(RecordScreencastsOptions),
    #[command(
        about = "Rewrite the defaults block in docs/configuration.md from `bootvine defaults`"
    )]
    SyncDefaultConfig,
}

#[derive(Args)]
struct RecordScreencastsOptions {
    #[arg(long, help = "passphrase.png produced by the vm-luks-passphrase check")]
    passphrase_png: PathBuf,
}

fn main() -> ExitCode {
    let outcome = match RepoTasks::parse().task {
        Task::RecordScreencasts(options) => {
            record_screencasts::record_screencasts(&options.passphrase_png)
        }
        Task::SyncDefaultConfig => sync_default_config::sync_default_config(),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
