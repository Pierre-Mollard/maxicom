//! Line mode's raw side in copy mode: the cursor on the bytes of each line, its char as the
//! ghost in the text (MAX-71).

use super::*;
use crate::app::selection::{CopyModeState, CopySide, TextSource};

/// A copy action routed as `LineMode` does: Tab switches the side, the raw side's actions go
/// here, the text's to `CopyModeState`
fn act<S: TextSource>(
    state: &mut CopyModeState,
    action: CopyModeAction,
    source: &mut S,
) -> CopyActionResult {
    let on_raw = match action {
        CopyModeAction::MousePress(_, side, _) => side == CopySide::Raw,
        _ => state.side == CopySide::Raw,
    };
    if action == CopyModeAction::ToggleSide {
        toggle_side(state, source)
    } else if on_raw {
        copy_action(state, action, source)
    } else {
        state.apply_action(action, source)
    }
}

/// A source whose raw bytes differ from its text, like a received colored line: text "ok",
/// bytes `ESC[32m` `ok` `ESC[0m` `\r\n` (13 bytes, "o" is byte 5), then a plain line
struct ColoredSource {
    raw_view: RawSideView,
}

impl TextSource for ColoredSource {
    fn total_lines(&mut self) -> usize {
        2
    }
    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        ["ok", "abc"].get(line_idx).map(|s| s.to_string())
    }
    fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
        let raw: [&[u8]; 2] = [b"\x1b[32mok\x1b[0m\r\n", b"abc\r\n"];
        raw.get(line_idx).map(|bytes| bytes.to_vec())
    }
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }
}

fn colored() -> ColoredSource {
    ColoredSource {
        raw_view: RawSideView::Hex,
    }
}

fn at(state: &CopyModeState) -> (usize, usize, usize) {
    (
        state.cursor.line_idx,
        state.cursor.char_idx,
        state.cursor.byte_idx,
    )
}

/// MAX-71: the toggle moves the cursor to the raw side on its char's first byte and back;
/// with no raw panel it does nothing.
#[test]
fn toggle_side_converts_the_cursor() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 1);
    act(&mut state, CopyModeAction::ToggleSide, &mut colored());
    assert_eq!(state.side, CopySide::Raw);
    assert_eq!(at(&state), (0, 1, 6));
    act(&mut state, CopyModeAction::ToggleSide, &mut colored());
    assert_eq!(state.side, CopySide::Text);
    assert_eq!(at(&state), (0, 1, 6));

    let mut no_panel = ColoredSource {
        raw_view: RawSideView::Off,
    };
    act(&mut state, CopyModeAction::ToggleSide, &mut no_panel);
    assert_eq!(state.side, CopySide::Text);
}

/// MAX-71: on the raw side the cursor steps over every byte, the escape sequences and the
/// line ending included; its char (the ghost in the text) is the char holding the byte, the
/// next char shown for a byte not shown, the last char for the line ending.
#[test]
fn raw_cursor_steps_over_every_byte() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 0);
    let src = &mut colored();
    act(&mut state, CopyModeAction::ToggleSide, src);
    assert_eq!(at(&state), (0, 0, 5));
    act(&mut state, CopyModeAction::MoveLeft, src);
    // "m" of ESC[32m: not shown, the ghost is on "o"
    assert_eq!(at(&state), (0, 0, 4));
    act(&mut state, CopyModeAction::EndOfLine, src);
    // the "\n": the ghost is on the last char "k"
    assert_eq!(at(&state), (0, 1, 12));
    act(&mut state, CopyModeAction::MoveRight, src);
    assert_eq!(at(&state), (1, 0, 0));
    act(&mut state, CopyModeAction::MoveLeft, src);
    assert_eq!(at(&state), (0, 1, 12));

    // vertical moves keep the byte column, clamped to the line's bytes
    act(&mut state, CopyModeAction::MoveDown, src);
    assert_eq!(at(&state), (1, 2, 4));
    act(&mut state, CopyModeAction::MoveUp, src);
    assert_eq!(at(&state), (0, 1, 12));
    // word motions don't apply to bytes
    assert!(matches!(
        act(&mut state, CopyModeAction::NextWord, src),
        CopyActionResult::None
    ));
}

/// MAX-71: a selection made on the raw side yanks its bytes in the panel's format, `y` and
/// `Y` alike, whatever the direction it was made in.
/// How: select from byte 4 back to byte 0 (the whole ESC[32m, no char of its own).
#[test]
fn raw_selection_yanks_its_bytes() {
    for yank in [CopyModeAction::Yank, CopyModeAction::YankRaw] {
        let mut state = CopyModeState::default();
        state.enter_at(0, 0);
        let src = &mut colored();
        act(&mut state, CopyModeAction::ToggleSide, src);
        act(&mut state, CopyModeAction::MoveLeft, src);
        act(&mut state, CopyModeAction::Select, src);
        act(&mut state, CopyModeAction::StartOfLine, src);
        match act(&mut state, yank, src) {
            CopyActionResult::Yanked(text) => assert_eq!(text, "1B 5B 33 32 6D"),
            _ => panic!("expected a yank"),
        }
        assert!(!state.active);
        assert_eq!(state.side, CopySide::Text);
    }
}

/// MAX-71: the selection anchor is converted with the cursor, both ways.
/// How: select "ok" in text, to the raw side (the anchor on byte 5), back to text.
#[test]
fn toggle_side_converts_the_anchor() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 0);
    let src = &mut colored();
    act(&mut state, CopyModeAction::Select, src);
    act(&mut state, CopyModeAction::MoveRight, src);
    act(&mut state, CopyModeAction::ToggleSide, src);
    let anchor = state.selection_anchor.unwrap();
    assert_eq!((anchor.char_idx, anchor.byte_idx), (0, 5));
    assert_eq!(at(&state), (0, 1, 6));
    act(&mut state, CopyModeAction::ToggleSide, src);
    match act(&mut state, CopyModeAction::Yank, src) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "ok"),
        _ => panic!("expected a yank"),
    }
}

/// MAX-127: a line-wise raw yank takes every byte of the lines, from the text side or the
/// raw side, the escape sequences and the line ending included.
/// How: colored source ("ok" in `ESC[32m` ... `\r\n`), `V` on the "k".
#[test]
fn select_line_raw_yank_takes_every_byte() {
    let whole = "1B 5B 33 32 6D 6F 6B 1B 5B 30 6D 0D 0A";
    let mut state = CopyModeState::default();
    state.enter_at(0, 1);
    act(&mut state, CopyModeAction::SelectLine, &mut colored());
    match act(&mut state, CopyModeAction::YankRaw, &mut colored()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, whole),
        _ => panic!("expected a raw yank"),
    }

    state.enter_at(0, 1);
    act(&mut state, CopyModeAction::ToggleSide, &mut colored());
    act(&mut state, CopyModeAction::SelectLine, &mut colored());
    match act(&mut state, CopyModeAction::Yank, &mut colored()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, whole),
        _ => panic!("expected a raw yank"),
    }
}

/// MAX-126: the raw side keeps char presses: a word press there selects nothing and its
/// release yanks nothing.
/// How: a word press on byte 1 of the colored line.
#[test]
fn raw_press_selects_a_byte() {
    let mut state = CopyModeState::default();
    let src = &mut colored();
    let at = LogicalPosition {
        line_idx: 0,
        byte_idx: 1,
        ..LogicalPosition::default()
    };
    act(
        &mut state,
        CopyModeAction::MousePress(at, CopySide::Raw, MouseUnit::Word),
        src,
    );
    assert_eq!(state.side, CopySide::Raw);
    assert!(state.selection_anchor.is_none());
    assert!(matches!(
        act(&mut state, CopyModeAction::MouseRelease, src),
        CopyActionResult::None
    ));
}
