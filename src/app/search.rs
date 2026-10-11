use std::collections::VecDeque;
use tui_input::Input;

use crate::app::data_processing::displayable_line_string;
use crate::app::types::LogEntry;
use crate::configs::LineDisplayMode;
use crate::configs::settings::RawSideView;

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
impl SearchState {
    pub fn update_matches_from_entries(
        &mut self,
        history: &VecDeque<LogEntry>,
        display_mode: LineDisplayMode,
        raw_view: RawSideView,
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
                if let Some(target_bytes) = parse_raw_bytes_for_view(&q_str, raw_view) {
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

        self.matches = new_matches;
        self.current_match = 0;

        if let Some(m) = self.matches.first() {
            self.target_jump = Some((m.line_idx, m.col_idx));
        } else {
            self.target_jump = None;
        }
    }

    pub fn update_matches_from_strings(&mut self, all_lines: &[String], raw_view: RawSideView) {
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
                if let Some(target_bytes) = parse_raw_bytes_for_view(&q_str, raw_view) {
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

// NOTE: check this
pub fn parse_raw_bytes_for_view(input: &str, raw_view: RawSideView) -> Option<Vec<u8>> {
    let q = input.trim();
    if q.is_empty() {
        return None;
    }

    match raw_view {
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
                // Unspaced 3-digit octal tokens: e.g. "155141" -> [0o155, 0o141]
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
                // Unspaced 8-bit bytes: e.g. "0110110101100001"
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
    }
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
