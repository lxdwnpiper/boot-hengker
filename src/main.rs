mod clock;
mod console;
mod error;
mod feed;
mod password;
mod run;
mod sysroot;
mod systemd;

use std::{fs, io::ErrorKind, path::PathBuf, process::ExitCode, time::Duration};

use bootvine::config::{Config, Timing};
use clap::Parser;
use jiff::fmt::friendly::SpanParser;

use crate::{
    feed::RootRequest,
    run::{Settings, run},
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
        value_parser = |raw: &str| SpanParser::new().parse_unsigned_duration(raw),
        help = "How long the final frame stays on screen, e.g. 1s or 500ms; overrides \
                timing.keep_final_frame_for"
    )]
    hold: Option<Duration>,
    #[arg(
        long,
        default_value = "/etc/bootvine.toml",
        help = "TOML file with [palette] and [timing] tables"
    )]
    config: PathBuf,
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
    let settings = Settings {
        scope: if cli.user { Scope::User } else { Scope::System },
        shutdown: cli.shutdown,
        target: cli
            .target
            .map_or(RootRequest::ManagerDefault, RootRequest::Named),
        timing: Timing {
            keep_final_frame_for: cli.hold.unwrap_or(config.timing.keep_final_frame_for),
            ..config.timing
        },
        palette: config.palette,
    };
    match run(&settings) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bootvine: {error}");
            ExitCode::FAILURE
        }
    }
}
