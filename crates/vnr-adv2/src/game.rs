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
//! | `40` | show the staged scene (cross-fade unless the first argument is 0) | data, provisional |
//! | `45` | fill the scene with a colour | data, provisional |
//! | `60` | sound effect | data |
//! | `70`, `73` | music track `MM%03d`, stop music | data, provisional |
//! | `80` | voice | data (`usr 113`) |
//! | `d5` | title menu; 1-based choice into the variable | `start.obj` |
//!
//! Everything else is ignored for now and reported once on stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vnr_core::text::find_system_font;
use vnr_core::{
    App, BackendError, Canvas, Color, FULL_VOLUME, Flow, Font, FontFamily, Frame, InputEvent, Key,
    Mixer, MouseButton, Sprite, TextStyle,
};

use crate::assets::Assets;
use crate::ini::Settings;
use crate::vm::{Control, Host, Machine, Status, Value};

/// Screen size of the engine.
pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

/// Mixer channels.
pub const CHANNEL_MUSIC: usize = 0;
pub const CHANNEL_SOUND: usize = 1;
pub const CHANNEL_VOICE: usize = 2;
pub const CHANNELS: usize = 3;

const FADE_FRAMES: u64 = 15;
const CHARS_PER_FRAME: usize = 1;
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
    /// The script stopped.
    Ended(String),
}

/// What is on screen, apart from text.
#[derive(Clone, Default)]
struct Scene {
    fill: Option<Color>,
    background: Option<Arc<Sprite>>,
    characters: BTreeMap<i32, Arc<Sprite>>,
}

impl Scene {
    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(self.fill.unwrap_or(Color::BLACK));
        if let Some(bg) = &self.background {
            canvas.draw(bg, 0, 0, 255);
        }
        for sprite in self.characters.values() {
            canvas.draw(sprite, 0, 0, 255);
        }
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
    /// Cross-fade in progress: the old picture and the start frame.
    fade: Option<(Canvas, u64)>,
    window_visible: bool,
    speaker: (usize, String),
    chapter: String,
    engine_vars: BTreeMap<u16, i32>,
    unknown: BTreeSet<u8>,
    skipping: bool,
    hidden: bool,
}

impl Engine {
    fn play(&self, channel: usize, clip: Option<Arc<vnr_core::Clip>>) {
        if let (Some(mixer), Some(clip)) = (&self.mixer, clip) {
            mixer.play(channel, clip, FULL_VOLUME);
        }
    }

    /// Shows the staged scene, with a cross-fade if `fade`.
    fn present(&mut self, fade: bool) {
        if fade && !self.skipping {
            self.fade = Some((self.shown_canvas.clone(), self.frame));
        }
        self.shown = self.staged.clone();
        self.shown.draw(&mut self.shown_canvas);
    }

    fn present_if_changed(&mut self) {
        let same = self.shown.fill == self.staged.fill
            && opt_ptr(&self.shown.background) == opt_ptr(&self.staged.background)
            && self.shown.characters.len() == self.staged.characters.len()
            && self
                .shown
                .characters
                .iter()
                .zip(&self.staged.characters)
                .all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y));
        if !same {
            self.present(true);
        }
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
        if let Some(voice) = voice {
            let clip = self.assets.voice(&voice);
            self.play(CHANNEL_VOICE, clip);
        }
        let text = clean_text(&text);
        if text.trim().is_empty() {
            return Control::Continue;
        }
        self.present_if_changed();
        self.window_visible = true;
        let speaker = clean_text(&self.speaker.1);
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
                    Ok(script) => vm.start(script),
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
                self.staged.background = self.assets.picture(&text(0), &["crgb"]);
                self.staged.characters.clear();
                self.staged.fill = None;
            }
            0x31 => {
                let slot = num(vm, self, 0);
                match self.assets.picture(&text(2), &["char"]) {
                    Some(sprite) => {
                        self.staged.characters.insert(slot, sprite);
                    }
                    None => {
                        self.staged.characters.remove(&slot);
                    }
                }
            }
            0x40 => {
                let effect = num(vm, self, 0);
                self.present(effect != 0);
            }
            0x45 => {
                let c = |v: i32| v.clamp(0, 255) as u8;
                self.staged.fill = Some(Color::rgb(
                    c(num(vm, self, 0)),
                    c(num(vm, self, 1)),
                    c(num(vm, self, 2)),
                ));
                self.staged.background = None;
                self.staged.characters.clear();
            }
            0x60 => {
                let clip = self.assets.sound(&text(0));
                self.play(CHANNEL_SOUND, clip);
            }
            0x70 => {
                let track = num(vm, self, 0);
                let clip = self.assets.sound(&format!("mm{track:03}"));
                self.play(CHANNEL_MUSIC, clip);
            }
            0x73 => {
                if let Some(mixer) = &self.mixer {
                    mixer.stop(CHANNEL_MUSIC);
                }
            }
            0x80 => {
                let clip = self.assets.voice(&text(1));
                self.play(CHANNEL_VOICE, clip);
            }
            0xD5 => {
                let variable = args.iter().find_map(|&v| vm.variable_ref(v));
                self.staged = Scene {
                    background: self.assets.picture("TITL00a", &["crgb"]),
                    ..Scene::default()
                };
                self.present(true);
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
        vm.load_library(assets.script("pmail")?);
        vm.start(assets.script(entry.unwrap_or("start"))?);
        let settings = Settings::load(vnr_core::find_ci(root, "OF_ADV2.INI").as_deref());
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
            fade: None,
            window_visible: false,
            speaker: (0, String::new()),
            chapter: String::new(),
            engine_vars: BTreeMap::new(),
            unknown: BTreeSet::new(),
            skipping: false,
            hidden: false,
        };
        Ok(Self { title, vm, engine })
    }

    fn input(&mut self, frame: &Frame<'_>) -> Option<Flow> {
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
        let mut answer: Option<(Option<u16>, i32)> = None;
        match &mut e.mode {
            Mode::Message { text, shown, .. } => {
                let total = text.chars().count();
                if *shown < total && !(advance || skip) {
                    *shown += CHARS_PER_FRAME;
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
            Mode::Running | Mode::Ended(_) => {}
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
                canvas.fill_rect(x, y, w, h, e.settings.message_window().with_opacity(150));
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
                canvas.fill_rect(x, y, w, h, e.settings.message_window().with_opacity(150));
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

        frame.canvas.blend_from(&self.engine.shown_canvas, 255);
        if let Some((old, start)) = &self.engine.fade {
            let t = frame.number.saturating_sub(*start);
            if t >= FADE_FRAMES {
                self.engine.fade = None;
            } else {
                let mut faded = old.clone();
                faded.blend_from(&self.engine.shown_canvas, (t * 255 / FADE_FRAMES) as u8);
                frame.canvas.blend_from(&faded, 255);
            }
        }
        self.draw_title(frame.canvas);
        self.draw_text_box(frame.canvas);
        self.draw_end(frame.canvas);
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
            (20..400).step_by(20).map(|f| (f, start)).collect();
        script.extend((410..1400).step_by(20).map(|f| (f, click)));
        let canvas = vnr_core::run_headless(&mut game, &mixer, 1400, &script);
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
