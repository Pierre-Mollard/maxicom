use std::collections::VecDeque;
use tui_input::Input;

use crate::app::data_processing::{
    entry_display_text, entry_display_to_wire_map, map_disp_byte_to_raw, raw_byte_to_display_col,
    safe_byte_to_char_col,
};
use crate::app::selection::LogicalPosition;
use crate::app::types::LogEntry;
use crate::configs::LineDisplayMode;
use crate::configs::settings::RawSideView;
use crate::inputs::vi::find_word_bounds;

pub const MAX_SEARCH_MATCHES: usize = 1000;
/// The searches kept for Up / Down in the bar, as many as the command line's by default
pub const SEARCH_HISTORY_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchModeKind {
    #[default]
    Text,
    RawBytes,
}

/// Where a search starts (MAX-215): `/` from the beginning, `?` from the end
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
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
    pub direction: SearchDirection,
    pub origin: Option<LogicalPosition>,
    pub kept: bool,
    pub exits_copy: bool,
    pub last_query: String,
    /// The history walk: the search shown and its index, and the query typed before
    pub history_walk: Option<(usize, String)>,
    pub history_draft: String,
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
        let backward = self.direction == SearchDirection::Backward;

        self.matches = match self.mode_kind {
            _ if q_str.is_empty() => Vec::new(),
            SearchModeKind::RawBytes => match parse_raw_bytes_for_view(&q_str, raw_view) {
                Some(target_bytes) => collect_matches(
                    history.len(),
                    self.start_line(history.len()),
                    backward,
                    |line_idx| {
                        let entry = &history[line_idx];
                        let needle_len = target_bytes.len();
                        let mut found =
                            find_byte_subslice_iter(entry.wire_bytes(), &target_bytes).peekable();
                        if found.peek().is_none() {
                            return Vec::new();
                        }
                        let display_text = entry_display_text(entry, display_mode);
                        let disp_map = entry_display_to_wire_map(entry, display_mode);

                        found
                            .enumerate()
                            .map(|(occ, raw_byte_idx)| SearchMatch {
                                line_idx,
                                col_idx: raw_byte_to_display_col(
                                    raw_byte_idx,
                                    &disp_map,
                                    &display_text,
                                ),
                                occurrence_in_line: occ,
                                match_char_len: needle_len,
                                byte_offset: raw_byte_idx,
                                byte_len: needle_len,
                            })
                            .collect()
                    },
                ),
                None => Vec::new(),
            },
            SearchModeKind::Text => {
                let q_char_len = q_str.chars().count();

                collect_matches(
                    history.len(),
                    self.start_line(history.len()),
                    backward,
                    |line_idx| {
                        let entry = &history[line_idx];
                        let raw_bytes = entry.wire_bytes();
                        let display_text = entry_display_text(entry, display_mode);
                        let mut found =
                            find_case_insensitive_iter(&display_text, &q_str).peekable();
                        if found.peek().is_none() {
                            return Vec::new();
                        }
                        let disp_map = entry_display_to_wire_map(entry, display_mode);

                        found
                            .enumerate()
                            .map(|(occurrence, (disp_byte_idx, disp_byte_end))| {
                                let raw_byte_offset =
                                    map_disp_byte_to_raw(disp_byte_idx, &disp_map, raw_bytes.len());
                                let raw_byte_end =
                                    map_disp_byte_to_raw(disp_byte_end, &disp_map, raw_bytes.len());
                                SearchMatch {
                                    line_idx,
                                    col_idx: safe_byte_to_char_col(&display_text, disp_byte_idx),
                                    occurrence_in_line: occurrence,
                                    match_char_len: q_char_len,
                                    byte_offset: raw_byte_offset,
                                    byte_len: raw_byte_end
                                        .saturating_sub(raw_byte_offset)
                                        .max(disp_byte_end - disp_byte_idx),
                                }
                            })
                            .collect()
                    },
                )
            }
        };
        self.select_first_match();
    }

    pub fn update_matches_from_strings(&mut self, all_lines: &[String], raw_view: RawSideView) {
        let q_str = self.query.value().to_string();
        let backward = self.direction == SearchDirection::Backward;

        self.matches = match self.mode_kind {
            _ if q_str.is_empty() => Vec::new(),
            SearchModeKind::RawBytes => match parse_raw_bytes_for_view(&q_str, raw_view) {
                Some(target_bytes) => collect_matches(
                    all_lines.len(),
                    self.start_line(all_lines.len()),
                    backward,
                    |line_idx| {
                        let line_text = &all_lines[line_idx];
                        find_byte_subslice_iter(line_text.as_bytes(), &target_bytes)
                            .enumerate()
                            .map(|(occ, byte_idx)| SearchMatch {
                                line_idx,
                                col_idx: safe_byte_to_char_col(line_text, byte_idx),
                                occurrence_in_line: occ,
                                match_char_len: target_bytes.len(),
                                byte_offset: byte_idx,
                                byte_len: target_bytes.len(),
                            })
                            .collect()
                    },
                ),
                None => Vec::new(),
            },
            SearchModeKind::Text => {
                let q_char_len = q_str.chars().count();
                collect_matches(
                    all_lines.len(),
                    self.start_line(all_lines.len()),
                    backward,
                    |line_idx| {
                        let line_text = &all_lines[line_idx];
                        find_case_insensitive_iter(line_text, &q_str)
                            .enumerate()
                            .map(|(occurrence, (byte_idx, byte_end))| SearchMatch {
                                line_idx,
                                col_idx: safe_byte_to_char_col(line_text, byte_idx),
                                occurrence_in_line: occurrence,
                                match_char_len: q_char_len,
                                byte_offset: byte_idx,
                                byte_len: byte_end - byte_idx,
                            })
                            .collect()
                    },
                )
            }
        };
        self.select_first_match();
    }

    /// The line a search reads first (MAX-215): the origin's from copy mode, else the first
    /// line forward and the last one backward
    fn start_line(&self, line_count: usize) -> usize {
        let last = line_count.saturating_sub(1);
        match (self.origin, self.direction) {
            (Some(origin), _) => origin.line_idx.min(last),
            (None, SearchDirection::Forward) => 0,
            (None, SearchDirection::Backward) => last,
        }
    }

    /// The match a new search starts on (MAX-215): the first one after the origin forward, the
    /// last one before it backward, around the history's end past the last one. Without an
    /// origin, the oldest forward and the newest backward
    fn select_first_match(&mut self) {
        let forward = self.direction == SearchDirection::Forward;
        self.current_match = match self.origin {
            None if forward => 0,
            None => self.matches.len().saturating_sub(1),
            Some(origin) => (self.match_from((origin.line_idx, origin.char_idx), forward))
                .map_or(0, |(idx, _)| idx),
        };
        self.target_jump = self.current_match_position();
    }

    /// The first match after `from` forward, the last one before it backward; around the
    /// history's end when nothing is past it, said by the bool
    fn match_from(&self, from: (usize, usize), forward: bool) -> Option<(usize, bool)> {
        let at = |m: &SearchMatch| (m.line_idx, m.col_idx);
        let found = if forward {
            self.matches.iter().position(|m| at(m) > from)
        } else {
            self.matches.iter().rposition(|m| at(m) < from)
        };
        match found {
            Some(idx) => Some((idx, false)),
            None if self.matches.is_empty() => None,
            None if forward => Some((0, true)),
            None => Some((self.matches.len() - 1, true)),
        }
    }

    /// The current match is behind `origin` in the search's direction: reached around the
    /// history's end (MAX-218)
    pub fn went_around(&self, origin: LogicalPosition) -> bool {
        let from = (origin.line_idx, origin.char_idx);
        match (self.current_match_position(), self.direction) {
            (Some(at), SearchDirection::Forward) => at <= from,
            (Some(at), SearchDirection::Backward) => at >= from,
            (None, _) => false,
        }
    }

    /// The match at `from` the current one, else the next one in the search's direction, the
    /// matches found again (MAX-218)
    pub fn select_at(&mut self, from: (usize, usize)) {
        let at = |m: &SearchMatch| (m.line_idx, m.col_idx);
        let forward = self.direction == SearchDirection::Forward;
        let found = self.matches.iter().position(|m| at(m) == from);
        if let Some(idx) = found.or_else(|| self.match_from(from, forward).map(|(idx, _)| idx)) {
            self.current_match = idx;
        }
    }

    /// `n` from the copy cursor at `from`, `N` with `opposite` (MAX-215): the current match
    /// becomes the next one in the search's direction, or the other one. None without a
    /// match, `Some(true)` when it went around the history's end
    pub fn step_from(&mut self, from: (usize, usize), opposite: bool) -> Option<bool> {
        let forward = (self.direction == SearchDirection::Forward) != opposite;
        let (idx, wrapped) = self.match_from(from, forward)?;
        self.current_match = idx;
        self.target_jump = self.current_match_position();
        Some(wrapped)
    }

    /// The matches are drawn while the bar is open, and in copy mode after Enter (MAX-215)
    pub fn shows_matches(&self) -> bool {
        (self.active || self.kept) && !self.matches.is_empty()
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
        if let Some(origin) = &mut self.origin {
            origin.line_idx = origin.line_idx.saturating_sub(evicted);
        }
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

    /// The bar opens empty, the query it had kept as the last one: shown as a placeholder,
    /// searched again by Enter on the empty bar (MAX-218)
    pub fn open_empty(&mut self) {
        let query = std::mem::take(&mut self.query);
        if !query.value().is_empty() {
            self.last_query = query.value().to_string();
        }
    }

    /// Enter on the empty bar: the last query back in it; true when there was one (MAX-218)
    pub fn reuse_last_query(&mut self) -> bool {
        let reuse = self.query.value().is_empty() && !self.last_query.is_empty();
        if reuse {
            self.restore_last_query();
        }
        reuse
    }

    /// Esc: what was typed is dropped for the last query, which `n` / `N` go on with, as in
    /// vi; empty before any search (MAX-218)
    pub fn restore_last_query(&mut self) {
        self.query = Input::new(self.last_query.clone());
    }

    /// Up / Down in the bar (MAX-218): an older (`up`) or newer search of `history`, oldest
    /// first, in place of the query, among those starting with what was typed; past the newest,
    /// what was typed. An edit starts a new walk. False when the query is unchanged
    pub fn walk_history(&mut self, history: &VecDeque<String>, up: bool) -> bool {
        let walking = (self.history_walk.as_ref())
            .filter(|(_, shown)| shown == self.query.value())
            .map(|&(index, _)| index);
        if walking.is_none() {
            if !up {
                return false;
            }
            self.history_draft = self.query.value().to_string();
        }
        let draft = self.history_draft.as_str();
        let matches = |i: &usize| history[*i].starts_with(draft);
        let index = match (walking, up) {
            (None, _) => (0..history.len()).rev().find(matches),
            (Some(i), true) => (0..i).rev().find(matches).or(Some(i)),
            (Some(i), false) => (i + 1..history.len()).find(matches),
        };
        if index.is_none() && walking.is_none() {
            return false;
        }
        let text = match index {
            Some(i) => history[i].clone(),
            None => std::mem::take(&mut self.history_draft),
        };
        self.history_walk = index.map(|i| (i, text.clone()));
        self.query = Input::new(text);
        true
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

/// The word `*` searches (MAX-218): the one under char `cursor`, as a double-click selects it
/// (vi `iw`), or on a blank the next one on the line; its first char and its text
pub fn word_at(text: &str, cursor: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let at = cursor
        + chars
            .get(cursor..)?
            .iter()
            .position(|c| !c.is_whitespace())?;
    let (start, end) = find_word_bounds(text, at, false, true);
    Some((start, chars[start..end].iter().collect()))
}

/// The matches of every line, sorted by line, up to MAX_SEARCH_MATCHES. The lines are read
/// from `start` in the search's direction, around the history's end, so the cap keeps the
/// matches a search reaches first
fn collect_matches(
    line_count: usize,
    start: usize,
    backward: bool,
    mut line_matches: impl FnMut(usize) -> Vec<SearchMatch>,
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    for i in 0..line_count {
        let line_idx = if backward {
            (start + line_count - i) % line_count
        } else {
            (start + i) % line_count
        };
        let mut found = line_matches(line_idx);
        if backward {
            found.reverse();
        }
        found.truncate(MAX_SEARCH_MATCHES - matches.len());
        matches.extend(found);
        if matches.len() == MAX_SEARCH_MATCHES {
            break;
        }
    }
    matches.sort_unstable_by_key(|m| (m.line_idx, m.occurrence_in_line));
    matches
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

/// Byte offsets of every occurrence of `needle`, overlapping ones included ("aa" in "aaa" is
/// found at 0 and 1). An empty needle matches nothing
pub fn find_byte_subslice_iter<'a>(
    haystack: &'a [u8],
    needle: &'a [u8],
) -> impl Iterator<Item = usize> + 'a {
    let finder = memchr::memmem::Finder::new(needle);
    let mut start = 0;
    std::iter::from_fn(move || {
        if needle.is_empty() {
            return None;
        }
        let found = start + finder.find(haystack.get(start..)?)?;
        // next search one byte after this match start, to keep overlapping matches
        start = found + 1;
        Some(found)
    })
}

/// Byte offsets of non-overlapping occurrences of `needle`, ignoring ASCII case.
/// Works on the original bytes (no lowercased copy), so offsets are always valid in `haystack`
/// and nothing is allocated. Non-ASCII chars must match exactly ('É' does not match 'é').
/// An empty needle matches nothing
pub fn find_ascii_case_insensitive_iter<'a>(
    haystack: &'a [u8],
    needle: &'a [u8],
) -> impl Iterator<Item = usize> + 'a {
    let mut start = 0;
    std::iter::from_fn(move || {
        let (&first, rest) = needle.split_first()?;
        let rest_len = rest.len();
        while start + needle.len() <= haystack.len() {
            // candidates: the first needle byte in either case
            let offset = memchr::memchr2(
                first.to_ascii_lowercase(),
                first.to_ascii_uppercase(),
                &haystack[start..haystack.len() - rest_len],
            )?;
            let at = start + offset;
            if haystack[at + 1..at + 1 + rest_len].eq_ignore_ascii_case(rest) {
                start = at + needle.len();
                return Some(at);
            }
            start = at + 1;
        }
        None
    })
}

/// Case-insensitive matches of `needle`, non-overlapping, as start and end bytes in
/// `haystack` (MAX-218). An ASCII query takes the ASCII finder; one with other chars compares
/// char by char, their lowercase forms equal, so `é` finds `É` too
pub fn find_case_insensitive_iter<'a>(
    haystack: &'a str,
    needle: &'a str,
) -> Box<dyn Iterator<Item = (usize, usize)> + 'a> {
    if needle.is_ascii() {
        let found = find_ascii_case_insensitive_iter(haystack.as_bytes(), needle.as_bytes());
        return Box::new(found.map(move |at| (at, at + needle.len())));
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut rest = haystack.char_indices();
    Box::new(std::iter::from_fn(move || {
        loop {
            let (start, _) = rest.clone().next()?;
            let mut chars = rest.clone();
            let mut end = start;
            let matched = needle.chars().all(|n| match chars.next() {
                Some((at, c)) if same(c, n) => {
                    end = at + c.len_utf8();
                    true
                }
                _ => false,
            });
            if matched {
                rest = chars;
                return Some((start, end));
            }
            rest.next();
        }
    }))
}

#[cfg(test)]
#[path = "../tests/app/search.rs"]
mod tests;
