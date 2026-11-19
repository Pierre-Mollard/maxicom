/// Convert data stream to strings or raw bytes/hex/octal while cleaning escape sequences and stuff
use ansi_to_tui::IntoText;
use ratatui::{style::Style, text::Span};
use std::fmt::Write as _;

use crate::{
    app::data_processing::{
        Piece, binary_byte_str, displayable_line_string, hex_byte_str, octal_byte_str,
        plain_visible_line, walk_visible_line,
    },
    app::types::LogEntry,
    configs::{LineDisplayMode, TextEncoding, ThemeConfig},
    raw_side::{ByteClass, byte_classes},
};

/// Spans of a history entry: the byte modes show the bytes received, the others its decoded
/// text (#20); the bytes in the colors of their classes in `encoding`
pub fn format_entry_content(
    entry: &LogEntry,
    mode: LineDisplayMode,
    encoding: TextEncoding,
    theme: &ThemeConfig,
) -> Vec<Span<'static>> {
    match mode {
        LineDisplayMode::Hex | LineDisplayMode::Octal | LineDisplayMode::Binary => {
            let bytes = entry.wire_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let classes = byte_classes(bytes, 0..bytes.len(), encoding);
            byte_digit_spans(bytes, &classes, mode, Style::default(), theme)
        }
        _ => format_line_content(&entry.text, mode, theme),
    }
}

pub fn format_line_content(
    text: &str,
    mode: LineDisplayMode,
    theme: &ThemeConfig,
) -> Vec<Span<'static>> {
    match mode {
        LineDisplayMode::Pretty => {
            // nothing for ansi_to_tui to parse
            if let Some(line) = plain_visible_line(text) {
                return vec![Span::raw(line.to_owned())];
            }
            // the shown text with only its SGR colors around it: the same text as
            // `displayable_line_string` (#25)
            let mut styled = String::with_capacity(text.len());
            walk_visible_line(text, |piece| match piece {
                Piece::Shown(shown, _) | Piece::Sgr(shown) => styled.push_str(shown),
            });
            if let Ok(parsed) = styled.as_bytes().into_text()
                && let Some(first_line) = parsed.lines.into_iter().next()
            {
                return first_line.spans;
            }
            vec![Span::raw(displayable_line_string(text, mode).into_owned())]
        }
        LineDisplayMode::Clean => vec![Span::raw(displayable_line_string(text, mode).into_owned())],
        LineDisplayMode::Raw => {
            let mut raw = String::with_capacity(text.len());
            for ch in text.chars() {
                match ch {
                    '\x1b' => raw.push_str("^["),
                    '\r' => raw.push_str("\\r"),
                    '\t' => raw.push_str("    "),
                    c if c.is_control() => {
                        let _ = write!(raw, "\\x{:02X}", c as u32);
                    }
                    c => raw.push(c),
                }
            }
            vec![Span::styled(
                raw,
                Style::default().fg(theme.raw_inspector.byte_any),
            )]
        }
        LineDisplayMode::Hex | LineDisplayMode::Octal | LineDisplayMode::Binary => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let classes = byte_classes(bytes, 0..bytes.len(), TextEncoding::Utf8);
            byte_digit_spans(bytes, &classes, mode, Style::default(), theme)
        }
    }
}

/// One span per byte in the color of its class in `classes`, separated by `separator_style`
/// spaces (spans 2*i are bytes, 2*i+1 separators). Spans borrow static digit strings, no alloc
/// per byte.
pub fn byte_digit_spans(
    bytes: &[u8],
    classes: &[ByteClass],
    mode: LineDisplayMode,
    separator_style: Style,
    theme: &ThemeConfig,
) -> Vec<Span<'static>> {
    let digits_of = match mode {
        LineDisplayMode::Octal => octal_byte_str,
        LineDisplayMode::Binary => binary_byte_str,
        _ => hex_byte_str,
    };

    let mut spans = Vec::with_capacity(bytes.len() * 2);
    for (i, (&b, &class)) in bytes.iter().zip(classes).enumerate() {
        spans.push(Span::styled(
            digits_of(b),
            Style::default().fg(theme.byte_class_color(class)),
        ));
        if i + 1 < bytes.len() {
            spans.push(Span::styled(" ", separator_style));
        }
    }
    spans
}

#[cfg(test)]
#[path = "../tests/ui/data_display.rs"]
mod tests;
