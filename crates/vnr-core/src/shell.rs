//! The universal interface around every game.
//!
//! [`Shell`] is the [`App`] the frontends run. It draws at
//! [`ui::WIDTH`] × [`ui::HEIGHT`], shows the game's own screen scaled to
//! fit (with bands at the sides when the aspect ratios differ) and owns
//! everything that is not the game itself: the main menu, the pause menu,
//! saving and loading, the history, the settings and the extras (CG
//! gallery, music room, scene replay). The game is a [`Session`]
//! provided by its backend.
//!
//! In a game, Escape or the right mouse button open the pause menu; F2
//! saves, F3 loads, F4 opens the settings and the wheel (up) or Page Up
//! open the history.

mod extras;
mod library;

pub use extras::MUSIC_CHANNEL;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::{App, Flow, Frame, InputEvent, Key, MouseButton};
use crate::audio::Mixer;
use crate::gfx::{Canvas, Color, Sprite};
use crate::lang::{Language, Strings};
use crate::library::Library;
use crate::saves::{SLOTS, SlotSummary, Slots};
use crate::session::{HistoryEntry, Session, SessionEvent, Start};
use crate::settings::{MAX_LEVEL, MAX_STEP, Outline, Settings, StartScreen};
use crate::text::{Font, FontChoice, FontFamily, TextStyle};
use crate::ui::{self, Rect, State, theme};
use crate::{GameInfo, Launcher};

/// Where the shell keeps its files.
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    /// Folder of `settings.json` and `lang/`; `None` keeps settings in
    /// memory only.
    pub data_dir: Option<PathBuf>,
    /// The game's save folder; `None` disables saving.
    pub saves: Option<PathBuf>,
    /// A font file chosen on the command line: it replaces both font
    /// settings.
    pub font_file: Option<PathBuf>,
}

/// A change of the window the shell asks the frontend for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRequest {
    pub fullscreen: bool,
}

/// Loads the interface font: `path` if given, else an installed
/// Japanese gothic font (game texts in the history are Japanese).
pub fn load_font(path: Option<&Path>) -> Result<Font, String> {
    match path {
        Some(path) => Font::load(path, 0),
        None => FontChoice::default().load(FontFamily::Gothic),
    }
}

const MENU_ITEMS: [&str; 6] = [
    "menu.start",
    "menu.load",
    "menu.extras",
    "menu.settings",
    "menu.library",
    "menu.quit",
];
const PAUSE_ITEMS: [&str; 8] = [
    "pause.resume",
    "pause.save",
    "pause.load",
    "pause.history",
    "pause.auto",
    "pause.settings",
    "pause.menu",
    "pause.quit",
];
const TABS: [&str; 4] = [
    "settings.tab.system",
    "settings.tab.sound",
    "settings.tab.screen",
    "settings.tab.reading",
];

/// One setting row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Language,
    Start,
    Fullscreen,
    UnlockAll,
    Master,
    Music,
    Sound,
    Voice,
    TextSpeed,
    Window,
    Font,
    GameFont,
    TextSize,
    LineSpacing,
    Outline,
    HighContrast,
    AutoSpeed,
    WaitVoice,
    SkipUnread,
}

impl Setting {
    fn tab(tab: usize) -> &'static [Setting] {
        match tab {
            0 => &[
                Setting::Language,
                Setting::Font,
                Setting::Start,
                Setting::Fullscreen,
                Setting::UnlockAll,
            ],
            1 => &[
                Setting::Master,
                Setting::Music,
                Setting::Sound,
                Setting::Voice,
            ],
            2 => &[
                Setting::GameFont,
                Setting::TextSize,
                Setting::LineSpacing,
                Setting::Outline,
                Setting::HighContrast,
                Setting::Window,
            ],
            _ => &[
                Setting::TextSpeed,
                Setting::AutoSpeed,
                Setting::WaitVoice,
                Setting::SkipUnread,
            ],
        }
    }

    /// Highest value of a level setting.
    fn max(self) -> u8 {
        match self {
            Setting::TextSize | Setting::LineSpacing => MAX_STEP,
            _ => MAX_LEVEL,
        }
    }

    /// Value of an on/off setting.
    fn toggle(self, s: &mut Settings) -> Option<&mut bool> {
        Some(match self {
            Setting::Fullscreen => &mut s.fullscreen,
            Setting::UnlockAll => &mut s.unlock_all,
            Setting::HighContrast => &mut s.high_contrast,
            Setting::WaitVoice => &mut s.wait_voice,
            Setting::SkipUnread => &mut s.skip_unread,
            _ => return None,
        })
    }

    fn key(self) -> &'static str {
        match self {
            Setting::Language => "settings.language",
            Setting::Start => "settings.start",
            Setting::Fullscreen => "settings.fullscreen",
            Setting::UnlockAll => "settings.unlock_all",
            Setting::Master => "settings.master",
            Setting::Music => "settings.music",
            Setting::Sound => "settings.sound",
            Setting::Voice => "settings.voice",
            Setting::TextSpeed => "settings.text_speed",
            Setting::Window => "settings.window",
            Setting::Font => "settings.font",
            Setting::GameFont => "settings.game_font",
            Setting::TextSize => "settings.text_size",
            Setting::LineSpacing => "settings.line_spacing",
            Setting::Outline => "settings.outline",
            Setting::HighContrast => "settings.high_contrast",
            Setting::AutoSpeed => "settings.auto_speed",
            Setting::WaitVoice => "settings.wait_voice",
            Setting::SkipUnread => "settings.skip_unread",
        }
    }

    fn level(self, s: &mut Settings) -> Option<&mut u8> {
        Some(match self {
            Setting::Master => &mut s.master,
            Setting::Music => &mut s.music,
            Setting::Sound => &mut s.sound,
            Setting::Voice => &mut s.voice,
            Setting::TextSpeed => &mut s.text_speed,
            Setting::Window => &mut s.window,
            Setting::TextSize => &mut s.text_size,
            Setting::LineSpacing => &mut s.line_spacing,
            Setting::AutoSpeed => &mut s.auto_speed,
            _ => return None,
        })
    }

    /// Steps a choice or toggle forwards or backwards.
    fn cycle(self, s: &mut Settings, forward: bool) {
        match self {
            Setting::Language => {
                let all = Language::ALL;
                let i = all.iter().position(|&l| l == s.language).unwrap_or(0);
                let n = all.len();
                s.language = all[if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }];
            }
            Setting::Start => {
                s.start = match s.start {
                    StartScreen::Universal => StartScreen::Original,
                    StartScreen::Original => StartScreen::Universal,
                }
            }
            Setting::Font | Setting::GameFont => {
                let font = if self == Setting::Font {
                    &mut s.font
                } else {
                    &mut s.game_font
                };
                let all = FontChoice::installed();
                let i = all.iter().position(|f| f == font).unwrap_or(0);
                let n = all.len();
                *font = all[if forward {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }];
            }
            Setting::Outline => {
                const ALL: [Outline; 3] = [Outline::Off, Outline::Shadow, Outline::Outline];
                let i = ALL.iter().position(|&o| o == s.outline).unwrap_or(1);
                s.outline = ALL[if forward { (i + 1) % 3 } else { (i + 2) % 3 }];
            }
            other => {
                let max = other.max();
                if let Some(on) = other.toggle(s) {
                    *on = !*on;
                } else if let Some(level) = other.level(s) {
                    *level = if forward {
                        (*level + 1).min(max)
                    } else {
                        level.saturating_sub(1)
                    };
                }
            }
        }
    }
}

/// History rows, laid out once when the screen opens.
#[derive(Debug, Clone)]
struct HistoryRow {
    entry: usize,
    text: String,
    speaker: bool,
    /// The first text row of an entry (marked when it has a voice).
    first: bool,
    /// Space above the row.
    gap: i32,
}

enum Screen {
    MainMenu {
        focus: Option<usize>,
    },
    Pause {
        focus: Option<usize>,
    },
    Slots {
        saving: bool,
        list: Vec<Option<SlotSummary>>,
        focus: Option<usize>,
        error: Option<String>,
        /// Opened by the game itself (its title's load option).
        requested: bool,
    },
    History {
        entries: Vec<HistoryEntry>,
        rows: Vec<HistoryRow>,
        /// First row shown.
        scroll: usize,
        /// Text size and row height, from the text settings.
        size: f32,
        row: i32,
    },
    Settings {
        tab: usize,
        focus: Option<usize>,
    },
    Extras(extras::Extras),
    Viewer(extras::Viewer),
    Library(library::LibraryScreen),
    Picker(library::Picker),
}

/// What a screen asks for after its input.
enum Action {
    Push(Screen),
    /// Close the top screen.
    Close,
    Start(Start),
    Save(usize),
    Load(usize),
    /// Leave the game for the main menu.
    ToMenu,
    /// Replace the top screen.
    Replace(Screen),
    /// Leave the game for the library.
    ChangeGame,
    /// Open a game of the library.
    OpenGame(PathBuf),
    Quit,
}

/// The session when no game is open (the library is on screen).
struct NoGame;

impl Session for NoGame {
    fn title(&self) -> String {
        "VNRevival".into()
    }

    fn resolution(&self) -> (u32, u32) {
        (800, 600)
    }

    fn start(&mut self, _start: Start) -> Result<(), String> {
        Err("no game is open".into())
    }

    fn update(&mut self, _frame: &mut Frame<'_>) -> SessionEvent {
        SessionEvent::ToMenu
    }

    fn can_save(&self) -> bool {
        false
    }

    fn save(&self) -> Result<crate::session::SaveData, String> {
        Err("no game is open".into())
    }

    fn load(&mut self, _data: &crate::session::SaveData) -> Result<(), String> {
        Err("no game is open".into())
    }
}

/// The universal interface: see the [module docs](self).
pub struct Shell {
    session: Box<dyn Session>,
    game: Canvas,
    /// Frames the game has run (it stands still under the menus).
    game_frame: u64,
    started: bool,
    /// Whether a game is in progress (else the main menu is the base).
    in_game: bool,
    font: Font,
    settings: Settings,
    data_dir: Option<PathBuf>,
    strings: Strings,
    slots: Option<Slots>,
    screens: Vec<Screen>,
    mouse: (i32, i32),
    mixer: Option<Mixer>,
    window: Option<WindowRequest>,
    quit: bool,
    /// Whether the game has any extras.
    extras: bool,
    /// Extras thumbnails loaded so far.
    thumbs: HashMap<extras::ThumbKey, Option<Arc<Sprite>>>,
    /// A scene of the extras is being replayed.
    replaying: bool,
    /// The game is in progress but a screen covers it.
    frozen: bool,
    /// The font file from the command line.
    font_file: Option<PathBuf>,
    /// Font of the game text in the history and the settings preview
    /// (`None`: it could not be loaded, the interface font is used).
    game_font: Option<Font>,
    /// Auto mode is on.
    auto: bool,
    /// Finds and opens games for the library, when there is one.
    launcher: Option<Box<dyn Launcher>>,
    library: Library,
    /// Install folder of the open game, when it came from the launcher.
    game_root: Option<PathBuf>,
}

/// History layout.
const HISTORY_PANEL: Rect = Rect::new(140, 40, 1000, 640);
/// History text size for the default game text size (22).
const HISTORY_SIZE: f32 = 20.0;
const HISTORY_GAP: i32 = 12;
const HISTORY_TOP: i32 = HISTORY_PANEL.y + 80;
const HISTORY_BOTTOM: i32 = HISTORY_PANEL.y + 640 - 50;

impl Shell {
    pub fn new(session: Box<dyn Session>, font: Font, options: ShellOptions) -> Self {
        let mut font = font;
        let settings: Settings = options
            .data_dir
            .as_ref()
            .map(|d| Settings::load(&d.join("settings.json")))
            .unwrap_or_default();
        if options.font_file.is_none() && settings.font != FontChoice::default() {
            match settings.font.load(FontFamily::Gothic) {
                Ok(chosen) => font = chosen,
                Err(e) => eprintln!("{e}"),
            }
        }
        let game_font = match &options.font_file {
            Some(path) => Font::load(path, 0),
            None => settings.game_font.load(FontFamily::Gothic),
        };
        let game_font = game_font.map_err(|e| eprintln!("{e}")).ok();
        let lang_dir = options.data_dir.as_ref().map(|d| d.join("lang"));
        let strings = Strings::new(settings.language, lang_dir.as_deref());
        let (w, h) = session.resolution();
        let mut session = session;
        let extras = !(session.gallery().is_empty()
            && session.tracks().is_empty()
            && session.scenes().is_empty());
        Self {
            session,
            extras,
            thumbs: HashMap::new(),
            replaying: false,
            frozen: false,
            font_file: options.font_file.clone(),
            game_font,
            auto: false,
            launcher: None,
            library: Library::open(options.data_dir.clone()),
            game_root: None,
            game: Canvas::new(w, h),
            game_frame: 0,
            started: false,
            in_game: false,
            font,
            window: settings
                .fullscreen
                .then_some(WindowRequest { fullscreen: true }),
            settings,
            data_dir: options.data_dir,
            strings,
            slots: options.saves.map(Slots::new),
            screens: Vec::new(),
            mouse: (0, 0),
            mixer: None,
            quit: false,
        }
    }

    /// The launcher alone: the library of games is the first screen.
    pub fn launcher(launcher: Box<dyn Launcher>, font: Font, options: ShellOptions) -> Self {
        let mut shell = Self::new(Box::new(NoGame), font, options);
        shell.launcher = Some(launcher);
        shell.started = true;
        shell.screens = vec![shell.library_screen()];
        shell
    }

    /// Gives a shell opened on a game the launcher, so that "Change game"
    /// works; the game is added to the library.
    pub fn attach_launcher(&mut self, launcher: Box<dyn Launcher>, game: &GameInfo) {
        self.launcher = Some(launcher);
        self.library.add(game);
        self.library.touch(&game.root);
        if self.library.last_folder().is_none()
            && let Some(parent) = game.root.parent()
        {
            self.library.set_last_folder(parent);
        }
        self.game_root = Some(game.root.clone());
        self.store_cover();
    }

    /// Saves the open game's cover in the library, once.
    fn store_cover(&mut self) {
        let Some(root) = self.game_root.clone() else {
            return;
        };
        if let Some(cover) = self.session.cover()
            && let Err(e) = self.library.save_cover(&root, &cover)
        {
            eprintln!("could not save the cover: {e}");
        }
    }

    /// Opens the game in `root` from the library.
    fn open_game(&mut self, root: &Path) -> Result<(), String> {
        let launcher = self.launcher.as_ref().ok_or("no launcher")?;
        let info = launcher
            .detect(root)?
            .ok_or_else(|| format!("no supported game in {} any more", root.display()))?;
        let session = launcher.open(&info)?;
        self.set_session(session);
        self.slots = crate::save_dir(&info).map(Slots::new);
        self.library.touch(root);
        self.game_root = Some(root.to_path_buf());
        self.store_cover();
        self.screens.clear();
        match self.settings.start {
            StartScreen::Original => self.start(Start::OriginalTitle),
            StartScreen::Universal => self.screens.push(Screen::MainMenu { focus: None }),
        }
        Ok(())
    }

    /// Puts `session` in place of the current one, before it starts.
    fn set_session(&mut self, session: Box<dyn Session>) {
        let mut session = session;
        self.extras = !(session.gallery().is_empty()
            && session.tracks().is_empty()
            && session.scenes().is_empty());
        let (w, h) = session.resolution();
        self.session = session;
        self.game = Canvas::new(w, h);
        self.game_frame = 0;
        self.in_game = false;
        self.replaying = false;
        self.frozen = false;
        self.auto = false;
        self.thumbs.clear();
        self.apply_settings();
    }

    /// Closes the game and shows the library.
    fn change_game(&mut self) {
        self.session.silence();
        self.set_session(Box::new(NoGame));
        self.slots = None;
        self.game_root = None;
        self.screens = vec![self.library_screen()];
    }

    /// Starts the game at once instead of showing the start screen of
    /// the settings.
    pub fn begin(&mut self, start: Start) {
        self.started = true;
        self.start(start);
    }

    /// A change of the window the frontend should make, once.
    pub fn take_window_request(&mut self) -> Option<WindowRequest> {
        self.window.take()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings (as the settings screen does).
    pub fn set_settings(&mut self, settings: Settings) {
        let language = self.settings.language;
        let fullscreen = self.settings.fullscreen;
        let (font, game_font) = (self.settings.font, self.settings.game_font);
        self.settings = settings;
        if self.font_file.is_none() {
            if self.settings.font != font {
                match self.settings.font.load(FontFamily::Gothic) {
                    Ok(chosen) => self.font = chosen,
                    Err(e) => eprintln!("{e}"),
                }
            }
            if self.settings.game_font != game_font {
                match self.settings.game_font.load(FontFamily::Gothic) {
                    Ok(chosen) => self.game_font = Some(chosen),
                    Err(e) => eprintln!("{e}"),
                }
            }
        }
        if self.settings.language != language {
            let dir = self.data_dir.as_ref().map(|d| d.join("lang"));
            self.strings = Strings::new(self.settings.language, dir.as_deref());
        }
        if self.settings.fullscreen != fullscreen {
            self.window = Some(WindowRequest {
                fullscreen: self.settings.fullscreen,
            });
        }
        self.apply_settings();
        if let Some(dir) = &self.data_dir {
            let path = dir.join("settings.json");
            if let Err(e) = self.settings.store(&path) {
                eprintln!("could not save settings to {}: {e}", path.display());
            }
        }
    }

    fn apply_settings(&mut self) {
        if let Some(mixer) = &self.mixer {
            mixer.set_master_volume(Settings::volume(self.settings.master));
        }
        self.session.apply(&self.settings);
    }

    /// Whether the game is running with no menu over it.
    pub fn in_game(&self) -> bool {
        self.in_game && self.screens.is_empty()
    }

    /// The game being played.
    pub fn session(&self) -> &dyn Session {
        self.session.as_ref()
    }

    /// The game's screen, as last drawn.
    pub fn game_canvas(&self) -> &Canvas {
        &self.game
    }

    /// Where the game's screen is drawn.
    pub fn viewport(&self) -> Rect {
        let (gw, gh) = (self.game.width().max(1), self.game.height().max(1));
        let scale = f64::min(
            f64::from(ui::WIDTH) / f64::from(gw),
            f64::from(ui::HEIGHT) / f64::from(gh),
        );
        let w = (f64::from(gw) * scale).round() as u32;
        let h = (f64::from(gh) * scale).round() as u32;
        Rect::new(
            (ui::WIDTH as i32 - w as i32) / 2,
            (ui::HEIGHT as i32 - h as i32) / 2,
            w,
            h,
        )
    }

    /// Interface coordinates to game coordinates.
    pub fn to_game(&self, (x, y): (i32, i32)) -> (i32, i32) {
        let v = self.viewport();
        let gx = i64::from(x - v.x) * i64::from(self.game.width()) / i64::from(v.w.max(1));
        let gy = i64::from(y - v.y) * i64::from(self.game.height()) / i64::from(v.h.max(1));
        (gx as i32, gy as i32)
    }

    fn start(&mut self, start: Start) {
        self.session.silence();
        self.replaying = matches!(start, Start::Scene(_));
        match self.session.start(start) {
            Ok(()) => {
                self.in_game = true;
                self.screens.clear();
                self.apply_settings();
            }
            Err(e) => {
                eprintln!("could not start the game: {e}");
                self.back_to_menu();
            }
        }
    }

    fn back_to_menu(&mut self) {
        self.session.silence();
        self.set_auto(false);
        if self.replaying {
            self.end_replay();
            return;
        }
        if self.settings.start == StartScreen::Original && self.in_game {
            // The game's own title is the menu.
            self.start(Start::OriginalTitle);
            return;
        }
        self.in_game = false;
        self.screens = vec![Screen::MainMenu { focus: None }];
    }

    /// Back to the scene list after a replayed scene.
    fn end_replay(&mut self) {
        self.replaying = false;
        self.set_auto(false);
        self.in_game = false;
        self.session.silence();
        let list = self.extras_screen(false, extras::SCENES);
        self.screens = vec![Screen::MainMenu { focus: None }, list];
    }

    fn slots_screen(&self, saving: bool, requested: bool) -> Screen {
        Screen::Slots {
            saving,
            list: self
                .slots
                .as_ref()
                .map_or_else(|| vec![None; SLOTS], Slots::summaries),
            focus: None,
            error: self
                .slots
                .is_none()
                .then(|| self.strings.get("slots.disabled").to_string()),
            requested,
        }
    }

    fn history_screen(&mut self) -> Option<Screen> {
        let entries = self.session.history();
        if entries.is_empty() {
            return None;
        }
        let width = HISTORY_PANEL.w as f32 - 120.0;
        let size = (self.settings.text_px() * HISTORY_SIZE / 22.0).round();
        let row = self.settings.line_height(size) as i32 + 3;
        let mut rows = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let mut gap = HISTORY_GAP;
            if let Some(speaker) = &entry.speaker {
                rows.push(HistoryRow {
                    entry: i,
                    text: speaker.clone(),
                    speaker: true,
                    first: false,
                    gap,
                });
                gap = 0;
            }
            for (n, line) in self
                .text_font()
                .wrap(&entry.text, size, width)
                .into_iter()
                .enumerate()
            {
                rows.push(HistoryRow {
                    entry: i,
                    text: line,
                    speaker: false,
                    first: n == 0,
                    gap,
                });
                gap = 0;
            }
        }
        let scroll = history_last_page(&rows, row);
        Some(Screen::History {
            entries,
            rows,
            scroll,
            size,
            row,
        })
    }

    fn can_save(&self) -> bool {
        self.in_game && !self.replaying && self.slots.is_some() && self.session.can_save()
    }

    /// Runs one game frame with the input translated to game pixels.
    fn run_game(&mut self, input: &[InputEvent], mixer: &Mixer) {
        let translated: Vec<InputEvent> = input
            .iter()
            .map(|event| match *event {
                InputEvent::MouseMove { x, y } => {
                    let (x, y) = self.to_game((x, y));
                    InputEvent::MouseMove { x, y }
                }
                InputEvent::MouseDown { button, x, y } => {
                    let (x, y) = self.to_game((x, y));
                    InputEvent::MouseDown { button, x, y }
                }
                InputEvent::MouseUp { button, x, y } => {
                    let (x, y) = self.to_game((x, y));
                    InputEvent::MouseUp { button, x, y }
                }
                other => other,
            })
            .collect();
        let mut frame = Frame {
            number: self.game_frame,
            input: &translated,
            canvas: &mut self.game,
            mixer,
        };
        let event = self.session.update(&mut frame);
        self.game_frame += 1;
        match event {
            SessionEvent::Continue => {}
            SessionEvent::ToMenu if self.replaying => self.end_replay(),
            SessionEvent::ToMenu => {
                self.in_game = false;
                self.session.silence();
                self.screens = vec![Screen::MainMenu { focus: None }];
            }
            SessionEvent::OpenLoad => {
                let screen = self.slots_screen(false, true);
                self.screens.push(screen);
            }
            SessionEvent::OpenExtras if self.extras => {
                let screen = self.extras_screen(true, extras::GALLERY);
                self.screens.push(screen);
            }
            SessionEvent::OpenExtras => self.session.dismissed(),
            SessionEvent::Quit => self.quit = true,
        }
    }

    /// Shortcuts while playing; `true` if one was taken.
    fn game_shortcuts(&mut self, input: &[InputEvent]) -> bool {
        for event in input {
            let screen = match event {
                InputEvent::KeyDown(Key::Escape)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => Some(Screen::Pause { focus: None }),
                InputEvent::KeyDown(Key::F(2)) if self.can_save() => {
                    Some(self.slots_screen(true, false))
                }
                InputEvent::KeyDown(Key::F(3)) => Some(self.slots_screen(false, false)),
                InputEvent::KeyDown(Key::F(4)) => Some(Screen::Settings {
                    tab: 0,
                    focus: None,
                }),
                InputEvent::Wheel(1..) | InputEvent::KeyDown(Key::PageUp)
                    if self.session.can_save() =>
                {
                    self.history_screen()
                }
                InputEvent::KeyDown(Key::Char('a')) => {
                    self.set_auto(!self.auto);
                    None
                }
                _ => None,
            };
            if let Some(screen) = screen {
                self.screens.push(screen);
                return true;
            }
        }
        false
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Push(screen) => self.screens.push(screen),
            Action::Close => {
                match self.screens.pop() {
                    Some(Screen::Slots {
                        requested: true, ..
                    }) => self.session.dismissed(),
                    Some(Screen::Extras(ex)) if ex.requested => self.session.dismissed(),
                    _ => {}
                }
                if self.screens.is_empty() && !self.in_game {
                    self.screens.push(Screen::MainMenu { focus: None });
                }
            }
            Action::Start(start) => self.start(start),
            Action::Save(slot) => {
                let result = match (&self.slots, self.session.save()) {
                    (Some(slots), Ok(data)) => slots.write(slot, &data).map_err(|e| e.to_string()),
                    (None, _) => Err(self.strings.get("slots.disabled").to_string()),
                    (_, Err(e)) => Err(e),
                };
                self.finish_slot(result, false);
            }
            Action::Load(slot) => {
                let result = match &self.slots {
                    Some(slots) => match slots.read(slot) {
                        Ok(Some(data)) => {
                            self.session.silence();
                            self.session.load(&data)
                        }
                        Ok(None) => Err(self.strings.get("slots.empty").to_string()),
                        Err(e) => Err(e.to_string()),
                    },
                    None => Err(self.strings.get("slots.disabled").to_string()),
                };
                self.finish_slot(result, true);
            }
            Action::ToMenu => self.back_to_menu(),
            Action::Replace(screen) => {
                self.screens.pop();
                self.screens.push(screen);
            }
            Action::ChangeGame => self.change_game(),
            Action::OpenGame(root) => {
                if let Err(e) = self.open_game(&root)
                    && let Some(Screen::Library(lib)) = self.screens.last_mut()
                {
                    lib.error = Some(e);
                }
            }
            Action::Quit => self.quit = true,
        }
    }

    fn finish_slot(&mut self, result: Result<(), String>, loading: bool) {
        match result {
            Ok(()) if loading => {
                self.in_game = true;
                self.screens.clear();
                self.apply_settings();
            }
            Ok(()) => {
                self.screens.pop();
            }
            Err(e) => {
                if let Some(Screen::Slots { error, .. }) = self.screens.last_mut() {
                    *error = Some(e);
                }
            }
        }
    }

    fn screen_input(&mut self, screen: &mut Screen, input: &[InputEvent]) -> Vec<Action> {
        let mut actions = Vec::new();
        let back = |event: &InputEvent| {
            matches!(
                event,
                InputEvent::KeyDown(Key::Escape)
                    | InputEvent::MouseDown {
                        button: MouseButton::Right,
                        ..
                    }
            )
        };
        match screen {
            Screen::Extras(ex) => return self.extras_input(ex, input),
            Screen::Viewer(v) => return self.viewer_input(v, input),
            Screen::Library(lib) => return self.library_input(lib, input),
            Screen::Picker(p) => return self.picker_input(p, input),
            Screen::MainMenu { focus } => {
                let rects = menu_rects();
                for event in input {
                    let picked = list_input(event, &rects, focus);
                    if let Some(i) = picked.filter(|&i| self.menu_enabled(i)) {
                        actions.push(match MENU_ITEMS[i] {
                            "menu.start" => Action::Start(Start::NewGame),
                            "menu.load" => Action::Push(self.slots_screen(false, false)),
                            "menu.extras" => {
                                Action::Push(self.extras_screen(false, extras::GALLERY))
                            }
                            "menu.settings" => Action::Push(Screen::Settings {
                                tab: 0,
                                focus: None,
                            }),
                            "menu.library" => Action::ChangeGame,
                            "menu.quit" => Action::Quit,
                            _ => continue,
                        });
                    }
                }
            }
            Screen::Pause { focus } => {
                let rects = pause_rects();
                for event in input {
                    if back(event) {
                        actions.push(Action::Close);
                        continue;
                    }
                    let picked = list_input(event, &rects, focus);
                    if let Some(i) = picked.filter(|&i| self.pause_enabled(i)) {
                        let action = match PAUSE_ITEMS[i] {
                            "pause.resume" => Action::Close,
                            "pause.save" => Action::Push(self.slots_screen(true, false)),
                            "pause.load" => Action::Push(self.slots_screen(false, false)),
                            "pause.history" => match self.history_screen() {
                                Some(screen) => Action::Push(screen),
                                None => continue,
                            },
                            "pause.settings" => Action::Push(Screen::Settings {
                                tab: 0,
                                focus: None,
                            }),
                            "pause.menu" => Action::ToMenu,
                            "pause.auto" => {
                                self.set_auto(!self.auto);
                                Action::Close
                            }
                            _ => Action::Quit,
                        };
                        actions.push(action);
                    }
                }
            }
            Screen::Slots {
                saving,
                list,
                focus,
                ..
            } => {
                let rects: Vec<Rect> = (0..SLOTS).map(slot_rect).collect();
                for event in input {
                    if back(event) {
                        actions.push(Action::Close);
                        continue;
                    }
                    if let Some(i) = list_input(event, &rects, focus) {
                        if *saving {
                            actions.push(Action::Save(i));
                        } else if list[i].is_some() {
                            actions.push(Action::Load(i));
                        }
                    }
                }
            }
            Screen::History {
                entries,
                rows,
                scroll,
                row,
                ..
            } => {
                let (row_h, last) = (*row, history_last_page(rows, *row));
                for event in input {
                    match *event {
                        _ if back(event) => actions.push(Action::Close),
                        InputEvent::Wheel(n) if n > 0 => *scroll = scroll.saturating_sub(3),
                        InputEvent::KeyDown(Key::Up) => *scroll = scroll.saturating_sub(1),
                        InputEvent::KeyDown(Key::PageUp) => *scroll = scroll.saturating_sub(10),
                        InputEvent::Wheel(_) | InputEvent::KeyDown(Key::Down | Key::PageDown) => {
                            if *scroll >= last {
                                actions.push(Action::Close);
                            } else {
                                let step = match event {
                                    InputEvent::KeyDown(Key::Down) => 1,
                                    InputEvent::KeyDown(Key::PageDown) => 10,
                                    _ => 3,
                                };
                                *scroll = (*scroll + step).min(last);
                            }
                        }
                        InputEvent::MouseDown {
                            button: MouseButton::Left,
                            x,
                            y,
                        } => {
                            let hit = history_layout(rows, *scroll, row_h)
                                .find(|(_, top, row)| {
                                    (top..&(top + row_h)).contains(&&y)
                                        && x >= HISTORY_PANEL.x
                                        && x < HISTORY_PANEL.right()
                                        && entries[row.entry].voice.is_some()
                                })
                                .map(|(_, _, row)| row.entry);
                            if let Some(voice) = hit.and_then(|i| entries[i].voice.clone()) {
                                self.session.play_voice(&voice);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Screen::Settings { tab, focus } => {
                let mut settings = self.settings.clone();
                for event in input {
                    if back(event) {
                        actions.push(Action::Close);
                        continue;
                    }
                    let rows = Setting::tab(*tab);
                    match *event {
                        InputEvent::MouseMove { x, y } => {
                            if let Some(i) =
                                (0..rows.len()).find(|&i| setting_row(i).contains((x, y)))
                            {
                                *focus = Some(i);
                            }
                        }
                        InputEvent::MouseDown {
                            button: MouseButton::Left,
                            x,
                            y,
                        } => {
                            if let Some(t) = (0..TABS.len()).find(|&t| tab_rect(t).contains((x, y)))
                            {
                                *tab = t;
                                *focus = None;
                            } else if let Some(i) =
                                (0..rows.len()).find(|&i| setting_row(i).contains((x, y)))
                            {
                                *focus = Some(i);
                                let setting = rows[i];
                                let bar = level_rect(i);
                                let max = setting.max();
                                if let Some(level) = setting.level(&mut settings) {
                                    if (bar.x..bar.right() + 8).contains(&x) {
                                        *level = ui::level_at(bar, max, x);
                                    }
                                } else {
                                    let back_half = x < bar.x + bar.w as i32 / 3;
                                    setting.cycle(&mut settings, !back_half);
                                }
                            }
                        }
                        InputEvent::KeyDown(Key::Up) => {
                            *focus = Some(focus.map_or(0, |i| (i + rows.len() - 1) % rows.len()));
                        }
                        InputEvent::KeyDown(Key::Down) => {
                            *focus = Some(focus.map_or(0, |i| (i + 1) % rows.len()));
                        }
                        InputEvent::KeyDown(Key::PageUp) => {
                            *tab = (*tab + TABS.len() - 1) % TABS.len();
                            *focus = None;
                        }
                        InputEvent::KeyDown(Key::PageDown) => {
                            *tab = (*tab + 1) % TABS.len();
                            *focus = None;
                        }
                        InputEvent::KeyDown(key @ (Key::Left | Key::Right | Key::Enter)) => {
                            if let Some(&setting) = focus.and_then(|i| rows.get(i)) {
                                setting.cycle(&mut settings, key != Key::Left);
                            }
                        }
                        _ => {}
                    }
                }
                if settings != self.settings {
                    self.set_settings(settings);
                }
            }
        }
        actions
    }

    fn menu_enabled(&self, i: usize) -> bool {
        match MENU_ITEMS[i] {
            "menu.load" => self.slots.is_some(),
            "menu.extras" => self.extras,
            "menu.library" => self.launcher.is_some(),
            _ => true,
        }
    }

    fn pause_enabled(&self, i: usize) -> bool {
        match PAUSE_ITEMS[i] {
            "pause.save" => self.can_save(),
            "pause.load" => self.slots.is_some(),
            "pause.history" => self.session.can_save(),
            _ => true,
        }
    }

    /// Turns auto mode on or off.
    fn set_auto(&mut self, auto: bool) {
        self.auto = auto;
        self.session.set_auto(auto);
    }

    /// Whether auto mode is on.
    pub fn auto(&self) -> bool {
        self.auto
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear(theme::BACKDROP);
        if self.in_game {
            canvas.clear(Color::BLACK);
            let v = self.viewport();
            canvas.draw_scaled(&self.game, v.x, v.y, v.w, v.h);
            if self.auto {
                let text = self.strings.get("game.auto").to_string();
                let badge = Rect::new(v.right() - 96, v.y + 8, 84, 30);
                canvas.fill_rect(
                    badge.x,
                    badge.y,
                    badge.w,
                    badge.h,
                    Color::rgba(0, 0, 0, 150),
                );
                ui::label_centered(canvas, &mut self.font, badge, &text, 18.0, theme::ACCENT);
            }
            if !self.screens.is_empty() {
                canvas.fill_rect(0, 0, ui::WIDTH, ui::HEIGHT, theme::DIM);
            }
        }
        let screens = std::mem::take(&mut self.screens);
        for screen in &screens {
            self.draw_screen(canvas, screen);
        }
        self.screens = screens;
    }

    fn draw_screen(&mut self, canvas: &mut Canvas, screen: &Screen) {
        let mouse = self.mouse;
        match screen {
            Screen::Extras(ex) => self.draw_extras(canvas, ex),
            Screen::Viewer(v) => self.draw_viewer(canvas, v),
            Screen::Library(lib) => self.draw_library(canvas, lib),
            Screen::Picker(p) => self.draw_picker(canvas, p),
            Screen::MainMenu { focus } => {
                let title = self.session.title();
                let f = &mut self.font;
                ui::label_centered(
                    canvas,
                    f,
                    Rect::new(0, 110, ui::WIDTH, 60),
                    &title,
                    48.0,
                    theme::TEXT,
                );
                canvas.fill_rect(440, 190, 400, 1, theme::BORDER);
                for (i, r) in menu_rects().into_iter().enumerate() {
                    let enabled = self.menu_enabled(i);
                    let state = State::of(*focus == Some(i), enabled);
                    let text = self.strings.get(MENU_ITEMS[i]).to_string();
                    ui::button(canvas, &mut self.font, r, &text, state);
                }
                ui::label(
                    canvas,
                    &mut self.font,
                    16,
                    ui::HEIGHT as i32 - 30,
                    concat!("VNRevival ", env!("CARGO_PKG_VERSION")),
                    ui::SMALL_SIZE,
                    theme::MUTED,
                );
            }
            Screen::Pause { focus } => {
                let rects = pause_rects();
                let panel = Rect::new(
                    rects[0].x - 30,
                    rects[0].y - 80,
                    rects[0].w + 60,
                    (rects[rects.len() - 1].bottom() - rects[0].y + 110) as u32,
                );
                ui::panel(canvas, panel);
                let title = self.strings.get("pause.title").to_string();
                ui::heading(canvas, &mut self.font, panel, &title);
                for (i, r) in rects.into_iter().enumerate() {
                    let state = State::of(*focus == Some(i), self.pause_enabled(i));
                    let text = self.strings.get(PAUSE_ITEMS[i]).to_string();
                    ui::button(canvas, &mut self.font, r, &text, state);
                }
            }
            Screen::Slots {
                saving,
                list,
                focus,
                error,
                ..
            } => {
                let panel = Rect::new(60, 30, 1160, 660);
                ui::panel(canvas, panel);
                let title = self
                    .strings
                    .get(if *saving { "slots.save" } else { "slots.load" })
                    .to_string();
                ui::heading(canvas, &mut self.font, panel, &title);
                let empty = self.strings.get("slots.empty").to_string();
                for (i, slot) in list.iter().enumerate() {
                    let r = slot_rect(i);
                    let usable = *saving || slot.is_some();
                    ui::item(canvas, r, State::of(*focus == Some(i), usable));
                    let f = &mut self.font;
                    ui::label(
                        canvas,
                        f,
                        r.x + 16,
                        r.y + 12,
                        &format!("{:02}", i + 1),
                        ui::TITLE_SIZE,
                        theme::ACCENT,
                    );
                    match slot {
                        Some(s) => {
                            let head = format!("{}   {}", s.saved_at, s.chapter);
                            let head = ui::fit(f, &head, ui::TEXT_SIZE, r.w as f32 - 90.0);
                            ui::label(
                                canvas,
                                f,
                                r.x + 72,
                                r.y + 12,
                                &head,
                                ui::TEXT_SIZE,
                                theme::TEXT,
                            );
                            let text = ui::fit(f, &s.excerpt, ui::SMALL_SIZE, r.w as f32 - 90.0);
                            ui::label(
                                canvas,
                                f,
                                r.x + 72,
                                r.y + 48,
                                &text,
                                ui::SMALL_SIZE,
                                theme::MUTED,
                            );
                        }
                        None => {
                            ui::label(
                                canvas,
                                f,
                                r.x + 72,
                                r.y + 30,
                                &empty,
                                ui::TEXT_SIZE,
                                theme::DISABLED,
                            );
                        }
                    }
                }
                if let Some(error) = error {
                    ui::label(
                        canvas,
                        &mut self.font,
                        panel.x + 28,
                        panel.bottom() - 36,
                        error,
                        ui::SMALL_SIZE,
                        theme::ERROR,
                    );
                }
            }
            Screen::History {
                entries,
                rows,
                scroll,
                size,
                row,
            } => {
                let (size, row_h) = (*size, *row);
                ui::panel(canvas, HISTORY_PANEL);
                let title = self.strings.get("history.title").to_string();
                ui::heading(canvas, &mut self.font, HISTORY_PANEL, &title);
                let hover = history_layout(rows, *scroll, row_h)
                    .find(|(_, top, _)| (*top..top + row_h).contains(&mouse.1))
                    .map(|(_, _, row)| row.entry)
                    .filter(|&e| entries[e].voice.is_some() && HISTORY_PANEL.contains(mouse));
                for (_, top, row) in history_layout(rows, *scroll, row_h) {
                    let color = if row.speaker {
                        theme::ACCENT
                    } else if hover == Some(row.entry) {
                        Color::rgb(255, 230, 150)
                    } else {
                        theme::TEXT
                    };
                    let x = HISTORY_PANEL.x + if row.speaker { 40 } else { 60 };
                    if row.first && entries[row.entry].voice.is_some() {
                        // A mark before lines with a voice.
                        let mark = top + (size * 0.55) as i32 - 4;
                        canvas.fill_rect(HISTORY_PANEL.x + 40, mark, 8, 8, theme::ACCENT);
                    }
                    ui::label(canvas, self.text_font(), x, top, &row.text, size, color);
                }
                let hint = self.strings.get("history.hint").to_string();
                ui::label(
                    canvas,
                    &mut self.font,
                    HISTORY_PANEL.x + 28,
                    HISTORY_PANEL.bottom() - 36,
                    &hint,
                    ui::SMALL_SIZE,
                    theme::MUTED,
                );
            }
            Screen::Settings { tab, focus } => {
                let panel = Rect::new(190, 60, 900, 600);
                ui::panel(canvas, panel);
                let title = self.strings.get("settings.title").to_string();
                ui::heading(canvas, &mut self.font, panel, &title);
                for (t, key) in TABS.iter().enumerate() {
                    let state = if t == *tab {
                        State::Active
                    } else {
                        State::of(tab_rect(t).contains(mouse), true)
                    };
                    let text = self.strings.get(key).to_string();
                    ui::button(canvas, &mut self.font, tab_rect(t), &text, state);
                }
                for (i, &setting) in Setting::tab(*tab).iter().enumerate() {
                    let row = setting_row(i);
                    ui::item(canvas, row, State::of(*focus == Some(i), true));
                    let name = self.strings.get(setting.key()).to_string();
                    ui::label(
                        canvas,
                        &mut self.font,
                        row.x + 20,
                        row.y + 14,
                        &name,
                        ui::TEXT_SIZE,
                        theme::TEXT,
                    );
                    let control = setting_control(i);
                    let mut settings = self.settings.clone();
                    if let Some(&mut level) = setting.level(&mut settings) {
                        let bar = level_rect(i);
                        ui::level_bar(canvas, bar, level, setting.max());
                        let value = match setting {
                            Setting::TextSpeed if level == MAX_LEVEL => {
                                self.strings.get("common.instant").to_string()
                            }
                            Setting::TextSize => format!("{}", self.settings.text_px()),
                            _ => level.to_string(),
                        };
                        ui::label(
                            canvas,
                            &mut self.font,
                            bar.right() + 16,
                            row.y + 14,
                            &value,
                            ui::TEXT_SIZE,
                            theme::ACCENT,
                        );
                    } else {
                        let value = match setting {
                            Setting::Language => self.settings.language.name().to_string(),
                            Setting::Start => self
                                .strings
                                .get(match self.settings.start {
                                    StartScreen::Universal => "settings.start.universal",
                                    StartScreen::Original => "settings.start.original",
                                })
                                .to_string(),
                            Setting::Font | Setting::GameFont => {
                                let font = if setting == Setting::Font {
                                    self.settings.font
                                } else {
                                    self.settings.game_font
                                };
                                match font {
                                    FontChoice::Original => {
                                        self.strings.get("settings.font.original").to_string()
                                    }
                                    font => font.name().to_string(),
                                }
                            }
                            Setting::Outline => self
                                .strings
                                .get(match self.settings.outline {
                                    Outline::Off => "settings.outline.off",
                                    Outline::Shadow => "settings.outline.shadow",
                                    Outline::Outline => "settings.outline.outline",
                                })
                                .to_string(),
                            other => match other.toggle(&mut settings) {
                                Some(&mut on) => self
                                    .strings
                                    .get(if on { "common.on" } else { "common.off" })
                                    .to_string(),
                                None => String::new(),
                            },
                        };
                        let f = &mut self.font;
                        ui::label(
                            canvas,
                            f,
                            control.x,
                            row.y + 14,
                            "◀",
                            ui::TEXT_SIZE,
                            theme::MUTED,
                        );
                        ui::label_centered(
                            canvas,
                            f,
                            control,
                            &value,
                            ui::TEXT_SIZE,
                            theme::ACCENT,
                        );
                        ui::label(
                            canvas,
                            f,
                            control.right() - 24,
                            row.y + 14,
                            "▶",
                            ui::TEXT_SIZE,
                            theme::MUTED,
                        );
                    }
                }
                if *tab == TEXT_TAB {
                    self.draw_text_preview(canvas);
                }
            }
        }
    }

    /// The font of the game text.
    fn text_font(&mut self) -> &mut Font {
        match &mut self.game_font {
            Some(font) => font,
            None => &mut self.font,
        }
    }

    /// A sample message as the game text settings draw it.
    fn draw_text_preview(&mut self, canvas: &mut Canvas) {
        let rows = Setting::tab(TEXT_TAB).len();
        let top = setting_row(rows - 1).bottom() + 10;
        let area = Rect::new(220, top, 840, (660 - top - 12).max(20) as u32);
        let alpha = self.settings.window_alpha();
        let back = if self.settings.high_contrast {
            Color::BLACK
        } else {
            Color::rgba(20, 30, 90, alpha.max(40))
        };
        canvas.fill_rect(area.x, area.y, area.w, area.h, back);
        // Game text is drawn at 800 × 600 and shown scaled by 1.2.
        let size = self.settings.text_px() * 1.2;
        let shadow = (self.settings.outline != Outline::Off).then_some(Color::BLACK);
        let style = TextStyle {
            shadow,
            outline: self.settings.outline == Outline::Outline,
            ..TextStyle::plain(size, Color::WHITE)
        };
        let sample = self.strings.get("settings.preview").to_string();
        let line = self.settings.line_height(size);
        let mut y = area.y as f32 + 6.0;
        for text in self
            .text_font()
            .wrap(&sample, size, area.w as f32 - 32.0)
            .iter()
            .take(2)
        {
            if y + line > area.bottom() as f32 + 4.0 {
                break;
            }
            self.text_font()
                .draw_line(canvas, area.x as f32 + 16.0, y, text, style);
            y += line;
        }
    }
}

/// Moves the focus of a list of items with the mouse or the keyboard;
/// returns the item picked by a click, Enter or Space.
fn list_input(event: &InputEvent, rects: &[Rect], focus: &mut Option<usize>) -> Option<usize> {
    let n = rects.len();
    match *event {
        InputEvent::MouseMove { x, y } => {
            *focus = rects.iter().position(|r| r.contains((x, y)));
            None
        }
        InputEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        } => {
            let hit = rects.iter().position(|r| r.contains((x, y)));
            if hit.is_some() {
                *focus = hit;
            }
            hit
        }
        InputEvent::KeyDown(Key::Up) if n > 0 => {
            *focus = Some(focus.map_or(n - 1, |i| (i + n - 1) % n));
            None
        }
        InputEvent::KeyDown(Key::Down) if n > 0 => {
            *focus = Some(focus.map_or(0, |i| (i + 1) % n));
            None
        }
        InputEvent::KeyDown(Key::Enter | Key::Space) => *focus,
        _ => None,
    }
}

fn menu_rects() -> Vec<Rect> {
    (0..MENU_ITEMS.len())
        .map(|i| Rect::new(490, 240 + i as i32 * 64, 300, 50))
        .collect()
}

fn pause_rects() -> Vec<Rect> {
    let h = PAUSE_ITEMS.len() as i32 * 60;
    let top = (ui::HEIGHT as i32 - h) / 2 + 30;
    (0..PAUSE_ITEMS.len())
        .map(|i| Rect::new(500, top + i as i32 * 60, 280, 48))
        .collect()
}

fn slot_rect(i: usize) -> Rect {
    let col = (i / 5) as i32;
    let row = (i % 5) as i32;
    Rect::new(90 + col * 560, 110 + row * 112, 540, 100)
}

fn tab_rect(t: usize) -> Rect {
    Rect::new(220 + t as i32 * 210, 140, 200, 46)
}

/// The settings tab with the text preview.
const TEXT_TAB: usize = 2;

fn setting_row(i: usize) -> Rect {
    Rect::new(220, 204 + i as i32 * 62, 840, 54)
}

fn setting_control(i: usize) -> Rect {
    let row = setting_row(i);
    Rect::new(row.x + 400, row.y + 8, 420, 42)
}

/// The level bar of a setting row.
fn level_rect(i: usize) -> Rect {
    let c = setting_control(i);
    Rect::new(c.x, c.y + 8, c.w - 110, c.h - 16)
}

/// Rows of the history from `scroll` that fit, with their top `y`.
fn history_layout(
    rows: &[HistoryRow],
    scroll: usize,
    height: i32,
) -> impl Iterator<Item = (usize, i32, &HistoryRow)> {
    let mut y = HISTORY_TOP;
    rows.iter()
        .enumerate()
        .skip(scroll)
        .map(move |(i, row)| {
            let top = y + if i == scroll { 0 } else { row.gap };
            y = top + height;
            (i, top, row)
        })
        .take_while(move |(_, top, _)| top + height <= HISTORY_BOTTOM)
}

/// The scroll position that shows the last rows.
fn history_last_page(rows: &[HistoryRow], row_height: i32) -> usize {
    let mut height = 0;
    let mut first = rows.len();
    for i in (0..rows.len()).rev() {
        let h = row_height
            + if first == rows.len() {
                0
            } else {
                rows[i + 1].gap
            };
        if height + h > HISTORY_BOTTOM - HISTORY_TOP {
            break;
        }
        height += h;
        first = i;
    }
    first.min(rows.len().saturating_sub(1))
}

impl App for Shell {
    fn title(&self) -> String {
        self.session.title()
    }

    fn resolution(&self) -> (u32, u32) {
        (ui::WIDTH, ui::HEIGHT)
    }

    fn update(&mut self, frame: &mut Frame<'_>) -> Flow {
        if self.mixer.is_none() {
            self.mixer = Some(frame.mixer.clone());
            self.apply_settings();
        }
        if !self.started {
            self.started = true;
            match self.settings.start {
                StartScreen::Original => self.start(Start::OriginalTitle),
                StartScreen::Universal => self.screens.push(Screen::MainMenu { focus: None }),
            }
        }
        for event in frame.input {
            match *event {
                InputEvent::MouseMove { x, y }
                | InputEvent::MouseDown { x, y, .. }
                | InputEvent::MouseUp { x, y, .. } => self.mouse = (x, y),
                _ => {}
            }
        }
        if let Some(mut screen) = self.screens.pop() {
            let depth = self.screens.len();
            let actions = self.screen_input(&mut screen, frame.input);
            // A screen can be replaced while handling its input (a load
            // that clears everything); keep it only if the stack is as it
            // was.
            if self.screens.len() == depth {
                self.screens.push(screen);
            }
            for action in actions {
                self.act(action);
            }
        } else if self.in_game && !self.game_shortcuts(frame.input) {
            self.run_game(frame.input, frame.mixer);
        }
        let frozen = self.in_game && !self.screens.is_empty();
        if frozen != self.frozen {
            self.frozen = frozen;
            self.session.freeze(frozen);
        }
        self.draw(frame.canvas);
        if self.quit {
            self.session.silence();
            Flow::Quit
        } else {
            Flow::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SaveData;

    use std::cell::RefCell;
    use std::rc::Rc;

    /// What the test game saw.
    #[derive(Default)]
    struct Log {
        frames: u64,
        clicks: Vec<(i32, i32)>,
        started: Vec<Start>,
        loaded: Option<SaveData>,
        dismissed: bool,
        /// Every `freeze` call.
        frozen: Vec<bool>,
        /// Last `set_auto`.
        auto: Option<bool>,
        /// What the next frame returns.
        next: Option<SessionEvent>,
    }

    /// A game that records what it is told.
    #[derive(Default, Clone)]
    struct Counter(Rc<RefCell<Log>>);

    impl Session for Counter {
        fn title(&self) -> String {
            "Counter".into()
        }

        fn resolution(&self) -> (u32, u32) {
            (800, 600)
        }

        fn start(&mut self, start: Start) -> Result<(), String> {
            self.0.borrow_mut().started.push(start);
            Ok(())
        }

        fn update(&mut self, frame: &mut Frame<'_>) -> SessionEvent {
            let mut log = self.0.borrow_mut();
            log.frames += 1;
            for event in frame.input {
                if let InputEvent::MouseDown { x, y, .. } = *event {
                    log.clicks.push((x, y));
                }
            }
            frame.canvas.clear(Color::rgb(200, 0, 0));
            log.next.take().unwrap_or(SessionEvent::Continue)
        }

        fn can_save(&self) -> bool {
            true
        }

        fn save(&self) -> Result<SaveData, String> {
            Ok(SaveData {
                payload: serde_json::json!({ "frames": self.0.borrow().frames }),
                chapter: "One".into(),
                excerpt: "Hello".into(),
            })
        }

        fn load(&mut self, data: &SaveData) -> Result<(), String> {
            self.0.borrow_mut().loaded = Some(data.clone());
            Ok(())
        }

        fn dismissed(&mut self) {
            self.0.borrow_mut().dismissed = true;
        }

        fn freeze(&mut self, frozen: bool) {
            self.0.borrow_mut().frozen.push(frozen);
        }

        fn set_auto(&mut self, auto: bool) {
            self.0.borrow_mut().auto = Some(auto);
        }

        fn history(&self) -> Vec<HistoryEntry> {
            vec![HistoryEntry {
                speaker: Some("A".into()),
                text: "line".into(),
                voice: None,
            }]
        }

        fn gallery(&mut self) -> Vec<crate::session::GalleryEntry> {
            vec![
                crate::session::GalleryEntry {
                    variants: vec![false, true],
                },
                crate::session::GalleryEntry {
                    variants: vec![false],
                },
            ]
        }

        fn scenes(&mut self) -> Vec<crate::session::ReplayScene> {
            vec![crate::session::ReplayScene {
                id: "s1".into(),
                title: "01".into(),
                group: "A".into(),
                unlocked: true,
            }]
        }
    }

    fn font() -> Option<Font> {
        load_font(None).ok()
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vnr-shell-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn click(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        }
    }

    fn step(shell: &mut Shell, mixer: &Mixer, canvas: &mut Canvas, input: &[InputEvent]) -> Flow {
        let mut frame = Frame {
            number: 0,
            input,
            canvas,
            mixer,
        };
        shell.update(&mut frame)
    }

    #[test]
    fn letterboxes_and_maps_the_mouse() {
        let Some(font) = font() else {
            eprintln!("no font, skipping");
            return;
        };
        let shell = Shell::new(Box::new(Counter::default()), font, ShellOptions::default());
        assert_eq!(shell.viewport(), Rect::new(160, 0, 960, 720));
        assert_eq!(shell.to_game((160, 0)), (0, 0));
        assert_eq!(shell.to_game((640, 360)), (400, 300));
        assert_eq!(shell.to_game((1119, 719)), (799, 599));
    }

    #[test]
    fn menu_game_pause_and_saves() {
        let Some(font) = font() else {
            eprintln!("no font, skipping");
            return;
        };
        let dir = scratch("flow");
        let options = ShellOptions {
            data_dir: Some(dir.join("data")),
            saves: Some(dir.join("saves")),
            font_file: None,
        };
        let game = Counter::default();
        let log = game.0.clone();
        let mut shell = Shell::new(Box::new(game), font, options);
        let mixer = Mixer::new(1);
        let mut canvas = Canvas::new(ui::WIDTH, ui::HEIGHT);

        // The main menu comes first; the game does not run under it.
        step(&mut shell, &mixer, &mut canvas, &[]);
        assert!(!shell.in_game());
        assert_eq!(log.borrow().frames, 0);
        let start = menu_rects()[0];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(start.x + 5, start.y + 5)],
        );
        assert!(shell.in_game());
        assert_eq!(log.borrow().started, [Start::NewGame]);

        // Clicks reach the game in its own pixels; the game is scaled.
        step(&mut shell, &mixer, &mut canvas, &[click(640, 360)]);
        assert_eq!(canvas.pixel(640, 360), Color::rgb(200, 0, 0));
        assert_eq!(canvas.pixel(10, 360), Color::BLACK);
        assert_eq!(log.borrow().clicks, [(400, 300)]);

        // A toggles auto mode, shown over the game.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Char('a'))],
        );
        assert!(shell.auto());
        assert_eq!(log.borrow().auto, Some(true));

        // Escape pauses: the game stops.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );
        assert!(!shell.in_game());
        assert_eq!(log.borrow().frozen, [true]);
        let frames = log.borrow().frames;
        step(&mut shell, &mixer, &mut canvas, &[]);
        assert_eq!(log.borrow().frames, frames);
        let save = pause_rects()[1];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(save.x + 5, save.y + 5)],
        );
        let slot = slot_rect(2);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(slot.x + 5, slot.y + 5)],
        );
        // Back at the pause menu after saving.
        assert!(matches!(shell.screens.last(), Some(Screen::Pause { .. })));
        let saved = Slots::new(dir.join("saves")).read(2).unwrap().unwrap();
        assert_eq!(saved.chapter, "One");

        // Load it from the pause menu: back in the game.
        let load = pause_rects()[2];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(load.x + 5, load.y + 5)],
        );
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(slot.x + 5, slot.y + 5)],
        );
        assert!(shell.in_game());
        assert_eq!(log.borrow().loaded.as_ref().unwrap().chapter, "One");
        assert_eq!(log.borrow().frozen, [true, false]);

        // The game's own load request: closing it tells the game.
        log.borrow_mut().next = Some(SessionEvent::OpenLoad);
        step(&mut shell, &mixer, &mut canvas, &[]);
        assert!(matches!(
            shell.screens.last(),
            Some(Screen::Slots {
                requested: true,
                ..
            })
        ));
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );
        assert!(log.borrow().dismissed);
        assert!(shell.in_game());

        // Settings persist.
        let mut settings = shell.settings().clone();
        settings.music = 1;
        shell.set_settings(settings);
        assert_eq!(Settings::load(&dir.join("data/settings.json")).music, 1);

        // Main menu from the pause menu, then quit.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );
        let menu = pause_rects()[6];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(menu.x + 5, menu.y + 5)],
        );
        assert!(!shell.in_game());
        assert!(matches!(
            shell.screens.last(),
            Some(Screen::MainMenu { .. })
        ));
        // Leaving the game ends auto mode.
        assert!(!shell.auto());
        assert_eq!(log.borrow().auto, Some(false));
        let quit = menu_rects()[5];
        let flow = step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(quit.x + 5, quit.y + 5)],
        );
        assert_eq!(flow, Flow::Quit);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Finds a game in folders named `game`; opens a [`Counter`].
    struct FakeLauncher(Counter);

    impl Launcher for FakeLauncher {
        fn detect(&self, dir: &Path) -> Result<Option<GameInfo>, String> {
            Ok(
                (dir.file_name() == Some("game".as_ref())).then(|| GameInfo {
                    engine: "test",
                    title: "Counter".into(),
                    root: dir.to_path_buf(),
                    details: Vec::new(),
                }),
            )
        }

        fn open(&self, _game: &GameInfo) -> Result<Box<dyn Session>, String> {
            Ok(Box::new(self.0.clone()))
        }
    }

    #[test]
    fn library_adds_opens_and_changes_games() {
        let Some(font) = font() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("vnr-launcher-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let games = dir.join("games");
        std::fs::create_dir_all(games.join("game")).unwrap();
        std::fs::create_dir_all(games.join("other")).unwrap();
        // The picker opens where it was last.
        Library::open(Some(dir.join("data"))).set_last_folder(&games);

        let game = Counter::default();
        let log = game.0.clone();
        let options = ShellOptions {
            data_dir: Some(dir.join("data")),
            saves: None,
            font_file: None,
        };
        let mut shell = Shell::launcher(Box::new(FakeLauncher(game)), font, options);
        let mixer = Mixer::new(1);
        let mut canvas = Canvas::new(ui::WIDTH, ui::HEIGHT);
        step(&mut shell, &mixer, &mut canvas, &[]);
        assert!(matches!(shell.screens.as_slice(), [Screen::Library(_)]));
        assert_eq!(shell.session().title(), "VNRevival");

        // Add a game: the picker lists `..`, `game`, `other`.
        let add = library::add_rect();
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(add.x + 5, add.y + 5)],
        );
        let Some(Screen::Picker(_)) = shell.screens.last() else {
            panic!("no picker");
        };
        let use_button = library::use_rect();
        // Nothing to use in the parent folder.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(use_button.x + 5, use_button.y + 5)],
        );
        assert!(matches!(shell.screens.last(), Some(Screen::Picker(_))));
        let row = library::picker_row(1);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(row.x + 5, row.y + 5)],
        );
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(use_button.x + 5, use_button.y + 5)],
        );
        assert!(matches!(shell.screens.as_slice(), [Screen::Library(_)]));
        assert_eq!(shell.library.games().len(), 1);

        // Open it: the universal menu of that game.
        let card = library::card_rect(0);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(card.x + 40, card.y + 60)],
        );
        assert!(matches!(
            shell.screens.as_slice(),
            [Screen::MainMenu { .. }]
        ));
        assert_eq!(shell.session().title(), "Counter");
        assert!(shell.library.games()[0].last_played.is_some());
        let start = menu_rects()[0];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(start.x + 5, start.y + 5)],
        );
        assert!(shell.in_game());
        assert_eq!(log.borrow().started, [Start::NewGame]);

        // Back to the main menu, then "Change game": the library again.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );
        let menu = pause_rects()[6];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(menu.x + 5, menu.y + 5)],
        );
        let change = menu_rects()[4];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(change.x + 5, change.y + 5)],
        );
        assert!(matches!(shell.screens.as_slice(), [Screen::Library(_)]));
        assert_eq!(shell.session().title(), "VNRevival");

        // With the game's own title at start, opening shows the game
        // itself, with nothing over it.
        let mut settings = shell.settings().clone();
        settings.start = StartScreen::Original;
        shell.set_settings(settings);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(card.x + 40, card.y + 60)],
        );
        assert!(
            shell.in_game(),
            "{} screens over the game",
            shell.screens.len()
        );
        assert_eq!(log.borrow().started.last(), Some(&Start::OriginalTitle));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interface_and_game_fonts_are_separate() {
        let mut s = Settings::default();
        Setting::GameFont.cycle(&mut s, false);
        // Backwards from the first installed font: the last one, Original.
        assert_eq!(s.game_font, FontChoice::Original);
        assert_eq!(s.font, FontChoice::NotoSansJp);
        Setting::Font.cycle(&mut s, false);
        assert_eq!(s.font, FontChoice::Original);
        assert!(Setting::tab(0).contains(&Setting::Font));
        assert!(Setting::tab(TEXT_TAB).contains(&Setting::GameFont));
    }

    #[test]
    fn extras_open_only_what_was_seen() {
        let Some(font) = font() else {
            eprintln!("no font, skipping");
            return;
        };
        let game = Counter::default();
        let log = game.0.clone();
        let mut shell = Shell::new(Box::new(game), font, ShellOptions::default());
        let mixer = Mixer::new(1);
        let mut canvas = Canvas::new(ui::WIDTH, ui::HEIGHT);
        step(&mut shell, &mixer, &mut canvas, &[]);
        let extras = menu_rects()[2];
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(extras.x + 5, extras.y + 5)],
        );
        let Some(Screen::Extras(ex)) = shell.screens.last() else {
            panic!("the extras did not open");
        };
        let items = ex.items();
        let (seen, locked) = (items[0].1, items[1].1);

        // A locked picture does nothing; a seen one opens on its seen
        // variant only.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(locked.x + 5, locked.y + 5)],
        );
        assert!(matches!(shell.screens.last(), Some(Screen::Extras(_))));
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(seen.x + 5, seen.y + 5)],
        );
        let Some(Screen::Viewer(v)) = shell.screens.last() else {
            panic!("the viewer did not open");
        };
        assert_eq!(v.variants(), [1]);
        step(&mut shell, &mixer, &mut canvas, &[click(5, 5)]);
        assert!(matches!(shell.screens.last(), Some(Screen::Extras(_))));

        // "Unlock all" opens everything.
        let mut settings = shell.settings().clone();
        settings.unlock_all = true;
        shell.set_settings(settings);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(locked.x + 5, locked.y + 5)],
        );
        assert!(matches!(shell.screens.last(), Some(Screen::Viewer(_))));
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );

        // A scene plays, and its end goes back to the scene list.
        let scenes = tab_rect(extras::SCENES);
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(scenes.x + 5, scenes.y + 5)],
        );
        let Some(Screen::Extras(ex)) = shell.screens.last() else {
            panic!("the extras closed");
        };
        let scene = ex.items()[0].1;
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[click(scene.x + 5, scene.y + 5)],
        );
        assert!(shell.in_game());
        assert_eq!(log.borrow().started, [Start::Scene("s1".into())]);
        assert!(!shell.can_save());
        log.borrow_mut().next = Some(SessionEvent::ToMenu);
        step(&mut shell, &mixer, &mut canvas, &[]);
        assert!(!shell.in_game());
        let Some(Screen::Extras(ex)) = shell.screens.last() else {
            panic!("not back at the scene list");
        };
        assert_eq!(ex.tab(), extras::SCENES);
    }
}
