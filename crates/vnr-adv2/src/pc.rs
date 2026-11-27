//! The in-game PC of Pure Mail: the chat client, the mailer, letters,
//! text labels, the taskbar clock and small pop-up animations.
//!
//! What is on the PC screen is part of the [`Scene`](crate::game) so it
//! cross-fades with the background. Backgrounds (`sys 30`) and colour
//! fills (`sys 45`) clear every overlay, like the engine's `0x4236c0`; the
//! chat log itself survives until `sys a1`.
//!
//! | `sys` | Meaning | Basis |
//! |---|---|---|
//! | `a0`, `a2`, `a4` | add a chat line and wait for a click | handlers `0x422e10`, `0x422e90`, `0x422f10`; runs of up to 113 lines |
//! | `a1` | clear the chat log | `0x409c50` |
//! | `a3` | show the chat log (last 15 rows, text from x = 80) | `0x40a120` |
//! | `b2`, `b0` | add a mail line (style 1: subject, sender, date) | `0x422f80`, `0x422ff0` |
//! | `b3` | open the mail window (x, y, w, h, text size, line gap) and wait for a click | `0x423080`; `b1` always follows |
//! | `b1` | close the mail window and forget its lines | `0x423070` |
//! | `c2` | add a letter paragraph | `0x423130` |
//! | `c3` | show letter paragraph *n* | data |
//! | `c1` | close the letter | `0x423220` |
//! | `33` | pop-up animation by number at x, y | `0x423960` (name table) |
//! | `34` | text label: text, x, y, w, h, character size, line gap | `0x4235b0` (10 slots) |
//! | `50` | taskbar clock | `0x423530` |
//!
//! `sys 32` places shortcut icons of other installed games on the
//! desktop (found through the registry); it is ignored.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use vnr_core::{Canvas, Color, Font, Sprite, TextStyle};

/// Rows the chat window shows.
pub const CHAT_ROWS: usize = 15;
const CHAT_LEFT: f32 = 80.0;
const CHAT_RIGHT: f32 = 796.0;
const CHAT_TOP: f32 = 4.0;
const CHAT_SIZE: f32 = 18.0;
const CHAT_LINE: f32 = 24.0;
/// Most text labels at once (`sys 34`).
pub const MAX_LABELS: usize = 10;

/// Mail window frame: top (title, subject and date fields), a middle
/// piece tiled down the text, and the bottom edge.
pub const MAIL_FRAME: [&str; 3] = ["mailmes_u", "mailmes_c", "mailmes_d"];

/// Pictures shown by `sys 33`, with their size, frame time in
/// milliseconds, whether the script waits for them, and whether they loop
/// (table in `0x423960`).
pub const POPUPS: [(&str, u32, u32, u32, bool, bool); 19] = [
    ("osboot", 320, 240, 54, true, false),
    ("ikari", 67, 87, 125, false, true),
    ("jitoase", 27, 97, 125, false, true),
    ("ase", 80, 131, 125, false, true),
    ("aser", 95, 120, 125, false, true),
    ("mouse", 40, 64, 160, false, true),
    ("ok", 98, 28, 200, false, true),
    ("error", 98, 28, 200, false, true),
    ("pass_o", 98, 28, 200, false, true),
    ("pass_c", 98, 28, 200, false, true),
    ("pass0103", 180, 12, 180, false, true),
    ("pass0104", 180, 12, 180, false, true),
    ("pass0105", 180, 12, 180, false, true),
    ("pass0106", 180, 12, 180, false, true),
    ("pass0107", 180, 12, 180, false, true),
    ("pass0108", 180, 12, 180, false, true),
    ("pass0109", 180, 12, 180, false, true),
    ("pass0110", 180, 12, 180, false, true),
    ("pass0111", 180, 12, 180, false, true),
];

/// A line of chat or mail and its character style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub style: i32,
    pub text: String,
}

/// Text placed with `sys 34`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub text: String,
    pub rect: (i32, i32, u32, u32),
    pub size: f32,
    pub gap: f32,
}

/// The open mail.
#[derive(Clone, Serialize, Deserialize)]
pub struct Mail {
    pub rect: (i32, i32, u32, u32),
    pub size: f32,
    pub gap: f32,
    pub lines: Vec<Line>,
    /// The window pictures ([`MAIL_FRAME`]); not saved, loaded again.
    #[serde(skip)]
    pub frame: [Option<Arc<Sprite>>; 3],
}

/// What the PC shows in a scene.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PcView {
    /// The chat log, when its window is drawn.
    pub chat: Option<Vec<Line>>,
    pub mail: Option<Mail>,
    /// The letter paragraph on screen.
    pub letter: Option<String>,
    pub labels: Vec<Label>,
    pub clock: Option<String>,
}

/// A pop-up animation (`sys 33`).
#[derive(Clone)]
pub struct Popup {
    pub frames: Arc<Vec<Sprite>>,
    pub at: (i32, i32),
    pub start: u64,
    /// Ticks (1/60 s) per frame.
    pub ticks: u64,
    pub looping: bool,
}

impl Popup {
    /// Ticks until a non-looping pop-up shows its last frame.
    pub fn length(&self) -> u64 {
        self.ticks * self.frames.len() as u64
    }

    pub fn draw(&self, canvas: &mut Canvas, now: u64) {
        let n = self.frames.len() as u64;
        if n == 0 {
            return;
        }
        let step = now.saturating_sub(self.start) / self.ticks.max(1);
        let i = if self.looping {
            step % n
        } else {
            step.min(n - 1)
        };
        canvas.draw(&self.frames[i as usize], self.at.0, self.at.1, 255);
    }
}

/// Text colour of a chat or mail line on the white windows.
fn ink(style: i32) -> Color {
    match style {
        1 => Color::rgb(90, 90, 110),
        2 => Color::rgb(20, 40, 150),
        _ => Color::rgb(150, 20, 40),
    }
}

fn plain(size: f32, color: Color) -> TextStyle {
    TextStyle {
        size,
        color,
        shadow: None,
        outline: false,
        bold: false,
    }
}

/// Wraps `lines` and keeps the last `rows` rows, with their style.
pub fn tail_rows(
    font: &mut Font,
    lines: &[Line],
    size: f32,
    width: f32,
    rows: usize,
) -> Vec<(i32, String)> {
    let mut out: Vec<(i32, String)> = Vec::new();
    for line in lines.iter().rev() {
        let wrapped = font.wrap(&line.text, size, width);
        for row in wrapped.into_iter().rev() {
            out.push((line.style, row));
            if out.len() == rows {
                out.reverse();
                return out;
            }
        }
    }
    out.reverse();
    out
}

impl PcView {
    /// Drops everything a new background clears.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn draw(&self, canvas: &mut Canvas, font: &mut Font) {
        if let Some(chat) = &self.chat {
            let rows = tail_rows(font, chat, CHAT_SIZE, CHAT_RIGHT - CHAT_LEFT, CHAT_ROWS);
            for (i, (style, row)) in rows.iter().enumerate() {
                let y = CHAT_TOP + i as f32 * CHAT_LINE;
                font.draw_line(canvas, CHAT_LEFT, y, row, plain(CHAT_SIZE, ink(*style)));
            }
        }
        for label in &self.labels {
            let (x, y, w, h) = label.rect;
            let mut top = y as f32;
            for row in font.wrap(&label.text, label.size, w as f32) {
                if top + label.size > (y + h as i32) as f32 + 1.0 {
                    break;
                }
                font.draw_line(
                    canvas,
                    x as f32,
                    top,
                    &row,
                    plain(label.size, Color::rgb(16, 16, 16)),
                );
                top += label.size + label.gap;
            }
        }
        if let Some(mail) = &self.mail {
            mail.draw(canvas, font);
        }
        if let Some(letter) = &self.letter {
            let size = 22.0;
            let rows = font.wrap(letter, size, 640.0);
            let height = rows.len() as f32 * 34.0;
            let mut top = ((600.0 - height) / 2.0).max(20.0);
            for row in rows {
                font.draw_line(canvas, 80.0, top, &row, plain(size, Color::rgb(30, 30, 30)));
                top += 34.0;
            }
        }
        if let Some(clock) = &self.clock {
            font.draw_line(canvas, 706.0, 584.0, clock, plain(12.0, Color::WHITE));
        }
    }
}

impl Mail {
    fn draw(&self, canvas: &mut Canvas, font: &mut Font) {
        let (x, y, w, h) = self.rect;
        // The frame is wider than the text; centre it on the text area.
        let frame_w = self.frame[0].as_ref().map_or(w + 37, |s| s.width);
        let fx = x - (frame_w as i32 - w as i32) / 2;
        let top_h = self.frame[0].as_ref().map_or(50, |s| s.height) as i32;
        match &self.frame {
            [Some(top), Some(middle), Some(bottom)] => {
                canvas.draw(top, fx, y - top_h, 255);
                let mut my = y;
                while my < y + h as i32 {
                    canvas.draw(middle, fx, my, 255);
                    my += middle.height.max(1) as i32;
                }
                canvas.draw(bottom, fx, my, 255);
            }
            _ => canvas.fill_rect(fx, y - top_h, frame_w, h + top_h as u32, Color::WHITE),
        }
        // Subject and date go in the title fields; sender is not shown.
        let header: Vec<&Line> = self.lines.iter().filter(|l| l.style == 1).collect();
        let field = plain(10.0, Color::rgb(16, 16, 16));
        if let Some(subject) = header.first() {
            font.draw_line(
                canvas,
                (fx + 48) as f32,
                (y - top_h + 18) as f32,
                &subject.text,
                field,
            );
        }
        if let Some(date) = header.get(2) {
            font.draw_line(
                canvas,
                (fx + 226) as f32,
                (y - top_h + 18) as f32,
                &date.text,
                field,
            );
        }
        let size = self.size.max(12.0);
        let mut top = y as f32;
        for line in self.lines.iter().filter(|l| l.style != 1) {
            for row in font.wrap(&line.text, size, w as f32) {
                if top + size > (y + h as i32) as f32 {
                    return;
                }
                font.draw_line(
                    canvas,
                    x as f32,
                    top,
                    &row,
                    plain(size, Color::rgb(16, 16, 16)),
                );
                top += size + self.gap;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_length_counts_every_frame() {
        let sprite = || Sprite::from_rgb(1, 1, &[0, 0, 0]);
        let popup = Popup {
            frames: Arc::new(vec![sprite(), sprite(), sprite()]),
            at: (0, 0),
            start: 10,
            ticks: 4,
            looping: false,
        };
        assert_eq!(popup.length(), 12);
    }

    #[test]
    fn popup_table_matches_the_engine() {
        assert_eq!(POPUPS[0].0, "osboot");
        assert_eq!(POPUPS[18].0, "pass0111");
    }
}
