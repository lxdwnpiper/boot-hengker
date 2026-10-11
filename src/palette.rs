use ratatui::style::Color;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub text: Color,
    pub muted: Color,
    pub waiting: Color,
    pub running: Color,
    pub done: Color,
    pub failed: Color,
    pub target: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            text: Color::White,
            muted: Color::DarkGray,
            waiting: Color::Gray,
            running: Color::Yellow,
            done: Color::Green,
            failed: Color::LightRed,
            target: Color::Cyan,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub palette: Palette,
}
