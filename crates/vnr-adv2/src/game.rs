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
//! | `d5` | title menu; 1-based choice into the variable. With VNRevival's menu, a new game answers 1 at once and otherwise the game returns to that menu | `start.obj` |
//! | `e1` | load screen (title); closing it without loading resumes the script | `start.obj` |
//! | `04` | history entry only (the text is in pictures or windows) | handler `0x422d80` |
//! | `20` | wait for a click | data (always before `83`) |
//! | `63`, `83` | stop the sound effect, the voice | `0x426e00`, `0x424590` |
//! | `65`, `85` | wait until the sound effect, the voice ends | `0x424000`, `0x4245c0` |
//! | `a0`–`c3`, `33`, `34`, `50` | the in-game PC | see [`crate::pc`] |
//! | `11` | clear the game variables (ids below `0x4000`) | `0x401040` |
//! | `13`, `14` | restore points of the original's save (nothing to do) | `0x425730`, `0x425de0` |
//! | `17` | new game: take the hero's names from the game options (the original asks in a dialog) | `0x421d20`; see [`crate::names`] |
//! | `18`, `d8` | clear the history and the chat log | `0x421d00`, `0x4220f0` |
//! | `d7` | unlock a scene of the replay list | `0x421f50`; see [`crate::system`] |
//! | `35` | play a movie (`MOVIE/<name>L_I5.AVI`); a click ends it | strings of `PMAIL.EXE`, `OF_ADV2.INI`; see [`crate::movie`] |
//!
//! Everything else is ignored for now and reported once on stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vnr_core::settings::StartScreen;
use vnr_core::text::find_system_font;
use vnr_core::{
    BackendError, Canvas, Clip, Color, Font, FontFamily, Frame, GalleryEntry, HistoryEntry,
    InputEvent, Key, Mixer, MouseButton, ReplayScene, SaveData, Session, SessionEvent,
    Settings as PlayerSettings, Sprite, Start, TextStyle, Track,
};

use crate::assets::Assets;
use crate::backlog::{Entry, History};
use crate::ini::Settings;
use crate::movie::Movie;
use crate::names::Names;
use crate::options::GameOptions;
use crate::pc::{self, Label, Line, Mail, PcView, Popup};
use crate::save::{self, EngineState, SaveFile, SavedMode, SceneState};
use crate::system::{self, HeardTracks, SystemData};
use crate::transition::{self, Effect, MorningPictures, Transition};
use crate::vm::{Control, Host, Machine, Status, Value};

/// Screen size of the engine.
pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

/// Mixer channels.
pub const CHANNEL_MUSIC: usize = 0;
pub const CHANNEL_SOUND: usize = 1;
pub const CHANNEL_VOICE: usize = 2;
pub const CHANNEL_MOVIE: usize = 3;
pub const CHANNELS: usize = 4;

/// Fade speed when a picture change is shown without `sys 40`
/// (240 ms).
const IMPLICIT_FADE_STEP: u32 = 16;
/// Variables of a playthrough: ids below this are cleared by `sys 11`
/// (`0x401040`); higher ones keep system data.
const GAME_VARIABLES_END: u16 = 0x4000;
/// Script with the shared functions, loaded first.
const LIBRARY: &str = "pmail";
const STEP_BUDGET: usize = 200_000;
const TEXT_SIZE: f32 = 22.0;
const LINE_HEIGHT: f32 = 30.0;
const BOX: (i32, i32, u32, u32) = (20, 424, 760, 166);
/// Background picture formats: `.crgb`, and one `.dib` (`hiro17`).
const BACKGROUND: &[&str] = &["crgb", "dib"];

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
    /// A movie is playing (a click ends it).
    Movie,
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
    /// What the interface should do after this frame.
    event: Option<SessionEvent>,
    /// Running the start-up at full speed, unseen, until the title,
    /// which is answered with "new game".
    fast_start: bool,
    /// Message history.
    history: History,
    /// The hero's names (set at a new game by `sys 17`).
    names: Names,
    /// Flags kept across playthroughs.
    system: SystemData,
    /// The player's settings (from the interface).
    player: PlayerSettings,
    /// This game's own options.
    options: GameOptions,
    /// Music heard, for the music room.
    heard: HeardTracks,
    /// Replaying a scene of the extras: going back to the extras script
    /// ends it.
    replay: bool,
    /// Fraction of a character of text still to reveal.
    reveal: f32,
    /// Install folder.
    root: PathBuf,
    /// The movie playing.
    movie: Option<Movie>,
}

impl Engine {
    fn play(&self, channel: usize, clip: Option<Arc<vnr_core::Clip>>) {
        if let (Some(mixer), Some(clip)) = (&self.mixer, clip) {
            mixer.play(channel, clip, self.channel_volume(channel));
        }
    }

    /// Mixer volume of a channel from the settings.
    fn channel_volume(&self, channel: usize) -> u16 {
        PlayerSettings::volume(match channel {
            CHANNEL_MUSIC | CHANNEL_MOVIE => self.player.music,
            CHANNEL_SOUND => self.player.sound,
            _ => self.player.voice,
        })
    }

    /// Applies changed settings to what is playing.
    fn apply_volumes(&self) {
        if let Some(mixer) = &self.mixer {
            for channel in [CHANNEL_MUSIC, CHANNEL_SOUND, CHANNEL_VOICE, CHANNEL_MOVIE] {
                mixer.set_volume(channel, self.channel_volume(channel));
            }
        }
    }

    /// Characters to reveal this frame (everything when text is instant).
    fn reveal_step(&mut self) -> usize {
        match self.player.chars_per_second() {
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
                if let Err(e) = self.heard.hear(track) {
                    eprintln!("could not save the music heard: {e}");
                }
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

    fn stop_all(&self) {
        for channel in [CHANNEL_MUSIC, CHANNEL_SOUND, CHANNEL_VOICE, CHANNEL_MOVIE] {
            self.stop(channel);
        }
    }

    /// Back to the state of a fresh start.
    fn reset(&mut self) {
        self.stop_all();
        self.mode = Mode::Running;
        self.staged = Scene::default();
        self.shown = Scene::default();
        self.shown_canvas.clear(Color::BLACK);
        self.transition = None;
        self.window_visible = false;
        self.speaker = (0, String::new());
        self.chapter.clear();
        self.engine_vars.clear();
        self.skipping = false;
        self.hidden = false;
        self.chat_log.clear();
        self.mail_lines.clear();
        self.letter.clear();
        self.popups.clear();
        self.music = None;
        self.event = None;
        self.fast_start = false;
        self.replay = false;
        self.history = History::default();
        self.names = self.options.names.clone();
        self.reveal = 0.0;
        self.movie = None;
    }

    /// `sys 35`: plays a movie and waits for it.
    fn play_movie(&mut self, name: &str) -> Control {
        if self.skipping {
            return Control::Continue;
        }
        let small = self.settings.int("MovieType") == Some(1);
        match Movie::open(&self.root, name, small) {
            Ok(mut movie) => {
                self.stop(CHANNEL_MUSIC);
                let volume = self.channel_volume(CHANNEL_MOVIE);
                movie.start(self.mixer.as_ref(), CHANNEL_MOVIE, volume, self.frame);
                self.movie = Some(movie);
                self.mode = Mode::Movie;
                Control::Yield
            }
            Err(e) => {
                eprintln!("{e}");
                Control::Continue
            }
        }
    }

    /// Ends the movie (finished or skipped).
    fn end_movie(&mut self) {
        self.movie = None;
        self.stop(CHANNEL_MOVIE);
        if self.mode == Mode::Movie {
            self.mode = Mode::Running;
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
        let voice = args.iter().find_map(|&v| voice_after(vm, &self.names, v));
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
            text: self.names.expand(&text),
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
            // The voice files are the two pool entries after the text:
            // plain, and one saying the hero's default name.
            let entry =
                |offset: u16| vm.pool_text(value.script(), value.low().wrapping_add(offset));
            voice = self.names.pick_voice(entry(1), entry(2)).or(voice);
        }
        if let Some(voice) = &voice {
            let clip = self.assets.voice(voice);
            self.play(CHANNEL_VOICE, clip);
        }
        let text = self.names.expand(&text);
        if text.trim().is_empty() {
            return Control::Continue;
        }
        self.present_if_changed();
        self.window_visible = true;
        let speaker = self.names.expand(&self.speaker.1);
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
fn voice_after(vm: &Machine, names: &Names, value: Value) -> Option<String> {
    vm.text(value)?;
    let entry = |offset: u16| {
        vm.pool_text(value.script(), value.low().wrapping_add(offset))
            .filter(|v| {
                v.len() >= 5
                    && v.starts_with(['V', 'v'])
                    && v.bytes().all(|b| b.is_ascii_alphanumeric())
            })
    };
    let plain = entry(1)?;
    names.pick_voice(Some(plain), entry(2))
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
                let options: Vec<String> =
                    vm.take_log().iter().map(|t| self.names.expand(t)).collect();
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
                let stem = name.to_ascii_lowercase();
                if self.replay && (stem.starts_with("extra.") || stem.starts_with("start.")) {
                    self.stop_all();
                    self.event = Some(SessionEvent::ToMenu);
                    self.mode = Mode::Paused;
                    return Control::Yield;
                }
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
                    self.mode = Mode::Wait(self.frame + (ms * 60).div_ceil(1000));
                    return Control::Yield;
                }
            }
            0x30 => {
                let name = text(0);
                self.staged.background = self.assets.picture(&name, BACKGROUND);
                if self.system.see_picture(&name)
                    && let Err(e) = self.system.store()
                {
                    eprintln!("could not save the system data: {e}");
                }
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
                    4 => self
                        .assets
                        .gray("OVERFLOW.ALPM")
                        .and_then(|(w, h, rule)| Effect::rule(mode, rule, w, h))
                        .unwrap_or(Effect::Fade),
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
                let voice = self.names.pick_voice(Some(text(2)), Some(text(3)));
                let line = self.names.expand(&text(1));
                self.history.push(Entry {
                    speaker: None,
                    text: line,
                    voice,
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
                    text: self.names.expand(&text(0)),
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
            0x35 => return self.play_movie(&text(0)),
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
                        && let Some(voice) =
                            args.iter().find_map(|&v| voice_after(vm, &self.names, v))
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
            0xC2 => self.letter.push(self.names.expand(&text(1))),
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
                self.event = Some(SessionEvent::OpenLoad);
                self.mode = Mode::Paused;
                return Control::Yield;
            }
            0x80 => {
                if let Some(voice) = self.names.pick_voice(Some(text(1)), Some(text(2))) {
                    let clip = self.assets.voice(&voice);
                    self.play(CHANNEL_VOICE, clip);
                }
            }
            0x17 => self.names = self.options.names.clone(),
            0x11 => vm.clear_variables(GAME_VARIABLES_END),
            0xD7 => {
                if self.system.unlock_scene(&text(0))
                    && let Err(e) = self.system.store()
                {
                    eprintln!("could not save the system data: {e}");
                }
            }
            // Clear the history and the PC logs (title, extras).
            0x18 | 0xD8 => {
                self.history = History::default();
                self.chat_log.clear();
                self.mail_lines.clear();
                self.letter.clear();
                self.stage_pc(|pc| pc.chat = None);
            }
            // Restore points for the original's save; ours hold the whole
            // state.
            0x13 | 0x14 => {}
            0xD5 => {
                let variable = args.iter().find_map(|&v| vm.variable_ref(v));
                if self.fast_start {
                    // "New game", on a clean screen.
                    self.fast_start = false;
                    self.skipping = false;
                    self.stop_all();
                    self.staged = Scene::default();
                    self.present(Effect::Cut, 1);
                    self.window_visible = false;
                    self.chapter.clear();
                    if let Some(var) = variable {
                        vm.set_variable(self, var, 1);
                    }
                    return Control::Continue;
                }
                if self.player.start == StartScreen::Universal {
                    self.stop_all();
                    self.event = Some(SessionEvent::ToMenu);
                    self.mode = Mode::Paused;
                    return Control::Yield;
                }
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
    /// Install folder (for the original's own files).
    root: PathBuf,
    /// Script to start instead of `start.obj` (for testing).
    entry: Option<String>,
    /// Music tracks in the sound pack, once listed.
    tracks: Option<Vec<i32>>,
    vm: Machine,
    engine: Engine,
    /// Folder of the game's own data (options, system flags); `None`
    /// keeps them in memory.
    data_dir: Option<PathBuf>,
}

impl Game {
    /// Opens the game; [`Session::start`] boots it. With `entry`, every
    /// start runs that script instead of `start.obj`.
    pub fn new(
        root: &Path,
        title: String,
        entry: Option<&str>,
        font: Option<PathBuf>,
    ) -> Result<Self, BackendError> {
        let assets = Assets::open(root)?;
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
            script_names: BTreeMap::new(),
            music: None,
            event: None,
            fast_start: false,
            history: History::default(),
            names: Names::default(),
            system: SystemData::default(),
            player: PlayerSettings::default(),
            options: GameOptions::default(),
            heard: HeardTracks::default(),
            replay: false,
            reveal: 0.0,
            root: root.to_path_buf(),
            movie: None,
        };
        Ok(Self {
            title,
            root: root.to_path_buf(),
            entry: entry.map(str::to_string),
            tracks: None,
            vm: Machine::new(),
            engine,
            data_dir: None,
        })
    }

    /// Sets the folder of the game's own data and reads it: the options
    /// and the flags kept across playthroughs (imported from the
    /// original's `SAVE/SAVEFL99.SAV` the first time).
    pub fn set_data_dir(&mut self, dir: Option<PathBuf>) {
        self.data_dir = dir;
        let Some(dir) = &self.data_dir else {
            return;
        };
        let original = vnr_core::resolve_ci(&self.root, "SAVE/SAVEFL99.SAV");
        match SystemData::open(dir.join("system.sav"), original.as_deref()) {
            Ok(data) => self.engine.system = data,
            Err(e) => eprintln!("could not read the system data: {e}"),
        }
        self.engine.options = GameOptions::load(dir);
        self.engine.heard = HeardTracks::open(dir.join("music.json"));
    }

    /// Loads the scripts and starts `script` from a clean state.
    fn boot(&mut self, script: &str) -> Result<(), String> {
        let e = &mut self.engine;
        let mut vm = Machine::new();
        let library = e.assets.script(LIBRARY).map_err(|e| e.to_string())?;
        let mut names = BTreeMap::from([(library.id, LIBRARY.to_string())]);
        vm.load_library(library);
        let first = e.assets.script(script).map_err(|e| e.to_string())?;
        names.insert(first.id, script.to_string());
        vm.start(first);
        self.vm = vm;
        e.script_names = names;
        e.reset();
        Ok(())
    }

    /// The current state as a save, with the chapter and text for the
    /// slot list, if the game is waiting for the player.
    fn save_state(&self) -> Result<(SaveFile, String, String), String> {
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
        let save = SaveFile {
            version: save::VERSION,
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
                names: e.names.clone(),
                mode,
            },
        };
        Ok((save, e.names.expand(&e.chapter), excerpt))
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
            scene.background = e.assets.picture(&name, BACKGROUND);
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
        e.names = state.names;
        e.skipping = false;
        e.hidden = false;
        e.event = None;
        e.fast_start = false;
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

    fn input(&mut self, frame: &Frame<'_>) {
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
                    button: MouseButton::Middle,
                    ..
                } => e.hidden = !e.hidden,
                InputEvent::KeyDown(Key::Control) if !e.fast_start => e.skipping = true,
                InputEvent::KeyUp(Key::Control) if !e.fast_start => e.skipping = false,
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
                match picked {
                    // The extras belong to the interface.
                    Some(2) => e.event = Some(SessionEvent::OpenExtras),
                    Some(i) => answer = Some((*variable, i as i32 + 1)),
                    None => {}
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
            Mode::Movie => {
                let playing = match e.movie.as_mut() {
                    Some(movie) => movie.advance(e.mixer.as_ref(), CHANNEL_MOVIE, frame.number),
                    None => false,
                };
                if !playing || advance || skip {
                    e.end_movie();
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
            let chapter = e.names.expand(&e.chapter);
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
                        .with_opacity(e.player.window_alpha()),
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
                        .with_opacity(e.player.window_alpha()),
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

impl Session for Game {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn resolution(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn start(&mut self, start: Start) -> Result<(), String> {
        if let Some(entry) = self.entry.clone() {
            return self.boot(&entry);
        }
        match start {
            Start::NewGame => {
                self.boot("start")?;
                self.engine.fast_start = true;
                self.engine.skipping = true;
                Ok(())
            }
            Start::OriginalTitle => self.boot("start"),
            Start::Scene(id) => {
                let known = system::SCENES.iter().any(|s| s.eq_ignore_ascii_case(&id));
                if !known {
                    return Err(format!("{id} is not a scene of the replay list"));
                }
                self.boot(&id)?;
                self.engine.replay = true;
                Ok(())
            }
        }
    }

    fn update(&mut self, frame: &mut Frame<'_>) -> SessionEvent {
        self.engine.frame = frame.number;
        if self.engine.mixer.is_none() {
            self.engine.mixer = Some(frame.mixer.clone());
            self.engine.apply_volumes();
            // A save loaded before the first frame had no mixer yet.
            if let Some(track) = self.engine.music {
                self.engine.play_music(Some(track));
            }
        }
        self.input(frame);
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
        let event = self.engine.event.take().unwrap_or(SessionEvent::Continue);
        if self.engine.fast_start {
            frame.canvas.clear(Color::BLACK);
            return event;
        }
        if let Some(movie) = &self.engine.movie {
            movie.draw(frame.canvas);
            return event;
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
        for popup in &self.engine.popups {
            popup.draw(frame.canvas, frame.number);
        }
        self.draw_title(frame.canvas);
        self.draw_text_box(frame.canvas);
        self.draw_end(frame.canvas);
        event
    }

    fn can_save(&self) -> bool {
        matches!(
            self.engine.mode,
            Mode::Message { .. } | Mode::Choice { .. } | Mode::Click
        )
    }

    fn save(&self) -> Result<SaveData, String> {
        let (save, chapter, excerpt) = self.save_state()?;
        Ok(SaveData {
            payload: serde_json::to_value(save).map_err(|e| e.to_string())?,
            chapter,
            excerpt,
        })
    }

    fn load(&mut self, data: &SaveData) -> Result<(), String> {
        let save: SaveFile = serde_json::from_value(data.payload.clone())
            .map_err(|e| format!("this save cannot be read: {e}"))?;
        if save.version != save::VERSION {
            return Err(format!(
                "save format {} is not supported (expected {})",
                save.version,
                save::VERSION
            ));
        }
        self.restore(save)
    }

    fn dismissed(&mut self) {
        if self.engine.mode == Mode::Paused {
            self.engine.mode = Mode::Running;
        }
    }

    fn history(&self) -> Vec<HistoryEntry> {
        self.engine
            .history
            .entries()
            .map(|e| HistoryEntry {
                speaker: e.speaker.clone(),
                text: e.text.clone(),
                voice: e.voice.clone(),
            })
            .collect()
    }

    fn play_voice(&mut self, voice: &str) {
        let clip = self.engine.assets.voice(voice);
        self.engine.play(CHANNEL_VOICE, clip);
    }

    fn apply(&mut self, settings: &PlayerSettings) {
        self.engine.player = settings.clone();
        self.engine.apply_volumes();
    }

    fn silence(&mut self) {
        self.engine.stop_all();
    }

    fn freeze(&mut self, frozen: bool) {
        if let Some(mixer) = &self.engine.mixer {
            mixer.set_paused(CHANNEL_MOVIE, frozen);
        }
    }

    fn gallery(&mut self) -> Vec<GalleryEntry> {
        let system = &self.engine.system;
        system::GALLERY
            .iter()
            .enumerate()
            .map(|(i, variants)| GalleryEntry {
                variants: (0..variants.len())
                    .map(|k| {
                        system.variant_seen(i, k + 1)
                            || (k == 0 && system.picture_seen(i) && variants.len() == 1)
                    })
                    .collect(),
            })
            .collect()
    }

    fn gallery_thumbnail(&mut self, index: usize) -> Option<Arc<Sprite>> {
        let variants = system::GALLERY.get(index)?;
        let assets = &mut self.engine.assets;
        variants
            .iter()
            .find_map(|name| assets.picture(name, &["cgm"]))
            .or_else(|| assets.picture(variants.first()?, BACKGROUND))
    }

    fn gallery_picture(&mut self, index: usize, variant: usize) -> Option<Arc<Sprite>> {
        let name = system::GALLERY.get(index)?.get(variant)?;
        self.engine.assets.picture(name, BACKGROUND)
    }

    fn tracks(&mut self) -> Vec<Track> {
        let tracks = self
            .tracks
            .get_or_insert_with(|| self.engine.assets.music_tracks());
        tracks
            .iter()
            .enumerate()
            .map(|(i, &n)| Track {
                title: format!("Track {:02}", i + 1),
                heard: self.engine.heard.heard(n),
            })
            .collect()
    }

    fn track(&mut self, index: usize) -> Option<Arc<Clip>> {
        let n = *self.tracks.as_ref()?.get(index)?;
        self.engine.assets.sound(&format!("mm{n:03}"))
    }

    fn scenes(&mut self) -> Vec<ReplayScene> {
        let mut counts = std::collections::HashMap::new();
        system::SCENES
            .iter()
            .zip(system::SCENE_INFO)
            .enumerate()
            .map(|(i, (script, (group, _)))| {
                let n = counts.entry(group).or_insert(0);
                *n += 1;
                ReplayScene {
                    id: script.to_string(),
                    title: format!("{n:02}"),
                    group: system::group_name(group).to_string(),
                    unlocked: self.engine.system.scene_unlocked(i),
                }
            })
            .collect()
    }

    fn scene_thumbnail(&mut self, index: usize) -> Option<Arc<Sprite>> {
        let (_, name) = system::SCENE_INFO.get(index)?;
        let assets = &mut self.engine.assets;
        assets
            .picture(name, &["cgm"])
            .or_else(|| assets.picture(name, BACKGROUND))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Opens the game in `ADV2_GAME_DIR`; `None` without it or without
    /// a Japanese font.
    fn open(entry: Option<&str>) -> Option<Game> {
        let dir = std::env::var_os("ADV2_GAME_DIR")?;
        match Game::new(Path::new(&dir), "Pure Mail".into(), entry, None) {
            Ok(game) => Some(game),
            Err(e) if e.to_string().contains("no Japanese font") => None,
            Err(e) => panic!("{e}"),
        }
    }

    /// Settings that show the game's own title.
    fn original_title() -> PlayerSettings {
        PlayerSettings {
            start: StartScreen::Original,
            ..PlayerSettings::default()
        }
    }

    fn click_at(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        }
    }

    /// One frame.
    fn step(
        game: &mut Game,
        mixer: &Mixer,
        canvas: &mut Canvas,
        number: u64,
        input: &[InputEvent],
    ) -> SessionEvent {
        let mut frame = Frame {
            number,
            input,
            canvas,
            mixer,
        };
        game.update(&mut frame)
    }

    fn shoot(canvas: &Canvas, name: &str) {
        if let Some(out) = std::env::var_os("VNR_SHOT_DIR") {
            canvas
                .save_png(&Path::new(&out).join(format!("{name}.png")))
                .unwrap();
        }
    }

    /// Plays the prologue clicking through (first option of every
    /// choice) until `done` holds, for at most `limit` frames. Saves the
    /// screen to `$VNR_SHOT_DIR/<name>.png` when that is set.
    fn play_prologue_until(name: &str, limit: u64, done: impl Fn(&Engine) -> bool) -> Option<Game> {
        let mut game = open(Some("PRO1"))?;
        game.start(Start::NewGame).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        let (x, y, ..) = choice_rect(0);
        let click = [click_at(x + 5, y + 5)];
        for number in 0..limit {
            let input: &[InputEvent] = if number % 20 == 19 { &click } else { &[] };
            step(&mut game, &mixer, &mut canvas, number, input);
            assert!(
                !matches!(game.engine.mode, Mode::Ended(_)),
                "{:?}",
                game.engine.mode
            );
            if done(&game.engine) {
                shoot(&canvas, name);
                return Some(game);
            }
        }
        panic!("{name}: not reached in {limit} frames");
    }

    fn chatting(e: &Engine) -> bool {
        e.mode == Mode::Click && e.shown.pc.chat.as_ref().is_some_and(|c| c.len() >= 10)
    }

    /// Saves in the middle of the first chat, loads the save into a fresh
    /// copy and keeps playing; the title's "Load" asks the interface for
    /// the load screen (needs `ADV2_GAME_DIR`).
    #[test]
    fn saves_and_loads() {
        let Some(mut game) = play_prologue_until("save", 40_000, chatting) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        assert!(game.can_save());
        let data = game.save().unwrap();
        assert!(!data.excerpt.is_empty());
        let machine = game.vm.snapshot();
        let chat = game.engine.chat_log.clone();

        let mut fresh = open(None).unwrap();
        fresh.load(&data).unwrap();
        assert_eq!(fresh.vm.snapshot(), machine);
        assert_eq!(fresh.engine.chat_log, chat);
        assert_eq!(fresh.engine.mode, Mode::Click);
        assert_eq!(
            fresh.engine.shown.background_name,
            game.engine.shown.background_name
        );

        // Both copies play on identically.
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        for number in 0..600 {
            let input: &[InputEvent] = if number % 20 == 5 {
                &[InputEvent::KeyDown(Key::Enter)]
            } else {
                &[]
            };
            step(&mut game, &mixer, &mut canvas, number, input);
            step(&mut fresh, &mixer, &mut canvas, number, input);
        }
        assert_eq!(fresh.vm.snapshot(), game.vm.snapshot());
        assert!(fresh.engine.chat_log.len() > chat.len());

        // From the original title: "Load" asks for the load screen.
        let mut booted = open(None).unwrap();
        booted.apply(&original_title());
        booted.start(Start::OriginalTitle).unwrap();
        let (x, y, ..) = title_button_rect(1);
        let asked = (0..1500).any(|number| {
            let input: &[InputEvent] = if number % 20 == 19 {
                &[click_at(x + 5, y + 5)]
            } else {
                &[]
            };
            step(&mut booted, &mixer, &mut canvas, number, input) == SessionEvent::OpenLoad
        });
        assert!(asked, "the title never asked for the load screen");
        booted.load(&data).unwrap();
        assert_eq!(booted.vm.snapshot(), machine);
    }

    /// The history has the chat lines and named messages (needs
    /// `ADV2_GAME_DIR`).
    #[test]
    fn keeps_a_history() {
        let Some(game) = play_prologue_until("history-before", 40_000, chatting) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        let history = game.history();
        assert!(history.iter().any(|e| e.speaker.is_some()));
        assert!(history.iter().any(|e| e.text.starts_with("ＥＶＥ：")));
    }

    /// "New game" from VNRevival's menu runs the start-up unseen and
    /// answers the title at once (needs `ADV2_GAME_DIR`).
    #[test]
    fn new_game_skips_the_title() {
        let Some(mut game) = open(None) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        game.apply(&PlayerSettings::default());
        game.start(Start::NewGame).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        for number in 0..4000 {
            let input: &[InputEvent] = if number % 20 == 19 && !game.engine.fast_start {
                &[click_at(0, 0)]
            } else {
                &[]
            };
            let event = step(&mut game, &mixer, &mut canvas, number, input);
            assert_eq!(event, SessionEvent::Continue);
            assert!(!matches!(
                game.engine.mode,
                Mode::Title { .. } | Mode::Ended(_)
            ));
            if game.engine.fast_start {
                assert_eq!(canvas.pixel(400, 300), Color::BLACK);
            }
            if game.engine.chapter == "プロローグ"
                && matches!(game.engine.mode, Mode::Message { .. })
            {
                shoot(&canvas, "new-game");
                return;
            }
        }
        panic!("the prologue never started");
    }

    /// The dream script reaches a morning wipe, which blocks the script
    /// and ends on the new picture (needs `ADV2_GAME_DIR`; screenshots of
    /// its middle go to `$VNR_SHOT_DIR`).
    #[test]
    fn plays_a_morning_wipe() {
        let Some(mut game) = open(Some("PM_DREAM")) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        game.start(Start::NewGame).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        let click = [click_at(0, 0)];
        let mut seen = None;
        for number in 0..30_000 {
            let waiting = matches!(game.engine.mode, Mode::Message { .. } | Mode::Click);
            let input: &[InputEvent] = if waiting && number % 10 == 9 {
                &click
            } else {
                &[]
            };
            step(&mut game, &mixer, &mut canvas, number, input);
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
                if [length / 4, length / 2, length * 3 / 4].contains(&at) {
                    shoot(&canvas, &format!("morning-{at}"));
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
        let settled = |e: &Engine| chatting(e) && e.transition.is_none();
        let Some(game) = play_prologue_until("chat", 40_000, settled) else {
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

    /// Boots Pure Mail to its own title, picks "new game" and reads a few
    /// messages (needs `ADV2_GAME_DIR`).
    #[test]
    fn plays_the_opening() {
        let Some(mut game) = open(None) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        game.apply(&original_title());
        game.start(Start::OriginalTitle).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        // Clicking on "Start" also skips the notices shown before the
        // title; afterwards, click elsewhere to read messages.
        let (x, y, ..) = title_button_rect(0);
        let start = [click_at(x + 5, y + 5)];
        let click = [click_at(0, 0)];
        for number in 0..2600 {
            let input: &[InputEvent] = match number {
                _ if number % 20 != 0 => &[],
                20..1200 => &start,
                1210.. => &click,
                _ => &[],
            };
            step(&mut game, &mixer, &mut canvas, number, input);
        }
        assert!(
            !matches!(game.engine.mode, Mode::Ended(_)),
            "{:?}",
            game.engine.mode
        );
        assert_eq!(game.engine.chapter, "プロローグ");
        let lit = canvas.rgba().chunks_exact(4).filter(|p| p[0] > 128).count();
        assert!(lit > 500, "only {lit} bright pixels");
    }

    /// The extras: every scene and gallery entry has a thumbnail, the
    /// music decodes, and a replayed scene ends by asking for the menu
    /// (needs `ADV2_GAME_DIR`).
    #[test]
    fn extras_and_scene_replay() {
        let Some(mut game) = open(None) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        assert_eq!(game.gallery().len(), system::GALLERY.len());
        for i in 0..system::GALLERY.len() {
            assert!(game.gallery_thumbnail(i).is_some(), "gallery {i}");
            assert!(game.gallery_picture(i, 0).is_some(), "gallery {i}");
        }
        let scenes = game.scenes();
        assert_eq!(scenes.len(), 32);
        for (i, scene) in scenes.iter().enumerate() {
            let thumb = game.scene_thumbnail(i).expect(&scene.id);
            assert_eq!((thumb.width, thumb.height), (160, 120));
        }
        let tracks = game.tracks();
        assert!(tracks.len() > 10, "{} tracks", tracks.len());
        assert!(game.track(0).is_some_and(|c| c.is_looping()));

        game.start(Start::Scene("hiro00".into())).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        let skip = [InputEvent::KeyDown(Key::Control)];
        let (x, y, ..) = choice_rect(0);
        let pick = [click_at(x + 5, y + 5)];
        for number in 0..20_000 {
            let input: &[InputEvent] = match number {
                0 => &skip,
                _ if number % 10 == 0 => &pick,
                _ => &[],
            };
            let event = step(&mut game, &mixer, &mut canvas, number, input);
            assert!(
                !matches!(game.engine.mode, Mode::Ended(_)),
                "{:?}",
                game.engine.mode
            );
            if event == SessionEvent::ToMenu {
                return;
            }
        }
        panic!("the scene never ended");
    }

    #[test]
    fn plays_the_opening_movie() {
        let Some(mut game) = open(None) else {
            eprintln!("ADV2_GAME_DIR not set or no font, skipping");
            return;
        };
        // The first scene of the replay list is the opening movie.
        game.start(Start::Scene("etc00".into())).unwrap();
        let mixer = Mixer::new(CHANNELS);
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        // One frame of sound per frame, as the audio device would take.
        let mut sound = vec![0i16; 2 * 44_100 / vnr_core::FPS as usize];
        let mut number = 0;
        let mut run = |game: &mut Game, canvas: &mut Canvas, frames: u64, input: &[InputEvent]| {
            let mut event = SessionEvent::Continue;
            for _ in 0..frames {
                event = step(game, &mixer, canvas, number, input);
                mixer.mix(&mut sound);
                number += 1;
                if event != SessionEvent::Continue {
                    break;
                }
            }
            event
        };
        run(&mut game, &mut canvas, 1, &[]);
        assert_eq!(game.engine.mode, Mode::Movie);
        assert!(mixer.is_playing(CHANNEL_MOVIE));

        // 40 seconds in: a picture over the whole screen.
        run(&mut game, &mut canvas, 40 * 60, &[]);
        assert_eq!(game.engine.mode, Mode::Movie);
        shoot(&canvas, "movie-40s");
        let lit = (0..HEIGHT)
            .step_by(20)
            .flat_map(|y| (0..WIDTH).step_by(20).map(move |x| (x, y)))
            .filter(|&(x, y)| canvas.pixel(x, y).r > 40)
            .count();
        assert!(lit > 100, "{lit} lit samples");

        // Behind a menu the sound holds, and so does the picture.
        let position = mixer.position(CHANNEL_MOVIE);
        game.freeze(true);
        mixer.mix(&mut [0i16; 1470]);
        assert_eq!(mixer.position(CHANNEL_MOVIE), position);
        game.freeze(false);

        // A click ends it, and the replay goes back to the extras.
        let event = run(&mut game, &mut canvas, 600, &[click_at(400, 300)]);
        assert_eq!(event, SessionEvent::ToMenu);
        assert!(game.engine.movie.is_none());
        assert!(!mixer.is_playing(CHANNEL_MOVIE));
    }
}
