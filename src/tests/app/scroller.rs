use super::*;

/// The status bar's scroll position is always 3 columns wide, so nothing after it moves: a
/// one-digit percent is padded with a space, and between the ends it stays 1..99 (a rounded
/// 0% or 100% would be 2 or 4 columns, and says top or bottom when it isn't).
/// How: the label at the ends, at one and two digits, and next to each end of a long scroll.
#[test]
fn position_is_three_columns() {
    let at = |offset_from_bottom, max_scroll| {
        ScrollStatus {
            max_scroll,
            offset_from_bottom,
        }
        .position_display()
    };
    assert_eq!(at(0, 0), "All");
    assert_eq!(at(0, 100), "Bot");
    assert_eq!(at(100, 100), "Top");
    assert_eq!(at(95, 100), " 5%");
    assert_eq!(at(58, 100), "42%");
    // 1 row off the bottom of 1000: 99.9% rounds to 100
    assert_eq!(at(1, 1000), "99%");
    // 1 row off the top: 0.1% rounds to 0
    assert_eq!(at(999, 1000), " 1%");
    for offset in 0..=1000 {
        assert_eq!(at(offset, 1000).chars().count(), 3, "offset {offset}");
    }
}
