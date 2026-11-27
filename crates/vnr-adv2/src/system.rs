//! Data kept across playthroughs: what the extras menu unlocks.
//!
//! The original keeps 4096 flags (`0x791bf8`; set and read by
//! `0x410ce0` / `0x410d00`) and writes them to `SAVE/SAVEFL99.SAV` as
//! 4096 little-endian 32-bit values. VNRevival uses the same layout in
//! its save folder (`system.sav`), so progress made with the original can
//! be imported by copying that file.
//!
//! Known ranges:
//!
//! | Flags | Meaning | Set by |
//! |---|---|---|
//! | `0xC00 + n` | scene `n` of the replay list ([`SCENES`]) unlocked | `sys d7` (`0x42af80`) |
//! | `0x129 + n` | ending `n` seen | end of `sys d3` (`0x411b74`) |
//! | `0x180`, `0x200 + 20·i + j` | gallery picture `i`, variant `j`, seen | showing it (`0x407f20`) |

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Number of flags.
pub const FLAGS: usize = 4096;

/// First flag of the scene replay list.
pub const SCENE_FLAGS: usize = 0xC00;

/// The scene replay list in the original's order (`0x44ffcc`); the
/// extras menu's entries 1–32 start these scripts.
pub const SCENES: [&str; 32] = [
    "etc00", "hiro00", "hiro01", "hiro02", "hiro03", "hiro04", "hiro05", "hiro06", "hiro07",
    "hiro08", "mika00", "mika01", "mika02", "mika03", "mika04", "mika05", "mika06", "saku00",
    "saku01", "yuzu00", "yuzu01", "tohk00", "tohk01", "tohk02", "tohk03", "nana00", "rinn00",
    "rinn01", "rinn02", "rinn03", "rinn04", "yuzu02",
];

/// The flags, with the file they are kept in.
#[derive(Debug, Clone)]
pub struct SystemData {
    flags: Vec<i32>,
    path: Option<PathBuf>,
    dirty: bool,
}

impl Default for SystemData {
    fn default() -> Self {
        Self {
            flags: vec![0; FLAGS],
            path: None,
            dirty: false,
        }
    }
}

impl SystemData {
    /// Reads `path` (missing: all flags clear). With `import`, a missing
    /// file starts from the original's `SAVEFL99.SAV` at that path.
    pub fn open(path: PathBuf, import: Option<&Path>) -> io::Result<Self> {
        let source = if path.exists() {
            Some(path.as_path())
        } else {
            import.filter(|p| p.exists())
        };
        let mut data = Self {
            path: Some(path.clone()),
            ..Self::default()
        };
        if let Some(source) = source {
            data.flags = parse(&fs::read(source)?)?;
            // An import is written to our own file on the next store.
            data.dirty = source != path;
        }
        Ok(data)
    }

    pub fn flag(&self, n: usize) -> i32 {
        self.flags.get(n).copied().unwrap_or(0)
    }

    pub fn set_flag(&mut self, n: usize, value: i32) {
        if let Some(f) = self.flags.get_mut(n)
            && *f != value
        {
            *f = value;
            self.dirty = true;
        }
    }

    /// Unlocks a scene of the replay list by script name (`sys d7`);
    /// returns whether the name is in the list.
    pub fn unlock_scene(&mut self, name: &str) -> bool {
        let name = name.trim_end_matches(".OBJ").trim_end_matches(".obj");
        match SCENES.iter().position(|s| s.eq_ignore_ascii_case(name)) {
            Some(i) => {
                self.set_flag(SCENE_FLAGS + i, 1);
                true
            }
            None => false,
        }
    }

    pub fn scene_unlocked(&self, index: usize) -> bool {
        index < SCENES.len() && self.flag(SCENE_FLAGS + index) != 0
    }

    /// Writes the flags if they changed.
    pub fn store(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes: Vec<u8> = self.flags.iter().flat_map(|v| v.to_le_bytes()).collect();
        let temp = path.with_extension("sav.tmp");
        fs::write(&temp, bytes)?;
        fs::rename(&temp, path)?;
        self.dirty = false;
        Ok(())
    }
}

/// Reads the 4096 values; shorter files leave the rest clear.
fn parse(bytes: &[u8]) -> io::Result<Vec<i32>> {
    if bytes.len() > FLAGS * 4 {
        return Err(io::Error::other(format!(
            "{} bytes is too long for a flag file",
            bytes.len()
        )));
    }
    let mut flags = vec![0; FLAGS];
    for (f, chunk) in flags.iter_mut().zip(bytes.chunks_exact(4)) {
        *f = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vnr-system-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scenes_unlock_and_persist() {
        let dir = temp("scenes");
        let path = dir.join("system.sav");
        let mut data = SystemData::open(path.clone(), None).unwrap();
        assert!(!data.scene_unlocked(10));
        assert!(data.unlock_scene("mika00.OBJ"));
        assert!(!data.unlock_scene("start"));
        data.store().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), 16384);
        let again = SystemData::open(path, None).unwrap();
        assert!(again.scene_unlocked(10));
        assert_eq!(again.flag(SCENE_FLAGS + 10), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imports_the_originals_flags() {
        let dir = temp("import");
        let original = dir.join("SAVEFL99.SAV");
        let mut bytes = vec![0u8; FLAGS * 4];
        bytes[SCENE_FLAGS * 4] = 1;
        fs::write(&original, bytes).unwrap();
        let path = dir.join("system.sav");
        let mut data = SystemData::open(path.clone(), Some(&original)).unwrap();
        assert!(data.scene_unlocked(0));
        data.store().unwrap();
        assert!(path.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
