//! `.DAT` pack archives (`GRAPHIC.DAT`, `SCRIPT.DAT`, `SOUND.DAT`,
//! `VOICE.DAT`).
//!
//! Layout, from the start of the file:
//!
//! ```text
//! [member data ........................][index (LZSS)][trailer, 12 bytes]
//! ```
//!
//! * The trailer is three little-endian `u32` values, every byte XORed with
//!   `0xF0`: packed index size, unpacked index size, and an unknown value
//!   (possibly a checksum).
//! * The index sits right before the trailer. Once decompressed it is an
//!   array of 80-byte entries (see [`Entry`]).
//! * Member data starts at offset 0 and members are stored back to back.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use thiserror::Error;

use crate::lzss::{self, LzssError};

/// Size of the encrypted trailer at the end of a pack.
pub const TRAILER_SIZE: u64 = 12;
/// Key every trailer byte is XORed with.
pub const TRAILER_XOR: u8 = 0xF0;
/// Size of one decompressed index entry.
pub const ENTRY_SIZE: usize = 80;
const HEADER_SIZE: usize = 4;
const NAME_SIZE: usize = 64;
/// Upper bound for the unpacked index, far above any shipped pack
/// (VOICE.DAT has 15 429 entries, about 1.2 MiB).
const MAX_INDEX_SIZE: u32 = 64 << 20;
/// Header byte that marks an LZSS-compressed member.
const HEADER_COMPRESSED: usize = 2;

/// Errors produced while reading a pack.
#[derive(Debug, Error)]
pub enum PackError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("file is too small to hold a pack trailer ({0} bytes)")]
    TooSmall(u64),
    #[error("index size {packed} (packed) does not fit in a {file_size}-byte file")]
    IndexOutOfBounds { packed: u32, file_size: u64 },
    #[error("unpacked index size {0} is not a multiple of {ENTRY_SIZE} or is too large")]
    BadIndexSize(u32),
    #[error("index could not be decompressed: {0}")]
    Index(#[source] LzssError),
    #[error("entry {index} ({name:?}) points outside the data area")]
    EntryOutOfBounds { index: usize, name: String },
    #[error("entry {index} ({name:?}) is stored uncompressed but sizes differ")]
    InconsistentEntry { index: usize, name: String },
    #[error("member {name:?} could not be decompressed: {source}")]
    Member {
        name: String,
        #[source]
        source: LzssError,
    },
}

/// The decoded 12-byte trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trailer {
    /// Size of the LZSS-compressed index.
    pub index_packed_size: u32,
    /// Size of the index once decompressed.
    pub index_size: u32,
    /// Meaning not known yet; differs for every pack.
    pub unknown: u32,
}

impl Trailer {
    /// Decodes a raw (still XORed) trailer.
    pub fn decode(raw: [u8; TRAILER_SIZE as usize]) -> Self {
        let plain = raw.map(|b| b ^ TRAILER_XOR);
        let word =
            |i: usize| u32::from_le_bytes([plain[i], plain[i + 1], plain[i + 2], plain[i + 3]]);
        Self {
            index_packed_size: word(0),
            index_size: word(4),
            unknown: word(8),
        }
    }
}

/// One member of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File name, decoded from Shift-JIS.
    pub name: String,
    /// The four bytes in front of the name. Byte 2 is the compression flag;
    /// the others are not understood yet (scripts use bytes 0, 1 and 3).
    pub header: [u8; HEADER_SIZE],
    /// Offset of the member data from the start of the pack.
    pub offset: u32,
    /// Size of the member as stored in the pack.
    pub packed_size: u32,
    /// Size of the member once decompressed.
    pub size: u32,
}

impl Entry {
    /// Whether the member is LZSS-compressed.
    pub fn is_compressed(&self) -> bool {
        self.header[HEADER_COMPRESSED] != 0
    }

    fn parse(raw: &[u8]) -> Self {
        let word = |i: usize| u32::from_le_bytes([raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]);
        let name_field = &raw[HEADER_SIZE..HEADER_SIZE + NAME_SIZE];
        let name_len = name_field.iter().position(|&b| b == 0).unwrap_or(NAME_SIZE);
        let (name, _, _) = encoding_rs::SHIFT_JIS.decode(&name_field[..name_len]);
        let fields = HEADER_SIZE + NAME_SIZE;
        Self {
            name: name.into_owned(),
            header: [raw[0], raw[1], raw[2], raw[3]],
            offset: word(fields),
            packed_size: word(fields + 4),
            size: word(fields + 8),
        }
    }
}

/// An open pack archive.
#[derive(Debug)]
pub struct Pack<R> {
    reader: R,
    trailer: Trailer,
    data_end: u64,
    entries: Vec<Entry>,
}

impl Pack<BufReader<File>> {
    /// Opens the pack at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PackError> {
        Self::from_reader(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Pack<R> {
    /// Reads the trailer and index from `reader`.
    pub fn from_reader(mut reader: R) -> Result<Self, PackError> {
        let file_size = reader.seek(SeekFrom::End(0))?;
        if file_size < TRAILER_SIZE {
            return Err(PackError::TooSmall(file_size));
        }

        let mut raw = [0u8; TRAILER_SIZE as usize];
        reader.seek(SeekFrom::Start(file_size - TRAILER_SIZE))?;
        reader.read_exact(&mut raw)?;
        let trailer = Trailer::decode(raw);

        let packed = u64::from(trailer.index_packed_size);
        let data_end =
            (file_size - TRAILER_SIZE)
                .checked_sub(packed)
                .ok_or(PackError::IndexOutOfBounds {
                    packed: trailer.index_packed_size,
                    file_size,
                })?;
        if trailer.index_size > MAX_INDEX_SIZE || trailer.index_size as usize % ENTRY_SIZE != 0 {
            return Err(PackError::BadIndexSize(trailer.index_size));
        }

        let mut packed_index = vec![0u8; trailer.index_packed_size as usize];
        reader.seek(SeekFrom::Start(data_end))?;
        reader.read_exact(&mut packed_index)?;
        let index = lzss::decompress(&packed_index, trailer.index_size as usize)
            .map_err(PackError::Index)?;

        let entries = index
            .chunks_exact(ENTRY_SIZE)
            .map(Entry::parse)
            .collect::<Vec<_>>();
        for (index, entry) in entries.iter().enumerate() {
            let end = u64::from(entry.offset) + u64::from(entry.packed_size);
            if end > data_end {
                return Err(PackError::EntryOutOfBounds {
                    index,
                    name: entry.name.clone(),
                });
            }
            if !entry.is_compressed() && entry.packed_size != entry.size {
                return Err(PackError::InconsistentEntry {
                    index,
                    name: entry.name.clone(),
                });
            }
        }

        Ok(Self {
            reader,
            trailer,
            data_end,
            entries,
        })
    }

    /// The decoded trailer.
    pub fn trailer(&self) -> Trailer {
        self.trailer
    }

    /// Offset where member data ends and the index begins.
    pub fn data_end(&self) -> u64 {
        self.data_end
    }

    /// All members, in index order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Finds a member by name, ignoring ASCII case (the shipped indexes mix
    /// `se18.ADP4` and `ase.anim`).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// Reads a member exactly as stored.
    pub fn read_raw(&mut self, entry: &Entry) -> Result<Vec<u8>, PackError> {
        let mut buf = vec![0u8; entry.packed_size as usize];
        self.reader.seek(SeekFrom::Start(u64::from(entry.offset)))?;
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Reads a member and decompresses it when needed.
    pub fn read(&mut self, entry: &Entry) -> Result<Vec<u8>, PackError> {
        let raw = self.read_raw(entry)?;
        if !entry.is_compressed() {
            return Ok(raw);
        }
        lzss::decompress(&raw, entry.size as usize).map_err(|source| PackError::Member {
            name: entry.name.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Builds one raw 80-byte index entry.
    fn entry(
        name: &str,
        compressed: bool,
        offset: u32,
        packed: u32,
        size: u32,
    ) -> [u8; ENTRY_SIZE] {
        let mut raw = [0u8; ENTRY_SIZE];
        raw[HEADER_COMPRESSED] = u8::from(compressed);
        raw[HEADER_SIZE..HEADER_SIZE + name.len()].copy_from_slice(name.as_bytes());
        let fields = HEADER_SIZE + NAME_SIZE;
        raw[fields..fields + 4].copy_from_slice(&offset.to_le_bytes());
        raw[fields + 4..fields + 8].copy_from_slice(&packed.to_le_bytes());
        raw[fields + 8..fields + 12].copy_from_slice(&size.to_le_bytes());
        raw
    }

    /// Appends a literal-encoded index and an XORed trailer to `data`.
    fn assemble(data: &[u8], index: &[u8], unknown: u32) -> Vec<u8> {
        let packed_index = lzss::encode_literals(index);
        let mut file = data.to_vec();
        file.extend_from_slice(&packed_index);
        let words = [packed_index.len() as u32, index.len() as u32, unknown];
        file.extend(
            words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .map(|b| b ^ TRAILER_XOR),
        );
        file
    }

    fn sample() -> Vec<u8> {
        let first = b"compressed member payload";
        let first_stored = lzss::encode_literals(first);
        let second = b"stored member";
        let mut data = first_stored.clone();
        data.extend_from_slice(second);
        let mut index = Vec::new();
        index.extend_from_slice(&entry(
            "start.obj",
            true,
            0,
            first_stored.len() as u32,
            first.len() as u32,
        ));
        index.extend_from_slice(&entry(
            "se18.ADP4",
            false,
            first_stored.len() as u32,
            second.len() as u32,
            second.len() as u32,
        ));
        assemble(&data, &index, 0x1234_5678)
    }

    #[test]
    fn reads_index_and_members() {
        let mut pack = Pack::from_reader(Cursor::new(sample())).unwrap();
        assert_eq!(pack.trailer().unknown, 0x1234_5678);
        let names: Vec<_> = pack.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["start.obj", "se18.ADP4"]);

        let first = pack.entries()[0].clone();
        assert!(first.is_compressed());
        assert_eq!(pack.read(&first).unwrap(), b"compressed member payload");

        let second = pack.find("SE18.adp4").unwrap().clone();
        assert!(!second.is_compressed());
        assert_eq!(pack.read(&second).unwrap(), b"stored member");
    }

    #[test]
    fn rejects_tiny_file() {
        assert!(matches!(
            Pack::from_reader(Cursor::new(vec![0u8; 5])),
            Err(PackError::TooSmall(5))
        ));
    }

    #[test]
    fn rejects_index_larger_than_file() {
        let mut file = sample();
        let len = file.len();
        // Packed index size becomes 0x0FFF_FF00 once XORed back.
        file[len - 12..len - 8].copy_from_slice(&[0xF0, 0x0F, 0x0F, 0xFF]);
        assert!(matches!(
            Pack::from_reader(Cursor::new(file)),
            Err(PackError::IndexOutOfBounds { .. })
        ));
    }

    #[test]
    fn rejects_bad_index_size() {
        let index = entry("x.obj", false, 0, 0, 0);
        let file = assemble(&[], &index[..ENTRY_SIZE - 1], 0);
        assert!(matches!(
            Pack::from_reader(Cursor::new(file)),
            Err(PackError::BadIndexSize(79))
        ));
    }

    #[test]
    fn rejects_entry_past_data_area() {
        let file = assemble(b"abc", &entry("x.obj", false, 0, 1000, 1000), 0);
        assert!(matches!(
            Pack::from_reader(Cursor::new(file)),
            Err(PackError::EntryOutOfBounds { index: 0, .. })
        ));
    }

    #[test]
    fn rejects_stored_entry_with_mismatched_sizes() {
        let file = assemble(b"abc", &entry("x.obj", false, 0, 3, 9), 0);
        assert!(matches!(
            Pack::from_reader(Cursor::new(file)),
            Err(PackError::InconsistentEntry { index: 0, .. })
        ));
    }
}
