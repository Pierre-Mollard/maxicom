//! The dump's own scroll (MAX-252).

use super::*;

/// A dump of 8 bytes a line on 4 rows
fn dump() -> DumpScroller {
    DumpScroller {
        width: 8,
        rows: 4,
        ..DumpScroller::default()
    }
}

/// MAX-252: the dump shows its last lines until scrolled; scrolled it keeps its top line while
/// bytes come, stops at the first line kept, and follows again once back at the live end.
/// How: bytes 0..80 (10 lines), up 3, 16 more bytes, up 100, down 100, 8 more bytes
#[test]
fn dump_follows_the_live_end_until_scrolled() {
    let mut s = dump();
    assert_eq!(s.top_line(0, 80), 6);
    assert_eq!(s.top_line(0, 81), 7, "a line with one byte counts");
    s.scroll_up(3, 0, 80);
    assert_eq!(s.top_line(0, 80), 3);
    assert_eq!(s.top_line(0, 96), 3);
    // the bytes before 40 dropped: the first line kept
    assert_eq!(s.top_line(40, 96), 5);
    s.scroll_up(100, 0, 96);
    assert_eq!(s.top_line(0, 96), 0);
    s.scroll_down(100, 0, 96);
    assert_eq!(s.top_line(0, 104), 9);
}

/// MAX-252: a dump that all fits doesn't scroll, and keeps following; a resize changing the
/// bytes per line keeps the top line's first byte.
/// How: 20 bytes, up 1; scrolled to line 2 of 8 bytes (byte 16), then 4 bytes a line
#[test]
fn dump_scroll_fits_and_resizes() {
    let mut s = dump();
    s.scroll_up(1, 0, 20);
    assert_eq!(s.top_line(0, 80), 6);

    s.scroll_up(4, 0, 80);
    assert_eq!(s.top_line(0, 80), 2);
    s.width = 4;
    assert_eq!(s.top_line(0, 80), 4);
}
