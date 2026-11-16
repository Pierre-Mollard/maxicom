//! Invalid bytes kept through the terminal emulator (MAX-249)
//!
//! vt100 drops the U+FFFD its parser prints for invalid UTF-8, so terminal mode would show
//! nothing where line mode shows `�`. Before vt100, each broken char (cut as
//! `String::from_utf8_lossy` cuts them, one per maximal subpart) becomes a stand-in: one char
//! that vt100 draws in one cell, shown as `�`, and that holds the broken char's bytes.
//!
//! A broken char is 1 to 3 bytes, its bytes after the first are continuation bytes
//! (`10xxxxxx`), so it fits one plane 16 private use char (plane 15 has Nerd Font icons):
//!
//! | Bytes | Stand-in |
//! | --- | --- |
//! | `a` | `BASE + a` |
//! | `a` (E0..F4) `b` | `BASE + 0x100 + ((a - E0) << 6 \| b & 3F)` |
//! | `a` (F0..F4) `b c` | `BASE + 0x1000 + ((a - F0) << 12 \| (b & 3F) << 6 \| c & 3F)` |

use std::borrow::Cow;

const BASE: u32 = 0x10_0000;
const TWO: u32 = BASE + 0x100;
const THREE: u32 = BASE + 0x1000;
/// Past the last stand-in
const END: u32 = THREE + 0x5000;

/// UTF-8 received in reads: a char cut between two reads waits for the next one
#[derive(Debug, Default)]
pub struct InvalidBytes {
    pending: [u8; 3],
    pending_len: usize,
}

impl InvalidBytes {
    /// `bytes` with each broken char as its stand-in, borrowed when there is none (the common
    /// case). A char not complete at the end waits for the next read, as the parser does
    pub fn utf8<'a>(&mut self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        if self.pending_len == 0 {
            return self.repair(bytes);
        }
        let mut joined = self.pending[..self.pending_len].to_vec();
        joined.extend_from_slice(bytes);
        self.pending_len = 0;
        Cow::Owned(self.repair(&joined).into_owned())
    }

    fn repair<'b>(&mut self, bytes: &'b [u8]) -> Cow<'b, [u8]> {
        let err = match std::str::from_utf8(bytes) {
            Ok(_) => return Cow::Borrowed(bytes),
            Err(err) => err,
        };
        // only the last char is cut: the rest goes as it is
        if err.error_len().is_none() {
            self.keep(&bytes[err.valid_up_to()..]);
            return Cow::Borrowed(&bytes[..err.valid_up_to()]);
        }
        let mut out = Vec::with_capacity(bytes.len() + 8);
        let mut rest = bytes;
        loop {
            let err = match std::str::from_utf8(rest) {
                Ok(_) => {
                    out.extend_from_slice(rest);
                    break;
                }
                Err(err) => err,
            };
            let (valid, broken) = rest.split_at(err.valid_up_to());
            out.extend_from_slice(valid);
            let Some(len) = err.error_len() else {
                self.keep(broken);
                break;
            };
            push_stand_in(&broken[..len], &mut out);
            rest = &broken[len..];
        }
        Cow::Owned(out)
    }

    fn keep(&mut self, cut: &[u8]) {
        self.pending[..cut.len()].copy_from_slice(cut);
        self.pending_len = cut.len();
    }
}

/// `bytes` received as ASCII: each byte from 80 up is a broken char of its own
pub fn ascii(bytes: &[u8]) -> Cow<'_, [u8]> {
    if bytes.is_ascii() {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len() + 8);
    for &byte in bytes {
        if byte.is_ascii() {
            out.push(byte);
        } else {
            push_stand_in(&[byte], &mut out);
        }
    }
    Cow::Owned(out)
}

/// The stand-in of a broken char, one per byte for a shape a broken char can't have (no byte
/// is lost either way)
fn push_stand_in(broken: &[u8], out: &mut Vec<u8>) {
    let mut push = |code: u32| {
        if let Some(c) = char::from_u32(code) {
            out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        }
    };
    let low = |byte: u8| u32::from(byte & 0x3F);
    let next = |byte: &u8| byte & 0xC0 == 0x80;
    match *broken {
        [a] => push(BASE + u32::from(a)),
        [a @ 0xE0..=0xF4, b] if next(&b) => push(TWO + (u32::from(a - 0xE0) << 6 | low(b))),
        [a @ 0xF0..=0xF4, b, c] if next(&b) && next(&c) => {
            push(THREE + (u32::from(a - 0xF0) << 12 | low(b) << 6 | low(c)))
        }
        _ => broken.iter().for_each(|&byte| push(BASE + u32::from(byte))),
    }
}

/// `c` is a stand-in, drawn as `�`
pub fn is_stand_in(c: char) -> bool {
    (BASE..END).contains(&u32::from(c))
}

/// `text` with its stand-ins as `�`, so a char index stays a column; borrowed when there is
/// none (every plane 16 char starts with byte F4, rare in text)
pub fn shown(text: &str) -> Cow<'_, str> {
    if !text.as_bytes().contains(&0xF4) {
        return Cow::Borrowed(text);
    }
    let replaced = text
        .chars()
        .map(|c| if is_stand_in(c) { '\u{FFFD}' } else { c });
    Cow::Owned(replaced.collect())
}

/// The bytes a stand-in holds, the bytes received (at most 3)
fn stand_in_bytes(c: char) -> Option<([u8; 3], usize)> {
    let code = u32::from(c);
    let next = |bits: u32| 0x80 | (bits & 0x3F) as u8;
    match code {
        _ if !is_stand_in(c) => None,
        THREE.. => {
            let bits = code - THREE;
            Some(([0xF0 + (bits >> 12) as u8, next(bits >> 6), next(bits)], 3))
        }
        TWO.. => {
            let bits = code - TWO;
            Some(([0xE0 + (bits >> 6) as u8, next(bits), 0], 2))
        }
        _ => Some(([(code - BASE) as u8, 0, 0], 1)),
    }
}

/// The bytes received behind `text` (a vt100 row, stand-ins in it): each stand-in as the bytes
/// it holds. With, for each byte of [`shown`]`(text)` and its end, the byte of those it comes
/// from, so a map built on the shown text goes on to the bytes received. None when `text` has
/// no stand-in
pub fn received(text: &str) -> Option<(Vec<u8>, Vec<usize>)> {
    if !text.as_bytes().contains(&0xF4) || !text.chars().any(is_stand_in) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len() + 1);
    for c in text.chars() {
        let start = bytes.len();
        match stand_in_bytes(c) {
            Some((held, len)) => {
                // shown as `�`, 3 bytes, all from the stand-in's first byte
                offsets.extend([start; 3]);
                bytes.extend_from_slice(&held[..len]);
            }
            None => {
                offsets.extend(start..start + c.len_utf8());
                bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
        }
    }
    offsets.push(bytes.len());
    Some((bytes, offsets))
}

/// [`shown`] for a `String` vt100 gave, kept when it has no stand-in
pub fn shown_string(text: String) -> String {
    match shown(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(shown) => shown,
    }
}

#[cfg(test)]
#[path = "tests/invalid_bytes.rs"]
mod tests;
