use std::collections::VecDeque;
use tui_input::Input;

use crate::app::data_processing::displayable_line_string;
use crate::app::types::LogEntry;
use crate::configs::LineDisplayMode;

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

// NOTE: check this
// TODO: search with speical 0x, 0b, 0o (not working)
impl SearchState {
    pub fn update_matches_from_entries(
        &mut self,
        history: &VecDeque<LogEntry>,
        display_mode: LineDisplayMode,
    ) {
        let q_str = self.query_str().to_string();
        if q_str.is_empty() {
            self.matches.clear();
            self.current_match = 0;
            self.target_jump = None;
            return;
        }

        let mut new_matches = Vec::new();

        match self.mode_kind {
            SearchModeKind::RawBytes => {
                if let Some(target_bytes) = parse_raw_bytes(&q_str) {
                    let needle_len = target_bytes.len();
                    for (line_idx, entry) in history.iter().enumerate() {
                        let display_text = displayable_line_string(&entry.text, display_mode);
                        let disp_bytes = display_text.as_bytes();

                        let matches_in_disp = find_byte_subslice(disp_bytes, &target_bytes);
                        let raw_matches = find_byte_subslice(entry.text.as_bytes(), &target_bytes);

                        for (occ, &byte_idx) in matches_in_disp.iter().enumerate() {
                            let col_idx = safe_byte_to_char_col(&display_text, byte_idx);
                            let raw_offset = raw_matches.get(occ).copied().unwrap_or(byte_idx);

                            new_matches.push(SearchMatch {
                                line_idx,
                                col_idx,
                                occurrence_in_line: occ,
                                match_char_len: needle_len,
                                byte_offset: raw_offset,
                                byte_len: needle_len,
                            });
                        }
                    }
                }
            }
            SearchModeKind::Text => {
                let q_lower = q_str.to_lowercase();
                let q_char_len = q_str.chars().count();
                for (line_idx, entry) in history.iter().enumerate() {
                    let display_text = displayable_line_string(&entry.text, display_mode);
                    let line_lower = display_text.to_lowercase();
                    for (occurrence, (byte_idx, _)) in
                        line_lower.match_indices(&q_lower).enumerate()
                    {
                        let col_idx = safe_byte_to_char_col(&display_text, byte_idx);
                        new_matches.push(SearchMatch {
                            line_idx,
                            col_idx,
                            occurrence_in_line: occurrence,
                            match_char_len: q_char_len,
                            byte_offset: byte_idx,
                            byte_len: q_str.len(),
                        });
                    }
                }
            }
        }

        // Single write to self.matches avoids all borrow conflicts
        self.matches = new_matches;
        self.current_match = 0;

        if let Some(m) = self.matches.first() {
            self.target_jump = Some((m.line_idx, m.col_idx));
        } else {
            self.target_jump = None;
        }
    }

    pub fn update_matches_from_strings(&mut self, all_lines: &[String]) {
        let q_str = self.query_str().to_string();
        if q_str.is_empty() {
            self.matches.clear();
            self.current_match = 0;
            self.target_jump = None;
            return;
        }

        let mut new_matches = Vec::new();

        match self.mode_kind {
            SearchModeKind::RawBytes => {
                if let Some(target_bytes) = parse_raw_bytes(&q_str) {
                    let needle_len = target_bytes.len();
                    for (line_idx, line_text) in all_lines.iter().enumerate() {
                        let bytes = line_text.as_bytes();
                        for (occ, byte_idx) in find_byte_subslice(bytes, &target_bytes)
                            .into_iter()
                            .enumerate()
                        {
                            let col_idx = safe_byte_to_char_col(line_text, byte_idx);
                            new_matches.push(SearchMatch {
                                line_idx,
                                col_idx,
                                occurrence_in_line: occ,
                                match_char_len: needle_len,
                                byte_offset: byte_idx,
                                byte_len: needle_len,
                            });
                        }
                    }
                }
            }
            SearchModeKind::Text => {
                let q_lower = q_str.to_lowercase();
                let q_char_len = q_str.chars().count();
                for (line_idx, line_text) in all_lines.iter().enumerate() {
                    let line_lower = line_text.to_lowercase();
                    for (occurrence, (byte_idx, _)) in
                        line_lower.match_indices(&q_lower).enumerate()
                    {
                        let col_idx = safe_byte_to_char_col(line_text, byte_idx);
                        new_matches.push(SearchMatch {
                            line_idx,
                            col_idx,
                            occurrence_in_line: occurrence,
                            match_char_len: q_char_len,
                            byte_offset: byte_idx,
                            byte_len: q_str.len(),
                        });
                    }
                }
            }
        }

        self.matches = new_matches;
        self.current_match = 0;

        if let Some(m) = self.matches.first() {
            self.target_jump = Some((m.line_idx, m.col_idx));
        } else {
            self.target_jump = None;
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

    pub fn matches_count(&self) -> usize {
        self.matches.len()
    }

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

    pub fn active_occurrence_for_line(&self, line_idx: usize) -> Option<usize> {
        if let Some(m) = self.matches.get(self.current_match)
            && m.line_idx == line_idx
        {
            return Some(m.occurrence_in_line);
        }
        None
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

pub fn parse_raw_bytes(input: &str) -> Option<Vec<u8>> {
    let q = input.trim();
    if q.is_empty() {
        return None;
    }

    if q.starts_with("0x") || q.starts_with("\\x") {
        let clean = q.replace("0x", " ").replace("\\x", " ");
        return clean
            .split_whitespace()
            .map(|tok| u8::from_str_radix(tok, 16))
            .collect::<Result<Vec<u8>, _>>()
            .ok();
    }

    if q.starts_with("0b") {
        let clean = q.replace("0b", " ");
        return clean
            .split_whitespace()
            .map(|tok| u8::from_str_radix(tok, 2))
            .collect::<Result<Vec<u8>, _>>()
            .ok();
    }

    if q.starts_with("0o") {
        let clean = q.replace("0o", " ");
        return clean
            .split_whitespace()
            .map(|tok| u8::from_str_radix(tok, 8))
            .collect::<Result<Vec<u8>, _>>()
            .ok();
    }

    if q.contains(' ') {
        let tokens: Vec<&str> = q.split_whitespace().collect();
        if tokens
            .iter()
            .all(|t| t.len() <= 2 && t.chars().all(|c| c.is_ascii_hexdigit()))
        {
            let res: Result<Vec<u8>, _> =
                tokens.iter().map(|t| u8::from_str_radix(t, 16)).collect();
            if let Ok(b) = res
                && !b.is_empty()
            {
                return Some(b);
            }
        }
    }

    if q.len() >= 2 && q.len() % 2 == 0 && q.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut bytes = Vec::with_capacity(q.len() / 2);
        for i in (0..q.len()).step_by(2) {
            if let Ok(b) = u8::from_str_radix(&q[i..i + 2], 16) {
                bytes.push(b);
            } else {
                return None;
            }
        }
        return Some(bytes);
    }

    None
}

#[inline]
pub fn find_byte_subslice(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(idx, window)| if window == needle { Some(idx) } else { None })
        .collect()
}

#[inline]
pub fn safe_byte_to_char_col(s: &str, byte_idx: usize) -> usize {
    s.char_indices()
        .take_while(|&(b_idx, _)| b_idx < byte_idx)
        .count()
}
