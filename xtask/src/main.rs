mod record_screencasts;
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
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
