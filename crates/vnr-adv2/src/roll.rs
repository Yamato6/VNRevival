//! The ending's staff roll (`sys d3`, `0x411260`) and the memories it
//! shows (`sys d6`, `0x421fc0`).
//!
//! Along the story, `sys d6 (heroine, picture, voice, "")` records a
//! scene of a heroine; the engine keeps her last [`MEMORIES`] (a table at
//! `0xf864be`, oldest first, empty slots first). At her ending,
//! `sys d3 (heroine, track)` plays music track `track` and shows:
//!
//! 1. `END<hh>_00`, then `END<hh>_01` (each through a cross-fade);
//! 2. for each of the 12 slots: the slot's picture (`<picture>.EDG`,
//!    460 × 345, at 56, 128) over `END<hh>_01`, then one page of the staff
//!    roll (`Roll_NN.ROL`) scrolling up through the window at 556, 70
//!    (200 × 460), one pixel every 13 ms, then the plain background again;
//! 3. `END<hh>_02` and `END<hh>_03` through slow cross-fades, the music
//!    fading out with the last one.
//!
//! The fourth page depends on two flags (`0x10d`, `0x10e`): `Roll_03a`
//! with both, `03c` with `0x10d`, `03b` with `0x10e`, else `03d`. At the
//! end the roll sets the flag "ending seen" (`0x129 + heroine`).
//!
//! The times are the engine's: it waits for absolute times on a clock
//! that restarts when the slots begin (the waits before that point
//! compare against a clock base of zero, so they never wait), and its
//! cross-fades block for their whole length (64 steps of 15 ms). The
//! voices recorded with the pictures are not played.
//!
//! A roll page is 608 bytes per row: eight bytes whose first is non-zero
//! on rows with nothing to draw, then 200 pixels of three equal bytes,
//! subtracted from the background (dark text on the light picture).
//!
//! The original skips the rest with Enter once flag `0x136` is set, from
//! the second page on, when the page ends; VNRevival also lets an ending
//! already seen be skipped, at once, with a click, Enter or Space.

use serde::{Deserialize, Serialize};

use vnr_core::{Canvas, Sprite};

/// Memories kept per heroine.
pub const MEMORIES: usize = 12;

/// Roll pages and the height the engine times each one with.
pub const PAGES: [(&str, u32); 12] = [
    ("Roll_00", 437),
    ("Roll_01", 556),
    ("Roll_02", 574),
    ("Roll_03", 631),
    ("Roll_04", 543),
    ("Roll_05", 619),
    ("Roll_06", 459),
    ("Roll_07", 485),
    ("Roll_08", 280),
    ("Roll_09", 544),
    ("Roll_10", 772),
    ("Roll_11", 89),
];

/// Where a memory picture goes.
pub const MEMORY_POS: (i32, i32) = (56, 128);
/// The window the roll scrolls through.
const WINDOW: (usize, usize, usize, usize) = (556, 70, 200, 460);
/// Milliseconds per pixel of scrolling.
const MS_PER_PIXEL: u64 = 13;
/// Bytes per row of a roll page.
const ROW: usize = 608;
/// Cross-fade lengths: speed 0 and speed 4 (`sys 40` steps 4 and 1, 15
/// ms per step).
pub const FADE_MS: u64 = 960;
pub const SLOW_FADE_MS: u64 = 3840;

/// Flag "ending `heroine` seen".
pub fn ending_flag(heroine: i32) -> usize {
    (0x129 + heroine).clamp(0, 0xFFF) as usize
}

/// The file of page `page` with flags `0x10d` and `0x10e`.
pub fn page_file(page: usize, flag_10d: bool, flag_10e: bool) -> String {
    if page == 3 {
        let variant = match (flag_10d, flag_10e) {
            (true, true) => 'a',
            (true, false) => 'c',
            (false, true) => 'b',
            (false, false) => 'd',
        };
        format!("Roll_03{variant}.ROL")
    } else {
        format!("{}.ROL", PAGES[page].0)
    }
}

/// A memory picture placed on a transparent screen-sized sprite.
pub fn place(picture: &Sprite, (width, height): (u32, u32)) -> Sprite {
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    let (x0, y0) = (MEMORY_POS.0 as usize, MEMORY_POS.1 as usize);
    let pw = picture.width as usize;
    for y in 0..picture.height as usize {
        if y0 + y >= h {
            break;
        }
        let n = pw.min(w.saturating_sub(x0));
        let src = &picture.pixels[y * pw * 4..(y * pw + n) * 4];
        let at = ((y0 + y) * w + x0) * 4;
        pixels[at..at + n * 4].copy_from_slice(src);
    }
    Sprite {
        width,
        height,
        pixels,
    }
}

/// One heroine's memories: `(picture, voice)`, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memories(pub Vec<(String, String)>);

impl Memories {
    /// Records a scene; the oldest goes when there are too many.
    pub fn record(&mut self, picture: String, voice: String) {
        self.0.push((picture, voice));
        if self.0.len() > MEMORIES {
            self.0.remove(0);
        }
    }

    /// The picture of slot `slot` (the newest is in the last slot).
    pub fn slot(&self, slot: usize) -> Option<&str> {
        let first = MEMORIES - self.0.len();
        slot.checked_sub(first)
            .and_then(|i| self.0.get(i))
            .map(|(picture, _)| picture.as_str())
    }
}

/// What happens at a point of the roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// `END<hh>_<n>` through a cross-fade (slow or not).
    Ending { picture: u8, slow: bool },
    /// The picture of slot `n` over the background, through a cross-fade.
    Memory(usize),
    /// Page `n` starts scrolling.
    Page(usize),
    /// The music starts fading out.
    FadeMusic,
    /// The roll is over.
    End,
}

/// The cues in order, with their time in milliseconds.
pub fn timeline() -> Vec<(u64, Cue)> {
    let mut cues = vec![
        (
            0,
            Cue::Ending {
                picture: 0,
                slow: false,
            },
        ),
        (
            FADE_MS,
            Cue::Ending {
                picture: 1,
                slow: false,
            },
        ),
    ];
    // The clock the engine restarts.
    let base = 2 * FADE_MS;
    // `edi` of `0x411260`.
    let mut edi = 2400;
    let mut shown = 0;
    for (page, &(_, height)) in PAGES.iter().enumerate() {
        cues.push((base + shown, Cue::Memory(page)));
        edi += 1500;
        cues.push((base + edi, Cue::Page(page)));
        edi += (u64::from(height) + 460) * MS_PER_PIXEL;
        // The plain background again, then the next picture once that
        // fade is over.
        cues.push((
            base + edi + 2,
            Cue::Ending {
                picture: 1,
                slow: false,
            },
        ));
        edi += 1500;
        shown = edi - 1500 + 2 + FADE_MS;
    }
    let end = base + shown;
    cues.extend(skip_tail(end));
    cues
}

/// The last cues, from `at` (also what a skip jumps to).
pub fn skip_tail(at: u64) -> Vec<(u64, Cue)> {
    vec![
        (
            at,
            Cue::Ending {
                picture: 2,
                slow: true,
            },
        ),
        (
            at + SLOW_FADE_MS,
            Cue::Ending {
                picture: 3,
                slow: true,
            },
        ),
        (at + SLOW_FADE_MS, Cue::FadeMusic),
        (at + 2 * SLOW_FADE_MS, Cue::End),
    ]
}

/// A roll page.
pub struct Page {
    /// Top-down rows of 608 bytes.
    pub rows: Vec<u8>,
    /// When it started scrolling, in milliseconds of the roll.
    pub start: u64,
    /// The height the engine times it with.
    pub height: u32,
}

impl Page {
    /// Whether it is still scrolling at `now`.
    pub fn scrolling(&self, now: u64) -> bool {
        now >= self.start && self.offset(now) < u64::from(self.height) + 459
    }

    fn offset(&self, now: u64) -> u64 {
        now.saturating_sub(self.start) / MS_PER_PIXEL
    }

    /// Draws the page as it is at `now` over `canvas`.
    pub fn draw(&self, canvas: &mut Canvas, now: u64) {
        if !self.scrolling(now) {
            return;
        }
        let (x0, y0, w, h) = WINDOW;
        let offset = self.offset(now) as usize;
        let rows = self.rows.len() / ROW;
        let width = canvas.width() as usize;
        let pixels = canvas.rgba_mut();
        for wy in 0..h {
            // The page enters at the bottom of the window.
            let Some(r) = (wy + offset).checked_sub(h).filter(|&r| r < rows) else {
                continue;
            };
            let row = &self.rows[r * ROW..(r + 1) * ROW];
            if row[0] != 0 {
                continue;
            }
            for x in 0..w {
                let v = row[8 + 3 * x];
                if v == 0 {
                    continue;
                }
                let at = ((y0 + wy) * width + x0 + x) * 4;
                for c in &mut pixels[at..at + 3] {
                    *c = c.saturating_sub(v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnr_core::Color;

    #[test]
    fn memories_fill_the_last_slots() {
        let mut m = Memories::default();
        m.record("a".into(), "va".into());
        m.record("b".into(), "vb".into());
        assert_eq!(m.slot(10), Some("a"));
        assert_eq!(m.slot(11), Some("b"));
        assert_eq!(m.slot(0), None);
        for i in 0..20 {
            m.record(format!("p{i}"), String::new());
        }
        assert_eq!(m.0.len(), MEMORIES);
        assert_eq!(m.slot(0), Some("p8"));
        assert_eq!(m.slot(11), Some("p19"));
    }

    #[test]
    fn page_files() {
        assert_eq!(page_file(0, true, true), "Roll_00.ROL");
        assert_eq!(page_file(3, true, true), "Roll_03a.ROL");
        assert_eq!(page_file(3, true, false), "Roll_03c.ROL");
        assert_eq!(page_file(3, false, true), "Roll_03b.ROL");
        assert_eq!(page_file(3, false, false), "Roll_03d.ROL");
        assert_eq!(ending_flag(3), 0x12c);
    }

    #[test]
    fn timeline_follows_the_engine() {
        let cues = timeline();
        assert!(cues.windows(2).all(|w| w[0].0 <= w[1].0));
        let at = |cue: Cue| cues.iter().find(|c| c.1 == cue).unwrap().0;
        // The first page waits 3.9 s on the new clock (from 2.4 s), the
        // next ones 3 s after the previous page ends.
        assert_eq!(at(Cue::Page(0)), 1920 + 3900);
        let end0 = 1920 + 3900 + (437 + 460) * 13;
        assert_eq!(at(Cue::Page(1)), end0 + 3000);
        assert_eq!(at(Cue::Memory(1)), end0 + 2 + FADE_MS);
        let end = cues.last().unwrap().0;
        // About two and a half minutes of pages, plus the pauses.
        assert!((180_000..200_000).contains(&end), "{end}");
    }

    #[test]
    fn pages_scroll_up_through_the_window() {
        // A page of one marked row and two text rows.
        let mut rows = vec![0u8; 3 * ROW];
        rows[0] = 0xFF;
        rows[ROW + 8] = 100;
        rows[2 * ROW + 8] = 200;
        let page = Page {
            rows,
            start: 1000,
            height: 3,
        };
        let mut canvas = Canvas::new(800, 600);
        canvas.clear(Color::WHITE);
        // 5 pixels in: rows 0..5 of the page fill the last 5 rows.
        page.draw(&mut canvas, 1000 + 5 * 13);
        assert_eq!(canvas.pixel(556, 529), Color::WHITE);
        assert_eq!(canvas.pixel(556, 527), Color::rgb(55, 55, 55));
        assert_eq!(canvas.pixel(556, 528), Color::WHITE);
        assert_eq!(canvas.pixel(556, 526), Color::rgb(155, 155, 155));
        assert!(page.scrolling(1000));
        assert!(!page.scrolling(1000 + (3 + 459) * 13));
    }
}
