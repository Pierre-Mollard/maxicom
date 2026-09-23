use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler};

use crate::types::LogEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_idx: usize,
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
            for (occurrence, _) in line_lower.match_indices(&q).enumerate() {
                self.matches.push(SearchMatch {
                    line_idx,
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
            for (occurrence, _) in line_lower.match_indices(&q).enumerate() {
                self.matches.push(SearchMatch {
                    line_idx,
                    occurrence_in_line: occurrence,
                });
            }
        }

        if self.current_match >= self.matches.len() {
            self.current_match = self.matches.len().saturating_sub(1);
        }
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

// TODO: implement a vim motion mode (and generalize for any Input in project?)
pub fn dispatch_input_key(input: &mut Input, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('e') => {
                input.handle(InputRequest::GoToEnd);
                return;
            }
            KeyCode::Char('u') => {
                input.reset();
                return;
            }
            KeyCode::Char('w') => {
                input.handle(InputRequest::DeletePrevWord);
                return;
            }
            KeyCode::Char('d') => {
                input.handle(InputRequest::DeleteNextChar);
                return;
            }
            KeyCode::Char('k') => {
                let cursor = input.cursor();
                let text = input.value()[..cursor].to_string();
                *input = Input::new(text).with_cursor(cursor);
                return;
            }
            KeyCode::Left => {
                input.handle(InputRequest::GoToPrevWord);
                return;
            }
            KeyCode::Right => {
                input.handle(InputRequest::GoToNextWord);
                return;
            }
            _ => {}
        }
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('b') | KeyCode::Left => {
                input.handle(InputRequest::GoToPrevWord);
                return;
            }
            KeyCode::Char('f') | KeyCode::Right => {
                input.handle(InputRequest::GoToNextWord);
                return;
            }
            KeyCode::Backspace | KeyCode::Char('d') => {
                input.handle(InputRequest::DeleteNextWord);
                return;
            }
            _ => {}
        }
    }

    input.handle_event(&crossterm::event::Event::Key(key));
}
