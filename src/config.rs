use std::time::Duration;

use serde::Deserialize;

use crate::palette::Palette;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub palette: Palette,
    pub timing: Timing,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    pub keep_finished_units_for: Duration,
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
    pub hide_runs_shorter_than: Duration,
    #[serde(with = "jiff::fmt::serde::unsigned_duration::friendly::compact::required")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_reads_friendly_durations_and_keeps_unset_defaults() {
        let config: Config = toml::from_str("[timing]\nkeep_finished_units_for = \"500ms\"\n")
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
        assert!(toml::from_str::<Config>("[timing]\nkeep_final_frame_for = \"soon\"\n").is_err());
    }
}
