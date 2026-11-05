//! The terminal size dialog's fields.

use super::*;
use crossterm::event::{KeyEvent, KeyModifiers};

fn type_key(dialog: &mut ResizeDialog, c: char) {
    let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    dialog.handle_action(DialogAction::HandleKey(key));
}

/// A field takes 4 digits at most, so its value fits its column and the note after it stays
/// in the popup: a fifth typed digit is dropped, a paste keeps the digits that fit, and the
/// arrows stop at 9999.
/// How: rows typed past 4 digits, cols pasted with 8, then Right on 9999.
#[test]
fn fields_take_four_digits() {
    let mut dialog = ResizeDialog::new(24, 80);
    for c in "123456".chars() {
        type_key(&mut dialog, c);
    }
    assert_eq!(dialog.rows_input.value(), "2412");

    dialog.handle_action(DialogAction::CycleNext);
    dialog.handle_action(DialogAction::Paste("1x2345678".to_string()));
    assert_eq!(dialog.cols_input.value(), "8012");

    dialog.cols_input = Input::new("9999".to_string());
    dialog.handle_action(DialogAction::Right);
    assert_eq!(dialog.cols_input.value(), "9999");
}
