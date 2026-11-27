//! Pure Mail running on the script machine.
//!
//! [`Game`] boots like `PMAIL.EXE` (`0x424d40`): it loads `pmail.obj` for
//! its shared functions, then runs `start.obj`. The [`Machine`] executes
//! the bytecode; [`Engine`] implements the engine functions (`sys`) on top
//! of the runtime.
//!
//! Engine functions are being reverse-engineered one by one. The ones
//! implemented here, and how sure each one is:
//!
//! | `sys` | Implemented as | Basis |
//! |---|---|---|
//! | `01` | show the logged text; its voice is the pool entry after it | handler reads the log; data |
//! | `03` | hide the message window | data (always before scene changes) |
//! | `05` | speaker style and name | data |
//! | `07` | choice between the logged strings; 1-based answer into the variable | handler `0x422b80` |
//! | `10` | continue in another script | handler `0x423440` |
//! | `12` | chapter title | data |
//! | `21`, `22` | wait (milliseconds) | data |
//! | `30` | background, clears characters | handler `0x4247b0` |
//! | `31` | character in a slot | data |
//! | `40` | show the staged scene through a transition and wait for it | handler `0x423d40`; see [`crate::transition`] |
//! | `45` | fill the scene with a colour | data, provisional |
//! | `60` | sound effect | data |
//! | `70`, `73` | music track `MM%03d`, stop music | data, provisional |
//! | `80` | voice | data (`usr 113`) |
//! | `d5` | title menu; 1-based choice into the variable | `start.obj` |
//! | `e1` | load menu (title); closing it without loading resumes the script | `start.obj` |
//! | `04` | history entry only (the text is in pictures or windows) | handler `0x422d80` |
//! | `20` | wait for a click | data (always before `83`) |
//! | `63`, `83` | stop the sound effect, the voice | `0x426e00`, `0x424590` |
//! | `65`, `85` | wait until the sound effect, the voice ends | `0x424000`, `0x4245c0` |
//! | `a0`–`c3`, `33`, `34`, `50` | the in-game PC | see [`crate::pc`] |
//!
//! Everything else is ignored for now and reported once on stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vnr_core::text::find_system_font;
use vnr_core::{
    App, BackendError, Canvas, Color, Flow, Font, FontFamily, Frame, InputEvent, Key, Mixer,
    MouseButton, Sprite, TextStyle,
};

use crate::assets::Assets;
use crate::backlog::{self, Action, Entry, History};
use crate::ini::Settings;
use crate::pc::{self, Label, Line, Mail, PcView, Popup};
use crate::prefs::{self, Preferences};
use crate::save::{self, EngineState, SaveFile, SavedMode, SceneState, SlotSummary, Slots};
use crate::transition::{self, Effect, MorningPictures, Transition};
use crate::vm::{Control, Host, Machine, Status, Value};

/// Screen size of the engine.
pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

/// Mixer channels.
pub const CHANNEL_MUSIC: usize = 0;
pub const CHANNEL_SOUND: usize = 1;
pub const CHANNEL_VOICE: usize = 2;
pub const CHANNELS: usize = 3;

/// Fade speed when a picture change is shown without `sys 40`
/// (240 ms).
const IMPLICIT_FADE_STEP: u32 = 16;
/// Script with the shared functions, loaded first.
const LIBRARY: &str = "pmail";
const STEP_BUDGET: usize = 200_000;
const TEXT_SIZE: f32 = 22.0;
const LINE_HEIGHT: f32 = 30.0;
const BOX: (i32, i32, u32, u32) = (20, 424, 760, 166);

/// Replaces engine placeholders with something readable. The real names
/// come from the player's settings, which are not implemented yet.
fn clean_text(text: &str) -> String {
    text.replace("\\n", "\n")
        .replace("$N0", "主人公")
        .replace("$H0", "ＨＮ")
}

struct Fonts {
    mincho: Font,
    gothic: Font,
}

impl Fonts {
    fn load(override_path: Option<&Path>) -> Result<Self, BackendError> {
        let load = |family| -> Result<Font, BackendError> {
            let (path, index) = match override_path {
                Some(path) => (path.to_path_buf(), 0),
                None => find_system_font(family)
                    .ok_or("no Japanese font found on this system; pass one with --font")?,
            };
            Ok(Font::load(&path, index)?)
        };
        Ok(Self {
            mincho: load(FontFamily::Mincho)?,
            gothic: load(FontFamily::Gothic)?,
        })
    }

    fn get(&mut self, family: FontFamily) -> &mut Font {
        match family {
            FontFamily::Mincho => &mut self.mincho,
            FontFamily::Gothic => &mut self.gothic,
        }
    }
}

/// What the game is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Running the script.
    Running,
    /// A message is on screen.
    Message {
        text: String,
        speaker: Option<String>,
        style: usize,
        shown: usize,
    },
    /// A choice; the answer goes into `variable`.
    Choice {
        options: Vec<String>,
        variable: Option<u16>,
        hover: Option<usize>,
    },
    /// The title menu.
    Title {
        variable: Option<u16>,
        hover: Option<usize>,
    },
    /// Waiting until a frame number.
    Wait(u64),
    /// Waiting for a click.
    Click,
    /// Waiting for a transition to end.
    Transition,
    /// Stopped while a menu is open.
    Paused,
    /// Waiting until a mixer channel is silent (a click skips).
    Channel(usize),
    /// The script stopped.
    Ended(String),
}

/// What is on screen, apart from text.
#[derive(Clone, Default)]
struct Scene {
    fill: Option<Color>,
    background: Option<Arc<Sprite>>,
    characters: BTreeMap<i32, Arc<Sprite>>,
    /// The PC screen drawn over the background.
    pc: PcView,
    /// Bumped on every change to `pc`, to notice them cheaply.
    pc_revision: u64,
    /// Names of the background and character pictures, for saves.
    background_name: Option<String>,
    character_names: BTreeMap<i32, String>,
}

impl Scene {
    fn draw(&self, canvas: &mut Canvas, font: &mut Font) {
        canvas.clear(self.fill.unwrap_or(Color::BLACK));
        if let Some(bg) = &self.background {
            canvas.draw(bg, 0, 0, 255);
        }
        for sprite in self.characters.values() {
            canvas.draw(sprite, 0, 0, 255);
        }
        self.pc.draw(canvas, font);
    }

    /// Clears what a new background or fill replaces.
    fn clear_layers(&mut self) {
        self.characters.clear();
        self.character_names.clear();
        self.pc.clear();
        self.pc_revision += 1;
    }

    fn same_picture(&self, other: &Scene) -> bool {
        self.fill == other.fill
            && opt_ptr(&self.background) == opt_ptr(&other.background)
            && self.characters.len() == other.characters.len()
            && self
                .characters
                .iter()
                .zip(&other.characters)
                .all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y))
    }
}

/// The engine side: assets, presentation state and the `sys` functions.
pub struct Engine {
    assets: Assets,
    settings: Settings,
    fonts: Fonts,
    mixer: Option<Mixer>,
    frame: u64,

    mode: Mode,
    /// Scene being built by the script.
    staged: Scene,
    /// Scene on screen.
    shown: Scene,
    shown_canvas: Canvas,
    /// Transition in progress.
    transition: Option<Transition>,
    /// `GrpWipeWait` (ms per transition step).
    wipe_wait: u32,
    /// Pictures of the morning wipe, once loaded (`None` inside if
    /// missing).
    morning: Option<Option<Arc<MorningPictures>>>,
    window_visible: bool,
    speaker: (usize, String),
    chapter: String,
    engine_vars: BTreeMap<u16, i32>,
    unknown: BTreeSet<u8>,
    skipping: bool,
    hidden: bool,

    /// Chat lines since the last `sys a1`.
    chat_log: Vec<Line>,
    /// Lines of the mail being built (`sys b2`).
    mail_lines: Vec<Line>,
    /// Paragraphs of the letter being built (`sys c2`).
    letter: Vec<String>,
    /// Pop-up animations on screen.
    popups: Vec<Popup>,

    /// Names of the scripts loaded so far, by id (for saves).
    script_names: BTreeMap<u16, String>,
    /// Music track playing.
    music: Option<i32>,
    /// The script asked for the load menu (`sys e1`).
    load_requested: bool,
    /// Message history.
    history: History,
    prefs: Preferences,
    /// Fraction of a character of text still to reveal.
    reveal: f32,
}

impl Engine {
    fn play(&self, channel: usize, clip: Option<Arc<vnr_core::Clip>>) {
        if let (Some(mixer), Some(clip)) = (&self.mixer, clip) {
            mixer.play(channel, clip, self.channel_volume(channel));
        }
    }

    /// Mixer volume of a channel from the settings.
    fn channel_volume(&self, channel: usize) -> u16 {
        Preferences::volume(match channel {
            CHANNEL_MUSIC => self.prefs.music,
            CHANNEL_SOUND => self.prefs.sound,
            _ => self.prefs.voice,
        })
    }

    /// Applies changed settings to what is playing.
    fn apply_prefs(&self) {
        if let Some(mixer) = &self.mixer {
            for channel in [CHANNEL_MUSIC, CHANNEL_SOUND, CHANNEL_VOICE] {
                mixer.set_volume(channel, self.channel_volume(channel));
            }
        }
    }

    /// Characters to reveal this frame (everything when text is instant).
    fn reveal_step(&mut self) -> usize {
        match self.prefs.chars_per_second() {
            Some(cps) => {
                self.reveal += cps / vnr_core::FPS as f32;
                let n = self.reveal.floor();
                self.reveal -= n;
                n as usize
            }
            None => usize::MAX / 2,
        }
    }

    /// Starts music track `MM%03d`, or stops the music.
    fn play_music(&mut self, track: Option<i32>) {
        self.music = track;
        match track {
            Some(track) => {
                let clip = self.assets.sound(&format!("mm{track:03}"));
                self.play(CHANNEL_MUSIC, clip);
            }
            None => self.stop(CHANNEL_MUSIC),
        }
    }

    /// `ASABASE` and `ASAFLASH`, loaded on first use.
    fn morning_pictures(&mut self) -> Option<Arc<MorningPictures>> {
        if self.morning.is_none() {
            let base = self.assets.gray("ASABASE.ALPF");
            let flash = self.assets.gray("ASAFLASH.ALPA");
            self.morning = Some(match (base, flash) {
                (Some((200, 150, base)), Some((250, 200, flash))) => {
                    Some(Arc::new(MorningPictures { base, flash }))
                }
                _ => None,
            });
        }
        self.morning.clone().flatten()
    }

    fn stop(&self, channel: usize) {
        if let Some(mixer) = &self.mixer {
            mixer.stop(channel);
        }
    }

    /// Waits until `channel` is silent.
    fn wait_channel(&mut self, channel: usize) -> Control {
        let playing = self.mixer.as_ref().is_some_and(|m| m.is_playing(channel));
        if !playing || self.skipping {
            return Control::Continue;
        }
        self.present_if_changed();
        self.mode = Mode::Channel(channel);
        Control::Yield
    }

    /// What a new background or fill clears: characters, everything on
    /// the PC screen except the chat log, and pop-ups.
    fn clear_pc_layers(&mut self) {
        self.staged.clear_layers();
        self.mail_lines.clear();
        self.popups.clear();
    }

    /// Shows the staged scene through `effect` at progress step `step`.
    fn present(&mut self, effect: Effect, step: u32) {
        self.transition = match effect {
            Effect::Cut => None,
            _ if self.skipping => None,
            effect => Some(Transition::new(
                effect,
                self.shown_canvas.clone(),
                self.frame,
                step,
                self.wipe_wait,
            )),
        };
        self.shown = self.staged.clone();
        self.shown
            .draw(&mut self.shown_canvas, self.fonts.get(FontFamily::Gothic));
    }

    /// Shows the staged scene if it differs from the one on screen:
    /// picture changes fade, PC text changes appear at once.
    fn present_if_changed(&mut self) {
        if !self.shown.same_picture(&self.staged) {
            self.present(Effect::Fade, IMPLICIT_FADE_STEP);
        } else if self.shown.pc_revision != self.staged.pc_revision {
            self.present(Effect::Cut, 1);
        }
    }

    /// Changes the PC view of the staged scene.
    fn stage_pc(&mut self, change: impl FnOnce(&mut PcView)) {
        change(&mut self.staged.pc);
        self.staged.pc_revision += 1;
    }

    /// Adds a chat line, shows the log and waits for a click.
    fn chat_line(&mut self, vm: &Machine, args: &[Value]) -> Control {
        let Some(line) = self.line_arg(vm, args) else {
            return Control::Continue;
        };
        let voice = args.iter().find_map(|&v| voice_after(vm, v));
        if let Some(voice) = &voice {
            let clip = self.assets.voice(voice);
            self.play(CHANNEL_VOICE, clip);
        }
        self.history.push(Entry {
            speaker: None,
            text: line.text.clone(),
            voice,
        });
        self.chat_log.push(line);
        let tail = self.chat_tail();
        self.stage_pc(|pc| pc.chat = Some(tail));
        self.present_if_changed();
        if self.skipping {
            return Control::Continue;
        }
        self.mode = Mode::Click;
        Control::Yield
    }

    /// The end of the chat log: enough lines to fill the window.
    fn chat_tail(&self) -> Vec<Line> {
        let start = self.chat_log.len().saturating_sub(pc::CHAT_ROWS);
        self.chat_log[start..].to_vec()
    }

    /// `(style, text)` arguments as a [`Line`].
    fn line_arg(&mut self, vm: &Machine, args: &[Value]) -> Option<Line> {
        let style = args.first().map_or(0, |&v| vm.number(self, v));
        let text = args.get(1).and_then(|&v| vm.text(v))?;
        Some(Line {
            style,
            text: clean_text(&text),
        })
    }

    fn message(&mut self, vm: &mut Machine) -> Control {
        let logged = vm.take_log_values();
        let mut text = String::new();
        let mut voice = None;
        for value in &logged {
            if let Some(t) = vm.text(*value) {
                text.push_str(&t);
            }
            // The voice file is the pool entry right after the text.
            voice = vm
                .pool_text(value.script(), value.low().wrapping_add(1))
                .filter(|v| !v.is_empty())
                .or(voice);
        }
        if let Some(voice) = &voice {
            let clip = self.assets.voice(voice);
            self.play(CHANNEL_VOICE, clip);
        }
        let text = clean_text(&text);
        if text.trim().is_empty() {
            return Control::Continue;
        }
        self.present_if_changed();
        self.window_visible = true;
        let speaker = clean_text(&self.speaker.1);
        self.history.push(Entry {
            speaker: (!speaker.is_empty()).then(|| speaker.clone()),
            text: text.clone(),
            voice,
        });
        self.mode = Mode::Message {
            text,
            speaker: (!speaker.is_empty()).then_some(speaker),
            style: self.speaker.0,
            shown: 0,
        };
        Control::Yield
    }

    fn report_unknown(&mut self, number: u8) {
        if self.unknown.insert(number) {
            eprintln!(
                "note: {} is not implemented yet",
                crate::vm::sys_label(number)
            );
        }
    }
}

/// The voice file stored as the pool entry after a text argument, if
/// that entry looks like one (`V…`, letters and digits).
fn voice_after(vm: &Machine, value: Value) -> Option<String> {
    vm.text(value)?;
    let next = vm.pool_text(value.script(), value.low().wrapping_add(1))?;
    let looks_like_voice = next.len() >= 5
        && next.starts_with(['V', 'v'])
        && next.bytes().all(|b| b.is_ascii_alphanumeric());
    looks_like_voice.then_some(next)
}

fn opt_ptr(sprite: &Option<Arc<Sprite>>) -> Option<*const Sprite> {
    sprite.as_ref().map(Arc::as_ptr)
}

impl Host for Engine {
    fn sys(&mut self, vm: &mut Machine, number: u8, args: &[Value]) -> Control {
        let num = |vm: &Machine, host: &mut Self, i: usize| {
            args.get(i).map_or(0, |&v| vm.number(host, v))
        };
        let text = |i: usize| args.get(i).and_then(|&v| vm.text(v)).unwrap_or_default();
        match number {
            0x01 => return self.message(vm),
            0x03 => self.window_visible = false,
            0x05 => {
                let style = num(vm, self, 0).max(0) as usize;
                self.speaker = (style, text(1));
            }
            0x07 => {
                let options: Vec<String> = vm.take_log().iter().map(|t| clean_text(t)).collect();
                let variable = args.iter().find_map(|&v| vm.variable_ref(v));
                self.present_if_changed();
                self.window_visible = true;
                self.mode = Mode::Choice {
                    options,
                    variable,
                    hover: None,
                };
                return Control::Yield;
            }
            0x10 => {
                let name = text(0);
                match self.assets.script(&name) {
                    Ok(script) => {
                        self.script_names.insert(script.id, name);
                        vm.start(script);
                    }
                    Err(e) => {
                        self.mode = Mode::Ended(e.to_string());
                        return Control::Yield;
                    }
                }
            }
            0x12 => self.chapter = text(0),
            0x21 | 0x22 => {
                let ms = num(vm, self, 0).clamp(0, 10_000) as u64;
                if !self.skipping && ms > 0 {
                    self.present_if_changed();
                    self.mode = Mode::Wait(self.frame + (ms * 60).div_ceil(1000));
                    return Control::Yield;
                }
            }
            0x30 => {
                let name = text(0);
                self.staged.background = self.assets.picture(&name, &["crgb"]);
                self.staged.background_name = Some(name);
                self.staged.fill = None;
                self.clear_pc_layers();
            }
            0x31 => {
                let slot = num(vm, self, 0);
                let name = text(2);
                match self.assets.picture(&name, &["char"]) {
                    Some(sprite) => {
                        self.staged.characters.insert(slot, sprite);
                        self.staged.character_names.insert(slot, name);
                    }
                    None => {
                        self.staged.characters.remove(&slot);
                        self.staged.character_names.remove(&slot);
                    }
                }
            }
            0x40 => {
                let kind = num(vm, self, 0);
                let mode = num(vm, self, 1);
                let step = transition::step(num(vm, self, 2));
                let effect = match kind {
                    0 => Effect::Cut,
                    3 => match self.morning_pictures() {
                        Some(pictures) => Effect::morning(mode, pictures),
                        None => Effect::Fade,
                    },
                    _ => Effect::Fade,
                };
                self.present(effect, step);
                // The engine returns only when the effect is over.
                if self.transition.is_some() {
                    self.mode = Mode::Transition;
                    return Control::Yield;
                }
            }
            0x45 => {
                let c = |v: i32| v.clamp(0, 255) as u8;
                self.staged.fill = Some(Color::rgb(
                    c(num(vm, self, 0)),
                    c(num(vm, self, 1)),
                    c(num(vm, self, 2)),
                ));
                self.staged.background = None;
                self.staged.background_name = None;
                self.clear_pc_layers();
            }
            // Backlog entry; the visible text of these lines is in the
            // pictures or the mail window.
            0x04 => {
                let voice = text(2);
                self.history.push(Entry {
                    speaker: None,
                    text: clean_text(&text(1)),
                    voice: (!voice.is_empty()).then_some(voice),
                });
            }
            0x20 => {
                if !self.skipping {
                    self.present_if_changed();
                    self.mode = Mode::Click;
                    return Control::Yield;
                }
            }
            // Desktop shortcuts to other installed games; screen backup.
            0x32 | 0x41 => {}
            0x33 => {
                let kind = num(vm, self, 0);
                let at = (num(vm, self, 1), num(vm, self, 2));
                if let Some(&(name, w, h, ms, wait, looping)) =
                    usize::try_from(kind).ok().and_then(|k| pc::POPUPS.get(k))
                    && let Some(frames) = self.assets.animation(name, w, h)
                {
                    self.present_if_changed();
                    let popup = Popup {
                        frames,
                        at,
                        start: self.frame,
                        ticks: (u64::from(ms) * 60).div_ceil(1000).max(1),
                        looping,
                    };
                    let end = self.frame + popup.length();
                    self.popups.push(popup);
                    if wait && !self.skipping {
                        self.mode = Mode::Wait(end);
                        return Control::Yield;
                    }
                }
            }
            0x34 => {
                let label = Label {
                    text: clean_text(&text(0)),
                    rect: (
                        num(vm, self, 1),
                        num(vm, self, 2),
                        num(vm, self, 3).max(0) as u32,
                        num(vm, self, 4).max(0) as u32,
                    ),
                    size: num(vm, self, 5).clamp(8, 64) as f32,
                    gap: num(vm, self, 6).clamp(0, 64) as f32,
                };
                self.stage_pc(|pc| {
                    if pc.labels.len() < pc::MAX_LABELS {
                        pc.labels.push(label);
                    }
                });
            }
            0x50 => {
                let clock = text(0).trim().to_string();
                self.stage_pc(|pc| pc.clock = Some(clock));
            }
            0x63 => self.stop(CHANNEL_SOUND),
            0x65 => return self.wait_channel(CHANNEL_SOUND),
            0x83 => self.stop(CHANNEL_VOICE),
            0x85 => return self.wait_channel(CHANNEL_VOICE),
            0xA0 | 0xA2 | 0xA4 => return self.chat_line(vm, args),
            0xA1 => {
                self.chat_log.clear();
                self.stage_pc(|pc| pc.chat = None);
            }
            0xA3 => {
                let tail = self.chat_tail();
                self.stage_pc(|pc| pc.chat = Some(tail));
            }
            0xB0 | 0xB2 => {
                if let Some(line) = self.line_arg(vm, args) {
                    if number == 0xB0
                        && let Some(voice) = args.iter().find_map(|&v| voice_after(vm, v))
                    {
                        let clip = self.assets.voice(&voice);
                        self.play(CHANNEL_VOICE, clip);
                    }
                    self.mail_lines.push(line);
                }
            }
            0xB1 => {
                self.mail_lines.clear();
                self.stage_pc(|pc| pc.mail = None);
            }
            0xB3 => {
                let frame = pc::MAIL_FRAME.map(|name| self.assets.picture(name, &["img"]));
                let mail = Mail {
                    rect: (
                        num(vm, self, 0),
                        num(vm, self, 1),
                        num(vm, self, 2).max(0) as u32,
                        num(vm, self, 3).max(0) as u32,
                    ),
                    size: num(vm, self, 4).clamp(8, 64) as f32,
                    gap: num(vm, self, 5).clamp(0, 64) as f32,
                    lines: self.mail_lines.clone(),
                    frame,
                };
                self.stage_pc(|pc| pc.mail = Some(mail));
                // Scripts close the window right after; the player reads
                // it first.
                self.present_if_changed();
                if !self.skipping {
                    self.mode = Mode::Click;
                    return Control::Yield;
                }
            }
            0xC1 => {
                self.letter.clear();
                self.stage_pc(|pc| pc.letter = None);
            }
            0xC2 => self.letter.push(clean_text(&text(1))),
            0xC3 => {
                let page = usize::try_from(num(vm, self, 0))
                    .ok()
                    .and_then(|i| self.letter.get(i))
                    .cloned();
                self.stage_pc(|pc| pc.letter = page);
            }
            0x60 => {
                let clip = self.assets.sound(&text(0));
                self.play(CHANNEL_SOUND, clip);
            }
            0x70 => {
                let track = num(vm, self, 0);
                self.play_music(Some(track));
            }
            0x73 => self.play_music(None),
            0xE1 => {
                self.load_requested = true;
                self.mode = Mode::Paused;
                return Control::Yield;
            }
            0x80 => {
                let clip = self.assets.voice(&text(1));
                self.play(CHANNEL_VOICE, clip);
            }
            0xD5 => {
                let variable = args.iter().find_map(|&v| vm.variable_ref(v));
                self.staged = Scene {
                    background: self.assets.picture("TITL00a", &["crgb"]),
                    background_name: Some("TITL00a".into()),
                    ..Scene::default()
                };
                self.present(Effect::Fade, IMPLICIT_FADE_STEP);
                self.window_visible = false;
                self.chapter.clear();
                self.mode = Mode::Title {
                    variable,
                    hover: None,
                };
                return Control::Yield;
            }
            other => self.report_unknown(other),
        }
        Control::Continue
    }

    fn engine_variable(&mut self, id: u16) -> i32 {
        self.engine_vars.get(&id).copied().unwrap_or(0)
    }

    fn set_engine_variable(&mut self, id: u16, value: i32) {
        self.engine_vars.insert(id, value);
    }
}

/// Title menu buttons (`TITL01`–`TITL03.IMG`, 423 × 55): new game, load,
/// extras. Their real placement is not reverse-engineered yet.
const TITLE_BUTTONS: [&str; 3] = ["TITL01", "TITL02", "TITL03"];

fn title_button_rect(i: usize) -> (i32, i32, u32, u32) {
    (188, 330 + i as i32 * 70, 423, 55)
}

fn choice_rect(i: usize) -> (i32, i32, u32, u32) {
    let (x, y, w, _) = BOX;
    (
        x + 10,
        y + 12 + i as i32 * LINE_HEIGHT as i32,
        w - 20,
        LINE_HEIGHT as u32,
    )
}

fn hit(rect: (i32, i32, u32, u32), x: i32, y: i32) -> bool {
    x >= rect.0 && y >= rect.1 && x < rect.0 + rect.2 as i32 && y < rect.1 + rect.3 as i32
}

/// The game.
pub struct Game {
    title: String,
    vm: Machine,
    engine: Engine,
    /// Where saves go; `None` disables saving.
    slots: Option<Slots>,
    menu: Option<Menu>,
    /// The message history, while open.
    backlog: Option<backlog::View>,
    /// The settings screen, while open.
    settings: Option<prefs::View>,
}

/// The save or load menu.
struct Menu {
    saving: bool,
    slots: Vec<Option<SlotSummary>>,
    hover: Option<usize>,
    /// Opened by the script (`sys e1`): closing it resumes the script.
    resume: bool,
    error: Option<String>,
}

const MENU_PANEL: (i32, i32, u32, u32) = (60, 30, 680, 540);

fn menu_row(i: usize) -> (i32, i32, u32, u32) {
    (80, 80 + i as i32 * 47, 640, 44)
}

impl Game {
    /// Boots the game; with `entry`, starts that script instead of
    /// `start.obj`.
    pub fn new(
        root: &Path,
        title: String,
        entry: Option<&str>,
        font: Option<PathBuf>,
    ) -> Result<Self, BackendError> {
        let mut assets = Assets::open(root)?;
        let mut vm = Machine::new();
        let mut script_names = BTreeMap::new();
        let library = assets.script(LIBRARY)?;
        script_names.insert(library.id, LIBRARY.to_string());
        vm.load_library(library);
        let entry = entry.unwrap_or("start");
        let first = assets.script(entry)?;
        script_names.insert(first.id, entry.to_string());
        vm.start(first);
        let settings = Settings::load(vnr_core::find_ci(root, "OF_ADV2.INI").as_deref());
        let wipe_wait = settings
            .int("GrpWipeWait")
            .map_or(transition::DEFAULT_WAIT_MS, |v| v.clamp(1, 1000) as u32);
        let engine = Engine {
            assets,
            settings,
            fonts: Fonts::load(font.as_deref())?,
            mixer: None,
            frame: 0,
            mode: Mode::Running,
            staged: Scene::default(),
            shown: Scene::default(),
            shown_canvas: Canvas::new(WIDTH, HEIGHT),
            transition: None,
            wipe_wait,
            morning: None,
            window_visible: false,
            speaker: (0, String::new()),
            chapter: String::new(),
            engine_vars: BTreeMap::new(),
            unknown: BTreeSet::new(),
            skipping: false,
            hidden: false,
            chat_log: Vec::new(),
            mail_lines: Vec::new(),
            letter: Vec::new(),
            popups: Vec::new(),
            script_names,
            music: None,
            load_requested: false,
            history: History::default(),
            prefs: Preferences::default(),
            reveal: 0.0,
        };
        Ok(Self {
            title,
            vm,
            engine,
            slots: None,
            menu: None,
            backlog: None,
            settings: None,
        })
    }

    /// Sets the folder for saved games; `None` disables saving.
    pub fn set_save_dir(&mut self, dir: Option<PathBuf>) {
        self.slots = dir.map(Slots::new);
        if let Some(path) = self.prefs_path() {
            self.engine.prefs = Preferences::load(&path);
            self.engine.apply_prefs();
        }
    }

    /// `settings.json` in the save folder.
    fn prefs_path(&self) -> Option<PathBuf> {
        self.slots.as_ref().map(|s| s.dir().join("settings.json"))
    }

    fn close_settings(&mut self) {
        let Some(view) = self.settings.take() else {
            return;
        };
        self.engine.prefs = view.prefs;
        self.engine.apply_prefs();
        if let Some(path) = self.prefs_path()
            && let Err(e) = view.prefs.store(&path)
        {
            eprintln!("could not save settings to {}: {e}", path.display());
        }
    }

    /// Whether the game is at a point where it can be saved.
    fn can_save(&self) -> bool {
        matches!(
            self.engine.mode,
            Mode::Message { .. } | Mode::Choice { .. } | Mode::Click
        )
    }

    /// The current state as a save, if the game is waiting for the
    /// player.
    fn save_state(&self) -> Result<SaveFile, String> {
        let e = &self.engine;
        let (mode, excerpt) = match &e.mode {
            Mode::Message {
                text,
                speaker,
                style,
                ..
            } => (
                SavedMode::Message {
                    text: text.clone(),
                    speaker: speaker.clone(),
                    style: *style,
                },
                text.clone(),
            ),
            Mode::Choice {
                options, variable, ..
            } => (
                SavedMode::Choice {
                    options: options.clone(),
                    variable: *variable,
                },
                options.join(" / "),
            ),
            Mode::Click => {
                let shown = &e.shown.pc;
                let text = shown
                    .mail
                    .as_ref()
                    .and_then(|m| m.lines.first())
                    .or_else(|| shown.chat.as_ref().and_then(|c| c.last()))
                    .map(|l| l.text.clone())
                    .or_else(|| shown.letter.clone())
                    .unwrap_or_default();
                (SavedMode::Click, text)
            }
            _ => return Err("the game can only be saved while it waits for you".into()),
        };
        let machine = self.vm.snapshot();
        let scripts = Machine::scripts_used(&machine)
            .into_iter()
            .map(|id| {
                e.script_names
                    .get(&id)
                    .map(|name| (id, name.clone()))
                    .ok_or_else(|| format!("script {id} has no known name"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let scene = &e.shown;
        Ok(SaveFile {
            version: save::VERSION,
            saved_at: save::now(),
            chapter: clean_text(&e.chapter),
            excerpt: save::excerpt(&excerpt, 40),
            scripts,
            machine,
            engine: EngineState {
                scene: SceneState {
                    fill: scene.fill.map(|c| [c.r, c.g, c.b]),
                    background: scene.background_name.clone(),
                    characters: scene
                        .character_names
                        .iter()
                        .map(|(slot, name)| (*slot, name.clone()))
                        .collect(),
                    pc: scene.pc.clone(),
                },
                music: e.music,
                chapter: e.chapter.clone(),
                speaker: e.speaker.clone(),
                window_visible: e.window_visible,
                engine_variables: e.engine_vars.iter().map(|(k, v)| (*k, *v)).collect(),
                chat_log: e.chat_log.clone(),
                mail_lines: e.mail_lines.clone(),
                letter: e.letter.clone(),
                history: e.history.clone(),
                mode,
            },
        })
    }

    /// Saves into `slot`.
    pub fn save(&mut self, slot: usize) -> Result<(), String> {
        let slots = self.slots.as_ref().ok_or("saving is disabled")?;
        let save = self.save_state()?;
        slots
            .write(slot, &save)
            .map_err(|e| format!("{}: {e}", slots.dir().display()))
    }

    /// Loads `slot`; an empty slot is an error.
    pub fn load(&mut self, slot: usize) -> Result<(), String> {
        let slots = self.slots.as_ref().ok_or("saving is disabled")?;
        let save = slots
            .read(slot)
            .map_err(|e| e.to_string())?
            .ok_or("this slot is empty")?;
        self.restore(save)
    }

    /// Continues from a save.
    fn restore(&mut self, save: SaveFile) -> Result<(), String> {
        let e = &mut self.engine;
        // Build the machine first, so a broken save changes nothing.
        let mut vm = Machine::new();
        let library = e.assets.script(LIBRARY).map_err(|e| e.to_string())?;
        let mut names = BTreeMap::from([(library.id, LIBRARY.to_string())]);
        vm.load_library(library);
        for (id, name) in &save.scripts {
            let script = e.assets.script(name).map_err(|e| e.to_string())?;
            if script.id != *id {
                return Err(format!(
                    "{name} is script {}, the save expects {id}",
                    script.id
                ));
            }
            names.insert(*id, name.clone());
            vm.load_library(script);
        }
        vm.restore(&save.machine)?;
        self.vm = vm;

        let state = save.engine;
        e.script_names.extend(names);
        let mut scene = Scene {
            fill: state.scene.fill.map(|[r, g, b]| Color::rgb(r, g, b)),
            ..Scene::default()
        };
        if let Some(name) = state.scene.background {
            scene.background = e.assets.picture(&name, &["crgb"]);
            scene.background_name = Some(name);
        }
        for (slot, name) in state.scene.characters {
            if let Some(sprite) = e.assets.picture(&name, &["char"]) {
                scene.characters.insert(slot, sprite);
                scene.character_names.insert(slot, name);
            }
        }
        scene.pc = state.scene.pc;
        if let Some(mail) = &mut scene.pc.mail {
            mail.frame = pc::MAIL_FRAME.map(|name| e.assets.picture(name, &["img"]));
        }
        scene.pc_revision = e.staged.pc_revision + 1;
        e.staged = scene;
        e.present(Effect::Cut, 1);
        e.popups.clear();
        e.stop(CHANNEL_VOICE);
        e.stop(CHANNEL_SOUND);
        e.play_music(state.music);
        e.chapter = state.chapter;
        e.speaker = state.speaker;
        e.window_visible = state.window_visible;
        e.engine_vars = state.engine_variables.into_iter().collect();
        e.chat_log = state.chat_log;
        e.mail_lines = state.mail_lines;
        e.letter = state.letter;
        e.history = state.history;
        e.skipping = false;
        e.hidden = false;
        e.load_requested = false;
        e.mode = match state.mode {
            SavedMode::Message {
                text,
                speaker,
                style,
            } => Mode::Message {
                shown: text.chars().count(),
                text,
                speaker,
                style,
            },
            SavedMode::Choice { options, variable } => Mode::Choice {
                options,
                variable,
                hover: None,
            },
            SavedMode::Click => Mode::Click,
        };
        Ok(())
    }

    fn open_menu(&mut self, saving: bool, resume: bool) {
        let Some(slots) = &self.slots else {
            if resume {
                self.engine.mode = Mode::Running;
            }
            return;
        };
        if saving && !self.can_save() {
            return;
        }
        self.menu = Some(Menu {
            saving,
            slots: slots.summaries(),
            hover: None,
            resume,
            error: None,
        });
    }

    fn close_menu(&mut self) {
        if let Some(menu) = self.menu.take()
            && menu.resume
            && self.engine.mode == Mode::Paused
        {
            self.engine.mode = Mode::Running;
        }
    }

    fn menu_input(&mut self, frame: &Frame<'_>) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let mut picked = None;
        let mut close = false;
        for event in frame.input {
            match *event {
                InputEvent::MouseMove { x, y } => {
                    menu.hover = (0..save::SLOTS).find(|&i| hit(menu_row(i), x, y));
                }
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    picked = (0..save::SLOTS).find(|&i| hit(menu_row(i), x, y));
                    if picked.is_none() && !hit(MENU_PANEL, x, y) {
                        close = true;
                    }
                }
                InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                }
                | InputEvent::KeyDown(Key::Escape) => close = true,
                InputEvent::KeyDown(Key::Up) => {
                    menu.hover = Some(
                        menu.hover
                            .map_or(0, |i| (i + save::SLOTS - 1) % save::SLOTS),
                    );
                }
                InputEvent::KeyDown(Key::Down) => {
                    menu.hover = Some(menu.hover.map_or(0, |i| (i + 1) % save::SLOTS));
                }
                InputEvent::KeyDown(Key::Enter | Key::Space) => picked = menu.hover,
                _ => {}
            }
        }
        if close {
            self.close_menu();
            return;
        }
        let Some(slot) = picked else {
            return;
        };
        let saving = menu.saving;
        if !saving && menu.slots[slot].is_none() {
            return;
        }
        let result = if saving {
            self.save(slot)
        } else {
            self.load(slot)
        };
        match result {
            Ok(()) => {
                // A load replaces the paused script: do not resume it.
                if !saving && let Some(menu) = &mut self.menu {
                    menu.resume = false;
                }
                self.close_menu();
            }
            Err(e) => {
                if let Some(menu) = &mut self.menu {
                    menu.error = Some(e);
                }
            }
        }
    }

    fn draw_menu(&mut self, canvas: &mut Canvas) {
        let Some(menu) = &self.menu else {
            return;
        };
        let font = self.engine.fonts.get(FontFamily::Gothic);
        canvas.fill_rect(0, 0, WIDTH, HEIGHT, Color::rgba(0, 0, 0, 140));
        let (px, py, pw, ph) = MENU_PANEL;
        canvas.fill_rect(px, py, pw, ph, Color::rgba(20, 30, 60, 230));
        let style = |size: f32, color: Color| TextStyle {
            size,
            color,
            shadow: Some(Color::BLACK),
            bold: false,
        };
        let title = if menu.saving {
            "セーブ"
        } else {
            "ロード"
        };
        font.draw_line(
            canvas,
            (px + 20) as f32,
            (py + 12) as f32,
            title,
            style(22.0, Color::WHITE),
        );
        for (i, slot) in menu.slots.iter().enumerate() {
            let (x, y, w, h) = menu_row(i);
            let usable = menu.saving || slot.is_some();
            let back = if menu.hover == Some(i) && usable {
                Color::rgba(255, 120, 160, 90)
            } else {
                Color::rgba(255, 255, 255, 25)
            };
            canvas.fill_rect(x, y, w, h, back);
            let (line1, line2) = match slot {
                Some(s) => (
                    format!("{:02}  {}  {}", i + 1, s.saved_at, s.chapter),
                    s.excerpt.clone(),
                ),
                None => (format!("{:02}  ――", i + 1), String::new()),
            };
            font.draw_line(
                canvas,
                (x + 10) as f32,
                (y + 3) as f32,
                &line1,
                style(16.0, Color::WHITE),
            );
            font.draw_line(
                canvas,
                (x + 46) as f32,
                (y + 23) as f32,
                &line2,
                style(14.0, Color::rgb(200, 210, 230)),
            );
        }
        if let Some(error) = &menu.error {
            font.draw_line(
                canvas,
                (px + 20) as f32,
                (py + ph as i32 - 22) as f32,
                error,
                style(14.0, Color::rgb(255, 140, 140)),
            );
        }
    }

    fn input(&mut self, frame: &Frame<'_>) -> Option<Flow> {
        if self.menu.is_some() {
            self.menu_input(frame);
            return None;
        }
        if let Some(view) = &mut self.settings {
            let close = view.input(frame.input);
            // Hear the new volumes while adjusting them.
            self.engine.prefs = view.prefs;
            self.engine.apply_prefs();
            if close {
                self.close_settings();
            }
            return None;
        }
        if frame.input.contains(&InputEvent::KeyDown(Key::F(4))) {
            self.settings = Some(prefs::View::new(self.engine.prefs));
            return None;
        }
        if let Some(view) = &mut self.backlog {
            match view.input(frame.input) {
                Action::None => {}
                Action::Close => self.backlog = None,
                Action::Voice(voice) => {
                    let clip = self.engine.assets.voice(&voice);
                    self.engine.play(CHANNEL_VOICE, clip);
                }
            }
            return None;
        }
        let wants_history = frame.input.iter().any(|event| {
            matches!(
                event,
                InputEvent::Wheel(1..) | InputEvent::KeyDown(Key::PageUp)
            )
        });
        if wants_history && self.can_save() && !self.engine.history.is_empty() {
            let font = self.engine.fonts.get(FontFamily::Gothic);
            self.backlog = Some(backlog::View::new(&self.engine.history, font));
            return None;
        }
        let open = frame.input.iter().find_map(|event| match event {
            InputEvent::KeyDown(Key::F(2)) => Some(true),
            InputEvent::KeyDown(Key::F(3)) => Some(false),
            _ => None,
        });
        if let Some(saving) = open {
            self.open_menu(saving, false);
            if self.menu.is_some() {
                return None;
            }
        }
        let e = &mut self.engine;
        let mut advance = false;
        let mut pointer = None;
        let mut click = None;
        for event in frame.input {
            match *event {
                InputEvent::MouseMove { x, y } => pointer = Some((x, y)),
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    pointer = Some((x, y));
                    click = Some((x, y));
                    if e.hidden {
                        e.hidden = false;
                    } else {
                        advance = true;
                    }
                }
                InputEvent::KeyDown(Key::Enter | Key::Space) => advance = true,
                InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => e.hidden = !e.hidden,
                InputEvent::KeyDown(Key::Control) => e.skipping = true,
                InputEvent::KeyUp(Key::Control) => e.skipping = false,
                InputEvent::KeyDown(Key::Escape) => return Some(Flow::Quit),
                InputEvent::KeyDown(Key::Up) => move_hover(&mut e.mode, -1),
                InputEvent::KeyDown(Key::Down) => move_hover(&mut e.mode, 1),
                _ => {}
            }
        }
        let skip = e.skipping;
        let step = e.reveal_step();
        let mut answer: Option<(Option<u16>, i32)> = None;
        match &mut e.mode {
            Mode::Message { text, shown, .. } => {
                let total = text.chars().count();
                if *shown < total && !(advance || skip) {
                    *shown = (*shown + step).min(total);
                } else if *shown < total {
                    *shown = total;
                    if skip {
                        e.mode = Mode::Running;
                    }
                } else if advance || skip {
                    e.mode = Mode::Running;
                }
            }
            Mode::Choice {
                options,
                variable,
                hover,
            } => {
                if let Some((x, y)) = pointer {
                    *hover = (0..options.len()).find(|&i| hit(choice_rect(i), x, y));
                }
                let picked = match click {
                    Some((x, y)) => (0..options.len()).find(|&i| hit(choice_rect(i), x, y)),
                    None if advance => *hover,
                    None => None,
                };
                if let Some(i) = picked {
                    answer = Some((*variable, i as i32 + 1));
                }
            }
            Mode::Title { variable, hover } => {
                if let Some((x, y)) = pointer {
                    *hover = (0..3).find(|&i| hit(title_button_rect(i), x, y));
                }
                let picked = match click {
                    Some((x, y)) => (0..3).find(|&i| hit(title_button_rect(i), x, y)),
                    None if advance => *hover,
                    None => None,
                };
                if let Some(i) = picked {
                    answer = Some((*variable, i as i32 + 1));
                }
            }
            Mode::Wait(until) => {
                if frame.number >= *until || advance || skip {
                    e.mode = Mode::Running;
                }
            }
            Mode::Click => {
                if advance || skip {
                    e.mode = Mode::Running;
                }
            }
            Mode::Transition => {
                if skip {
                    e.transition = None;
                }
                if e.transition.as_ref().is_none_or(|t| t.done(frame.number)) {
                    e.mode = Mode::Running;
                }
            }
            Mode::Channel(channel) => {
                let playing = e.mixer.as_ref().is_some_and(|m| m.is_playing(*channel));
                if !playing || advance || skip {
                    e.mode = Mode::Running;
                }
            }
            Mode::Running | Mode::Paused | Mode::Ended(_) => {}
        }
        if let Some((variable, value)) = answer {
            if let Some(var) = variable {
                self.vm.set_variable(&mut self.engine, var, value);
            }
            self.engine.window_visible = false;
            self.engine.mode = Mode::Running;
        }
        None
    }

    fn draw_text_box(&mut self, canvas: &mut Canvas) {
        let e = &mut self.engine;
        if !e.chapter.is_empty() {
            let style = TextStyle {
                size: 18.0,
                color: Color::WHITE,
                shadow: Some(Color::BLACK),
                bold: false,
            };
            let chapter = clean_text(&e.chapter);
            e.fonts
                .get(FontFamily::Gothic)
                .draw_line(canvas, 12.0, 8.0, &chapter, style);
        }
        if e.hidden || !e.window_visible {
            return;
        }
        let (x, y, w, h) = BOX;
        match &e.mode {
            Mode::Message {
                text,
                speaker,
                style,
                shown,
            } => {
                canvas.fill_rect(
                    x,
                    y,
                    w,
                    h,
                    e.settings
                        .message_window()
                        .with_opacity(e.prefs.window_alpha()),
                );
                let cs = e.settings.char_style(*style);
                let ts = TextStyle {
                    size: TEXT_SIZE,
                    color: cs.color,
                    shadow: cs.shadow.then_some(Color::BLACK),
                    bold: cs.bold,
                };
                let font = e.fonts.get(cs.family);
                let mut top = y as f32 + 12.0;
                let left = x as f32 + 20.0;
                if let Some(speaker) = speaker {
                    font.draw_line(canvas, left, top, speaker, ts);
                    top += LINE_HEIGHT;
                }
                let visible: String = text.chars().take(*shown).collect();
                for line in font
                    .wrap(&visible, TEXT_SIZE, w as f32 - 40.0)
                    .iter()
                    .take(5)
                {
                    font.draw_line(canvas, left, top, line, ts);
                    top += LINE_HEIGHT;
                }
            }
            Mode::Choice { options, hover, .. } => {
                canvas.fill_rect(
                    x,
                    y,
                    w,
                    h,
                    e.settings
                        .message_window()
                        .with_opacity(e.prefs.window_alpha()),
                );
                let cs = e.settings.char_style(0);
                for (i, option) in options.iter().enumerate() {
                    let rect = choice_rect(i);
                    if *hover == Some(i) {
                        canvas.fill_rect(
                            rect.0,
                            rect.1,
                            rect.2,
                            rect.3,
                            Color::rgba(255, 255, 255, 60),
                        );
                    }
                    let ts = TextStyle {
                        size: TEXT_SIZE,
                        color: cs.color,
                        shadow: Some(Color::BLACK),
                        bold: false,
                    };
                    e.fonts.get(cs.family).draw_line(
                        canvas,
                        rect.0 as f32 + 10.0,
                        rect.1 as f32 + 2.0,
                        &format!("{}. {option}", i + 1),
                        ts,
                    );
                }
            }
            _ => {}
        }
    }

    fn draw_title(&mut self, canvas: &mut Canvas) {
        let Mode::Title { hover, .. } = self.engine.mode else {
            return;
        };
        for (i, name) in TITLE_BUTTONS.iter().enumerate() {
            let (x, y, w, h) = title_button_rect(i);
            if let Some(button) = self.engine.assets.picture(name, &["img"]) {
                canvas.draw(&button, x, y, 255);
            }
            if hover == Some(i) {
                canvas.fill_rect(x, y, w, h, Color::rgba(255, 120, 160, 50));
            }
        }
    }

    fn draw_end(&mut self, canvas: &mut Canvas) {
        let Mode::Ended(reason) = &self.engine.mode else {
            return;
        };
        let style = TextStyle {
            size: 18.0,
            color: Color::WHITE,
            shadow: Some(Color::BLACK),
            bold: false,
        };
        let reason = reason.clone();
        let font = self.engine.fonts.get(FontFamily::Gothic);
        let mut top = 250.0;
        for line in font.wrap(&reason, 18.0, 760.0) {
            font.draw_line(canvas, 20.0, top, &line, style);
            top += 24.0;
        }
    }
}

fn move_hover(mode: &mut Mode, delta: i32) {
    let (hover, count) = match mode {
        Mode::Choice { hover, options, .. } => (hover, options.len()),
        Mode::Title { hover, .. } => (hover, 3),
        _ => return,
    };
    if count == 0 {
        return;
    }
    let next = match *hover {
        None => 0,
        Some(i) => (i as i32 + delta).rem_euclid(count as i32) as usize,
    };
    *hover = Some(next);
}

impl App for Game {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn resolution(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn update(&mut self, frame: &mut Frame<'_>) -> Flow {
        self.engine.frame = frame.number;
        if self.engine.mixer.is_none() {
            self.engine.mixer = Some(frame.mixer.clone());
        }
        if let Some(flow) = self.input(frame) {
            return flow;
        }
        if self.engine.mode == Mode::Running {
            match self.vm.run(&mut self.engine, STEP_BUDGET) {
                Status::Running | Status::Yielded => {}
                Status::Halted => self.engine.mode = Mode::Ended("― END ―".into()),
                Status::Error(e) => {
                    eprintln!("script error: {e}");
                    self.engine.mode = Mode::Ended(format!("script error: {e}"));
                }
            }
        }

        match &self.engine.transition {
            Some(t) if !t.done(frame.number) => {
                t.draw(&self.engine.shown_canvas, frame.canvas, frame.number);
            }
            _ => {
                self.engine.transition = None;
                frame.canvas.blend_from(&self.engine.shown_canvas, 255);
            }
        }
        if self.engine.load_requested {
            self.engine.load_requested = false;
            self.open_menu(false, true);
        }
        for popup in &self.engine.popups {
            popup.draw(frame.canvas, frame.number);
        }
        self.draw_title(frame.canvas);
        self.draw_text_box(frame.canvas);
        self.draw_end(frame.canvas);
        if let Some(view) = &mut self.backlog {
            view.draw(frame.canvas, self.engine.fonts.get(FontFamily::Gothic));
        }
        if let Some(view) = &self.settings {
            view.draw(frame.canvas, self.engine.fonts.get(FontFamily::Gothic));
        }
        self.draw_menu(frame.canvas);
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_placeholders() {
        assert_eq!(clean_text("【$N0】"), "【主人公】");
        assert_eq!(clean_text("a\\nb"), "a\nb");
    }

    #[test]
    fn hover_wraps_around() {
        let mut mode = Mode::Title {
            variable: None,
            hover: None,
        };
        move_hover(&mut mode, -1);
        move_hover(&mut mode, -1);
        assert_eq!(
            mode,
            Mode::Title {
                variable: None,
                hover: Some(2)
            }
        );
    }

    /// Plays the prologue clicking through (first option of every
    /// choice) until `done` holds, for at most `limit` frames. Saves the
    /// screen to `$VNR_SHOT_DIR/<name>.png` when that is set.
    fn play_prologue_until(name: &str, limit: u64, done: impl Fn(&Engine) -> bool) -> Option<Game> {
        let dir = std::env::var_os("ADV2_GAME_DIR")?;
        let mut game = match Game::new(Path::new(&dir), "Pure Mail".into(), Some("PRO1"), None) {
            Ok(g) => g,
            Err(e) if e.to_string().contains("no Japanese font") => return None,
            Err(e) => panic!("{e}"),
        };
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        let (x, y, ..) = choice_rect(0);
        let click = [InputEvent::MouseDown {
            button: MouseButton::Left,
            x: x + 5,
            y: y + 5,
        }];
        for number in 0..limit {
            let input: &[InputEvent] = if number % 20 == 19 { &click } else { &[] };
            let mut frame = Frame {
                number,
                input,
                canvas: &mut canvas,
                mixer: &mixer,
            };
            game.update(&mut frame);
            assert!(
                !matches!(game.engine.mode, Mode::Ended(_)),
                "{:?}",
                game.engine.mode
            );
            if done(&game.engine) {
                if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
                    canvas
                        .save_png(&Path::new(&out).join(format!("{name}.png")))
                        .unwrap();
                }
                return Some(game);
            }
        }
        panic!("{name}: not reached in {limit} frames");
    }

    /// Saves in the middle of the first chat, loads the save into a fresh
    /// boot and keeps playing (needs `ADV2_GAME_DIR`).
    #[test]
    fn saves_and_loads() {
        let chatting = |e: &Engine| {
            e.mode == Mode::Click && e.shown.pc.chat.as_ref().is_some_and(|c| c.len() >= 10)
        };
        let Some(mut game) = play_prologue_until("save", 40_000, chatting) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("vnr-adv2-saves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        game.set_save_dir(Some(dir.clone()));
        game.save(0).unwrap();
        let machine = game.vm.snapshot();
        let chat = game.engine.chat_log.clone();

        let root = std::env::var_os("ADV2_GAME_DIR").unwrap();
        let mut fresh = Game::new(Path::new(&root), "Pure Mail".into(), None, None).unwrap();
        fresh.set_save_dir(Some(dir.clone()));
        fresh.load(0).unwrap();
        assert_eq!(fresh.vm.snapshot(), machine);
        assert_eq!(fresh.engine.chat_log, chat);
        assert_eq!(fresh.engine.mode, Mode::Click);
        assert_eq!(
            fresh.engine.shown.background_name,
            game.engine.shown.background_name
        );

        // Both copies play on identically.
        let mixer = Mixer::new(CHANNELS);
        let click = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: 0,
            y: 0,
        };
        let script: Vec<(u64, InputEvent)> = (5..600).step_by(20).map(|f| (f, click)).collect();
        vnr_core::run_headless(&mut game, &mixer, 600, &script);
        vnr_core::run_headless(&mut fresh, &mixer, 600, &script);
        assert_eq!(fresh.vm.snapshot(), game.vm.snapshot());
        assert!(fresh.engine.chat_log.len() > chat.len());

        // From the title: "Load" opens the menu, the first slot loads.
        let mut booted = Game::new(Path::new(&root), "Pure Mail".into(), None, None).unwrap();
        booted.set_save_dir(Some(dir.clone()));
        let (x, y, ..) = title_button_rect(1);
        let load = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: x + 5,
            y: y + 5,
        };
        let script: Vec<(u64, InputEvent)> = (20..600).step_by(20).map(|f| (f, load)).collect();
        let canvas = vnr_core::run_headless(&mut booted, &mixer, 600, &script);
        assert!(booted.menu.as_ref().is_some_and(|m| !m.saving));
        if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
            canvas
                .save_png(&Path::new(&out).join("load-menu.png"))
                .unwrap();
        }
        let (x, y, ..) = menu_row(0);
        let pick = [(
            0,
            InputEvent::MouseDown {
                button: MouseButton::Left,
                x: x + 5,
                y: y + 5,
            },
        )];
        vnr_core::run_headless(&mut booted, &mixer, 1, &pick);
        assert!(booted.menu.is_none());
        assert_eq!(booted.vm.snapshot(), machine);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The wheel opens the history over the first chat and closes it
    /// again (needs `ADV2_GAME_DIR`).
    #[test]
    fn shows_the_history() {
        let chatting = |e: &Engine| {
            e.mode == Mode::Click && e.shown.pc.chat.as_ref().is_some_and(|c| c.len() >= 10)
        };
        let Some(mut game) = play_prologue_until("history-before", 40_000, chatting) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        let history: Vec<Entry> = game.engine.history.entries().cloned().collect();
        assert!(history.iter().any(|e| e.speaker.is_some()));
        assert!(history.iter().any(|e| e.text.starts_with("ＥＶＥ：")));
        let mixer = Mixer::new(CHANNELS);
        let canvas = vnr_core::run_headless(&mut game, &mixer, 2, &[(0, InputEvent::Wheel(1))]);
        assert!(game.backlog.is_some());
        if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
            canvas
                .save_png(&Path::new(&out).join("history.png"))
                .unwrap();
        }
        // Scroll up, then back down past the end.
        let events = [
            (0, InputEvent::Wheel(2)),
            (1, InputEvent::Wheel(-2)),
            (2, InputEvent::Wheel(-1)),
        ];
        vnr_core::run_headless(&mut game, &mixer, 3, &events);
        assert!(game.backlog.is_none());
        assert_eq!(game.engine.mode, Mode::Click);
    }

    /// F4 opens the settings; they are stored when closed and read again
    /// by the next launch (needs `ADV2_GAME_DIR`).
    #[test]
    fn settings_persist() {
        let Some(root) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let boot = || match Game::new(Path::new(&root), "Pure Mail".into(), None, None) {
            Ok(g) => Some(g),
            Err(e) if e.to_string().contains("no Japanese font") => None,
            Err(e) => panic!("{e}"),
        };
        let Some(mut game) = boot() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("vnr-adv2-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        game.set_save_dir(Some(dir.clone()));
        let mixer = Mixer::new(CHANNELS);
        let key = |k| InputEvent::KeyDown(k);
        let events = [
            (10, key(Key::F(4))),
            (11, key(Key::Down)),
            (12, key(Key::Left)),
            (13, key(Key::Left)),
        ];
        let canvas = vnr_core::run_headless(&mut game, &mixer, 14, &events);
        assert!(game.settings.is_some());
        if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
            canvas
                .save_png(&Path::new(&out).join("settings.png"))
                .unwrap();
        }
        vnr_core::run_headless(&mut game, &mixer, 1, &[(0, key(Key::Escape))]);
        assert!(game.settings.is_none());
        let music = Preferences::default().music - 2;
        assert_eq!(game.engine.prefs.music, music);

        let mut again = boot().unwrap();
        again.set_save_dir(Some(dir.clone()));
        assert_eq!(again.engine.prefs.music, music);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The dream script reaches a morning wipe, which blocks the script
    /// and ends on the new picture (needs `ADV2_GAME_DIR`; screenshots of
    /// its middle go to `$VNR_SHOT_DIR`).
    #[test]
    fn plays_a_morning_wipe() {
        let Some(root) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let mut game = match Game::new(Path::new(&root), "Pure Mail".into(), Some("PM_DREAM"), None)
        {
            Ok(g) => g,
            Err(e) if e.to_string().contains("no Japanese font") => return,
            Err(e) => panic!("{e}"),
        };
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        let click = [InputEvent::MouseDown {
            button: MouseButton::Left,
            x: 0,
            y: 0,
        }];
        let shots = std::env::var_os("VNR_SHOT_DIR");
        let mut seen = None;
        for number in 0..30_000 {
            let waiting = matches!(game.engine.mode, Mode::Message { .. } | Mode::Click);
            let input: &[InputEvent] = if waiting && number % 10 == 9 {
                &click
            } else {
                &[]
            };
            let mut frame = Frame {
                number,
                input,
                canvas: &mut canvas,
                mixer: &mixer,
            };
            game.update(&mut frame);
            let morning = game
                .engine
                .transition
                .as_ref()
                .filter(|t| matches!(t.effect(), Effect::Morning { .. }));
            if let Some(t) = morning {
                assert_eq!(game.engine.mode, Mode::Transition);
                let start = t.start_frame();
                let length = t.length();
                seen = Some((start, length));
                let at = number - start;
                if let Some(out) = &shots
                    && [length / 4, length / 2, length * 3 / 4].contains(&at)
                {
                    canvas
                        .save_png(&Path::new(out).join(format!("morning-{at}.png")))
                        .unwrap();
                }
            } else if let Some((start, length)) = seen {
                assert!(number >= start + length, "ended early at {number}");
                // The picture is the new scene (text may be drawn on top
                // at the edges).
                for (x, y) in [(400, 150), (100, 300), (700, 350)] {
                    assert_eq!(canvas.pixel(x, y), game.engine.shown_canvas.pixel(x, y));
                }
                return;
            }
        }
        panic!("no morning wipe reached");
    }

    /// Every `sys 33` pop-up decodes at its size (needs `ADV2_GAME_DIR`).
    #[test]
    fn popups_decode() {
        let Some(dir) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let mut assets = Assets::open(Path::new(&dir)).unwrap();
        for (name, w, h, ..) in pc::POPUPS {
            let frames = assets.animation(name, w, h).expect(name);
            assert!(
                frames.iter().all(|f| (f.width, f.height) == (w, h)),
                "{name}"
            );
        }
    }

    /// The prologue opens the chat, then a mail from EVE (needs
    /// `ADV2_GAME_DIR`).
    #[test]
    fn prologue_uses_the_pc() {
        let chatting = |e: &Engine| {
            e.mode == Mode::Click
                && e.transition.is_none()
                && e.shown.pc.chat.as_ref().is_some_and(|c| c.len() >= 10)
        };
        let Some(game) = play_prologue_until("chat", 40_000, chatting) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        assert!(
            game.engine
                .chat_log
                .iter()
                .any(|l| l.text.contains("ＥＶＥ"))
        );

        let reading = |e: &Engine| {
            e.mode == Mode::Click && e.shown.pc.mail.is_some() && e.transition.is_none()
        };
        let game = play_prologue_until("mail", 80_000, reading).unwrap();
        let mail = game.engine.shown.pc.mail.as_ref().unwrap();
        assert_eq!(mail.lines[0].text, "ごめんなさい");
        assert!(mail.lines.len() > 5);
    }

    /// Boots Pure Mail headless, picks "new game" and reads a few
    /// messages (needs `ADV2_GAME_DIR`).
    #[test]
    fn plays_the_opening() {
        let Some(dir) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let mut game = match Game::new(Path::new(&dir), "Pure Mail".into(), None, None) {
            Ok(g) => g,
            Err(e) if e.to_string().contains("no Japanese font") => {
                eprintln!("{e}, skipping");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let mixer = Mixer::new(CHANNELS);
        // Clicking on "Start" also skips the notices shown before the
        // title; afterwards, click elsewhere to read messages.
        let (x, y, ..) = title_button_rect(0);
        let start = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: x + 5,
            y: y + 5,
        };
        let click = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: 0,
            y: 0,
        };
        let mut script: Vec<(u64, InputEvent)> =
            (20..1200).step_by(20).map(|f| (f, start)).collect();
        script.extend((1210..2600).step_by(20).map(|f| (f, click)));
        let canvas = vnr_core::run_headless(&mut game, &mixer, 2600, &script);
        assert!(
            !matches!(game.engine.mode, Mode::Ended(_)),
            "{:?}",
            game.engine.mode
        );
        assert_eq!(game.engine.chapter, "プロローグ");
        let lit = canvas.rgba().chunks_exact(4).filter(|p| p[0] > 128).count();
        assert!(lit > 500, "only {lit} bright pixels");
    }
}
