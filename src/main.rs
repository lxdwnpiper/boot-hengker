use std::{
    error::Error,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use bootvine::{
    console_session::watch_loop::{Settings, run},
    screen_settings::config_file::{
        Config, ConfigError, Timing, default_config_table, read_config_file,
    },
    systemd_link::{boot_feed::RootRequest, systemd_bus::Scope},
};
use clap::{Parser, Subcommand, ValueEnum};
use jiff::fmt::friendly::SpanParser;

#[derive(Parser)]
#[command(
    version,
    about = "Live dependency tree of systemd jobs while the system boots",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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
                timing.keep-final-frame-for"
    )]
    hold: Option<Duration>,
    #[arg(
        long,
        default_value = "/etc/bootvine.toml",
        help = "TOML file with [colors] and [timing] tables; see `bootvine defaults`"
    )]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Print every setting with its default value")]
    Defaults {
        #[arg(long, value_enum, default_value_t = DefaultsFormat::Toml)]
        format: DefaultsFormat,
    },
    #[command(about = "Exit with an error if a config file is unreadable or invalid")]
    Check { path: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum DefaultsFormat {
    Toml,
    Nix,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Defaults { format }) => print_defaults(format),
        Some(Command::Check { ref path }) => check_config_file(path),
        None => watch_boot(cli),
    }
}

fn render_defaults(format: DefaultsFormat) -> Result<String, Box<dyn Error>> {
    let defaults = default_config_table()?;
    Ok(match format {
        DefaultsFormat::Toml => toml::to_string_pretty(&defaults)?,
        DefaultsFormat::Nix => ser_nix::to_string(&defaults)?,
    })
}

fn print_defaults(format: DefaultsFormat) -> ExitCode {
    match render_defaults(format) {
        Ok(text) => {
            println!("{}", text.trim_end());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bootvine: {error}");
            ExitCode::FAILURE
        }
    }
}

fn check_config_file(path: &Path) -> ExitCode {
    match read_config_file(path) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bootvine: {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn watch_boot(cli: Cli) -> ExitCode {
    let config = match read_config_file(&cli.config) {
        Ok(config) => config,
        Err(ConfigError::Unreadable(error)) if error.kind() == ErrorKind::NotFound => {
            Config::default()
        }
        Err(error) => {
            eprintln!("bootvine: ignoring {}: {error}", cli.config.display());
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
        colors: config.colors,
    };
    match run(&settings) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bootvine: {error}");
            ExitCode::FAILURE
        }
    }
}
