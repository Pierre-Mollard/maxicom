/// Convert data stream to strings or raw bytes/hex/octal while cleaning escape sequences and stuff
use ansi_to_tui::IntoText;
use ratatui::{style::Style, text::Span};
use std::borrow::Cow;
use std::fmt::Write as _;

use crate::{
    app::data_processing::{binary_byte_str, hex_byte_str, octal_byte_str},
    configs::{LineDisplayMode, ThemeConfig},
};

pub fn sanitize_terminal_control_sequences(text: &str) -> Cow<'_, str> {
    if !text.contains('\x1b') {
        return Cow::Borrowed(text);
    }

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

    Cow::Owned(out)
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

pub fn format_line_content(
    text: &str,
    mode: LineDisplayMode,
    theme: &ThemeConfig,
) -> Vec<Span<'static>> {
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
            byte_digit_spans(bytes, mode, Style::default(), theme)
        }
    }
}

/// One span per byte colored by `theme.byte_color`, separated by `separator_style` spaces
/// (spans 2*i are bytes, 2*i+1 separators). Spans borrow static digit strings, no alloc per byte.
pub fn byte_digit_spans(
    bytes: &[u8],
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
    for (i, &b) in bytes.iter().enumerate() {
        spans.push(Span::styled(
            digits_of(b),
            Style::default().fg(theme.byte_color(b)),
        ));
        if i + 1 < bytes.len() {
            spans.push(Span::styled(" ", separator_style));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    //! Turning a history line into styled spans for each display mode.

    use super::*;
    use crate::app::data_processing::displayable_line_string;
    use ratatui::style::Color;

    fn concat(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Text without escapes is not copied.
    /// How: checks for `Cow::Borrowed`.
    #[test]
    fn sanitize_borrows_when_no_escape() {
        assert!(matches!(
            sanitize_terminal_control_sequences("plain"),
            Cow::Borrowed("plain")
        ));
    }

    /// CSI styling is kept; OSC, DCS and 2-byte escapes are removed.
    /// How: one input per sequence kind.
    #[test]
    fn sanitize_keeps_csi_and_drops_other_sequences() {
        assert_eq!(
            sanitize_terminal_control_sequences("\x1b[31mred\x1b[0m"),
            "\x1b[31mred\x1b[0m"
        );
        assert_eq!(
            sanitize_terminal_control_sequences("a\x1b]0;title\x07b"),
            "ab"
        );
        assert_eq!(
            sanitize_terminal_control_sequences("a\x1b]0;title\x1b\\b"),
            "ab"
        );
        assert_eq!(
            sanitize_terminal_control_sequences("a\x1bPdata\x1b\\b"),
            "ab"
        );
        assert_eq!(sanitize_terminal_control_sequences("a\x1b7b"), "ab");
    }

    /// An unfinished escape at the end of a line is cut off.
    /// How: complete and incomplete tails.
    #[test]
    fn trim_incomplete_ansi_tail() {
        assert_eq!(trim_trailing_incomplete_ansi("abc\x1b"), "abc");
        assert_eq!(trim_trailing_incomplete_ansi("abc\x1b[31"), "abc");
        assert_eq!(trim_trailing_incomplete_ansi("abc\x1b[31m"), "abc\x1b[31m");
        assert_eq!(trim_trailing_incomplete_ansi("abc"), "abc");
    }

    /// Pretty mode turns SGR colors into span styles.
    /// How: the red text becomes a red span.
    #[test]
    fn pretty_parses_colors() {
        let theme = ThemeConfig::default();
        let spans = format_line_content(
            "\x1b[31mred\x1b[0m plain\r\n",
            LineDisplayMode::Pretty,
            &theme,
        );
        assert_eq!(concat(&spans), "red plain");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
    }

    /// Hex/Octal/Binary emit one span per byte with separator spans in between.
    /// How: checks the span texts.
    #[test]
    fn numeric_modes_one_span_per_byte_and_separator() {
        let theme = ThemeConfig::default();
        let spans = format_line_content("AB", LineDisplayMode::Hex, &theme);
        let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, ["41", " ", "42"]);

        let spans = format_line_content("A", LineDisplayMode::Octal, &theme);
        assert_eq!(concat(&spans), "101");
        let spans = format_line_content("A", LineDisplayMode::Binary, &theme);
        assert_eq!(concat(&spans), "01000001");
    }

    /// Invariant: the rendered text equals `displayable_line_string` (the mapping relies on it).
    /// How: every mode over a set of samples.
    #[test]
    fn rendered_text_matches_displayable_string() {
        // the mapping code (search, copy cursor, raw panel) relies on both producing the same text
        let theme = ThemeConfig::default();
        let samples = [
            "",
            "plain text",
            "\x1b[31mred\x1b[0m and \x1b[1;32mgreen\x1b[0m\r\n",
            "héllo wörld ✓",
            "a\x1b]0;title\x07b",
        ];
        let modes = [
            LineDisplayMode::Pretty,
            LineDisplayMode::Clean,
            LineDisplayMode::Raw,
            LineDisplayMode::Hex,
            LineDisplayMode::Octal,
            LineDisplayMode::Binary,
        ];
        for text in samples {
            for mode in modes {
                let rendered = concat(&format_line_content(text, mode, &theme));
                assert_eq!(
                    rendered,
                    displayable_line_string(text, mode),
                    "{mode:?} on {text:?}"
                );
            }
        }
    }

    /// Locks bug #25: the rendered Pretty line keeps the tab, the display string drops it.
    /// How: compares both for "a\tb".
    #[test]
    fn pretty_tab_differs_from_displayable() {
        // FIXME: current behavior, see #25: the rendered line keeps the tab, displayable drops it
        let theme = ThemeConfig::default();
        let rendered = concat(&format_line_content(
            "a\tb",
            LineDisplayMode::Pretty,
            &theme,
        ));
        assert_ne!(
            rendered,
            displayable_line_string("a\tb", LineDisplayMode::Pretty)
        );
    }
}
