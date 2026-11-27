//! Engine-independent core of VNRevival.
//!
//! Every original engine (OverflowADV2, …) is supported by a *backend*: a
//! crate that implements [`Backend`] and turns that engine's data into a
//! [`Session`]: the game itself, drawing on a [`Canvas`], playing
//! [`Clip`]s through the [`Mixer`] and reacting to [`InputEvent`]s.
//!
//! Everything around the game is shared by all of them: the
//! [`Shell`](shell::Shell) is the universal interface (main menu, saves,
//! history, settings) that wraps a session into the [`App`] the
//! frontends run.
//!
//! The runtime pieces here know nothing about windows or audio devices;
//! the `vnrevival` binary drives them with SDL2, and [`run_headless`]
//! drives them without any device, for tests and screenshots.

pub mod app;
pub mod audio;
pub mod gfx;
pub mod lang;
pub mod library;
pub mod saves;
pub mod session;
pub mod settings;
pub mod shell;
pub mod text;
pub mod ui;

pub use app::{App, FPS, Flow, Frame, InputEvent, Key, MouseButton, run_headless};
pub use audio::{Clip, FULL_VOLUME, Mixer};
pub use gfx::{Canvas, Color, Sprite};
pub use session::{
    GalleryEntry, HistoryEntry, ReplayScene, SaveData, Session, SessionEvent, Start, Track,
};
pub use settings::{Outline, Settings};
pub use text::{Font, FontChoice, FontFamily, TextStyle};

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Error type returned by backends.
pub type BackendError = Box<dyn Error + Send + Sync>;

/// A game found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// Id of the backend that recognised it (see [`Backend::id`]).
    pub engine: &'static str,
    /// Human-readable title.
    pub title: String,
    /// Install folder.
    pub root: PathBuf,
    /// Extra facts worth showing to the user, as `(label, value)`.
    pub details: Vec<(String, String)>,
}

/// Support for one original engine.
pub trait Backend {
    /// Short stable id, used on the command line (`adv2`).
    fn id(&self) -> &'static str;

    /// Display name of the original engine.
    fn name(&self) -> &'static str;

    /// Checks whether `dir` holds a game for this engine.
    ///
    /// Returns `Ok(None)` when the folder is not for this engine, and an
    /// error when it is but its data cannot be read.
    fn detect(&self, dir: &Path) -> Result<Option<GameInfo>, BackendError>;

    /// Opens a game previously found by [`Backend::detect`]. The
    /// session starts when the interface calls [`Session::start`].
    fn open(
        &self,
        game: &GameInfo,
        options: &LaunchOptions,
    ) -> Result<Box<dyn Session>, BackendError>;
}

/// Settings that apply to every engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Where to start instead of the game's normal entry point (a script
    /// name, engine-specific).
    pub entry: Option<String>,
    /// Font file to use instead of the installed system fonts.
    pub font: Option<PathBuf>,
    /// Folder for saved games and the game's own data instead of the
    /// default one (see [`save_dir`]).
    pub saves: Option<PathBuf>,
}

/// Where VNRevival keeps its files for this user: `%APPDATA%\VNRevival`
/// on Windows, `~/Library/Application Support/VNRevival` on macOS and
/// `$XDG_DATA_HOME/vnrevival` (default `~/.local/share/vnrevival`)
/// elsewhere.
pub fn user_data_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        var("APPDATA").map(|d| d.join("VNRevival"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support/VNRevival"))
    } else {
        var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("vnrevival"))
    }
}

/// Default folder for one game's saves: `<user data>/saves/<engine>-<title>`
/// (see [`game_slug`]).
pub fn save_dir(game: &GameInfo) -> Option<PathBuf> {
    Some(
        user_data_dir()?
            .join("saves")
            .join(game_slug(game.engine, &game.title)),
    )
}

/// `<engine>-<title>`, with the title reduced to lowercase ASCII letters,
/// digits and dashes: a file name for things kept per game.
pub fn game_slug(engine: &str, title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        engine.to_string()
    } else {
        format!("{engine}-{slug}")
    }
}

/// Finds and opens games for the library: the frontend implements it on
/// top of every [`Backend`].
pub trait Launcher {
    /// The game in `dir`, if a backend recognises it.
    fn detect(&self, dir: &Path) -> Result<Option<GameInfo>, String>;

    /// Opens a game found by [`Launcher::detect`].
    fn open(&self, game: &GameInfo) -> Result<Box<dyn Session>, String>;
}

/// Finds `name` inside `dir`, ignoring ASCII case.
///
/// Games from the Windows 9x era mix `PACK\SCRIPT.DAT` and `pack\script.dat`
/// freely; on case-sensitive file systems (Linux, some macOS setups) an exact
/// lookup would miss them. An exact match is preferred when it exists.
pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// Resolves a relative path such as `PACK/SCRIPT.DAT` one component at a
/// time with [`find_ci`].
pub fn resolve_ci(root: &Path, relative: &str) -> Option<PathBuf> {
    relative
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .try_fold(root.to_path_buf(), |dir, part| find_ci(&dir, part))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vnr-core-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_dirs_use_a_readable_name() {
        let game = GameInfo {
            engine: "adv2",
            title: "Pure Mail".into(),
            root: PathBuf::new(),
            details: vec![],
        };
        if let Some(dir) = save_dir(&game) {
            assert!(dir.ends_with("saves/adv2-pure-mail"));
        }
    }

    #[test]
    fn resolves_paths_ignoring_case() {
        let root = scratch_dir("ci");
        fs::create_dir_all(root.join("Pack")).unwrap();
        fs::write(root.join("Pack").join("Script.Dat"), b"x").unwrap();

        let found = resolve_ci(&root, "PACK/SCRIPT.DAT").unwrap();
        assert_eq!(fs::read(found).unwrap(), b"x");
        assert!(resolve_ci(&root, "PACK/VOICE.DAT").is_none());
        assert!(resolve_ci(&root, "MOVIE/OP.AVI").is_none());

        fs::remove_dir_all(root).unwrap();
    }
}
