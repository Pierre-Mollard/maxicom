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
