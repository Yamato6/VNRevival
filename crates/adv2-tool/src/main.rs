//! `adv2-tool`: inspect and extract OverflowADV2 pack archives.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use adv2_formats::{Entry, Pack};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the trailer and a summary of a pack.
    Info {
        /// Path to a .DAT pack.
        pack: PathBuf,
    },
    /// List the members of a pack.
    List {
        /// Path to a .DAT pack.
        pack: PathBuf,
        /// Only list members whose name contains this text (case-insensitive).
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Extract members of a pack into a folder.
    Extract {
        /// Path to a .DAT pack.
        pack: PathBuf,
        /// Output folder (created if missing).
        #[arg(short, long)]
        output: PathBuf,
        /// Only extract members whose name contains this text (case-insensitive).
        #[arg(short, long)]
        filter: Option<String>,
        /// Write members as stored, without LZSS decompression.
        #[arg(long)]
        raw: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Info { pack } => info(&pack),
        Command::List { pack, filter } => list(&pack, filter.as_deref()),
        Command::Extract {
            pack,
            output,
            filter,
            raw,
        } => extract(&pack, &output, filter.as_deref(), raw),
    }
}

fn open(path: &Path) -> Result<Pack<std::io::BufReader<fs::File>>, String> {
    Pack::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn matches(entry: &Entry, filter: Option<&str>) -> bool {
    filter.is_none_or(|f| entry.name.to_lowercase().contains(&f.to_lowercase()))
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn info(path: &Path) -> Result<(), String> {
    let pack = open(path)?;
    let trailer = pack.trailer();
    println!("pack:          {}", path.display());
    println!("entries:       {}", pack.entries().len());
    println!("data end:      {:#x}", pack.data_end());
    println!(
        "index:         {} bytes packed, {} unpacked",
        trailer.index_packed_size, trailer.index_size
    );
    println!("trailer word3: {:#010x} (unknown)", trailer.unknown);

    let mut by_ext: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for entry in pack.entries() {
        let slot = by_ext.entry(extension(&entry.name)).or_default();
        slot.0 += 1;
        slot.1 += u64::from(entry.size);
    }
    println!("by extension:");
    for (ext, (count, bytes)) in by_ext {
        println!("  .{ext:<6} {count:>6} files {bytes:>12} bytes");
    }
    Ok(())
}

fn list(path: &Path, filter: Option<&str>) -> Result<(), String> {
    let pack = open(path)?;
    println!(
        "{:<40} {:>8} {:>10} {:>10} {:>10}",
        "name", "header", "offset", "packed", "size"
    );
    for entry in pack.entries().iter().filter(|e| matches(e, filter)) {
        let header: String = entry.header.iter().map(|b| format!("{b:02x}")).collect();
        println!(
            "{:<40} {header:>8} {:>10x} {:>10} {:>10}",
            entry.name, entry.offset, entry.packed_size, entry.size
        );
    }
    Ok(())
}

/// Returns `name` if it is a plain file name that is safe to create inside
/// the output folder (no separators, no parent references, no drive).
fn safe_file_name(name: &str) -> Option<&str> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || is_windows_device_name(name);
    (!bad).then_some(name)
}

/// `CON`, `NUL`, `COM1`… (with any extension) cannot be created on Windows.
fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

fn extract(path: &Path, output: &Path, filter: Option<&str>, raw: bool) -> Result<(), String> {
    let mut pack = open(path)?;
    fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;

    let selected: Vec<Entry> = pack
        .entries()
        .iter()
        .filter(|e| matches(e, filter))
        .cloned()
        .collect();
    let mut written = 0usize;
    let mut skipped = 0usize;
    for entry in &selected {
        let Some(name) = safe_file_name(&entry.name) else {
            eprintln!("skipping unsafe member name {:?}", entry.name);
            skipped += 1;
            continue;
        };
        let data = if raw {
            pack.read_raw(entry)
        } else {
            pack.read(entry)
        }
        .map_err(|e| e.to_string())?;
        let target = output.join(name);
        fs::write(&target, data).map_err(|e| format!("{}: {e}", target.display()))?;
        written += 1;
    }
    println!(
        "extracted {written} of {} members to {}",
        selected.len(),
        output.display()
    );
    if skipped > 0 {
        println!("skipped {skipped} members with unsafe names");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn rejects_path_traversal() {
        assert_eq!(safe_file_name("start.obj"), Some("start.obj"));
        assert_eq!(safe_file_name("console.obj"), Some("console.obj"));
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", "C:x", "x\0", "NUL", "con.txt", "com1.obj",
        ] {
            assert_eq!(safe_file_name(bad), None, "{bad:?}");
        }
    }
}
