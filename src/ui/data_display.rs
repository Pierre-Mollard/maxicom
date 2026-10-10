/// Convert data stream to strings or raw bytes/hex/octal while cleaning escape sequences and stuff
use ansi_to_tui::IntoText;
use ratatui::{
    style::{Color, Style},
    text::Span,
};

use crate::configs::LineDisplayMode;

pub fn sanitize_terminal_control_sequences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                // Keep CSI (styles, controls)
                Some('[') => {
                    out.push(ch);
                }
                // OSC (ESC] to BELL or ESC\)
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // DCS, APC, PM (ESC] to ESC\)
                Some('P') | Some('_') | Some('^') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // 2-byte escape sequence (DECKPAM `=`, DECKPNM `>`, DECSC `7`, DECRC `8`, RIS `c`, etc.)
                Some(next_ch) if next_ch.is_ascii() && !next_ch.is_ascii_control() => {
                    chars.next();
                }
                _ => {}
            }
        } else {
            out.push(ch);
        }
    }

    out
}

fn trim_trailing_incomplete_ansi(text: &str) -> &str {
    if let Some(esc_idx) = text.rfind('\x1b') {
        let tail = &text[esc_idx..];
        if tail.len() == 1 {
            return &text[..esc_idx];
        }

        if tail.starts_with("\x1b[") {
            let is_finished = tail
                .chars()
                .skip(2)
                .any(|c| c.is_ascii_alphabetic() || ('@'..='~').contains(&c));
            if !is_finished {
                return &text[..esc_idx];
            }
        }
    }
    text
}

pub fn format_line_content(text: &str, mode: LineDisplayMode) -> Vec<Span<'static>> {
    match mode {
        LineDisplayMode::Pretty => {
            let clean = text.trim_end_matches(['\r', '\n']);
            let sanitized = sanitize_terminal_control_sequences(clean);
            let displayable = trim_trailing_incomplete_ansi(&sanitized);
            if let Ok(parsed) = displayable.as_bytes().into_text()
                && let Some(first_line) = parsed.lines.into_iter().next()
            {
                return first_line.spans;
            }
            vec![Span::raw(displayable.to_string())]
        }
        LineDisplayMode::Clean => {
            let clean = text.trim_end_matches(['\r', '\n']);
            let clean_text = strip_ansi_escapes::strip_str(clean);
            vec![Span::raw(clean_text)]
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
            vec![Span::styled(raw, Style::default().fg(Color::White))]
        }
        LineDisplayMode::Hex => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }

            let mut spans = Vec::with_capacity(bytes.len() * 2);

            for (i, &b) in bytes.iter().enumerate() {
                let color = match b {
                    0x00 => Color::DarkGray,                        // \0
                    0x0A | 0x0D => Color::Yellow,                   // \r and \n
                    0xAA | 0x55 | 0x7E => Color::LightRed,          // sync / frame delimiters
                    b if (0x20..=0x7E).contains(&b) => Color::Cyan, // ASCII
                    _ => Color::White,
                };

                spans.push(Span::styled(format!("{b:02X}"), Style::default().fg(color)));

                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }

            spans
        }
        LineDisplayMode::Octal => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let mut spans = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = match b {
                    0 => Color::DarkGray,
                    0o12 | 0o15 => Color::Yellow, // \r and \n
                    b if (0x20..=0x7E).contains(&b) => Color::Green,
                    _ => Color::White,
                };
                spans.push(Span::styled(format!("{b:03o}"), Style::default().fg(color)));
                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }
            spans
        }
        LineDisplayMode::Binary => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let mut spans = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = if b == 0 {
                    Color::DarkGray
                } else if b == 0xFF {
                    Color::LightRed
                } else {
                    Color::LightBlue
                };
                spans.push(Span::styled(format!("{b:08b}"), Style::default().fg(color)));
                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }
            spans
        }
    }
}
