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
        about = "Record the screencasts with vhs and copy the passphrase and failures screenshots \
                 into recordings/"
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
    #[arg(long, help = "failures.png produced by the vm-failures check")]
    failures_png: PathBuf,
}

fn main() -> ExitCode {
    let outcome = match RepoTasks::parse().task {
        Task::RecordScreencasts(options) => {
            record_screencasts::record_screencasts(&record_screencasts::CheckScreenshots {
                passphrase_png: &options.passphrase_png,
                failures_png: &options.failures_png,
            })
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
