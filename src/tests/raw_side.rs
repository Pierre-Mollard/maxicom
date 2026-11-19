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

use ByteClass::{Ascii, Control, Escape, Invalid, Null, Text, Whitespace};

fn classes(bytes: &[u8], encoding: TextEncoding) -> Vec<ByteClass> {
    byte_classes(bytes, 0..bytes.len(), encoding)
}

/// MAX-265: ASCII by kind, `U` and `~` plain text (no sync class), and in UTF-8 the bytes of a
/// char as text, a broken char (one class per maximal subpart) and `AA` / `FF` invalid.
/// How: `U~` NUL TAB ESC 01 7F, then `é` (C3 A9), `F1 80`, `AA`, `FF`, `€` (E2 82 AC)
#[test]
fn utf8_bytes_are_classed_in_context() {
    assert_eq!(
        classes(b"U~\x00\t\x1b\x01\x7f", TextEncoding::Utf8),
        [Ascii, Ascii, Null, Whitespace, Escape, Control, Control]
    );
    assert_eq!(
        classes(b"\xC3\xA9\xF1\x80a\xAA\xFF\xE2\x82\xAC", TextEncoding::Utf8),
        [
            Text, Text, Invalid, Invalid, Ascii, Invalid, Invalid, Text, Text, Text
        ]
    );
}

/// MAX-265: a char cut by the range's edges is seen whole through the bytes around it; one not
/// complete at the end of the bytes is text, one cut by other bytes is invalid.
/// How: `€` (E2 82 AC) with the range from its second byte, then up to its second byte;
/// E2 82 at the very end, then followed by `a`
#[test]
fn utf8_chars_cut_by_the_range_are_seen_whole() {
    let bytes = b"a\xE2\x82\xACb";
    assert_eq!(
        byte_classes(bytes, 2..5, TextEncoding::Utf8),
        [Text, Text, Ascii]
    );
    assert_eq!(
        byte_classes(bytes, 0..3, TextEncoding::Utf8),
        [Ascii, Text, Text]
    );
    assert_eq!(
        classes(b"a\xE2\x82", TextEncoding::Utf8),
        [Ascii, Text, Text]
    );
    assert_eq!(
        classes(b"\xE2\x82a", TextEncoding::Utf8),
        [Invalid, Invalid, Ascii]
    );
}

/// MAX-265: from 80 up, `ascii` has no char (invalid), `latin1` has C1 controls then chars,
/// `cp437` a char for each byte; below 80 all encodings agree.
/// How: 41 80 9F A0 FF in each
#[test]
fn single_byte_encodings_class_the_high_bytes() {
    let bytes = b"A\x80\x9F\xA0\xFF";
    assert_eq!(
        classes(bytes, TextEncoding::Ascii),
        [Ascii, Invalid, Invalid, Invalid, Invalid]
    );
    assert_eq!(
        classes(bytes, TextEncoding::Latin1),
        [Ascii, Control, Control, Text, Text]
    );
    assert_eq!(
        classes(bytes, TextEncoding::Cp437),
        [Ascii, Text, Text, Text, Text]
    );
}
