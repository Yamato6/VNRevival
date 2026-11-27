//! Text rendering with TrueType/OpenType fonts (including `.ttc`
//! collections such as the Windows MS Mincho / MS Gothic files).
//!
//! Japanese text has no spaces, so wrapping is per character, with the
//! usual rule that closing punctuation never starts a line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::gfx::{Canvas, Color};

/// Which family a game asks for. The originals name Windows fonts
/// (`ＭＳ Ｐ明朝`, `ＭＳ Ｐゴシック`); [`find_system_font`] maps them to what
/// the current system has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Mincho,
    Gothic,
}

/// Looks for an installed Japanese font of the given family. Returns the
/// path and the index inside a font collection.
pub fn find_system_font(family: FontFamily) -> Option<(PathBuf, u32)> {
    let windows = std::env::var_os("WINDIR")
        .map(|w| PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|| PathBuf::from("C:/Windows/Fonts"));
    // Index 1 in msmincho.ttc / msgothic.ttc is the proportional variant
    // (ＭＳ Ｐ明朝 / ＭＳ Ｐゴシック) the games use.
    let candidates: Vec<(PathBuf, u32)> = match family {
        FontFamily::Mincho => vec![
            (windows.join("msmincho.ttc"), 1),
            (windows.join("YuMinR.ttf"), 0),
            (windows.join("BIZ-UDMinchoM.ttc"), 0),
            (
                "/usr/share/fonts/opentype/ipafont-mincho/ipamp.ttf".into(),
                0,
            ),
            (
                "/usr/share/fonts/truetype/fonts-japanese-mincho.ttf".into(),
                0,
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc".into(),
                0,
            ),
            (
                "/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc".into(),
                0,
            ),
            ("/System/Library/Fonts/ヒラギノ明朝 ProN.ttc".into(), 0),
        ],
        FontFamily::Gothic => vec![
            (windows.join("msgothic.ttc"), 1),
            (windows.join("YuGothR.ttc"), 0),
            (windows.join("meiryo.ttc"), 0),
            (
                "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf".into(),
                0,
            ),
            (
                "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf".into(),
                0,
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(),
                0,
            ),
            (
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".into(),
                0,
            ),
            ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc".into(), 0),
        ],
    };
    candidates.into_iter().find(|(path, _)| path.is_file())
}

/// How to draw a piece of text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in pixels.
    pub size: f32,
    pub color: Color,
    /// Drop shadow one pixel down and right.
    pub shadow: Option<Color>,
    /// Fake bold (each glyph drawn twice, one pixel apart).
    pub bold: bool,
}

struct Glyph {
    /// Coverage mask: width, height, bytes.
    mask: Option<(u32, u32, Vec<u8>)>,
    left: i32,
    /// Distance from the baseline up to the top of the bitmap.
    top: i32,
    advance: f32,
}

/// A loaded font with a cache of rasterised glyphs.
pub struct Font {
    font: fontdue::Font,
    cache: HashMap<(char, u32), Glyph>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("glyphs_cached", &self.cache.len())
            .finish()
    }
}

/// Characters that may not start a line (kinsoku).
const NO_LINE_START: &str =
    "、。，．・：；？！ー」』）］｝〕〉》】～…‥っゃゅょァィゥェォッャュョぁぃぅぇぉ!?,.)]";

impl Font {
    /// Loads a font file; `index` selects the face in a collection.
    pub fn load(path: &Path, index: u32) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(&bytes, index).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_bytes(bytes: &[u8], index: u32) -> Result<Self, String> {
        let settings = fontdue::FontSettings {
            collection_index: index,
            scale: 40.0,
            ..Default::default()
        };
        let font = fontdue::Font::from_bytes(bytes, settings).map_err(str::to_owned)?;
        Ok(Self {
            font,
            cache: HashMap::new(),
        })
    }

    fn glyph(&mut self, c: char, size: f32) -> &Glyph {
        let font = &self.font;
        self.cache.entry((c, size.to_bits())).or_insert_with(|| {
            let (m, coverage) = font.rasterize(c, size);
            let mask = (m.width > 0 && m.height > 0).then_some((
                m.width as u32,
                m.height as u32,
                coverage,
            ));
            Glyph {
                mask,
                left: m.xmin,
                top: m.ymin + m.height as i32,
                advance: m.advance_width,
            }
        })
    }

    /// Distance from the top of a line to its baseline.
    pub fn ascent(&self, size: f32) -> f32 {
        self.font
            .horizontal_line_metrics(size)
            .map_or(size * 0.88, |m| m.ascent)
    }

    /// Width of a single line of text.
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.glyph(c, size).advance).sum()
    }

    /// Splits text into lines no wider than `max_width`. `\n` forces a
    /// break.
    pub fn wrap(&mut self, text: &str, size: f32, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            let mut width = 0.0;
            for c in paragraph.chars() {
                let advance = self.glyph(c, size).advance;
                if width + advance > max_width && !line.is_empty() && !NO_LINE_START.contains(c) {
                    lines.push(std::mem::take(&mut line));
                    width = 0.0;
                }
                line.push(c);
                width += advance;
            }
            lines.push(line);
        }
        lines
    }

    /// Draws one line with its top-left corner at `(x, y)`; returns the
    /// pen position after the last character.
    pub fn draw_line(
        &mut self,
        canvas: &mut Canvas,
        x: f32,
        y: f32,
        text: &str,
        style: TextStyle,
    ) -> f32 {
        let baseline = y + self.ascent(style.size);
        let mut pen = x;
        for c in text.chars() {
            let glyph = self.glyph(c, style.size);
            let advance = glyph.advance;
            if let Some((w, h, mask)) = &glyph.mask {
                let gx = (pen + glyph.left as f32).round() as i32;
                let gy = (baseline - glyph.top as f32).round() as i32;
                let mut passes = vec![];
                if let Some(shadow) = style.shadow {
                    passes.push((1, 1, shadow));
                    if style.bold {
                        passes.push((2, 1, shadow));
                    }
                }
                passes.push((0, 0, style.color));
                if style.bold {
                    passes.push((1, 0, style.color));
                }
                for (ox, oy, color) in passes {
                    canvas.draw_mask(*w, *h, mask, gx + ox, gy + oy, color);
                }
            }
            pen += advance;
        }
        pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that need a real font use whatever the system has and skip
    /// otherwise (CI runners have no Japanese fonts).
    fn any_font() -> Option<Font> {
        let (path, index) = find_system_font(FontFamily::Gothic)
            .or_else(|| find_system_font(FontFamily::Mincho))?;
        Font::load(&path, index).ok()
    }

    #[test]
    fn wraps_japanese_text() {
        let Some(mut font) = any_font() else {
            eprintln!("no Japanese font installed, skipping");
            return;
        };
        let size = 20.0;
        let one = font.measure("あ", size);
        let lines = font.wrap("ああああ。あ\nい", size, one * 4.5);
        // The full stop may not start a line, so it stays on the first.
        assert_eq!(lines, ["ああああ。", "あ", "い"]);
    }

    #[test]
    fn draws_visible_text() {
        let Some(mut font) = any_font() else {
            eprintln!("no Japanese font installed, skipping");
            return;
        };
        let mut canvas = Canvas::new(64, 32);
        let style = TextStyle {
            size: 24.0,
            color: Color::WHITE,
            shadow: Some(Color::BLACK),
            bold: true,
        };
        let end = font.draw_line(&mut canvas, 2.0, 2.0, "字", style);
        assert!(end > 20.0);
        let lit = canvas.rgba().chunks_exact(4).filter(|p| p[0] > 200).count();
        assert!(lit > 20, "{lit} lit pixels");
    }
}
