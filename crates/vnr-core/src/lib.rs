//! Engine-independent core of VNRevival.
//!
//! Every original engine (OverflowADV2, …) is supported by a *backend*: a
//! crate that implements [`Backend`] and turns that engine's data into
//! what the shared runtime needs. The runtime itself (window, rendering,
//! audio, text, input, saves) will live here as it is written.

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
