//! Screen transitions (`sys 40`).
//!
//! `sys 40 (type, mode, speed)` shows the staged scene through one of the
//! engine's effects (`0x410b30`, table at `0x791398`). Every effect runs
//! a progress value up by a speed-dependent step once per `GrpWipeWait`
//! milliseconds (`OF_ADV2.INI`, 15 by default):
//!
//! | speed | 0 | 1 | 2 | 3 | 4 |
//! |---|---|---|---|---|---|
//! | step | 4 | 8 | 2 | 16 | 1 |
//!
//! Implemented effects:
//!
//! * **0**: cut (`0x40e090`).
//! * **1**: cross-fade, progress 0 → 256 (`0x40e110`, the DLL's `XFade`).
//!   Its modes 1–4 are variants only drawn with a fast CPU module; the
//!   cross-fade is what slower machines show.
//! * **3**: "morning light" rule wipe (`0x40eba0`). The rule is
//!   `ASABASE.ALPF` (200 × 150, one value per 4 × 4 pixels; mode flag
//!   `0x400` mirrors it, `0x800` inverts it) plus a window of
//!   `ASAFLASH.ALPA` that slides diagonally as progress grows from −256 to
//!   128. A pixel shows `clamp(progress + rule, 0, 128) / 128` of the new
//!   picture (the DLL's `AlpBlend4` tables).
//!
//! Types 2, 4, 7 and 9 (ten calls in the whole game) fall back to the
//! cross-fade for now; 5, 6 and 8 are never used.

use std::sync::Arc;

use vnr_core::{Canvas, FPS};

/// Progress steps per wait period, by speed (`0x40e2e8`).
const STEPS: [u32; 5] = [4, 8, 2, 16, 1];

/// Default `GrpWipeWait`.
pub const DEFAULT_WAIT_MS: u32 = 15;

/// Size of the rule pictures; each value covers 4 × 4 screen pixels.
const RULE_W: usize = 200;
const RULE_H: usize = 150;
const FLASH_W: usize = 250;
const FLASH_H: usize = 200;

/// Progress step for a speed argument; unknown speeds use the default
/// (as the jump table does).
pub fn step(speed: i32) -> u32 {
    usize::try_from(speed)
        .ok()
        .and_then(|s| STEPS.get(s))
        .copied()
        .unwrap_or(STEPS[0])
}

/// Grey pictures the morning wipe is built from.
#[derive(Debug, Clone)]
pub struct MorningPictures {
    /// `ASABASE.ALPF`, 200 × 150, top-down.
    pub base: Vec<u8>,
    /// `ASAFLASH.ALPA`, 250 × 200, top-down.
    pub flash: Vec<u8>,
}

/// What a transition does.
#[derive(Debug, Clone)]
pub enum Effect {
    Cut,
    Fade,
    Morning {
        /// The rule before the flash is added (mirrored/inverted).
        rule: Vec<u8>,
        pictures: Arc<MorningPictures>,
    },
}

impl Effect {
    /// Progress at the start and the value it must exceed to finish.
    fn range(&self) -> (i32, i32) {
        match self {
            Effect::Cut => (0, 0),
            Effect::Fade => (0, 256),
            Effect::Morning { .. } => (-256, 128),
        }
    }

    /// The morning effect with the mode flags of `sys 40`.
    pub fn morning(mode: i32, pictures: Arc<MorningPictures>) -> Self {
        let mut rule = pictures.base.clone();
        if mode & 0x400 != 0 {
            for row in rule.chunks_exact_mut(RULE_W) {
                row.reverse();
            }
        }
        if mode & 0x800 != 0 {
            for v in &mut rule {
                *v = 0x80u8.wrapping_sub(*v);
            }
        }
        Effect::Morning { rule, pictures }
    }
}

/// A transition in progress from `old` to whatever is shown now.
#[derive(Debug, Clone)]
pub struct Transition {
    effect: Effect,
    old: Canvas,
    start_frame: u64,
    step: u32,
    wait_ms: u32,
}

impl Transition {
    pub fn new(effect: Effect, old: Canvas, start_frame: u64, step: u32, wait_ms: u32) -> Self {
        Self {
            effect,
            old,
            start_frame,
            step: step.max(1),
            wait_ms: wait_ms.max(1),
        }
    }

    /// Progress at `frame`.
    fn progress(&self, frame: u64) -> i32 {
        let ms = frame.saturating_sub(self.start_frame) * 1000 / FPS as u64;
        let (start, end) = self.effect.range();
        let gained = ms * u64::from(self.step) / u64::from(self.wait_ms);
        (i64::from(start) + gained.min(1 << 20) as i64).min(i64::from(end) + 1) as i32
    }

    pub fn effect(&self) -> &Effect {
        &self.effect
    }

    pub fn start_frame(&self) -> u64 {
        self.start_frame
    }

    /// Whether the transition has finished at `frame`.
    pub fn done(&self, frame: u64) -> bool {
        self.progress(frame) > self.effect.range().1
    }

    /// Frames from start to end.
    pub fn length(&self) -> u64 {
        let (start, end) = self.effect.range();
        let units = u64::from((end - start + 1).max(0) as u32);
        let ms = (units * u64::from(self.wait_ms)).div_ceil(u64::from(self.step));
        (ms * FPS as u64).div_ceil(1000)
    }

    /// Draws the transition state at `frame` into `out`, mixing the old
    /// picture with `new`.
    pub fn draw(&self, new: &Canvas, out: &mut Canvas, frame: u64) {
        let p = self.progress(frame);
        match &self.effect {
            Effect::Cut => out.blend_from(new, 255),
            Effect::Fade => {
                let k = p.clamp(0, 256) as u16;
                out.mix(&self.old, new, |_, _| k);
            }
            Effect::Morning { rule, pictures } => {
                let map = morning_map(rule, &pictures.flash, p);
                out.mix(&self.old, new, |x, y| {
                    let m = i32::from(
                        map[(y as usize / 4).min(RULE_H - 1) * RULE_W
                            + (x as usize / 4).min(RULE_W - 1)],
                    );
                    ((p + m).clamp(0, 128) * 2) as u16
                });
            }
        }
    }
}

/// The rule for progress `p`: `rule` plus the part of the flash picture
/// that has slid into view, saturating (`0x40ed90`).
fn morning_map(rule: &[u8], flash: &[u8], p: i32) -> Vec<u8> {
    let s = ((p + 256).max(0) / 8) as usize;
    let mut map = rule.to_vec();
    if flash.len() < FLASH_W * FLASH_H || rule.len() < RULE_W * RULE_H {
        return map;
    }
    for y in 0..RULE_H {
        // The engine works on bottom-up rows; in top-down terms the window
        // starts 50 - s rows down and 47 - s columns in.
        let Some(fy) = (y + 50).checked_sub(s) else {
            continue;
        };
        if fy >= FLASH_H {
            continue;
        }
        for x in 0..RULE_W {
            let Some(fx) = (x + 47).checked_sub(s) else {
                continue;
            };
            let v = &mut map[y * RULE_W + x];
            *v = v.saturating_add(flash[fy * FLASH_W + fx]);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnr_core::Color;

    fn canvas(color: Color) -> Canvas {
        let mut c = Canvas::new(800, 600);
        c.clear(color);
        c
    }

    #[test]
    fn steps_follow_the_engine_table() {
        assert_eq!(step(0), 4);
        assert_eq!(step(2), 2);
        assert_eq!(step(4), 1);
        assert_eq!(step(9), 4);
    }

    #[test]
    fn fade_takes_the_engine_time() {
        let t = Transition::new(Effect::Fade, canvas(Color::BLACK), 0, step(0), 15);
        // 257 units of progress, 4 per 15 ms: just under a second.
        assert_eq!(t.length(), 58);
        assert!(!t.done(57));
        assert!(t.done(58));
        let mut out = canvas(Color::BLACK);
        t.draw(&canvas(Color::WHITE), &mut out, 29);
        let mid = out.pixel(0, 0).r;
        assert!((120..=135).contains(&mid), "{mid}");
    }

    #[test]
    fn morning_wipe_starts_old_and_ends_new() {
        let pictures = Arc::new(MorningPictures {
            base: (0..RULE_W * RULE_H).map(|i| (i % 129) as u8).collect(),
            flash: vec![0; FLASH_W * FLASH_H],
        });
        let t = Transition::new(
            Effect::morning(0, pictures),
            canvas(Color::BLACK),
            0,
            step(3),
            15,
        );
        let new = canvas(Color::WHITE);
        let mut out = canvas(Color::BLACK);
        t.draw(&new, &mut out, 0);
        assert_eq!(out.pixel(400, 300).r, 0);
        t.draw(&new, &mut out, t.length());
        assert_eq!(out.pixel(400, 300).r, 255);
        assert!(t.done(t.length()));
    }

    #[test]
    fn morning_flags_mirror_and_invert() {
        let mut base = vec![0; RULE_W * RULE_H];
        base[0] = 100;
        let pictures = Arc::new(MorningPictures {
            base,
            flash: vec![],
        });
        let Effect::Morning { rule, .. } = Effect::morning(0x400 | 0x800, pictures) else {
            unreachable!()
        };
        assert_eq!(rule[RULE_W - 1], 28);
        assert_eq!(rule[0], 128);
    }
}
