//! LZSS codec used by OverflowADV2.
//!
//! This mirrors the engine's `DecodeLz` routine (exported by `LOCPU.DLL`,
//! `MMX.DLL`, `SSE.DLL` and `SSE2.DLL`):
//!
//! * 4096-byte ring buffer, zero-filled, first write position `0xFEE`.
//! * One flag byte per group of eight items, read from the most significant
//!   bit down. A set bit is a back-reference, a clear bit a literal byte.
//! * A back-reference is a little-endian `u16`: the window position is
//!   `word >> 4` and the length is `(word & 0x0F) + 3`.

use thiserror::Error;

/// Size of the sliding window.
pub const WINDOW_SIZE: usize = 4096;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;
/// Position of the first byte written into the window.
pub const WINDOW_START: usize = 0xFEE;
const MIN_MATCH: usize = 3;

/// Errors produced while decoding an LZSS stream.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LzssError {
    /// The compressed stream ended before `expected` bytes were produced.
    #[error("compressed stream ended after {produced} of {expected} bytes")]
    Truncated { produced: usize, expected: usize },
}

/// The decoder's ring buffer.
struct Window {
    buf: [u8; WINDOW_SIZE],
    pos: usize,
}

impl Window {
    fn new() -> Self {
        Self {
            buf: [0; WINDOW_SIZE],
            pos: WINDOW_START,
        }
    }

    fn push(&mut self, byte: u8) {
        self.buf[self.pos] = byte;
        self.pos = (self.pos + 1) & WINDOW_MASK;
    }
}

/// Decompresses `src` into exactly `out_len` bytes.
///
/// Trailing input after `out_len` bytes have been produced is ignored, as
/// the original decoder does.
pub fn decompress(src: &[u8], out_len: usize) -> Result<Vec<u8>, LzssError> {
    let mut out = Vec::with_capacity(out_len);
    let mut window = Window::new();
    let mut input = src.iter().copied();

    'stream: while out.len() < out_len {
        let Some(flags) = input.next() else { break };
        for bit in 0..8 {
            if out.len() >= out_len {
                break 'stream;
            }
            if flags & (0x80 >> bit) != 0 {
                let (Some(lo), Some(hi)) = (input.next(), input.next()) else {
                    break 'stream;
                };
                let word = usize::from(u16::from_le_bytes([lo, hi]));
                let mut read_pos = word >> 4;
                let len = (word & 0x0F) + MIN_MATCH;
                for _ in 0..len {
                    if out.len() >= out_len {
                        break;
                    }
                    // Read before writing: overlapping copies repeat bytes,
                    // exactly like the original byte-by-byte loop.
                    let byte = window.buf[read_pos];
                    read_pos = (read_pos + 1) & WINDOW_MASK;
                    window.push(byte);
                    out.push(byte);
                }
            } else {
                let Some(byte) = input.next() else {
                    break 'stream;
                };
                window.push(byte);
                out.push(byte);
            }
        }
    }

    if out.len() < out_len {
        return Err(LzssError::Truncated {
            produced: out.len(),
            expected: out_len,
        });
    }
    Ok(out)
}

/// Encodes `data` as a valid LZSS stream made only of literals.
///
/// The output is larger than the input; it exists so tests (and tools that
/// write packs) can produce streams the engine accepts without a matcher.
pub fn encode_literals(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len().div_ceil(8));
    for chunk in data.chunks(8) {
        out.push(0x00);
        out.extend_from_slice(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_round_trip() {
        let data = b"OverflowADV2 literal stream, longer than one group";
        let packed = encode_literals(data);
        assert_eq!(decompress(&packed, data.len()).unwrap(), data);
    }

    #[test]
    fn reference_into_zeroed_window() {
        // Flag 0x80: first item is a reference to position 0, length 0+3.
        // The window starts zeroed, so this yields three zero bytes.
        let src = [0x80, 0x00, 0x00];
        assert_eq!(decompress(&src, 3).unwrap(), vec![0, 0, 0]);
    }

    #[test]
    fn overlapping_reference_repeats_bytes() {
        // Literal 'A' lands at 0xFEE; a reference to 0xFEE of length 5
        // (word = 0xFEE << 4 | 2) must repeat it as it is being written.
        let word: u16 = (0xFEE << 4) | 2;
        let [lo, hi] = word.to_le_bytes();
        let src = [0x40, b'A', lo, hi];
        assert_eq!(decompress(&src, 6).unwrap(), b"AAAAAA");
    }

    #[test]
    fn stops_at_requested_length() {
        let packed = encode_literals(b"abcdefghij");
        assert_eq!(decompress(&packed, 4).unwrap(), b"abcd");
    }

    #[test]
    fn truncated_input_is_an_error() {
        let packed = encode_literals(b"abc");
        assert_eq!(
            decompress(&packed, 10),
            Err(LzssError::Truncated {
                produced: 3,
                expected: 10
            })
        );
        // A reference cut in half.
        assert!(decompress(&[0x80, 0x00], 3).is_err());
    }
}
