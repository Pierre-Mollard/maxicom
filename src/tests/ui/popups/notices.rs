use super::*;

/// MAX-276: a text is cut anywhere in lines of the width (a path has no spaces), a wide char
/// never split; a row's text cut ends with `…` in its last column.
/// How: ASCII and CJK texts wrapped, then cut and not cut.
#[test]
fn wrap_and_cut() {
    assert_eq!(wrap("abcdefg", 3), ["abc", "def", "g"]);
    assert_eq!(wrap("日本語", 5), ["日本", "語"]);
    assert_eq!(wrap("", 4), [""]);
    assert_eq!(cut("abcdefg", 4), "abc…");
    assert_eq!(cut("abcd", 4), "abcd");
}
