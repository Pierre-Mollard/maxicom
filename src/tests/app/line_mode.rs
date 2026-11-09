//! RX ingest: line splitting, EOL modes, history cap and known data-model bugs.

use super::*;
use crate::app::search::SearchModeKind;
use crossterm::event::{KeyCode, KeyModifiers};

fn line_mode(max_lines: usize) -> LineMode {
    LineMode::new(max_lines, 1000)
}

fn texts(lm: &LineMode) -> Vec<&str> {
    lm.history().iter().map(|e| e.text.as_str()).collect()
}

/// CRLF and LF both commit a line, and the stored text keeps its EOL.
/// How: one chunk with two lines.
#[test]
fn crlf_line_is_committed_with_its_eol() {
    let mut lm = line_mode(10);
    lm.feed_rx(false, TextEncoding::Utf8, b"abc\r\ndef\n");
    assert_eq!(texts(&lm), ["abc\r\n", "def\n"]);
    assert!(lm.partial_rx().is_empty());
}

/// A line split over two chunks is joined into one entry.
/// How: feeds "ab" then "c\n".
#[test]
fn line_split_across_chunks() {
    let mut lm = line_mode(10);
    lm.feed_rx(false, TextEncoding::Utf8, b"ab");
    assert_eq!(lm.partial_rx(), b"ab");
    lm.feed_rx(false, TextEncoding::Utf8, b"c\n");
    assert_eq!(texts(&lm), ["abc\n"]);
}

/// A lone CR restarts the line (progress-bar style).
/// How: "10%\r20%\n" keeps only the last part.
#[test]
fn lone_cr_overwrites_line_in_auto_mode() {
    let mut lm = line_mode(10);
    lm.feed_rx(false, TextEncoding::Utf8, b"progress 10%\rprogress 20%\n");
    assert_eq!(texts(&lm), ["progress 20%\n"]);
}

/// MAX-181: with add LF a lone CR ends its line, with its bytes as received; the
/// device's CR LF stays one line, also split across two chunks.
/// How: "a\rb\n", then "c\r" and "\nd\n" fed apart.
#[test]
fn add_lf_ends_a_line_at_a_lone_cr() {
    let mut lm = line_mode(10);
    lm.feed_rx(true, TextEncoding::Utf8, b"a\rb\n");
    assert_eq!(texts(&lm), ["a\r", "b\n"]);
    lm.feed_rx(true, TextEncoding::Utf8, b"c\r");
    lm.feed_rx(true, TextEncoding::Utf8, b"\nd\n");
    assert_eq!(texts(&lm), ["a\r", "b\n", "c\r\n", "d\n"]);
}

/// History keeps only the newest max_lines entries.
/// How: three lines with a cap of 2.
#[test]
fn history_is_capped() {
    let mut lm = line_mode(2);
    lm.feed_rx(false, TextEncoding::Utf8, b"1\n2\n3\n");
    assert_eq!(texts(&lm), ["2\n", "3\n"]);
}

/// Search matches keep pointing at the same line when old lines are dropped (#19).
/// How: match on the newest line, push more lines, check the line the index points at.
#[test]
fn search_matches_follow_their_line_when_history_is_full() {
    let mut lm = line_mode(2);
    lm.feed_rx(false, TextEncoding::Utf8, b"a\ntarget\n");
    *lm.search_mut() = SearchState {
        active: true,
        query: Input::from("target"),
        ..Default::default()
    };
    lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(lm.search().matches[0].line_idx, 1);

    lm.feed_rx(false, TextEncoding::Utf8, b"other\n");
    let idx = lm.search().matches[0].line_idx;
    assert_eq!(lm.history()[idx].text, "target\n");

    // once its line is dropped too, the match is gone
    lm.feed_rx(false, TextEncoding::Utf8, b"more\n");
    assert!(lm.search().matches.is_empty());
}

/// The copy cursor stays on its line when old lines are dropped (#19).
/// How: cursor on "b", stream two lines into a 3-line history.
#[test]
fn copy_cursor_follows_its_line_when_history_is_full() {
    let mut lm = line_mode(3);
    lm.feed_rx(false, TextEncoding::Utf8, b"a\nb\nc\n");
    lm.copy_state.enter_at(1, 0);
    lm.feed_rx(false, TextEncoding::Utf8, b"d\n");
    assert_eq!(lm.history()[lm.copy_state().cursor.line_idx].text, "b\n");
}

/// MAX-164: a mark goes on the last line, or on the copy cursor's in copy mode; it follows its
/// line when old lines are dropped, goes with it, and clearing the history takes them all.
/// How: a 3-line history: a on "c" (last), b on "b" (copy cursor), then "d" and "e" streamed in,
/// then cleared.
#[test]
fn marks_follow_their_lines_and_go_with_them() {
    let mut lm = line_mode(3);
    assert_eq!(lm.add_mark('a'), None);
    lm.feed_rx(false, TextEncoding::Utf8, b"a\nb\nc\n");
    assert_eq!(lm.add_mark('a'), Some(2));
    assert_eq!(lm.add_mark('A'), None);
    lm.copy_state.enter_at(1, 3);
    assert_eq!(lm.add_mark('b'), Some(1));
    lm.copy_state.active = false;

    lm.feed_rx(false, TextEncoding::Utf8, b"d\n");
    assert_eq!(lm.history()[lm.marks().get('a').unwrap()].text, "c\n");
    assert_eq!(lm.history()[lm.marks().get('b').unwrap()].text, "b\n");
    lm.feed_rx(false, TextEncoding::Utf8, b"e\n");
    assert_eq!(lm.marks().sorted(), [('a', 0)]);

    assert_eq!(lm.remove_mark('a'), Some(0));
    lm.add_mark('c');
    lm.clear_history();
    assert!(lm.marks().sorted().is_empty());
}

/// A large configured history doesn't reserve memory up front (#21).
/// How: capacity of a fresh 100k-line LineMode.
#[test]
fn history_is_not_preallocated() {
    let lm = line_mode(100_000);
    assert!(lm.history().capacity() < 100_000);
    assert!(lm.local_send_history.capacity() < 100_000);
}

/// Sent lines are capped at the configured `send_history_size`, oldest dropped first (#21).
/// How: submits 8 lines with a cap of 3, checks size, oldest kept and history up.
#[test]
fn send_history_is_capped() {
    let mut lm = LineMode::new(10, 3);
    let settings = SettingsConfig::default();
    for i in 0..8 {
        lm.user_input = Input::from(format!("cmd {i}").as_str());
        lm.update_action_lineinput(&settings, LineInputAction::Submit);
    }
    let kept: Vec<&str> = lm.local_send_history.iter().map(String::as_str).collect();
    assert_eq!(kept, ["cmd 5", "cmd 6", "cmd 7"]);

    lm.history_up();
    assert_eq!(lm.input().value(), "cmd 7");
}

/// A send history size of 0 keeps nothing, and history up does nothing.
/// How: cap 0, one submit, then history up on an empty input.
#[test]
fn send_history_size_zero_keeps_nothing() {
    let mut lm = LineMode::new(10, 0);
    lm.user_input = Input::from("cmd");
    lm.update_action_lineinput(&SettingsConfig::default(), LineInputAction::Submit);
    assert!(lm.local_send_history.is_empty());
    lm.history_up();
    assert_eq!(lm.input().value(), "");
}

/// Local echo (`push_history`) rebases too, not only received chunks.
/// How: match on line 1 of a full history, then push one echoed line.
#[test]
fn push_history_rebases_line_refs() {
    let mut lm = line_mode(2);
    lm.feed_rx(false, TextEncoding::Utf8, b"a\ntarget\n");
    lm.copy_state.enter_at(1, 0);
    lm.push_history("echo".to_string());
    assert_eq!(lm.copy_state().cursor.line_idx, 0);
}

/// A Latin1 line keeps its decoded text and the byte received (#20).
/// How: E9 received in Latin1 is "é" as text and E9 on the wire.
#[test]
fn latin1_line_keeps_wire_bytes() {
    let mut lm = line_mode(10);
    lm.feed_rx(false, TextEncoding::Latin1, &[0xE9, b'\n']);
    assert_eq!(lm.history()[0].text, "é\n");
    assert_eq!(lm.history()[0].wire_bytes(), [0xE9, b'\n']);
}
/// A multi-line paste in the prompt acts like typing it: each line break submits, all
/// submitted lines go out in one write, and the text after the last break stays typed.
/// How: pastes "ls\r\npwd\nec" in a prompt that already holds "a".
#[test]
fn paste_submits_each_line_in_one_write() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    let eol = settings.line_mode.tx_eol.as_bytes();
    lm.update_action_lineinput(&settings, LineInputAction::Paste("a".to_string()));

    let effect = lm.update_action_lineinput(
        &settings,
        LineInputAction::Paste("ls\r\npwd\nec".to_string()),
    );
    let UpdateEffect::Transmit(bytes) = effect else {
        panic!("expected one transmit");
    };
    assert_eq!(bytes, [b"als", eol, b"pwd", eol].concat());
    assert_eq!(lm.input().value(), "ec");
}

/// Feeds one line received as `wire` (plus LF) with `encoding`
fn received(encoding: TextEncoding, wire: &[u8]) -> LineMode {
    let mut lm = line_mode(10);
    let mut line = wire.to_vec();
    line.push(b'\n');
    lm.feed_rx(false, encoding, &line);
    lm
}

/// The bytes raw yank and the raw panel get for an entry (`get_line_raw_bytes`) are the
/// bytes received, whatever the encoding (#20).
/// How: one byte that is not plain ASCII, received with each encoding.
#[test]
fn raw_bytes_of_entries_are_the_received_bytes() {
    let cases: [(TextEncoding, &[u8]); 5] = [
        (TextEncoding::Utf8, &[0xC3, 0xA9]),
        (TextEncoding::Latin1, &[0xE9]),
        (TextEncoding::Cp437, &[0x82]),
        (TextEncoding::Utf8, &[0xFF]),
        (TextEncoding::Ascii, &[0xE9]),
    ];
    for (encoding, wire) in cases {
        let lm = received(encoding, wire);
        let mut source = LineModeTextSource {
            history: &lm.local_history,
            display_mode: LineDisplayMode::Pretty,
            raw_view: RawSideView::Hex,
            view_height: 10,
        };
        let mut expected = wire.to_vec();
        expected.push(b'\n');
        assert_eq!(
            source.get_line_raw_bytes(0),
            Some(expected),
            "{encoding:?} {wire:02X?}"
        );
    }
}

/// Yanking a Latin1 line in the Hex raw view gives the bytes received (#20).
/// How: select "xéy" received as 78 E9 79 and yank raw.
#[test]
fn raw_yank_gives_received_bytes() {
    let mut lm = received(TextEncoding::Latin1, b"x\xE9y");
    let mut settings = SettingsConfig::default();
    settings.raw_side.view = RawSideView::Hex;
    lm.copy_state.enter_at(0, 0);
    lm.handle_copy_action(CopyModeAction::Select, &settings);
    lm.handle_copy_action(CopyModeAction::EndOfLine, &settings);
    let yanked = lm.handle_copy_action(CopyModeAction::YankRaw, &settings);
    assert_eq!(yanked.as_deref(), Some("78 E9 79 0A"));
}

/// Raw search looks for the bytes received: a Latin1 "é" is found as E9, not as its
/// decoded C3 A9, and the match's column is the "é" in the text (#20).
/// How: raw searches for E9 and for C3 A9 on "xéy" received as 78 E9 79.
#[test]
fn raw_search_sees_received_bytes() {
    let mut lm = received(TextEncoding::Latin1, b"x\xE9y");
    let mut found = |query: &str| {
        lm.search_state = SearchState {
            active: true,
            query: Input::from(query),
            mode_kind: SearchModeKind::RawBytes,
            ..Default::default()
        };
        lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
        lm.search_state.matches.len()
    };
    assert_eq!(found("E9"), 1);
    assert_eq!(found("C3 A9"), 0);
    found("E9");
    let m = &lm.search_state.matches[0];
    assert_eq!((m.col_idx, m.byte_offset, m.byte_len), (1, 1, 1));
}

/// MAX-72: keys that only move the query cursor don't re-run the search, so the current
/// match and the view stay; a key that changes the query does re-run it.
/// How: two matches, Next, then Left and Right in the query; then a typed space.
#[test]
fn query_cursor_keys_keep_the_current_match() {
    let mut lm = line_mode(10);
    lm.feed_rx(false, TextEncoding::Utf8, b"hit a\nhit b\n");
    lm.search_state.active = true;
    lm.search_state.query = tui_input::Input::new("hit".to_string());
    lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
    lm.search_state.next_match();
    lm.search_state.target_jump = None;

    let press = |lm: &mut LineMode, code| {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        lm.handle_input_search(key, LineDisplayMode::Pretty, RawSideView::Hex);
    };
    press(&mut lm, KeyCode::Left);
    assert_eq!(lm.search_state.current_match, 1);
    assert_eq!(lm.search_state.target_jump, None);

    press(&mut lm, KeyCode::Right);
    press(&mut lm, KeyCode::Char(' '));
    assert_eq!(lm.search_state.current_match, 0);
    assert!(lm.search_state.target_jump.is_some());
}

/// MAX-215: Next moves the copy cursor even out of copy mode; Confirm closes the bar,
/// keeps the matches and enters copy mode on the current match, without a selection;
/// Next and Previous do nothing once the bar is closed.
/// How: matches on lines 1 and 2, Next, Confirm, then Next and Previous.
#[test]
fn search_confirm_enters_copy_mode_on_the_match() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"x\nhit one\nthe hit\n");
    lm.handle_search_action(SearchModeAction::Query("hit".to_string()), &settings);
    assert_eq!(lm.search_state.current_match_position(), Some((1, 0)));

    lm.handle_search_action(SearchModeAction::Next, &settings);
    assert!(!lm.copy_state.active);
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (2, 4));

    lm.handle_search_action(SearchModeAction::Confirm, &settings);
    assert!(!lm.search_state.active);
    assert_eq!(lm.search_state.matches.len(), 2);
    assert!(lm.copy_state.active);
    assert_eq!(lm.copy_state.selection_anchor, None);
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (2, 4));

    lm.handle_search_action(SearchModeAction::Next, &settings);
    lm.handle_search_action(SearchModeAction::Previous, &settings);
    assert_eq!(lm.search_state.current_match, 1);
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (2, 4));
}

/// MAX-215: Confirm without a match enters copy mode on the first line; from copy mode it
/// stays where it was, the selection kept.
/// How: a search for a missing text confirmed, out of copy mode then from copy mode on
/// line 1 with a selection.
#[test]
fn search_confirm_without_match() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"a\nb\n");
    lm.handle_search_action(SearchModeAction::Query("zzz".to_string()), &settings);
    lm.handle_search_action(SearchModeAction::Confirm, &settings);
    assert!(lm.copy_state.active);
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (0, 0));

    lm.copy_state.enter_at(1, 0);
    lm.copy_state.selection_anchor = Some(lm.copy_state.cursor);
    lm.handle_search_action(SearchModeAction::Query("zzz".to_string()), &settings);
    lm.handle_search_action(SearchModeAction::Confirm, &settings);
    assert!(lm.copy_state.active);
    assert!(lm.copy_state.selection_anchor.is_some());
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (1, 0));
}

/// MAX-215: Cancel closes the bar and leaves copy mode as it was: cursor and selection.
/// How: copy mode on line 1 with a selection, a search with a match on line 0, Cancel.
#[test]
fn search_cancel_leaves_copy_mode_as_it_was() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"hit\nb\n");
    lm.copy_state.enter_at(1, 0);
    lm.copy_state.selection_anchor = Some(lm.copy_state.cursor);

    lm.handle_search_action(SearchModeAction::Query("hit".to_string()), &settings);
    lm.handle_search_action(SearchModeAction::Cancel, &settings);
    assert!(!lm.search_state.active);
    assert!(lm.copy_state.active);
    assert!(lm.copy_state.selection_anchor.is_some());
    let cursor = &lm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (1, 0));
}

/// MAX-215: a backward start opens the bar on the newest match; the `search` command's
/// Query searches forward again.
/// How: matches on lines 0 and 2, Start backward with a typed query, then Query.
#[test]
fn search_backward_starts_on_the_newest_match() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"hit\nb\nhit\n");
    lm.handle_search_action(
        SearchModeAction::Start(SearchDirection::Backward),
        &settings,
    );
    assert!(lm.search_state.active);
    paste_into_input(&mut lm.search_state.query, "hit");
    lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(lm.search_state.current_match_position(), Some((2, 0)));

    lm.handle_search_action(SearchModeAction::Query("hit".to_string()), &settings);
    assert_eq!(lm.search_state.direction, SearchDirection::Forward);
    assert_eq!(lm.search_state.current_match_position(), Some((0, 0)));
}

/// MAX-215: a search opened from copy mode moves the cursor to the match as the query
/// changes and keeps the selection; Esc brings the cursor back, Enter leaves it on the match.
/// How: copy mode on line 1 with a selection, `/` with "hit", Esc; again, Enter.
#[test]
fn search_from_copy_mode_moves_the_cursor() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"hit\nb\nc hit\n");
    lm.copy_state.enter_at(1, 0);
    lm.copy_state.selection_anchor = Some(lm.copy_state.cursor);
    let cursor = |lm: &LineMode| (lm.copy_state.cursor.line_idx, lm.copy_state.cursor.char_idx);
    let search = |lm: &mut LineMode| {
        lm.handle_search_action(SearchModeAction::Start(SearchDirection::Forward), &settings);
        lm.handle_search_action(SearchModeAction::Paste("hit".to_string()), &settings);
    };

    search(&mut lm);
    assert_eq!(cursor(&lm), (2, 2));
    assert!(lm.copy_state.selection_anchor.is_some());
    lm.handle_search_action(SearchModeAction::Cancel, &settings);
    assert_eq!(cursor(&lm), (1, 0));
    assert!(lm.copy_state.active);

    lm.search_state.query = Input::default();
    search(&mut lm);
    lm.handle_search_action(SearchModeAction::Confirm, &settings);
    assert!(!lm.search_state.active);
    assert_eq!(cursor(&lm), (2, 2));
    assert!(lm.copy_state.selection_anchor.is_some());
    assert_eq!(lm.search_state.origin, None);
}

/// MAX-215: after Enter the matches stay shown in copy mode and `n` / `N` move the cursor
/// between them, the selection kept; leaving copy mode stops showing them, and `n` shows
/// them again. Esc in the bar doesn't keep them.
/// How: "hit" on lines 0, 2, 3, a forward search confirmed, n, N, a wrap, copy mode left.
#[test]
fn n_and_n_reverse_in_copy_mode() {
    let mut lm = line_mode(10);
    let settings = SettingsConfig::default();
    lm.feed_rx(false, TextEncoding::Utf8, b"hit\nb\nhit\nhit\n");
    let cursor = |lm: &LineMode| (lm.copy_state.cursor.line_idx, lm.copy_state.cursor.char_idx);
    lm.handle_search_action(SearchModeAction::Query("hit".to_string()), &settings);
    lm.handle_search_action(SearchModeAction::Confirm, &settings);
    assert!(lm.search_state.shows_matches());
    assert_eq!(cursor(&lm), (0, 0));
    lm.copy_state.selection_anchor = Some(lm.copy_state.cursor);

    assert_eq!(lm.repeat_search(false), Some(false));
    assert_eq!(cursor(&lm), (2, 0));
    assert_eq!(lm.repeat_search(true), Some(false));
    assert_eq!(cursor(&lm), (0, 0));
    assert_eq!(lm.repeat_search(true), Some(true));
    assert_eq!(cursor(&lm), (3, 0));
    assert!(lm.copy_state.selection_anchor.is_some());

    lm.handle_copy_action(CopyModeAction::Exit, &settings);
    assert!(!lm.search_state.shows_matches());
    lm.copy_state.enter_at(1, 0);
    assert_eq!(lm.repeat_search(false), Some(false));
    assert_eq!(cursor(&lm), (2, 0));
    assert!(lm.search_state.shows_matches());

    lm.handle_search_action(SearchModeAction::Start(SearchDirection::Forward), &settings);
    lm.handle_search_action(SearchModeAction::Cancel, &settings);
    assert!(!lm.search_state.shows_matches());
}
