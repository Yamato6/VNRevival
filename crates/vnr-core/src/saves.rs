//! Save slots: one JSON file per slot (`slot01.json` …) in the game's
//! save folder (see [`crate::save_dir`]).
//!
//! The file holds what the slot list shows (date, chapter, text) and the
//! game's own state as an opaque `game` object (see
//! [`crate::session::SaveData`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::SaveData;

/// Slots per game.
pub const SLOTS: usize = 10;
/// Format version of the slot files.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SlotFile {
    version: u32,
    /// Local time of the save (`2026-11-26 14:03`).
    saved_at: String,
    chapter: String,
    excerpt: String,
    game: serde_json::Value,
}

/// What the slot list shows for a used slot.
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
    pub fn write(&self, slot: usize, save: &SaveData) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let file = SlotFile {
            version: VERSION,
            saved_at: now(),
            chapter: save.chapter.clone(),
            excerpt: excerpt(&save.excerpt, 40),
            game: save.payload.clone(),
        };
        let json = serde_json::to_vec(&file).map_err(io::Error::other)?;
        let path = self.path(slot);
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json)?;
        fs::rename(&temp, &path)
    }

    fn read_file(&self, slot: usize) -> io::Result<Option<SlotFile>> {
        let data = match fs::read(self.path(slot)) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut value: serde_json::Value =
            serde_json::from_slice(&data).map_err(io::Error::other)?;
        // Saves written before the shared interface kept the game state at
        // the top level.
        if value.get("game").is_none() {
            let whole = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.insert("game".into(), whole);
            }
        }
        let file: SlotFile = serde_json::from_value(value).map_err(io::Error::other)?;
        if file.version != VERSION {
            return Err(io::Error::other(format!(
                "save format {} is not supported (expected {VERSION})",
                file.version
            )));
        }
        Ok(Some(file))
    }

    /// Reads a slot; `Ok(None)` if it is empty.
    pub fn read(&self, slot: usize) -> io::Result<Option<SaveData>> {
        Ok(self.read_file(slot)?.map(|file| SaveData {
            payload: file.game,
            chapter: file.chapter,
            excerpt: file.excerpt,
        }))
    }

    /// Summaries of every slot; unreadable slots show their error.
    pub fn summaries(&self) -> Vec<Option<SlotSummary>> {
        (0..SLOTS)
            .map(|slot| match self.read_file(slot) {
                Ok(file) => file.map(|f| SlotSummary {
                    saved_at: f.saved_at,
                    chapter: f.chapter,
                    excerpt: f.excerpt,
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

    #[test]
    fn slots_round_trip() {
        let dir = std::env::temp_dir().join(format!("vnr-slots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let slots = Slots::new(dir.clone());
        assert!(slots.read(0).unwrap().is_none());
        let save = SaveData {
            payload: serde_json::json!({"pc": 12}),
            chapter: "プロローグ".into(),
            excerpt: "こんばんわ".into(),
        };
        slots.write(3, &save).unwrap();
        assert_eq!(slots.read(3).unwrap().unwrap(), save);
        let summaries = slots.summaries();
        assert_eq!(summaries.len(), SLOTS);
        assert_eq!(summaries[3].as_ref().unwrap().chapter, "プロローグ");
        assert!(summaries[0].is_none());

        // An older file with the state at the top level.
        fs::write(
            dir.join("slot02.json"),
            r#"{"version":1,"saved_at":"x","chapter":"c","excerpt":"e","machine":{}}"#,
        )
        .unwrap();
        let old = slots.read(1).unwrap().unwrap();
        assert_eq!(old.chapter, "c");
        assert!(old.payload.get("machine").is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn excerpts_are_short_and_flat() {
        assert_eq!(excerpt("あいう\nえお", 10), "あいう えお");
        assert_eq!(excerpt("あいうえお", 3), "あいう…");
    }
}
