//! The game state in a save.
//!
//! A save is taken while the game waits for the player (a message, a
//! choice or a click) and holds the script machine's state plus what the
//! engine shows and plays, with pictures and scripts referred to by name.
//! The interface stores it in its save slots (see `vnr_core::saves`).
//!
//! The original engine's own save files are not read or written.

use serde::{Deserialize, Serialize};

use crate::backlog::History;
use crate::names::Names;
use crate::pc::{Line, PcView};
use crate::vm::Snapshot;

/// Format version written to every save.
pub const VERSION: u32 = 1;

/// One saved game.
#[derive(Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub version: u32,
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
    /// The hero's names (missing in older saves: the defaults).
    #[serde(default)]
    pub names: Names,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SaveFile {
        SaveFile {
            version: VERSION,
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
                names: Names::default(),
                mode: SavedMode::Click,
            },
        }
    }

    #[test]
    fn round_trips_through_json() {
        let json = serde_json::to_value(sample()).unwrap();
        let back: SaveFile = serde_json::from_value(json).unwrap();
        assert_eq!(back.machine, sample().machine);
        assert_eq!(back.engine.chat_log, sample().engine.chat_log);
        assert_eq!(back.engine.mode, SavedMode::Click);
    }
}
