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
mod tests {
    //! Display strings and display-to-raw byte maps used by search, copy mode and the raw panel.

    use super::*;

    const ALL_MODES: [LineDisplayMode; 6] = [
        LineDisplayMode::Pretty,
        LineDisplayMode::Clean,
        LineDisplayMode::Raw,
        LineDisplayMode::Hex,
        LineDisplayMode::Octal,
        LineDisplayMode::Binary,
    ];

    /// The compile-time digit tables agree with `format!` for every byte value.
    /// How: compares all 256 bytes in each radix.
    #[test]
    fn byte_digit_tables_match_format() {
        for b in 0..=255u8 {
            assert_eq!(hex_byte_str(b), format!("{b:02X}"));
            assert_eq!(octal_byte_str(b), format!("{b:03o}"));
            assert_eq!(binary_byte_str(b), format!("{b:08b}"));
        }
    }

    /// Pretty and Clean drop ANSI styling and the trailing EOL.
    /// How: compares against a colored CRLF line.
    #[test]
    fn displayable_pretty_and_clean_strip_ansi_and_eol() {
        let text = "\x1b[31mHi\x1b[0m there\r\n";
        assert_eq!(
            displayable_line_string(text, LineDisplayMode::Pretty),
            "Hi there"
        );
        assert_eq!(
            displayable_line_string(text, LineDisplayMode::Clean),
            "Hi there"
        );
    }

    /// Raw mode makes control bytes visible (`^[`, `\r`, `\xNN`, tab as spaces).
    /// How: checks ESC, tab, CR, LF and DEL.
    #[test]
    fn displayable_raw_expands_controls() {
        assert_eq!(
            displayable_line_string("\x1b[1ma\tb\r\n", LineDisplayMode::Raw),
            "^[[1ma    b\\r\\x0A"
        );
        assert_eq!(
            displayable_line_string("\x7f", LineDisplayMode::Raw),
            "\\x7F"
        );
    }

    /// Hex/Octal/Binary format each byte, space separated, and empty input stays empty.
    /// How: known values for "AB", plus every mode on "".
    #[test]
    fn displayable_numeric_modes() {
        assert_eq!(displayable_line_string("AB", LineDisplayMode::Hex), "41 42");
        assert_eq!(
            displayable_line_string("AB", LineDisplayMode::Octal),
            "101 102"
        );
        assert_eq!(
            displayable_line_string("AB", LineDisplayMode::Binary),
            "01000001 01000010"
        );
        for mode in ALL_MODES {
            assert_eq!(displayable_line_string("", mode), "", "{mode:?}");
        }
    }

    /// The Pretty map skips CSI sequences and the trailing CRLF.
    /// How: exact map of a colored "Hi" line.
    #[test]
    fn map_pretty_skips_csi_and_trailing_eol() {
        // ESC [ 3 1 m H i ESC [ 0 m \r \n
        let raw = b"\x1b[31mHi\x1b[0m\r\n";
        let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
        assert_eq!(map, vec![5, 6, 11]);
    }

    /// The Pretty map skips OSC sequences ended by BEL.
    /// How: exact map of an OSC title followed by text.
    #[test]
    fn map_pretty_skips_osc() {
        // ESC ] 0 ; t i t l e BEL o k
        let raw = b"\x1b]0;title\x07ok";
        let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
        assert_eq!(map, vec![10, 11, 12]);
    }

    /// In Raw mode every expanded display byte points back to its source byte.
    /// How: exact map of "a\tb\r\n".
    #[test]
    fn map_raw_points_expansions_to_source_byte() {
        let raw = b"a\tb\r\n";
        let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Raw);
        assert_eq!(map, vec![0, 1, 1, 1, 1, 2, 3, 3, 4, 4, 4, 4, 5]);
    }

    /// Invalid UTF-8 in Raw mode falls back to a 1:1 map.
    /// How: map of [0xFF, 'a'].
    #[test]
    fn map_raw_invalid_utf8_falls_back_to_identity() {
        let map = build_display_to_raw_byte_map(&[0xff, b'a'], LineDisplayMode::Raw);
        assert_eq!(map, vec![0, 1, 2]);
    }

    /// Hex/Octal/Binary maps repeat each byte index once per digit, plus its separator.
    /// How: exact maps for short inputs.
    #[test]
    fn map_numeric_modes_follow_stride() {
        assert_eq!(
            build_display_to_raw_byte_map(b"AB", LineDisplayMode::Hex),
            vec![0, 0, 0, 1, 1, 2]
        );
        assert_eq!(
            build_display_to_raw_byte_map(b"AB", LineDisplayMode::Octal),
            vec![0, 0, 0, 0, 1, 1, 1, 2]
        );
        assert_eq!(
            build_display_to_raw_byte_map(b"A", LineDisplayMode::Binary),
            vec![0, 0, 0, 0, 0, 0, 0, 0, 1]
        );
    }

    /// Invariant: map length == display string length + 1 (the end sentinel).
    /// How: every mode over a set of samples (ANSI, UTF-8).
    #[test]
    fn map_len_is_display_len_plus_one() {
        let samples = [
            "",
            "plain text",
            "\x1b[31mred\x1b[0m and \x1b[1;32mgreen\x1b[0m\r\n",
            "héllo wörld ✓",
            "name\tsize\tdate",
            "10%\r50%\r100% done\r\n",
            "a\x07b\x7fc\x1bPdcs\x1b\\d",
        ];
        for text in samples {
            for mode in ALL_MODES {
                let display = displayable_line_string(text, mode);
                let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
                assert_eq!(map.len(), display.len() + 1, "{mode:?} on {text:?}");
            }
        }
    }

    /// A tab is shown as spaces up to the next 8 column stop, each mapped to the tab byte (#25).
    /// How: display and exact map of "a\tb".
    #[test]
    fn map_pretty_points_tab_spaces_to_tab() {
        let display = displayable_line_string("a\tb", LineDisplayMode::Pretty);
        let map = build_display_to_raw_byte_map(b"a\tb", LineDisplayMode::Pretty);
        assert_eq!(display, "a       b");
        assert_eq!(map, vec![0, 1, 1, 1, 1, 1, 1, 1, 2, 3]);
    }

    /// A mid-line CR shows only the text after the last one, mapped to its own bytes (#25).
    /// How: exact map of "ab\rcd".
    #[test]
    fn map_pretty_after_last_cr() {
        assert_eq!(
            displayable_line_string("ab\rcd", LineDisplayMode::Pretty),
            "cd"
        );
        let map = build_display_to_raw_byte_map(b"ab\rcd", LineDisplayMode::Pretty);
        assert_eq!(map, vec![3, 4, 5]);
    }

    /// Text shown by the walker for a line, with the SGR sequences it reported.
    fn walk(text: &str) -> (String, Vec<&str>) {
        let mut shown = String::new();
        let mut sgr = Vec::new();
        walk_visible_line(text, |piece| match piece {
            Piece::Shown(piece, _) => shown.push_str(piece),
            Piece::Sgr(seq) => sgr.push(seq),
        });
        (shown, sgr)
    }

    /// SGR is reported for the render; OSC, DCS, 2-byte and incomplete escapes are dropped
    /// (the cases the render's old sanitizer and incomplete-tail trim covered) (#25).
    /// How: one input per sequence kind.
    #[test]
    fn walk_reports_sgr_and_drops_other_sequences() {
        assert_eq!(
            walk("\x1b[31mred\x1b[0m"),
            (String::from("red"), vec!["\x1b[31m", "\x1b[0m"])
        );
        for (text, shown) in [
            ("a\x1b]0;title\x07b", "ab"),
            ("a\x1b]0;title\x1b\\b", "ab"),
            ("a\x1bPdata\x1b\\b", "ab"),
            ("a\x1b7b", "ab"),
            ("a\x1b[2Kb", "ab"),
            ("abc\x1b", "abc"),
            ("abc\x1b[31", "abc"),
        ] {
            assert_eq!(walk(text), (String::from(shown), vec![]), "{text:?}");
        }
    }

    /// Escapes with intermediate bytes (charset `ESC ( B` sent by `tput sgr0`, `ESC # 8`) end at
    /// their final byte, not at the first byte after ESC.
    /// How: charset and DEC test escapes between text, and one cut at the end of the line.
    #[test]
    fn walk_drops_escapes_with_intermediates() {
        for (text, shown) in [
            ("a\x1b(Bb", "ab"),
            ("a\x1b(0b", "ab"),
            ("a\x1b#8b", "ab"),
            ("x\x1b(B\x1b[mdone", "xdone"),
            ("a\x1b(", "a"),
        ] {
            assert_eq!(walk(text).0, shown, "{text:?}");
        }
    }

    /// A new ESC aborts an unfinished CSI, OSC or DCS like in a terminal, and the new sequence
    /// is parsed; a CSI with bytes outside its grammar is never passed on as SGR.
    /// How: a cut CSI, OSC and DCS each followed by a full SGR, and a malformed SGR.
    #[test]
    fn walk_new_escape_aborts_unfinished_sequence() {
        let sgr = |text, shown: &str, seq| (text, (String::from(shown), vec![seq]));
        for (text, expected) in [
            sgr("\x1b[31\x1b[0mok", "ok", "\x1b[0m"),
            sgr("a\x1b]0;title\x1b[31mb", "ab", "\x1b[31m"),
            sgr("a\x1bPdcs\x1b[1mb", "ab", "\x1b[1m"),
        ] {
            assert_eq!(walk(text), expected, "{text:?}");
        }
        assert_eq!(walk("a\x1b[3é1mb"), (String::from("ab"), vec![]));
    }

    /// Only a CR outside escape sequences hides the text before it.
    /// How: a CR inside an OSC title, then a real CR after the OSC.
    #[test]
    fn walk_ignores_cr_inside_escape() {
        assert_eq!(walk("a\x1b]0;ti\rtle\x07b").0, "ab");
        assert_eq!(walk("a\x1b]0;t\x07\rb").0, "b");
    }

    /// Lines with nothing to clean are borrowed as they are (minus the EOL), any control char
    /// (ESC, tab, CR, BEL, DEL, C1) goes through the walker; other modes always build.
    /// How: borrowed and owned samples in Pretty and Clean, plus Raw and Hex.
    #[test]
    fn displayable_borrows_plain_lines() {
        for mode in [LineDisplayMode::Pretty, LineDisplayMode::Clean] {
            for (text, shown) in [
                ("plain text\r\n", "plain text"),
                ("héllo ✓ 漢", "héllo ✓ 漢"),
            ] {
                let display = displayable_line_string(text, mode);
                assert!(
                    matches!(display, Cow::Borrowed(d) if d == shown),
                    "{text:?}"
                );
            }
            for text in [
                "a\tb",
                "\x1b[31mred",
                "50%\rdone",
                "a\x07b",
                "a\x7fb",
                "a\u{85}b",
            ] {
                let display = displayable_line_string(text, mode);
                assert!(matches!(display, Cow::Owned(_)), "{text:?} {mode:?}");
            }
        }
        for mode in [LineDisplayMode::Raw, LineDisplayMode::Hex] {
            assert!(matches!(displayable_line_string("ab", mode), Cow::Owned(_)));
        }
    }

    /// The shortcut map of a plain line is the one the walker builds.
    /// How: walker-built map compared to `build_display_to_raw_byte_map` on plain samples.
    #[test]
    fn plain_line_map_matches_walker() {
        for text in ["", "plain text\r\n", "héllo ✓ 漢", "a b  c"] {
            let mut walked = Vec::new();
            walk_visible_line(text, |piece| {
                if let Piece::Shown(shown, Source::Copied(start)) = piece {
                    walked.extend(start..start + shown.len());
                }
            });
            walked.push(text.trim_end_matches(['\r', '\n']).len());
            assert_eq!(
                build_display_to_raw_byte_map(text.as_bytes(), LineDisplayMode::Pretty),
                walked,
                "{text:?}"
            );
        }
    }

    /// Entry display text and map go to the bytes received: byte modes format them, the other
    /// modes map their decoded text back through `text_to_wire_offsets` (#20).
    /// How: "xéy" received as 78 E9 79 0A in Latin1, in Hex and Pretty; a UTF-8 line unchanged.
    #[test]
    fn entry_display_and_map_use_received_bytes() {
        use crate::configs::TextEncoding;
        let wire = b"x\xE9y\n";
        let entry = LogEntry::received(
            TextEncoding::Latin1.decode(wire),
            wire,
            Some(chrono::Local::now()),
        );
        assert_eq!(
            entry_display_text(&entry, LineDisplayMode::Hex),
            "78 E9 79 0A"
        );
        assert_eq!(
            entry_display_to_wire_map(&entry, LineDisplayMode::Hex),
            build_display_to_raw_byte_map(wire, LineDisplayMode::Hex)
        );
        // "xéy": é is 2 display bytes, both on the received byte 1; the end is the wire's
        assert_eq!(entry_display_text(&entry, LineDisplayMode::Pretty), "xéy");
        assert_eq!(
            entry_display_to_wire_map(&entry, LineDisplayMode::Pretty),
            [0, 1, 1, 2, 3]
        );

        let utf8 = LogEntry::new(String::from("xéy\n"), chrono::Local::now());
        assert_eq!(
            entry_display_to_wire_map(&utf8, LineDisplayMode::Pretty),
            build_display_to_raw_byte_map("xéy\n".as_bytes(), LineDisplayMode::Pretty)
        );
    }

    /// Tabs expand to the next 8 column stop counted in display width, wide chars count 2.
    /// How: tabs after 0, 2 and 9 columns, two tabs in a row, and after a wide char.
    #[test]
    fn walk_tab_stops() {
        assert_eq!(walk("\tx").0, "        x");
        assert_eq!(walk("ab\tc").0, "ab      c");
        assert_eq!(walk("123456789\tx").0, "123456789       x");
        assert_eq!(walk("\t\tx").0, format!("{}x", " ".repeat(16)));
        assert_eq!(walk("漢\tx").0, "漢      x");
    }

    /// Only the text after the last mid-line CR is shown, SGR before it is still reported and
    /// tab stops restart after it.
    /// How: a progress line overwritten twice, colored before the CR.
    #[test]
    fn walk_keeps_text_after_last_cr() {
        assert_eq!(walk("10%\r50%\r100% done\r\n").0, "100% done");
        assert_eq!(
            walk("\x1b[31m50%\rok\tx"),
            (String::from("ok      x"), vec!["\x1b[31m"])
        );
    }

    /// Out-of-range display bytes map to the raw length.
    /// How: one in-range and one past-the-end lookup.
    #[test]
    fn map_disp_byte_to_raw_clamps() {
        let map = [5, 6, 11];
        assert_eq!(map_disp_byte_to_raw(1, &map, 13), 6);
        assert_eq!(map_disp_byte_to_raw(3, &map, 13), 13);
    }

    /// A raw byte maps back to the right display column when there is ANSI before it.
    /// How: 'i' of a colored "Hi" gives col 1.
    #[test]
    fn raw_byte_to_display_col_pretty() {
        let raw = b"\x1b[31mHi\x1b[0m";
        let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
        assert_eq!(raw_byte_to_display_col(6, &map, "Hi"), 1);
    }

    /// A raw byte maps to the first column of its digit group in Hex/Octal/Binary (#24).
    /// How: every byte of "ABC" in each mode, expected col = byte index * stride.
    #[test]
    fn raw_byte_to_display_col_lands_on_group_start() {
        let modes = [
            (LineDisplayMode::Hex, 3),
            (LineDisplayMode::Octal, 4),
            (LineDisplayMode::Binary, 9),
        ];
        for (mode, stride) in modes {
            let display = displayable_line_string("ABC", mode);
            let map = build_display_to_raw_byte_map(b"ABC", mode);
            for raw_byte in 0..3 {
                let col = raw_byte_to_display_col(raw_byte, &map, &display);
                assert_eq!(col, raw_byte * stride, "{mode:?} byte {raw_byte}");
            }
        }
    }

    /// In Raw mode an expanded control maps to the start of its expansion (#24).
    /// How: the tab of "a\tb" (raw byte 1) is display col 1, 'b' is col 5.
    #[test]
    fn raw_byte_to_display_col_raw_expansion_start() {
        let display = displayable_line_string("a\tb", LineDisplayMode::Raw);
        let map = build_display_to_raw_byte_map(b"a\tb", LineDisplayMode::Raw);
        assert_eq!(raw_byte_to_display_col(1, &map, &display), 1);
        assert_eq!(raw_byte_to_display_col(2, &map, &display), 5);
    }

    /// Byte offsets convert to char columns across multibyte chars, clamped at the end.
    /// How: offsets around 'é' and past the end.
    #[test]
    fn safe_byte_to_char_col_handles_multibyte() {
        assert_eq!(safe_byte_to_char_col("héllo", 0), 0);
        assert_eq!(safe_byte_to_char_col("héllo", 2), 1);
        assert_eq!(safe_byte_to_char_col("héllo", 3), 2);
        assert_eq!(safe_byte_to_char_col("héllo", 99), 5);
    }
}
