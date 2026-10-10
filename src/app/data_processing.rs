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
