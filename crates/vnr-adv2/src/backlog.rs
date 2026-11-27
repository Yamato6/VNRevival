//! The message history (backlog).
//!
//! The original adds an entry (`0x405ae0`) for messages (`sys 01`), chat
//! lines (`sys a0`/`a2`) and the lines of `sys 04`, which are text that is
//! otherwise only in pictures or windows (the monologues, mails read
//! aloud). [`History`] keeps the last [`LIMIT`] of them; [`View`] shows
//! them over the game.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use vnr_core::{Canvas, Color, Font, InputEvent, Key, MouseButton, TextStyle};

/// Entries kept.
pub const LIMIT: usize = 300;

const LEFT: f32 = 60.0;
const WIDTH: f32 = 680.0;
const TOP: i32 = 40;
const BOTTOM: i32 = 570;
const SIZE: f32 = 18.0;
const ROW: i32 = 26;
/// Blank space between entries.
const GAP: i32 = 10;

/// One line of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub speaker: Option<String>,
    pub text: String,
    /// Voice file to replay.
    pub voice: Option<String>,
}

/// The entries so far, oldest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    entries: VecDeque<Entry>,
}

impl History {
    pub fn push(&mut self, entry: Entry) {
        if entry.text.trim().is_empty() {
            return;
        }
        if self.entries.len() == LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }
}

/// A laid-out row: which entry, its text, and whether it is the speaker.
#[derive(Debug, Clone)]
struct Row {
    entry: usize,
    text: String,
    speaker: bool,
    /// Extra space above (the gap before a new entry).
    gap: i32,
}

/// What the player asked for in the history view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    /// Replay this voice file.
    Voice(String),
}

/// The open history, scrolled to some position.
pub struct View {
    rows: Vec<Row>,
    voices: Vec<Option<String>>,
    /// Rows scrolled up from the end.
    scroll: usize,
    /// Screen rectangles of the visible entries, for clicks.
    hits: Vec<(i32, i32, usize)>,
}

impl View {
    /// Lays out `history` with `font`, scrolled to the end.
    pub fn new(history: &History, font: &mut Font) -> Self {
        let mut rows = Vec::new();
        for (i, entry) in history.entries().enumerate() {
            let mut gap = if i == 0 { 0 } else { GAP };
            if let Some(speaker) = &entry.speaker {
                rows.push(Row {
                    entry: i,
                    text: speaker.clone(),
                    speaker: true,
                    gap,
                });
                gap = 0;
            }
            for line in font.wrap(&entry.text, SIZE, WIDTH) {
                rows.push(Row {
                    entry: i,
                    text: line,
                    speaker: false,
                    gap,
                });
                gap = 0;
            }
        }
        Self {
            rows,
            voices: history.entries().map(|e| e.voice.clone()).collect(),
            scroll: 0,
            hits: Vec::new(),
        }
    }

    /// Rows that fit on screen.
    fn page() -> usize {
        ((BOTTOM - TOP) / ROW) as usize
    }

    fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(Self::page())
    }

    fn scroll_by(&mut self, rows: i32) -> Action {
        if rows < 0 && self.scroll == 0 {
            // Scrolling down past the newest entry closes the history.
            return Action::Close;
        }
        let next = self.scroll as i64 + i64::from(rows);
        self.scroll = next.clamp(0, self.max_scroll() as i64) as usize;
        Action::None
    }

    pub fn input(&mut self, events: &[InputEvent]) -> Action {
        for event in events {
            let action = match *event {
                InputEvent::Wheel(steps) => self.scroll_by(steps * 3),
                InputEvent::KeyDown(Key::Up) => self.scroll_by(1),
                InputEvent::KeyDown(Key::Down) => self.scroll_by(-1),
                InputEvent::KeyDown(Key::PageUp) => self.scroll_by(Self::page() as i32 - 1),
                InputEvent::KeyDown(Key::PageDown) => self.scroll_by(1 - Self::page() as i32),
                InputEvent::KeyDown(Key::Escape | Key::Backspace)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => Action::Close,
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    y,
                    ..
                } => self
                    .hits
                    .iter()
                    .find(|&&(top, bottom, _)| y >= top && y < bottom)
                    .and_then(|&(_, _, entry)| self.voices.get(entry).cloned().flatten())
                    .map_or(Action::None, Action::Voice),
                _ => Action::None,
            };
            if action != Action::None {
                return action;
            }
        }
        Action::None
    }

    pub fn draw(&mut self, canvas: &mut Canvas, font: &mut Font) {
        canvas.fill_rect(
            0,
            0,
            canvas.width(),
            canvas.height(),
            Color::rgba(0, 0, 20, 235),
        );
        let style = |color| TextStyle {
            size: SIZE,
            color,
            shadow: Some(Color::BLACK),
            bold: false,
        };
        // Lay out upwards from the bottom.
        let end = self.rows.len() - self.scroll.min(self.rows.len());
        let mut placed = Vec::new();
        let mut y = BOTTOM;
        for row in self.rows[..end].iter().rev() {
            if y - ROW < TOP {
                break;
            }
            y -= ROW;
            placed.push((y, row));
            y -= row.gap;
        }
        self.hits.clear();
        for &(y, row) in placed.iter().rev() {
            let color = if row.speaker {
                Color::rgb(255, 208, 112)
            } else {
                Color::rgb(235, 235, 235)
            };
            font.draw_line(canvas, LEFT, y as f32, &row.text, style(color));
            match self.hits.last_mut() {
                Some(last) if last.2 == row.entry => last.1 = y + ROW,
                _ => self.hits.push((y, y + ROW, row.entry)),
            }
        }
        for &(top, _, entry) in &self.hits {
            if self.voices.get(entry).is_some_and(Option::is_some) {
                font.draw_line(
                    canvas,
                    LEFT - 28.0,
                    top as f32,
                    "♪",
                    style(Color::rgb(140, 200, 255)),
                );
            }
        }
        // Scroll bar.
        let total = self.rows.len().max(1) as f32;
        let page = Self::page() as f32;
        if total > page {
            let track = (BOTTOM - TOP) as f32;
            let height = (track * page / total).max(12.0);
            let top = TOP as f32
                + (track - height) * (1.0 - self.scroll as f32 / self.max_scroll() as f32);
            canvas.fill_rect(
                772,
                TOP,
                6,
                (BOTTOM - TOP) as u32,
                Color::rgba(255, 255, 255, 30),
            );
            canvas.fill_rect(
                772,
                top as i32,
                6,
                height as u32,
                Color::rgba(255, 255, 255, 140),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Entry {
        Entry {
            speaker: None,
            text: text.into(),
            voice: None,
        }
    }

    #[test]
    fn keeps_the_newest_entries() {
        let mut history = History::default();
        for i in 0..LIMIT + 5 {
            history.push(entry(&i.to_string()));
        }
        history.push(entry("  "));
        assert_eq!(history.len(), LIMIT);
        assert_eq!(history.entries().next().unwrap().text, "5");
    }
}
