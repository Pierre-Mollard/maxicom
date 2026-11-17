//! The dump's layout (MAX-252).

use super::*;

fn plain(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// MAX-252: the dump's bytes per line are the most that fit the panel in groups of 8, at
/// least 8; the offset column grows past 8 hex digits.
/// How: Hex (3 + 1 columns a byte) at 63, 200 and 10 columns; Binary at 63; an end past 4 GiB
#[test]
fn dump_fits_the_panel_by_groups_of_8() {
    let fit = |width, view| DumpLayout::fit(width, view, 0x100).bytes_per_line;
    // 63 - 10 offset columns = 53: 13 bytes fit, 8 kept
    assert_eq!(fit(63, RawSideView::Hex), 8);
    // 200 - 10 = 190: 47 fit, 40 kept
    assert_eq!(fit(200, RawSideView::Hex), 40);
    assert_eq!(fit(10, RawSideView::Hex), 8);
    assert_eq!(fit(63, RawSideView::Binary), 8);

    let layout = DumpLayout::fit(200, RawSideView::Hex, 0x100);
    assert_eq!(layout.offset_digits, 8);
    assert_eq!(layout.body_width, 40 * 4 + 1);
    assert!(layout.offset_width() + layout.body_width <= 200);
    assert_eq!(
        DumpLayout::fit(200, RawSideView::Hex, 1 << 36).offset_digits,
        10
    );
}

/// MAX-252: a dump line's text column starts at the same place on a full line, a short last
/// line and a first line whose start was dropped (its bytes blank); the offset is the line's
/// first byte.
/// How: 8 bytes a line in Hex: "ABCDEFGH", "AB" ESC, and "xyz" 5 bytes into its line
#[test]
fn dump_lines_keep_their_columns() {
    let theme = ThemeConfig::default();
    let layout = DumpLayout::fit(63, RawSideView::Hex, 0x100);
    let line =
        |lead, bytes: &[u8]| plain(&layout.body_line(lead, bytes, RawSideView::Hex, &theme, &[]));
    assert_eq!(line(0, b"ABCDEFGH"), "41 42 43 44 45 46 47 48  ABCDEFGH");
    assert_eq!(line(0, b"AB\x1b"), "41 42 1B                 AB.");
    assert_eq!(line(5, b"xyz"), "               78 79 7A       xyz");
    assert_eq!(plain(&layout.offset_line(3, &theme)), "00000018");
}
