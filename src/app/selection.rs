use crate::inputs::{
    CopyModeAction,
    vi::{find_next_word, find_prev_word, find_prev_word_end, find_word_end},
};

// target need to impl this
pub trait TextSource {
    fn total_lines(&mut self) -> usize;
    fn get_line_text(&mut self, line_idx: usize) -> Option<String>;

    fn get_line_len(&mut self, line_idx: usize) -> usize {
        self.get_line_text(line_idx)
            .map(|s| s.trim_end().chars().count())
            .unwrap_or(0)
    }

    fn view_height(&mut self) -> usize {
        20 // fallback default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogicalPosition {
    pub line_idx: usize,
    pub char_idx: usize,
    pub preferred_col: usize,
}

pub enum CopyActionResult {
    Moved,
    Exited,
    Yanked(String),
    None,
}

#[derive(Debug, Clone, Default)]
pub struct CopyModeState {
    pub active: bool,
    pub cursor: LogicalPosition,
    pub selection_anchor: Option<LogicalPosition>,
}

impl CopyModeState {
    pub fn exit(&mut self) {
        self.active = false;
        self.selection_anchor = None;
    }

    pub fn toggle_selection(&mut self) {
        if self.selection_anchor.is_some() {
            self.selection_anchor = None;
        } else {
            self.selection_anchor = Some(self.cursor);
        }
    }

    pub fn enter_at(&mut self, line_idx: usize, char_idx: usize) {
        self.active = true;
        self.cursor = LogicalPosition {
            line_idx,
            char_idx,
            preferred_col: char_idx,
        };
        self.selection_anchor = None;
    }

    pub fn normalized_selection(&self) -> Option<(LogicalPosition, LogicalPosition)> {
        let anchor = self.selection_anchor?;
        if anchor.line_idx < self.cursor.line_idx
            || (anchor.line_idx == self.cursor.line_idx && anchor.char_idx <= self.cursor.char_idx)
        {
            Some((anchor, self.cursor))
        } else {
            Some((self.cursor, anchor))
        }
    }

    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        let Some((start, end)) = self.normalized_selection() else {
            return false;
        };
        if row < start.line_idx || row > end.line_idx {
            return false;
        }
        if start.line_idx == end.line_idx {
            return col >= start.char_idx && col <= end.char_idx;
        }
        if row == start.line_idx {
            return col >= start.char_idx;
        }
        if row == end.line_idx {
            return col <= end.char_idx;
        }
        true
    }

    pub fn apply_action<S: TextSource>(
        &mut self,
        action: CopyModeAction,
        source: &mut S,
    ) -> CopyActionResult {
        let total_lines = source.total_lines();
        if total_lines == 0 {
            if action == CopyModeAction::Exit {
                self.exit();
                return CopyActionResult::Exited;
            }
            return CopyActionResult::None;
        }

        let max_line = total_lines.saturating_sub(1);
        let view_height = source.view_height().saturating_sub(1).max(1);

        match action {
            CopyModeAction::Exit => {
                self.exit();
                CopyActionResult::Exited
            }
            CopyModeAction::MoveUp => {
                let target_line = self.cursor.line_idx.saturating_sub(1);
                self.cursor.line_idx = target_line;
                let max_char = source.get_line_len(target_line).saturating_sub(1);
                self.cursor.char_idx = self.cursor.preferred_col.min(max_char);
                CopyActionResult::Moved
            }
            CopyModeAction::MoveDown => {
                let target_line = self.cursor.line_idx.saturating_add(1).min(max_line);
                self.cursor.line_idx = target_line;
                let max_char = source.get_line_len(target_line).saturating_sub(1);
                self.cursor.char_idx = self.cursor.preferred_col.min(max_char);
                CopyActionResult::Moved
            }
            CopyModeAction::MoveLeft => {
                self.cursor.char_idx = self.cursor.char_idx.saturating_sub(1);
                self.cursor.preferred_col = self.cursor.char_idx;
                CopyActionResult::Moved
            }
            CopyModeAction::MoveRight => {
                let max_char = source.get_line_len(self.cursor.line_idx).saturating_sub(1);
                self.cursor.char_idx = (self.cursor.char_idx + 1).min(max_char);
                self.cursor.preferred_col = self.cursor.char_idx;
                CopyActionResult::Moved
            }
            CopyModeAction::PageUp => {
                let target_line = self.cursor.line_idx.saturating_sub(view_height);
                self.cursor.line_idx = target_line;
                let max_char = source.get_line_len(target_line).saturating_sub(1);
                self.cursor.char_idx = self.cursor.preferred_col.min(max_char);
                CopyActionResult::Moved
            }
            CopyModeAction::PageDown => {
                let target_line = (self.cursor.line_idx + view_height).min(max_line);
                self.cursor.line_idx = target_line;
                let max_char = source.get_line_len(target_line).saturating_sub(1);
                self.cursor.char_idx = self.cursor.preferred_col.min(max_char);
                CopyActionResult::Moved
            }
            CopyModeAction::StartOfLine => {
                self.cursor.char_idx = 0;
                self.cursor.preferred_col = 0;
                CopyActionResult::Moved
            }
            CopyModeAction::EndOfLine => {
                let last = source.get_line_len(self.cursor.line_idx).saturating_sub(1);
                self.cursor.char_idx = last;
                self.cursor.preferred_col = last;
                CopyActionResult::Moved
            }
            CopyModeAction::JumpToTop => {
                self.cursor.line_idx = 0;
                self.cursor.char_idx = 0;
                self.cursor.preferred_col = 0;
                CopyActionResult::Moved
            }
            CopyModeAction::JumpToBottom => {
                self.cursor.line_idx = max_line;
                let last = source.get_line_len(max_line).saturating_sub(1);
                self.cursor.char_idx = last;
                self.cursor.preferred_col = last;
                CopyActionResult::Moved
            }
            CopyModeAction::NextWord => self.move_word(source, false, find_next_word),
            CopyModeAction::NextBigWord => self.move_word(source, true, find_next_word),
            CopyModeAction::PrevWord => self.move_word(source, false, find_prev_word),
            CopyModeAction::PrevBigWord => self.move_word(source, true, find_prev_word),
            CopyModeAction::WordEnd => self.move_word(source, false, find_word_end),
            CopyModeAction::BigWordEnd => self.move_word(source, true, find_word_end),
            CopyModeAction::PrevWordEnd => self.move_word(source, false, find_prev_word_end),
            CopyModeAction::PrevBigWordEnd => self.move_word(source, true, find_prev_word_end),
            CopyModeAction::Select => {
                self.toggle_selection();
                CopyActionResult::Moved
            }
            CopyModeAction::SelectRange(start, end) => {
                let line_idx = self.cursor.line_idx;
                let max_char = source.get_line_len(line_idx).saturating_sub(1);
                let clamped_start = start.min(max_char);
                let clamped_end = end.saturating_sub(1).min(max_char);

                self.selection_anchor = Some(LogicalPosition {
                    line_idx,
                    char_idx: clamped_start,
                    preferred_col: clamped_start,
                });
                self.cursor.char_idx = clamped_end;
                self.cursor.preferred_col = clamped_end;
                CopyActionResult::Moved
            }
            CopyModeAction::Yank => {
                if let Some((start, end)) = self.normalized_selection() {
                    let mut yanked = String::new();
                    for idx in start.line_idx..=end.line_idx {
                        if let Some(text) = source.get_line_text(idx) {
                            let chars: Vec<char> = text.chars().collect();
                            let len = chars.len();
                            let from = if idx == start.line_idx {
                                start.char_idx.min(len)
                            } else {
                                0
                            };
                            let to = if idx == end.line_idx {
                                (end.char_idx + 1).min(len)
                            } else {
                                len
                            };
                            if from < to {
                                yanked.extend(&chars[from..to]);
                            }
                            if idx != end.line_idx {
                                yanked.push('\n');
                            }
                        }
                    }
                    self.exit();
                    CopyActionResult::Yanked(yanked)
                } else {
                    self.exit();
                    CopyActionResult::Exited
                }
            }

            _ => CopyActionResult::None,
        }
    }

    fn move_word<S, F>(&mut self, source: &mut S, big: bool, finder: F) -> CopyActionResult
    where
        S: TextSource,
        F: Fn(&str, usize, bool) -> usize,
    {
        if let Some(line) = source.get_line_text(self.cursor.line_idx) {
            let max_char = source.get_line_len(self.cursor.line_idx).saturating_sub(1);
            let target = finder(&line, self.cursor.char_idx, big).min(max_char);
            self.cursor.char_idx = target;
            self.cursor.preferred_col = target;
            CopyActionResult::Moved
        } else {
            CopyActionResult::None
        }
    }
}
