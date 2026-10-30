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
        about = "Record the screencasts with vhs and convert the passphrase and failures \
                 screenshots to WebP in recordings/"
    )]
    RecordScreencasts(RecordScreencastsOptions),
    #[command(
        about = "Rewrite the defaults block in docs/configuration.md from `bootvine defaults`"
    )]
    SyncDefaultConfig,
}

#[derive(Args)]
struct RecordScreencastsOptions {
    #[arg(
        long,
        help = "Directory with one entry per sample machine, each the output of its vm-<machine> \
                check"
    )]
    vm_check_outputs: PathBuf,
}

fn main() -> ExitCode {
    let outcome = match RepoTasks::parse().task {
        Task::RecordScreencasts(options) => {
            record_screencasts::record_screencasts(&options.vm_check_outputs)
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
