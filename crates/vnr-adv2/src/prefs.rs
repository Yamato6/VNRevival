//! Player settings: text speed, volumes and message window opacity.
//!
//! They are VNRevival's own, kept as `settings.json` next to the saves;
//! the original's `OF_ADV2.INI` is only read for the look of the text.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::names::Names;
use vnr_core::{Canvas, Color, FULL_VOLUME, Font, InputEvent, Key, MouseButton, TextStyle};

/// Highest level of every setting.
pub const MAX: u8 = 10;

/// The settings, each a level from 0 to [`MAX`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Text speed; [`MAX`] shows messages at once.
    pub text_speed: u8,
    pub music: u8,
    pub sound: u8,
    pub voice: u8,
    /// Opacity of the message window.
    pub window: u8,
    /// The hero's names for new games (VNRevival cannot type Japanese
    /// in a dialog yet, so they are set here).
    pub names: Names,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            text_speed: 5,
            music: 8,
            sound: 8,
            voice: MAX,
            window: 6,
            names: Names::default(),
        }
    }
}

impl Preferences {
    /// Characters per second, or `None` for instant text.
    pub fn chars_per_second(&self) -> Option<f32> {
        (self.text_speed < MAX).then(|| 15.0 + f32::from(self.text_speed) * 15.0)
    }

    /// A level as a mixer volume.
    pub fn volume(level: u8) -> u16 {
        (u32::from(FULL_VOLUME) * u32::from(level.min(MAX)) / u32::from(MAX)) as u16
    }

    /// Message window alpha.
    pub fn window_alpha(&self) -> u8 {
        (u32::from(self.window.min(MAX)) * 255 / u32::from(MAX)) as u8
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

    fn level(&self, row: usize) -> u8 {
        match row {
            0 => self.text_speed,
            1 => self.music,
            2 => self.sound,
            3 => self.voice,
            _ => self.window,
        }
    }

    fn level_mut(&mut self, row: usize) -> &mut u8 {
        match row {
            0 => &mut self.text_speed,
            1 => &mut self.music,
            2 => &mut self.sound,
            3 => &mut self.voice,
            _ => &mut self.window,
        }
    }
}

const LABELS: [&str; 5] = ["文字速度", "BGM", "効果音", "ボイス", "ウィンドウ"];
const PANEL: (i32, i32, u32, u32) = (140, 120, 520, 360);
const BAR_LEFT: i32 = 330;
const STEP: i32 = 28;

fn row_top(row: usize) -> i32 {
    PANEL.1 + 70 + row as i32 * 50
}

/// The settings screen.
pub struct View {
    pub prefs: Preferences,
    selected: usize,
}

impl View {
    pub fn new(prefs: Preferences) -> Self {
        Self { prefs, selected: 0 }
    }

    /// Handles input; returns `true` when the screen should close.
    pub fn input(&mut self, events: &[InputEvent]) -> bool {
        for event in events {
            match *event {
                InputEvent::KeyDown(Key::Escape | Key::F(4))
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => return true,
                InputEvent::KeyDown(Key::Up) => {
                    self.selected = (self.selected + LABELS.len() - 1) % LABELS.len();
                }
                InputEvent::KeyDown(Key::Down) => {
                    self.selected = (self.selected + 1) % LABELS.len();
                }
                InputEvent::KeyDown(Key::Left) => {
                    let level = self.prefs.level_mut(self.selected);
                    *level = level.saturating_sub(1);
                }
                InputEvent::KeyDown(Key::Right) => {
                    let level = self.prefs.level_mut(self.selected);
                    *level = (*level + 1).min(MAX);
                }
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    let (px, py, pw, ph) = PANEL;
                    if x < px || y < py || x >= px + pw as i32 || y >= py + ph as i32 {
                        return true;
                    }
                    if let Some(row) =
                        (0..LABELS.len()).find(|&r| (row_top(r)..row_top(r) + 40).contains(&y))
                    {
                        self.selected = row;
                        if x >= BAR_LEFT - STEP / 2 {
                            let level = ((x - BAR_LEFT + STEP / 2) / STEP).clamp(0, i32::from(MAX));
                            *self.prefs.level_mut(row) = level as u8;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub fn draw(&self, canvas: &mut Canvas, font: &mut Font) {
        canvas.fill_rect(
            0,
            0,
            canvas.width(),
            canvas.height(),
            Color::rgba(0, 0, 0, 140),
        );
        let (px, py, pw, ph) = PANEL;
        canvas.fill_rect(px, py, pw, ph, Color::rgba(20, 30, 60, 250));
        let style = |size: f32, color: Color| TextStyle {
            size,
            color,
            shadow: Some(Color::BLACK),
            bold: false,
        };
        font.draw_line(
            canvas,
            (px + 20) as f32,
            (py + 14) as f32,
            "設定",
            style(22.0, Color::WHITE),
        );
        for (row, label) in LABELS.iter().enumerate() {
            let top = row_top(row);
            if row == self.selected {
                canvas.fill_rect(
                    px + 10,
                    top - 4,
                    pw - 20,
                    40,
                    Color::rgba(255, 120, 160, 60),
                );
            }
            font.draw_line(
                canvas,
                (px + 30) as f32,
                (top + 6) as f32,
                label,
                style(18.0, Color::WHITE),
            );
            let level = self.prefs.level(row);
            for step in 0..=MAX {
                let x = BAR_LEFT + i32::from(step) * STEP - 5;
                let color = if step <= level {
                    Color::rgb(255, 150, 180)
                } else {
                    Color::rgba(255, 255, 255, 50)
                };
                let h = 10 + u32::from(step) * 2;
                canvas.fill_rect(x, top + 30 - h as i32, 10, h, color);
            }
        }
        let speed = match self.prefs.chars_per_second() {
            Some(cps) => format!("{cps:.0} 文字/秒"),
            None => "瞬間".into(),
        };
        font.draw_line(
            canvas,
            (px + 30) as f32,
            (py + ph as i32 - 30) as f32,
            &format!("文字速度: {speed}　　Esc / 右クリックで閉じる"),
            style(14.0, Color::rgb(200, 210, 230)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_map_to_engine_values() {
        let mut prefs = Preferences::default();
        assert_eq!(Preferences::volume(MAX), FULL_VOLUME);
        assert_eq!(Preferences::volume(0), 0);
        prefs.text_speed = MAX;
        assert_eq!(prefs.chars_per_second(), None);
        prefs.text_speed = 0;
        assert_eq!(prefs.chars_per_second(), Some(15.0));
        prefs.window = MAX;
        assert_eq!(prefs.window_alpha(), 255);
    }

    #[test]
    fn keys_and_clicks_change_levels() {
        let mut view = View::new(Preferences::default());
        assert!(!view.input(&[InputEvent::KeyDown(Key::Right)]));
        assert_eq!(view.prefs.text_speed, 6);
        // Click the lowest step of the music bar.
        let click = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: BAR_LEFT,
            y: row_top(1) + 10,
        };
        assert!(!view.input(&[click]));
        assert_eq!(view.prefs.music, 0);
        assert!(view.input(&[InputEvent::KeyDown(Key::Escape)]));
    }

    #[test]
    fn missing_or_partial_files_use_defaults() {
        assert_eq!(
            Preferences::load(Path::new("/nonexistent/settings.json")),
            Preferences::default()
        );
        let partial: Preferences = serde_json::from_str(r#"{"music": 3}"#).unwrap();
        assert_eq!(partial.music, 3);
        assert_eq!(partial.voice, Preferences::default().voice);
    }
}
