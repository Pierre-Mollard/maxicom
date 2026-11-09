//! Query parsing and match finding for text and raw-byte search.

use super::*;
use chrono::Local;

fn history(lines: &[&str]) -> VecDeque<LogEntry> {
    lines
        .iter()
        .map(|t| LogEntry::new(t.to_string(), Local::now()))
        .collect()
}

fn search(query: &str, kind: SearchModeKind) -> SearchState {
    SearchState {
        active: true,
        query: Input::from(query),
        mode_kind: kind,
        ..Default::default()
    }
}

/// Hex queries accept spaced, packed, `0x` and `\x` forms, and reject invalid input.
/// How: a table of inputs and expected bytes.
#[test]
fn parse_hex_variants() {
    let v = RawSideView::Hex;
    assert_eq!(parse_raw_bytes_for_view("41 42", v), Some(vec![0x41, 0x42]));
    assert_eq!(parse_raw_bytes_for_view("4142", v), Some(vec![0x41, 0x42]));
    assert_eq!(
        parse_raw_bytes_for_view("0x41 0x42", v),
        Some(vec![0x41, 0x42])
    );
    assert_eq!(
        parse_raw_bytes_for_view("\\x41\\x42", v),
        Some(vec![0x41, 0x42])
    );
    assert_eq!(parse_raw_bytes_for_view("4", v), Some(vec![0x04]));
    assert_eq!(parse_raw_bytes_for_view("414", v), None);
    assert_eq!(parse_raw_bytes_for_view("zz", v), None);
    assert_eq!(parse_raw_bytes_for_view("   ", v), None);
    assert_eq!(
        parse_raw_bytes_for_view("41", RawSideView::Off),
        Some(vec![0x41])
    );
}

/// Octal and binary queries accept spaced, packed and prefixed forms.
/// How: a table of inputs, including an overflow.
#[test]
fn parse_octal_and_binary_variants() {
    assert_eq!(
        parse_raw_bytes_for_view("101 102", RawSideView::Octal),
        Some(vec![65, 66])
    );
    assert_eq!(
        parse_raw_bytes_for_view("101102", RawSideView::Octal),
        Some(vec![65, 66])
    );
    assert_eq!(
        parse_raw_bytes_for_view("0o101", RawSideView::Octal),
        Some(vec![65])
    );
    assert_eq!(parse_raw_bytes_for_view("777", RawSideView::Octal), None);

    let b = RawSideView::Binary;
    assert_eq!(parse_raw_bytes_for_view("01000001", b), Some(vec![65]));
    assert_eq!(
        parse_raw_bytes_for_view("0b01000001 0b01000010", b),
        Some(vec![65, 66])
    );
    assert_eq!(parse_raw_bytes_for_view("1", b), Some(vec![1]));
    assert_eq!(parse_raw_bytes_for_view("2", b), None);
}

/// The byte search reports every occurrence, overlapping ones included.
/// How: known haystacks and needles.
#[test]
fn subslice_finds_all_including_overlaps() {
    let found: Vec<usize> = find_byte_subslice_iter(b"abcabc", b"bc").collect();
    assert_eq!(found, [1, 4]);
    let found: Vec<usize> = find_byte_subslice_iter(b"aaa", b"aa").collect();
    assert_eq!(found, [0, 1]);
    assert_eq!(find_byte_subslice_iter(b"ab", b"abc").count(), 0);
}

/// A prefix alone ("0x", "0o", "0b") is not a valid query (#27).
/// How: parses each prefix in its view.
#[test]
fn parse_prefix_only_is_rejected() {
    assert_eq!(parse_raw_bytes_for_view("0x", RawSideView::Hex), None);
    assert_eq!(parse_raw_bytes_for_view("0o", RawSideView::Octal), None);
    assert_eq!(parse_raw_bytes_for_view("0b", RawSideView::Binary), None);
}

/// An empty needle matches nothing instead of panicking (#27).
/// How: counts matches of b"" in a haystack.
#[test]
fn subslice_empty_needle_matches_nothing() {
    assert_eq!(find_byte_subslice_iter(b"abc", b"").count(), 0);
}

/// Text search ignores case and jumps to the first match.
/// How: three lines, two matching "hello".
#[test]
fn text_search_is_case_insensitive() {
    let mut s = search("hello", SearchModeKind::Text);
    s.update_matches_from_entries(
        &history(&["hello world", "nothing", "say Hello"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    let found: Vec<(usize, usize)> = s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
    assert_eq!(found, [(0, 0), (2, 4)]);
    assert_eq!(s.current_match, 0);
    assert_eq!(s.target_jump, Some((0, 0)));
}

/// A text match gets the right display column and raw byte span.
/// How: match "here" after an ANSI-colored word.
#[test]
fn text_search_maps_back_to_raw_bytes() {
    // display "ERROR here": 'h' is col 6, raw byte 15 (after ESC[31m ERROR ESC[0m and a space)
    let mut s = search("here", SearchModeKind::Text);
    s.update_matches_from_entries(
        &history(&["\x1b[31mERROR\x1b[0m here"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    assert_eq!(s.matches.len(), 1);
    let m = s.matches[0];
    assert_eq!((m.col_idx, m.byte_offset, m.byte_len), (6, 15, 4));
}

/// A raw-byte match maps back to its display column.
/// How: search 45 52 ("ER") inside a colored ERROR.
#[test]
fn raw_search_maps_to_display_col() {
    let mut s = search("45 52", SearchModeKind::RawBytes);
    s.update_matches_from_entries(
        &history(&["\x1b[31mERROR\x1b[0m"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    assert_eq!(s.matches.len(), 1);
    let m = s.matches[0];
    assert_eq!((m.col_idx, m.byte_offset, m.byte_len), (0, 5, 2));
}

/// A raw-byte match in Hex display mode highlights from its digit group, not a separator (#24).
/// How: search byte 0x42 in "ABC" shown as "41 42 43", expect col 3.
#[test]
fn raw_search_in_hex_display_starts_on_group() {
    let mut s = search("42", SearchModeKind::RawBytes);
    s.update_matches_from_entries(&history(&["ABC"]), LineDisplayMode::Hex, RawSideView::Hex);
    assert_eq!(s.matches.len(), 1);
    assert_eq!((s.matches[0].col_idx, s.matches[0].byte_offset), (3, 1));
}

/// A match after a non-ASCII char gets the right column, in line and terminal search (#18).
/// How: 'İ' (lowercases from 2 to 3 bytes) before "x": col 1 (was 2 with to_lowercase).
#[test]
fn text_search_after_non_ascii_keeps_col() {
    let mut s = search("x", SearchModeKind::Text);
    s.update_matches_from_entries(&history(&["İx"]), LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches[0].col_idx, 1);

    let mut s = search("X", SearchModeKind::Text);
    s.update_matches_from_strings(&["İx".to_string()], RawSideView::Hex);
    assert_eq!((s.matches[0].col_idx, s.matches[0].byte_offset), (1, 2));
}

/// Case-insensitive finder: ASCII case ignored, non-overlapping, offsets in the original.
/// How: known haystacks, including a multibyte char before the match.
#[test]
fn ascii_case_insensitive_finder() {
    let found: Vec<usize> =
        find_ascii_case_insensitive_iter(b"Error error ERROR", b"eRRor").collect();
    assert_eq!(found, [0, 6, 12]);
    let found: Vec<usize> = find_ascii_case_insensitive_iter(b"aaaa", b"aa").collect();
    assert_eq!(found, [0, 2]);
    let found: Vec<usize> = find_ascii_case_insensitive_iter("é OK".as_bytes(), b"ok").collect();
    assert_eq!(found, [3]);
    assert_eq!(find_ascii_case_insensitive_iter(b"abc", b"").count(), 0);
    assert_eq!(find_ascii_case_insensitive_iter(b"ab", b"abc").count(), 0);
    let found: Vec<usize> = find_ascii_case_insensitive_iter(b"xab", b"AB").collect();
    assert_eq!(found, [1]);
}

/// Non-ASCII chars must match exactly: only ASCII case is folded (#18 behavior change).
/// How: 'é' matches 'é' but not 'É'.
#[test]
fn ascii_case_insensitive_finder_non_ascii_exact() {
    assert_eq!(
        find_ascii_case_insensitive_iter("café".as_bytes(), "é".as_bytes()).count(),
        1
    );
    assert_eq!(
        find_ascii_case_insensitive_iter("CAFÉ".as_bytes(), "é".as_bytes()).count(),
        0
    );
}

/// Lines without a match are skipped and matches on later lines keep the right line index.
/// How: a match only on the last of several lines, in text and raw search.
#[test]
fn search_skips_lines_without_match() {
    let lines = history(&["\x1b[32mok\x1b[0m", "nothing here", "the TARGET line"]);
    let mut s = search("target", SearchModeKind::Text);
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    let found: Vec<(usize, usize, usize)> = s
        .matches
        .iter()
        .map(|m| (m.line_idx, m.col_idx, m.byte_len))
        .collect();
    assert_eq!(found, [(2, 4, 6)]);

    let mut s = search("54 41", SearchModeKind::RawBytes);
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    let found: Vec<(usize, usize)> = s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
    assert_eq!(found, [(2, 4)]);
}

/// The match cap still stops the search at MAX_SEARCH_MATCHES.
/// How: more matching lines than the cap.
#[test]
fn search_stops_at_match_cap() {
    let lines: Vec<&str> = vec!["a"; MAX_SEARCH_MATCHES + 10];
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(&history(&lines), LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches.len(), MAX_SEARCH_MATCHES);
}

/// Clearing the query clears matches and the jump target.
/// How: search, then search again with an empty query.
#[test]
fn empty_query_clears_matches() {
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(&history(&["a"]), LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches.len(), 1);
    s.query = Input::default();
    s.update_matches_from_entries(&history(&["a"]), LineDisplayMode::Pretty, RawSideView::Hex);
    assert!(s.matches.is_empty());
    assert_eq!(s.target_jump, None);
}

/// Eviction shifts matches, drops the ones on removed lines and keeps the current one.
/// How: matches on lines 0, 2, 3 with the current on line 2, then 1 then 3 lines evicted.
#[test]
fn rebase_after_eviction_shifts_and_drops() {
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(
        &history(&["a", "b", "a", "a"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    s.next_match();
    assert_eq!(s.current_match_position(), Some((2, 0)));

    s.rebase_after_eviction(1);
    let lines: Vec<usize> = s.matches.iter().map(|m| m.line_idx).collect();
    assert_eq!(lines, [1, 2]);
    assert_eq!(s.current_match_position(), Some((1, 0)));
    assert_eq!(s.target_jump, Some((1, 0)));

    s.rebase_after_eviction(3);
    assert!(s.matches.is_empty());
    assert_eq!(s.current_match, 0);
    assert_eq!(s.target_jump, None);
}

/// Next/previous cycle through matches, wrapping at both ends.
/// How: two matches, stepping both ways.
#[test]
fn next_prev_wrap_around() {
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(
        &history(&["a", "a"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    assert_eq!(s.prev_match(), Some((1, 0)));
    assert_eq!(s.next_match(), Some((0, 0)));
    assert_eq!(s.next_match(), Some((1, 0)));
}

/// Terminal-mode search over plain strings finds every case-insensitive occurrence.
/// How: three lines, checked (line, col) pairs.
#[test]
fn terminal_string_search() {
    let mut s = search("ok", SearchModeKind::Text);
    let lines = vec![
        "all OK".to_string(),
        "none".to_string(),
        "ok ok".to_string(),
    ];
    s.update_matches_from_strings(&lines, RawSideView::Hex);
    let found: Vec<(usize, usize)> = s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
    assert_eq!(found, [(0, 4), (2, 0), (2, 3)]);
}

/// MAX-215: a new search starts on the oldest match, also after a step to another one.
/// How: matches on lines 0 and 2, Next, then the search runs again (a typed key).
#[test]
fn a_new_search_starts_on_the_oldest_match() {
    let lines = history(&["a", "b", "a"]);
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.current_match_position(), Some((0, 0)));
    assert_eq!(s.target_jump, Some((0, 0)));

    s.next_match();
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.current_match_position(), Some((0, 0)));
    assert_eq!(s.target_jump, Some((0, 0)));
}

/// MAX-215: a backward search starts on the newest match, the last one of its line, over
/// history entries and terminal strings.
/// How: "a" on lines 0 and 2 (twice), searched backward both ways.
#[test]
fn a_backward_search_starts_on_the_newest_match() {
    let mut s = search("a", SearchModeKind::Text);
    s.direction = SearchDirection::Backward;
    s.update_matches_from_entries(
        &history(&["a", "b", "a a"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    assert_eq!(s.current_match_position(), Some((2, 2)));
    assert_eq!(s.target_jump, Some((2, 2)));
    assert_eq!(s.current_match_display(), 3);

    let lines = ["a".to_string(), "b".to_string(), "a a".to_string()];
    s.update_matches_from_strings(&lines, RawSideView::Hex);
    assert_eq!(s.current_match_position(), Some((2, 2)));
    let found: Vec<(usize, usize)> = s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
    assert_eq!(found, [(0, 0), (2, 0), (2, 2)]);
}

/// MAX-215: past the match cap, a forward search keeps the oldest matches and a backward
/// one the newest, still sorted by line with their occurrence numbers.
/// How: two matches per line on more lines than the cap holds, searched both ways.
#[test]
fn the_match_cap_keeps_the_end_a_search_starts_from() {
    let lines: Vec<&str> = vec!["a a"; MAX_SEARCH_MATCHES / 2 + 10];
    let lines = history(&lines);
    let last_line = lines.len() - 1;
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches.len(), MAX_SEARCH_MATCHES);
    assert_eq!(s.matches[0].line_idx, 0);
    assert_eq!(s.matches.last().unwrap().line_idx, last_line - 10);

    s.direction = SearchDirection::Backward;
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches.len(), MAX_SEARCH_MATCHES);
    let first = s.matches[0];
    assert_eq!((first.line_idx, first.occurrence_in_line), (10, 0));
    assert!(s.matches.is_sorted_by_key(|m| (m.line_idx, m.col_idx)));
    assert_eq!(s.current_match_position(), Some((last_line, 2)));
}

fn origin(line_idx: usize, char_idx: usize) -> Option<LogicalPosition> {
    Some(LogicalPosition {
        line_idx,
        char_idx,
        ..Default::default()
    })
}

/// MAX-215: from copy mode a search starts after the cursor forward and before it backward,
/// a match on the cursor itself skipped, around the history's end when nothing is past it.
/// How: "a" at (0,0), (1,0), (1,2), (3,0), searched both ways from (1,0) then from the ends.
#[test]
fn a_search_from_copy_mode_starts_at_the_cursor() {
    let lines = history(&["a", "a a", "b", "a"]);
    let start = |direction, origin| {
        let mut s = search("a", SearchModeKind::Text);
        s.direction = direction;
        s.origin = origin;
        s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
        s.current_match_position()
    };
    use SearchDirection::{Backward, Forward};
    assert_eq!(start(Forward, origin(1, 0)), Some((1, 2)));
    assert_eq!(start(Backward, origin(1, 0)), Some((0, 0)));
    assert_eq!(start(Forward, origin(3, 0)), Some((0, 0)));
    assert_eq!(start(Backward, origin(0, 0)), Some((3, 0)));
}

/// MAX-215: past the match cap, a search from copy mode keeps the matches it reaches first
/// from the cursor, around the history's end, still sorted by line.
/// How: one match per line on more lines than the cap holds, from a cursor near the end.
#[test]
fn the_match_cap_keeps_the_matches_after_the_cursor() {
    let lines: Vec<&str> = vec!["a"; MAX_SEARCH_MATCHES + 100];
    let lines = history(&lines);
    let mut s = search("a", SearchModeKind::Text);
    s.origin = origin(MAX_SEARCH_MATCHES + 50, 0);
    s.update_matches_from_entries(&lines, LineDisplayMode::Pretty, RawSideView::Hex);
    assert_eq!(s.matches.len(), MAX_SEARCH_MATCHES);
    assert!(s.matches.is_sorted_by_key(|m| m.line_idx));
    assert_eq!(s.matches[0].line_idx, 0);
    assert_eq!(
        s.current_match_position(),
        Some((MAX_SEARCH_MATCHES + 51, 0))
    );
    assert_eq!(s.matches.last().unwrap().line_idx, MAX_SEARCH_MATCHES + 99);
}

/// MAX-215: `n` goes to the next match from the cursor in the search's direction, `N` the
/// other way, a match on the cursor skipped; past the last one it goes around, said by the
/// bool. Without a match there is nowhere to go.
/// How: "a" at (0,0), (1,0), (1,2), stepping from a few cursors in both directions.
#[test]
fn step_from_the_cursor_both_ways() {
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(
        &history(&["a", "a a"]),
        LineDisplayMode::Pretty,
        RawSideView::Hex,
    );
    let mut step = |from, opposite| {
        let wrapped = s.step_from(from, opposite)?;
        assert_eq!(s.target_jump, s.current_match_position());
        Some((s.current_match_position()?, wrapped))
    };
    assert_eq!(step((1, 0), false), Some(((1, 2), false)));
    assert_eq!(step((1, 0), true), Some(((0, 0), false)));
    assert_eq!(step((1, 2), false), Some(((0, 0), true)));
    assert_eq!(step((0, 0), true), Some(((1, 2), true)));

    s.direction = SearchDirection::Backward;
    assert_eq!(s.step_from((1, 0), false), Some(false));
    assert_eq!(s.current_match_position(), Some((0, 0)));
    assert_eq!(s.step_from((1, 0), true), Some(false));
    assert_eq!(s.current_match_position(), Some((1, 2)));

    s.matches.clear();
    assert_eq!(s.step_from((0, 0), false), None);
}

/// MAX-215: the matches are drawn while the bar is open, or kept after Enter, never without
/// a match.
/// How: shows_matches with the bar open, closed, kept, and without matches.
#[test]
fn matches_shown_while_open_or_kept() {
    let mut s = search("a", SearchModeKind::Text);
    s.update_matches_from_entries(&history(&["a"]), LineDisplayMode::Pretty, RawSideView::Hex);
    assert!(s.shows_matches());
    s.toggle(false);
    assert!(!s.shows_matches());
    s.kept = true;
    assert!(s.shows_matches());
    s.matches.clear();
    assert!(!s.shows_matches());
}

/// MAX-218: the bar opens empty, the query it had kept as the last one; an empty query (or
/// none on the first search) keeps the last one. Enter on the empty bar puts it back, and
/// Esc drops what was typed for it.
/// How: open_empty, reuse_last_query and restore_last_query on a few queries.
#[test]
fn last_query_kept_when_the_bar_opens() {
    let mut s = search("hit", SearchModeKind::Text);
    s.open_empty();
    assert_eq!((s.query_str(), s.last_query.as_str()), ("", "hit"));
    s.open_empty();
    assert_eq!(s.last_query, "hit");

    assert!(s.reuse_last_query());
    assert_eq!(s.query_str(), "hit");
    assert!(!s.reuse_last_query());

    s.query = Input::from("typed");
    s.restore_last_query();
    assert_eq!(s.query_str(), "hit");

    let mut first = search("typed", SearchModeKind::Text);
    first.restore_last_query();
    assert_eq!(first.query_str(), "");
    assert!(!first.reuse_last_query());
}

/// MAX-218: Up walks to older searches among those starting with what was typed, Down back
/// to newer ones and past the newest to what was typed; an edit starts a new walk, Down
/// alone does nothing.
/// How: a history of four searches, walks from "er" and from an empty query.
#[test]
fn walk_the_search_history() {
    let history: VecDeque<String> = ["error", "boot", "err 2", "warn"].map(String::from).into();
    let mut s = search("er", SearchModeKind::Text);
    let walk = |s: &mut SearchState, up| {
        let changed = s.walk_history(&history, up);
        (changed, s.query_str().to_string())
    };
    assert_eq!(walk(&mut s, false), (false, "er".into()));
    assert_eq!(walk(&mut s, true), (true, "err 2".into()));
    assert_eq!(walk(&mut s, true), (true, "error".into()));
    assert_eq!(walk(&mut s, true), (true, "error".into()));
    assert_eq!(walk(&mut s, false), (true, "err 2".into()));
    assert_eq!(walk(&mut s, false), (true, "er".into()));
    assert_eq!(walk(&mut s, false), (false, "er".into()));

    s.query = Input::default();
    assert_eq!(walk(&mut s, true), (true, "warn".into()));
    s.query = Input::from("b");
    assert_eq!(walk(&mut s, true), (true, "boot".into()));
}
