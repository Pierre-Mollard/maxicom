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

/// MAX-274: a key's row is ` key  help    name`, the name at the room's end less a column for
/// the scrollbar; a name that would leave the help under `MIN_HELP` columns is left out, and
/// the help takes its room, cut with `…`; a row without help shows the name.
/// How: one row in 60 columns, then in 40; a row with no help.
#[test]
fn key_row_puts_the_name_last() {
    let theme = ThemeConfig::default();
    let row = KeyRow {
        key: "e".to_string(),
        name: "toggle-display-timestamp".to_string(),
        help: "Show or hide each line's receive time".to_string(),
        order: (0, 0, 0, false),
    };
    let text = |line: Line| line.to_string();
    let wide = text(row.line(3, 70, &theme));
    assert_eq!(wide.width(), 69, "{wide:?}");
    assert!(wide.starts_with(" e    Show or hide"), "{wide:?}");
    assert!(wide.ends_with("time  toggle-display-timestamp"), "{wide:?}");

    let narrow = text(row.line(3, 40, &theme));
    assert_eq!(narrow, " e    Show or hide each line's receive…");

    let bare = KeyRow {
        help: String::new(),
        ..row
    };
    assert_eq!(
        text(bare.line(3, 40, &theme)),
        " e    toggle-display-timestamp"
    );
}
