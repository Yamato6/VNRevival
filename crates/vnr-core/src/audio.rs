//! A small software mixer: a fixed number of channels, each playing one
//! clip of 44.1 kHz 16-bit stereo, optionally looping.
//!
//! The frontend pulls samples with [`Mixer::mix`] (from its audio
//! callback, or once per frame in headless mode); games start and stop
//! clips from the main thread. The mixer is cheap to clone and shared
//! between both.

use std::sync::{Arc, Mutex, MutexGuard};

/// Output rate of the mixer.
pub const SAMPLE_RATE: u32 = 44_100;

/// Full volume for [`Mixer::play`] and [`Mixer::set_volume`].
pub const FULL_VOLUME: u16 = 256;

/// Decoded audio: interleaved stereo 16-bit samples at [`SAMPLE_RATE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    samples: Vec<i16>,
    /// Frame where playback continues after the end, when looping.
    loop_start: Option<usize>,
}

impl Clip {
    /// From interleaved stereo samples.
    pub fn stereo(samples: Vec<i16>) -> Self {
        assert!(
            samples.len() % 2 == 0,
            "stereo clip with an odd sample count"
        );
        Self {
            samples,
            loop_start: None,
        }
    }

    /// From mono samples (played on both channels).
    pub fn mono(samples: &[i16]) -> Self {
        Self::stereo(samples.iter().flat_map(|&s| [s, s]).collect())
    }

    /// Makes the clip repeat from `frame` (0 = the whole clip) after it
    /// reaches its end.
    pub fn looping_from(mut self, frame: usize) -> Self {
        self.loop_start = Some(frame.min(self.frames().saturating_sub(1)));
        self
    }

    /// Length in stereo frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn is_looping(&self) -> bool {
        self.loop_start.is_some()
    }
}

#[derive(Debug)]
struct Voice {
    clip: Arc<Clip>,
    frame: usize,
    volume: u16,
}

#[derive(Debug)]
struct State {
    channels: Vec<Option<Voice>>,
    master: u16,
}

/// Shared handle to the mixer.
#[derive(Debug, Clone)]
pub struct Mixer {
    state: Arc<Mutex<State>>,
}

impl Mixer {
    /// A mixer with `channels` independent channels.
    pub fn new(channels: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                channels: (0..channels).map(|_| None).collect(),
                master: FULL_VOLUME,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock only loses audio state; keep going.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts `clip` on `channel`, replacing whatever was playing there.
    pub fn play(&self, channel: usize, clip: Arc<Clip>, volume: u16) {
        if let Some(slot) = self.lock().channels.get_mut(channel) {
            *slot = Some(Voice {
                clip,
                frame: 0,
                volume,
            });
        }
    }

    pub fn stop(&self, channel: usize) {
        if let Some(slot) = self.lock().channels.get_mut(channel) {
            *slot = None;
        }
    }

    pub fn stop_all(&self) {
        for slot in &mut self.lock().channels {
            *slot = None;
        }
    }

    pub fn is_playing(&self, channel: usize) -> bool {
        self.lock()
            .channels
            .get(channel)
            .is_some_and(Option::is_some)
    }

    /// Volume of one channel (0..=[`FULL_VOLUME`]).
    pub fn set_volume(&self, channel: usize, volume: u16) {
        if let Some(Some(voice)) = self.lock().channels.get_mut(channel) {
            voice.volume = volume;
        }
    }

    /// Volume applied to everything (0..=[`FULL_VOLUME`]).
    pub fn set_master_volume(&self, volume: u16) {
        self.lock().master = volume;
    }

    /// Fills `out` (interleaved stereo) with the next samples.
    pub fn mix(&self, out: &mut [i16]) {
        let mut state = self.lock();
        let master = i32::from(state.master);
        let mut acc = vec![0i32; out.len()];
        for slot in &mut state.channels {
            let Some(voice) = slot else { continue };
            let samples = &voice.clip.samples;
            let gain = i32::from(voice.volume) * master;
            let mut i = 0;
            while i + 1 < acc.len() {
                if voice.frame >= voice.clip.frames() {
                    match voice.clip.loop_start {
                        Some(start) if voice.clip.frames() > 0 => voice.frame = start,
                        _ => break,
                    }
                }
                let s = voice.frame * 2;
                acc[i] += (i32::from(samples[s]) * gain) >> 16;
                acc[i + 1] += (i32::from(samples[s + 1]) * gain) >> 16;
                voice.frame += 1;
                i += 2;
            }
            if voice.frame >= voice.clip.frames() && !voice.clip.is_looping() {
                *slot = None;
            }
        }
        for (o, a) in out.iter_mut().zip(acc) {
            *o = a.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_and_ends_one_shot_clips() {
        let mixer = Mixer::new(2);
        mixer.play(0, Arc::new(Clip::mono(&[1000, 2000])), FULL_VOLUME);
        mixer.play(1, Arc::new(Clip::stereo(vec![10, 20])), FULL_VOLUME / 2);
        let mut out = [0i16; 6];
        mixer.mix(&mut out);
        assert_eq!(out, [1005, 1010, 2000, 2000, 0, 0]);
        assert!(!mixer.is_playing(0));
        assert!(!mixer.is_playing(1));
    }

    #[test]
    fn loops_from_the_loop_point() {
        let mixer = Mixer::new(1);
        let clip = Clip::mono(&[1, 2, 3]).looping_from(1);
        mixer.play(0, Arc::new(clip), FULL_VOLUME);
        let mut out = [0i16; 12];
        mixer.mix(&mut out);
        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        assert_eq!(left, [1, 2, 3, 2, 3, 2]);
        assert!(mixer.is_playing(0));
        mixer.stop(0);
        assert!(!mixer.is_playing(0));
    }

    #[test]
    fn clamps_and_scales() {
        let mixer = Mixer::new(2);
        for ch in 0..2 {
            mixer.play(ch, Arc::new(Clip::mono(&[30_000])), FULL_VOLUME);
        }
        let mut out = [0i16; 2];
        mixer.mix(&mut out);
        assert_eq!(out, [i16::MAX, i16::MAX]);
        mixer.set_master_volume(0);
        mixer.play(0, Arc::new(Clip::mono(&[30_000])), FULL_VOLUME);
        mixer.mix(&mut out);
        assert_eq!(out, [0, 0]);
    }
}
