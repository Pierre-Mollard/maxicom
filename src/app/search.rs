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

pub const MAX_SEARCH_MATCHES: usize = 1000;

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
                let needle = q_str.as_bytes();
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
                            find_ascii_case_insensitive_iter(display_text.as_bytes(), needle)
                                .peekable();
                        if found.peek().is_none() {
                            return Vec::new();
                        }
                        let disp_map = entry_display_to_wire_map(entry, display_mode);

                        found
                            .enumerate()
                            .map(|(occurrence, disp_byte_idx)| {
                                let raw_byte_offset =
                                    map_disp_byte_to_raw(disp_byte_idx, &disp_map, raw_bytes.len());
                                let raw_byte_end = map_disp_byte_to_raw(
                                    disp_byte_idx + needle.len(),
                                    &disp_map,
                                    raw_bytes.len(),
                                );
                                SearchMatch {
                                    line_idx,
                                    col_idx: safe_byte_to_char_col(&display_text, disp_byte_idx),
                                    occurrence_in_line: occurrence,
                                    match_char_len: q_char_len,
                                    byte_offset: raw_byte_offset,
                                    byte_len: raw_byte_end
                                        .saturating_sub(raw_byte_offset)
                                        .max(needle.len()),
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
                        find_ascii_case_insensitive_iter(line_text.as_bytes(), q_str.as_bytes())
                            .enumerate()
                            .map(|(occurrence, byte_idx)| SearchMatch {
                                line_idx,
                                col_idx: safe_byte_to_char_col(line_text, byte_idx),
                                occurrence_in_line: occurrence,
                                match_char_len: q_char_len,
                                byte_offset: byte_idx,
                                byte_len: q_str.len(),
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

#[cfg(test)]
#[path = "../tests/app/search.rs"]
mod tests;
