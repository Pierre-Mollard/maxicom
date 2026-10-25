use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use crate::{
    app::data_processing::{
        build_display_to_raw_byte_map, map_disp_byte_to_raw, raw_byte_to_display_col,
    },
    configs::{LineDisplayMode, RawSideView, settings::raw_view_to_display_mode},
    inputs::{
        CopyModeAction, find_next_word, find_prev_word, find_prev_word_end, find_word_end,
        vi::find_word_bounds,
    },
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

    /// Display byte -> raw byte map of a line, built from its raw bytes by default (a source
    /// whose raw bytes are not its text's bytes overrides it, #20)
    fn get_line_raw_map(&mut self, _line_idx: usize, raw_bytes: &[u8]) -> Vec<usize> {
        build_display_to_raw_byte_map(raw_bytes, self.display_mode())
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
    pub preferred_col: usize,
    pub char_idx: usize,
    pub byte_idx: usize,
}

/// Which panel the copy cursor moves in: the text, or the raw side panel's bytes (MAX-71).
/// The other panel shows where it is as a ghost cursor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopySide {
    #[default]
    Text,
    Raw,
}

/// What a mouse press selects: the char under it (one click), its word (two quick clicks) or
/// its line (three). A drag after it extends the selection by the same unit (MAX-126)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseUnit {
    #[default]
    Char,
    Word,
    Line,
}

/// The mouse selection a press started: its unit, the first unit selected (first and last
/// char) and whether the release yanks (a drag, or a word or line press)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseSelect {
    pub unit: MouseUnit,
    pub origin: (LogicalPosition, LogicalPosition),
    pub dragged: bool,
}

/// Longest time between two presses of a double or triple click
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// Tells a double or triple click from quick presses on the same cell and side, which crossterm
/// doesn't report. The count goes back to one after three, as in xterm (MAX-126)
#[derive(Debug, Default)]
pub struct ClickCounter {
    last: Option<(Instant, (usize, usize), CopySide)>,
    count: u8,
}

impl ClickCounter {
    /// A press at `(line, column)` on `side`: the unit it selects
    pub fn press(&mut self, now: Instant, at: (usize, usize), side: CopySide) -> MouseUnit {
        let repeat = self.last.is_some_and(|(time, last_at, last_side)| {
            now.duration_since(time) <= MULTI_CLICK && last_at == at && last_side == side
        });
        self.count = if repeat { self.count % 3 + 1 } else { 1 };
        self.last = Some((now, at, side));
        match self.count {
            2 => MouseUnit::Word,
            3 => MouseUnit::Line,
            _ => MouseUnit::Char,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
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
    pub side: CopySide,
    pub mouse_drag: Option<MouseSelect>,
}

impl CopyModeState {
    pub fn exit(&mut self) {
        self.active = false;
        self.selection_anchor = None;
        self.side = CopySide::Text;
        self.mouse_drag = None;
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
            byte_idx: 0,
        };
        self.selection_anchor = None;
        self.side = CopySide::Text;
        self.mouse_drag = None;
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
        let col = |pos: &LogicalPosition| match self.side {
            CopySide::Text => pos.char_idx,
            CopySide::Raw => pos.byte_idx,
        };
        if anchor.line_idx < self.cursor.line_idx
            || (anchor.line_idx == self.cursor.line_idx && col(&anchor) <= col(&self.cursor))
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
            CopyModeAction::MousePress(pos, side, unit) => {
                self.active = true;
                self.side = side;
                self.selection_anchor = None;
                self.move_to(pos, source, max_line);
                // the raw side has no words
                let unit = if side == CopySide::Raw {
                    MouseUnit::Char
                } else {
                    unit
                };
                let origin = unit_bounds(source, self.cursor, unit);
                let dragged = unit != MouseUnit::Char;
                if dragged {
                    self.selection_anchor = Some(origin.0);
                    self.cursor = origin.1;
                }
                self.mouse_drag = Some(MouseSelect {
                    unit,
                    origin,
                    dragged,
                });
                return CopyActionResult::Moved;
            }
            CopyModeAction::MouseDrag(pos, _) => {
                let Some(mut select) = self.mouse_drag else {
                    return CopyActionResult::None;
                };
                if select.unit == MouseUnit::Char && !select.dragged {
                    self.selection_anchor = Some(self.cursor);
                }
                select.dragged = true;
                self.mouse_drag = Some(select);
                self.move_to(pos, source, max_line);
                if select.unit != MouseUnit::Char {
                    // whole units from the first one to the one under the pointer
                    let (start, end) = unit_bounds(source, self.cursor, select.unit);
                    let (origin_start, origin_end) = select.origin;
                    if (start.line_idx, start.char_idx)
                        < (origin_start.line_idx, origin_start.char_idx)
                    {
                        self.selection_anchor = Some(origin_end);
                        self.cursor = start;
                    } else {
                        self.selection_anchor = Some(origin_start);
                        self.cursor = end;
                    }
                }
                return CopyActionResult::Moved;
            }
            CopyModeAction::MouseRelease => {
                return match self.mouse_drag.take() {
                    Some(MouseSelect { dragged: true, .. }) => {
                        self.apply_action(CopyModeAction::Yank, source)
                    }
                    _ => CopyActionResult::None,
                };
            }
            CopyModeAction::ToggleSide => return self.toggle_side(source),
            _ => {}
        }
        if self.side == CopySide::Raw {
            return self.apply_raw_action(action, source, max_line, view_height);
        }

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
                    byte_idx: 0,
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

                            let disp_map = source.get_line_raw_map(idx, &raw_bytes);
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

    /// Switches the cursor between the text and the raw panel (no raw panel: nothing). The
    /// cursor and the selection anchor are converted: a char goes to its first byte, a byte to
    /// the char showing it (MAX-71)
    fn toggle_side<S: TextSource>(&mut self, source: &mut S) -> CopyActionResult {
        if source.raw_view() == RawSideView::Off {
            return CopyActionResult::None;
        }
        match self.side {
            CopySide::Text => {
                self.side = CopySide::Raw;
                for pos in std::iter::once(&mut self.cursor).chain(&mut self.selection_anchor) {
                    pos.byte_idx = char_to_byte(source, pos.line_idx, pos.char_idx);
                    pos.preferred_col = pos.byte_idx;
                }
            }
            CopySide::Raw => {
                // the chars are kept in sync on the raw side
                self.side = CopySide::Text;
                for pos in std::iter::once(&mut self.cursor).chain(&mut self.selection_anchor) {
                    pos.preferred_col = pos.char_idx;
                }
            }
        }
        CopyActionResult::Moved
    }

    /// Copy mode actions on the raw side: the cursor moves over the line's raw bytes, then its
    /// char follows (the ghost in the text). Word motions and text objects don't apply
    fn apply_raw_action<S: TextSource>(
        &mut self,
        action: CopyModeAction,
        source: &mut S,
        max_line: usize,
        view_height: usize,
    ) -> CopyActionResult {
        let mut last_byte = |line_idx| {
            source
                .get_line_raw_bytes(line_idx)
                .map_or(0, |bytes| bytes.len().saturating_sub(1))
        };
        let cursor = &mut self.cursor;
        let vertical = |cursor: &mut LogicalPosition, line_idx, last: usize| {
            cursor.line_idx = line_idx;
            cursor.byte_idx = cursor.preferred_col.min(last);
        };
        match action {
            CopyModeAction::MoveLeft => {
                if cursor.byte_idx > 0 {
                    cursor.byte_idx -= 1;
                } else if cursor.line_idx > 0 {
                    cursor.line_idx -= 1;
                    cursor.byte_idx = last_byte(cursor.line_idx);
                }
                cursor.preferred_col = cursor.byte_idx;
            }
            CopyModeAction::MoveRight => {
                if cursor.byte_idx < last_byte(cursor.line_idx) {
                    cursor.byte_idx += 1;
                } else if cursor.line_idx < max_line {
                    cursor.line_idx += 1;
                    cursor.byte_idx = 0;
                }
                cursor.preferred_col = cursor.byte_idx;
            }
            CopyModeAction::MoveUp => {
                let line_idx = cursor.line_idx.saturating_sub(1);
                vertical(cursor, line_idx, last_byte(line_idx));
            }
            CopyModeAction::MoveDown => {
                let line_idx = (cursor.line_idx + 1).min(max_line);
                vertical(cursor, line_idx, last_byte(line_idx));
            }
            CopyModeAction::PageUp => {
                let line_idx = cursor.line_idx.saturating_sub(view_height);
                vertical(cursor, line_idx, last_byte(line_idx));
            }
            CopyModeAction::PageDown => {
                let line_idx = (cursor.line_idx + view_height).min(max_line);
                vertical(cursor, line_idx, last_byte(line_idx));
            }
            CopyModeAction::StartOfLine => {
                cursor.byte_idx = 0;
                cursor.preferred_col = 0;
            }
            CopyModeAction::EndOfLine => {
                cursor.byte_idx = last_byte(cursor.line_idx);
                cursor.preferred_col = cursor.byte_idx;
            }
            CopyModeAction::JumpToTop => {
                cursor.line_idx = 0;
                cursor.byte_idx = 0;
                cursor.preferred_col = 0;
            }
            CopyModeAction::JumpToBottom => {
                cursor.line_idx = max_line;
                cursor.byte_idx = last_byte(max_line);
                cursor.preferred_col = cursor.byte_idx;
            }
            CopyModeAction::Select => self.toggle_selection(),
            CopyModeAction::Exit => {
                self.exit();
                return CopyActionResult::Exited;
            }
            CopyModeAction::Yank | CopyModeAction::YankRaw => return self.yank_raw_bytes(source),
            _ => return CopyActionResult::None,
        }
        self.cursor.char_idx = byte_to_char(source, self.cursor.line_idx, self.cursor.byte_idx);
        CopyActionResult::Moved
    }

    /// Puts the cursor on a position the mouse gave, on the current side: a char of the text,
    /// or a byte with the char showing it as its ghost (MAX-113)
    fn move_to<S: TextSource>(&mut self, pos: LogicalPosition, source: &mut S, max_line: usize) {
        let line_idx = pos.line_idx.min(max_line);
        self.cursor = match self.side {
            CopySide::Text => {
                let char_idx = pos
                    .char_idx
                    .min(source.get_line_len(line_idx).saturating_sub(1));
                LogicalPosition {
                    line_idx,
                    char_idx,
                    preferred_col: char_idx,
                    byte_idx: 0,
                }
            }
            CopySide::Raw => LogicalPosition {
                line_idx,
                char_idx: byte_to_char(source, line_idx, pos.byte_idx),
                preferred_col: pos.byte_idx,
                byte_idx: pos.byte_idx,
            },
        };
    }

    /// The selected bytes (raw side), in the raw panel's format, one line per source line
    fn yank_raw_bytes<S: TextSource>(&mut self, source: &mut S) -> CopyActionResult {
        let Some((start, end)) = self.normalized_selection() else {
            self.exit();
            return CopyActionResult::Exited;
        };
        let display_mode = raw_view_to_display_mode(source.raw_view());
        let mut raw_lines = Vec::new();
        for idx in start.line_idx..=end.line_idx {
            let raw_bytes = source.get_line_raw_bytes(idx).unwrap_or_default();
            let from = if idx == start.line_idx {
                start.byte_idx
            } else {
                0
            };
            let to = if idx == end.line_idx {
                end.byte_idx + 1
            } else {
                raw_bytes.len()
            };
            let slice = &raw_bytes[from.min(raw_bytes.len())..to.min(raw_bytes.len())];
            raw_lines.push(format_raw_bytes_to_plain_string(slice, display_mode));
        }
        self.exit();
        CopyActionResult::Yanked(raw_lines.join("\n"))
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

/// The first and last char of the word or line holding a text position (MAX-126). A word is a vi
/// `iw` word; on whitespace it is the run of whitespace, as xterm selects
fn unit_bounds<S: TextSource>(
    source: &mut S,
    pos: LogicalPosition,
    unit: MouseUnit,
) -> (LogicalPosition, LogicalPosition) {
    let last = source.get_line_len(pos.line_idx).saturating_sub(1);
    let (start, end) = match unit {
        MouseUnit::Char => (pos.char_idx, pos.char_idx),
        MouseUnit::Line => (0, last),
        MouseUnit::Word => {
            let text = source.get_line_text(pos.line_idx).unwrap_or_default();
            let chars: Vec<char> = text.chars().collect();
            match chars.get(pos.char_idx) {
                Some(c) if c.is_whitespace() => {
                    let start = chars[..pos.char_idx]
                        .iter()
                        .rposition(|c| !c.is_whitespace())
                        .map_or(0, |i| i + 1);
                    let end = chars[pos.char_idx..]
                        .iter()
                        .position(|c| !c.is_whitespace())
                        .map_or(chars.len(), |i| pos.char_idx + i);
                    (start, end - 1)
                }
                Some(_) => {
                    let (start, end) = find_word_bounds(&text, pos.char_idx, false, true);
                    (start, end - 1)
                }
                None => (pos.char_idx, pos.char_idx),
            }
        }
    };
    let at = |char_idx: usize| LogicalPosition {
        line_idx: pos.line_idx,
        char_idx: char_idx.min(last),
        preferred_col: char_idx.min(last),
        byte_idx: 0,
    };
    (at(start), at(end))
}

/// The first raw byte of char `char_idx` of a line (the last byte when past the end)
pub fn char_to_byte<S: TextSource>(source: &mut S, line_idx: usize, char_idx: usize) -> usize {
    let text = source.get_line_text(line_idx).unwrap_or_default();
    let raw_bytes = source.get_line_raw_bytes(line_idx).unwrap_or_default();
    let map = source.get_line_raw_map(line_idx, &raw_bytes);
    let disp_byte = text
        .char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(byte, _)| byte);
    map_disp_byte_to_raw(disp_byte, &map, raw_bytes.len()).min(raw_bytes.len().saturating_sub(1))
}

/// The char of a line showing raw byte `byte_idx`: the char holding it, or for a byte not shown
/// (escape sequence, control byte) the next char shown, or the last char for the line ending
pub fn byte_to_char<S: TextSource>(source: &mut S, line_idx: usize, byte_idx: usize) -> usize {
    let text = source.get_line_text(line_idx).unwrap_or_default();
    let raw_bytes = source.get_line_raw_bytes(line_idx).unwrap_or_default();
    let map = source.get_line_raw_map(line_idx, &raw_bytes);
    let max_char = source.get_line_len(line_idx).saturating_sub(1);
    raw_byte_to_display_col(byte_idx, &map, &text).min(max_char)
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
            byte_idx: 0,
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

    /// A source whose raw bytes differ from its text, like a received colored line: text "ok",
    /// bytes `ESC[32m` `ok` `ESC[0m` `\r\n` (13 bytes, "o" is byte 5), then a plain line
    struct ColoredSource {
        raw_view: RawSideView,
    }

    impl TextSource for ColoredSource {
        fn total_lines(&mut self) -> usize {
            2
        }
        fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
            ["ok", "abc"].get(line_idx).map(|s| s.to_string())
        }
        fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
            let raw: [&[u8]; 2] = [b"\x1b[32mok\x1b[0m\r\n", b"abc\r\n"];
            raw.get(line_idx).map(|bytes| bytes.to_vec())
        }
        fn raw_view(&mut self) -> RawSideView {
            self.raw_view
        }
    }

    fn colored() -> ColoredSource {
        ColoredSource {
            raw_view: RawSideView::Hex,
        }
    }

    fn at(state: &CopyModeState) -> (usize, usize, usize) {
        (
            state.cursor.line_idx,
            state.cursor.char_idx,
            state.cursor.byte_idx,
        )
    }

    /// MAX-71: the toggle moves the cursor to the raw side on its char's first byte and back;
    /// with no raw panel it does nothing.
    #[test]
    fn toggle_side_converts_the_cursor() {
        let mut state = CopyModeState::default();
        state.enter_at(0, 1);
        state.apply_action(CopyModeAction::ToggleSide, &mut colored());
        assert_eq!(state.side, CopySide::Raw);
        assert_eq!(at(&state), (0, 1, 6));
        state.apply_action(CopyModeAction::ToggleSide, &mut colored());
        assert_eq!(state.side, CopySide::Text);
        assert_eq!(at(&state), (0, 1, 6));

        let mut no_panel = ColoredSource {
            raw_view: RawSideView::Off,
        };
        state.apply_action(CopyModeAction::ToggleSide, &mut no_panel);
        assert_eq!(state.side, CopySide::Text);
    }

    /// MAX-71: on the raw side the cursor steps over every byte, the escape sequences and the
    /// line ending included; its char (the ghost in the text) is the char holding the byte, the
    /// next char shown for a byte not shown, the last char for the line ending.
    #[test]
    fn raw_cursor_steps_over_every_byte() {
        let mut state = CopyModeState::default();
        state.enter_at(0, 0);
        let src = &mut colored();
        state.apply_action(CopyModeAction::ToggleSide, src);
        assert_eq!(at(&state), (0, 0, 5));
        state.apply_action(CopyModeAction::MoveLeft, src);
        // "m" of ESC[32m: not shown, the ghost is on "o"
        assert_eq!(at(&state), (0, 0, 4));
        state.apply_action(CopyModeAction::EndOfLine, src);
        // the "\n": the ghost is on the last char "k"
        assert_eq!(at(&state), (0, 1, 12));
        state.apply_action(CopyModeAction::MoveRight, src);
        assert_eq!(at(&state), (1, 0, 0));
        state.apply_action(CopyModeAction::MoveLeft, src);
        assert_eq!(at(&state), (0, 1, 12));

        // vertical moves keep the byte column, clamped to the line's bytes
        state.apply_action(CopyModeAction::MoveDown, src);
        assert_eq!(at(&state), (1, 2, 4));
        state.apply_action(CopyModeAction::MoveUp, src);
        assert_eq!(at(&state), (0, 1, 12));
        // word motions don't apply to bytes
        assert!(matches!(
            state.apply_action(CopyModeAction::NextWord, src),
            CopyActionResult::None
        ));
    }

    /// MAX-71: a selection made on the raw side yanks its bytes in the panel's format, `y` and
    /// `Y` alike, whatever the direction it was made in.
    /// How: select from byte 4 back to byte 0 (the whole ESC[32m, no char of its own).
    #[test]
    fn raw_selection_yanks_its_bytes() {
        for yank in [CopyModeAction::Yank, CopyModeAction::YankRaw] {
            let mut state = CopyModeState::default();
            state.enter_at(0, 0);
            let src = &mut colored();
            state.apply_action(CopyModeAction::ToggleSide, src);
            state.apply_action(CopyModeAction::MoveLeft, src);
            state.apply_action(CopyModeAction::Select, src);
            state.apply_action(CopyModeAction::StartOfLine, src);
            match state.apply_action(yank, src) {
                CopyActionResult::Yanked(text) => assert_eq!(text, "1B 5B 33 32 6D"),
                _ => panic!("expected a yank"),
            }
            assert!(!state.active);
            assert_eq!(state.side, CopySide::Text);
        }
    }

    /// MAX-71: the selection anchor is converted with the cursor, both ways.
    /// How: select "ok" in text, to the raw side (the anchor on byte 5), back to text.
    #[test]
    fn toggle_side_converts_the_anchor() {
        let mut state = CopyModeState::default();
        state.enter_at(0, 0);
        let src = &mut colored();
        state.apply_action(CopyModeAction::Select, src);
        state.apply_action(CopyModeAction::MoveRight, src);
        state.apply_action(CopyModeAction::ToggleSide, src);
        let anchor = state.selection_anchor.unwrap();
        assert_eq!((anchor.char_idx, anchor.byte_idx), (0, 5));
        assert_eq!(at(&state), (0, 1, 6));
        state.apply_action(CopyModeAction::ToggleSide, src);
        match state.apply_action(CopyModeAction::Yank, src) {
            CopyActionResult::Yanked(text) => assert_eq!(text, "ok"),
            _ => panic!("expected a yank"),
        }
    }

    fn words() -> MockSource {
        MockSource {
            lines: vec!["foo.bar  baz", "next line here"],
            raw_view: RawSideView::Hex,
        }
    }

    fn press(state: &mut CopyModeState, src: &mut MockSource, at: (usize, usize), unit: MouseUnit) {
        let action = CopyModeAction::MousePress(pos(at.0, at.1), CopySide::Text, unit);
        state.apply_action(action, src);
    }

    fn release_yank(state: &mut CopyModeState, src: &mut MockSource) -> String {
        match state.apply_action(CopyModeAction::MouseRelease, src) {
            CopyActionResult::Yanked(text) => text,
            _ => panic!("expected a yank"),
        }
    }

    /// MAX-126: a word press selects the vi `iw` word under it, or the run of whitespace; the
    /// release yanks it without a drag.
    /// How: presses on a word char, on punctuation and on whitespace of "foo.bar  baz".
    #[test]
    fn mouse_word_press_selects_the_word() {
        let mut src = words();
        let mut state = CopyModeState::default();
        for (col, word) in [(1, "foo"), (3, "."), (6, "bar"), (7, "  "), (11, "baz")] {
            press(&mut state, &mut src, (0, col), MouseUnit::Word);
            assert_eq!(release_yank(&mut state, &mut src), word, "col {col}");
            assert!(!state.active);
        }
    }

    /// MAX-126: a line press selects the whole line.
    /// How: press in the middle of line 1, release.
    #[test]
    fn mouse_line_press_selects_the_line() {
        let mut src = words();
        let mut state = CopyModeState::default();
        press(&mut state, &mut src, (1, 6), MouseUnit::Line);
        assert_eq!(release_yank(&mut state, &mut src), "next line here");
    }

    /// MAX-126: a drag after a word press extends by whole words both ways, across lines, and
    /// always keeps the first word.
    /// How: press on "bar", drag into "baz", into "foo", then to "line" on the next line.
    #[test]
    fn mouse_word_drag_extends_by_words() {
        let mut src = words();
        let mut state = CopyModeState::default();
        let drag = |state: &mut CopyModeState, src: &mut MockSource, at: (usize, usize)| {
            state.apply_action(CopyModeAction::MouseDrag(pos(at.0, at.1), (0, 0)), src);
        };

        press(&mut state, &mut src, (0, 5), MouseUnit::Word);
        drag(&mut state, &mut src, (0, 10));
        assert_eq!(release_yank(&mut state, &mut src), "bar  baz");

        press(&mut state, &mut src, (0, 5), MouseUnit::Word);
        drag(&mut state, &mut src, (0, 10));
        drag(&mut state, &mut src, (0, 1));
        assert_eq!(release_yank(&mut state, &mut src), "foo.bar");

        press(&mut state, &mut src, (0, 5), MouseUnit::Word);
        drag(&mut state, &mut src, (1, 6));
        assert_eq!(release_yank(&mut state, &mut src), "bar  baz\nnext line");
    }

    /// MAX-126: a drag after a line press extends by whole lines, upward too; the raw side
    /// keeps char presses.
    /// How: line press on line 1 dragged to line 0; a word press on the raw side yanks nothing.
    #[test]
    fn mouse_line_drag_and_raw_side() {
        let mut src = words();
        let mut state = CopyModeState::default();
        press(&mut state, &mut src, (1, 6), MouseUnit::Line);
        state.apply_action(CopyModeAction::MouseDrag(pos(0, 4), (0, 0)), &mut src);
        assert_eq!(
            release_yank(&mut state, &mut src),
            "foo.bar  baz\nnext line here"
        );

        let action = CopyModeAction::MousePress(pos(0, 1), CopySide::Raw, MouseUnit::Word);
        state.apply_action(action, &mut src);
        assert!(state.selection_anchor.is_none());
        assert!(matches!(
            state.apply_action(CopyModeAction::MouseRelease, &mut src),
            CopyActionResult::None
        ));
    }

    /// MAX-126: quick presses on the same cell and side count 2 (word) and 3 (line), then start
    /// over; a slow press, another cell, the other side or a reset count 1.
    /// How: made-up instants around the 400 ms limit.
    #[test]
    fn click_counter_counts_quick_presses() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let text = CopySide::Text;
        let mut clicks = ClickCounter::default();
        assert_eq!(clicks.press(ms(0), (0, 1), text), MouseUnit::Char);
        assert_eq!(clicks.press(ms(300), (0, 1), text), MouseUnit::Word);
        assert_eq!(clicks.press(ms(600), (0, 1), text), MouseUnit::Line);
        assert_eq!(clicks.press(ms(900), (0, 1), text), MouseUnit::Char);

        assert_eq!(clicks.press(ms(1400), (0, 1), text), MouseUnit::Char);
        assert_eq!(clicks.press(ms(1500), (0, 2), text), MouseUnit::Char);
        assert_eq!(
            clicks.press(ms(1600), (0, 2), CopySide::Raw),
            MouseUnit::Char
        );
        clicks.reset();
        assert_eq!(
            clicks.press(ms(1700), (0, 2), CopySide::Raw),
            MouseUnit::Char
        );
        assert_eq!(
            clicks.press(ms(1800), (0, 2), CopySide::Raw),
            MouseUnit::Word
        );
    }
}
