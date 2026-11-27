//! Options of Pure Mail itself, kept as `options.json` in the game's data
//! folder: the hero's names for new games (the original asks for them
//! in a dialog at `sys 17`; so does VNRevival, through the interface).

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vnr_core::{GameOption, Language};

use crate::names::Names;

/// The fields, as `(id, most characters, English, Spanish, Japanese)`.
const FIELDS: [(&str, usize, &str, &str, &str); 5] = [
    ("surname", 6, "Surname", "Apellido", "姓"),
    (
        "surname_reading",
        12,
        "Surname (reading)",
        "Apellido (lectura)",
        "姓（よみ）",
    ),
    ("given", 6, "Given name", "Nombre", "名"),
    (
        "given_reading",
        12,
        "Given name (reading)",
        "Nombre (lectura)",
        "名（よみ）",
    ),
    ("handle", 10, "Chat handle", "Nick del chat", "ハンドル名"),
];

/// Why a field matters, for the ones that change the voices.
fn voice_hint(language: Language) -> &'static str {
    match language {
        Language::English => {
            "Voices call the hero by name only while both readings and the handle are the original ones."
        }
        Language::Spanish => {
            "Las voces dicen el nombre del protagonista solo si las dos lecturas y el nick son los originales."
        }
        Language::Japanese => {
            "よみとハンドル名が初期値のときだけ、ボイスが主人公の名前を呼びます。"
        }
    }
}

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

    fn field_mut<'a>(names: &'a mut Names, id: &str) -> Option<&'a mut String> {
        Some(match id {
            "surname" => &mut names.surname,
            "surname_reading" => &mut names.surname_reading,
            "given" => &mut names.given,
            "given_reading" => &mut names.given_reading,
            "handle" => &mut names.handle,
            _ => return None,
        })
    }

    /// The options as the interface shows them.
    pub fn fields(&self, language: Language) -> Vec<GameOption> {
        let mut names = self.names.clone();
        let mut defaults = Names::default();
        FIELDS
            .iter()
            .map(|&(id, max_chars, en, es, ja)| GameOption {
                id: id.into(),
                label: match language {
                    Language::English => en,
                    Language::Spanish => es,
                    Language::Japanese => ja,
                }
                .into(),
                value: Self::field_mut(&mut names, id).cloned().unwrap_or_default(),
                default: Self::field_mut(&mut defaults, id)
                    .cloned()
                    .unwrap_or_default(),
                max_chars,
                hint: matches!(id, "surname_reading" | "given_reading" | "handle")
                    .then(|| voice_hint(language).to_string()),
            })
            .collect()
    }

    /// Changes field `id`.
    pub fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        let &(_, max_chars, ..) = FIELDS
            .iter()
            .find(|f| f.0 == id)
            .ok_or_else(|| format!("unknown option {id}"))?;
        if value.is_empty() {
            return Err("it cannot be empty".into());
        }
        if value.chars().count() > max_chars {
            return Err(format!("at most {max_chars} characters"));
        }
        // `$` starts a placeholder in the texts.
        if value.contains('$') {
            return Err("it cannot contain $".into());
        }
        if let Some(field) = Self::field_mut(&mut self.names, id) {
            *field = value.to_string();
        }
        Ok(())
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

    #[test]
    fn fields_edit_the_names() {
        let mut options = GameOptions::default();
        let fields = options.fields(Language::Spanish);
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0].label, "Apellido");
        assert_eq!(
            (fields[2].value.as_str(), fields[2].default.as_str()),
            ("圭", "圭")
        );
        assert!(fields[0].hint.is_none() && fields[1].hint.is_some());

        options.set("given", " 優 ").unwrap();
        options.set("given_reading", "ユウ").unwrap();
        assert_eq!(options.names.given, "優");
        assert_eq!(options.fields(Language::English)[2].value, "優");
        assert!(!options.names.voiced());
        assert!(options.set("given", "").is_err());
        assert!(options.set("given", "あいうえおかき").is_err());
        assert!(options.set("handle", "$N0").is_err());
        assert!(options.set("nope", "x").is_err());
        assert_eq!(options.names.given, "優");
    }
}
