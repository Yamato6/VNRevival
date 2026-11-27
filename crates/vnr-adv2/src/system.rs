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
//! | `0x180 + i`, `0x200 + 20·i + j` | gallery picture `i` (below 128) and its variant `j` seen | `sys 30` showing it (`0x408bc0`) |

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

/// The gallery: one entry per picture, listing its variants (table at
/// `0x43a11c`). Showing any of these pictures marks it seen.
pub const GALLERY: [&[&str]; 129] = [
    &["OPEN01", "OPEN01_1"],
    &["OPEN04"],
    &["OPEN05"],
    &["OPEN07"],
    &["saku13"],
    &["MASI01"],
    &["ENDI01"],
    &["hiro01"],
    &[
        "hiro11_11",
        "hiro11_12",
        "hiro11_13",
        "hiro11_21",
        "hiro11_22",
        "hiro11_23",
    ],
    &[
        "hiro09_11",
        "hiro09_12",
        "hiro09_13",
        "hiro09_21",
        "hiro09_22",
        "hiro09_23",
    ],
    &[
        "hiro09_31",
        "hiro09_32",
        "hiro09_33",
        "hiro09_41",
        "hiro09_42",
        "hiro09_43",
    ],
    &["hiro10_1", "hiro10_2"],
    &["hiro24_1", "hiro24_2"],
    &["hiro12_21", "hiro12_22", "hiro12_23"],
    &["hiro13_11", "hiro13_12", "hiro13_13"],
    &["hiro14_11", "hiro14_12", "hiro14_13"],
    &["hiro16_21", "hiro16_22"],
    &["hiro17"],
    &["hiro02", "hiro02_1"],
    &["hiro04_1", "hiro04_2"],
    &["hiro03_1", "hiro03_2"],
    &["hiro19"],
    &["hiro20_1", "hiro20_2"],
    &["hiro27"],
    &["hiro13_21", "hiro13_22", "hiro13_23"],
    &["hiro14_21", "hiro14_22", "hiro14_23"],
    &["hiro12_11", "hiro12_12", "hiro12_13"],
    &["hiro21"],
    &["hiro22_1", "hiro22_2"],
    &["hiro16_11", "hiro16_12"],
    &["hiro13_51", "hiro13_52", "hiro13_53"],
    &["hiro28"],
    &["OPEN06"],
    &["hiro19_1", "hiro19_2"],
    &["hiro22_3"],
    &["hiro15_1", "hiro15_2"],
    &["hiro23_2", "hiro23_3", "hiro23_4"],
    &["ENDI04"],
    &["ENDI02"],
    &["ENDI03"],
    &["ENDI16"],
    &["hiro26"],
    &["hiro25_1", "hiro25_2", "hiro25_3"],
    &["mika01", "mika01_1"],
    &["mika02_1", "mika02_2"],
    &["mika03"],
    &["mika11_1", "mika11_2"],
    &["mika12"],
    &["mika13_1", "mika13_2"],
    &["mika10"],
    &["mika04"],
    &["mika05_1", "mika05_2", "mika05_9"],
    &["mika04_1"],
    &["mika05_7", "mika05_8", "mika05_12"],
    &["mika07_11", "mika07_12", "mika07_21", "mika07_22"],
    &["mika08"],
    &["mika09_3", "mika09_4", "mika09_1", "mika09_2"],
    &["mika14", "mika14_1", "mika14_2"],
    &["mika15"],
    &["ENDI13"],
    &["ENDI12"],
    &["saku10_11"],
    &["SAKU01"],
    &[
        "saku09_1", "saku09_2", "saku09_3", "saku09_4", "saku09_5", "saku09_6",
    ],
    &["saku02_11", "saku02_12", "saku02_13"],
    &["saku11_1", "saku11_2"],
    &["saku07_1", "saku07_2"],
    &["SAKU03"],
    &["SAKU04"],
    &["saku05_11", "saku05_12"],
    &["saku02_2"],
    &["saku14_1", "saku14_2"],
    &["saku05_21", "saku05_22", "saku05_31", "saku05_32"],
    &["saku06_1", "saku06_2", "saku06_3"],
    &["saku12_1", "saku12_2"],
    &["saku08_11", "saku08_12", "saku08_13", "saku08_2"],
    &["ENDI11"],
    &["OPEN02_1", "OPEN02_2"],
    &["yuzu02_3"],
    &["yuzu02_1", "yuzu02_2"],
    &["yuzu10_1", "yuzu10_2"],
    &["YUZU01"],
    &[
        "yuzu09_41",
        "yuzu09_42",
        "yuzu09_51",
        "yuzu09_61",
        "yuzu09_62",
    ],
    &[
        "yuzu09_11",
        "yuzu09_12",
        "yuzu09_21",
        "yuzu09_22",
        "yuzu09_31",
        "yuzu09_32",
    ],
    &["yuzu03_1", "yuzu03_21", "yuzu03_22"],
    &["YUZU04"],
    &[
        "yuzu05_41",
        "yuzu05_42",
        "yuzu05_31",
        "yuzu05_32",
        "yuzu05_21",
        "yuzu05_22",
        "yuzu05_11",
        "yuzu05_12",
    ],
    &["yuzu06_11", "yuzu06_12", "yuzu06_21", "yuzu06_22"],
    &["yuzu07_11", "yuzu07_12", "yuzu07_21", "yuzu07_22"],
    &["YUZU08"],
    &["ENDI09"],
    &["ENDI08"],
    &["tohk10_1", "tohk10_2"],
    &["tohk02_1", "tohk02_2"],
    &["tohk11"],
    &["tohk03_1", "tohk03_2"],
    &["tohk01"],
    &["tohk09_11", "tohk09_21", "tohk09_22"],
    &["tohk04_1", "tohk04_21", "tohk04_22"],
    &["tohk05_1", "tohk05_2", "tohk05_3"],
    &["tohk06_1", "tohk06_2"],
    &["tohk12", "tohk12_1", "tohk12_2"],
    &["tohk07_1", "tohk07_2", "tohk07_22", "tohk07_3"],
    &[
        "tohk08_11",
        "tohk08_12",
        "tohk08_21",
        "tohk08_22",
        "tohk08_31",
        "tohk08_32",
    ],
    &["ENDI10"],
    &["OPEN03_1"],
    &["nana01_1", "nana01_2"],
    &["nana02"],
    &[
        "nana03_11",
        "nana03_12",
        "nana03_21",
        "nana03_22",
        "nana03_4",
    ],
    &["nana06"],
    &["nana04"],
    &["nana05_1", "nana05_2", "nana05_21"],
    &["ENDI07"],
    &["rinn01"],
    &["RINN02_2", "RINN02_1"],
    &["RINN06_1", "RINN06_2", "RINN06_3"],
    &["RINN05_1", "RINN05_2"],
    &["RINN12_1", "RINN12_2"],
    &["RINN07_1", "RINN07"],
    &["RINN03"],
    &["RINN13_1", "RINN13_2"],
    &["RINN10_1", "RINN10_2"],
    &["RINN08_1", "RINN08_2", "RINN08_3"],
    &["RINN09_11", "RINN09_12", "RINN09_21", "RINN09_22"],
    &["RINN11"],
    &["ENDI14"],
    &["ENDI15"],
    &["hiro29"],
    &["hiro30_1", "hiro30_2", "hiro30_3"],
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

    /// Marks a shown picture in the gallery (`0x408bc0`); returns
    /// whether it is a gallery picture.
    pub fn see_picture(&mut self, name: &str) -> bool {
        // The engine files one picture under its first variant.
        let name = if name.eq_ignore_ascii_case("RINN02") {
            "RINN02_1"
        } else {
            name
        };
        for (i, variants) in GALLERY.iter().enumerate() {
            if let Some(k) = variants.iter().position(|v| v.eq_ignore_ascii_case(name)) {
                self.set_flag(picture_flag(i), 1);
                self.set_flag(variant_flag(i, k + 1), 1);
                return true;
            }
        }
        false
    }

    /// Whether gallery picture `i` has been seen.
    pub fn picture_seen(&self, i: usize) -> bool {
        self.flag(picture_flag(i)) != 0
    }

    /// Whether variant `k` (1-based) of gallery picture `i` has been seen.
    pub fn variant_seen(&self, i: usize, k: usize) -> bool {
        self.flag(variant_flag(i, k)) != 0
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

/// Flag of gallery picture `i` (`0x407f70`).
fn picture_flag(i: usize) -> usize {
    if i < 0x80 { 0x180 + i } else { 0x200 + i * 21 }
}

/// Flag of variant `k` of gallery picture `i` (`0x407f20`).
fn variant_flag(i: usize, k: usize) -> usize {
    if i < 0x80 {
        0x200 + i * 20 + k
    } else {
        0x201 + i * 21 + k
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
    fn pictures_are_marked_seen() {
        let mut data = SystemData::default();
        assert!(data.see_picture("hiro09_12"));
        assert!(data.picture_seen(9));
        assert!(data.variant_seen(9, 2));
        assert!(!data.variant_seen(9, 1));
        assert_eq!(data.flag(0x180 + 9), 1);
        assert_eq!(data.flag(0x200 + 9 * 20 + 2), 1);
        assert!(data.see_picture("rinn02"));
        assert!(data.variant_seen(114, 2));
        assert!(!data.see_picture("BG001"));
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
