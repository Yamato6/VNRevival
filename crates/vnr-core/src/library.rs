//! The games the player added: `library.json` in the user data folder,
//! with a cover picture per game in `covers/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::GameInfo;
use crate::gfx::{Canvas, Sprite};
use crate::saves::now;

/// Size of the stored covers.
pub const COVER_WIDTH: u32 = 240;
pub const COVER_HEIGHT: u32 = 180;

/// One game of the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryGame {
    /// Install folder.
    pub root: PathBuf,
    pub title: String,
    /// Id of the backend (see [`crate::Backend::id`]).
    pub engine: String,
    /// When it was added and last played (`YYYY-MM-DD HH:MM`).
    pub added: String,
    #[serde(default)]
    pub last_played: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    games: Vec<LibraryGame>,
    /// Folder the folder picker showed last.
    #[serde(default)]
    last_folder: Option<PathBuf>,
}

/// The library; changes are written at once.
#[derive(Debug, Default)]
pub struct Library {
    file: File,
    /// Folder of `library.json` and `covers/`; `None` keeps it in memory.
    dir: Option<PathBuf>,
}

/// Whether two install paths are the same folder.
fn same_folder(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || canonical(a) == canonical(b)
}

impl Library {
    /// Reads `dir/library.json`; empty if it is missing or unreadable.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let file = dir
            .as_ref()
            .and_then(|d| fs::read(d.join("library.json")).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        Self { file, dir }
    }

    /// The games, the last played first (then the last added).
    pub fn games(&self) -> Vec<LibraryGame> {
        let mut games = self.file.games.clone();
        games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| b.added.cmp(&a.added))
        });
        games
    }

    pub fn is_empty(&self) -> bool {
        self.file.games.is_empty()
    }

    /// Adds a game (once per folder; the title is refreshed).
    pub fn add(&mut self, game: &GameInfo) {
        match self
            .file
            .games
            .iter_mut()
            .find(|g| same_folder(&g.root, &game.root))
        {
            Some(known) => {
                known.title = game.title.clone();
                known.engine = game.engine.to_string();
            }
            None => self.file.games.push(LibraryGame {
                root: game.root.clone(),
                title: game.title.clone(),
                engine: game.engine.to_string(),
                added: now(),
                last_played: None,
            }),
        }
        self.store();
    }

    /// Takes a game out of the library (its files stay where they are).
    pub fn remove(&mut self, root: &Path) {
        self.file.games.retain(|g| !same_folder(&g.root, root));
        self.store();
    }

    /// Marks a game as played now.
    pub fn touch(&mut self, root: &Path) {
        if let Some(game) = self
            .file
            .games
            .iter_mut()
            .find(|g| same_folder(&g.root, root))
        {
            game.last_played = Some(now());
            self.store();
        }
    }

    pub fn last_folder(&self) -> Option<&Path> {
        self.file.last_folder.as_deref()
    }

    pub fn set_last_folder(&mut self, dir: &Path) {
        self.file.last_folder = Some(dir.to_path_buf());
        self.store();
    }

    /// Where the cover of a game is kept.
    pub fn cover_path(&self, root: &Path) -> Option<PathBuf> {
        let game = self
            .file
            .games
            .iter()
            .find(|g| same_folder(&g.root, root))?;
        let name = crate::game_slug(&game.engine, &game.title);
        Some(
            self.dir
                .as_ref()?
                .join("covers")
                .join(format!("{name}.png")),
        )
    }

    /// The stored cover of a game.
    pub fn cover(&self, root: &Path) -> Option<Sprite> {
        Sprite::load_png(&self.cover_path(root)?).ok()
    }

    /// Stores `picture`, scaled down, as the cover of a game that has none
    /// yet.
    pub fn save_cover(&self, root: &Path, picture: &Sprite) -> io::Result<()> {
        let Some(path) = self.cover_path(root) else {
            return Ok(());
        };
        if path.is_file() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut canvas = Canvas::new(COVER_WIDTH, COVER_HEIGHT);
        canvas.draw_sprite_scaled(picture, 0, 0, COVER_WIDTH, COVER_HEIGHT);
        canvas.save_png(&path)
    }

    fn store(&self) {
        let Some(dir) = &self.dir else {
            return;
        };
        let write = || -> io::Result<()> {
            fs::create_dir_all(dir)?;
            let json = serde_json::to_vec_pretty(&self.file).map_err(io::Error::other)?;
            fs::write(dir.join("library.json"), json)
        };
        if let Err(e) = write() {
            eprintln!("could not save the library: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(root: &Path, title: &str) -> GameInfo {
        GameInfo {
            engine: "adv2",
            title: title.into(),
            root: root.to_path_buf(),
            details: Vec::new(),
        }
    }

    #[test]
    fn adds_touches_removes_and_persists() {
        let dir = std::env::temp_dir().join(format!("vnr-library-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("game-a"), dir.join("game-b"));
        let mut library = Library::open(Some(dir.clone()));
        assert!(library.is_empty());
        library.add(&info(&a, "A"));
        library.add(&info(&b, "B"));
        // The same folder again only refreshes the title.
        library.add(&info(&a, "A2"));
        assert_eq!(library.games().len(), 2);
        library.touch(&a);
        library.set_last_folder(&dir);

        let mut again = Library::open(Some(dir.clone()));
        let games = again.games();
        assert_eq!(games[0].title, "A2", "the last played comes first");
        assert!(games[0].last_played.is_some() && games[1].last_played.is_none());
        assert_eq!(again.last_folder(), Some(dir.as_path()));

        // Covers are stored once, scaled.
        let picture = Sprite::from_rgb(2, 2, &[255; 12]);
        again.save_cover(&a, &picture).unwrap();
        let cover = again.cover(&a).unwrap();
        assert_eq!((cover.width, cover.height), (COVER_WIDTH, COVER_HEIGHT));
        assert!(again.cover(&b).is_none());

        again.remove(&a);
        assert_eq!(Library::open(Some(dir.clone())).games().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
