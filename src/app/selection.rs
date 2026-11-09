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

/// How the view follows a copy cursor move (MAX-143): by the fewest rows when the cursor steps
/// out of it, centered when it jumps (`gg`, `G`, a search match) or wasn't shown before the move
/// (the view was scrolled away from it)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveal {
    Edge,
    Center,
}

impl Reveal {
    pub fn for_move(action: &CopyModeAction, cursor_shown: bool) -> Self {
        let jump = matches!(
            action,
            CopyModeAction::JumpToTop | CopyModeAction::JumpToBottom
        );
        if jump || !cursor_shown {
            Self::Center
        } else {
            Self::Edge
        }
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
    pub line_select: bool,
}

/// A column past the end of any line, as a line-wise selection's end: the selection's users
/// clamp it to the line, and it leaves room for their `+ 1` (MAX-127)
const PAST_LINE_END: usize = usize::MAX / 2;

impl CopyModeState {
    pub fn exit(&mut self) {
        self.active = false;
        self.selection_anchor = None;
        self.side = CopySide::Text;
        self.mouse_drag = None;
        self.line_select = false;
    }

    /// `v` (char-wise) or `V` (line-wise), as in vim: starts a selection at the cursor, the
    /// same key again ends it, the other one switches the selection to its kind
    pub fn toggle_selection(&mut self, lines: bool) {
        if self.selection_anchor.is_some() && self.line_select == lines {
            self.selection_anchor = None;
            self.line_select = false;
        } else {
            self.selection_anchor.get_or_insert(self.cursor);
            self.line_select = lines;
        }
    }

    /// Moves the cursor to a search match, the selection kept
    pub fn jump_to(&mut self, line_idx: usize, char_idx: usize) {
        self.cursor.line_idx = line_idx;
        self.cursor.char_idx = char_idx;
        self.cursor.preferred_col = char_idx;
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
        self.line_select = false;
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
        let (mut start, mut end) = if anchor.line_idx < self.cursor.line_idx
            || (anchor.line_idx == self.cursor.line_idx && col(&anchor) <= col(&self.cursor))
        {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        if self.line_select {
            (start.char_idx, start.byte_idx) = (0, 0);
            (end.char_idx, end.byte_idx) = (PAST_LINE_END, PAST_LINE_END);
        }
        Some((start, end))
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
                self.line_select = false;
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
            CopyModeAction::Select | CopyModeAction::SelectLine => {
                self.toggle_selection(action == CopyModeAction::SelectLine);
                CopyActionResult::Moved
            }
            CopyModeAction::SelectRange(start, end) => {
                let line_idx = self.cursor.line_idx;
                let max_char = source.get_line_len(line_idx).saturating_sub(1);
                let clamped_start = start.min(max_char);
                let clamped_end = end.saturating_sub(1).min(max_char);
                self.line_select = false;

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
                            // from the line's start: its leading escape sequences too, as the
                            // panel highlights them (MAX-127)
                            let raw_from_byte = if from_char == 0 {
                                0
                            } else {
                                map_disp_byte_to_raw(disp_from_byte, &disp_map, raw_bytes.len())
                            };
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
            CopyModeAction::Select | CopyModeAction::SelectLine => {
                self.toggle_selection(action == CopyModeAction::SelectLine)
            }
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
        // below the last line is the end of it, as in an editor (MAX-138)
        let past_end = pos.line_idx > max_line;
        self.cursor = match self.side {
            CopySide::Text => {
                let last = source.get_line_len(line_idx).saturating_sub(1);
                let char_idx = if past_end {
                    last
                } else {
                    pos.char_idx.min(last)
                };
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
#[path = "../tests/app/selection.rs"]
mod tests;
