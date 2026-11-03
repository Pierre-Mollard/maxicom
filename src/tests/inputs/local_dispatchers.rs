
//! Pasting into single line input fields (#28).

use super::*;

fn pasted(value: &str, cursor: usize, text: &str) -> (String, usize) {
    let mut input = Input::new(value.to_string()).with_cursor(cursor);
    paste_into_input(&mut input, text);
    (input.value().to_string(), input.cursor())
}

/// The text goes in at the cursor, and the cursor ends after it.
/// How: pastes in the middle of a value.
#[test]
fn paste_inserts_at_cursor() {
    assert_eq!(pasted("ad", 1, "bc"), ("abcd".to_string(), 3));
}

/// The cursor counts chars, so a paste after a non-ASCII char lands at the right place.
/// How: cursor after "é" (2 bytes, 1 char).
#[test]
fn paste_after_non_ascii() {
    assert_eq!(pasted("éz", 1, "x"), ("éxz".to_string(), 2));
}

/// Trailing line breaks are dropped, inner ones (LF, CRLF, CR) and tabs become one space,
/// other control chars are skipped.
/// How: a path copied with its newline, then a multi-line text with an ESC.
#[test]
fn paste_flattens_line_breaks() {
    assert_eq!(pasted("", 0, "log.txt\r\n"), ("log.txt".to_string(), 7));
    assert_eq!(
        pasted("", 0, "a\nb\r\nc\rd\te\x1bf"),
        ("a b c d ef".to_string(), 10)
    );
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn char_key(c: char) -> KeyEvent {
    key(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Sends vi normal mode keys to `value` with the cursor at char `cursor`.
fn vi_normal(value: &str, cursor: usize, keys: &str) -> (String, usize) {
    let mut input = Input::new(value.to_string()).with_cursor(cursor);
    let mut state = ViState {
        mode: ViMode::Normal,
        pending: None,
    };
    for c in keys.chars() {
        dispatch_input_key_vi(&mut input, &mut state, char_key(c));
    }
    (input.value().to_string(), input.cursor())
}

/// Ctrl+K keeps the text before the cursor, which counts chars (#32).
/// How: once in ASCII, then with a 2-byte char before the cursor, where a byte slice
/// would cut "é" in half and panic.
#[test]
fn ctrl_k_kills_to_end_after_non_ascii() {
    let ctrl_k = key(KeyCode::Char('k'), KeyModifiers::CONTROL);
    for (value, cursor, kept) in [("ab|cd", 2, "ab"), ("é|x", 1, "é"), ("éa|b", 2, "éa")] {
        let value = value.replace('|', "");
        let mut input = Input::new(value).with_cursor(cursor);
        dispatch_input_key(&mut input, ctrl_k);
        assert_eq!((input.value(), input.cursor()), (kept, cursor));
    }
}

/// Vi `D`, `C` and `d$` keep the text before the cursor, `d0` the text from it (#32).
/// How: each on "éa|b", cursor after the 2-byte char and one ASCII char.
#[test]
fn vi_kills_after_non_ascii() {
    assert_eq!(vi_normal("éab", 2, "D"), ("éa".to_string(), 2));
    assert_eq!(vi_normal("éab", 2, "C"), ("éa".to_string(), 2));
    assert_eq!(vi_normal("éab", 2, "d$"), ("éa".to_string(), 2));
    assert_eq!(vi_normal("éab", 2, "d0"), ("b".to_string(), 0));
}

/// Text objects (`diw`, `ci"`…) cut on char bounds, not bytes (#32).
/// How: `diw` on "ab" after "é ", then after "éé " where a byte slice would land inside a
/// char and panic.
#[test]
fn vi_text_object_after_non_ascii() {
    assert_eq!(vi_normal("é ab", 2, "diw"), ("é ".to_string(), 2));
    assert_eq!(vi_normal("éé ab", 3, "diw"), ("éé ".to_string(), 3));
}
