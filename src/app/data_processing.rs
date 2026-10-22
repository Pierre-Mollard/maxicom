// Process what data would look like processed by the ui (simplified)

use crate::configs::LineDisplayMode;

pub fn displayable_line_string(text: &str, mode: LineDisplayMode) -> String {
    match mode {
        LineDisplayMode::Pretty | LineDisplayMode::Clean => {
            let clean = text.trim_end_matches(['\r', '\n']);
            strip_ansi_escapes::strip_str(clean)
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
            raw
        }
        LineDisplayMode::Hex => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return String::new();
            }
            bytes
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<String>>()
                .join(" ")
        }
        LineDisplayMode::Octal => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return String::new();
            }
            bytes
                .iter()
                .map(|b| format!("{b:03o}"))
                .collect::<Vec<String>>()
                .join(" ")
        }
        LineDisplayMode::Binary => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return String::new();
            }
            bytes
                .iter()
                .map(|b| format!("{b:08b}"))
                .collect::<Vec<String>>()
                .join(" ")
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
            // Strips trailing \r, \n and skips ANSI sequences:
            let mut end = raw_bytes.len();
            while end > 0 && (raw_bytes[end - 1] == b'\r' || raw_bytes[end - 1] == b'\n') {
                end -= 1;
            }
            let content_bytes = &raw_bytes[..end];

            let mut map = Vec::with_capacity(content_bytes.len() + 1);
            let mut i = 0;
            while i < content_bytes.len() {
                if content_bytes[i] == 0x1B {
                    if i + 1 < content_bytes.len() && content_bytes[i + 1] == b'[' {
                        i += 2;
                        while i < content_bytes.len() {
                            let b = content_bytes[i];
                            i += 1;
                            if (0x40..=0x7E).contains(&b) {
                                break;
                            }
                        }
                        continue;
                    }
                    if i + 1 < content_bytes.len() && content_bytes[i + 1] == b']' {
                        i += 2;
                        while i < content_bytes.len() {
                            if content_bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if content_bytes[i] == 0x1B
                                && i + 1 < content_bytes.len()
                                && content_bytes[i + 1] == b'\\'
                            {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    if i + 1 < content_bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    continue;
                }

                map.push(i);
                i += 1;
            }
            map.push(end);
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
    // Binary search where this raw byte falls in the display mapping
    let disp_byte = match map.binary_search(&raw_byte) {
        Ok(idx) => idx,
        Err(idx) => idx,
    };
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
        ];
        for text in samples {
            for mode in ALL_MODES {
                let display = displayable_line_string(text, mode);
                let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
                assert_eq!(map.len(), display.len() + 1, "{mode:?} on {text:?}");
            }
        }
    }

    /// Locks bug #25: the tab is dropped from display text but kept in the map.
    /// How: compares display and map for "a\tb".
    #[test]
    fn map_pretty_keeps_tab_that_display_drops() {
        // FIXME: current behavior, see #25: strip_ansi_escapes drops C0 controls (tab) but the
        // map keeps them, so every offset after a tab is shifted by one
        let display = displayable_line_string("a\tb", LineDisplayMode::Pretty);
        let map = build_display_to_raw_byte_map(b"a\tb", LineDisplayMode::Pretty);
        assert_eq!(display, "ab");
        assert_eq!(map, vec![0, 1, 2, 3]);
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

    /// Locks bug #24: in Hex the column lands on the separator instead of the byte start.
    /// How: raw byte 1 of "ABC" gives col 5 (3 expected).
    #[test]
    fn raw_byte_to_display_col_hex_lands_on_byte_group() {
        let map = build_display_to_raw_byte_map(b"ABC", LineDisplayMode::Hex);
        let col = raw_byte_to_display_col(1, &map, "41 42 43");
        // FIXME: current behavior, see #24: binary_search over duplicated entries returns an
        // unspecified one, here the separator after "42" (5) instead of the group start (3)
        assert_eq!(col, 5);
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
