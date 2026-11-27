//! The SDL2 frontend: a window that shows the interface scaled to fit
//! (letterboxed, keeping the aspect ratio), an audio device fed by the
//! mixer, and input translated to canvas coordinates.
//!
//! Game logic runs at a fixed [`FPS`]; drawing happens once per display
//! refresh with vsync. Alt+Enter toggles full screen.
//!
//! Text input (with the system's input method, for Japanese) is on only
//! while the interface asks for it ([`Shell::text_input`]), so that the
//! input method does not take the keys of the game.

use std::time::{Duration, Instant};

use sdl2::audio::{AudioCallback, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::mouse::MouseButton as SdlButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::video::{FullscreenType, Window};
use vnr_core::app::Frame;
use vnr_core::audio::SAMPLE_RATE;
use vnr_core::shell::Shell;
use vnr_core::ui::Rect;
use vnr_core::{App, Canvas, FPS, Flow, InputEvent, Key, Mixer, MouseButton};

/// Most logic frames run back to back when the window falls behind (for
/// example while it is being dragged); after that, time is dropped.
const MAX_CATCH_UP: u32 = 4;

struct Feed(Mixer);

impl AudioCallback for Feed {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        self.0.mix(out);
    }
}

fn key(code: Keycode) -> Option<Key> {
    Some(match code {
        Keycode::RETURN | Keycode::KP_ENTER => Key::Enter,
        Keycode::SPACE => Key::Space,
        Keycode::ESCAPE => Key::Escape,
        Keycode::LCTRL | Keycode::RCTRL => Key::Control,
        Keycode::UP => Key::Up,
        Keycode::DOWN => Key::Down,
        Keycode::LEFT => Key::Left,
        Keycode::RIGHT => Key::Right,
        Keycode::PAGEUP => Key::PageUp,
        Keycode::PAGEDOWN => Key::PageDown,
        Keycode::BACKSPACE => Key::Backspace,
        Keycode::F1 => Key::F(1),
        Keycode::F2 => Key::F(2),
        Keycode::F3 => Key::F(3),
        Keycode::F4 => Key::F(4),
        Keycode::F5 => Key::F(5),
        Keycode::F6 => Key::F(6),
        Keycode::F7 => Key::F(7),
        Keycode::F8 => Key::F(8),
        Keycode::F9 => Key::F(9),
        Keycode::F10 => Key::F(10),
        Keycode::F11 => Key::F(11),
        Keycode::F12 => Key::F(12),
        _ => {
            let c = u8::try_from(code.into_i32()).ok()?;
            return c.is_ascii_lowercase().then_some(Key::Char(char::from(c)));
        }
    })
}

fn button(b: SdlButton) -> Option<MouseButton> {
    match b {
        SdlButton::Left => Some(MouseButton::Left),
        SdlButton::Right => Some(MouseButton::Right),
        SdlButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Translates an SDL event. Mouse coordinates are already in canvas
/// pixels because the renderer has a logical size.
fn translate(event: &Event) -> Option<InputEvent> {
    match event {
        Event::TextInput { text, .. } => return Some(InputEvent::Text(text.clone())),
        Event::TextEditing { text, .. } => return Some(InputEvent::Composition(text.clone())),
        _ => {}
    }
    Some(match *event {
        Event::KeyDown {
            keycode: Some(code),
            repeat: false,
            ..
        } => InputEvent::KeyDown(key(code)?),
        Event::KeyUp {
            keycode: Some(code),
            ..
        } => InputEvent::KeyUp(key(code)?),
        Event::MouseMotion { x, y, .. } => InputEvent::MouseMove { x, y },
        Event::MouseButtonDown {
            mouse_btn, x, y, ..
        } => InputEvent::MouseDown {
            button: button(mouse_btn)?,
            x,
            y,
        },
        Event::MouseButtonUp {
            mouse_btn, x, y, ..
        } => InputEvent::MouseUp {
            button: button(mouse_btn)?,
            x,
            y,
        },
        Event::MouseWheel { y, .. } if y != 0 => InputEvent::Wheel(y),
        _ => return None,
    })
}

/// `r`, in canvas pixels, in window coordinates (the canvas is scaled to
/// fit the window, centred).
fn to_window(window: &Window, (width, height): (u32, u32), r: Rect) -> sdl2::rect::Rect {
    let (ww, wh) = window.size();
    let scale = f64::min(
        f64::from(ww) / f64::from(width),
        f64::from(wh) / f64::from(height),
    );
    let ox = (f64::from(ww) - f64::from(width) * scale) / 2.0;
    let oy = (f64::from(wh) - f64::from(height) * scale) / 2.0;
    sdl2::rect::Rect::new(
        (ox + f64::from(r.x) * scale) as i32,
        (oy + f64::from(r.y) * scale) as i32,
        (f64::from(r.w) * scale).max(1.0) as u32,
        (f64::from(r.h) * scale).max(1.0) as u32,
    )
}

/// Opens a window and runs `app` until it quits or the window is closed.
/// VNRevival's icon, for the title bar and the task bar.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/vnrevival-256.png");

/// Gives the window VNRevival's icon. A window without it still works,
/// so a failure is ignored.
fn set_icon(window: &mut Window) {
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    let Ok(mut reader) = decoder.read_info() else {
        return;
    };
    let Some(size) = reader.output_buffer_size() else {
        return;
    };
    let mut pixels = vec![0; size];
    let Ok(info) = reader.next_frame(&mut pixels) else {
        return;
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return;
    }
    if let Ok(surface) = sdl2::surface::Surface::from_data(
        &mut pixels,
        info.width,
        info.height,
        info.line_size as u32,
        PixelFormatEnum::RGBA32,
    ) {
        window.set_icon(surface);
    }
}

pub fn run(app: &mut Shell, mixer: &Mixer, scale: u32) -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let (width, height) = app.resolution();
    // Smooth scaling; must be set before the texture is created.
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "linear");
    // The input method draws its own composition and candidate windows.
    sdl2::hint::set("SDL_IME_SHOW_UI", "1");
    let text_input = video.text_input();
    // SDL starts with text input on.
    text_input.stop();
    let mut typing: Option<Rect> = None;

    let mut window = video
        .window(&app.title(), width * scale, height * scale)
        .position_centered()
        .resizable()
        .allow_highdpi()
        .build()
        .map_err(|e| e.to_string())?;
    set_icon(&mut window);
    let mut screen = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    screen
        .set_logical_size(width, height)
        .map_err(|e| e.to_string())?;
    let textures = screen.texture_creator();
    let mut texture = textures
        .create_texture_streaming(PixelFormatEnum::RGBA32, width, height)
        .map_err(|e| e.to_string())?;

    // Audio is optional: without a device the game still runs silently.
    let audio_device = sdl.audio().and_then(|audio| {
        let desired = AudioSpecDesired {
            freq: Some(SAMPLE_RATE as i32),
            channels: Some(2),
            samples: Some(1024),
        };
        audio.open_playback(None, &desired, |_| Feed(mixer.clone()))
    });
    match &audio_device {
        Ok(device) => device.resume(),
        Err(e) => eprintln!("audio disabled: {e}"),
    }

    let mut events = sdl.event_pump()?;
    let mut canvas = Canvas::new(width, height);
    let step = Duration::from_secs(1) / FPS;
    let mut next = Instant::now();
    let mut number = 0u64;
    let mut pending = Vec::new();

    'main: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'main,
                Event::KeyDown {
                    keycode: Some(Keycode::RETURN),
                    keymod,
                    repeat: false,
                    ..
                } if keymod.intersects(Mod::LALTMOD | Mod::RALTMOD) => {
                    let window = screen.window_mut();
                    let mode = match window.fullscreen_state() {
                        FullscreenType::Off => FullscreenType::Desktop,
                        _ => FullscreenType::Off,
                    };
                    window.set_fullscreen(mode)?;
                }
                other => pending.extend(translate(&other)),
            }
        }

        if let Some(request) = app.take_window_request() {
            let mode = if request.fullscreen {
                FullscreenType::Desktop
            } else {
                FullscreenType::Off
            };
            screen.window_mut().set_fullscreen(mode)?;
        }

        let mut ran = 0;
        while Instant::now() >= next && ran < MAX_CATCH_UP {
            let input = std::mem::take(&mut pending);
            let mut frame = Frame {
                number,
                input: &input,
                canvas: &mut canvas,
                mixer,
            };
            if app.update(&mut frame) == Flow::Quit {
                break 'main;
            }
            number += 1;
            next += step;
            ran += 1;
        }
        if ran == MAX_CATCH_UP {
            next = Instant::now() + step;
        }

        let wanted = app.text_input();
        if wanted != typing {
            match wanted {
                Some(r) => {
                    text_input.set_rect(to_window(screen.window(), (width, height), r));
                    if typing.is_none() {
                        text_input.start();
                    }
                }
                None => text_input.stop(),
            }
            typing = wanted;
        }

        texture
            .update(None, canvas.rgba(), width as usize * 4)
            .map_err(|e| e.to_string())?;
        screen.clear();
        screen.copy(&texture, None, None)?;
        screen.present();

        // Without vsync (some drivers ignore it) avoid spinning.
        let now = Instant::now();
        if now < next {
            std::thread::sleep((next - now).min(step));
        }
    }
    mixer.stop_all();
    Ok(())
}
