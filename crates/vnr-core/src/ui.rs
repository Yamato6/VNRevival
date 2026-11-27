//! Drawing helpers of the universal interface: one look for every game
//! (dark panels with a blue border), in one place.

use crate::gfx::{Canvas, Color};
use crate::text::{Font, TextStyle};

/// Size of the interface.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Colours of the interface.
pub mod theme {
    use crate::gfx::Color;

    pub const BACKDROP: Color = Color::rgb(10, 14, 28);
    pub const DIM: Color = Color::rgba(0, 0, 0, 150);
    pub const PANEL: Color = Color::rgba(16, 24, 48, 235);
    pub const BORDER: Color = Color::rgb(70, 130, 220);
    pub const ITEM: Color = Color::rgba(255, 255, 255, 18);
    pub const HOVER: Color = Color::rgba(90, 160, 255, 90);
    pub const ACTIVE: Color = Color::rgba(90, 160, 255, 150);
    pub const TEXT: Color = Color::rgb(235, 240, 250);
    pub const MUTED: Color = Color::rgb(150, 165, 195);
    pub const DISABLED: Color = Color::rgb(90, 100, 125);
    pub const ACCENT: Color = Color::rgb(120, 190, 255);
    pub const ERROR: Color = Color::rgb(255, 140, 140);
}

/// Text sizes.
pub const TITLE_SIZE: f32 = 30.0;
pub const TEXT_SIZE: f32 = 20.0;
pub const SMALL_SIZE: f32 = 16.0;

/// A rectangle in interface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, (x, y): (i32, i32)) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    /// Shrunk by `d` on every side.
    pub fn inset(&self, d: i32) -> Self {
        Self::new(
            self.x + d,
            self.y + d,
            (self.w as i32 - 2 * d).max(0) as u32,
            (self.h as i32 - 2 * d).max(0) as u32,
        )
    }

    /// A rectangle of size `w × h` centred on the interface.
    pub fn centered(w: u32, h: u32) -> Self {
        Self::new(
            (WIDTH as i32 - w as i32) / 2,
            (HEIGHT as i32 - h as i32) / 2,
            w,
            h,
        )
    }
}

/// How an item looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Normal,
    Hover,
    /// Selected (a tab, a choice).
    Active,
    Disabled,
}

impl State {
    pub fn of(hover: bool, enabled: bool) -> Self {
        match (enabled, hover) {
            (false, _) => State::Disabled,
            (true, true) => State::Hover,
            (true, false) => State::Normal,
        }
    }
}

pub fn style(size: f32, color: Color) -> TextStyle {
    TextStyle {
        size,
        color,
        shadow: Some(Color::rgba(0, 0, 0, 160)),
        bold: false,
    }
}

/// A one-pixel frame inside `r`.
pub fn frame(canvas: &mut Canvas, r: Rect, color: Color) {
    canvas.fill_rect(r.x, r.y, r.w, 1, color);
    canvas.fill_rect(r.x, r.bottom() - 1, r.w, 1, color);
    canvas.fill_rect(r.x, r.y, 1, r.h, color);
    canvas.fill_rect(r.right() - 1, r.y, 1, r.h, color);
}

/// A panel with a border.
pub fn panel(canvas: &mut Canvas, r: Rect) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, theme::PANEL);
    frame(canvas, r, theme::BORDER);
}

/// Text with its top-left corner at `(x, y)`.
pub fn label(
    canvas: &mut Canvas,
    font: &mut Font,
    x: i32,
    y: i32,
    text: &str,
    size: f32,
    color: Color,
) {
    font.draw_line(canvas, x as f32, y as f32, text, style(size, color));
}

/// Text centred in `r`.
pub fn label_centered(
    canvas: &mut Canvas,
    font: &mut Font,
    r: Rect,
    text: &str,
    size: f32,
    color: Color,
) {
    let w = font.measure(text, size);
    let x = r.x as f32 + (r.w as f32 - w) / 2.0;
    let y = r.y as f32 + (r.h as f32 - size * 1.2) / 2.0;
    font.draw_line(canvas, x, y, text, style(size, color));
}

/// Text cut with `…` to fit `width`.
pub fn fit(font: &mut Font, text: &str, size: f32, width: f32) -> String {
    if font.measure(text, size) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let limit = width - font.measure("…", size);
    let mut used = 0.0;
    for c in text.chars() {
        let w = font.measure(c.encode_utf8(&mut [0; 4]), size);
        if used + w > limit {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    out
}

/// The background of an item in `state`.
pub fn item(canvas: &mut Canvas, r: Rect, state: State) {
    let back = match state {
        State::Normal | State::Disabled => theme::ITEM,
        State::Hover => theme::HOVER,
        State::Active => theme::ACTIVE,
    };
    canvas.fill_rect(r.x, r.y, r.w, r.h, back);
    if matches!(state, State::Hover | State::Active) {
        frame(canvas, r, theme::BORDER);
    }
}

/// A button: an item with its text centred.
pub fn button(canvas: &mut Canvas, font: &mut Font, r: Rect, text: &str, state: State) {
    item(canvas, r, state);
    let color = if state == State::Disabled {
        theme::DISABLED
    } else {
        theme::TEXT
    };
    label_centered(canvas, font, r, text, TEXT_SIZE, color);
}

/// A level from 0 to `max` as a row of blocks in `r`.
pub fn level_bar(canvas: &mut Canvas, r: Rect, level: u8, max: u8) {
    let n = i32::from(max.max(1));
    let gap = 4;
    let w = ((r.w as i32 - gap * (n - 1)) / n).max(1);
    for i in 0..n {
        let x = r.x + i * (w + gap);
        let color = if i < i32::from(level) {
            theme::ACCENT
        } else {
            theme::ITEM
        };
        canvas.fill_rect(x, r.y, w as u32, r.h, color);
    }
}

/// The level a click at `x` on a bar drawn in `r` asks for.
pub fn level_at(r: Rect, max: u8, x: i32) -> u8 {
    let f = (x - r.x) as f32 / r.w.max(1) as f32;
    ((f * f32::from(max)).ceil().clamp(0.0, f32::from(max))) as u8
}

/// The title of a screen, at the top of `panel`.
pub fn heading(canvas: &mut Canvas, font: &mut Font, panel: Rect, text: &str) {
    label(
        canvas,
        font,
        panel.x + 28,
        panel.y + 18,
        text,
        TITLE_SIZE,
        theme::TEXT,
    );
    canvas.fill_rect(panel.x + 24, panel.y + 62, panel.w - 48, 1, theme::BORDER);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects() {
        let r = Rect::new(10, 20, 30, 40);
        assert!(r.contains((10, 20)));
        assert!(r.contains((39, 59)));
        assert!(!r.contains((40, 20)));
        assert_eq!(r.inset(5), Rect::new(15, 25, 20, 30));
        assert_eq!(Rect::centered(100, 50), Rect::new(590, 335, 100, 50));
    }

    #[test]
    fn level_from_click() {
        let r = Rect::new(100, 0, 100, 10);
        assert_eq!(level_at(r, 10, 100), 0);
        assert_eq!(level_at(r, 10, 105), 1);
        assert_eq!(level_at(r, 10, 199), 10);
        assert_eq!(level_at(r, 10, 300), 10);
        assert_eq!(level_at(r, 10, 0), 0);
    }
}
