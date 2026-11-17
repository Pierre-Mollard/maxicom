//! Line mode's raw side panel: each history line next to its bytes, scrolling with the history.
//! Copy mode's cursor is the text's own there: a position holds both its char and its byte, the
//! other side showing it as a ghost (MAX-71), so the state stays in [`CopyModeState`] and only
//! what the raw side does with it is here

use crate::{
    app::{
        data_processing::{map_disp_byte_to_raw, raw_byte_to_display_col},
        selection::{
            CopyActionResult, CopyModeState, CopySide, LogicalPosition, MouseSelect, MouseUnit,
            TextSource,
        },
    },
    configs::{RawSideView, settings::raw_view_to_display_mode},
    inputs::CopyModeAction,
    raw_side::RawSideScroller,
    ui::raw_side::format_raw_bytes_to_plain_string,
};

/// The panel's pan; its rows are the history's
#[derive(Debug, Default)]
pub struct LineBytes {
    pan: RawSideScroller,
}

impl LineBytes {
    pub fn pan_left(&mut self, bytes: usize, view: RawSideView) {
        self.pan.pan_left(bytes, view);
    }

    pub fn pan_right(&mut self, bytes: usize, view: RawSideView) {
        self.pan.pan_right(bytes, view);
    }

    /// The lines once drawn `overflow` columns wider than the panel
    pub fn set_pan_limit(&mut self, overflow: usize, view: RawSideView) {
        self.pan.set_limit(overflow, view);
    }

    /// The columns panned, within the limit
    pub fn pan_x(&self) -> usize {
        self.pan.pan_x.min(self.pan.max_pan_x)
    }

    /// The pan centered on byte `byte` of a line of `bytes` bytes, in a panel `cols` wide
    pub fn center_pan_on(&mut self, view: RawSideView, cols: usize, byte: usize, bytes: usize) {
        self.pan.center_on_byte(view, cols, byte, bytes);
    }

    /// The pan showing byte `byte` of a line of `bytes` bytes, when it's off the panel
    pub fn pan_to(&mut self, view: RawSideView, cols: usize, byte: usize, bytes: usize) {
        self.pan.sync_with_cursor(view, cols, byte, bytes);
    }
}

/// Switches the cursor between the text and the raw panel (no raw panel: nothing). The
/// cursor and the selection anchor are converted: a char goes to its first byte, a byte to
/// the char showing it (MAX-71)
pub fn toggle_side<S: TextSource>(state: &mut CopyModeState, source: &mut S) -> CopyActionResult {
    if source.raw_view() == RawSideView::Off {
        return CopyActionResult::None;
    }
    match state.side {
        CopySide::Text => {
            state.side = CopySide::Raw;
            for pos in std::iter::once(&mut state.cursor).chain(&mut state.selection_anchor) {
                pos.byte_idx = char_to_byte(source, pos.line_idx, pos.char_idx);
                pos.preferred_col = pos.byte_idx;
            }
        }
        CopySide::Raw => {
            // the chars are kept in sync on the raw side
            state.side = CopySide::Text;
            for pos in std::iter::once(&mut state.cursor).chain(&mut state.selection_anchor) {
                pos.preferred_col = pos.char_idx;
            }
        }
    }
    CopyActionResult::Moved
}

/// Copy mode actions on the raw side, the mouse's included: the cursor moves over the line's
/// raw bytes, then its char follows (the ghost in the text). Word motions and text objects
/// don't apply, and a press selects a byte whatever the clicks
pub fn copy_action<S: TextSource>(
    state: &mut CopyModeState,
    action: CopyModeAction,
    source: &mut S,
) -> CopyActionResult {
    let total_lines = source.total_lines();
    if total_lines == 0 {
        if action == CopyModeAction::Exit {
            state.exit();
            return CopyActionResult::Exited;
        }
        return CopyActionResult::None;
    }
    let max_line = total_lines - 1;
    let view_height = source.view_height().saturating_sub(1).max(1);

    match action {
        CopyModeAction::MousePress(pos, _, _) => {
            state.active = true;
            state.side = CopySide::Raw;
            state.selection_anchor = None;
            state.line_select = false;
            state.cursor = byte_position(source, pos, max_line);
            state.mouse_drag = Some(MouseSelect {
                unit: MouseUnit::Char,
                origin: (state.cursor, state.cursor),
                dragged: false,
            });
            return CopyActionResult::Moved;
        }
        CopyModeAction::MouseDrag(pos, _) => {
            let Some(select) = state.mouse_drag.as_mut() else {
                return CopyActionResult::None;
            };
            if !select.dragged {
                select.dragged = true;
                state.selection_anchor = Some(state.cursor);
            }
            state.cursor = byte_position(source, pos, max_line);
            return CopyActionResult::Moved;
        }
        CopyModeAction::MouseRelease => {
            return match state.mouse_drag.take() {
                Some(MouseSelect { dragged: true, .. }) => yank_bytes(state, source),
                _ => CopyActionResult::None,
            };
        }
        _ => {}
    }

    let mut last_byte = |line_idx| {
        source
            .get_line_raw_bytes(line_idx)
            .map_or(0, |bytes| bytes.len().saturating_sub(1))
    };
    let cursor = &mut state.cursor;
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
            state.toggle_selection(action == CopyModeAction::SelectLine)
        }
        CopyModeAction::Exit => {
            state.exit();
            return CopyActionResult::Exited;
        }
        CopyModeAction::Yank | CopyModeAction::YankRaw => return yank_bytes(state, source),
        _ => return CopyActionResult::None,
    }
    state.cursor.char_idx = byte_to_char(source, state.cursor.line_idx, state.cursor.byte_idx);
    CopyActionResult::Moved
}

/// The byte the mouse gave, with the char showing it as its ghost (MAX-113); below the last
/// line is the end of it, as in an editor (MAX-138)
fn byte_position<S: TextSource>(
    source: &mut S,
    pos: LogicalPosition,
    max_line: usize,
) -> LogicalPosition {
    let line_idx = pos.line_idx.min(max_line);
    LogicalPosition {
        line_idx,
        char_idx: byte_to_char(source, line_idx, pos.byte_idx),
        preferred_col: pos.byte_idx,
        byte_idx: pos.byte_idx,
    }
}

/// The selected bytes, in the raw panel's format, one line per source line
fn yank_bytes<S: TextSource>(state: &mut CopyModeState, source: &mut S) -> CopyActionResult {
    let Some((start, end)) = state.normalized_selection() else {
        state.exit();
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
    state.exit();
    CopyActionResult::Yanked(raw_lines.join("\n"))
}

/// The first raw byte of char `char_idx` of a line (the last byte when past the end)
fn char_to_byte<S: TextSource>(source: &mut S, line_idx: usize, char_idx: usize) -> usize {
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
fn byte_to_char<S: TextSource>(source: &mut S, line_idx: usize, byte_idx: usize) -> usize {
    let text = source.get_line_text(line_idx).unwrap_or_default();
    let raw_bytes = source.get_line_raw_bytes(line_idx).unwrap_or_default();
    let map = source.get_line_raw_map(line_idx, &raw_bytes);
    let max_char = source.get_line_len(line_idx).saturating_sub(1);
    raw_byte_to_display_col(byte_idx, &map, &text).min(max_char)
}

#[cfg(test)]
#[path = "../tests/raw_side/line.rs"]
mod tests;
