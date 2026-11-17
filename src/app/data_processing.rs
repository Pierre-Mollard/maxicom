// Process what data would look like processed by the ui (simplified)

use std::borrow::Cow;

use unicode_width::UnicodeWidthChar;

use crate::{app::types::LogEntry, configs::LineDisplayMode};

/// Digits of every byte value packed one after another ("00" "01" .. "FF" for hex),
/// built at compile time so formatting a byte is a slice lookup, no allocation.
/// Slices are `&'static str`, so a `Span` can borrow them instead of owning a `String`
/// per byte per frame (what `const_hex::Buffer` + `to_string` forced before).
/// One routine for every radix: `bits_per_digit` is 4 (hex), 3 (octal) or 1 (binary)
/// and `width` the digits per byte, each digit is a shift + mask into `DIGITS`.
/// -Claude (mon gars sure)
const fn byte_digits_table<const LEN: usize>(width: usize, bits_per_digit: usize) -> [u8; LEN] {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mask = (1 << bits_per_digit) - 1;
    let mut table = [0u8; LEN];
    let mut byte = 0;
    while byte < 256 {
        let mut digit = 0;
        while digit < width {
            let shift = bits_per_digit * (width - 1 - digit);
            table[byte * width + digit] = DIGITS[(byte >> shift) & mask];
            digit += 1;
        }
        byte += 1;
    }
    table
}

const fn table_as_str(table: &'static [u8]) -> &'static str {
    match std::str::from_utf8(table) {
        Ok(s) => s,
        Err(_) => panic!("byte digits table is not ascii"),
    }
}

static HEX_TABLE: [u8; 512] = byte_digits_table(2, 4);
static OCTAL_TABLE: [u8; 768] = byte_digits_table(3, 3);
static BINARY_TABLE: [u8; 2048] = byte_digits_table(8, 1);
static HEX_DIGITS: &str = table_as_str(&HEX_TABLE);
static OCTAL_DIGITS: &str = table_as_str(&OCTAL_TABLE);
static BINARY_DIGITS: &str = table_as_str(&BINARY_TABLE);

/// "00".."FF"
#[inline]
pub fn hex_byte_str(b: u8) -> &'static str {
    &HEX_DIGITS[b as usize * 2..b as usize * 2 + 2]
}

/// "000".."377"
#[inline]
pub fn octal_byte_str(b: u8) -> &'static str {
    &OCTAL_DIGITS[b as usize * 3..b as usize * 3 + 3]
}

/// "00000000".."11111111"
#[inline]
pub fn binary_byte_str(b: u8) -> &'static str {
    &BINARY_DIGITS[b as usize * 8..b as usize * 8 + 8]
}

/// Space separated digits of each byte, in one allocation
fn join_byte_digits(bytes: &[u8], width: usize, digits_of: fn(u8) -> &'static str) -> String {
    let mut out = String::with_capacity(bytes.len() * (width + 1));
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(digits_of(b));
    }
    out
}

/// Columns between two tab stops in Pretty/Clean lines, like a terminal
const TAB_STOP: usize = 8;
const TAB_SPACES: &str = "        ";

/// What [`walk_visible_line`] reports, in line order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Text shown on screen, and where its bytes come from
    Shown(&'a str, Source),
    /// An SGR sequence (`ESC[..m`): not shown, only the render uses it for colors
    Sgr(&'a str),
}

/// Where a shown piece comes from in the line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Text copied from the line: shown byte `k` is source byte `start + k`
    Copied(usize),
    /// Spaces a tab expands to: every shown byte is the tab byte
    Tab(usize),
}

/// One pass over a Pretty/Clean line deciding what is shown, so the search/copy text, the raw
/// panel map and the render can't disagree (#25):
/// - the trailing EOL is dropped, and a mid-line CR hides the text before it (what a terminal
///   ends up showing for progress bars); SGR before it still applies
/// - SGR sequences are reported as [`Piece::Sgr`]; every other escape sequence
///   (CSI, OSC, DCS/APC/PM/SOS, 2-byte) and control char (C0, DEL, C1) is dropped, as ratatui
///   would not draw them anyway
/// - a tab becomes spaces up to the next `TAB_STOP` column, counted from the line start
///
/// Every shown piece is reported as [`Piece::Shown`] with where its bytes come from
pub fn walk_visible_line<'a>(text: &'a str, mut on_piece: impl FnMut(Piece<'a>)) {
    fn flush<'a>(
        line: &'a str,
        run: &mut Option<usize>,
        end: usize,
        on_piece: &mut impl FnMut(Piece<'a>),
    ) {
        if let Some(start) = run.take()
            && start < end
        {
            on_piece(Piece::Shown(&line[start..end], Source::Copied(start)));
        }
    }

    let line = text.trim_end_matches(['\r', '\n']);
    let shown_from = after_last_cr(line.as_bytes());
    let bytes = line.as_bytes();
    let mut run = None;
    let mut col = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B {
            flush(line, &mut run, i, &mut on_piece);
            let (end, is_sgr) = escape_sequence_end(bytes, i);
            if is_sgr {
                on_piece(Piece::Sgr(&line[i..end]));
            }
            i = end;
            continue;
        }
        // `i` is always on a char boundary: escape sequences end on ASCII bytes
        let Some(ch) = line[i..].chars().next() else {
            break;
        };
        if i < shown_from {
            // overwritten by the text after a later CR
        } else if ch == '\t' {
            flush(line, &mut run, i, &mut on_piece);
            let spaces = TAB_STOP - col % TAB_STOP;
            on_piece(Piece::Shown(&TAB_SPACES[..spaces], Source::Tab(i)));
            col += spaces;
        } else if ch.is_control() {
            flush(line, &mut run, i, &mut on_piece);
        } else {
            run.get_or_insert(i);
            col += ch.width().unwrap_or(0);
        }
        i += ch.len_utf8();
    }
    flush(line, &mut run, bytes.len(), &mut on_piece);
}

/// End of the escape sequence starting at the ESC at `start`, and whether it is SGR (`ESC[..m`).
/// Like a terminal, a new ESC aborts an unfinished sequence and is left for the next one
fn escape_sequence_end(bytes: &[u8], start: usize) -> (usize, bool) {
    let Some(&kind) = bytes.get(start + 1) else {
        return (start + 1, false);
    };
    match kind {
        // CSI: parameter and intermediate bytes, then a final byte. An unfinished one runs to the
        // end of the line; only a well formed one ending in `m` is SGR
        b'[' => {
            let mut well_formed = true;
            let mut i = start + 2;
            while i < bytes.len() {
                let b = bytes[i];
                if b == 0x1B {
                    return (i, false);
                }
                i += 1;
                if (0x40..=0x7E).contains(&b) {
                    return (i, well_formed && b == b'm');
                }
                well_formed &= (0x20..=0x3F).contains(&b);
            }
            (bytes.len(), false)
        }
        // OSC ends at BEL or ST (`ESC \\`), DCS / SOS / PM / APC at ST
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let mut i = start + 2;
            while i < bytes.len() {
                match bytes[i] {
                    0x07 if kind == b']' => return (i + 1, false),
                    0x1B if bytes.get(i + 1) == Some(&b'\\') => return (i + 2, false),
                    0x1B => return (i, false),
                    _ => i += 1,
                }
            }
            (bytes.len(), false)
        }
        // intermediate bytes, then a final byte (charset `ESC ( B` sent by `tput sgr0`, `ESC # 8`)
        0x20..=0x2F => {
            let mut i = start + 1;
            while i < bytes.len() && (0x20..=0x2F).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() && (0x30..=0x7E).contains(&bytes[i]) {
                i += 1;
            }
            (i, false)
        }
        // 2-byte escapes (DECSC `7`, DECKPAM `=`, RIS `c`...)
        0x30..=0x7E => (start + 2, false),
        // a lone ESC before a control or non-ASCII char: only the ESC goes
        _ => (start + 1, false),
    }
}

/// Byte after the last CR that is not inside an escape sequence (a title can hold one), 0 when
/// there is none
fn after_last_cr(bytes: &[u8]) -> usize {
    if memchr::memchr(b'\r', bytes).is_none() {
        return 0;
    }
    let mut after = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1B => i = escape_sequence_end(bytes, i).0,
            b'\r' => {
                i += 1;
                after = i;
            }
            _ => i += 1,
        }
    }
    after
}

/// The line without its trailing EOL when Pretty/Clean have nothing to clean in it: no control
/// char (C0, so no ESC / tab / CR, DEL or C1). Its shown text is then the line itself, borrowed,
/// and its map the identity
pub fn plain_visible_line(text: &str) -> Option<&str> {
    let line = text.trim_end_matches(['\r', '\n']);
    let bytes = line.as_bytes();
    let has_control = bytes.iter().enumerate().any(|(i, &b)| {
        b < 0x20 || b == 0x7F || (b == 0xC2 && matches!(bytes.get(i + 1), Some(0x80..=0x9F)))
    });
    (!has_control).then_some(line)
}

pub fn displayable_line_string(text: &str, mode: LineDisplayMode) -> Cow<'_, str> {
    match mode {
        LineDisplayMode::Pretty | LineDisplayMode::Clean => {
            if let Some(line) = plain_visible_line(text) {
                return Cow::Borrowed(line);
            }
            let mut shown = String::with_capacity(text.len());
            walk_visible_line(text, |piece| {
                if let Piece::Shown(piece, _) = piece {
                    shown.push_str(piece);
                }
            });
            Cow::Owned(shown)
        }
        LineDisplayMode::Raw => {
            let mut raw = String::new();
            for ch in text.chars() {
                match ch {
                    '\x1b' => raw.push_str("^["),
                    '\r' => raw.push_str("\\r"),
                    '\t' => raw.push_str("    "),
                    c if c.is_control() => raw.push_str(&format!("\\x{:02X}", c as u32)),
                    c => raw.push(c),
                }
            }
            Cow::Owned(raw)
        }
        LineDisplayMode::Hex => Cow::Owned(join_byte_digits(text.as_bytes(), 2, hex_byte_str)),
        LineDisplayMode::Octal => Cow::Owned(join_byte_digits(text.as_bytes(), 3, octal_byte_str)),
        LineDisplayMode::Binary => {
            Cow::Owned(join_byte_digits(text.as_bytes(), 8, binary_byte_str))
        }
    }
}

/// Display text of a history entry: the byte modes (Hex, Octal, Binary) show the bytes received,
/// the others the decoded text (#20)
pub fn entry_display_text(entry: &LogEntry, mode: LineDisplayMode) -> Cow<'_, str> {
    let bytes = entry.wire_bytes();
    match mode {
        LineDisplayMode::Hex => Cow::Owned(join_byte_digits(bytes, 2, hex_byte_str)),
        LineDisplayMode::Octal => Cow::Owned(join_byte_digits(bytes, 3, octal_byte_str)),
        LineDisplayMode::Binary => Cow::Owned(join_byte_digits(bytes, 8, binary_byte_str)),
        _ => displayable_line_string(&entry.text, mode),
    }
}

/// For each byte of [`entry_display_text`] (and its end), the received byte it shows: straight
/// from the received bytes in the byte modes, through the decoded text and
/// [`LogEntry::text_to_wire_offsets`] in the others (#20)
pub fn entry_display_to_wire_map(entry: &LogEntry, mode: LineDisplayMode) -> Vec<usize> {
    match mode {
        LineDisplayMode::Hex | LineDisplayMode::Octal | LineDisplayMode::Binary => {
            build_display_to_raw_byte_map(entry.wire_bytes(), mode)
        }
        _ => {
            let to_text = build_display_to_raw_byte_map(entry.text.as_bytes(), mode);
            if entry.wire.is_none() {
                return to_text;
            }
            let to_wire = entry.text_to_wire_offsets();
            to_text
                .into_iter()
                .map(|text_byte| to_wire[text_byte])
                .collect()
        }
    }
}

/// Builds a byte-by-byte mapping from `display_text` byte offsets
/// to `raw_bytes` byte offsets, tailored to the current `LineDisplayMode`.
///
/// map[disp_byte_idx] == raw_wire_byte_idx
pub fn build_display_to_raw_byte_map(raw_bytes: &[u8], mode: LineDisplayMode) -> Vec<usize> {
    match mode {
        LineDisplayMode::Pretty | LineDisplayMode::Clean => {
            // lines are `String`s (line mode) or vt100 rows: always UTF-8 in practice
            let Ok(text) = std::str::from_utf8(raw_bytes) else {
                return (0..=raw_bytes.len()).collect();
            };
            if let Some(line) = plain_visible_line(text) {
                return (0..=line.len()).collect();
            }
            let mut map = Vec::with_capacity(raw_bytes.len() + 1);
            walk_visible_line(text, |piece| match piece {
                Piece::Shown(shown, Source::Copied(start)) => {
                    map.extend(start..start + shown.len());
                }
                Piece::Shown(shown, Source::Tab(tab)) => {
                    map.extend(std::iter::repeat_n(tab, shown.len()));
                }
                Piece::Sgr(_) => {}
            });
            map.push(text.trim_end_matches(['\r', '\n']).len());
            map
        }

        LineDisplayMode::Raw => {
            // In Raw mode:
            // '\x1b' -> "^[" (2 bytes)
            // '\r'   -> "\\r" (2 bytes)
            // '\t'   -> "    " (4 bytes)
            // control -> "\\xNN" (4 bytes)
            // other char -> ch.len_utf8() bytes
            let raw_str = match std::str::from_utf8(raw_bytes) {
                Ok(s) => s,
                Err(_) => {
                    // Fallback 1:1 if not valid UTF-8
                    let mut map: Vec<usize> = (0..raw_bytes.len()).collect();
                    map.push(raw_bytes.len());
                    return map;
                }
            };

            let mut map = Vec::new();
            for (byte_idx, ch) in raw_str.char_indices() {
                let expanded_len = match ch {
                    '\x1b' => 2,              // "^["
                    '\r' => 2,                // "\\r"
                    '\t' => 4,                // "    "
                    c if c.is_control() => 4, // "\\x00"
                    c => c.len_utf8(),
                };
                // All expanded display bytes point to the source raw byte!
                for _ in 0..expanded_len {
                    map.push(byte_idx);
                }
            }
            map.push(raw_bytes.len());
            map
        }

        LineDisplayMode::Hex => {
            // In Hex mode:
            // Every 1 byte 'b' becomes: "XX" (2 chars) + optional space " " (1 char) = 3 chars (except last)
            let mut map = Vec::new();
            for (i, _) in raw_bytes.iter().enumerate() {
                map.push(i); // 'X'
                map.push(i); // 'X'
                if i + 1 < raw_bytes.len() {
                    map.push(i); // ' '
                }
            }
            map.push(raw_bytes.len());
            map
        }

        LineDisplayMode::Octal => {
            // In Octal mode:
            // Every 1 byte 'b' becomes: "OOO" (3 chars) + optional space " " (1 char) = 4 chars
            let mut map = Vec::new();
            for (i, _) in raw_bytes.iter().enumerate() {
                map.push(i);
                map.push(i);
                map.push(i);
                if i + 1 < raw_bytes.len() {
                    map.push(i); // ' '
                }
            }
            map.push(raw_bytes.len());
            map
        }

        LineDisplayMode::Binary => {
            // In Binary mode:
            // Every 1 byte 'b' becomes: "BBBBBBBB" (8 chars) + optional space " " (1 char) = 9 chars
            let mut map = Vec::new();
            for (i, _) in raw_bytes.iter().enumerate() {
                for _ in 0..8 {
                    map.push(i);
                }
                if i + 1 < raw_bytes.len() {
                    map.push(i); // ' '
                }
            }
            map.push(raw_bytes.len());
            map
        }
    }
}

#[inline]
pub fn map_disp_byte_to_raw(disp_byte: usize, map: &[usize], max_raw_len: usize) -> usize {
    if disp_byte >= map.len() {
        max_raw_len
    } else {
        map[disp_byte]
    }
}

pub fn raw_byte_to_display_col(raw_byte: usize, map: &[usize], display_text: &str) -> usize {
    // first display byte mapped to this raw byte (the map is sorted with one entry per display
    // byte, so a raw byte has duplicates in Hex/Raw/...), binary_search would return any of them
    let disp_byte = map.partition_point(|&r| r < raw_byte);
    safe_byte_to_char_col(display_text, disp_byte)
}

#[inline]
pub fn safe_byte_to_char_col(s: &str, byte_idx: usize) -> usize {
    for (char_idx, (b_start, ch)) in s.char_indices().enumerate() {
        let b_end = b_start + ch.len_utf8();
        if byte_idx < b_end {
            return char_idx;
        }
    }
    s.chars().count()
}

#[cfg(test)]
#[path = "../tests/app/data_processing.rs"]
mod tests;
