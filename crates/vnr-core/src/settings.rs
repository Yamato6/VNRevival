//! The player's settings, shared by every game: `settings.json` in the
//! user data folder (see [`crate::user_data_dir`]).

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::audio::FULL_VOLUME;
use crate::lang::Language;

/// Highest level of the level settings.
pub const MAX_LEVEL: u8 = 10;

/// What the program shows when a game starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartScreen {
    /// VNRevival's main menu.
    #[default]
    Universal,
    /// The game's own start-up and title screen.
    Original,
}

/// The settings. Levels go from 0 to [`MAX_LEVEL`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub start: StartScreen,
    pub fullscreen: bool,
    /// Show every picture, track and scene in the extras, seen or not.
    pub unlock_all: bool,
    pub master: u8,
    pub music: u8,
    pub sound: u8,
    pub voice: u8,
    /// Text speed; [`MAX_LEVEL`] shows messages at once.
    pub text_speed: u8,
    /// Opacity of the message window.
    pub window: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            start: StartScreen::default(),
            fullscreen: false,
            unlock_all: false,
            master: MAX_LEVEL,
            music: 8,
            sound: 8,
            voice: MAX_LEVEL,
            text_speed: 5,
            window: 6,
        }
    }
}

impl Settings {
    /// Characters per second, or `None` for instant text.
    pub fn chars_per_second(&self) -> Option<f32> {
        (self.text_speed < MAX_LEVEL).then(|| 15.0 + f32::from(self.text_speed) * 15.0)
    }

    /// A level as a mixer volume.
    pub fn volume(level: u8) -> u16 {
        (u32::from(FULL_VOLUME) * u32::from(level.min(MAX_LEVEL)) / u32::from(MAX_LEVEL)) as u16
    }

    /// Message window alpha.
    pub fn window_alpha(&self) -> u8 {
        (u32::from(self.window.min(MAX_LEVEL)) * 255 / u32::from(MAX_LEVEL)) as u8
    }

    /// Reads the settings; defaults if the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    pub fn store(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_missing_fields() {
        let dir = std::env::temp_dir().join(format!("vnr-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = Settings {
            unlock_all: true,
            music: 3,
            language: Language::Spanish,
            ..Settings::default()
        };
        settings.store(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::write(&path, br#"{"music": 2}"#).unwrap();
        let partial = Settings::load(&path);
        assert_eq!(partial.music, 2);
        assert_eq!(partial.voice, MAX_LEVEL);
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn levels() {
        assert_eq!(Settings::volume(MAX_LEVEL), FULL_VOLUME);
        assert_eq!(Settings::volume(0), 0);
        let instant = Settings {
            text_speed: MAX_LEVEL,
            ..Settings::default()
        };
        assert!(instant.chars_per_second().is_none());
        assert!(Settings::default().chars_per_second().unwrap() > 0.0);
    }
}
