//! How received chars are drawn: what `drawn` keeps of a cell.

use super::*;

/// MAX-270: `drawn` borrows a cell without invisible chars, and keeps a cell's first char
/// whatever it is.
/// How: plain, combining, invisible only after the first, an invisible first char
#[test]
fn drawn_keeps_the_first_char() {
    let drawn = |text| drawn(text, false, false);
    assert!(matches!(drawn("a"), Cow::Borrowed("a")));
    assert!(matches!(drawn("e\u{301}"), Cow::Borrowed(_)));
    assert_eq!(drawn("a\u{FE0F}\u{301}\u{200D}"), "a\u{301}");
    assert_eq!(drawn("\u{200B}"), "\u{200B}");
}

/// MAX-275: `drawn_text` borrows plain text; invisible chars dropped, unsure ones as the
/// marker padded to their width, unless asked.
/// How: plain, a zero-width space, a 1 → 2 char, a wide 2 → 1 char, then both settings on
#[test]
fn drawn_text_like_cells() {
    assert!(matches!(
        drawn_text("board", false, false),
        Cow::Borrowed("board")
    ));
    assert_eq!(drawn_text("a\u{200B}b", false, false), "ab");
    assert_eq!(drawn_text("a\u{3248}b", false, false), "a\u{25AF}b");
    assert_eq!(drawn_text("a\u{2630}b", false, false), "a\u{25AF} b");
    assert_eq!(
        drawn_text("a\u{200B}\u{3248}", true, true),
        "a\u{200B}\u{3248}"
    );
}
