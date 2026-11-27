//! A running game as the universal interface sees it.
//!
//! Backends only play the game: they turn script and data into pictures
//! and sound. Everything around it (the main menu, saving and loading,
//! the history, settings and the extras) belongs to the
//! [`Shell`](crate::shell::Shell), which talks to the game through the
//! [`Session`] trait.

use crate::app::Frame;
use crate::settings::Settings;

/// How to begin a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Straight into a new playthrough, skipping the game's own title.
    NewGame,
    /// The game's own start-up and title screen.
    OriginalTitle,
    /// Replay one scene of the extras, by the id the game gives it.
    Scene(String),
}

/// What the game asks of the interface after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    Continue,
    /// Back to the main menu (the game reached its title screen, or a
    /// replayed scene ended).
    ToMenu,
    /// The game's own title asked for the load screen. If the player
    /// closes it without loading, call [`Session::dismissed`].
    OpenLoad,
    /// The game's own title asked for the extras.
    OpenExtras,
    Quit,
}

/// A saved game as the game sees it; the interface adds the slot, the
/// date and the file.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveData {
    /// Engine-specific state.
    pub payload: serde_json::Value,
    /// Chapter or scene title, for the slot list.
    pub chapter: String,
    /// The text on screen, for the slot list.
    pub excerpt: String,
}

/// One entry of the message history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub speaker: Option<String>,
    pub text: String,
    /// Voice to replay with [`Session::play_voice`].
    pub voice: Option<String>,
}

/// A running game.
pub trait Session {
    /// Window title.
    fn title(&self) -> String;

    /// Size of the game's own screen.
    fn resolution(&self) -> (u32, u32);

    /// Starts (or restarts) the game. Called before the first frame.
    fn start(&mut self, start: Start) -> Result<(), String>;

    /// Advances the game one frame and draws it on `frame.canvas` (the
    /// game's resolution). Input coordinates are in game pixels.
    fn update(&mut self, frame: &mut Frame<'_>) -> SessionEvent;

    /// Whether the game is at a point where it can be saved.
    fn can_save(&self) -> bool;

    /// The current state.
    fn save(&self) -> Result<SaveData, String>;

    /// Continues from a saved state.
    fn load(&mut self, data: &SaveData) -> Result<(), String>;

    /// The player closed a screen the game asked for (see
    /// [`SessionEvent::OpenLoad`]) without using it.
    fn dismissed(&mut self) {}

    /// The message history, oldest first.
    fn history(&self) -> Vec<HistoryEntry> {
        Vec::new()
    }

    /// Plays a voice from the history.
    fn play_voice(&mut self, _voice: &str) {}

    /// Applies the player's settings (text speed, volumes, window).
    fn apply(&mut self, _settings: &Settings) {}

    /// Stops all sound (when leaving the game for the menu).
    fn silence(&mut self) {}
}
