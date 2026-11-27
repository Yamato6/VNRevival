//! The interface between a running game and a frontend.
//!
//! A game is an [`App`] that gets called once per frame at a fixed
//! [`FPS`], with that frame's input, a canvas to draw on and the mixer.
//! Frontends (the SDL window, or [`run_headless`] for tests and
//! screenshots) only move pixels, samples and events around.

use crate::audio::{Mixer, SAMPLE_RATE};
use crate::gfx::Canvas;

/// Frames per second of game logic. Timing in the original engines is
/// tied to frames or milliseconds; 60 covers both.
pub const FPS: u32 = 60;

/// Mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Keys the games care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Space,
    Escape,
    Control,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Backspace,
    /// Function keys F1–F12.
    F(u8),
}

/// One input event, in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    MouseMove {
        x: i32,
        y: i32,
    },
    MouseDown {
        button: MouseButton,
        x: i32,
        y: i32,
    },
    MouseUp {
        button: MouseButton,
        x: i32,
        y: i32,
    },
    /// Wheel steps; positive is up (away from the user).
    Wheel(i32),
    KeyDown(Key),
    KeyUp(Key),
}

/// Everything a game gets for one frame.
pub struct Frame<'a> {
    /// Frame counter, starting at 0.
    pub number: u64,
    /// Events since the previous frame, oldest first.
    pub input: &'a [InputEvent],
    /// The picture to draw. It keeps the previous frame's content.
    pub canvas: &'a mut Canvas,
    pub mixer: &'a Mixer,
}

/// What the frontend should do after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// A running game.
pub trait App {
    /// Window title.
    fn title(&self) -> String;

    /// Size of the canvas, fixed for the whole run.
    fn resolution(&self) -> (u32, u32);

    /// Advances the game by one frame and draws it.
    fn update(&mut self, frame: &mut Frame<'_>) -> Flow;
}

/// Stereo samples the mixer produces per frame.
pub const SAMPLES_PER_FRAME: usize = (SAMPLE_RATE / FPS) as usize * 2;

/// Runs `app` for `frames` frames without a window or audio device.
///
/// `script` holds input to inject, as `(frame, event)` pairs. Audio is
/// mixed and discarded so clips advance exactly as they would in real
/// time. Stops early if the app quits. Returns the last frame drawn.
pub fn run_headless(
    app: &mut dyn App,
    mixer: &Mixer,
    frames: u64,
    script: &[(u64, InputEvent)],
) -> Canvas {
    let (width, height) = app.resolution();
    let mut canvas = Canvas::new(width, height);
    let mut audio = vec![0i16; SAMPLES_PER_FRAME];
    for number in 0..frames {
        let input: Vec<InputEvent> = script
            .iter()
            .filter(|(at, _)| *at == number)
            .map(|(_, e)| *e)
            .collect();
        let mut frame = Frame {
            number,
            input: &input,
            canvas: &mut canvas,
            mixer,
        };
        let flow = app.update(&mut frame);
        mixer.mix(&mut audio);
        if flow == Flow::Quit {
            break;
        }
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Color;

    /// Counts clicks and paints the screen brighter for each.
    struct Clicker {
        clicks: u8,
    }

    impl App for Clicker {
        fn title(&self) -> String {
            "clicker".into()
        }

        fn resolution(&self) -> (u32, u32) {
            (4, 4)
        }

        fn update(&mut self, frame: &mut Frame<'_>) -> Flow {
            for event in frame.input {
                if let InputEvent::MouseDown { .. } = event {
                    self.clicks += 1;
                }
            }
            frame.canvas.clear(Color::rgb(self.clicks * 10, 0, 0));
            if self.clicks >= 3 {
                Flow::Quit
            } else {
                Flow::Continue
            }
        }
    }

    #[test]
    fn headless_runs_are_deterministic() {
        let click = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: 0,
            y: 0,
        };
        let script = [(1, click), (5, click)];
        let mixer = Mixer::new(1);
        let mut app = Clicker { clicks: 0 };
        let canvas = run_headless(&mut app, &mixer, 100, &script);
        assert_eq!(canvas.pixel(0, 0), Color::rgb(20, 0, 0));
        // A third click makes it quit.
        let mut app = Clicker { clicks: 0 };
        let script = [(1, click), (2, click), (3, click)];
        let canvas = run_headless(&mut app, &mixer, 100, &script);
        assert_eq!(canvas.pixel(0, 0), Color::rgb(30, 0, 0));
    }
}
