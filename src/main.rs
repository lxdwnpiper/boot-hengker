mod clock;
mod console;
mod error;
mod feed;
mod run;
mod sysroot;
mod systemd;

use std::{fs, io::ErrorKind, path::PathBuf, process::ExitCode, time::Duration};

use bootvine::palette::Config;
use clap::Parser;
use jiff::SignedDuration;

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
        help = "Follow a shutdown instead of a boot; started by bootvine-shutdown.service"
    )]
    shutdown: bool,
    #[arg(
        long,
        help = "Root unit of the tree; defaults to the manager's default target"
    )]
    target: Option<String>,
    #[arg(
        long,
        default_value = "1s",
        value_parser = parse_duration,
        help = "How long the final frame stays on screen, e.g. 1s or 500ms"
    )]
    hold: Duration,
    #[arg(
        long,
        default_value = "/etc/bootvine.toml",
        help = "TOML file with a [palette] table"
    )]
    config: PathBuf,
}

fn parse_duration(raw: &str) -> Result<Duration, String> {
    let signed: SignedDuration = raw.parse().map_err(|error| format!("{error}"))?;
    Duration::try_from(signed).map_err(|error| format!("{error}"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match fs::read_to_string(&cli.config) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
            eprintln!("bootvine: ignoring {}: {error}", cli.config.display());
            Config::default()
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Config::default(),
        Err(error) => {
            eprintln!("bootvine: cannot read {}: {error}", cli.config.display());
            Config::default()
        }
    };
    let options = Options {
        scope: if cli.user { Scope::User } else { Scope::System },
        shutdown: cli.shutdown,
        target: cli.target,
        hold: cli.hold,
        palette: config.palette,
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bootvine: {error}");
            ExitCode::FAILURE
        }
    }
}
