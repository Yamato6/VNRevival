//! The extras: CG gallery (with a viewer for each picture's variants),
//! music room and scene replay. Only what the player has seen or heard is
//! open, unless the settings unlock everything.

use std::sync::Arc;

use super::{Action, Screen, Shell, tab_rect};
use crate::app::{InputEvent, Key, MouseButton};
use crate::gfx::{Canvas, Color, Sprite};
use crate::session::{GalleryEntry, ReplayScene, Start, Track};
use crate::settings::Settings;
use crate::ui::{self, Rect, State, theme};

/// Mixer channel of the music room (the games use the lower ones).
pub const MUSIC_CHANNEL: usize = 7;

pub(super) const GALLERY: usize = 0;
pub(super) const MUSIC: usize = 1;
pub(super) const SCENES: usize = 2;
const TABS: [&str; 3] = ["extras.gallery", "extras.music", "extras.scenes"];

const PANEL: Rect = Rect::new(60, 30, 1160, 660);
const COLUMNS: usize = 6;
const ROWS: usize = 3;
const PER_PAGE: usize = COLUMNS * ROWS;
const THUMB_W: u32 = 160;
const THUMB_H: u32 = 120;
/// Top of the items, under the tabs.
const ITEMS_TOP: i32 = 206;
const CAPTION: u32 = 26;
const TRACK_ROWS: usize = 10;
const TRACKS_PER_PAGE: usize = TRACK_ROWS * 2;

/// Thumbnails are cached by `(tab, index)`.
pub(super) type ThumbKey = (usize, usize);

/// The extras screen.
pub(super) struct Extras {
    tab: usize,
    page: usize,
    focus: Option<usize>,
    gallery: Vec<GalleryEntry>,
    tracks: Vec<Track>,
    scenes: Vec<ReplayScene>,
    playing: Option<usize>,
    /// Opened by the game itself (its title's extras option).
    pub(super) requested: bool,
}

/// A gallery picture shown full size.
pub(super) struct Viewer {
    index: usize,
    /// Variants that can be shown.
    variants: Vec<usize>,
    at: usize,
}

impl Viewer {
    #[cfg(test)]
    pub(super) fn variants(&self) -> &[usize] {
        &self.variants
    }
}

impl Extras {
    #[cfg(test)]
    pub(super) fn tab(&self) -> usize {
        self.tab
    }

    fn len(&self) -> usize {
        match self.tab {
            GALLERY => self.gallery.len(),
            MUSIC => self.tracks.len(),
            _ => self.scenes.len(),
        }
    }

    fn per_page(&self) -> usize {
        if self.tab == MUSIC {
            TRACKS_PER_PAGE
        } else {
            PER_PAGE
        }
    }

    fn pages(&self) -> usize {
        self.len().div_ceil(self.per_page()).max(1)
    }

    /// Rectangles of the items on the current page, with their index.
    pub(super) fn items(&self) -> Vec<(usize, Rect)> {
        let first = self.page * self.per_page();
        (first..self.len().min(first + self.per_page()))
            .map(|i| {
                let n = i - first;
                let r = if self.tab == MUSIC {
                    let (col, row) = ((n / TRACK_ROWS) as i32, (n % TRACK_ROWS) as i32);
                    Rect::new(100 + col * 550, ITEMS_TOP + row * 46, 530, 40)
                } else {
                    let (col, row) = ((n % COLUMNS) as i32, (n / COLUMNS) as i32);
                    Rect::new(
                        100 + col * (THUMB_W as i32 + 24),
                        ITEMS_TOP + row * (THUMB_H + CAPTION + 10) as i32,
                        THUMB_W,
                        THUMB_H + CAPTION,
                    )
                };
                (i, r)
            })
            .collect()
    }

    fn open(&self, i: usize, all: bool) -> bool {
        all || match self.tab {
            GALLERY => self.gallery[i].seen(),
            MUSIC => self.tracks[i].heard,
            _ => self.scenes[i].unlocked,
        }
    }

    fn turn(&mut self, forward: bool) {
        let pages = self.pages();
        self.page = if forward {
            (self.page + 1) % pages
        } else {
            (self.page + pages - 1) % pages
        };
        self.focus = None;
    }
}

/// Page buttons, right of the tabs.
fn prev_rect() -> Rect {
    Rect::new(900, 143, 60, 40)
}

fn next_rect() -> Rect {
    Rect::new(1120, 143, 60, 40)
}

impl Shell {
    /// The extras screen, on `tab`.
    pub(super) fn extras_screen(&mut self, requested: bool, tab: usize) -> Screen {
        Screen::Extras(Extras {
            tab,
            page: 0,
            focus: None,
            gallery: self.session.gallery(),
            tracks: self.session.tracks(),
            scenes: self.session.scenes(),
            playing: None,
            requested,
        })
    }

    fn stop_music_room(&self) {
        if let Some(mixer) = &self.mixer {
            mixer.stop(MUSIC_CHANNEL);
        }
    }

    pub(super) fn extras_input(&mut self, ex: &mut Extras, input: &[InputEvent]) -> Vec<Action> {
        let mut actions = Vec::new();
        let all = self.settings.unlock_all;
        for event in input {
            match *event {
                InputEvent::KeyDown(Key::Escape)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => {
                    self.stop_music_room();
                    actions.push(Action::Close);
                }
                InputEvent::MouseMove { x, y } => {
                    ex.focus = ex
                        .items()
                        .iter()
                        .find(|(_, r)| r.contains((x, y)))
                        .map(|&(i, _)| i);
                }
                InputEvent::KeyDown(Key::Left | Key::PageUp) => ex.turn(false),
                InputEvent::KeyDown(Key::Right | Key::PageDown) => ex.turn(true),
                InputEvent::Wheel(n) => ex.turn(n < 0),
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    if let Some(t) = (0..TABS.len()).find(|&t| tab_rect(t).contains((x, y))) {
                        ex.tab = t;
                        ex.page = 0;
                        ex.focus = None;
                    } else if prev_rect().contains((x, y)) && ex.pages() > 1 {
                        ex.turn(false);
                    } else if next_rect().contains((x, y)) && ex.pages() > 1 {
                        ex.turn(true);
                    } else if let Some(&(i, _)) =
                        ex.items().iter().find(|(_, r)| r.contains((x, y)))
                        && ex.open(i, all)
                    {
                        actions.extend(self.pick(ex, i));
                    }
                }
                _ => {}
            }
        }
        actions
    }

    /// What clicking item `i` of the extras does.
    fn pick(&mut self, ex: &mut Extras, i: usize) -> Option<Action> {
        match ex.tab {
            GALLERY => {
                let entry = &ex.gallery[i];
                let all = self.settings.unlock_all;
                let variants: Vec<usize> = (0..entry.variants.len())
                    .filter(|&v| all || entry.variants[v])
                    .collect();
                (!variants.is_empty()).then_some(Action::Push(Screen::Viewer(Viewer {
                    index: i,
                    variants,
                    at: 0,
                })))
            }
            MUSIC => {
                let mixer = self.mixer.clone()?;
                if ex.playing == Some(i) {
                    mixer.stop(MUSIC_CHANNEL);
                    ex.playing = None;
                } else if let Some(clip) = self.session.track(i) {
                    let volume = Settings::volume(self.settings.music);
                    mixer.play(MUSIC_CHANNEL, clip, volume);
                    ex.playing = Some(i);
                }
                None
            }
            _ => {
                self.stop_music_room();
                Some(Action::Start(Start::Scene(ex.scenes[i].id.clone())))
            }
        }
    }

    pub(super) fn viewer_input(&mut self, v: &mut Viewer, input: &[InputEvent]) -> Vec<Action> {
        let mut actions = Vec::new();
        for event in input {
            match event {
                InputEvent::KeyDown(Key::Escape)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } => actions.push(Action::Close),
                InputEvent::KeyDown(Key::Left) | InputEvent::Wheel(1..) => {
                    v.at = v.at.saturating_sub(1);
                }
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    ..
                }
                | InputEvent::KeyDown(Key::Right | Key::Enter | Key::Space)
                | InputEvent::Wheel(..=-1) => {
                    if v.at + 1 < v.variants.len() {
                        v.at += 1;
                    } else {
                        actions.push(Action::Close);
                    }
                }
                _ => {}
            }
        }
        actions
    }

    fn thumbnail(&mut self, tab: usize, i: usize) -> Option<Arc<Sprite>> {
        let key: ThumbKey = (tab, i);
        if let Some(cached) = self.thumbs.get(&key) {
            return cached.clone();
        }
        let sprite = if tab == GALLERY {
            self.session.gallery_thumbnail(i)
        } else {
            self.session.scene_thumbnail(i)
        };
        self.thumbs.insert(key, sprite.clone());
        sprite
    }

    pub(super) fn draw_extras(&mut self, canvas: &mut Canvas, ex: &Extras) {
        canvas.fill_rect(0, 0, ui::WIDTH, ui::HEIGHT, theme::BACKDROP);
        ui::panel(canvas, PANEL);
        let title = self.strings.get("menu.extras").to_string();
        ui::heading(canvas, &mut self.font, PANEL, &title);
        let mouse = self.mouse;
        for (t, key) in TABS.iter().enumerate() {
            let state = if t == ex.tab {
                State::Active
            } else {
                State::of(tab_rect(t).contains(mouse), true)
            };
            let text = self.strings.get(key).to_string();
            ui::button(canvas, &mut self.font, tab_rect(t), &text, state);
        }
        let all = self.settings.unlock_all;
        let items = ex.items();
        if items.is_empty() {
            let text = self.strings.get("extras.empty").to_string();
            ui::label_centered(
                canvas,
                &mut self.font,
                Rect::new(PANEL.x, 330, PANEL.w, 40),
                &text,
                ui::TEXT_SIZE,
                theme::MUTED,
            );
        }
        for (i, r) in items {
            let open = ex.open(i, all);
            let state = State::of(ex.focus == Some(i), open);
            if ex.tab == MUSIC {
                let playing = ex.playing == Some(i);
                ui::item(canvas, r, if playing { State::Active } else { state });
                let track = &ex.tracks[i];
                let text = if open {
                    format!("{}  {}", if playing { "♪" } else { "  " }, track.title)
                } else {
                    format!("    {}", self.strings.get("extras.locked"))
                };
                let color = if open { theme::TEXT } else { theme::DISABLED };
                ui::label(
                    canvas,
                    &mut self.font,
                    r.x + 14,
                    r.y + 9,
                    &text,
                    ui::TEXT_SIZE,
                    color,
                );
                continue;
            }
            let picture = Rect::new(r.x, r.y, THUMB_W, THUMB_H);
            ui::item(canvas, picture, state);
            match open.then(|| self.thumbnail(ex.tab, i)).flatten() {
                Some(thumb) => {
                    canvas.draw_sprite_scaled(&thumb, picture.x, picture.y, picture.w, picture.h);
                    if state == State::Hover {
                        ui::frame(canvas, picture, theme::ACCENT);
                        ui::frame(canvas, picture.inset(1), theme::ACCENT);
                    }
                }
                None => {
                    let text = if open {
                        ""
                    } else {
                        self.strings.get("extras.locked")
                    }
                    .to_string();
                    ui::label_centered(
                        canvas,
                        &mut self.font,
                        picture,
                        &text,
                        ui::TITLE_SIZE,
                        theme::DISABLED,
                    );
                }
            }
            let caption = match ex.tab {
                GALLERY => {
                    let entry = &ex.gallery[i];
                    let seen = entry.variants.iter().filter(|&&v| v).count();
                    if entry.variants.len() > 1 {
                        format!("{:03}  {seen}/{}", i + 1, entry.variants.len())
                    } else {
                        format!("{:03}", i + 1)
                    }
                }
                _ => {
                    let scene = &ex.scenes[i];
                    format!("{}  {}", scene.group, scene.title)
                }
            };
            let caption = ui::fit(&mut self.font, &caption, ui::SMALL_SIZE, THUMB_W as f32);
            let color = if open { theme::MUTED } else { theme::DISABLED };
            ui::label(
                canvas,
                &mut self.font,
                r.x + 2,
                r.y + THUMB_H as i32 + 4,
                &caption,
                ui::SMALL_SIZE,
                color,
            );
        }
        if ex.pages() > 1 {
            let f = &mut self.font;
            ui::button(
                canvas,
                f,
                prev_rect(),
                "◀",
                State::of(prev_rect().contains(mouse), true),
            );
            ui::button(
                canvas,
                f,
                next_rect(),
                "▶",
                State::of(next_rect().contains(mouse), true),
            );
            let page = format!("{} / {}", ex.page + 1, ex.pages());
            ui::label_centered(
                canvas,
                f,
                Rect::new(
                    prev_rect().right(),
                    prev_rect().y,
                    (next_rect().x - prev_rect().right()) as u32,
                    prev_rect().h,
                ),
                &page,
                ui::TEXT_SIZE,
                theme::TEXT,
            );
        }
    }

    pub(super) fn draw_viewer(&mut self, canvas: &mut Canvas, v: &Viewer) {
        canvas.clear(Color::BLACK);
        let variant = v.variants[v.at];
        if let Some(picture) = self.session.gallery_picture(v.index, variant) {
            let r = self.fit(picture.width, picture.height);
            canvas.draw_sprite_scaled(&picture, r.x, r.y, r.w, r.h);
        }
        if v.variants.len() > 1 {
            let text = format!("{} / {}", v.at + 1, v.variants.len());
            ui::label(
                canvas,
                &mut self.font,
                16,
                ui::HEIGHT as i32 - 30,
                &text,
                ui::SMALL_SIZE,
                theme::MUTED,
            );
        }
    }

    /// The largest rectangle of `w × h`'s aspect ratio centred on the
    /// interface.
    fn fit(&self, w: u32, h: u32) -> Rect {
        let scale = f64::min(
            f64::from(ui::WIDTH) / f64::from(w.max(1)),
            f64::from(ui::HEIGHT) / f64::from(h.max(1)),
        );
        let (w, h) = (
            (f64::from(w) * scale).round() as u32,
            (f64::from(h) * scale).round() as u32,
        );
        Rect::centered(w, h)
    }
}
