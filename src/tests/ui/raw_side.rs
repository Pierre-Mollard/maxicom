
//! Raw side panel formatting and byte highlights.

use super::*;
use ratatui::style::Color;

/// The plain-text output of each raw mode (used by raw yank).
/// How: known values per mode, including empty.
#[test]
fn plain_string_per_mode() {
    assert_eq!(
        format_raw_bytes_to_plain_string(b"AB", LineDisplayMode::Hex),
        "41 42"
    );
    assert_eq!(
        format_raw_bytes_to_plain_string(b"AB", LineDisplayMode::Octal),
        "101 102"
    );
    assert_eq!(
        format_raw_bytes_to_plain_string(b"A", LineDisplayMode::Binary),
        "01000001"
    );
    assert_eq!(
        format_raw_bytes_to_plain_string(b"AB", LineDisplayMode::Clean),
        "AB"
    );
    assert_eq!(
        format_raw_bytes_to_plain_string(b"", LineDisplayMode::Hex),
        ""
    );
}

/// A highlight styles its bytes and the separators between them, not the one after.
/// How: checks which spans got the style.
#[test]
fn highlight_range_covers_bytes_and_inner_separators() {
    let theme = ThemeConfig::default();
    let style = Style::default().bg(Color::Magenta);
    let highlights = [CharHighlight {
        start_col: 0,
        end_col: 2,
        style,
    }];
    let line = format_raw_bytes_with_highlights(b"ABC", LineDisplayMode::Hex, &highlights, &theme);
    let styled: Vec<bool> = line.spans.iter().map(|s| s.style == style).collect();
    // spans: "41" " " "42" " " "43"
    assert_eq!(styled, [true, true, true, false, false]);
}

/// A highlight past the end is clamped to the line.
/// How: end_col far past the length.
#[test]
fn highlight_out_of_range_is_clamped() {
    let theme = ThemeConfig::default();
    let style = Style::default().bg(Color::Magenta);
    let highlights = [CharHighlight {
        start_col: 1,
        end_col: 99,
        style,
    }];
    let line = format_raw_bytes_with_highlights(b"AB", LineDisplayMode::Hex, &highlights, &theme);
    let styled: Vec<bool> = line.spans.iter().map(|s| s.style == style).collect();
    assert_eq!(styled, [false, false, true]);
}
