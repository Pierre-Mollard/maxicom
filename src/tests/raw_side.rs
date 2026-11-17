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
