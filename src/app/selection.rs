#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisualPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CopyModeState {
    pub active: bool,
    pub cursor: VisualPosition,
    pub selection_anchor: Option<VisualPosition>,
    pub pending_yank: bool,
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

    pub fn enter_at(&mut self, row: usize, col: usize) {
        self.active = true;
        self.cursor = VisualPosition { row, col };
        self.selection_anchor = None;
    }

    pub fn normalized_selection(&self) -> Option<(VisualPosition, VisualPosition)> {
        let anchor = self.selection_anchor?;
        if anchor.row < self.cursor.row
            || (anchor.row == self.cursor.row && anchor.col <= self.cursor.col)
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
        if row < start.row || row > end.row {
            return false;
        }
        if start.row == end.row {
            return col >= start.col && col <= end.col;
        }
        if row == start.row {
            return col >= start.col;
        }
        if row == end.row {
            return col <= end.col;
        }
        true
    }

    pub fn extract_selected_from_buffer(
        &self,
        buf: &ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        paragraph_y_offset: usize,
        line_x_offset: usize,
    ) -> Option<String> {
        let (start, end) = self.normalized_selection()?;
        let mut out = String::new();

        for visual_row in start.row..=end.row {
            if visual_row < paragraph_y_offset {
                continue;
            }
            let viewer_row = visual_row - paragraph_y_offset;
            if viewer_row >= area.height as usize {
                break;
            }

            let col_start = if visual_row == start.row {
                start.col
            } else {
                0
            };
            let col_end = if visual_row == end.row {
                end.col
            } else {
                area.width as usize
            };

            let mut line_chars = String::new();
            for visual_col in col_start..=col_end {
                if visual_col < line_x_offset {
                    continue;
                }
                let viewer_col = visual_col - line_x_offset;
                if viewer_col >= area.width as usize {
                    break;
                }

                let cell = &buf[(area.x + viewer_col as u16, area.y + viewer_row as u16)];
                line_chars.push_str(cell.symbol());
            }

            out.push_str(line_chars.trim_end());
            if visual_row != end.row {
                out.push('\n');
            }
        }

        if out.trim().is_empty() {
            None
        } else {
            Some(out)
        }
    }
}
