//! Copy mode: selection ordering, hit testing, yanking and cursor moves.

use super::*;

struct MockSource {
    lines: Vec<&'static str>,
    raw_view: RawSideView,
}

impl TextSource for MockSource {
    fn total_lines(&mut self) -> usize {
        self.lines.len()
    }
    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        self.lines.get(line_idx).map(|s| s.to_string())
    }
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }
}

fn source() -> MockSource {
    MockSource {
        lines: vec!["hello world", "second line"],
        raw_view: RawSideView::Hex,
    }
}

fn pos(line_idx: usize, char_idx: usize) -> LogicalPosition {
    LogicalPosition {
        line_idx,
        char_idx,
        preferred_col: char_idx,
        byte_idx: 0,
    }
}

/// The selection is returned as (start, end), whatever the drag direction.
/// How: anchor placed after the cursor.
#[test]
fn normalized_selection_orders_anchor_and_cursor() {
    let mut state = CopyModeState::default();
    state.enter_at(1, 3);
    state.selection_anchor = Some(pos(0, 6));
    let (start, end) = state.normalized_selection().unwrap();
    assert_eq!((start.line_idx, start.char_idx), (0, 6));
    assert_eq!((end.line_idx, end.char_idx), (1, 3));
}

/// Hit testing across a multi-line selection, edges included.
/// How: checks cells inside, on and outside the bounds.
#[test]
fn is_selected_multi_line() {
    let mut state = CopyModeState::default();
    state.enter_at(2, 1);
    state.selection_anchor = Some(pos(0, 4));
    assert!(!state.is_selected(0, 3));
    assert!(state.is_selected(0, 4));
    assert!(state.is_selected(1, 0));
    assert!(state.is_selected(2, 1));
    assert!(!state.is_selected(2, 2));
    assert!(!state.is_selected(3, 0));
}

/// Yank copies the selected text across lines and leaves copy mode.
/// How: mock source, selection from line 0 to 1.
#[test]
fn yank_multi_line_selection() {
    let mut state = CopyModeState::default();
    state.enter_at(1, 5);
    state.selection_anchor = Some(pos(0, 6));
    match state.apply_action(CopyModeAction::Yank, &mut source()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "world\nsecond"),
        _ => panic!("expected a yank"),
    }
    assert!(!state.active);
}

/// Raw yank copies the selected bytes in the raw view format.
/// How: selecting "he" in Hex gives "68 65".
#[test]
fn yank_raw_formats_selected_bytes() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 1);
    state.selection_anchor = Some(pos(0, 0));
    match state.apply_action(CopyModeAction::YankRaw, &mut source()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "68 65"),
        _ => panic!("expected a raw yank"),
    }
}

/// Yank with nothing selected just exits.
/// How: checks the `Exited` result.
#[test]
fn yank_without_selection_exits() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 0);
    assert!(matches!(
        state.apply_action(CopyModeAction::Yank, &mut source()),
        CopyActionResult::Exited
    ));
}

/// Eviction moves cursor and anchor up, clamping to line 0 when their line is gone.
/// How: anchor on line 1, cursor on line 4, evict 2 lines.
#[test]
fn rebase_after_eviction_moves_cursor_and_anchor() {
    let mut state = CopyModeState::default();
    state.enter_at(4, 3);
    state.selection_anchor = Some(pos(1, 2));
    state.rebase_after_eviction(2);
    assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (2, 3));
    let anchor = state.selection_anchor.unwrap();
    assert_eq!((anchor.line_idx, anchor.char_idx), (0, 2));
}

/// Moving up/down clamps to short lines but returns to the preferred column.
/// How: long, short, long lines.
#[test]
fn vertical_moves_keep_preferred_col() {
    let mut src = MockSource {
        lines: vec!["long line here", "ab", "another long one"],
        raw_view: RawSideView::Hex,
    };
    let mut state = CopyModeState::default();
    state.enter_at(0, 10);
    state.apply_action(CopyModeAction::MoveDown, &mut src);
    assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (1, 1));
    state.apply_action(CopyModeAction::MoveDown, &mut src);
    assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (2, 10));
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
    state.apply_action(CopyModeAction::ToggleSide, &mut colored());
    assert_eq!(state.side, CopySide::Raw);
    assert_eq!(at(&state), (0, 1, 6));
    state.apply_action(CopyModeAction::ToggleSide, &mut colored());
    assert_eq!(state.side, CopySide::Text);
    assert_eq!(at(&state), (0, 1, 6));

    let mut no_panel = ColoredSource {
        raw_view: RawSideView::Off,
    };
    state.apply_action(CopyModeAction::ToggleSide, &mut no_panel);
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
    state.apply_action(CopyModeAction::ToggleSide, src);
    assert_eq!(at(&state), (0, 0, 5));
    state.apply_action(CopyModeAction::MoveLeft, src);
    // "m" of ESC[32m: not shown, the ghost is on "o"
    assert_eq!(at(&state), (0, 0, 4));
    state.apply_action(CopyModeAction::EndOfLine, src);
    // the "\n": the ghost is on the last char "k"
    assert_eq!(at(&state), (0, 1, 12));
    state.apply_action(CopyModeAction::MoveRight, src);
    assert_eq!(at(&state), (1, 0, 0));
    state.apply_action(CopyModeAction::MoveLeft, src);
    assert_eq!(at(&state), (0, 1, 12));

    // vertical moves keep the byte column, clamped to the line's bytes
    state.apply_action(CopyModeAction::MoveDown, src);
    assert_eq!(at(&state), (1, 2, 4));
    state.apply_action(CopyModeAction::MoveUp, src);
    assert_eq!(at(&state), (0, 1, 12));
    // word motions don't apply to bytes
    assert!(matches!(
        state.apply_action(CopyModeAction::NextWord, src),
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
        state.apply_action(CopyModeAction::ToggleSide, src);
        state.apply_action(CopyModeAction::MoveLeft, src);
        state.apply_action(CopyModeAction::Select, src);
        state.apply_action(CopyModeAction::StartOfLine, src);
        match state.apply_action(yank, src) {
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
    state.apply_action(CopyModeAction::Select, src);
    state.apply_action(CopyModeAction::MoveRight, src);
    state.apply_action(CopyModeAction::ToggleSide, src);
    let anchor = state.selection_anchor.unwrap();
    assert_eq!((anchor.char_idx, anchor.byte_idx), (0, 5));
    assert_eq!(at(&state), (0, 1, 6));
    state.apply_action(CopyModeAction::ToggleSide, src);
    match state.apply_action(CopyModeAction::Yank, src) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "ok"),
        _ => panic!("expected a yank"),
    }
}

fn words() -> MockSource {
    MockSource {
        lines: vec!["foo.bar  baz", "next line here"],
        raw_view: RawSideView::Hex,
    }
}

fn press(state: &mut CopyModeState, src: &mut MockSource, at: (usize, usize), unit: MouseUnit) {
    let action = CopyModeAction::MousePress(pos(at.0, at.1), CopySide::Text, unit);
    state.apply_action(action, src);
}

fn release_yank(state: &mut CopyModeState, src: &mut MockSource) -> String {
    match state.apply_action(CopyModeAction::MouseRelease, src) {
        CopyActionResult::Yanked(text) => text,
        _ => panic!("expected a yank"),
    }
}

/// MAX-126: a word press selects the vi `iw` word under it, or the run of whitespace; the
/// release yanks it without a drag.
/// How: presses on a word char, on punctuation and on whitespace of "foo.bar  baz".
#[test]
fn mouse_word_press_selects_the_word() {
    let mut src = words();
    let mut state = CopyModeState::default();
    for (col, word) in [(1, "foo"), (3, "."), (6, "bar"), (7, "  "), (11, "baz")] {
        press(&mut state, &mut src, (0, col), MouseUnit::Word);
        assert_eq!(release_yank(&mut state, &mut src), word, "col {col}");
        assert!(!state.active);
    }
}

/// MAX-126: a line press selects the whole line.
/// How: press in the middle of line 1, release.
#[test]
fn mouse_line_press_selects_the_line() {
    let mut src = words();
    let mut state = CopyModeState::default();
    press(&mut state, &mut src, (1, 6), MouseUnit::Line);
    assert_eq!(release_yank(&mut state, &mut src), "next line here");
}

/// MAX-126: a drag after a word press extends by whole words both ways, across lines, and
/// always keeps the first word.
/// How: press on "bar", drag into "baz", into "foo", then to "line" on the next line.
#[test]
fn mouse_word_drag_extends_by_words() {
    let mut src = words();
    let mut state = CopyModeState::default();
    let drag = |state: &mut CopyModeState, src: &mut MockSource, at: (usize, usize)| {
        state.apply_action(CopyModeAction::MouseDrag(pos(at.0, at.1), (0, 0)), src);
    };

    press(&mut state, &mut src, (0, 5), MouseUnit::Word);
    drag(&mut state, &mut src, (0, 10));
    assert_eq!(release_yank(&mut state, &mut src), "bar  baz");

    press(&mut state, &mut src, (0, 5), MouseUnit::Word);
    drag(&mut state, &mut src, (0, 10));
    drag(&mut state, &mut src, (0, 1));
    assert_eq!(release_yank(&mut state, &mut src), "foo.bar");

    press(&mut state, &mut src, (0, 5), MouseUnit::Word);
    drag(&mut state, &mut src, (1, 6));
    assert_eq!(release_yank(&mut state, &mut src), "bar  baz\nnext line");
}

/// MAX-126: a drag after a line press extends by whole lines, upward too; the raw side
/// keeps char presses.
/// How: line press on line 1 dragged to line 0; a word press on the raw side yanks nothing.
#[test]
fn mouse_line_drag_and_raw_side() {
    let mut src = words();
    let mut state = CopyModeState::default();
    press(&mut state, &mut src, (1, 6), MouseUnit::Line);
    state.apply_action(CopyModeAction::MouseDrag(pos(0, 4), (0, 0)), &mut src);
    assert_eq!(
        release_yank(&mut state, &mut src),
        "foo.bar  baz\nnext line here"
    );

    let action = CopyModeAction::MousePress(pos(0, 1), CopySide::Raw, MouseUnit::Word);
    state.apply_action(action, &mut src);
    assert!(state.selection_anchor.is_none());
    assert!(matches!(
        state.apply_action(CopyModeAction::MouseRelease, &mut src),
        CopyActionResult::None
    ));
}

/// MAX-126: quick presses on the same cell and side count 2 (word) and 3 (line), then start
/// over; a slow press, another cell, the other side or a reset count 1.
/// How: made-up instants around the 400 ms limit.
#[test]
fn click_counter_counts_quick_presses() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let text = CopySide::Text;
    let mut clicks = ClickCounter::default();
    assert_eq!(clicks.press(ms(0), (0, 1), text), MouseUnit::Char);
    assert_eq!(clicks.press(ms(300), (0, 1), text), MouseUnit::Word);
    assert_eq!(clicks.press(ms(600), (0, 1), text), MouseUnit::Line);
    assert_eq!(clicks.press(ms(900), (0, 1), text), MouseUnit::Char);

    assert_eq!(clicks.press(ms(1400), (0, 1), text), MouseUnit::Char);
    assert_eq!(clicks.press(ms(1500), (0, 2), text), MouseUnit::Char);
    assert_eq!(
        clicks.press(ms(1600), (0, 2), CopySide::Raw),
        MouseUnit::Char
    );
    clicks.reset();
    assert_eq!(
        clicks.press(ms(1700), (0, 2), CopySide::Raw),
        MouseUnit::Char
    );
    assert_eq!(
        clicks.press(ms(1800), (0, 2), CopySide::Raw),
        MouseUnit::Word
    );
}

/// MAX-127: `V` selects whole lines from the first one's start to the last one's end,
/// whatever the columns of the cursor and the anchor; on one line too.
/// How: `V` on line 1 col 3, up to line 0 col 3: both lines selected and yanked whole.
#[test]
fn select_line_takes_whole_lines() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 4);
    state.apply_action(CopyModeAction::SelectLine, &mut source());
    match state.apply_action(CopyModeAction::Yank, &mut source()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "hello world"),
        _ => panic!("expected a yank"),
    }

    state.enter_at(1, 3);
    state.apply_action(CopyModeAction::SelectLine, &mut source());
    state.apply_action(CopyModeAction::MoveUp, &mut source());
    assert!(state.is_selected(0, 0));
    assert!(state.is_selected(1, 10));
    match state.apply_action(CopyModeAction::Yank, &mut source()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, "hello world\nsecond line"),
        _ => panic!("expected a yank"),
    }
    assert!(!state.line_select);
}

/// MAX-127: as in vim, `v` and `V` switch the selection to their kind and keep its
/// anchor; the same key again ends it.
/// How: `V`, `v`, `v`, then `V`, `V`, from a cursor on line 0.
#[test]
fn select_and_select_line_switch_and_toggle() {
    let mut state = CopyModeState::default();
    state.enter_at(0, 2);
    state.apply_action(CopyModeAction::SelectLine, &mut source());
    state.apply_action(CopyModeAction::MoveRight, &mut source());
    state.apply_action(CopyModeAction::Select, &mut source());
    assert_eq!(state.selection_anchor, Some(pos(0, 2)));
    assert!(!state.line_select);
    assert!(!state.is_selected(0, 0));
    state.apply_action(CopyModeAction::Select, &mut source());
    assert_eq!(state.selection_anchor, None);

    state.apply_action(CopyModeAction::SelectLine, &mut source());
    assert!(state.line_select);
    state.apply_action(CopyModeAction::SelectLine, &mut source());
    assert_eq!(state.selection_anchor, None);
    assert!(!state.line_select);
}

/// MAX-127: a line-wise raw yank takes every byte of the lines, from the text side or the
/// raw side, the escape sequences and the line ending included.
/// How: colored source ("ok" in `ESC[32m` ... `\r\n`), `V` on the "k".
#[test]
fn select_line_raw_yank_takes_every_byte() {
    let whole = "1B 5B 33 32 6D 6F 6B 1B 5B 30 6D 0D 0A";
    let mut state = CopyModeState::default();
    state.enter_at(0, 1);
    state.apply_action(CopyModeAction::SelectLine, &mut colored());
    match state.apply_action(CopyModeAction::YankRaw, &mut colored()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, whole),
        _ => panic!("expected a raw yank"),
    }

    state.enter_at(0, 1);
    state.apply_action(CopyModeAction::ToggleSide, &mut colored());
    state.apply_action(CopyModeAction::SelectLine, &mut colored());
    match state.apply_action(CopyModeAction::Yank, &mut colored()) {
        CopyActionResult::Yanked(text) => assert_eq!(text, whole),
        _ => panic!("expected a raw yank"),
    }
}
