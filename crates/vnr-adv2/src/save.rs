//! Saved games.
//!
//! A save is taken while the game waits for the player (a message, a
//! choice or a click) and holds the script machine's state plus what the
//! engine shows and plays, with pictures and scripts referred to by name.
//! Each slot is a JSON file (`slot01.json` …) in the game's save folder.
//!
//! The original engine's own save files are not read or written.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backlog::History;
use crate::pc::{Line, PcView};
use crate::vm::Snapshot;

/// Slots offered in the menu.
pub const SLOTS: usize = 10;
/// Format version written to every save.
pub const VERSION: u32 = 1;

/// One saved game.
#[derive(Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub version: u32,
    /// Local time of the save, for the menu (`2026-11-26 14:03`).
    pub saved_at: String,
    /// Chapter title at the time.
    pub chapter: String,
    /// The text on screen, shortened, for the menu.
    pub excerpt: String,
    /// Scripts the machine state needs, as `(id, name)`.
    pub scripts: Vec<(u16, String)>,
    pub machine: Snapshot,
    pub engine: EngineState,
}

/// The engine side of a save.
#[derive(Clone, Serialize, Deserialize)]
pub struct EngineState {
    pub scene: SceneState,
    /// Music track playing (`MM%03d`).
    pub music: Option<i32>,
    pub chapter: String,
    pub speaker: (usize, String),
    pub window_visible: bool,
    pub engine_variables: Vec<(u16, i32)>,
    pub chat_log: Vec<Line>,
    pub mail_lines: Vec<Line>,
    pub letter: Vec<String>,
    /// Message history (missing in older saves).
    #[serde(default)]
    pub history: History,
    pub mode: SavedMode,
}

/// The picture on screen, by name.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SceneState {
    /// Colour fill as `[r, g, b]`.
    pub fill: Option<[u8; 3]>,
    pub background: Option<String>,
    pub characters: Vec<(i32, String)>,
    pub pc: PcView,
}

/// What the game was waiting for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedMode {
    Message {
        text: String,
        speaker: Option<String>,
        style: usize,
    },
    Choice {
        options: Vec<String>,
        variable: Option<u16>,
    },
    Click,
}

/// What the menu shows for a used slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotSummary {
    pub saved_at: String,
    pub chapter: String,
    pub excerpt: String,
}

/// The save slots of one game.
#[derive(Debug, Clone)]
pub struct Slots {
    dir: PathBuf,
}

impl Slots {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, slot: usize) -> PathBuf {
        self.dir.join(format!("slot{:02}.json", slot + 1))
    }

    /// Writes a slot (through a temporary file, so a failed write leaves
    /// the old save intact).
    pub fn write(&self, slot: usize, save: &SaveFile) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_vec(save).map_err(io::Error::other)?;
        let path = self.path(slot);
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json)?;
        fs::rename(&temp, &path)
    }

    /// Reads a slot; `Ok(None)` if it is empty.
    pub fn read(&self, slot: usize) -> io::Result<Option<SaveFile>> {
        let data = match fs::read(self.path(slot)) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let save: SaveFile = serde_json::from_slice(&data).map_err(io::Error::other)?;
        if save.version != VERSION {
            return Err(io::Error::other(format!(
                "save format {} is not supported (expected {VERSION})",
                save.version
            )));
        }
        Ok(Some(save))
    }

    /// Summaries of every slot; unreadable slots show their error.
    pub fn summaries(&self) -> Vec<Option<SlotSummary>> {
        (0..SLOTS)
            .map(|slot| match self.read(slot) {
                Ok(save) => save.map(|s| SlotSummary {
                    saved_at: s.saved_at,
                    chapter: s.chapter,
                    excerpt: s.excerpt,
                }),
                Err(e) => Some(SlotSummary {
                    saved_at: String::new(),
                    chapter: String::new(),
                    excerpt: format!("(unreadable: {e})"),
                }),
            })
            .collect()
    }
}

/// The first `max` characters of `text` on one line, with `…` if cut.
pub fn excerpt(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut cut: String = flat.chars().take(max).collect();
        cut.push('…');
        cut
    }
}

/// The current local time as `YYYY-MM-DD HH:MM`.
pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SaveFile {
        SaveFile {
            version: VERSION,
            saved_at: "2026-11-26 14:03".into(),
            chapter: "プロローグ".into(),
            excerpt: "こんばんわ".into(),
            scripts: vec![(490, "PRO1".into())],
            machine: Snapshot {
                current: 490,
                pc: 12,
                acc: 0,
                stack: vec![(1, 0)],
                variables: vec![(12388, 1)],
                seed: 1,
                halted: false,
            },
            engine: EngineState {
                scene: SceneState::default(),
                music: Some(3),
                chapter: "プロローグ".into(),
                speaker: (2, String::new()),
                window_visible: true,
                engine_variables: vec![],
                chat_log: vec![Line {
                    style: 11,
                    text: "ＥＶＥ：こんばんわ".into(),
                }],
                mail_lines: vec![],
                letter: vec![],
                history: History::default(),
                mode: SavedMode::Click,
            },
        }
    }

    #[test]
    fn slots_round_trip() {
        let dir = std::env::temp_dir().join(format!("vnr-saves-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let slots = Slots::new(dir.clone());
        assert!(slots.read(0).unwrap().is_none());
        slots.write(3, &sample()).unwrap();
        let back = slots.read(3).unwrap().unwrap();
        assert_eq!(back.machine, sample().machine);
        assert_eq!(back.engine.chat_log, sample().engine.chat_log);
        let summaries = slots.summaries();
        assert_eq!(summaries.len(), SLOTS);
        assert_eq!(summaries[3].as_ref().unwrap().chapter, "プロローグ");
        assert!(summaries[0].is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn excerpts_are_short_and_flat() {
        assert_eq!(excerpt("あいう\nえお", 10), "あいう えお");
        assert_eq!(excerpt("あいうえお", 3), "あいう…");
    }
}
