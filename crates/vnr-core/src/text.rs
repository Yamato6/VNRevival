//! Text rendering with TrueType/OpenType fonts (including `.ttc`
//! collections such as the Windows MS Mincho / MS Gothic files, and
//! variable fonts such as `NotoSansJP-VF.ttf`, drawn at a normal weight).
//!
//! Japanese text has no spaces, so wrapping is per character, with the
//! usual rule that closing punctuation never starts a line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontVec, PxScale, VariableFont, point};
use serde::{Deserialize, Serialize};

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
    // The proportional variants the games use: ＭＳ Ｐ明朝 is face 1 of
    // msmincho.ttc, ＭＳ Ｐゴシック face 2 of msgothic.ttc.
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
            (windows.join("msgothic.ttc"), 2),
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

/// The font the player picked for the interface and the game text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontChoice {
    #[default]
    NotoSansJp,
    NotoSerifJp,
    BizUdGothic,
    BizUdMincho,
    YuGothic,
    Meiryo,
    /// What the game asked for: ＭＳ Ｐゴシック or ＭＳ Ｐ明朝 (or the
    /// closest installed font, see [`find_system_font`]).
    Original,
}

impl FontChoice {
    pub const ALL: [FontChoice; 7] = [
        FontChoice::NotoSansJp,
        FontChoice::NotoSerifJp,
        FontChoice::BizUdGothic,
        FontChoice::BizUdMincho,
        FontChoice::YuGothic,
        FontChoice::Meiryo,
        FontChoice::Original,
    ];

    /// Order tried when the chosen font is not installed.
    const FALLBACK: [FontChoice; 5] = [
        FontChoice::NotoSansJp,
        FontChoice::BizUdGothic,
        FontChoice::YuGothic,
        FontChoice::Meiryo,
        FontChoice::Original,
    ];

    /// The font's own name (the interface has its own word for
    /// [`FontChoice::Original`]).
    pub fn name(self) -> &'static str {
        match self {
            FontChoice::NotoSansJp => "Noto Sans JP",
            FontChoice::NotoSerifJp => "Noto Serif JP",
            FontChoice::BizUdGothic => "BIZ UDPGothic",
            FontChoice::BizUdMincho => "BIZ UDPMincho",
            FontChoice::YuGothic => "Yu Gothic",
            FontChoice::Meiryo => "Meiryo",
            FontChoice::Original => "Original",
        }
    }

    /// Known files of the font: a file name in a font folder, or an
    /// absolute path; and the face index.
    fn files(self) -> &'static [(&'static str, u32)] {
        match self {
            FontChoice::NotoSansJp => &[
                ("NotoSansJP-VF.ttf", 0),
                ("NotoSansJP-Regular.ttf", 0),
                ("NotoSansJP-Regular.otf", 0),
                ("NotoSansCJKjp-Regular.otf", 0),
                ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
                ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
                (
                    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                    0,
                ),
            ],
            FontChoice::NotoSerifJp => &[
                ("NotoSerifJP-VF.ttf", 0),
                ("NotoSerifJP-Regular.ttf", 0),
                ("NotoSerifJP-Regular.otf", 0),
                ("NotoSerifCJKjp-Regular.otf", 0),
                ("/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc", 0),
                ("/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc", 0),
                (
                    "/usr/share/fonts/google-noto-cjk/NotoSerifCJK-Regular.ttc",
                    0,
                ),
            ],
            // Face 1 of the collections is the proportional (P) variant.
            FontChoice::BizUdGothic => &[("BIZ-UDGothicR.ttc", 1), ("BIZUDPGothic-Regular.ttf", 0)],
            FontChoice::BizUdMincho => &[("BIZ-UDMinchoM.ttc", 1), ("BIZUDPMincho-Regular.ttf", 0)],
            FontChoice::YuGothic => &[("YuGothM.ttc", 0), ("YuGothR.ttc", 0)],
            FontChoice::Meiryo => &[("meiryo.ttc", 0)],
            FontChoice::Original => &[],
        }
    }

    /// Where the font is installed, if it is. For
    /// [`FontChoice::Original`], the system font of `family`.
    pub fn find(self, family: FontFamily) -> Option<(PathBuf, u32)> {
        if self == FontChoice::Original {
            return find_system_font(family);
        }
        let dirs = font_dirs();
        self.files().iter().find_map(|&(file, index)| {
            let path = Path::new(file);
            if path.is_absolute() {
                return path.is_file().then(|| (path.to_path_buf(), index));
            }
            dirs.iter()
                .map(|dir| dir.join(file))
                .find(|p| p.is_file())
                .map(|p| (p, index))
        })
    }

    /// The choices installed on this system ([`FontChoice::Original`] is
    /// always there).
    pub fn installed() -> Vec<FontChoice> {
        Self::ALL
            .into_iter()
            .filter(|c| *c == FontChoice::Original || c.find(FontFamily::Gothic).is_some())
            .collect()
    }

    /// The font file for text of `family`: this choice, or the first
    /// installed one in the fallback order.
    pub fn resolve(self, family: FontFamily) -> Option<(PathBuf, u32)> {
        std::iter::once(self)
            .chain(Self::FALLBACK)
            .find_map(|c| c.find(family))
            .or_else(|| find_system_font(FontFamily::Gothic))
            .or_else(|| find_system_font(FontFamily::Mincho))
    }

    /// Loads the font for text of `family` (see [`FontChoice::resolve`]).
    pub fn load(self, family: FontFamily) -> Result<Font, String> {
        let (path, index) = self
            .resolve(family)
            .ok_or("no Japanese font found on this system; pass one with --font")?;
        Font::load(&path, index)
    }
}

/// Folders where fonts are installed.
fn font_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.push(
            var("WINDIR")
                .map(|w| w.join("Fonts"))
                .unwrap_or_else(|| PathBuf::from("C:/Windows/Fonts")),
        );
        if let Some(local) = var("LOCALAPPDATA") {
            dirs.push(local.join("Microsoft/Windows/Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push("/Library/Fonts".into());
        dirs.push("/System/Library/Fonts".into());
        if let Some(home) = var("HOME") {
            dirs.push(home.join("Library/Fonts"));
        }
    } else {
        for dir in [
            "/usr/share/fonts/opentype/noto",
            "/usr/share/fonts/truetype/noto",
            "/usr/share/fonts/noto",
            "/usr/local/share/fonts",
        ] {
            dirs.push(dir.into());
        }
        if let Some(home) = var("HOME") {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

/// How to draw a piece of text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in pixels (the em square).
    pub size: f32,
    pub color: Color,
    /// Drop shadow one pixel down and right, or with `outline` a border
    /// all around.
    pub shadow: Option<Color>,
    /// Draw `shadow` as an outline instead of a drop shadow.
    pub outline: bool,
    /// Bold: the bold weight of a variable font, otherwise each glyph
    /// drawn twice one pixel apart.
    pub bold: bool,
}

impl TextStyle {
    /// Plain text of `size` and `color`.
    pub fn plain(size: f32, color: Color) -> Self {
        Self {
            size,
            color,
            shadow: None,
            outline: false,
            bold: false,
        }
    }
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
    regular: FontVec,
    /// The bold weight of a variable font.
    bold: Option<FontVec>,
    cache: HashMap<(char, u32, bool), Glyph>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("variable", &self.bold.is_some())
            .field("glyphs_cached", &self.cache.len())
            .finish()
    }
}

/// Characters that may not start a line (kinsoku).
const NO_LINE_START: &str =
    "、。，．・：；？！ー」』）］｝〕〉》】～…‥っゃゅょァィゥェォッャュョぁぃぅぇぉ!?,.)]";

/// Weights used with variable fonts.
const REGULAR_WEIGHT: f32 = 400.0;
const BOLD_WEIGHT: f32 = 700.0;

/// Sets the weight of a variable font; `false` if it has no weight axis.
fn set_weight(font: &mut FontVec, weight: f32) -> bool {
    let Some(axis) = font.variations().into_iter().find(|a| &a.tag == b"wght") else {
        return false;
    };
    font.set_variation(b"wght", weight.clamp(axis.min_value, axis.max_value))
}

impl Font {
    /// Loads a font file; `index` selects the face in a collection.
    pub fn load(path: &Path, index: u32) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(&bytes, index).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_bytes(bytes: &[u8], index: u32) -> Result<Self, String> {
        let open = || {
            FontVec::try_from_vec_and_index(bytes.to_vec(), index)
                .map_err(|_| "not a font file (or no such face)".to_string())
        };
        let mut regular = open()?;
        let bold = if set_weight(&mut regular, REGULAR_WEIGHT) {
            let mut bold = open()?;
            set_weight(&mut bold, BOLD_WEIGHT);
            Some(bold)
        } else {
            None
        };
        Ok(Self {
            regular,
            bold,
            cache: HashMap::new(),
        })
    }

    /// Whether bold text uses a real bold weight.
    pub fn has_bold_weight(&self) -> bool {
        self.bold.is_some()
    }

    fn units_per_em(&self) -> f32 {
        self.regular.units_per_em().unwrap_or(1000.0)
    }

    fn glyph(&mut self, c: char, size: f32, bold: bool) -> &Glyph {
        let bold = bold && self.bold.is_some();
        let font = match (&self.bold, bold) {
            (Some(b), true) => b,
            _ => &self.regular,
        };
        self.cache
            .entry((c, size.to_bits(), bold))
            .or_insert_with(|| rasterize(font, c, size))
    }

    /// Distance from the top of a line to its baseline.
    pub fn ascent(&self, size: f32) -> f32 {
        self.regular.ascent_unscaled() * size / self.units_per_em()
    }

    /// Width of a single line of text.
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|c| self.glyph(c, size, false).advance)
            .sum()
    }

    /// Splits text into lines no wider than `max_width`. `\n` forces a
    /// break.
    pub fn wrap(&mut self, text: &str, size: f32, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            let mut width = 0.0;
            for c in paragraph.chars() {
                let advance = self.glyph(c, size, false).advance;
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
        let fake_bold = style.bold && self.bold.is_none();
        let mut passes: Vec<(i32, i32, Color)> = Vec::new();
        if let Some(shadow) = style.shadow {
            if style.outline {
                let reach = if fake_bold { 2 } else { 1 };
                for oy in -1..=1 {
                    for ox in -1..=reach {
                        if (ox, oy) != (0, 0) {
                            passes.push((ox, oy, shadow));
                        }
                    }
                }
            } else {
                passes.push((1, 1, shadow));
                if fake_bold {
                    passes.push((2, 1, shadow));
                }
            }
        }
        passes.push((0, 0, style.color));
        if fake_bold {
            passes.push((1, 0, style.color));
        }
        let mut pen = x;
        for c in text.chars() {
            let glyph = self.glyph(c, style.size, style.bold);
            if let Some((w, h, mask)) = &glyph.mask {
                let gx = (pen + glyph.left as f32).round() as i32;
                let gy = (baseline - glyph.top as f32).round() as i32;
                for &(ox, oy, color) in &passes {
                    canvas.draw_mask(*w, *h, mask, gx + ox, gy + oy, color);
                }
            }
            pen += glyph.advance;
        }
        pen
    }
}

/// Rasterises one character at `size` pixels per em.
fn rasterize(font: &FontVec, c: char, size: f32) -> Glyph {
    let upem = font.units_per_em().unwrap_or(1000.0);
    let id = font.glyph_id(c);
    let advance = font.h_advance_unscaled(id) * size / upem;
    // ab_glyph scales by the font's height (ascent − descent), not the em.
    let scale = PxScale::from(size * font.height_unscaled() / upem);
    let empty = Glyph {
        mask: None,
        left: 0,
        top: 0,
        advance,
    };
    let Some(outlined) = font.outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0)))
    else {
        return empty;
    };
    let bounds = outlined.px_bounds();
    let (w, h) = (bounds.width() as u32, bounds.height() as u32);
    if w == 0 || h == 0 {
        return empty;
    }
    let mut mask = vec![0u8; (w * h) as usize];
    outlined.draw(|x, y, coverage| {
        if x < w && y < h {
            mask[(y * w + x) as usize] = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    });
    Glyph {
        mask: Some((w, h, mask)),
        left: bounds.min.x as i32,
        top: -bounds.min.y as i32,
        advance,
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
            shadow: Some(Color::BLACK),
            bold: true,
            ..TextStyle::plain(24.0, Color::WHITE)
        };
        let end = font.draw_line(&mut canvas, 2.0, 2.0, "字", style);
        assert!(end > 20.0);
        let lit = canvas.rgba().chunks_exact(4).filter(|p| p[0] > 200).count();
        assert!(lit > 20, "{lit} lit pixels");
    }

    /// Ink of `text` drawn in white on black.
    fn ink(font: &mut Font, text: &str, size: f32) -> u64 {
        let mut canvas = Canvas::new(400, 60);
        font.draw_line(
            &mut canvas,
            0.0,
            0.0,
            text,
            TextStyle::plain(size, Color::WHITE),
        );
        canvas.rgba().chunks_exact(4).map(|p| u64::from(p[0])).sum()
    }

    #[test]
    fn variable_fonts_draw_at_a_normal_weight() {
        let Some((path, index)) = FontChoice::NotoSansJp.find(FontFamily::Gothic) else {
            eprintln!("Noto Sans JP not installed, skipping");
            return;
        };
        let Some((gothic, gothic_index)) = FontChoice::Original.find(FontFamily::Gothic) else {
            eprintln!("no gothic font, skipping");
            return;
        };
        let mut noto = Font::load(&path, index).unwrap();
        let mut original = Font::load(&gothic, gothic_index).unwrap();
        let text = "永遠の字。あいう";
        let (a, b) = (ink(&mut noto, text, 24.0), ink(&mut original, text, 24.0));
        // A thin (100) weight has well under half the ink of a regular one.
        assert!(a * 10 > b * 7, "Noto {a} vs original {b}");
        if noto.has_bold_weight() {
            let mut bold = TextStyle::plain(24.0, Color::WHITE);
            bold.bold = true;
            let mut canvas = Canvas::new(400, 60);
            noto.draw_line(&mut canvas, 0.0, 0.0, text, bold);
            let heavy: u64 = canvas.rgba().chunks_exact(4).map(|p| u64::from(p[0])).sum();
            assert!(heavy > a * 11 / 10, "bold {heavy} vs regular {a}");
        }
        if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
            let mut canvas = Canvas::new(640, 40 * FontChoice::ALL.len() as u32);
            canvas.clear(Color::rgb(20, 24, 40));
            for (i, choice) in FontChoice::ALL.into_iter().enumerate() {
                let Ok(mut font) = choice.load(FontFamily::Gothic) else {
                    continue;
                };
                let style = TextStyle {
                    shadow: Some(Color::BLACK),
                    ..TextStyle::plain(24.0, Color::WHITE)
                };
                let line = format!("{}: 永遠の字。Pure Mail", choice.name());
                font.draw_line(&mut canvas, 8.0, 4.0 + 40.0 * i as f32, &line, style);
            }
            canvas.save_png(&Path::new(&out).join("fonts.png")).unwrap();
        }
    }

    #[test]
    fn font_choices_resolve_to_something_installed() {
        let installed = FontChoice::installed();
        assert!(installed.contains(&FontChoice::Original));
        if find_system_font(FontFamily::Gothic).is_none() {
            eprintln!("no Japanese font installed, skipping");
            return;
        }
        // Every choice gives a font, installed or not (falling back).
        for choice in FontChoice::ALL {
            let (path, _) = choice.resolve(FontFamily::Mincho).unwrap();
            assert!(path.is_file(), "{choice:?}");
            if installed.contains(&choice) && choice != FontChoice::Original {
                assert_eq!(
                    choice.resolve(FontFamily::Gothic),
                    choice.find(FontFamily::Gothic)
                );
            }
        }
    }
}
