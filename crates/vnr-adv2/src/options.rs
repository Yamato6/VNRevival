//! Options of Pure Mail itself, kept as `options.json` in the game's data
//! folder: the hero's names for new games (the original asks for them
//! in a dialog at `sys 17`).

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::names::Names;

const FILE: &str = "options.json";
/// Where earlier versions kept the names, with the player's settings.
const OLD_FILE: &str = "settings.json";

/// The game's options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameOptions {
    pub names: Names,
}

impl GameOptions {
    /// Reads the options from `dir`; defaults if there are none.
    pub fn load(dir: &Path) -> Self {
        [FILE, OLD_FILE]
            .iter()
            .find_map(|name| {
                let data = fs::read(dir.join(name)).ok()?;
                serde_json::from_slice(&data).ok()
            })
            .unwrap_or_default()
    }

    pub fn store(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(dir.join(FILE), json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_reads_older_settings() {
        let dir = std::env::temp_dir().join(format!("vnr-adv2-options-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(GameOptions::load(&dir), GameOptions::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(OLD_FILE),
            r#"{"music": 3, "names": {"given": "優"}}"#,
        )
        .unwrap();
        assert_eq!(GameOptions::load(&dir).names.given, "優");
        let options = GameOptions {
            names: Names {
                handle: "ツカサ".into(),
                ..Names::default()
            },
        };
        options.store(&dir).unwrap();
        assert_eq!(GameOptions::load(&dir), options);
        fs::remove_dir_all(dir).unwrap();
    }
}
