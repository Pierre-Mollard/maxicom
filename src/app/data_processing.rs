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
// NOTE: review this mapping
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
