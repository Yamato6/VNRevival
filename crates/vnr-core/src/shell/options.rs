//! The game's own options (the hero's name, for example): text fields
//! typed with the keyboard or an input method (IME).
//!
//! The screen is the "Game" tab of the settings, or, when a new game
//! asks for them ([`SessionEvent::AskOptions`](crate::SessionEvent)), a
//! screen of its own with a button to start.

use super::{Action, Screen, Shell, setting_control, setting_row};
use crate::app::{InputEvent, Key, MouseButton};
use crate::gfx::Canvas;
use crate::session::GameOption;
use crate::ui::{self, Rect, State, theme};

/// The settings tab of the game's options.
pub(super) const GAME_TAB: usize = 4;

/// The options screen.
pub(super) struct OptionsScreen {
    fields: Vec<GameOption>,
    /// The text of each field as typed.
    pub(super) values: Vec<String>,
    /// The field being typed into.
    focus: Option<usize>,
    /// What the input method is composing in that field.
    composition: String,
    pub(super) error: Option<String>,
    /// Asked by a new game.
    pub(super) asked: bool,
}

pub(super) fn field_rect(i: usize) -> Rect {
    setting_control(i)
}

pub(super) fn defaults_rect() -> Rect {
    Rect::new(220, 600, 280, 44)
}

pub(super) fn start_rect() -> Rect {
    Rect::new(840, 600, 220, 44)
}

impl OptionsScreen {
    /// Typing goes into a field.
    pub(super) fn editing(&self) -> Option<usize> {
        self.focus
    }
}

impl Shell {
    pub(super) fn options_screen(&mut self, asked: bool) -> Screen {
        let fields = self.session.options(self.settings.language);
        Screen::Options(OptionsScreen {
            values: fields.iter().map(|f| f.value.clone()).collect(),
            fields,
            focus: asked.then_some(0),
            composition: String::new(),
            error: None,
            asked,
        })
    }

    /// Keeps what was typed in field `i`; `false` (and the reason on the
    /// screen) if the game does not take it.
    fn commit(&mut self, o: &mut OptionsScreen, i: usize) -> bool {
        let value = o.values[i].trim().to_string();
        let field = &mut o.fields[i];
        if value == field.value {
            o.values[i] = value;
            return true;
        }
        match self.session.set_option(&field.id, &value) {
            Ok(()) => {
                field.value.clone_from(&value);
                o.values[i] = value;
                o.error = None;
                true
            }
            Err(e) => {
                o.error = Some(format!("{}: {e}", field.label));
                o.focus = Some(i);
                false
            }
        }
    }

    /// Keeps the field being typed into, if any.
    fn commit_focus(&mut self, o: &mut OptionsScreen) -> bool {
        o.composition.clear();
        match o.focus {
            Some(i) => self.commit(o, i),
            None => true,
        }
    }

    pub(super) fn options_input(
        &mut self,
        o: &mut OptionsScreen,
        input: &[InputEvent],
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        // An input method may let the key that ends a composition through
        // in the same frame as the text it commits.
        let typed = input
            .iter()
            .any(|e| matches!(e, InputEvent::Text(_) | InputEvent::Composition(_)));
        let n = o.fields.len();
        for event in input {
            let composing = !o.composition.is_empty();
            match event {
                InputEvent::Text(text) => {
                    if let Some(i) = o.focus {
                        let max = o.fields[i].max_chars;
                        let value = &mut o.values[i];
                        for c in text.chars().filter(|c| !c.is_control()) {
                            if value.chars().count() < max {
                                value.push(c);
                            }
                        }
                    }
                    o.composition.clear();
                }
                InputEvent::Composition(text) => {
                    if o.focus.is_some() {
                        o.composition.clone_from(text);
                    }
                }
                InputEvent::KeyDown(Key::Backspace) if !composing && !typed => {
                    if let Some(i) = o.focus {
                        o.values[i].pop();
                    }
                }
                InputEvent::KeyDown(Key::Escape)
                | InputEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } if !composing => {
                    if self.commit_focus(o) {
                        actions.push(Action::Close);
                    }
                }
                InputEvent::KeyDown(Key::Enter) if !composing && !typed => match o.focus {
                    Some(i) => {
                        if self.commit(o, i) {
                            o.focus = (i + 1 < n).then_some(i + 1);
                        }
                    }
                    None if o.asked => actions.push(Action::Close),
                    None => o.focus = (n > 0).then_some(0),
                },
                InputEvent::KeyDown(key @ (Key::Up | Key::Down)) if !composing && n > 0 => {
                    if self.commit_focus(o) {
                        let i = o.focus.unwrap_or(0);
                        o.focus = Some(if *key == Key::Up {
                            (i + n - 1) % n
                        } else {
                            (i + 1) % n
                        });
                    }
                }
                InputEvent::KeyDown(key @ (Key::PageUp | Key::PageDown)) if !o.asked => {
                    if self.commit_focus(o) {
                        let tab = if *key == Key::PageUp { GAME_TAB - 1 } else { 0 };
                        actions.push(Action::Replace(Screen::Settings { tab, focus: None }));
                    }
                }
                &InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                } => {
                    if !self.commit_focus(o) {
                        continue;
                    }
                    let tab = (0..self.tab_count()).find(|&t| super::tab_rect(t).contains((x, y)));
                    if let Some(i) = (0..n).find(|&i| setting_row(i).contains((x, y))) {
                        o.focus = Some(i);
                    } else if o.asked && start_rect().contains((x, y)) {
                        actions.push(Action::Close);
                    } else if defaults_rect().contains((x, y)) {
                        for i in 0..n {
                            o.values[i].clone_from(&o.fields[i].default);
                            if !self.commit(o, i) {
                                break;
                            }
                        }
                    } else if let Some(tab) = tab.filter(|&t| !o.asked && t != GAME_TAB) {
                        actions.push(Action::Replace(Screen::Settings { tab, focus: None }));
                    } else {
                        o.focus = None;
                    }
                }
                _ => {}
            }
        }
        actions
    }

    pub(super) fn draw_options(&mut self, canvas: &mut Canvas, o: &OptionsScreen) {
        let panel = Rect::new(190, 60, 900, 600);
        ui::panel(canvas, panel);
        if o.asked {
            let title = self.strings.get("options.ask").to_string();
            ui::heading(canvas, &mut self.font, panel, &title);
            let note = self.strings.get("options.ask.note").to_string();
            ui::label(
                canvas,
                &mut self.font,
                220,
                150,
                &note,
                ui::TEXT_SIZE,
                theme::MUTED,
            );
            let start = self.strings.get("options.start").to_string();
            let state = State::of(start_rect().contains(self.mouse), true);
            ui::button(canvas, &mut self.font, start_rect(), &start, state);
        } else {
            let title = self.strings.get("settings.title").to_string();
            ui::heading(canvas, &mut self.font, panel, &title);
            self.draw_tabs(canvas, GAME_TAB);
        }
        for (i, field) in o.fields.iter().enumerate() {
            let row = setting_row(i);
            let editing = o.focus == Some(i);
            ui::item(
                canvas,
                row,
                if editing {
                    State::Active
                } else {
                    State::of(row.contains(self.mouse), true)
                },
            );
            ui::label(
                canvas,
                &mut self.font,
                row.x + 20,
                row.y + 14,
                &field.label,
                ui::TEXT_SIZE,
                theme::TEXT,
            );
            let r = field_rect(i);
            canvas.fill_rect(r.x, r.y, r.w, r.h, crate::gfx::Color::rgba(0, 0, 0, 120));
            ui::frame(
                canvas,
                r,
                if editing {
                    theme::ACCENT
                } else {
                    theme::BORDER
                },
            );
            // Game texts are Japanese: the game text font draws them.
            let size = ui::TEXT_SIZE + 2.0;
            let (x, y) = (r.x + 12, r.y + 7);
            let value = &o.values[i];
            ui::label(canvas, self.text_font(), x, y, value, size, theme::TEXT);
            let mut end = x + self.text_font().measure(value, size).ceil() as i32;
            if editing {
                if !o.composition.is_empty() {
                    let comp = &o.composition;
                    ui::label(canvas, self.text_font(), end, y, comp, size, theme::ACCENT);
                    let w = self.text_font().measure(comp, size).ceil() as i32;
                    canvas.fill_rect(end, r.bottom() - 8, w.max(1) as u32, 2, theme::ACCENT);
                    end += w;
                }
                canvas.fill_rect(end + 1, r.y + 8, 2, r.h - 16, theme::TEXT);
            }
        }
        let below = setting_row(o.fields.len().max(1) - 1).bottom() + 16;
        let note = match (&o.error, o.focus) {
            (Some(error), _) => Some((error.clone(), theme::ERROR)),
            (None, Some(i)) => o.fields[i].hint.clone().map(|h| (h, theme::MUTED)),
            _ => None,
        };
        if let Some((text, color)) = note {
            let f = &mut self.font;
            for (n, line) in f
                .wrap(&text, ui::SMALL_SIZE, 840.0)
                .iter()
                .take(3)
                .enumerate()
            {
                ui::label(
                    canvas,
                    f,
                    220,
                    below + n as i32 * 22,
                    line,
                    ui::SMALL_SIZE,
                    color,
                );
            }
        }
        let defaults = self.strings.get("options.defaults").to_string();
        let state = State::of(defaults_rect().contains(self.mouse), true);
        ui::button(canvas, &mut self.font, defaults_rect(), &defaults, state);
    }
}
