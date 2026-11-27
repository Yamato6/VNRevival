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

pub use extras::MUSIC_CHANNEL;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::{App, Flow, Frame, InputEvent, Key, MouseButton};
use crate::audio::Mixer;
use crate::gfx::{Canvas, Color, Sprite};
use crate::lang::{Language, Strings};
use crate::saves::{SLOTS, SlotSummary, Slots};
use crate::session::{HistoryEntry, Session, SessionEvent, Start};
use crate::settings::{MAX_LEVEL, Settings, StartScreen};
use crate::text::{Font, FontFamily, find_system_font};
use crate::ui::{self, Rect, State, theme};

/// Where the shell keeps its files.
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    /// Folder of `settings.json` and `lang/`; `None` keeps settings in
    /// memory only.
    pub data_dir: Option<PathBuf>,
    /// The game's save folder; `None` disables saving.
    pub saves: Option<PathBuf>,
}

/// A change of the window the shell asks the frontend for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRequest {
    pub fullscreen: bool,
}

/// Loads the interface font: `path` if given, else an installed
/// Japanese gothic font (game texts in the history are Japanese).
pub fn load_font(path: Option<&Path>) -> Result<Font, String> {
    let (path, index) = match path {
        Some(path) => (path.to_path_buf(), 0),
        None => find_system_font(FontFamily::Gothic)
            .or_else(|| find_system_font(FontFamily::Mincho))
            .ok_or("no Japanese font found on this system; pass one with --font")?,
    };
    Font::load(&path, index)
}

const MENU_ITEMS: [&str; 6] = [
    "menu.start",
    "menu.load",
    "menu.extras",
    "menu.settings",
    "menu.library",
    "menu.quit",
];
const PAUSE_ITEMS: [&str; 7] = [
    "pause.resume",
    "pause.save",
    "pause.load",
    "pause.history",
    "pause.settings",
    "pause.menu",
    "pause.quit",
];
const TABS: [&str; 3] = [
    "settings.tab.system",
    "settings.tab.sound",
    "settings.tab.screen",
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
}

impl Setting {
    fn tab(tab: usize) -> &'static [Setting] {
        match tab {
            0 => &[
                Setting::Language,
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
            _ => &[Setting::TextSpeed, Setting::Window],
        }
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
            Setting::Fullscreen => s.fullscreen = !s.fullscreen,
            Setting::UnlockAll => s.unlock_all = !s.unlock_all,
            other => {
                if let Some(level) = other.level(s) {
                    *level = if forward {
                        (*level + 1).min(MAX_LEVEL)
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
    },
    Settings {
        tab: usize,
        focus: Option<usize>,
    },
    Extras(extras::Extras),
    Viewer(extras::Viewer),
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
    Quit,
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
}

/// History layout.
const HISTORY_PANEL: Rect = Rect::new(140, 40, 1000, 640);
const HISTORY_SIZE: f32 = 20.0;
const HISTORY_ROW: i32 = 30;
const HISTORY_GAP: i32 = 12;
const HISTORY_TOP: i32 = HISTORY_PANEL.y + 80;
const HISTORY_BOTTOM: i32 = HISTORY_PANEL.y + 640 - 50;

impl Shell {
    pub fn new(session: Box<dyn Session>, font: Font, options: ShellOptions) -> Self {
        let settings = options
            .data_dir
            .as_ref()
            .map(|d| Settings::load(&d.join("settings.json")))
            .unwrap_or_default();
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
        self.settings = settings;
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
                .font
                .wrap(&entry.text, HISTORY_SIZE, width)
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
        let scroll = history_last_page(&rows);
        Some(Screen::History {
            entries,
            rows,
            scroll,
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
            } => {
                let last = history_last_page(rows);
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
                            let hit = history_layout(rows, *scroll)
                                .find(|(_, top, row)| {
                                    (top..&(top + HISTORY_ROW)).contains(&&y)
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
                                if let Some(level) = setting.level(&mut settings) {
                                    if (bar.x..bar.right() + 8).contains(&x) {
                                        *level = ui::level_at(bar, MAX_LEVEL, x);
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
            "menu.library" => false,
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

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear(theme::BACKDROP);
        if self.in_game {
            canvas.clear(Color::BLACK);
            let v = self.viewport();
            canvas.draw_scaled(&self.game, v.x, v.y, v.w, v.h);
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
            } => {
                ui::panel(canvas, HISTORY_PANEL);
                let title = self.strings.get("history.title").to_string();
                ui::heading(canvas, &mut self.font, HISTORY_PANEL, &title);
                let hover = history_layout(rows, *scroll)
                    .find(|(_, top, _)| (*top..top + HISTORY_ROW).contains(&mouse.1))
                    .map(|(_, _, row)| row.entry)
                    .filter(|&e| entries[e].voice.is_some() && HISTORY_PANEL.contains(mouse));
                for (_, top, row) in history_layout(rows, *scroll) {
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
                        canvas.fill_rect(HISTORY_PANEL.x + 40, top + 11, 8, 8, theme::ACCENT);
                    }
                    ui::label(
                        canvas,
                        &mut self.font,
                        x,
                        top,
                        &row.text,
                        HISTORY_SIZE,
                        color,
                    );
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
                        ui::level_bar(canvas, bar, level, MAX_LEVEL);
                        let value = if setting == Setting::TextSpeed && level == MAX_LEVEL {
                            self.strings.get("common.instant").to_string()
                        } else {
                            level.to_string()
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
                            Setting::Fullscreen | Setting::UnlockAll => {
                                let on = if setting == Setting::Fullscreen {
                                    self.settings.fullscreen
                                } else {
                                    self.settings.unlock_all
                                };
                                self.strings
                                    .get(if on { "common.on" } else { "common.off" })
                                    .to_string()
                            }
                            _ => String::new(),
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
            }
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

fn setting_row(i: usize) -> Rect {
    Rect::new(220, 210 + i as i32 * 70, 840, 58)
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
) -> impl Iterator<Item = (usize, i32, &HistoryRow)> {
    let mut y = HISTORY_TOP;
    rows.iter()
        .enumerate()
        .skip(scroll)
        .map(move |(i, row)| {
            let top = y + if i == scroll { 0 } else { row.gap };
            y = top + HISTORY_ROW;
            (i, top, row)
        })
        .take_while(|(_, top, _)| top + HISTORY_ROW <= HISTORY_BOTTOM)
}

/// The scroll position that shows the last rows.
fn history_last_page(rows: &[HistoryRow]) -> usize {
    let mut height = 0;
    let mut first = rows.len();
    for i in (0..rows.len()).rev() {
        let h = HISTORY_ROW
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

        // Escape pauses: the game stops.
        step(
            &mut shell,
            &mixer,
            &mut canvas,
            &[InputEvent::KeyDown(Key::Escape)],
        );
        assert!(!shell.in_game());
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
        let menu = pause_rects()[5];
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
