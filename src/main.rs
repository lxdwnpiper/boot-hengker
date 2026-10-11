mod clock;
mod console;
mod error;
mod feed;
mod model;
mod run;
mod sysroot;
mod systemd;
mod view;

use std::{process::ExitCode, time::Duration};

use clap::Parser;

use crate::{
    run::{Options, run},
    systemd::Scope,
};

#[derive(Parser)]
#[command(about = "Live dependency tree of systemd jobs while the system boots")]
struct Cli {
    #[arg(long, help = "Watch the user service manager instead of PID 1")]
    user: bool,
    #[arg(
        long,
        help = "Root unit of the tree; defaults to the manager's default target"
    )]
    target: Option<String>,
    #[arg(
        long,
        default_value_t = 0,
        help = "Milliseconds to keep the final frame on screen"
    )]
    hold_ms: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = Options {
        scope: if cli.user { Scope::User } else { Scope::System },
        target: cli.target,
        hold: Duration::from_millis(cli.hold_ms),
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("boot-tree: {error}");
            ExitCode::FAILURE
        }
    }
}
