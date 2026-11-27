//! Readers for the data formats used by the OverflowADV2 engine
//! (Pure Mail and other early titles built on it).
//!
//! Format notes live in `docs/FORMATS.md` at the repository root.

pub mod lzss;
pub mod pack;

pub use pack::{Entry, Pack, PackError};
