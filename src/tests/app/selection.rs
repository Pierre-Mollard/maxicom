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

/// MAX-126: a drag after a line press extends by whole lines, upward too.
/// How: line press on line 1 dragged to line 0.
#[test]
fn mouse_line_drag_extends_by_lines() {
    let mut src = words();
    let mut state = CopyModeState::default();
    press(&mut state, &mut src, (1, 6), MouseUnit::Line);
    state.apply_action(CopyModeAction::MouseDrag(pos(0, 4), (0, 0)), &mut src);
    assert_eq!(
        release_yank(&mut state, &mut src),
        "foo.bar  baz\nnext line here"
    );
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
