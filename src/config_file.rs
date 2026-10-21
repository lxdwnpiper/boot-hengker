use std::{fs, io, path::Path, time::Duration};

use heck::ToKebabCase;
use jiff::fmt::serde::unsigned_duration::friendly::compact::required as friendly_duration;
use serde::{Deserialize, Serialize};
use toml::{Table, Value, de::Error as ParseError, ser::Error as SerializeError};

use crate::color_scheme::Colors;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    pub colors: Colors,
    pub timing: Timing,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Timing {
    #[serde(with = "friendly_duration")]
    pub keep_finished_units_for: Duration,
    #[serde(with = "friendly_duration")]
    pub hide_runs_shorter_than: Duration,
    #[serde(with = "friendly_duration")]
    pub keep_final_frame_for: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            keep_finished_units_for: Duration::from_secs(2),
            hide_runs_shorter_than: Duration::from_secs(1),
            keep_final_frame_for: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read: {0}")]
    Unreadable(#[from] io::Error),
    #[error("invalid: {0}")]
    Invalid(#[from] ParseError),
}

pub fn read_config_file(path: &Path) -> Result<Config, ConfigError> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

pub fn default_config_table() -> Result<Table, SerializeError> {
    Table::try_from(Config::default()).map(kebab_color_names)
}

fn kebab_color_names(table: Table) -> Table {
    table
        .into_iter()
        .map(|(key, value)| {
            let value = match value {
                Value::Table(nested) => Value::Table(kebab_color_names(nested)),
                Value::String(name) if name.starts_with(char::is_alphabetic) => {
                    Value::String(name.to_kebab_case())
                }
                other => other,
            };
            (key, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed_defaults() -> String {
        toml::to_string_pretty(&default_config_table().expect("serializable defaults"))
            .expect("printable defaults")
    }

    #[test]
    fn timing_reads_friendly_durations_and_keeps_unset_defaults() {
        let config: Config = toml::from_str("[timing]\nkeep-finished-units-for = \"500ms\"\n")
            .expect("valid config");
        assert_eq!(
            config.timing.keep_finished_units_for,
            Duration::from_millis(500)
        );
        assert_eq!(
            config.timing.keep_final_frame_for,
            Timing::default().keep_final_frame_for
        );
    }

    #[test]
    fn nonsense_durations_are_rejected() {
        assert!(toml::from_str::<Config>("[timing]\nkeep-final-frame-for = \"soon\"\n").is_err());
    }

    #[test]
    fn unknown_color_keys_are_rejected() {
        assert!(toml::from_str::<Config>("[colors.tree]\nbogus = \"red\"\n").is_err());
    }

    #[test]
    fn printed_defaults_read_back_unchanged() {
        let printed = printed_defaults();
        assert!(printed.contains("failed-unit = \"light-red\""), "{printed}");
        assert_eq!(
            toml::from_str::<Config>(&printed).expect("valid defaults"),
            Config::default()
        );
    }
}
