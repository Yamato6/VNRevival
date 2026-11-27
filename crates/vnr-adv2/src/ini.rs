//! `OF_ADV2.INI`: engine settings, including the font and colour of every
//! speaker. The file is Shift-JIS, one `[OverflowADV2]` section.

use std::collections::HashMap;
use std::path::Path;

use vnr_core::{Color, FontFamily};

/// Parsed `OF_ADV2.INI` (keys are case-insensitive).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    values: HashMap<String, String>,
}

/// How one speaker's lines are drawn (`CharNN_*` keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharStyle {
    pub family: FontFamily,
    pub bold: bool,
    pub shadow: bool,
    pub color: Color,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            family: FontFamily::Mincho,
            bold: false,
            shadow: true,
            color: Color::rgb(240, 240, 232),
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let values = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        Self { values }
    }

    /// Reads and decodes the file. A missing file gives empty settings.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(bytes) = path.and_then(|p| std::fs::read(p).ok()) else {
            return Self::default();
        };
        let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&bytes);
        Self::parse(&text)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key)?.parse().ok()
    }

    fn byte(&self, key: &str, default: u8) -> u8 {
        self.int(key).map_or(default, |v| v.clamp(0, 255) as u8)
    }

    /// Colour of the message window (`MesWin_R/G/B`).
    pub fn message_window(&self) -> Color {
        Color::rgb(
            self.byte("MesWin_R", 0),
            self.byte("MesWin_G", 0),
            self.byte("MesWin_B", 150),
        )
    }

    /// Style of speaker `n` (`CharNN_*`).
    pub fn char_style(&self, n: usize) -> CharStyle {
        let key = |field: &str| format!("Char{n:02}_{field}");
        let default = CharStyle::default();
        let family = match self.get(&key("FontName")) {
            Some(name) if name.contains("ゴシック") => FontFamily::Gothic,
            Some(_) => FontFamily::Mincho,
            None => default.family,
        };
        CharStyle {
            family,
            bold: self.int(&key("FontBold")).map_or(default.bold, |v| v != 0),
            shadow: self
                .int(&key("FontShadow"))
                .map_or(default.shadow, |v| v != 0),
            color: Color::rgb(
                self.byte(&key("FontR"), default.color.r),
                self.byte(&key("FontG"), default.color.g),
                self.byte(&key("FontB"), default.color.b),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_speaker_styles() {
        let ini = Settings::parse(
            "[OverflowADV2]\nMesWin_B=120\nChar04_FontName=ＭＳ Ｐゴシック\nChar04_FontBold=0\n\
             Char04_FontR=255\nChar04_FontG=208\nchar04_fontb=112\n",
        );
        assert_eq!(ini.message_window(), Color::rgb(0, 0, 120));
        let style = ini.char_style(4);
        assert_eq!(style.family, FontFamily::Gothic);
        assert!(!style.bold);
        assert!(style.shadow);
        assert_eq!(style.color, Color::rgb(255, 208, 112));
        assert_eq!(ini.char_style(9), CharStyle::default());
    }
}
