//! VNRevival backend for OverflowADV2, the engine of Pure Mail (2001).
//!
//! Data formats are read through the `overflow-adv2` crate from VNTools-rs.
//!
//! Launching a game boots it on the script machine ([`vm`]), with the
//! engine functions implemented in [`game`].

pub mod assets;
pub mod backlog;
pub mod game;
pub mod ini;
pub mod pc;
pub mod prefs;
pub mod save;
pub mod transition;
pub mod vm;

use std::path::Path;

use overflow_adv2::Pack;
use vnr_core::{App, Backend, BackendError, GameInfo, LaunchOptions, find_ci, resolve_ci};

/// Packs the engine loads, in the order they are reported.
pub const PACKS: [&str; 4] = ["SCRIPT.DAT", "GRAPHIC.DAT", "SOUND.DAT", "VOICE.DAT"];

/// Known titles, recognised by their executable.
const TITLES: &[(&str, &str)] = &[("PMAIL.EXE", "Pure Mail")];

/// The OverflowADV2 backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Adv2;

impl Backend for Adv2 {
    fn id(&self) -> &'static str {
        "adv2"
    }

    fn name(&self) -> &'static str {
        "OverflowADV2"
    }

    fn detect(&self, dir: &Path) -> Result<Option<GameInfo>, BackendError> {
        // OF_ADV2.INI is written by every OverflowADV2 game; SCRIPT.DAT is
        // required to play. Either one marks the folder as ours.
        let has_ini = find_ci(dir, "OF_ADV2.INI").is_some();
        let Some(script) = resolve_ci(dir, "PACK/SCRIPT.DAT") else {
            if has_ini {
                return Err("OF_ADV2.INI found but PACK/SCRIPT.DAT is missing".into());
            }
            return Ok(None);
        };

        let title = TITLES
            .iter()
            .find(|(exe, _)| find_ci(dir, exe).is_some())
            .map_or("Unknown OverflowADV2 game", |(_, title)| *title)
            .to_string();

        let mut details = Vec::new();
        for name in PACKS {
            let path = if name == "SCRIPT.DAT" {
                Some(script.clone())
            } else {
                resolve_ci(dir, &format!("PACK/{name}"))
            };
            let value = match path {
                Some(path) => {
                    let pack = Pack::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    format!("{} entries", pack.entries().len())
                }
                None => "missing".to_string(),
            };
            details.push((name.to_string(), value));
        }

        Ok(Some(GameInfo {
            engine: self.id(),
            title,
            root: dir.to_path_buf(),
            details,
        }))
    }

    fn launch(
        &self,
        game: &GameInfo,
        options: &LaunchOptions,
    ) -> Result<Box<dyn App>, BackendError> {
        let mut running = game::Game::new(
            &game.root,
            game.title.clone(),
            options.entry.as_deref(),
            options.font.clone(),
        )?;
        running.set_save_dir(options.saves.clone().or_else(|| vnr_core::save_dir(game)));
        Ok(Box::new(running))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vnr-adv2-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ignores_unrelated_folders() {
        let dir = scratch_dir("empty");
        assert_eq!(Adv2.detect(&dir).unwrap(), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ini_without_scripts_is_an_error() {
        let dir = scratch_dir("ini");
        fs::write(dir.join("OF_ADV2.INI"), "[OverflowADV2]\n").unwrap();
        assert!(Adv2.detect(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    /// Runs against a real install when `ADV2_GAME_DIR` is set.
    #[test]
    fn detects_pure_mail() {
        let Some(dir) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let info = Adv2.detect(Path::new(&dir)).unwrap().expect("not detected");
        assert_eq!(info.title, "Pure Mail");
        assert_eq!(info.details[0], ("SCRIPT.DAT".into(), "385 entries".into()));
    }
}
