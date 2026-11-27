//! The launcher: the library of games and the folder picker that adds
//! them. Only there when the frontend gives the shell a
//! [`Launcher`](crate::Launcher).

use std::fs;
use std::path::{Path, PathBuf};

use super::{Action, Screen, Shell};
use crate::GameInfo;
use crate::app::{InputEvent, Key, MouseButton};
use crate::gfx::{Canvas, Color, Sprite};
use crate::library::{COVER_HEIGHT, COVER_WIDTH, LibraryGame};
use crate::ui::{self, Rect, State, theme};

const COLUMNS: usize = 4;
const ROWS: usize = 2;
const PER_PAGE: usize = COLUMNS * ROWS;
const CARD_W: u32 = 270;
const CARD_H: u32 = 250;

/// The library screen.
pub(super) struct LibraryScreen {
    games: Vec<LibraryGame>,
    covers: Vec<Option<Sprite>>,
    page: usize,
    focus: Option<usize>,
    pub(super) error: Option<String>,
}

/// The folder picker.
pub(super) struct Picker {
    dir: PathBuf,
    /// Subfolders of `dir`, `..` first when there is a parent.
    entries: Vec<String>,
    drives: Vec<PathBuf>,
    scroll: usize,
    focus: Option<usize>,
    /// The game in `dir`, if any.
    found: Option<GameInfo>,
    error: Option<String>,
}

pub(super) fn add_rect() -> Rect {
    Rect::new(690, 50, 240, 48)
}

fn settings_rect() -> Rect {
    Rect::new(946, 50, 150, 48)
}

fn quit_rect() -> Rect {
    Rect::new(1112, 50, 104, 48)
}

fn prev_rect() -> Rect {
    Rect::new(540, 662, 60, 40)
}

fn next_rect() -> Rect {
    Rect::new(680, 662, 60, 40)
}

pub(super) fn card_rect(n: usize) -> Rect {
    let (col, row) = ((n % COLUMNS) as i32, (n / COLUMNS) as i32);
    Rect::new(
        64 + col * (CARD_W as i32 + 24),
        130 + row * (CARD_H as i32 + 20),
        CARD_W,
        CARD_H,
    )
}

/// The "remove from the library" button of a card.
fn remove_rect(card: Rect) -> Rect {
    Rect::new(card.right() - 36, card.y + 6, 30, 30)
}

impl LibraryScreen {
    fn pages(&self) -> usize {
        self.games.len().div_ceil(PER_PAGE).max(1)
    }

    /// Cards on the current page, with their game index.
    fn cards(&self) -> Vec<(usize, Rect)> {
        let first = self.page * PER_PAGE;
        (first..self.games.len().min(first + PER_PAGE))
            .map(|i| (i, card_rect(i - first)))
            .collect()
    }
}

const PICKER: Rect = Rect::new(40, 24, 1200, 672);
const PICKER_ROWS: usize = 12;

pub(super) fn picker_row(i: usize) -> Rect {
    Rect::new(230, 130 + i as i32 * 42, 990, 38)
}

fn drive_rect(i: usize) -> Rect {
    Rect::new(60, 130 + i as i32 * 42, 150, 38)
}

pub(super) fn use_rect() -> Rect {
    Rect::new(910, 640, 200, 44)
}

fn cancel_rect() -> Rect {
    Rect::new(1120, 640, 100, 44)
}

/// Drives to offer (Windows), or the file system root and home.
fn drives() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|d| PathBuf::from(format!("{}:\\", d as char)))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        let mut list = vec![PathBuf::from("/")];
        if let Some(home) = std::env::var_os("HOME") {
            list.push(PathBuf::from(home));
        }
        list
    }
}

/// Subfolders of `dir`, sorted, without hidden ones.
pub(super) fn subfolders(dir: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !n.starts_with('$'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

impl Shell {
    pub(super) fn library_screen(&self) -> Screen {
        let games = self.library.games();
        let covers = games.iter().map(|g| self.library.cover(&g.root)).collect();
        Screen::Library(LibraryScreen {
            games,
            covers,
            page: 0,
            focus: None,
            error: None,
        })
    }

    fn picker_screen(&mut self) -> Screen {
        let start = self
            .library
            .last_folder()
            .filter(|d| d.is_dir())
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut picker = Picker {
            dir: PathBuf::new(),
            entries: Vec::new(),
            drives: drives(),
            scroll: 0,
            focus: None,
            found: None,
            error: None,
        };
        self.go_to(&mut picker, start);
        Screen::Picker(picker)
    }

    /// Shows `dir` in the picker and looks for a game there.
    fn go_to(&self, picker: &mut Picker, dir: PathBuf) {
        picker.scroll = 0;
        picker.focus = None;
        match subfolders(&dir) {
            Ok(names) => {
                picker.entries = dir
                    .parent()
                    .map(|_| "..".to_string())
                    .into_iter()
                    .chain(names)
                    .collect();
                picker.error = None;
            }
            Err(e) => {
                picker.entries = vec!["..".to_string()];
                picker.error = Some(e);
            }
        }
        picker.found = self
            .launcher
            .as_ref()
            .and_then(|l| l.detect(&dir).ok().flatten());
        picker.dir = dir;
    }

    pub(super) fn library_input(
        &mut self,
        lib: &mut LibraryScreen,
        input: &[InputEvent],
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for event in input {
            match *event {
                InputEvent::MouseMove { x, y } => {
                    lib.focus = lib
                        .cards()
                        .iter()
                        .find(|(_, r)| r.contains((x, y)))
                        .map(|&(i, _)| i);
                }
                InputEvent::KeyDown(Key::Left | Key::PageUp) if lib.page > 0 => lib.page -= 1,
                InputEvent::KeyDown(Key::Right | Key::PageDown) if lib.page + 1 < lib.pages() => {
                    lib.page += 1;
                }
                InputEvent::KeyDown(Key::Escape) => actions.push(Action::Quit),
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    let at = (x, y);
                    if add_rect().contains(at) {
                        actions.push(Action::Push(self.picker_screen()));
                    } else if settings_rect().contains(at) {
                        actions.push(Action::Push(Screen::Settings {
                            tab: 0,
                            focus: None,
                        }));
                    } else if quit_rect().contains(at) {
                        actions.push(Action::Quit);
                    } else if prev_rect().contains(at) && lib.page > 0 {
                        lib.page -= 1;
                    } else if next_rect().contains(at) && lib.page + 1 < lib.pages() {
                        lib.page += 1;
                    } else if let Some(&(i, card)) =
                        lib.cards().iter().find(|(_, r)| r.contains(at))
                    {
                        let root = lib.games[i].root.clone();
                        if remove_rect(card).contains(at) {
                            self.library.remove(&root);
                            actions.push(Action::Replace(self.library_screen()));
                        } else {
                            actions.push(Action::OpenGame(root));
                        }
                    }
                }
                _ => {}
            }
        }
        actions
    }

    pub(super) fn picker_input(
        &mut self,
        picker: &mut Picker,
        input: &[InputEvent],
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let rows = picker.entries.len();
        let last = rows.saturating_sub(PICKER_ROWS);
        for event in input {
            let mut enter = None;
            match *event {
                InputEvent::KeyDown(Key::Escape)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => actions.push(Action::Close),
                InputEvent::Wheel(n) => {
                    picker.scroll = if n > 0 {
                        picker.scroll.saturating_sub(3)
                    } else {
                        (picker.scroll + 3).min(last)
                    };
                }
                InputEvent::KeyDown(Key::PageUp) => {
                    picker.scroll = picker.scroll.saturating_sub(PICKER_ROWS);
                }
                InputEvent::KeyDown(Key::PageDown) => {
                    picker.scroll = (picker.scroll + PICKER_ROWS).min(last);
                }
                InputEvent::KeyDown(Key::Up) if rows > 0 => {
                    let i = picker.focus.map_or(0, |i| i.saturating_sub(1));
                    picker.focus = Some(i);
                    picker.scroll = picker.scroll.min(i);
                }
                InputEvent::KeyDown(Key::Down) if rows > 0 => {
                    let i = picker.focus.map_or(0, |i| (i + 1).min(rows - 1));
                    picker.focus = Some(i);
                    if i >= picker.scroll + PICKER_ROWS {
                        picker.scroll = i + 1 - PICKER_ROWS;
                    }
                }
                InputEvent::KeyDown(Key::Enter) => enter = picker.focus,
                InputEvent::KeyDown(Key::Backspace) if rows > 0 && picker.entries[0] == ".." => {
                    enter = Some(0);
                }
                InputEvent::MouseMove { x, y } => {
                    picker.focus = (0..PICKER_ROWS)
                        .find(|&i| picker_row(i).contains((x, y)))
                        .map(|i| i + picker.scroll)
                        .filter(|&i| i < rows);
                }
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    let at = (x, y);
                    if use_rect().contains(at) {
                        if let Some(game) = picker.found.clone() {
                            self.library.add(&game);
                            let parent = game.root.parent().unwrap_or(&game.root).to_path_buf();
                            self.library.set_last_folder(&parent);
                            actions.push(Action::Close);
                            actions.push(Action::Replace(self.library_screen()));
                        }
                    } else if cancel_rect().contains(at) {
                        actions.push(Action::Close);
                    } else if let Some(d) =
                        (0..picker.drives.len()).find(|&d| drive_rect(d).contains(at))
                    {
                        let drive = picker.drives[d].clone();
                        self.go_to(picker, drive);
                        return actions;
                    } else {
                        enter = (0..PICKER_ROWS)
                            .find(|&i| picker_row(i).contains(at))
                            .map(|i| i + picker.scroll)
                            .filter(|&i| i < rows);
                    }
                }
                _ => {}
            }
            if let Some(i) = enter {
                let next = if picker.entries[i] == ".." {
                    picker.dir.parent().map(Path::to_path_buf)
                } else {
                    Some(picker.dir.join(&picker.entries[i]))
                };
                if let Some(next) = next {
                    self.go_to(picker, next);
                }
                // The listing changed: the rest of the input no longer
                // applies to it.
                return actions;
            }
        }
        actions
    }

    pub(super) fn draw_library(&mut self, canvas: &mut Canvas, lib: &LibraryScreen) {
        let mouse = self.mouse;
        let f = &mut self.font;
        ui::label(canvas, f, 64, 40, "VNRevival", 40.0, theme::TEXT);
        let title = self.strings.get("library.title").to_string();
        ui::label(canvas, f, 70, 90, &title, ui::TEXT_SIZE, theme::MUTED);
        for (rect, key) in [
            (add_rect(), "library.add"),
            (settings_rect(), "menu.settings"),
            (quit_rect(), "menu.quit"),
        ] {
            let text = self.strings.get(key).to_string();
            let state = State::of(rect.contains(mouse), true);
            ui::button(canvas, &mut self.font, rect, &text, state);
        }
        if lib.games.is_empty() {
            let text = self.strings.get("library.empty").to_string();
            let area = Rect::new(140, 300, 1000, 120);
            let f = &mut self.font;
            for (n, line) in f.wrap(&text, 22.0, 1000.0).iter().enumerate() {
                let row = Rect::new(area.x, area.y + n as i32 * 34, area.w, 34);
                ui::label_centered(canvas, f, row, line, 22.0, theme::MUTED);
            }
        }
        for (i, card) in lib.cards() {
            let game = &lib.games[i];
            let hover = lib.focus == Some(i);
            ui::item(canvas, card, State::of(hover, true));
            let cover = Rect::new(card.x + 15, card.y + 10, COVER_WIDTH, COVER_HEIGHT);
            match &lib.covers[i] {
                Some(sprite) => {
                    canvas.draw_sprite_scaled(sprite, cover.x, cover.y, cover.w, cover.h)
                }
                None => {
                    canvas.fill_rect(cover.x, cover.y, cover.w, cover.h, Color::rgb(30, 40, 70));
                    let initial: String = game.title.chars().take(1).collect();
                    ui::label_centered(canvas, &mut self.font, cover, &initial, 64.0, theme::MUTED);
                }
            }
            let f = &mut self.font;
            let name = ui::fit(f, &game.title, ui::TEXT_SIZE, CARD_W as f32 - 30.0);
            ui::label(
                canvas,
                f,
                card.x + 15,
                card.y + 196,
                &name,
                ui::TEXT_SIZE,
                theme::TEXT,
            );
            let when = match &game.last_played {
                Some(date) => format!("{} · {date}", game.engine),
                None => format!("{} · {}", game.engine, self.strings.get("library.new")),
            };
            let when = ui::fit(f, &when, ui::SMALL_SIZE, CARD_W as f32 - 30.0);
            ui::label(
                canvas,
                f,
                card.x + 15,
                card.y + 224,
                &when,
                ui::SMALL_SIZE,
                theme::MUTED,
            );
            if hover {
                let x = remove_rect(card);
                let over = x.contains(mouse);
                canvas.fill_rect(
                    x.x,
                    x.y,
                    x.w,
                    x.h,
                    Color::rgba(0, 0, 0, if over { 220 } else { 150 }),
                );
                let color = if over { theme::ERROR } else { theme::TEXT };
                ui::label_centered(canvas, f, x, "×", 22.0, color);
            }
        }
        if lib.pages() > 1 {
            let f = &mut self.font;
            ui::button(
                canvas,
                f,
                prev_rect(),
                "◀",
                State::of(prev_rect().contains(mouse), lib.page > 0),
            );
            let more = lib.page + 1 < lib.pages();
            ui::button(
                canvas,
                f,
                next_rect(),
                "▶",
                State::of(next_rect().contains(mouse), more),
            );
            let text = format!("{} / {}", lib.page + 1, lib.pages());
            ui::label_centered(
                canvas,
                f,
                Rect::new(600, 662, 80, 40),
                &text,
                ui::TEXT_SIZE,
                theme::MUTED,
            );
        }
        if let Some(error) = &lib.error {
            let f = &mut self.font;
            let text = ui::fit(f, error, ui::SMALL_SIZE, 1150.0);
            ui::label(canvas, f, 64, 632, &text, ui::SMALL_SIZE, theme::ERROR);
        }
    }

    pub(super) fn draw_picker(&mut self, canvas: &mut Canvas, picker: &Picker) {
        let mouse = self.mouse;
        ui::panel(canvas, PICKER);
        let title = self.strings.get("picker.title").to_string();
        ui::heading(canvas, &mut self.font, PICKER, &title);
        let f = &mut self.font;
        let path = ui::fit(f, &picker.dir.display().to_string(), ui::TEXT_SIZE, 1150.0);
        ui::label(canvas, f, 64, 90, &path, ui::TEXT_SIZE, theme::ACCENT);
        for (d, drive) in picker.drives.iter().enumerate().take(PICKER_ROWS) {
            let r = drive_rect(d);
            let current = picker.dir.starts_with(drive);
            let state = if current {
                State::Active
            } else {
                State::of(r.contains(mouse), true)
            };
            ui::item(canvas, r, state);
            let name = drive.display().to_string();
            ui::label(
                canvas,
                f,
                r.x + 14,
                r.y + 8,
                &name,
                ui::TEXT_SIZE,
                theme::TEXT,
            );
        }
        let up = self.strings.get("picker.up").to_string();
        for i in 0..PICKER_ROWS {
            let Some(name) = picker.entries.get(picker.scroll + i) else {
                break;
            };
            let r = picker_row(i);
            ui::item(
                canvas,
                r,
                State::of(picker.focus == Some(picker.scroll + i), true),
            );
            let (text, color) = if name == ".." {
                (up.clone(), theme::MUTED)
            } else {
                folder_icon(canvas, r.x + 14, r.y + 12);
                (name.clone(), theme::TEXT)
            };
            let text = ui::fit(f, &text, ui::TEXT_SIZE, r.w as f32 - 60.0);
            ui::label(canvas, f, r.x + 44, r.y + 8, &text, ui::TEXT_SIZE, color);
        }
        let (status, color) = match (&picker.found, &picker.error) {
            (Some(game), _) => (
                self.strings.get("picker.found").replace("{}", &game.title),
                theme::ACCENT,
            ),
            (None, Some(_)) => (
                self.strings.get("picker.unreadable").to_string(),
                theme::ERROR,
            ),
            (None, None) => (self.strings.get("picker.none").to_string(), theme::MUTED),
        };
        let status = ui::fit(f, &status, ui::TEXT_SIZE, 640.0);
        ui::label(canvas, f, 64, 650, &status, ui::TEXT_SIZE, color);
        let use_text = self.strings.get("picker.use").to_string();
        let found = picker.found.is_some();
        ui::button(
            canvas,
            f,
            use_rect(),
            &use_text,
            State::of(use_rect().contains(mouse), found),
        );
        let back = self.strings.get("common.back").to_string();
        ui::button(
            canvas,
            f,
            cancel_rect(),
            &back,
            State::of(cancel_rect().contains(mouse), true),
        );
    }
}

/// A small folder drawn with rectangles (no font has to have it).
fn folder_icon(canvas: &mut Canvas, x: i32, y: i32) {
    canvas.fill_rect(x, y, 9, 4, theme::ACCENT);
    canvas.fill_rect(x, y + 3, 20, 13, theme::ACCENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_visible_subfolders_sorted() {
        let dir = std::env::temp_dir().join(format!("vnr-picker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for name in ["b", "A", ".hidden", "$RECYCLE.BIN"] {
            fs::create_dir_all(dir.join(name)).unwrap();
        }
        fs::write(dir.join("file.txt"), b"x").unwrap();
        assert_eq!(subfolders(&dir).unwrap(), ["A", "b"]);
        assert!(subfolders(&dir.join("missing")).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
