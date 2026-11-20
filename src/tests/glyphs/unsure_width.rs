//! Chars a host may draw with another width (MAX-271): the table catches what drifted in tmux,
//! leaves common text alone.

use super::*;

/// MAX-271: one char of each width disagreement measured in tmux 3.8 (ratatui → tmux).
/// How: 0 → 1, 0 → 2, 1 → 2, 1 → 0, 2 → 1 (assigned and unassigned), 3 → 1
#[test]
fn chars_that_drifted_in_tmux_are_unsure() {
    let drifted = [
        '\u{0605}',
        '\u{0890}',
        '\u{302E}',
        '\u{16FF0}',
        '\u{3248}',
        '\u{324F}',
        '\u{2D7F}',
        '\u{FFF9}',
        '\u{13430}',
        '\u{2630}',
        '\u{17A4}',
        '\u{18DE1}',
        '\u{2EE6A}',
        '\u{17D8}',
    ];
    for c in drifted {
        assert!(is_unsure(c), "U+{:04X}", u32::from(c));
    }
}

/// MAX-271: text every host draws the same isn't touched.
/// How: ASCII, Latin, Greek, Cyrillic, Hebrew, CJK, Hangul, emoji, box drawing, arrows,
/// combining marks and the invisible chars `drawn` already handles (MAX-270)
#[test]
fn common_chars_are_not_unsure() {
    let common = "az09~ éèçœ€£°±§×÷ αβΩ Жж שא 中文日本語 한국어 😀👍✓✗ ─│┌┘═║▶●■▲ ←→…“”«»• \
                  e\u{301}\u{200B}\u{200D}\u{FE0F}";
    for c in common.chars() {
        assert!(!is_unsure(c), "U+{:04X}", u32::from(c));
    }
}

/// The table was generated for the Unicode version `unicode-width` uses: after an update that
/// changes it, run `tools/widths` again (see its README) and change this test.
/// How: `unicode-width`'s version against the generator's `NEW`
#[test]
fn table_matches_unicode_width() {
    assert_eq!(unicode_width::UNICODE_VERSION, (18, 0, 0));
}

/// The ranges are sorted and apart, so the binary search finds every one.
/// How: each range starts after the previous one ends, and its two ends are found
#[test]
fn ranges_are_sorted() {
    for pair in RANGES.windows(2) {
        assert!(pair[0].1 < pair[1].0, "{pair:X?}");
    }
    for &(start, end) in RANGES {
        for cp in [start, end] {
            if let Some(c) = char::from_u32(cp) {
                assert!(is_unsure(c), "U+{cp:04X}");
            }
        }
    }
}
