//! A linear script preview: walks one script from the top and shows what
//! it can without a full interpreter.
//!
//! It exists to exercise the runtime (pictures, text, sound, input) on
//! real data while the interpreter is being written. It does **not**
//! evaluate expressions or variables, ignores conditional jumps, follows
//! only forward unconditional jumps, and does not run the shared
//! functions in `pmail.obj` (so there is no music yet). What it does
//! show:
//!
//! | Script call | Shown as |
//! |---|---|
//! | `text` + `sys 0x01` | message, typed out |
//! | `sys 0x04` | monologue message |
//! | `sys 0x05` | speaker name and text colour |
//! | `sys 0x12` | chapter title |
//! | `sys 0x30` | background, with a cross-fade |
//! | `sys 0x31` | character sprite |
//! | `sys 0x21` | wait (milliseconds) |
//! | `sys 0x60` | sound effect |
//! | `sys 0x10` | continue in another script |
//! | `usr 113` | voice (its file-name argument) |

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use overflow_adv2::script::{Opcode, Operand, Script};
use overflow_adv2::{Pack, PixelFormat, audio, graphic};
use vnr_core::text::find_system_font;
use vnr_core::{
    App, BackendError, Canvas, Clip, Color, FULL_VOLUME, Flow, Font, FontFamily, Frame, InputEvent,
    Key, MouseButton, Sprite, TextStyle,
};

use crate::ini::Settings;

/// Screen size of the engine.
pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

/// Mixer channels.
pub const CHANNEL_MUSIC: usize = 0;
pub const CHANNEL_SOUND: usize = 1;
pub const CHANNEL_VOICE: usize = 2;
pub const CHANNELS: usize = 3;

/// Cross-fade length in frames (`GrpWipeWait=15` in the INI).
const FADE_FRAMES: u64 = 15;
/// Characters revealed per frame while a message is typed out.
const CHARS_PER_FRAME: usize = 1;
/// Instructions run per frame at most, so a loop cannot hang the window.
const STEP_BUDGET: usize = 50_000;
/// The shared function that plays a voice (see `pmail.obj`).
const USR_VOICE: u8 = 113;

const TEXT_SIZE: f32 = 22.0;
const LINE_HEIGHT: f32 = 30.0;
const BOX: (i32, i32, u32, u32) = (20, 424, 760, 166);

type PackFile = Pack<BufReader<File>>;

fn open_pack(root: &Path, name: &str) -> Result<Option<PackFile>, BackendError> {
    let Some(path) = vnr_core::resolve_ci(root, &format!("PACK/{name}")) else {
        return Ok(None);
    };
    Ok(Some(
        Pack::open(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    ))
}

/// Pictures and sounds, decoded on first use.
struct Assets {
    script: PackFile,
    graphic: PackFile,
    sound: Option<PackFile>,
    voice: Option<PackFile>,
    pictures: HashMap<String, Option<Arc<Sprite>>>,
}

impl Assets {
    fn open(root: &Path) -> Result<Self, BackendError> {
        let required = |name: &str| -> Result<PackFile, BackendError> {
            open_pack(root, name)?.ok_or_else(|| format!("PACK/{name} is missing").into())
        };
        Ok(Self {
            script: required("SCRIPT.DAT")?,
            graphic: required("GRAPHIC.DAT")?,
            sound: open_pack(root, "SOUND.DAT")?,
            voice: open_pack(root, "VOICE.DAT")?,
            pictures: HashMap::new(),
        })
    }

    fn script(&mut self, name: &str) -> Result<Script, BackendError> {
        let file = if name.to_ascii_lowercase().ends_with(".obj") {
            name.to_owned()
        } else {
            format!("{name}.obj")
        };
        let entry = self
            .script
            .find(&file)
            .ok_or_else(|| format!("script {file} not found"))?
            .clone();
        let data = self.script.read(&entry)?;
        Ok(Script::parse(&data).map_err(|e| format!("{file}: {e}"))?)
    }

    /// The first picture of `stem` with one of `extensions`.
    fn picture(&mut self, stem: &str, extensions: &[&str]) -> Option<Arc<Sprite>> {
        let key = format!("{}|{}", stem.to_ascii_lowercase(), extensions.join(","));
        if let Some(cached) = self.pictures.get(&key) {
            return cached.clone();
        }
        let sprite = extensions.iter().find_map(|ext| {
            let entry = self.graphic.find(&format!("{stem}.{ext}"))?.clone();
            let data = self.graphic.read(&entry).ok()?;
            let picture = graphic::decode(&entry.name, &data)
                .ok()?
                .into_iter()
                .next()?;
            let image = picture.image;
            Some(Arc::new(match image.format {
                PixelFormat::Rgb8 => Sprite::from_rgb(image.width, image.height, &image.pixels),
                PixelFormat::Rgba8 => Sprite::from_rgba(image.width, image.height, image.pixels),
                PixelFormat::Gray8 => {
                    Sprite::from_mask(image.width, image.height, &image.pixels, Color::WHITE)
                }
            }))
        });
        self.pictures.insert(key, sprite.clone());
        sprite
    }

    fn sound(pack: Option<&mut PackFile>, name: &str) -> Option<Arc<Clip>> {
        let pack = pack?;
        let entry = pack
            .find(&format!("{name}.adp4"))
            .or_else(|| pack.find(&format!("{name}.adps")))?
            .clone();
        let data = pack.read(&entry).ok()?;
        let clip = if entry.name.to_ascii_lowercase().ends_with(".adps") {
            let music = audio::decode_adps(&data).ok()?;
            let start = music.intro.len() / 2;
            let mut samples = music.intro;
            samples.extend_from_slice(&music.looped);
            Clip::stereo(samples).looping_from(start)
        } else {
            Clip::mono(&audio::decode_adp4(&data).ok()?)
        };
        Some(Arc::new(clip))
    }
}

/// A value on the machine's stack, as far as the preview tracks it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Number(i32),
    Text(String),
    /// Something the preview does not evaluate (expressions, variables).
    Unknown,
}

impl Value {
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    fn number(&self) -> Option<i32> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// Replaces engine placeholders with something readable.
fn clean_text(text: &str) -> String {
    text.replace("\\n", "\n")
        .replace("$N0", "主人公")
        .replace("$H0", "ＨＮ")
}

struct Message {
    text: String,
    speaker: Option<String>,
    style: usize,
    shown: usize,
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

/// The preview app.
pub struct Preview {
    title: String,
    assets: Assets,
    settings: Settings,
    fonts: Fonts,

    script_name: String,
    script: Script,
    pc: usize,
    acc: Value,
    stack: Vec<Value>,
    queue: Vec<String>,
    speaker: (usize, String),

    chapter: String,
    background: Option<Arc<Sprite>>,
    characters: Vec<Arc<Sprite>>,
    scene: Canvas,
    scene_dirty: bool,
    previous: Option<(Canvas, u64)>,
    fade_pending: bool,

    message: Option<Message>,
    wait_until: Option<u64>,
    finished: bool,
    hidden: bool,
    skipping: bool,
}

impl Preview {
    pub fn new(
        root: &Path,
        title: String,
        entry: &str,
        font: Option<PathBuf>,
    ) -> Result<Self, BackendError> {
        let mut assets = Assets::open(root)?;
        let script = assets.script(entry)?;
        let settings = Settings::load(vnr_core::find_ci(root, "OF_ADV2.INI").as_deref());
        Ok(Self {
            title,
            assets,
            settings,
            fonts: Fonts::load(font.as_deref())?,
            script_name: entry.to_owned(),
            script,
            pc: 0,
            acc: Value::Unknown,
            stack: Vec::new(),
            queue: Vec::new(),
            speaker: (0, String::new()),
            chapter: String::new(),
            background: None,
            characters: Vec::new(),
            scene: Canvas::new(WIDTH, HEIGHT),
            scene_dirty: true,
            previous: None,
            fade_pending: false,
            message: None,
            wait_until: None,
            finished: false,
            hidden: false,
            skipping: false,
        })
    }

    /// Arguments of a call with `count` parameters, oldest first.
    fn args(&self, count: usize) -> Vec<Value> {
        let start = self.stack.len().saturating_sub(count);
        self.stack[start..].to_vec()
    }

    fn operand_value(&self, operand: Operand) -> Value {
        match operand {
            Operand::Zero => Value::Number(0),
            Operand::One => Value::Number(1),
            Operand::High(n) => Value::Number(i32::from(n)),
            Operand::Number(n) => Value::Number(i32::from(n)),
            Operand::Pool(n) => self.script.text(n).map_or(Value::Unknown, Value::Text),
            _ => Value::Unknown,
        }
    }

    /// Runs instructions until something needs the player or time.
    fn run(&mut self, frame: &Frame<'_>) {
        for _ in 0..STEP_BUDGET {
            if self.finished || self.message.is_some() || self.wait_until.is_some() {
                return;
            }
            let Some(&ins) = self.script.code.get(self.pc) else {
                self.finished = true;
                return;
            };
            let pc = self.pc;
            self.pc += 1;
            let Some(op) = ins.op() else { continue };
            let operand = ins.operand();
            match op {
                Opcode::Load => self.acc = self.operand_value(operand),
                Opcode::Push => self.stack.push(self.acc.clone()),
                Opcode::Pop => {
                    let n = usize::from(ins.value);
                    let keep = self.stack.len().saturating_sub(n);
                    if let Some(last) = self.stack.drain(keep..).next_back() {
                        self.acc = last;
                    }
                }
                Opcode::Text => {
                    if let Some(text) = self.acc.text() {
                        self.queue.push(text.to_owned());
                    }
                }
                Opcode::Jump => {
                    if let (Operand::Forward(_), Some(target)) = (operand, ins.jump_target(pc)) {
                        self.pc = target;
                    }
                }
                Opcode::Halt => self.finished = true,
                Opcode::Usr => {
                    if operand == Operand::High(USR_VOICE) {
                        self.play_voice(frame);
                    }
                }
                Opcode::Sys => {
                    if let Operand::Pair(number, argc) = operand {
                        let args = self.args(usize::from(argc));
                        if let Err(e) = self.sys(frame, number, &args) {
                            eprintln!("{}: {e}", self.script_name);
                            self.finished = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn play_voice(&mut self, frame: &Frame<'_>) {
        let names: Vec<String> = self
            .args(4)
            .iter()
            .filter_map(|v| v.text().map(str::to_owned))
            .collect();
        for name in names.iter().filter(|n| !n.is_empty()) {
            if let Some(clip) = Assets::sound(self.assets.voice.as_mut(), name) {
                frame.mixer.play(CHANNEL_VOICE, clip, FULL_VOLUME);
                return;
            }
        }
    }

    fn sys(&mut self, frame: &Frame<'_>, number: u8, args: &[Value]) -> Result<(), BackendError> {
        let text_arg = |i: usize| args.get(i).and_then(Value::text).unwrap_or("");
        match number {
            0x01 => {
                let text = clean_text(&self.queue.concat());
                self.queue.clear();
                let speaker = clean_text(&self.speaker.1);
                self.show(
                    text,
                    (!speaker.is_empty()).then_some(speaker),
                    self.speaker.0,
                );
            }
            0x04 => {
                let style = args.first().and_then(Value::number).unwrap_or(0);
                self.show(clean_text(text_arg(1)), None, style.max(0) as usize);
            }
            0x05 => {
                let style = args.first().and_then(Value::number).unwrap_or(0);
                self.speaker = (style.max(0) as usize, text_arg(1).to_owned());
            }
            0x10 => {
                let name = text_arg(0).to_owned();
                self.script = self.assets.script(&name)?;
                self.script_name = name;
                self.pc = 0;
                self.stack.clear();
            }
            0x12 => self.chapter = text_arg(0).to_owned(),
            0x21 => {
                let ms = args
                    .first()
                    .and_then(Value::number)
                    .unwrap_or(0)
                    .clamp(0, 5000);
                if !self.skipping {
                    self.wait_until = Some(frame.number + (ms as u64 * 60).div_ceil(1000));
                }
            }
            0x30 => {
                self.background = self.assets.picture(text_arg(0), &["crgb"]);
                self.characters.clear();
                self.scene_dirty = true;
                self.fade_pending = true;
            }
            0x31 => {
                let sprite = args
                    .iter()
                    .filter_map(Value::text)
                    .find_map(|name| self.assets.picture(name, &["char"]));
                if let Some(sprite) = sprite {
                    self.characters = vec![sprite];
                    self.scene_dirty = true;
                    self.fade_pending = true;
                }
            }
            0x60 => {
                if let Some(clip) = Assets::sound(self.assets.sound.as_mut(), text_arg(0)) {
                    frame.mixer.play(CHANNEL_SOUND, clip, FULL_VOLUME);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn show(&mut self, text: String, speaker: Option<String>, style: usize) {
        if text.trim().is_empty() {
            return;
        }
        self.message = Some(Message {
            text,
            speaker,
            style,
            shown: 0,
        });
    }

    /// Handles input; returns whether the player asked to advance.
    fn input(&mut self, frame: &Frame<'_>) -> Option<Flow> {
        let mut advance = false;
        for event in frame.input {
            match event {
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    ..
                }
                | InputEvent::KeyDown(Key::Enter | Key::Space | Key::Down) => {
                    if self.hidden {
                        self.hidden = false;
                    } else {
                        advance = true;
                    }
                }
                InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => self.hidden = !self.hidden,
                InputEvent::KeyDown(Key::Control) => self.skipping = true,
                InputEvent::KeyUp(Key::Control) => self.skipping = false,
                InputEvent::KeyDown(Key::Escape) => return Some(Flow::Quit),
                _ => {}
            }
        }
        let advance = advance || self.skipping;
        if let Some(message) = &mut self.message {
            let total = message.text.chars().count();
            if message.shown < total {
                message.shown = if advance {
                    total
                } else {
                    message.shown + CHARS_PER_FRAME
                };
            } else if advance {
                self.message = None;
            }
        }
        if let Some(until) = self.wait_until
            && (frame.number >= until || advance)
        {
            self.wait_until = None;
        }
        None
    }

    fn compose_scene(&mut self) {
        self.scene.clear(Color::BLACK);
        if let Some(bg) = &self.background {
            self.scene.draw(bg, 0, 0, 255);
        }
        for sprite in &self.characters {
            self.scene.draw(sprite, 0, 0, 255);
        }
        self.scene_dirty = false;
    }

    fn draw_message(&mut self, canvas: &mut Canvas) {
        let (x, y, w, h) = BOX;
        if !self.chapter.is_empty() {
            let style = TextStyle {
                size: 18.0,
                color: Color::WHITE,
                shadow: Some(Color::BLACK),
                bold: false,
            };
            let text = clean_text(&self.chapter);
            self.fonts
                .get(FontFamily::Gothic)
                .draw_line(canvas, 12.0, 8.0, &text, style);
        }
        let Some(message) = &self.message else { return };
        if self.hidden {
            return;
        }
        canvas.fill_rect(x, y, w, h, self.settings.message_window().with_opacity(150));
        let char_style = self.settings.char_style(message.style);
        let style = TextStyle {
            size: TEXT_SIZE,
            color: char_style.color,
            shadow: char_style.shadow.then_some(Color::BLACK),
            bold: char_style.bold,
        };
        let font = self.fonts.get(char_style.family);
        let mut top = y as f32 + 12.0;
        let left = x as f32 + 20.0;
        if let Some(speaker) = &message.speaker {
            font.draw_line(canvas, left, top, speaker, style);
            top += LINE_HEIGHT;
        }
        let visible: String = message.text.chars().take(message.shown).collect();
        for line in font
            .wrap(&visible, TEXT_SIZE, w as f32 - 40.0)
            .iter()
            .take(5)
        {
            font.draw_line(canvas, left, top, line, style);
            top += LINE_HEIGHT;
        }
    }
}

impl App for Preview {
    fn title(&self) -> String {
        format!("{} — preview of {}", self.title, self.script_name)
    }

    fn resolution(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn update(&mut self, frame: &mut Frame<'_>) -> Flow {
        if let Some(flow) = self.input(frame) {
            return flow;
        }
        self.run(frame);
        if self.scene_dirty {
            // The scene canvas still holds the old picture until it is
            // composed again below.
            if self.fade_pending && !self.skipping {
                self.previous = Some((self.scene.clone(), frame.number));
            }
            self.fade_pending = false;
            self.compose_scene();
        }
        frame.canvas.blend_from(&self.scene, 255);
        if let Some((old, start)) = &self.previous {
            let t = frame.number.saturating_sub(*start);
            if t >= FADE_FRAMES {
                self.previous = None;
            } else {
                let mut faded = old.clone();
                faded.blend_from(&self.scene, (t * 255 / FADE_FRAMES) as u8);
                frame.canvas.blend_from(&faded, 255);
            }
        }
        self.draw_message(frame.canvas);
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays the prologue headless against a real install
    /// (`ADV2_GAME_DIR`) and checks that pictures and text appear.
    #[test]
    fn previews_the_prologue() {
        let Some(dir) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let mut preview = match Preview::new(Path::new(&dir), "Pure Mail".into(), "PRO1", None) {
            Ok(p) => p,
            Err(e) if e.to_string().contains("no Japanese font") => {
                eprintln!("{e}, skipping");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        assert!(preview.assets.picture("STHI01K0", &["char"]).is_some());
        assert!(preview.assets.picture("nothing", &["crgb"]).is_none());

        let mixer = vnr_core::Mixer::new(CHANNELS);
        let click = InputEvent::MouseDown {
            button: MouseButton::Left,
            x: 0,
            y: 0,
        };
        let script: Vec<(u64, InputEvent)> = (30..600).step_by(30).map(|f| (f, click)).collect();
        let canvas = vnr_core::run_headless(&mut preview, &mixer, 600, &script);
        assert_eq!(preview.chapter, "プロローグ");
        assert!(preview.background.is_some());
        assert!(!preview.finished);
        let lit = canvas.rgba().chunks_exact(4).filter(|p| p[0] > 128).count();
        assert!(lit > 500, "only {lit} bright pixels");
    }

    #[test]
    fn cleans_placeholders() {
        assert_eq!(clean_text("【$N0】"), "【主人公】");
        assert_eq!(clean_text("a\\nb"), "a\nb");
    }
}
