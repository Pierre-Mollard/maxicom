//! Bytes sent in terminal mode: typed keys (#31) and bracketed paste (#28).

use super::*;

/// Bytes restored without their times
fn untimed(bytes: &[u8]) -> [TimedChunk<'_>; 1] {
    [TimedChunk { bytes, time: None }]
}
use crossterm::event::{KeyCode, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sent(code: KeyCode, modifiers: KeyModifiers, application_cursor: bool) -> Vec<u8> {
    encode_terminal_key(KeyEvent::new(code, modifiers), application_cursor).unwrap()
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
fn timeline(tm: &TerminalMode) -> Vec<Option<DateTime<Local>>> {
    (0..tm.total_timeline_len())
        .map(|i| tm.get_timeline_timestamp(i))
        .collect()
}

fn terminal_with(history: usize, rows: u16) -> TerminalMode {
    TerminalMode::new(history, rows, 80)
}

/// Feeds each line as its own chunk, 1 ms apart, and returns each line's text with the
/// timestamp of the row the cursor is on right after it arrived.
fn feed_and_stamp(tm: &mut TerminalMode, lines: &[&str]) -> Vec<(String, Option<DateTime<Local>>)> {
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
fn assert_rows_keep_their_stamps(
    tm: &mut TerminalMode,
    stamps: &[(String, Option<DateTime<Local>>)],
) {
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
        .update_action_terminal(TerminalAction::Key(key(KeyCode::Up)))
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
    tm.update_viewport(ViewportSize::new(cols, rows), mode, 24, 80);
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
    tm.restore(TextEncoding::Utf8, &untimed(raw), "-- restored --");
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
    tm.restore(TextEncoding::Utf8, &untimed(b"abc"), "-- restored --");
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
        );
        if let Some(live) = live {
            tm.process_rx(TextEncoding::Utf8, format!("{live}\r\n").as_bytes());
        }
        let viewport = ViewportSize::new(80, 50);
        tm.update_viewport(viewport, TerminalDimensionMode::GrowOnly, 24, 80);
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
    let stty = |tm: &mut TerminalMode| match tm
        .update_action_terminal(TerminalAction::AutoResizeRemoteToEmulator)
    {
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
