use std::collections::VecDeque;
use tui_input::Input;

use crate::app::data_processing::{
    build_display_to_raw_byte_map, displayable_line_string, map_disp_byte_to_raw,
    raw_byte_to_display_col, safe_byte_to_char_col,
};
use crate::app::types::LogEntry;
use crate::configs::LineDisplayMode;
use crate::configs::settings::RawSideView;

pub const MAX_SEARCH_MATCHES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchModeKind {
    #[default]
    Text,
    RawBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_idx: usize,
    pub col_idx: usize,
    pub occurrence_in_line: usize,
    pub match_char_len: usize,
    pub byte_offset: usize,
    pub byte_len: usize,
}

#[derive(Default)]
pub struct SearchState {
    pub active: bool,
    pub query: Input,
    pub mode_kind: SearchModeKind,
    pub matches: Vec<SearchMatch>,
    pub current_match: usize,
    pub target_jump: Option<(usize, usize)>,
}

impl SearchState {
    pub fn update_matches_from_entries(
        &mut self,
        history: &VecDeque<LogEntry>,
        display_mode: LineDisplayMode,
        raw_view: RawSideView,
    ) {
        let q_str = self.query.value().to_string();
        if q_str.is_empty() {
            self.matches.clear();
            self.current_match = 0;
            self.target_jump = None;
            return;
        }

        self.matches.clear();

        match self.mode_kind {
            SearchModeKind::RawBytes => {
                if let Some(target_bytes) = parse_raw_bytes_for_view(&q_str, raw_view) {
                    let needle_len = target_bytes.len();
                    'outer_raw: for (line_idx, entry) in history.iter().enumerate() {
                        let raw_bytes = entry.text.as_bytes();
                        let display_text = displayable_line_string(&entry.text, display_mode);
                        let disp_map = build_display_to_raw_byte_map(raw_bytes, display_mode);

                        for (occ, raw_byte_idx) in
                            find_byte_subslice_iter(raw_bytes, &target_bytes).enumerate()
                        {
                            let col_idx =
                                raw_byte_to_display_col(raw_byte_idx, &disp_map, &display_text);

                            self.matches.push(SearchMatch {
                                line_idx,
                                col_idx,
                                occurrence_in_line: occ,
                                match_char_len: needle_len,
                                byte_offset: raw_byte_idx,
                                byte_len: needle_len,
                            });

                            if self.matches.len() >= MAX_SEARCH_MATCHES {
                                break 'outer_raw;
                            }
                        }
                    }
                }
            }
            SearchModeKind::Text => {
                let q_lower = q_str.to_lowercase();
                let q_char_len = q_str.chars().count();

                'outer_text: for (line_idx, entry) in history.iter().enumerate() {
                    let raw_bytes = entry.text.as_bytes();
                    let display_text = displayable_line_string(&entry.text, display_mode);
                    let line_lower = display_text.to_lowercase();
                    let disp_map = build_display_to_raw_byte_map(raw_bytes, display_mode);

                    for (occurrence, (disp_byte_idx, matched_str)) in
                        line_lower.match_indices(&q_lower).enumerate()
                    {
                        let col_idx = safe_byte_to_char_col(&display_text, disp_byte_idx);
                        let raw_byte_offset =
                            map_disp_byte_to_raw(disp_byte_idx, &disp_map, raw_bytes.len());
                        let raw_byte_end = map_disp_byte_to_raw(
                            disp_byte_idx + matched_str.len(),
                            &disp_map,
                            raw_bytes.len(),
                        );

                        self.matches.push(SearchMatch {
                            line_idx,
                            col_idx,
                            occurrence_in_line: occurrence,
                            match_char_len: q_char_len,
                            byte_offset: raw_byte_offset,
                            byte_len: raw_byte_end
                                .saturating_sub(raw_byte_offset)
                                .max(matched_str.len()),
                        });

                        if self.matches.len() >= MAX_SEARCH_MATCHES {
                            break 'outer_text;
                        }
                    }
                }
            }
        }

        self.current_match = 0;
        self.target_jump = self.matches.first().map(|m| (m.line_idx, m.col_idx));
    }

    pub fn update_matches_from_strings(&mut self, all_lines: &[String], raw_view: RawSideView) {
        let q_str = self.query.value().to_string();
        if q_str.is_empty() {
            self.matches.clear();
            self.current_match = 0;
            self.target_jump = None;
            return;
        }

        self.matches.clear();

        match self.mode_kind {
            SearchModeKind::RawBytes => {
                if let Some(target_bytes) = parse_raw_bytes_for_view(&q_str, raw_view) {
                    let needle_len = target_bytes.len();
                    'outer_raw: for (line_idx, line_text) in all_lines.iter().enumerate() {
                        let bytes = line_text.as_bytes();
                        for (occ, byte_idx) in
                            find_byte_subslice_iter(bytes, &target_bytes).enumerate()
                        {
                            let col_idx = safe_byte_to_char_col(line_text, byte_idx);
                            self.matches.push(SearchMatch {
                                line_idx,
                                col_idx,
                                occurrence_in_line: occ,
                                match_char_len: needle_len,
                                byte_offset: byte_idx,
                                byte_len: needle_len,
                            });

                            if self.matches.len() >= MAX_SEARCH_MATCHES {
                                break 'outer_raw;
                            }
                        }
                    }
                }
            }
            SearchModeKind::Text => {
                let q_lower = q_str.to_lowercase();
                let q_char_len = q_str.chars().count();
                'outer_text: for (line_idx, line_text) in all_lines.iter().enumerate() {
                    let line_lower = line_text.to_lowercase();
                    for (occurrence, (byte_idx, _)) in
                        line_lower.match_indices(&q_lower).enumerate()
                    {
                        let col_idx = safe_byte_to_char_col(line_text, byte_idx);
                        self.matches.push(SearchMatch {
                            line_idx,
                            col_idx,
                            occurrence_in_line: occurrence,
                            match_char_len: q_char_len,
                            byte_offset: byte_idx,
                            byte_len: q_str.len(),
                        });

                        if self.matches.len() >= MAX_SEARCH_MATCHES {
                            break 'outer_text;
                        }
                    }
                }
            }
        }

        self.current_match = 0;
        self.target_jump = self.matches.first().map(|m| (m.line_idx, m.col_idx));
    }

    /// The `evicted` oldest history lines were dropped: shift matches up, drop those that
    /// were on removed lines. Matches are sorted by line (built in history order)
    pub fn rebase_after_eviction(&mut self, evicted: usize) {
        let removed = self.matches.partition_point(|m| m.line_idx < evicted);
        self.matches.drain(..removed);
        for m in &mut self.matches {
            m.line_idx -= evicted;
        }

        self.current_match = self
            .current_match
            .saturating_sub(removed)
            .min(self.matches.len().saturating_sub(1));
        self.target_jump = self
            .target_jump
            .and_then(|(line, col)| line.checked_sub(evicted).map(|line| (line, col)));
    }

    pub fn current_match_position(&self) -> Option<(usize, usize)> {
        self.matches
            .get(self.current_match)
            .map(|m| (m.line_idx, m.col_idx))
    }

    pub fn next_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();

        let pos = (
            self.matches[self.current_match].line_idx,
            self.matches[self.current_match].col_idx,
        );
        self.target_jump = Some(pos);
        Some(pos)
    }

    pub fn prev_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len().saturating_sub(1);
        } else {
            self.current_match -= 1;
        }
        let pos = (
            self.matches[self.current_match].line_idx,
            self.matches[self.current_match].col_idx,
        );
        self.target_jump = Some(pos);
        Some(pos)
    }

    #[inline]
    pub fn matches_count(&self) -> usize {
        self.matches.len()
    }

    #[inline]
    pub fn current_match_display(&self) -> usize {
        if self.matches.is_empty() {
            0
        } else {
            self.current_match + 1
        }
    }

    pub fn toggle(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.target_jump = None;
        }
    }

    pub fn toggle_itself(&mut self) {
        self.toggle(!self.active);
    }

    #[inline]
    pub fn query_str(&self) -> &str {
        self.query.value()
    }

    pub fn toggle_mode(&mut self) {
        self.mode_kind = match self.mode_kind {
            SearchModeKind::Text => SearchModeKind::RawBytes,
            SearchModeKind::RawBytes => SearchModeKind::Text,
        };
    }
}

pub fn parse_raw_bytes_for_view(input: &str, raw_view: RawSideView) -> Option<Vec<u8>> {
    let q = input.trim();
    if q.is_empty() {
        return None;
    }

    let parsed = match raw_view {
        RawSideView::Hex | RawSideView::Off => {
            let clean = q.replace("0x", " ").replace("\\x", " ");
            if clean.contains(' ') {
                clean
                    .split_whitespace()
                    .map(|tok| u8::from_str_radix(tok, 16))
                    .collect::<Result<Vec<u8>, _>>()
                    .ok()
            } else if clean.len() >= 2 && clean.len().is_multiple_of(2) {
                let mut bytes = Vec::with_capacity(clean.len() / 2);
                for i in (0..clean.len()).step_by(2) {
                    if let Ok(b) = u8::from_str_radix(&clean[i..i + 2], 16) {
                        bytes.push(b);
                    } else {
                        return None;
                    }
                }
                Some(bytes)
            } else {
                u8::from_str_radix(&clean, 16).map(|b| vec![b]).ok()
            }
        }
        RawSideView::Octal => {
            let clean = q.replace("0o", " ");
            if clean.contains(' ') {
                clean
                    .split_whitespace()
                    .map(|tok| u8::from_str_radix(tok, 8))
                    .collect::<Result<Vec<u8>, _>>()
                    .ok()
            } else if clean.len().is_multiple_of(3) && clean.len() >= 3 {
                let mut bytes = Vec::with_capacity(clean.len() / 3);
                for i in (0..clean.len()).step_by(3) {
                    if let Ok(b) = u8::from_str_radix(&clean[i..i + 3], 8) {
                        bytes.push(b);
                    } else {
                        return None;
                    }
                }
                Some(bytes)
            } else {
                u8::from_str_radix(&clean, 8).map(|b| vec![b]).ok()
            }
        }
        RawSideView::Binary => {
            let clean = q.replace("0b", " ");
            if clean.contains(' ') {
                clean
                    .split_whitespace()
                    .map(|tok| u8::from_str_radix(tok, 2))
                    .collect::<Result<Vec<u8>, _>>()
                    .ok()
            } else if clean.len().is_multiple_of(8) && clean.len() >= 8 {
                let mut bytes = Vec::with_capacity(clean.len() / 8);
                for i in (0..clean.len()).step_by(8) {
                    if let Ok(b) = u8::from_str_radix(&clean[i..i + 8], 2) {
                        bytes.push(b);
                    } else {
                        return None;
                    }
                }
                Some(bytes)
            } else {
                u8::from_str_radix(&clean, 2).map(|b| vec![b]).ok()
            }
        }
    };

    // a prefix alone ("0x", "0b"...) leaves no byte to search
    parsed.filter(|bytes| !bytes.is_empty())
}

#[inline]
pub fn find_byte_subslice_iter<'a>(
    haystack: &'a [u8],
    needle: &'a [u8],
) -> impl Iterator<Item = usize> + 'a {
    let needle_len = needle.len();
    let is_valid = !needle.is_empty() && needle_len <= haystack.len();

    // windows(0) panics, an empty needle simply matches nothing (is_valid)
    haystack
        .windows(needle_len.max(1))
        .enumerate()
        .filter_map(move |(idx, window)| {
            if is_valid && window == needle {
                Some(idx)
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    //! Query parsing and match finding for text and raw-byte search.

    use super::*;
    use chrono::Local;

    fn history(lines: &[&str]) -> VecDeque<LogEntry> {
        lines
            .iter()
            .map(|t| LogEntry {
                text: t.to_string(),
                timestamp: Local::now(),
            })
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
        let found: Vec<(usize, usize)> =
            s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
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

    /// Locks bug #18: lowercasing non-ASCII shifts the match column.
    /// How: 'İ' grows from 2 to 3 bytes, so col 2 instead of 1.
    #[test]
    fn text_search_non_ascii_lowercase_shifts_col() {
        // FIXME: current behavior, see #18: 'İ' lowercases to 3 bytes (2 in the original), so the
        // offset found in the lowered string maps to col 2 instead of 1
        let mut s = search("x", SearchModeKind::Text);
        s.update_matches_from_entries(&history(&["İx"]), LineDisplayMode::Pretty, RawSideView::Hex);
        assert_eq!(s.matches[0].col_idx, 2);
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
        let found: Vec<(usize, usize)> =
            s.matches.iter().map(|m| (m.line_idx, m.col_idx)).collect();
        assert_eq!(found, [(0, 4), (2, 0), (2, 3)]);
    }
}
