//! The player's settings, shared by every game: `settings.json` in the
//! user data folder (see [`crate::user_data_dir`]).

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::audio::FULL_VOLUME;
use crate::lang::Language;
use crate::text::FontChoice;

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

/// How text is set off from the picture behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outline {
    /// No shadow at all.
    Off,
    /// A drop shadow where the game has one.
    #[default]
    Shadow,
    /// A dark border all around every character.
    Outline,
}

/// Highest step of [`Settings::text_size`] and [`Settings::line_spacing`].
pub const MAX_STEP: u8 = 4;

/// Game text size in pixels (of an 800 × 600 game) per step.
const TEXT_SIZES: [f32; 5] = [18.0, 20.0, 22.0, 26.0, 30.0];
/// Line height as a multiple of the text size, per step.
const LINE_SPACINGS: [f32; 5] = [1.2, 1.28, 1.36, 1.55, 1.8];

/// The settings. Levels go from 0 to [`MAX_LEVEL`], steps to
/// [`MAX_STEP`].
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
    /// Font of VNRevival's own screens.
    pub font: FontChoice,
    /// Font of the game text (messages, choices, the history).
    pub game_font: FontChoice,
    /// Size of the game text (step).
    pub text_size: u8,
    /// Space between lines (step).
    pub line_spacing: u8,
    pub outline: Outline,
    /// Opaque black message window with white text.
    pub high_contrast: bool,
    /// How fast auto mode moves on.
    pub auto_speed: u8,
    /// Auto mode waits for the voice to end.
    pub wait_voice: bool,
    /// Skipping (Ctrl) also skips text not read yet.
    pub skip_unread: bool,
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
            font: FontChoice::default(),
            game_font: FontChoice::default(),
            text_size: 2,
            line_spacing: 2,
            outline: Outline::default(),
            high_contrast: false,
            auto_speed: 5,
            wait_voice: true,
            skip_unread: false,
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

    /// Message window alpha (opaque in high contrast).
    pub fn window_alpha(&self) -> u8 {
        if self.high_contrast {
            return 255;
        }
        (u32::from(self.window.min(MAX_LEVEL)) * 255 / u32::from(MAX_LEVEL)) as u8
    }

    /// Game text size in pixels, for a game drawn at 800 × 600.
    pub fn text_px(&self) -> f32 {
        TEXT_SIZES[usize::from(self.text_size.min(MAX_STEP))]
    }

    /// Line height for text of `size` pixels.
    pub fn line_height(&self, size: f32) -> f32 {
        (size * LINE_SPACINGS[usize::from(self.line_spacing.min(MAX_STEP))]).round()
    }

    /// Frames auto mode waits once a message of `chars` characters is
    /// fully shown (at 60 frames per second).
    pub fn auto_wait(&self, chars: usize) -> u64 {
        let fast = f32::from(self.auto_speed.min(MAX_LEVEL));
        let base = 2.5 - fast * 0.2;
        let per_char = 0.09 - fast * 0.006;
        ((base + per_char * chars as f32) * 60.0).round() as u64
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
        // Files from before the accessibility settings keep the defaults.
        assert_eq!(partial.font, FontChoice::NotoSansJp);
        assert_eq!(partial.game_font, FontChoice::NotoSansJp);
        assert_eq!((partial.text_px(), partial.line_height(22.0)), (22.0, 30.0));
        assert_eq!(partial.outline, Outline::Shadow);
        assert!(partial.wait_voice && !partial.skip_unread);
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

    #[test]
    fn text_and_reading_helpers() {
        let mut s = Settings {
            text_size: 9,
            ..Settings::default()
        };
        assert_eq!(s.text_px(), 30.0);
        s.high_contrast = true;
        assert_eq!(s.window_alpha(), 255);
        // Faster auto mode waits less; longer messages wait more.
        let slow = Settings {
            auto_speed: 0,
            ..Settings::default()
        };
        let fast = Settings {
            auto_speed: MAX_LEVEL,
            ..Settings::default()
        };
        assert!(slow.auto_wait(20) > fast.auto_wait(20));
        assert!(fast.auto_wait(60) > fast.auto_wait(10));
        assert!(fast.auto_wait(0) >= 30);
    }
}
