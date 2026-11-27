//! Checks against a real game install.
//!
//! Set `ADV2_GAME_DIR` to the folder that contains `PACK\` (for example the
//! Pure Mail DVD install). Without it these tests are skipped, so CI never
//! needs copyrighted data.

use std::path::PathBuf;

use adv2_formats::Pack;

fn pack_path(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("ADV2_GAME_DIR")?;
    Some(PathBuf::from(dir).join("PACK").join(name))
}

/// Expected entry counts for the Pure Mail DVD release.
const PURE_MAIL: &[(&str, usize)] = &[
    ("SCRIPT.DAT", 385),
    ("SOUND.DAT", 544),
    ("GRAPHIC.DAT", 2359),
    ("VOICE.DAT", 15429),
];

#[test]
fn pure_mail_packs_open_and_members_decode() {
    for &(name, count) in PURE_MAIL {
        let Some(path) = pack_path(name) else {
            eprintln!("ADV2_GAME_DIR not set, skipping {name}");
            return;
        };
        let mut pack = Pack::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(pack.entries().len(), count, "{name} entry count");

        // Members are stored back to back from offset 0 up to the index.
        let entries = pack.entries().to_vec();
        let mut expected_offset = 0u64;
        for entry in &entries {
            assert_eq!(
                u64::from(entry.offset),
                expected_offset,
                "{name}: {}",
                entry.name
            );
            expected_offset += u64::from(entry.packed_size);
        }
        assert_eq!(expected_offset, pack.data_end(), "{name}: data end");

        // Decode the first and last members fully.
        for entry in [&entries[0], &entries[entries.len() - 1]] {
            let data = pack.read(entry).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(data.len(), entry.size as usize, "{name}: {}", entry.name);
        }
    }
}

#[test]
fn pure_mail_every_script_decodes() {
    let Some(path) = pack_path("SCRIPT.DAT") else {
        eprintln!("ADV2_GAME_DIR not set, skipping");
        return;
    };
    let mut pack = Pack::open(&path).unwrap();
    let entries = pack.entries().to_vec();
    for entry in &entries {
        assert!(entry.is_compressed(), "{}", entry.name);
        let data = pack
            .read(entry)
            .unwrap_or_else(|e| panic!("{}: {e}", entry.name));
        assert_eq!(data.len(), entry.size as usize, "{}", entry.name);
    }
    assert!(pack.find("START.OBJ").is_some());
}
