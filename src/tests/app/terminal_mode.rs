//! Bytes sent in terminal mode: typed keys (#31) and bracketed paste (#28).

use super::*;
use crate::inputs::SearchModeAction;

/// Bytes restored without their times
fn untimed(bytes: &[u8]) -> [TimedChunk<'_>; 1] {
    [TimedChunk { bytes, time: None }]
}
use crossterm::event::{KeyCode, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sent(code: KeyCode, modifiers: KeyModifiers, application_cursor: bool) -> Vec<u8> {
    encode_terminal_key(
        KeyEvent::new(code, modifiers),
        application_cursor,
        BackspaceByte::Del,
    )
    .unwrap()
}

/// Normal cursor mode sends the same bytes as before #31 (captured from the old
/// `handle_terminal_mode` in inputs.rs).
/// How: arrows, Home, End, a modified arrow, a char, Enter, F1 and PageUp.
#[test]
fn keys_in_normal_cursor_mode() {
    let none = KeyModifiers::NONE;
    assert_eq!(sent(KeyCode::Up, none, false), b"\x1b[A");
    assert_eq!(sent(KeyCode::Down, none, false), b"\x1b[B");
    assert_eq!(sent(KeyCode::Right, none, false), b"\x1b[C");
    assert_eq!(sent(KeyCode::Left, none, false), b"\x1b[D");
    assert_eq!(sent(KeyCode::Home, none, false), b"\x1b[H");
    assert_eq!(sent(KeyCode::End, none, false), b"\x1b[F");
    assert_eq!(
        sent(KeyCode::Up, KeyModifiers::CONTROL, false),
        b"\x1b[1;5A"
    );
    assert_eq!(sent(KeyCode::Char('a'), none, false), b"a");
    assert_eq!(sent(KeyCode::Enter, none, false), b"\r");
    assert_eq!(sent(KeyCode::F(1), none, false), b"\x1bOP");
    assert_eq!(sent(KeyCode::PageUp, none, false), b"\x1b[5~");
}

/// Application cursor mode only changes unmodified arrows, Home and End to `ESC O x`.
/// How: the same keys with the mode on.
#[test]
fn keys_in_application_cursor_mode() {
    let none = KeyModifiers::NONE;
    assert_eq!(sent(KeyCode::Up, none, true), b"\x1bOA");
    assert_eq!(sent(KeyCode::Down, none, true), b"\x1bOB");
    assert_eq!(sent(KeyCode::Right, none, true), b"\x1bOC");
    assert_eq!(sent(KeyCode::Left, none, true), b"\x1bOD");
    assert_eq!(sent(KeyCode::Home, none, true), b"\x1bOH");
    assert_eq!(sent(KeyCode::End, none, true), b"\x1bOF");
    assert_eq!(sent(KeyCode::Up, KeyModifiers::CONTROL, true), b"\x1b[1;5A");
    assert_eq!(sent(KeyCode::Char('a'), none, true), b"a");
    assert_eq!(sent(KeyCode::PageUp, none, true), b"\x1b[5~");
}

fn terminal(rows: u16) -> TerminalMode {
    TerminalMode::new(100, rows, 80)
}

/// Feeds one line per chunk, 1 ms apart so every row gets a distinct timestamp.
fn feed_lines(tm: &mut TerminalMode, lines: &[&str]) {
    for (i, line) in lines.iter().enumerate() {
        let sep = if i + 1 < lines.len() { "\r\n" } else { "" };
        tm.process_rx(TextEncoding::Utf8, format!("{line}{sep}").as_bytes());
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Timestamps of every world row, history first (top to bottom).
fn timeline(tm: &TerminalMode) -> Vec<Option<Timestamp>> {
    (0..tm.total_timeline_len())
        .map(|i| tm.get_timeline_timestamp(i))
        .collect()
}

fn terminal_with(history: usize, rows: u16) -> TerminalMode {
    TerminalMode::new(history, rows, 80)
}

/// Feeds each line as its own chunk, 1 ms apart, and returns each line's text with the
/// timestamp of the row the cursor is on right after it arrived.
fn feed_and_stamp(tm: &mut TerminalMode, lines: &[&str]) -> Vec<(String, Option<Timestamp>)> {
    let mut stamps = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let sep = if i == 0 { "" } else { "\r\n" };
        tm.process_rx(TextEncoding::Utf8, format!("{sep}{line}").as_bytes());
        let cursor_world = tm.get_max_scrollback() + tm.vt100_screen().cursor_position().0 as usize;
        stamps.push((line.to_string(), tm.get_timeline_timestamp(cursor_world)));
        std::thread::sleep(Duration::from_millis(1));
    }
    stamps
}

/// Checks every world row with text has the timestamp its line got when it arrived.
fn assert_rows_keep_their_stamps(tm: &mut TerminalMode, stamps: &[(String, Option<Timestamp>)]) {
    let world = tm.get_max_scrollback() + tm.get_vt100_size().0 as usize;
    assert_eq!(
        tm.total_timeline_len(),
        world,
        "one timestamp slot per world row"
    );
    for row in 0..world {
        let text = read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
            .unwrap();
        if let Some((_, ts)) = stamps.iter().find(|(line, _)| *line == text.trim_end()) {
            assert!(ts.is_some(), "{text:?} was never stamped");
            assert_eq!(tm.get_timeline_timestamp(row), *ts, "row {row} {text:?}");
        }
    }
}

/// Text of a world row, without the trailing spaces.
fn row_text(tm: &mut TerminalMode, row: usize) -> String {
    read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

/// Search text is built once and reused while typing, and rebuilt after anything that
/// changes the terminal's text: RX, resize, clear (#12).
/// How: count the matches of "hit" before and after each of those.
#[test]
fn search_text_follows_terminal_changes() {
    let mut tm = terminal(3);
    feed_lines(&mut tm, &["hit", "miss"]);
    tm.search_state.query = tui_input::Input::new("hit".to_string());
    let matches = |tm: &mut TerminalMode| {
        tm.update_search_matches(RawSideView::default());
        tm.search_state.matches.len()
    };
    assert_eq!(matches(&mut tm), 1);
    assert_eq!(matches(&mut tm), 1);

    tm.process_rx(TextEncoding::Utf8, b"\r\nhit again");
    assert_eq!(matches(&mut tm), 2);

    // a shrink of 3 cols cuts "hit again" to "hit": still 2 rows, and nothing stale
    tm.set_vt100_size(3, 3);
    assert_eq!(matches(&mut tm), 2);

    tm.clear_history();
    assert_eq!(matches(&mut tm), 0);
}

/// Once scrollback is full each new row evicts the oldest: the copy cursor, the selection
/// anchor and the search matches stay on their rows instead of sliding (#34).
/// How: 3 rows of history, 2 on screen, cursor and anchor on "l4", a match on "l5", then
/// 2 more lines; then enough lines to evict the match's row too.
#[test]
fn line_refs_follow_rows_when_scrollback_is_full() {
    let mut tm = terminal_with(3, 2);
    feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5", "l6"]);
    let l4 = (0..5).find(|&r| row_text(&mut tm, r) == "l4").unwrap();
    tm.copy_state.enter_at(l4, 1);
    tm.copy_state.selection_anchor = Some(tm.copy_state.cursor);
    tm.search_state.query = tui_input::Input::new("l5".to_string());
    tm.update_search_matches(RawSideView::default());
    assert_eq!(tm.search_state.matches.len(), 1);

    tm.process_rx(TextEncoding::Utf8, b"\r\nl7\r\nl8");
    let cursor_row = tm.copy_state.cursor.line_idx;
    assert_eq!(row_text(&mut tm, cursor_row), "l4");
    assert_eq!(tm.copy_state.cursor.char_idx, 1);
    let anchor_row = tm.copy_state.selection_anchor.unwrap().line_idx;
    assert_eq!(row_text(&mut tm, anchor_row), "l4");
    let match_row = tm.search_state.matches[0].line_idx;
    assert_eq!(row_text(&mut tm, match_row), "l5");

    // "l5" and "l4" scroll out of the history: the match goes, the cursor stays on row 0
    tm.process_rx(TextEncoding::Utf8, b"\r\nl9\r\nl10\r\nl11");
    assert!(tm.search_state.matches.is_empty());
    assert_eq!(tm.copy_state.cursor.line_idx, 0);
}

/// MAX-164: a mark goes on the terminal cursor's row, or on the copy cursor's in copy mode; it
/// follows its row when the scrollback is full, goes with it, and clearing the history takes
/// them all.
/// How: 3 rows of history, 2 on screen: a on "l4" (cursor), b on "l2" (copy cursor), then
/// lines until "l2" is evicted (row 0 after "l6", gone after "l7"), then cleared.
#[test]
fn marks_follow_their_rows_and_go_with_them() {
    let mut tm = terminal_with(3, 2);
    feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4"]);
    let a = tm.add_mark('a').unwrap();
    assert_eq!(row_text(&mut tm, a), "l4");
    assert_eq!(tm.add_mark('1'), None);
    let l2 = (0..5).find(|&r| row_text(&mut tm, r) == "l2").unwrap();
    tm.copy_state.enter_at(l2, 1);
    assert_eq!(tm.add_mark('b'), Some(l2));
    tm.copy_state.active = false;

    tm.process_rx(TextEncoding::Utf8, b"\r\nl5");
    let a = tm.marks().get('a').unwrap();
    assert_eq!(row_text(&mut tm, a), "l4");
    let b = tm.marks().get('b').unwrap();
    assert_eq!(row_text(&mut tm, b), "l2");
    tm.process_rx(TextEncoding::Utf8, b"\r\nl6");
    assert_eq!(tm.marks().get('b'), Some(0));
    tm.process_rx(TextEncoding::Utf8, b"\r\nl7");
    assert_eq!(tm.marks().get('b'), None);
    assert!(tm.marks().get('a').is_some());

    assert!(tm.remove_mark('a').is_some());
    tm.add_mark('c');
    tm.clear_history();
    assert!(tm.marks().sorted().is_empty());
}

/// One chunk with several lines stamps every row it writes, the first one included (#13).
/// How: "a", "b", "c" in a single chunk on a 5 row screen; the 2 rows below stay unstamped.
#[test]
fn one_chunk_stamps_every_row_it_writes() {
    let mut tm = terminal(5);
    tm.process_rx(TextEncoding::Utf8, b"a\r\nb\r\nc");
    let timeline = timeline(&tm);
    assert!(timeline[..3].iter().all(Option::is_some), "{timeline:?}");
    assert!(timeline[3..].iter().all(Option::is_none), "{timeline:?}");
}

/// Once scrollback is full, every new row drops the oldest one: timestamps stay on their
/// rows instead of drifting (#13).
/// How: 3 rows of history, 2 on screen, 8 lines one chunk each.
#[test]
fn timestamps_follow_rows_with_full_scrollback() {
    let mut tm = terminal_with(3, 2);
    let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"]);
    assert_eq!(tm.get_max_scrollback(), 3);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// Without scrollback, rows scrolled off the top take their timestamps with them (#13).
/// How: `history_size = 0`, 2 rows, 5 lines.
#[test]
fn timestamps_follow_rows_without_scrollback() {
    let mut tm = terminal_with(0, 2);
    let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4"]);
    assert_eq!(tm.get_max_scrollback(), 0);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// `CSI 3 J` clears scrollback: its timestamps go too, the screen keeps its own (#13).
/// How: 6 lines on 3 rows, then `ESC[3J`.
#[test]
fn clear_scrollback_drops_its_timestamps() {
    let mut tm = terminal(3);
    let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5"]);
    tm.process_rx(TextEncoding::Utf8, b"\x1b[3J");
    assert_eq!(tm.get_max_scrollback(), 0);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// The alternate screen (vim, less) never touches the main screen's timestamps (#13).
/// How: 4 lines, then 10 lines in the alternate screen, then back.
#[test]
fn alternate_screen_keeps_main_timestamps() {
    let mut tm = terminal(3);
    let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3"]);
    let before = timeline(&tm);
    tm.process_rx(TextEncoding::Utf8, b"\x1b[?1049h");
    for i in 0..10 {
        tm.process_rx(TextEncoding::Utf8, format!("alt{i}\r\n").as_bytes());
    }
    tm.process_rx(TextEncoding::Utf8, b"\x1b[?1049l");
    assert_eq!(timeline(&tm), before);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// The max scrollback is the number of rows scrolled off, read with `scrollback_len` instead
/// of the old set/read/restore offset trick, and the view offset is left alone (#29).
/// How: 6 lines on a 3 row screen, scrolled back by 1.
#[test]
fn max_scrollback_counts_scrolled_rows() {
    let mut tm = terminal(3);
    feed_lines(&mut tm, &["1", "2", "3", "4", "5", "6"]);
    assert_eq!(tm.get_max_scrollback(), 3);
    tm.vt100_parser.screen_mut().set_scrollback(1);
    tm.refresh_max_scrollback();
    assert_eq!(tm.get_max_scrollback(), 3);
    assert_eq!(tm.vt100_screen().scrollback(), 1);
}

/// vt100 0.18 pushes the top rows into scrollback when the height shrinks (cursor at the
/// bottom) and pulls them back when it grows: the timestamps follow their rows (#29).
/// How: 4 lines on 4 rows, shrink to 2, then grow back to 4, comparing the timestamps of
/// the whole world (history + screen) with the ones before.
#[test]
fn resize_moves_timestamps_with_rows() {
    let mut tm = terminal(4);
    feed_lines(&mut tm, &["a", "b", "c", "d"]);
    let before = timeline(&tm);
    assert_eq!(before.len(), 4);
    assert!(before.iter().all(Option::is_some));

    tm.set_vt100_size(2, 80);
    assert_eq!(tm.get_max_scrollback(), 2);
    assert_eq!(timeline(&tm), before);

    tm.set_vt100_size(4, 80);
    assert_eq!(tm.get_max_scrollback(), 0);
    assert_eq!(timeline(&tm), before);
}

/// Shrinking with a full scrollback pushes the top rows in and evicts as many of the oldest:
/// timestamps leave from the top, not the bottom.
/// How: 3 rows of history, 4 on screen, 8 lines (scrollback full), shrink to 2.
#[test]
fn shrink_with_full_scrollback_evicts_oldest_timestamps() {
    let mut tm = terminal_with(3, 4);
    let lines = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"];
    let stamps = feed_and_stamp(&mut tm, &lines);
    assert_eq!(tm.get_max_scrollback(), 3);
    tm.set_vt100_size(2, 80);
    assert_eq!(tm.get_max_scrollback(), 3);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// Growing past what scrollback still holds makes vt100 insert blank rows at the top (for the
/// rows it evicted): they get empty slots there, the other rows keep theirs (#13b).
/// How: 3 rows of history, 2 on screen, 8 lines, grow to 7 (3 rows pulled back, 2 blank).
#[test]
fn grow_past_scrollback_inserts_blank_rows_at_top() {
    let mut tm = terminal_with(3, 2);
    let lines = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"];
    let stamps = feed_and_stamp(&mut tm, &lines);
    tm.set_vt100_size(7, 80);
    assert_eq!(tm.get_max_scrollback(), 0);
    assert_eq!(timeline(&tm)[..2], [None, None]);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// Without scrollback every row inserted at the top on grow is blank (#13b).
/// How: `history_size = 0`, 2 rows, 5 lines, grow to 4.
#[test]
fn grow_without_scrollback_inserts_blank_rows_at_top() {
    let mut tm = terminal_with(0, 2);
    let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4"]);
    tm.set_vt100_size(4, 80);
    assert_eq!(timeline(&tm)[..2], [None, None]);
    assert_rows_keep_their_stamps(&mut tm, &stamps);
}

/// With the cursor at the top, shrinking drops rows from the bottom (nothing goes to
/// scrollback), like before 0.18: the bottom timestamps are dropped.
/// How: 1 line on 4 rows, shrink to 2.
#[test]
fn resize_with_cursor_at_top_drops_bottom_rows() {
    let mut tm = terminal(4);
    feed_lines(&mut tm, &["a"]);
    let before = timeline(&tm);
    tm.set_vt100_size(2, 80);
    assert_eq!(tm.get_max_scrollback(), 0);
    assert_eq!(timeline(&tm), before[..2]);
}

/// Copy mode reads a row like vt100 shows it: the gaps a tab or a cursor move leave are
/// spaces, so the copy cursor's char index is the screen column search and the selection use.
/// How: "a\\tb" and "ESC[5C" then "c", read from the screen and once scrolled into history.
#[test]
fn copy_mode_row_text_keeps_gaps() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\tb\r\n\x1b[5Cc");
    let read = |tm: &mut TerminalMode, row| {
        read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
    };
    assert_eq!(read(&mut tm, 0).as_deref(), Some("a       b"));
    assert_eq!(read(&mut tm, 1).as_deref(), Some("     c"));

    tm.process_rx(TextEncoding::Utf8, b"\r\n\r\n\r\n");
    assert_eq!(tm.get_max_scrollback(), 2);
    assert_eq!(read(&mut tm, 0).as_deref(), Some("a       b"));
    assert_eq!(read(&mut tm, 1).as_deref(), Some("     c"));
}

/// The mode follows what the remote sends: `ESC[?1h` turns it on, `ESC[?1l` off.
/// How: an Up key through `update_action_terminal` before, during and after the mode.
#[test]
fn cursor_mode_follows_remote() {
    let mut tm = TerminalMode::new(100, 24, 80);
    let up = |tm: &mut TerminalMode| match tm
        .update_action_terminal(TerminalAction::Key(key(KeyCode::Up)), BackspaceByte::Del)
    {
        UpdateEffect::Transmit(bytes) => bytes,
        _ => panic!("expected a transmit"),
    };
    assert_eq!(up(&mut tm), b"\x1b[A");
    tm.process_rx(TextEncoding::Utf8, b"\x1b[?1h");
    assert_eq!(up(&mut tm), b"\x1bOA");
    tm.process_rx(TextEncoding::Utf8, b"\x1b[?1l");
    assert_eq!(up(&mut tm), b"\x1b[A");
}

/// LF, CRLF and CR all become one `\r`, like pressing Enter.
/// How: one paste with each kind of line break.
#[test]
fn terminal_paste_line_breaks_become_cr() {
    assert_eq!(terminal_paste_bytes("a\nb\r\nc\rd", false), b"a\rb\rc\rd");
}

/// The paste markers are only added when the remote enabled bracketed paste.
/// How: same text with the remote flag off then on.
#[test]
fn terminal_paste_markers_follow_remote_mode() {
    assert_eq!(terminal_paste_bytes("ls", false), b"ls");
    assert_eq!(terminal_paste_bytes("ls", true), b"\x1b[200~ls\x1b[201~");
}

/// Non-ASCII text is sent as its UTF-8 bytes, like typed chars.
/// How: a paste with an accented char.
#[test]
fn terminal_paste_keeps_utf8() {
    assert_eq!(terminal_paste_bytes("é", false), "é".as_bytes());
}

/// MAX-72: keys that only move the query cursor don't re-run the search (same as line
/// mode); a key that changes the query does.
#[test]
fn query_cursor_keys_keep_the_current_match() {
    let mut tm = terminal(3);
    feed_lines(&mut tm, &["hit a", "hit b"]);
    tm.search_state.active = true;
    tm.search_state.query = tui_input::Input::new("hit".to_string());
    tm.update_search_matches(RawSideView::default());
    tm.search_state.next_match();
    tm.search_state.target_jump = None;

    tm.handle_input_search(key(KeyCode::Left), RawSideView::default());
    assert_eq!(tm.search_state.current_match, 1);
    assert_eq!(tm.search_state.target_jump, None);

    tm.handle_input_search(key(KeyCode::Right), RawSideView::default());
    tm.handle_input_search(key(KeyCode::Char(' ')), RawSideView::default());
    assert_eq!(tm.search_state.current_match, 0);
    assert!(tm.search_state.target_jump.is_some());
}

/// A view of `cols`x`rows` in `mode`, the configured size 24x80
fn view(tm: &mut TerminalMode, mode: TerminalDimensionMode, cols: u16, rows: u16) {
    tm.update_viewport(ViewportSize::new(cols, rows), 0, mode, 24, 80);
}

/// MAX-140: `match_view` and `fixed_cols` don't shrink the grid past the content below
/// the cursor (vt100 would drop those rows), and finish shrinking once it is gone.
/// How: a 30-row grid with "menu" on row 20 and the prompt on row 1, a 10-row view in each
/// mode, then the screen cleared.
#[test]
fn shrinking_keeps_the_rows_below_the_cursor() {
    use TerminalDimensionMode::{FixedCols, MatchView};
    for mode in [MatchView, FixedCols] {
        let mut tm = terminal(30);
        tm.process_rx(TextEncoding::Utf8, b"\x1b[20;1Hmenu\x1b[1;1H$ ");
        view(&mut tm, mode, 80, 10);
        assert_eq!(tm.get_vt100_size(), (20, 80), "{mode:?}");
        let menu = tm.get_max_scrollback() + 19;
        assert_eq!(row_text(&mut tm, menu), "menu", "{mode:?}");

        tm.process_rx(TextEncoding::Utf8, b"\x1b[2J");
        view(&mut tm, mode, 80, 10);
        assert_eq!(tm.get_vt100_size(), (10, 80), "{mode:?}");
    }
}

/// MAX-140: `match_view` doesn't make the grid narrower than its widest row (vt100 would cut
/// the rows), and finishes once that row is gone; with blank rows and columns only it
/// matches the view.
/// How: a 70-column row on an 80-column grid and a 40x10 view, then the screen cleared; a
/// fresh grid with a prompt only.
#[test]
fn shrinking_keeps_the_widest_row() {
    let mut tm = terminal(10);
    let long = "x".repeat(70);
    tm.process_rx(TextEncoding::Utf8, long.as_bytes());
    view(&mut tm, TerminalDimensionMode::MatchView, 40, 10);
    assert_eq!(tm.get_vt100_size(), (10, 70));
    let row = tm.get_max_scrollback();
    assert_eq!(row_text(&mut tm, row), long);

    tm.process_rx(TextEncoding::Utf8, b"\x1b[2J");
    view(&mut tm, TerminalDimensionMode::MatchView, 40, 10);
    assert_eq!(tm.get_vt100_size(), (10, 40));

    let mut tm = terminal(30);
    tm.process_rx(TextEncoding::Utf8, b"$ ");
    view(&mut tm, TerminalDimensionMode::MatchView, 40, 10);
    assert_eq!(tm.get_vt100_size(), (10, 40));
}

/// MAX-140: a size the user asks for (the resize popup, fit to the view) applies exactly,
/// content or not; only the automatic resizes keep the content.
/// How: a 70-column row and "menu" below the cursor, then a 5x40 size.
#[test]
fn an_explicit_size_applies_as_asked() {
    let mut tm = terminal(30);
    let long = "x".repeat(70);
    tm.process_rx(
        TextEncoding::Utf8,
        format!("{long}\x1b[20;1Hmenu\x1b[1;1H").as_bytes(),
    );
    tm.set_vt100_size(5, 40);
    assert_eq!(tm.get_vt100_size(), (5, 40));
}

/// MAX-148: a restore replays the raw bytes, then leaves a session that ended inside a
/// full-screen program usable: main screen, normal cursor keys, no mouse reports, no
/// bracketed paste, cursor shown. The separator follows on its own line, the restored rows
/// have no time, and a live line after them gets one.
#[test]
fn restore_resets_the_modes_left_set() {
    let mut tm = TerminalMode::new(100, 24, 80);
    let raw = b"before\r\nkept\r\n\x1b[?1049h\x1b[?1h\x1b[?1000h\x1b[?2004h\x1b[?25l\x1b[5;10rvim";
    tm.restore(TextEncoding::Utf8, &untimed(raw), "-- restored --", true);
    let screen = tm.vt100_screen();
    assert!(!screen.alternate_screen());
    assert!(!screen.application_cursor());
    assert_eq!(screen.mouse_protocol_mode(), vt100::MouseProtocolMode::None);
    assert!(!screen.bracketed_paste());
    assert!(!screen.hide_cursor());
    // like a `clear`: the separator on the top row, the restored rows in the scrollback
    let contents = screen.contents();
    assert!(contents.starts_with("-- restored --"), "{contents:?}");
    let lines = tm.all_text_lines();
    let separator = lines.iter().position(|l| l == "-- restored --").unwrap();
    assert_eq!(
        lines[separator - 2..separator],
        ["before", "kept"],
        "{lines:?}"
    );
    assert!(tm.row_timestamps.iter().all(Option::is_none));

    tm.process_rx(TextEncoding::Utf8, b"live\r\n");
    assert!(
        tm.vt100_screen()
            .contents()
            .contains("-- restored --\nlive")
    );
    // the live row and the one the cursor went on to, the restored ones stay without
    let stamped = tm.row_timestamps.iter().filter(|t| t.is_some()).count();
    assert_eq!(stamped, 2);

    // a session that ended mid-line: the separator goes on the next one
    let mut tm = TerminalMode::new(100, 24, 80);
    tm.restore(TextEncoding::Utf8, &untimed(b"abc"), "-- restored --", true);
    let lines = tm.all_text_lines();
    let separator = lines.iter().position(|l| l == "-- restored --").unwrap();
    assert_eq!(lines[separator - 1], "abc", "{lines:?}");

    // text left under the cursor (a full-screen program's) goes up too: none stays under
    // the separator
    let mut tm = TerminalMode::new(100, 24, 80);
    tm.restore(
        TextEncoding::Utf8,
        &untimed(b"top\x1b[10;1Hstatus\x1b[2;1H"),
        "-- restored --",
        true,
    );
    let screen = tm.vt100_screen();
    assert_eq!(screen.contents().trim_end(), "-- restored --");
    assert_eq!(screen.cursor_position(), (1, 0));
    let lines = tm.all_text_lines();
    assert!(lines.iter().any(|l| l == "status"), "{lines:?}");
}

/// MAX-176: `clear` puts the cursor's row (a prompt) on the top row, the rows above it in
/// the scrollback, the cursor's column kept; on the alternate screen it does nothing.
/// How: 3 lines and a `$ ` prompt in a 24-row grid, cleared; then a full-screen program's.
#[test]
fn clear_puts_the_cursor_row_on_top() {
    let mut tm = TerminalMode::new(100, 24, 80);
    tm.process_rx(TextEncoding::Utf8, b"one\r\ntwo\r\nthree\r\n$ ");
    tm.clear_view();
    let screen = tm.vt100_screen();
    assert_eq!(screen.contents().trim_end(), "$");
    assert_eq!(screen.cursor_position(), (0, 2));
    let lines = tm.all_text_lines();
    let prompt = lines.iter().position(|l| l.trim_end() == "$").unwrap();
    assert_eq!(lines[..prompt], ["one", "two", "three"], "{lines:?}");

    let mut tm = TerminalMode::new(100, 24, 80);
    tm.process_rx(TextEncoding::Utf8, b"\x1b[?1049h\x1b[5;1Hmenu");
    tm.clear_view();
    let screen = tm.vt100_screen();
    assert_eq!(screen.cursor_position(), (4, 4));
    assert!(screen.contents().starts_with("\n\n\n\nmenu"));
}

/// MAX-176: the restore separator stays on the top row when the first frame grows the grid
/// (vt100 pulls rows back from the scrollback then), also with lines received before the
/// grid got its size (an app started in line mode); the restored lines stay a scroll up.
/// How: 30 lines restored in a 24-row grid, grown to 50 rows by `grow_only`; then the same
/// with a live line received before.
#[test]
fn restore_separator_stays_on_top_when_the_grid_grows() {
    let raw: String = (0..30).map(|i| format!("line {i:02}\r\n")).collect();
    for live in [None, Some("live")] {
        let mut tm = TerminalMode::new(100, 24, 80);
        tm.restore(
            TextEncoding::Utf8,
            &untimed(raw.as_bytes()),
            "-- restored --",
            true,
        );
        if let Some(live) = live {
            tm.process_rx(TextEncoding::Utf8, format!("{live}\r\n").as_bytes());
        }
        let viewport = ViewportSize::new(80, 50);
        tm.update_viewport(viewport, 0, TerminalDimensionMode::GrowOnly, 24, 80);
        assert_eq!(tm.get_vt100_size().0, 50);
        let screen = tm.vt100_screen();
        let rows: Vec<String> = screen.rows(0, 80).take(3).collect();
        let expected = ["-- restored --", live.unwrap_or(""), ""];
        assert_eq!(rows, expected, "{live:?}");
        let cursor_row = if live.is_some() { 2 } else { 1 };
        assert_eq!(screen.cursor_position(), (cursor_row, 0), "{live:?}");
        let lines = tm.all_text_lines();
        let separator = lines.iter().position(|l| l == "-- restored --").unwrap();
        assert_eq!(lines[separator - 1], "line 29", "{lines:?}");
        // the line feeds give the cursor's row no time (the live line's gave it one)
        assert_eq!(tm.cursor_row_time().is_some(), live.is_some());
    }
}

/// MAX-151: Alt-R tells the remote the emulator's size, where its lines wrap, the size the
/// query answers give too, not the window's.
/// How: `fixed` 24x80 in a 50x200 window, then `grow_only` after the window shrank.
#[test]
fn auto_resize_remote_sends_the_emulator_size() {
    let stty = |tm: &mut TerminalMode| match tm.update_action_terminal(
        TerminalAction::AutoResizeRemoteToEmulator,
        BackspaceByte::Del,
    ) {
        UpdateEffect::Transmit(bytes) => String::from_utf8(bytes).unwrap(),
        _ => panic!("nothing sent"),
    };
    let mut tm = terminal(24);
    view(&mut tm, TerminalDimensionMode::Fixed, 200, 50);
    assert_eq!(
        stty(&mut tm),
        "stty rows 24 cols 80; export LINES=24 COLUMNS=80\n"
    );

    view(&mut tm, TerminalDimensionMode::GrowOnly, 120, 40);
    view(&mut tm, TerminalDimensionMode::GrowOnly, 60, 20);
    assert_eq!(tm.get_vt100_size(), (40, 120));
    assert_eq!(
        stty(&mut tm),
        "stty rows 40 cols 120; export LINES=40 COLUMNS=120\n"
    );
}

/// MAX-181: add CR makes a lone LF CR LF, add LF a CR CR LF; the device's own CR LF stays
/// one line break with either, also split across two chunks; both off leaves the bytes.
/// How: each input with each setting, then CR LF split between two calls.
#[test]
fn add_newlines_cases() {
    let run = |bytes: &[u8], add_cr, add_lf| {
        let mut after_cr = false;
        add_newlines(bytes, add_cr, add_lf, &mut after_cr).into_owned()
    };
    let input = b"a\nb\rc\r\nd";
    assert_eq!(run(input, false, false), input);
    assert_eq!(run(input, true, false), b"a\r\nb\rc\r\nd");
    assert_eq!(run(input, false, true), b"a\nb\r\nc\r\nd");
    assert_eq!(run(input, true, true), b"a\r\nb\r\nc\r\nd");

    for (add_cr, add_lf) in [(true, false), (false, true), (true, true)] {
        let mut after_cr = false;
        let first = add_newlines(b"x\r", add_cr, add_lf, &mut after_cr).into_owned();
        let second = add_newlines(b"\ny", add_cr, add_lf, &mut after_cr).into_owned();
        assert_eq!([first, second].concat(), b"x\r\ny", "{add_cr} {add_lf}");
    }
    // off, the last CR is still known when it's turned on before the LF comes
    let mut after_cr = false;
    let _ = add_newlines(b"x\r", false, false, &mut after_cr);
    let lf = add_newlines(b"\n", true, false, &mut after_cr);
    assert_eq!(lf.as_ref(), b"\n");
}

/// MAX-215: a new search starts on the oldest match; Confirm closes the bar, keeps the
/// matches and enters copy mode on the current match, without a selection; Next and
/// Previous do nothing once the bar is closed.
/// How: "hit" on the first and last of 5 rows, Next, Confirm, then Next and Previous.
#[test]
fn search_confirm_enters_copy_mode_on_the_match() {
    let raw_view = RawSideView::default();
    let mut tm = terminal(3);
    feed_lines(&mut tm, &["hit one", "b", "c", "d", "the hit"]);
    tm.handle_search_action(SearchModeAction::Query("hit".to_string()), raw_view);
    let first = tm.search_state.matches[0];
    let last = tm.search_state.matches[1];
    assert!(first.line_idx < last.line_idx);
    assert_eq!(
        tm.search_state.current_match_position(),
        Some((first.line_idx, 0))
    );

    tm.handle_search_action(SearchModeAction::Next, raw_view);
    tm.handle_search_action(SearchModeAction::Confirm, raw_view);
    assert!(!tm.search_state.active);
    assert_eq!(tm.search_state.matches.len(), 2);
    assert!(tm.copy_state.active);
    assert_eq!(tm.copy_state.selection_anchor, None);
    let cursor = &tm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (last.line_idx, 4));

    tm.handle_search_action(SearchModeAction::Next, raw_view);
    tm.handle_search_action(SearchModeAction::Previous, raw_view);
    assert_eq!(tm.search_state.current_match, 1);
    let cursor = &tm.copy_state.cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (last.line_idx, 4));
}

/// The contents of the first `n` cells of screen row 0 (an empty string for an empty cell)
fn first_cells(tm: &TerminalMode, n: u16) -> Vec<String> {
    let screen = tm.vt100_screen();
    (0..n)
        .map(|col| {
            screen
                .cell(0, col)
                .map(|c| c.contents().to_string())
                .unwrap_or_default()
        })
        .collect()
}

/// MAX-249 step 0: valid UTF-8 cut between two reads is one char, a wide char takes two
/// cells; the invalid bytes fix must not change it.
/// How: "a" C3 | A9 "b", then a rocket fed byte by byte and "b".
#[test]
fn valid_utf8_split_across_reads() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\xC3");
    tm.process_rx(TextEncoding::Utf8, b"\xA9b");
    assert_eq!(first_cells(&tm, 3), ["a", "é", "b"]);

    let mut tm = terminal(3);
    for byte in "🚀b".as_bytes() {
        tm.process_rx(TextEncoding::Utf8, &[*byte]);
    }
    assert_eq!(first_cells(&tm, 3), ["🚀", "", "b"]);
    assert!(tm.vt100_screen().cell(0, 0).unwrap().is_wide());
}

/// MAX-249 step 0: escape sequences right after text, and cut between two reads, still
/// apply.
/// How: "a" ESC[31m "b" ESC[0m "c", then ESC[3 | 1m "x": b and x red, a and c not.
#[test]
fn escape_sequences_next_to_text_and_split() {
    let red = vt100::Color::Idx(1);
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\x1b[31mb\x1b[0mc\x1b[3");
    tm.process_rx(TextEncoding::Utf8, b"1mx");
    assert_eq!(first_cells(&tm, 4), ["a", "b", "c", "x"]);
    let fg = |col| tm.vt100_screen().cell(0, col).unwrap().fgcolor();
    assert_eq!(fg(0), vt100::Color::Default);
    assert_eq!(fg(1), red);
    assert_eq!(fg(2), vt100::Color::Default);
    assert_eq!(fg(3), red);
}

/// MAX-249 step 0: the single byte encodings decode every byte, none is invalid.
/// How: E9 FF in Latin1, 82 in CP437.
#[test]
fn single_byte_encodings_decode_every_byte() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Latin1, b"\xE9\xFF");
    assert_eq!(first_cells(&tm, 2), ["é", "ÿ"]);

    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Cp437, b"\x82");
    assert_eq!(first_cells(&tm, 1), ["é"]);
}

/// MAX-249 step 0: the control bytes the emulator ignores stay hidden (marking them is
/// MAX-253, not the invalid bytes fix).
/// How: "a" NUL 01 DEL "b".
#[test]
fn ignored_control_bytes_stay_hidden() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\x00\x01\x7Fb");
    assert_eq!(first_cells(&tm, 3), ["a", "b", ""]);
}

/// MAX-249: invalid UTF-8 shows as one `�` per broken char, like line mode; the text copy
/// and search read keeps a char per column, so a match after it is in the right column.
/// How: "a" F1 80 "b" 80 80 "c": a�b��c, "c" found at column 5; a screenshot has it too.
#[test]
fn invalid_utf8_shows_like_line_mode() {
    let raw_view = RawSideView::default();
    let input = b"a\xF1\x80b\x80\x80c";
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, input);
    assert_eq!(row_text(&mut tm, 0), String::from_utf8_lossy(input));
    assert_eq!(row_text(&mut tm, 0), "a\u{FFFD}b\u{FFFD}\u{FFFD}c");

    tm.handle_search_action(SearchModeAction::Query("c".to_string()), raw_view);
    assert_eq!(tm.search_state.matches[0].col_idx, 5);
    assert!(
        tm.screenshot()
            .text
            .starts_with("a\u{FFFD}b\u{FFFD}\u{FFFD}c")
    );
}

/// MAX-249: received as ASCII, each byte from 80 up is a `�`, as line mode shows it.
/// How: "a" E9 "b" in ASCII.
#[test]
fn ascii_high_bytes_show_as_replacement() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Ascii, b"a\xE9b");
    assert_eq!(row_text(&mut tm, 0), "a\u{FFFD}b");
}

/// MAX-249: a broken char cut at the end of a read shows once the next read says it's broken,
/// a valid one cut the same way is whole.
/// How: "a" F1 then "b": a�b.
#[test]
fn broken_char_cut_between_reads() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\xF1");
    assert_eq!(row_text(&mut tm, 0), "a");
    tm.process_rx(TextEncoding::Utf8, b"b");
    assert_eq!(row_text(&mut tm, 0), "a\u{FFFD}b");
}

/// MAX-249 step 3: the marks popup shows a row's invalid bytes as `�`.
/// How: "a" F1 "b" marked a.
#[test]
fn marks_popup_shows_invalid_bytes() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, b"a\xF1b");
    tm.marks.set('a', 0);
    assert_eq!(tm.mark_rows()[0].text.trim_end(), "a\u{FFFD}b");
}

/// Every byte the stream holds, both slices joined (MAX-252)
fn stream_bytes(tm: &TerminalMode) -> Vec<u8> {
    let stream = tm.dump.stream();
    let (front, back) = stream.slices(stream.start()..stream.end());
    [front, back].concat()
}

/// MAX-252 step 1: the stream keeps the device's bytes as received, before add CR, the
/// invalid bytes included.
/// How: "a\nb" F1 with add CR on, then "c".
#[test]
fn stream_keeps_the_device_bytes_as_received() {
    let mut tm = terminal(3);
    tm.process_device_rx(TextEncoding::Utf8, b"a\nb\xF1", true, false, true);
    tm.process_device_rx(TextEncoding::Utf8, b"c", true, false, true);
    assert_eq!(stream_bytes(&tm), b"a\nb\xF1c");
}

/// MAX-252 step 1: a restored session's chunks go in the stream, maxicom's own restore reset
/// doesn't; Clear history empties it.
/// How: restore "one\r\n" and "two" (no time), then clear the history.
#[test]
fn stream_holds_restored_bytes_not_the_reset() {
    let mut tm = terminal(3);
    let chunks = [
        TimedChunk {
            bytes: b"one\r\n",
            time: None,
        },
        TimedChunk {
            bytes: b"two",
            time: None,
        },
    ];
    tm.restore(TextEncoding::Utf8, &chunks, "--", true);
    assert_eq!(stream_bytes(&tm), b"one\r\ntwo");

    tm.clear_history();
    assert!(stream_bytes(&tm).is_empty());
}

// ---- MAX-256: which bytes wrote which cells ----

/// The device's bytes, read as they come in `reads`
fn device(tm: &mut TerminalMode, reads: &[&[u8]]) {
    for read in reads {
        tm.process_device_rx(TextEncoding::Utf8, read, false, false, true);
    }
}

/// The byte range `start..end`
fn span(start: u64, end: u64) -> std::ops::Range<u64> {
    start..end
}

/// The bytes of the writes at a world position, newest first
fn bytes_at(tm: &mut TerminalMode, line: usize, col: usize) -> Option<Vec<std::ops::Range<u64>>> {
    let writes = tm.writes_at(line, col, TextEncoding::Utf8, 32)?;
    Some(writes.into_iter().map(|write| write.bytes).collect())
}

/// MAX-256: a printable byte leads to the cell it drew, which still shows it; a control byte
/// or a sequence draws nothing. Colors between runs don't move the cells.
/// How: "ab" CR LF ESC[31m "red" ESC[0m " x": 'b' (1), 'r' (9), 'x' (17), the CR, the
/// sequence
#[test]
fn a_byte_leads_to_its_cell() {
    let mut tm = terminal(24);
    device(&mut tm, &[b"ab\r\n\x1b[31mred\x1b[0m x"]);
    let at = |tm: &mut TerminalMode, offset| tm.written_at(offset, TextEncoding::Utf8);
    assert_eq!(at(&mut tm, 1), Some(((0, 1), true)));
    assert_eq!(at(&mut tm, 9), Some(((1, 0), true)));
    assert_eq!(at(&mut tm, 17), Some(((1, 4), true)));
    assert_eq!(at(&mut tm, 2), None);
    assert_eq!(at(&mut tm, 4), None);
    assert_eq!(bytes_at(&mut tm, 1, 4), Some(vec![span(17, 18)]));
}

/// MAX-256: a cell written again and again (a progress bar) leads to the write it shows now,
/// then the earlier ones; an earlier byte leads to its cell, which no longer shows it.
/// How: CR "10%", CR "20%", CR "30%": the cell of the tens
#[test]
fn a_redrawn_cell_keeps_its_history() {
    let mut tm = terminal(24);
    device(&mut tm, &[b"\r10%", b"\r20%", b"\r30%"]);
    assert_eq!(bytes_at(&mut tm, 0, 0), Some(vec![9..10, 5..6, 1..2]));
    assert_eq!(tm.written_at(1, TextEncoding::Utf8), Some(((0, 0), false)));
    assert_eq!(tm.written_at(9, TextEncoding::Utf8), Some(((0, 0), true)));
}

/// MAX-256: a cell that no longer shows its newest write leads to no bytes, never wrong ones: an
/// erase leaves it blank, a scroll region moves other text onto it. (A moved char equal to the
/// one written there passes: the check reads the char, not where it came from.)
/// How: "abc" erased by ESC[2K; then "one" "two" "six" on rows 2 to 4, a scroll region on
/// them scrolled once: "two" is on the row "one" was written on
#[test]
fn a_changed_cell_has_no_link() {
    let mut tm = terminal(24);
    device(&mut tm, &[b"abc\x1b[2K"]);
    assert_eq!(bytes_at(&mut tm, 0, 0), None);
    assert_eq!(tm.written_at(0, TextEncoding::Utf8), Some(((0, 0), false)));

    let mut tm = terminal(24);
    device(
        &mut tm,
        &[b"\x1b[2;4r\x1b[2;1Hone\x1b[3;1Htwo\x1b[4;1Hsix\n"],
    );
    assert_eq!(bytes_at(&mut tm, 1, 0), None);
    assert_eq!(bytes_at(&mut tm, 2, 0), None);
}

/// MAX-256: a run wraps at the row's end, also when it starts there with the cursor waiting
/// past the last cell; wide chars take two cells, a char cut between two reads is one.
/// How: 80 columns: 85 'x'; 80 'y', a color, "z"; "中a"; 'é' cut between two reads
#[test]
fn cells_follow_wraps_wide_and_cut_chars() {
    let mut tm = terminal(24);
    device(&mut tm, &[&[b'x'; 85]]);
    assert_eq!(tm.written_at(82, TextEncoding::Utf8), Some(((1, 2), true)));

    let mut tm = terminal(24);
    let mut bytes = vec![b'y'; 80];
    bytes.extend_from_slice(b"\x1b[1mz");
    device(&mut tm, &[&bytes]);
    assert_eq!(tm.written_at(84, TextEncoding::Utf8), Some(((1, 0), true)));

    let mut tm = terminal(24);
    device(&mut tm, &["中a".as_bytes()]);
    assert_eq!(tm.written_at(3, TextEncoding::Utf8), Some(((0, 2), true)));
    assert_eq!(bytes_at(&mut tm, 0, 1), Some(vec![span(0, 3)]));

    let mut tm = terminal(24);
    device(&mut tm, &[b"a\xC3", b"\xA9b"]);
    assert_eq!(bytes_at(&mut tm, 0, 1), Some(vec![span(1, 3)]));
    assert_eq!(tm.written_at(3, TextEncoding::Utf8), Some(((0, 2), true)));
}

/// MAX-256: after a run and only colors, the next run starts where the run before ended (vt100
/// isn't asked): past the row's end it wraps. On the alternate screen's last row a wrap scrolls
/// the screen (it has no history, its rows move): the char past it leads nowhere, and vt100 is
/// asked again for the next run.
/// How: 78 'a', a color, "bcd" ('d' wraps); the alternate screen, its last row: 79 'e', a
/// color, "fg" ('g' scrolls the screen up), a color, "h"
#[test]
fn runs_after_colors_start_where_the_last_ended() {
    let mut tm = terminal(24);
    let mut bytes = vec![b'a'; 78];
    bytes.extend_from_slice(b"\x1b[1mbcd");
    device(&mut tm, &[&bytes]);
    // ESC[1m is bytes 78..82: 'b' 82, 'c' 83, 'd' 84
    assert_eq!(tm.written_at(82, TextEncoding::Utf8), Some(((0, 78), true)));
    assert_eq!(tm.written_at(83, TextEncoding::Utf8), Some(((0, 79), true)));
    assert_eq!(tm.written_at(84, TextEncoding::Utf8), Some(((1, 0), true)));

    let mut tm = terminal(24);
    let mut bytes = b"\x1b[?1049h\x1b[24;1H".to_vec();
    let start = bytes.len() as u64;
    bytes.extend_from_slice(&[b'e'; 79]);
    bytes.extend_from_slice(b"\x1b[31mfg\x1b[0mh");
    device(&mut tm, &[&bytes]);
    tm.refresh_max_scrollback();
    let last = tm.max_content_scrollback + 23;
    let g = start + 79 + 5 + 1;
    assert_eq!(tm.written_at(g, TextEncoding::Utf8), None);
    assert_eq!(
        tm.written_at(g + 5, TextEncoding::Utf8),
        Some(((last, 1), true))
    );
}

/// MAX-256: with the byte trace off (`byte_trace`) nothing is recorded: no bytes lead to the
/// screen, no cell to bytes.
/// How: "abc" received with the trace off
#[test]
fn no_trace_when_off() {
    let mut tm = terminal(24);
    tm.process_device_rx(TextEncoding::Utf8, b"abc", false, false, false);
    assert_eq!(tm.dump.writes().len(), 0);
    assert_eq!(tm.written_at(0, TextEncoding::Utf8), None);
    assert_eq!(bytes_at(&mut tm, 0, 0), None);
}

/// MAX-256: a broken char before a run is drawn (as its `�`) before the run's cell is read, also
/// when it was cut between two reads: the run lands after it.
/// How: "a" F1 80 "b" in one read, then "a" F1 | 80 "b"
#[test]
fn a_broken_char_before_a_run_takes_its_cell() {
    for reads in [&[&b"a\xF1\x80b"[..]][..], &[b"a\xF1", b"\x80b"]] {
        let mut tm = terminal(24);
        device(&mut tm, reads);
        assert_eq!(
            tm.written_at(3, TextEncoding::Utf8),
            Some(((0, 2), true)),
            "{reads:?}"
        );
    }
}

/// MAX-256: a `�` received as such is shown (vt100 alone drops it), and the byte trace finds its
/// cell as showing it.
/// How: "a" U+FFFD "b": the line reads "a�b", byte 1 drew column 1
#[test]
fn a_received_replacement_char_is_shown() {
    let mut tm = terminal(24);
    device(&mut tm, &["a\u{FFFD}b".as_bytes()]);
    assert_eq!(tm.copy_line_text(), "a\u{FFFD}b");
    assert_eq!(tm.written_at(1, TextEncoding::Utf8), Some(((0, 1), true)));
    assert_eq!(tm.written_at(4, TextEncoding::Utf8), Some(((0, 2), true)));
}

/// MAX-256: small reads don't multiply the runs, and rows scrolled into the history keep their
/// bytes; add CR doesn't shift the offsets.
/// How: "hello" a byte a read; 30 numbered lines on 24 rows; "a" LF "b" with add CR
#[test]
fn trace_survives_small_reads_scrolling_and_added_cr() {
    let mut tm = terminal(24);
    let bytes: Vec<&[u8]> = b"hello".chunks(1).collect();
    device(&mut tm, &bytes);
    assert_eq!(tm.dump.writes().len(), 1);

    let mut tm = terminal(24);
    let lines: String = (0..30).map(|i| format!("line {i:02}\r\n")).collect();
    device(&mut tm, &[lines.as_bytes()]);
    tm.refresh_max_scrollback();
    // "line 00" is in the history now
    assert_eq!(tm.written_at(0, TextEncoding::Utf8), Some(((0, 0), true)));
    assert_eq!(
        bytes_at(&mut tm, 29, 5),
        Some(vec![span(29 * 9 + 5, 29 * 9 + 6)])
    );

    let mut tm = terminal(24);
    tm.process_device_rx(TextEncoding::Utf8, b"a\nb", true, false, true);
    assert_eq!(tm.written_at(2, TextEncoding::Utf8), Some(((1, 0), true)));
}

/// MAX-256: on the alternate screen the trace is its own and holds while it's shown; back on
/// the main screen its writes lead nowhere, the main screen's lead to their cells again.
/// How: "main", then the alternate screen with "alt", then back
#[test]
fn the_alternate_screen_has_its_own_links() {
    let mut tm = terminal(24);
    device(&mut tm, &[b"main\x1b[?1049h\x1b[Halt"]);
    let alt = 4 + 8 + 3;
    tm.refresh_max_scrollback();
    let line = tm.max_content_scrollback;
    assert_eq!(
        tm.written_at(alt, TextEncoding::Utf8),
        Some(((line, 0), true))
    );
    assert_eq!(tm.written_at(0, TextEncoding::Utf8), None);

    device(&mut tm, &[b"\x1b[?1049l"]);
    assert_eq!(tm.written_at(alt, TextEncoding::Utf8), None);
    assert_eq!(tm.written_at(0, TextEncoding::Utf8), Some(((0, 0), true)));
}

// ---- MAX-270: differences found against minicom ----

/// MAX-270 step 0: vt100 keeps a zero-width char in the cell before it, and a variation
/// selector widens its emoji to two cells; copy and search read the cell's whole text. The
/// render fix (step 1) must not change either.
/// How: "ab" ZWSP "cd", "x" soft hyphen "y", "a" heart VS16 "b"
#[test]
fn zero_width_chars_stay_in_their_cell() {
    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, "ab\u{200B}cd".as_bytes());
    assert_eq!(first_cells(&tm, 4), ["a", "b\u{200B}", "c", "d"]);
    assert_eq!(row_text(&mut tm, 0), "ab\u{200B}cd");

    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, "x\u{AD}y".as_bytes());
    assert_eq!(first_cells(&tm, 2), ["x\u{AD}", "y"]);
    assert_eq!(row_text(&mut tm, 0), "x\u{AD}y");

    let mut tm = terminal(3);
    tm.process_rx(TextEncoding::Utf8, "a\u{2764}\u{FE0F}b".as_bytes());
    assert_eq!(first_cells(&tm, 4), ["a", "\u{2764}\u{FE0F}", "", "b"]);
    assert!(tm.vt100_screen().cell(0, 1).unwrap().is_wide());
    assert_eq!(row_text(&mut tm, 0), "a\u{2764}\u{FE0F}b");
}

/// MAX-270 step 0: Backspace sends DEL and Ctrl-h BS (xterm), Alt-Backspace ESC DEL; the
/// backspace setting (step 3) keeps these as the default.
/// How: the three keys
#[test]
fn backspace_bytes() {
    let none = KeyModifiers::NONE;
    assert_eq!(sent(KeyCode::Backspace, none, false), b"\x7f");
    assert_eq!(
        sent(KeyCode::Char('h'), KeyModifiers::CONTROL, false),
        b"\x08"
    );
    assert_eq!(
        sent(KeyCode::Backspace, KeyModifiers::ALT, false),
        b"\x1b\x7f"
    );
}

/// MAX-270: with `backspace = "bs"` Backspace sends BS, Alt-Backspace ESC BS; Ctrl-h,
/// Ctrl-Backspace (BS with both) and the other keys don't change.
/// How: the keys encoded with each setting
#[test]
fn backspace_setting_picks_the_byte() {
    let bs = |code, modifiers| {
        encode_terminal_key(KeyEvent::new(code, modifiers), false, BackspaceByte::Bs).unwrap()
    };
    let none = KeyModifiers::NONE;
    assert_eq!(bs(KeyCode::Backspace, none), b"\x08");
    assert_eq!(bs(KeyCode::Backspace, KeyModifiers::ALT), b"\x1b\x08");
    assert_eq!(bs(KeyCode::Char('h'), KeyModifiers::CONTROL), b"\x08");
    assert_eq!(bs(KeyCode::Delete, none), b"\x1b[3~");
    assert_eq!(bs(KeyCode::Char('a'), none), b"a");
    // Ctrl-Backspace is BS with both
    assert_eq!(bs(KeyCode::Backspace, KeyModifiers::CONTROL), b"\x08");
    assert_eq!(
        sent(KeyCode::Backspace, KeyModifiers::CONTROL, false),
        b"\x08"
    );
}

/// MAX-270 step 0: autowrap is on by default: a char past the last column goes to the next
/// row, the row marked wrapped; a full row then CR LF makes no blank row; a wide char that
/// doesn't fit on the last column goes whole to the next row. DECAWM (step 4) keeps this
/// while the mode is set.
/// How: 80 columns: 80 'x' "ab"; 80 'y' CR LF "z"; 79 'w' then a wide char
#[test]
fn autowrap_on_by_default() {
    let mut tm = terminal(5);
    let mut bytes = vec![b'x'; 80];
    bytes.extend_from_slice(b"ab");
    tm.process_rx(TextEncoding::Utf8, &bytes);
    assert_eq!(row_text(&mut tm, 0), "x".repeat(80));
    assert_eq!(row_text(&mut tm, 1), "ab");
    assert!(tm.vt100_screen().row_wrapped(0));

    let mut tm = terminal(5);
    let mut bytes = vec![b'y'; 80];
    bytes.extend_from_slice(b"\r\nz");
    tm.process_rx(TextEncoding::Utf8, &bytes);
    assert_eq!(row_text(&mut tm, 1), "z");

    let mut tm = terminal(5);
    let mut bytes = vec![b'w'; 79];
    bytes.extend_from_slice("中".as_bytes());
    tm.process_rx(TextEncoding::Utf8, &bytes);
    assert_eq!(row_text(&mut tm, 0), "w".repeat(79));
    assert_eq!(row_text(&mut tm, 1), "中");
}

// ---- MAX-256 benches: the byte trace's cost and coverage (ignored: run by hand, in release) ----
//
// cargo test --release trace_coverage_bench -- --ignored --nocapture
// cargo test --release trace_cost_bench -- --ignored --nocapture   (a few minutes)
//
// The cost bench feeds the same bytes with the trace off and on, for 2 sizes, 2 amounts, 7
// kinds of data and 3 read sizes; both print a table

/// Non-blank cells on the live screen: (with a current link, without), through the lookup the
/// UI uses
fn bench_coverage(tm: &mut TerminalMode) -> (usize, usize) {
    let (rows, cols) = tm.get_vt100_size();
    tm.refresh_max_scrollback();
    let sb = tm.max_content_scrollback;
    let lines: Vec<usize> = if tm.vt100_parser.screen().alternate_screen() {
        (sb..sb + rows as usize).collect()
    } else {
        // the screen shown live: a lookup scans the runs, every history row would take long
        (sb..sb + rows as usize).collect()
    };
    let (mut current, mut none) = (0, 0);
    for line in lines {
        let text = if tm.vt100_parser.screen().alternate_screen() {
            let screen = tm.vt100_parser.screen();
            screen.rows(0, cols).nth(line - sb).unwrap_or_default()
        } else {
            read_vt100_world_line_text(&mut tm.vt100_parser, sb, line).unwrap_or_default()
        };
        for (col, ch) in text.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            match tm.writes_at(line, col, TextEncoding::Utf8, 1) {
                Some(_) => current += 1,
                None => none += 1,
            }
        }
    }
    (current, none)
}

/// Feeds `bytes` in reads of `read` bytes, as a serial port gives them
fn bench_feed(tm: &mut TerminalMode, bytes: &[u8], read: usize) {
    for chunk in bytes.chunks(read) {
        tm.process_device_rx(TextEncoding::Utf8, chunk, false, false, true);
    }
}

#[test]
#[ignore = "MAX-256 bench: prints the byte trace's coverage"]
fn trace_coverage_bench() {
    let boot: String = (0..300)
        .map(|i| {
            format!(
                "[{:5}.{:06}] \x1b[32m  OK  \x1b[0m Started service number {i}.\r\n",
                i / 10,
                i * 337 % 1_000_000
            )
        })
        .collect();
    let mut progress = String::from("Downloading firmware.bin\r\n");
    for p in 0..=100 {
        let bar = "#".repeat(p / 2);
        progress.push_str(&format!("\r[{bar:<50}] \x1b[1m{p:3}%\x1b[0m"));
    }
    progress.push_str("\r\ndone\r\n$ ");
    let mut top = String::from("\x1b[?1049h");
    for frame in 0..10 {
        top.push_str("\x1b[H");
        for n in 0..22 {
            top.push_str(&format!(
                "{:5} root 20 0 {:6} S {:4.1} \x1b[1mproc{n}\x1b[0m\x1b[K\r\n",
                1000 + n,
                (n * 977 + frame * 13) % 100_000,
                ((n + frame) % 37) as f32 / 3.0
            ));
        }
        top.push_str("\x1b[J");
    }
    let mut vi = String::from("\x1b[?1049h\x1b[H\x1b[2J");
    for r in 1..=23 {
        vi.push_str(&format!("\x1b[{r};1Hfn line_{r}() {{ body {r} }}"));
    }
    vi.push_str("\x1b[1;23r");
    for n in 0..10 {
        vi.push_str(&format!("\x1b[23;1H\nfn added_{n}() {{}}"));
    }
    vi.push_str("\x1b[5;1H\x1b[Linserted line\x1b[10;3H\x1b[2P\x1b[r");
    let mut shell = String::from("$ ");
    for ch in "ls -la /tmp".chars() {
        shell.push(ch);
    }
    shell.push_str("\x08\x1b[K\x08\x1b[K\x08\x1b[Kvar\r\x1b[K$ cat notes.txt\r\n");
    for i in 0..30 {
        shell.push_str(&format!(
            "note {i}: \x1b[33mtodo\x1b[0m check the board\r\n"
        ));
    }
    shell.push_str("$ ");

    for (name, bytes) in [
        ("boot log", boot.as_bytes()),
        ("progress bar", progress.as_bytes()),
        ("top (alternate)", top.as_bytes()),
        ("vi (alternate, scroll region, IL, DCH)", vi.as_bytes()),
        ("shell line editing", shell.as_bytes()),
    ] {
        let mut tm = TerminalMode::new(1000, 24, 80);
        bench_feed(&mut tm, bytes, 64);
        let (current, none) = bench_coverage(&mut tm);
        let all = (current + none).max(1) as f64;
        println!(
            "{name:40} cells {:6}  current {:5.1}%  no link {:5.1}%  runs {}",
            current + none,
            100.0 * current as f64 / all,
            100.0 * none as f64 / all,
            tm.dump.writes().len(),
        );
    }
}

/// The bench's streams by kind, about `amount` bytes each, for a `cols` wide screen
fn bench_streams(amount: usize, cols: usize) -> Vec<(&'static str, Vec<u8>)> {
    let fill = |unit: &dyn Fn(usize) -> String| {
        let mut out = String::new();
        let mut i = 0;
        while out.len() < amount {
            out.push_str(&unit(i));
            i += 1;
        }
        out.into_bytes()
    };
    let words = [
        "error", "warn", "info", "debug", "trace", "ok", "fail", "done",
    ];
    let mut garbage = Vec::with_capacity(amount);
    let mut seed: u32 = 0x1234_5678;
    while garbage.len() < amount {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        garbage.push((seed >> 16) as u8);
    }
    vec![
        (
            "plain log",
            fill(&|i| format!("[{:8}] sensor {i}: temp=21.{} hum=40 ok\r\n", i * 7, i % 10)),
        ),
        (
            "colored words",
            fill(&|i| {
                let line: Vec<String> = (0..8)
                    .map(|w| format!("\x1b[3{}m{}\x1b[0m", (i + w) % 7 + 1, words[(i + w) % 8]))
                    .collect();
                format!("{}\r\n", line.join(" "))
            }),
        ),
        (
            "progress bar",
            fill(&|i| {
                let p = i % 101;
                let bar = "#".repeat(p / 2);
                let end = if p == 100 { "\r\n" } else { "" };
                format!("\r[{bar:<50}] \x1b[1m{p:3}%\x1b[0m{end}")
            }),
        ),
        (
            "top redraw",
            fill(&|i| {
                let mut frame = String::from("\x1b[H");
                for n in 0..20 {
                    frame.push_str(&format!(
                        "{:5} root 20 0 {:6} S {:4.1} \x1b[1mproc{n}\x1b[0m\x1b[K\r\n",
                        1000 + n,
                        (n * 977 + i * 13) % 100_000,
                        ((n + i) % 37) as f32 / 3.0
                    ));
                }
                frame
            }),
        ),
        (
            "vi scroll region",
            fill(&|i| {
                // a line inserted now and then, as vi opening one
                let insert = if i % 10 == 0 { "\x1b[5;1H\x1b[L" } else { "" };
                format!("\x1b[1;20r\x1b[20;1H\nfn added_{i}() {{ x }}{insert}\x1b[r")
            }),
        ),
        (
            "long wrapping lines",
            fill(&|i| format!("{}\r\n", "x".repeat(cols * 3 + i % 7))),
        ),
        ("binary garbage", garbage),
    ]
}

/// The bytes received in reads of `read` bytes, the trace off or on: seconds taken
fn bench_time(bytes: &[u8], rows: u16, cols: u16, read: usize, trace: bool) -> (f64, TerminalMode) {
    let mut tm = TerminalMode::new(10_000, rows, cols);
    let start = std::time::Instant::now();
    for chunk in bytes.chunks(read) {
        tm.process_device_rx(TextEncoding::Utf8, chunk, false, false, trace);
    }
    (start.elapsed().as_secs_f64(), tm)
}

#[test]
#[ignore = "MAX-256 bench: prints the byte trace's cost (run in release)"]
fn trace_cost_bench() {
    let mb = 1024.0 * 1024.0;
    println!(
        "{:10} {:6} {:22} {:>6} {:>9} {:>9} {:>7} {:>9} {:>8} {:>8}",
        "size",
        "MB",
        "kind",
        "read",
        "off MB/s",
        "on MB/s",
        "cost",
        "runs/MB",
        "current",
        "no link"
    );
    for (rows, cols) in [(24u16, 80u16), (60, 200)] {
        for amount in [1 << 20, 10 << 20] {
            for (kind, bytes) in bench_streams(amount, cols as usize) {
                for read in [1usize, 64, 4096] {
                    // a 1 byte read of 10 MB takes long and says the same as at 1 MB
                    if read == 1 && amount > 1 << 20 {
                        continue;
                    }
                    let (old, _) = bench_time(&bytes, rows, cols, read, false);
                    let (new, mut tm) = bench_time(&bytes, rows, cols, read, true);
                    let size = bytes.len() as f64 / mb;
                    let runs = tm.dump.writes().len() as f64;
                    let (current, none) = bench_coverage(&mut tm);
                    let all = (current + none).max(1) as f64;
                    println!(
                        "{:10} {:6.1} {:22} {:>6} {:>9.1} {:>9.1} {:>+6.0}% {:>9.0} {:>7.1}% {:>7.1}%",
                        format!("{cols}x{rows}"),
                        size,
                        kind,
                        read,
                        size / old,
                        size / new,
                        100.0 * (new - old) / old,
                        runs / size,
                        100.0 * current as f64 / all,
                        100.0 * none as f64 / all,
                    );
                }
            }
        }
    }
}
