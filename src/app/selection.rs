use std::io::{self, Write};

use crate::{
    app::data_processing::{build_display_to_raw_byte_map, map_disp_byte_to_raw},
    configs::{LineDisplayMode, RawSideView, settings::raw_view_to_display_mode},
    inputs::{CopyModeAction, find_next_word, find_prev_word, find_prev_word_end, find_word_end},
    ui::raw_side::format_raw_bytes_to_plain_string,
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

    fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
        self.get_line_text(line_idx).map(|s| s.into_bytes()) // fallback default
    }

    fn raw_view(&mut self) -> RawSideView;
    fn display_mode(&mut self) -> LineDisplayMode {
        LineDisplayMode::Pretty
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

    /// The `evicted` oldest lines of the source were dropped: keep cursor and anchor on the
    /// same lines, clamped to the new first line if theirs is gone
    pub fn rebase_after_eviction(&mut self, evicted: usize) {
        self.cursor.line_idx = self.cursor.line_idx.saturating_sub(evicted);
        if let Some(anchor) = &mut self.selection_anchor {
            anchor.line_idx = anchor.line_idx.saturating_sub(evicted);
        }
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
                if self.cursor.char_idx > 0 {
                    self.cursor.char_idx -= 1;
                } else if self.cursor.line_idx > 0 {
                    self.cursor.line_idx -= 1;
                    let prev_line_len = source.get_line_len(self.cursor.line_idx);
                    self.cursor.char_idx = prev_line_len.saturating_sub(1);
                }
                self.cursor.preferred_col = self.cursor.char_idx;
                CopyActionResult::Moved
            }

            CopyModeAction::MoveRight => {
                let current_line_len = source.get_line_len(self.cursor.line_idx);
                let max_char = current_line_len.saturating_sub(1);
                if self.cursor.char_idx < max_char {
                    self.cursor.char_idx += 1;
                } else if self.cursor.line_idx < max_line {
                    self.cursor.line_idx += 1;
                    self.cursor.char_idx = 0;
                }
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
            CopyModeAction::NextWord | CopyModeAction::NextBigWord => {
                let big = matches!(action, CopyModeAction::NextBigWord);
                let current_line_len = source.get_line_len(self.cursor.line_idx);
                let max_char = current_line_len.saturating_sub(1);
                if self.cursor.char_idx >= max_char && self.cursor.line_idx < max_line {
                    self.cursor.line_idx += 1;
                    self.cursor.char_idx = 0;
                    self.cursor.preferred_col = 0;
                    CopyActionResult::Moved
                } else {
                    self.move_word(source, big, find_next_word)
                }
            }
            CopyModeAction::PrevWord | CopyModeAction::PrevBigWord => {
                let big = matches!(action, CopyModeAction::PrevBigWord);
                if self.cursor.char_idx == 0 && self.cursor.line_idx > 0 {
                    self.cursor.line_idx -= 1;
                    let prev_line_len = source.get_line_len(self.cursor.line_idx);
                    self.cursor.char_idx = prev_line_len.saturating_sub(1);
                    self.cursor.preferred_col = self.cursor.char_idx;
                    self.move_word(source, big, find_prev_word)
                } else {
                    self.move_word(source, big, find_prev_word)
                }
            }
            CopyModeAction::WordEnd | CopyModeAction::BigWordEnd => {
                let big = matches!(action, CopyModeAction::BigWordEnd);
                let current_line_len = source.get_line_len(self.cursor.line_idx);
                let max_char = current_line_len.saturating_sub(1);
                if self.cursor.char_idx >= max_char && self.cursor.line_idx < max_line {
                    self.cursor.line_idx += 1;
                    self.cursor.char_idx = 0;
                    self.cursor.preferred_col = 0;
                    CopyActionResult::Moved
                } else {
                    self.move_word(source, big, find_word_end)
                }
            }
            CopyModeAction::PrevWordEnd | CopyModeAction::PrevBigWordEnd => {
                let big = matches!(action, CopyModeAction::PrevBigWordEnd);
                if self.cursor.char_idx == 0 && self.cursor.line_idx > 0 {
                    self.cursor.line_idx -= 1;
                    let prev_line_len = source.get_line_len(self.cursor.line_idx);
                    self.cursor.char_idx = prev_line_len.saturating_sub(1);
                    self.cursor.preferred_col = self.cursor.char_idx;
                    self.move_word(source, big, find_prev_word_end)
                } else {
                    self.move_word(source, big, find_prev_word_end)
                }
            }
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
            CopyModeAction::YankRaw => {
                if let Some((start, end)) = self.normalized_selection()
                    && source.raw_view() != RawSideView::Off
                {
                    let mut raw_lines = Vec::new();
                    let display_mode = raw_view_to_display_mode(source.raw_view());

                    for idx in start.line_idx..=end.line_idx {
                        if let Some(raw_bytes) = source.get_line_raw_bytes(idx) {
                            let text = source.get_line_text(idx).unwrap_or_default();
                            let text_chars: Vec<char> = text.chars().collect();
                            let len = text_chars.len();

                            let from_char = if idx == start.line_idx {
                                start.char_idx.min(len)
                            } else {
                                0
                            };
                            let to_char = if idx == end.line_idx {
                                (end.char_idx + 1).min(len)
                            } else {
                                len
                            };

                            let mut disp_char_to_byte = Vec::with_capacity(len + 1);
                            for (b_idx, _) in text.char_indices() {
                                disp_char_to_byte.push(b_idx);
                            }
                            disp_char_to_byte.push(text.len());

                            let disp_from_byte =
                                disp_char_to_byte.get(from_char).copied().unwrap_or(0);
                            let disp_to_byte = disp_char_to_byte
                                .get(to_char)
                                .copied()
                                .unwrap_or(text.len());

                            let disp_map =
                                build_display_to_raw_byte_map(&raw_bytes, source.display_mode());
                            let raw_from_byte =
                                map_disp_byte_to_raw(disp_from_byte, &disp_map, raw_bytes.len());
                            let raw_to_byte = if to_char >= len {
                                raw_bytes.len()
                            } else {
                                map_disp_byte_to_raw(disp_to_byte, &disp_map, raw_bytes.len())
                            };

                            let slice = &raw_bytes[raw_from_byte.min(raw_bytes.len())
                                ..raw_to_byte.min(raw_bytes.len())];
                            let formatted = format_raw_bytes_to_plain_string(slice, display_mode);
                            raw_lines.push(formatted);
                        }
                    }

                    self.exit();
                    CopyActionResult::Yanked(raw_lines.join("\n"))
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

/// copies text via ANSI OSC 52 escape code
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

#[cfg(test)]
mod tests {
    //! Copy mode: selection ordering, hit testing, yanking and cursor moves.

    use super::*;

    struct MockSource {
        lines: Vec<&'static str>,
        raw_view: RawSideView,
    }

    impl TextSource for MockSource {
        fn total_lines(&mut self) -> usize {
            self.lines.len()
        }
        fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
            self.lines.get(line_idx).map(|s| s.to_string())
        }
        fn raw_view(&mut self) -> RawSideView {
            self.raw_view
        }
    }

    fn source() -> MockSource {
        MockSource {
            lines: vec!["hello world", "second line"],
            raw_view: RawSideView::Hex,
        }
    }

    fn pos(line_idx: usize, char_idx: usize) -> LogicalPosition {
        LogicalPosition {
            line_idx,
            char_idx,
            preferred_col: char_idx,
        }
    }

    /// The selection is returned as (start, end), whatever the drag direction.
    /// How: anchor placed after the cursor.
    #[test]
    fn normalized_selection_orders_anchor_and_cursor() {
        let mut state = CopyModeState::default();
        state.enter_at(1, 3);
        state.selection_anchor = Some(pos(0, 6));
        let (start, end) = state.normalized_selection().unwrap();
        assert_eq!((start.line_idx, start.char_idx), (0, 6));
        assert_eq!((end.line_idx, end.char_idx), (1, 3));
    }

    /// Hit testing across a multi-line selection, edges included.
    /// How: checks cells inside, on and outside the bounds.
    #[test]
    fn is_selected_multi_line() {
        let mut state = CopyModeState::default();
        state.enter_at(2, 1);
        state.selection_anchor = Some(pos(0, 4));
        assert!(!state.is_selected(0, 3));
        assert!(state.is_selected(0, 4));
        assert!(state.is_selected(1, 0));
        assert!(state.is_selected(2, 1));
        assert!(!state.is_selected(2, 2));
        assert!(!state.is_selected(3, 0));
    }

    /// Yank copies the selected text across lines and leaves copy mode.
    /// How: mock source, selection from line 0 to 1.
    #[test]
    fn yank_multi_line_selection() {
        let mut state = CopyModeState::default();
        state.enter_at(1, 5);
        state.selection_anchor = Some(pos(0, 6));
        match state.apply_action(CopyModeAction::Yank, &mut source()) {
            CopyActionResult::Yanked(text) => assert_eq!(text, "world\nsecond"),
            _ => panic!("expected a yank"),
        }
        assert!(!state.active);
    }

    /// Raw yank copies the selected bytes in the raw view format.
    /// How: selecting "he" in Hex gives "68 65".
    #[test]
    fn yank_raw_formats_selected_bytes() {
        let mut state = CopyModeState::default();
        state.enter_at(0, 1);
        state.selection_anchor = Some(pos(0, 0));
        match state.apply_action(CopyModeAction::YankRaw, &mut source()) {
            CopyActionResult::Yanked(text) => assert_eq!(text, "68 65"),
            _ => panic!("expected a raw yank"),
        }
    }

    /// Yank with nothing selected just exits.
    /// How: checks the `Exited` result.
    #[test]
    fn yank_without_selection_exits() {
        let mut state = CopyModeState::default();
        state.enter_at(0, 0);
        assert!(matches!(
            state.apply_action(CopyModeAction::Yank, &mut source()),
            CopyActionResult::Exited
        ));
    }

    /// Eviction moves cursor and anchor up, clamping to line 0 when their line is gone.
    /// How: anchor on line 1, cursor on line 4, evict 2 lines.
    #[test]
    fn rebase_after_eviction_moves_cursor_and_anchor() {
        let mut state = CopyModeState::default();
        state.enter_at(4, 3);
        state.selection_anchor = Some(pos(1, 2));
        state.rebase_after_eviction(2);
        assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (2, 3));
        let anchor = state.selection_anchor.unwrap();
        assert_eq!((anchor.line_idx, anchor.char_idx), (0, 2));
    }

    /// Moving up/down clamps to short lines but returns to the preferred column.
    /// How: long, short, long lines.
    #[test]
    fn vertical_moves_keep_preferred_col() {
        let mut src = MockSource {
            lines: vec!["long line here", "ab", "another long one"],
            raw_view: RawSideView::Hex,
        };
        let mut state = CopyModeState::default();
        state.enter_at(0, 10);
        state.apply_action(CopyModeAction::MoveDown, &mut src);
        assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (1, 1));
        state.apply_action(CopyModeAction::MoveDown, &mut src);
        assert_eq!((state.cursor.line_idx, state.cursor.char_idx), (2, 10));
    }
}
