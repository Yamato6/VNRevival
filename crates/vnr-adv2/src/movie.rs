//! Movies (`sys 35`): Indeo 5 AVI files in `MOVIE/`, decoded by the
//! `indeo` crate from VNTools-rs.
//!
//! The script names a movie (`pm01`); the files are `MOVIE/PM01L_I5.AVI`
//! (640 × 480) and `MOVIE/PM01S_I5.AVI` (320 × 240). `OF_ADV2.INI`'s
//! `MovieType` picks one (0: large, 1: small; 2 and 3 are Indeo 3
//! versions that are not on the disc), and `MovieStrech=1` shows it over
//! the whole screen, which is what is done here.
//!
//! The sound track plays on its own mixer channel and is the clock: the
//! picture shown is the one for the sound's position, so both stay
//! together. Without sound, the game's frame counter is the clock.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use indeo::{Avi, Decoder};
use vnr_core::audio::SAMPLE_RATE;
use vnr_core::{Canvas, Clip, Mixer, Sprite};

/// A movie being played.
pub struct Movie {
    avi: Avi<BufReader<File>>,
    decoder: Decoder,
    frames: usize,
    frame_micros: u64,
    /// Index of the next frame to decode.
    next: usize,
    picture: Option<Sprite>,
    sound: Option<Arc<Clip>>,
    /// Game frame when it started (clock without sound).
    started: u64,
}

impl Movie {
    /// Opens movie `name` (as the script gives it) from `root/MOVIE`.
    /// `small` prefers the 320 × 240 version.
    pub fn open(root: &Path, name: &str, small: bool) -> Result<Self, String> {
        let stem = name.to_ascii_uppercase();
        let sizes = if small { ["S", "L"] } else { ["L", "S"] };
        let path = sizes
            .iter()
            .map(|size| format!("MOVIE/{stem}{size}_I5.AVI"))
            .chain([format!("MOVIE/{stem}.AVI")])
            .find_map(|relative| vnr_core::resolve_ci(root, &relative))
            .ok_or_else(|| format!("movie {name} not found in MOVIE/"))?;
        let mut avi = Avi::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let video = avi
            .video()
            .ok_or_else(|| format!("{}: no video", path.display()))?;
        if !video.codec.eq_ignore_ascii_case(b"IV50") {
            return Err(format!(
                "{}: codec {} is not supported",
                path.display(),
                String::from_utf8_lossy(&video.codec)
            ));
        }
        let (frames, frame_micros) = (video.frame_count(), video.frame_micros().max(1));
        let sound = match avi.audio() {
            Some(a)
                if a.is_pcm()
                    && a.bits == 16
                    && a.sample_rate == SAMPLE_RATE
                    && a.channels <= 2 =>
            {
                let stereo = a.channels == 2;
                let data = avi.read_audio().map_err(|e| e.to_string())?;
                let samples: Vec<i16> = data
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                Some(Arc::new(if stereo {
                    let even = samples.len() & !1;
                    Clip::stereo(samples[..even].to_vec())
                } else {
                    Clip::mono(&samples)
                }))
            }
            _ => None,
        };
        Ok(Self {
            avi,
            decoder: Decoder::new(),
            frames,
            frame_micros,
            next: 0,
            picture: None,
            sound,
            started: 0,
        })
    }

    /// Starts the sound on `channel` at `volume`, and the clock at game
    /// frame `now`.
    pub fn start(&mut self, mixer: Option<&Mixer>, channel: usize, volume: u16, now: u64) {
        self.started = now;
        if let (Some(mixer), Some(sound)) = (mixer, &self.sound) {
            mixer.play(channel, sound.clone(), volume);
        }
    }

    /// Decodes up to the frame for the current time. Returns `false` once
    /// the movie is over.
    pub fn advance(&mut self, mixer: Option<&Mixer>, channel: usize, now: u64) -> bool {
        let micros = match (&self.sound, mixer) {
            (Some(_), Some(mixer)) => match mixer.position(channel) {
                Some(position) => position as u64 * 1_000_000 / u64::from(SAMPLE_RATE),
                // The sound is over (or was stopped): the movie is too.
                None => return false,
            },
            _ => (now.saturating_sub(self.started)) * 1_000_000 / u64::from(vnr_core::FPS),
        };
        let wanted = ((micros / self.frame_micros) as usize).min(self.frames.saturating_sub(1));
        let mut changed = false;
        while self.next <= wanted {
            let packet = match self.avi.read_video(self.next) {
                Ok(packet) => packet,
                Err(e) => {
                    eprintln!("movie: {e}");
                    return false;
                }
            };
            self.next += 1;
            // Only the last frame reached is shown, but each one is
            // decoded so the decoder keeps its state.
            if let Err(e) = self.decoder.decode(&packet) {
                eprintln!("movie frame {}: {e}", self.next - 1);
                return false;
            }
            changed = true;
        }
        if changed && let Ok(frame) = self.decoder.decode(&[]) {
            self.picture = Some(Sprite::from_rgba(
                frame.width as u32,
                frame.height as u32,
                frame.to_rgba(),
            ));
        }
        let duration = self.frames as u64 * self.frame_micros;
        micros < duration
    }

    /// Draws the current picture over the whole canvas.
    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(vnr_core::Color::BLACK);
        if let Some(picture) = &self.picture {
            canvas.draw_sprite_scaled(picture, 0, 0, canvas.width(), canvas.height());
        }
    }
}
