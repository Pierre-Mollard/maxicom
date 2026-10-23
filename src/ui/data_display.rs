/// Convert data stream to strings or raw bytes/hex/octal while cleaning escape sequences and stuff
use ansi_to_tui::IntoText;
use ratatui::{style::Style, text::Span};
use std::fmt::Write as _;

use crate::{
    app::data_processing::{
        Piece, binary_byte_str, displayable_line_string, hex_byte_str, octal_byte_str,
        plain_visible_line, walk_visible_line,
    },
    configs::{LineDisplayMode, ThemeConfig},
};

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
    use crate::app::data_processing::{build_display_to_raw_byte_map, displayable_line_string};
    use ratatui::style::Color;

    fn concat(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
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
            "name\tsize\tdate",
            "\x1b[31m10%\r50%\r100% done\x1b[0m",
            "a\x07b\x08c\x7fd",
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

    /// A tab is drawn as the spaces the display string has, and a mid-line CR keeps the colors
    /// set before it (#25).
    /// How: "a\tb" rendered in Pretty, then a red progress line overwritten by a CR.
    #[test]
    fn pretty_tab_and_cr_match_displayable() {
        let theme = ThemeConfig::default();
        let rendered = concat(&format_line_content(
            "a\tb",
            LineDisplayMode::Pretty,
            &theme,
        ));
        assert_eq!(rendered, "a       b");
        assert_eq!(
            rendered,
            displayable_line_string("a\tb", LineDisplayMode::Pretty)
        );

        let spans = format_line_content("\x1b[31m50%\rdone", LineDisplayMode::Pretty, &theme);
        assert_eq!(concat(&spans), "done");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
    }

    /// Every control byte is handled the same way by what search/copy get
    /// (`displayable_line_string`), the map (one offset per displayed byte + the end) and what is
    /// drawn (rendered text without the control chars ratatui skips) (#25).
    /// How: Pretty and Clean over tab, BEL, BS, DEL, mid-line CR and DCS/APC payloads.
    #[test]
    fn control_bytes_agree_between_display_map_and_render() {
        let theme = ThemeConfig::default();
        let cases = [
            ("a\tb", "a       b"),
            ("a\x07b", "ab"),
            ("a\x08b", "ab"),
            ("a\x7fb", "ab"),
            ("50%\rdone", "done"),
            ("a\x1bPdata\x1b\\b", "ab"),
            ("a\x1b_apc\x1b\\b", "ab"),
        ];
        for (text, shown) in cases {
            for mode in [LineDisplayMode::Pretty, LineDisplayMode::Clean] {
                let display = displayable_line_string(text, mode);
                let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
                let rendered = concat(&format_line_content(text, mode, &theme));
                assert_eq!(display, shown, "{text:?} {mode:?}");
                assert_eq!(rendered, shown, "{text:?} {mode:?}");
                assert_eq!(map.len(), shown.len() + 1, "{text:?} {mode:?}");
            }
        }
    }

    /// Escape sequences the three already agree on stay that way (#25 must not change them).
    /// How: a non-SGR CSI, an incomplete CSI at the end, and SGR colors around text.
    #[test]
    fn escape_sequences_agree_between_display_map_and_render() {
        let theme = ThemeConfig::default();
        for text in ["a\x1b[2Kb", "a\x1b[31", "\x1b[31mred\x1b[0m"] {
            for mode in [LineDisplayMode::Pretty, LineDisplayMode::Clean] {
                let display = displayable_line_string(text, mode);
                let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
                let rendered = concat(&format_line_content(text, mode, &theme));
                assert_eq!(rendered, display, "{text:?} {mode:?}");
                assert_eq!(map.len(), display.len() + 1, "{text:?} {mode:?}");
            }
        }
    }

    /// Lines built from fragments chosen by a fixed-seed xorshift: reproducible without a
    /// property-test crate, weighted towards what breaks mappings (escapes, controls, tabs, CR,
    /// multi-byte chars) and always valid UTF-8.
    fn generated_lines(count: usize) -> Vec<String> {
        const FRAGMENTS: &[&str] = &[
            "a",
            "Z",
            " ",
            "0",
            "1",
            "3",
            ";",
            "m",
            "[",
            "]",
            "(",
            "#",
            "B",
            "P",
            "\\",
            "\t",
            "\r",
            "\x07",
            "\x08",
            "\x7f",
            "\x1b",
            "é",
            "漢",
            "✓",
            "\u{85}",
            "\x1b[31m",
            "\x1b[0m",
            "\x1b[38;5;200m",
            "\x1b[2K",
            "\x1b(B",
            "\x1b]0;t\x07",
            "\x1bPd\x1b\\",
            "\x1b7",
        ];
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..count)
            .map(|_| {
                let len = next() % 24;
                let mut line: String = (0..len)
                    .map(|_| FRAGMENTS[(next() % FRAGMENTS.len() as u64) as usize])
                    .collect();
                if next() % 3 == 0 {
                    line.push_str("\r\n");
                }
                line
            })
            .collect()
    }

    /// For any line, in every mode the map has one offset per displayed byte (+ the end), and
    /// Pretty/Clean render exactly the display text.
    /// How: 2000 generated lines; a failure prints the line to add as a fixed test.
    #[test]
    fn display_map_render_agree_on_generated_lines() {
        let theme = ThemeConfig::default();
        for text in generated_lines(2000) {
            for mode in [
                LineDisplayMode::Pretty,
                LineDisplayMode::Clean,
                LineDisplayMode::Raw,
                LineDisplayMode::Hex,
                LineDisplayMode::Octal,
                LineDisplayMode::Binary,
            ] {
                let display = displayable_line_string(&text, mode);
                let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
                assert_eq!(map.len(), display.len() + 1, "map {mode:?} on {text:?}");
                if matches!(mode, LineDisplayMode::Pretty | LineDisplayMode::Clean) {
                    let rendered = concat(&format_line_content(&text, mode, &theme));
                    assert_eq!(rendered, display, "render {mode:?} on {text:?}");
                }
            }
        }
    }

    /// A plain Pretty line skips ansi_to_tui and gives the very spans it would have.
    /// How: spans compared to ansi_to_tui's first line on plain samples.
    #[test]
    fn pretty_plain_line_spans_match_ansi_to_tui() {
        let theme = ThemeConfig::default();
        for text in ["plain text\r\n", "héllo ✓ 漢", "a b  c"] {
            let line = text.trim_end_matches(['\r', '\n']);
            let parsed = line.as_bytes().into_text().unwrap();
            let expected = parsed.lines.into_iter().next().unwrap().spans;
            assert_eq!(
                format_line_content(text, LineDisplayMode::Pretty, &theme),
                expected,
                "{text:?}"
            );
        }
    }
}
