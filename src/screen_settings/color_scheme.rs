use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Colors {
    pub header: HeaderColors,
    pub stages: StageColors,
    pub tree: TreeColors,
    pub timeline: TimelineColors,
    pub footer: FooterColors,
    pub password: PasswordColors,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct HeaderColors {
    pub origin: Color,
    pub separators: Color,
    pub total_time: Color,
}

impl Default for HeaderColors {
    fn default() -> Self {
        Self {
            origin: Color::White,
            separators: Color::DarkGray,
            total_time: Color::White,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct StageColors {
    pub branches: Color,
    pub finished_label: Color,
    pub finished_time: Color,
    pub unit_count: Color,
    pub failed_count: Color,
    pub current: Color,
}

impl Default for StageColors {
    fn default() -> Self {
        Self {
            branches: Color::DarkGray,
            finished_label: Color::Gray,
            finished_time: Color::Green,
            unit_count: Color::Gray,
            failed_count: Color::LightRed,
            current: Color::White,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TreeColors {
    pub branches: Color,
    pub columns: Color,
    pub target: Color,
    pub current_stage: Color,
    pub reached_stage: Color,
    pub running_unit: Color,
    pub running_marker: Color,
    pub waiting_unit: Color,
    pub finished_unit: Color,
    pub finished_marker: Color,
    pub failed_unit: Color,
    pub overflow: Color,
}

impl Default for TreeColors {
    fn default() -> Self {
        Self {
            branches: Color::DarkGray,
            columns: Color::DarkGray,
            target: Color::Cyan,
            current_stage: Color::Yellow,
            reached_stage: Color::Green,
            running_unit: Color::White,
            running_marker: Color::Yellow,
            waiting_unit: Color::Gray,
            finished_unit: Color::DarkGray,
            finished_marker: Color::Green,
            failed_unit: Color::LightRed,
            overflow: Color::Gray,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TimelineColors {
    pub rule: Color,
    pub title: Color,
    pub axis: Color,
    pub milestone: Color,
    pub finished: Color,
    pub finished_label: Color,
    pub failed: Color,
    pub queued_bar: Color,
    pub running: Color,
    pub running_label: Color,
}

impl Default for TimelineColors {
    fn default() -> Self {
        Self {
            rule: Color::DarkGray,
            title: Color::Gray,
            axis: Color::DarkGray,
            milestone: Color::Green,
            finished: Color::Green,
            finished_label: Color::Gray,
            failed: Color::LightRed,
            queued_bar: Color::Gray,
            running: Color::Yellow,
            running_label: Color::White,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FooterColors {
    pub rule: Color,
    pub done: Color,
    pub running: Color,
    pub waiting: Color,
    pub failed: Color,
    pub no_failures: Color,
}

impl Default for FooterColors {
    fn default() -> Self {
        Self {
            rule: Color::DarkGray,
            done: Color::Green,
            running: Color::Yellow,
            waiting: Color::Gray,
            failed: Color::LightRed,
            no_failures: Color::Gray,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct PasswordColors {
    pub frame: Color,
    pub message: Color,
    pub hint: Color,
    pub checking: Color,
    pub rejected: Color,
    pub backdrop: Color,
}

impl Default for PasswordColors {
    fn default() -> Self {
        Self {
            frame: Color::Yellow,
            message: Color::White,
            hint: Color::DarkGray,
            checking: Color::Gray,
            rejected: Color::LightRed,
            backdrop: Color::DarkGray,
        }
    }
}
