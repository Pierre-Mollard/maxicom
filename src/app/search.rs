use std::collections::VecDeque;
use std::io::{self, Write};

use tui_input::Input;

use crate::app::types::LogEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_idx: usize,
    pub col_idx: usize,
    pub occurrence_in_line: usize,
}

#[derive(Default)]
pub struct SearchState {
    pub active: bool,
    pub query: Input,
    pub matches: Vec<SearchMatch>,
    pub current_match: usize,
    pub target_jump_index: Option<usize>,
}

impl SearchState {
    pub fn update_matches_from_entries(&mut self, history: &VecDeque<LogEntry>) {
        self.matches.clear();
        if self.query_str().is_empty() {
            self.current_match = 0;
            return;
        }

        let q = self.query_str().to_lowercase();
        for (line_idx, entry) in history.iter().enumerate() {
            let line_lower = entry.text.to_lowercase();
            for (occurrence, (byte_idx, _)) in line_lower.match_indices(&q).enumerate() {
                let col_idx = entry.text[..byte_idx].chars().count();
                self.matches.push(SearchMatch {
                    line_idx,
                    col_idx,
                    occurrence_in_line: occurrence,
                });
            }
        }

        if self.current_match >= self.matches.len() {
            self.current_match = self.matches.len().saturating_sub(1);
        }
    }

    pub fn update_matches_from_strings(&mut self, all_lines: &[String]) {
        self.matches.clear();
        if self.query_str().is_empty() {
            self.current_match = 0;
            return;
        }

        let q = self.query_str().to_lowercase();

        for (line_idx, line_text) in all_lines.iter().enumerate() {
            let line_lower = line_text.to_lowercase();
            for (occurrence, (byte_idx, _)) in line_lower.match_indices(&q).enumerate() {
                let col_idx = line_text[..byte_idx].chars().count();
                self.matches.push(SearchMatch {
                    line_idx,
                    col_idx,
                    occurrence_in_line: occurrence,
                });
            }
        }

        if self.current_match >= self.matches.len() {
            self.current_match = self.matches.len().saturating_sub(1);
        }
    }

    pub fn current_match_position(&self) -> Option<(usize, usize)> {
        self.matches
            .get(self.current_match)
            .map(|m| (m.line_idx, m.col_idx))
    }

    pub fn next_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        Some(self.matches[self.current_match].line_idx)
    }

    pub fn prev_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len().saturating_sub(1);
        } else {
            self.current_match -= 1;
        }
        Some(self.matches[self.current_match].line_idx)
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
            self.target_jump_index = None;
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
}

/// Copies text to the host system clipboard via ANSI OSC 52 escape code.
pub fn copy_via_osc52(text: &str) -> io::Result<()> {
    // simple standard base64 encoding (RFC 4648)
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        encoded.push(B64_CHARS[(b0 >> 2) as usize] as char);
        encoded.push(B64_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(B64_CHARS[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(B64_CHARS[(b2 & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    // write \x1b]52;c;<base64>\x07 to stdout
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}
