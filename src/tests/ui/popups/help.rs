use super::*;

/// Tabs 10 wide in 32 columns: three fit (10 + 1 + 10 + 1 + 10). The window moves only
/// when the selected tab leaves it, by as little as it takes, and never leaves room after
/// the last tab.
#[test]
fn tab_window_follows_the_selection() {
    let widths = [10; 6];
    assert_eq!(tab_window(&widths, 0, 0, 32), 0..3);
    assert_eq!(tab_window(&widths, 2, 0, 32), 0..3);
    assert_eq!(tab_window(&widths, 3, 0, 32), 1..4);
    assert_eq!(tab_window(&widths, 5, 1, 32), 3..6);
    assert_eq!(tab_window(&widths, 4, 3, 32), 3..6);
    assert_eq!(tab_window(&widths, 2, 3, 32), 2..5);
    // the cycle wraps to the first tab
    assert_eq!(tab_window(&widths, 0, 3, 32), 0..3);
    // a first tab kept from a narrower popup, and all fitting
    assert_eq!(tab_window(&widths, 5, 3, 54), 1..6);
    assert_eq!(tab_window(&widths, 5, 3, 65), 0..6);
    // a tab wider than the room is still the one shown
    assert_eq!(tab_window(&widths, 1, 0, 5), 1..2);
}
