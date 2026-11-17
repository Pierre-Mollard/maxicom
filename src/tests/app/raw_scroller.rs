//! The raw panel pan stays on byte starts (MAX-71).

use super::*;

fn scroller(pan_x: usize, max_pan_x: usize) -> RawSideScroller {
    RawSideScroller { pan_x, max_pan_x }
}

/// The pan moves by whole bytes of the view's width, from a byte start.
/// How: Binary (9 columns a byte) and Hex (3), from an aligned and an unaligned pan.
#[test]
fn pan_moves_by_bytes_of_the_view() {
    let mut s = scroller(0, 1000);
    s.pan_right(2, RawSideView::Binary);
    assert_eq!(s.pan_x, 18);
    s.pan_left(1, RawSideView::Binary);
    assert_eq!(s.pan_x, 9);
    s.pan_left(5, RawSideView::Binary);
    assert_eq!(s.pan_x, 0);

    // an unaligned pan (left by an older step) snaps back to its byte start first
    let mut s = scroller(13, 1000);
    s.pan_right(1, RawSideView::Hex);
    assert_eq!(s.pan_x, 15);
    let mut s = scroller(13, 1000);
    s.pan_left(1, RawSideView::Hex);
    assert_eq!(s.pan_x, 9);
}

/// The limit is rounded up to a byte start, and panning right stops on it.
#[test]
fn pan_limit_is_a_byte_start() {
    let mut s = scroller(0, 0);
    s.set_limit(10, RawSideView::Hex);
    assert_eq!(s.max_pan_x, 12);
    s.pan_right(100, RawSideView::Hex);
    assert_eq!(s.pan_x, 12);
    s.set_limit(10, RawSideView::Octal);
    assert_eq!(s.max_pan_x, 12);
    s.set_limit(10, RawSideView::Binary);
    assert_eq!(s.max_pan_x, 18);
}

/// Centering on a byte and following a cursor both land on byte starts, with the byte
/// shown whole.
/// How: Binary, a 40-column panel, a row of 30 bytes, byte 10 then byte 25.
#[test]
fn focus_lands_on_byte_starts() {
    let mut s = scroller(0, 0);
    s.center_on_byte(RawSideView::Binary, 40, 10, 30);
    assert!(s.pan_x.is_multiple_of(9));
    assert!(s.pan_x <= 90 && 99 <= s.pan_x + 40, "{}", s.pan_x);

    let mut s = scroller(0, 0);
    s.sync_with_cursor(RawSideView::Binary, 40, 25, 30);
    assert!(s.pan_x.is_multiple_of(9));
    assert!(s.pan_x <= 225 && 234 <= s.pan_x + 40, "{}", s.pan_x);
}

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
