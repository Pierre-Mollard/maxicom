/// Terminal widget custom from term-ui
// TODO: show symbol/detection when new content arrived i scroll area if not live
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Clear, Widget},
};
use tui_term::widget::{Cell, Screen};

use crate::config::CursorConfig;

#[derive(Debug, Clone)]
pub struct MaxicomCursor {
    pub show: bool,
    pub unfocused_symbol: String,
    pub unfocused_style: Style,
    pub focused_style: Style,
}

impl Default for MaxicomCursor {
    fn default() -> Self {
        Self {
            show: true,
            unfocused_symbol: String::from("█"),
            unfocused_style: Style::default(),
            focused_style: Style::default(),
        }
    }
}

impl MaxicomCursor {
    pub fn from_config(config: &CursorConfig) -> Self {
        Self {
            show: true,
            unfocused_symbol: config.unfocused_glyph.clone(),
            unfocused_style: config.unfocused_style,
            focused_style: config.focused_style,
        }
    }
}

pub struct MaxicomTerminal<'a, S> {
    screen: &'a S,
    x_offset: u16,
    y_offset: u16,
    block: Option<Block<'a>>,
    style: Option<Style>,
    cursor: MaxicomCursor,
    empty_filling_char: &'a str,
    focused: bool,
}

impl<'a, S: Screen> MaxicomTerminal<'a, S> {
    /// Creates a new instance of `MaxicomTerminal`.
    #[inline]
    #[must_use]
    pub fn new(screen: &'a S) -> Self {
        MaxicomTerminal {
            screen,
            x_offset: 0,
            y_offset: 0,
            block: None,
            style: None,
            cursor: MaxicomCursor::default(),
            empty_filling_char: " ",
            focused: true,
        }
    }

    #[inline]
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    #[inline]
    #[must_use]
    pub fn cursor(mut self, cursor: MaxicomCursor) -> Self {
        self.cursor = cursor;
        self
    }

    #[inline]
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    #[inline]
    #[must_use]
    pub const fn x_offset(mut self, x_offset: u16) -> Self {
        self.x_offset = x_offset;
        self
    }

    #[inline]
    #[must_use]
    pub const fn y_offset(mut self, y_offset: u16) -> Self {
        self.y_offset = y_offset;
        self
    }

    #[inline]
    pub const fn filling_char(mut self, char: &'a str) -> Self {
        self.empty_filling_char = char;
        self
    }

    #[inline]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    #[inline]
    #[must_use]
    pub const fn screen(&self) -> &S {
        self.screen
    }
}

impl<S: Screen> Widget for &MaxicomTerminal<'_, S> {
    #[inline]
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let area = self.block.as_ref().map_or(area, |b| {
            let inner_area = b.inner(area);
            b.clone().render(area, buf);
            inner_area
        });
        handle(self, area, buf);
    }
}

impl<S: Screen> Widget for MaxicomTerminal<'_, S> {
    #[inline]
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// Draw the [`Screen`] to the [`Buffer`],
/// area is the designated area that the consumer provides
fn handle<S: Screen>(term: &MaxicomTerminal<S>, area: Rect, buf: &mut Buffer) {
    let cols = area.width;
    let rows = area.height;
    let col_start = area.x;
    let row_start = area.y;
    let area_cols = area.width + area.x;
    let area_rows = area.height + area.y;
    let screen = term.screen();

    // The [`Screen`] is made out of rows of cells
    for row in 0..rows {
        for col in 0..cols {
            let buf_col = col + col_start;
            let buf_row = row + row_start;

            if buf_row >= area_rows || buf_col >= area_cols {
                // Skip writing outside the area
                continue;
            }

            let source_col = col.saturating_add(term.x_offset);
            let source_row = row.saturating_add(term.y_offset);
            let cell = &mut buf[(buf_col, buf_row)];
            if let Some(screen_cell) = screen.cell(source_row, source_col) {
                screen_cell.apply(cell);
            } else {
                // Outside the terminal canvas (viewer area padding)
                cell.set_symbol(term.empty_filling_char);
                cell.set_style(Style::default().fg(Color::DarkGray));
            }
        }
    }

    // fake cursor when not focused
    if !term.focused && !screen.hide_cursor() && term.cursor.show {
        let (c_row, c_col) = screen.cursor_position();

        let Some(local_col) = c_col.checked_sub(term.x_offset) else {
            // Cursor left of viewport so not visible
            return;
        };

        let Some(local_row) = c_row.checked_sub(term.y_offset) else {
            // Cursor top of viewport so not visible
            return;
        };

        if local_row >= area.height || local_col >= area.width {
            // Cursor above or below or right of viewport so not visible
            return;
        }

        let dst_x = area.x + local_col;
        let dst_y = area.y + local_row;
        let dst = &mut buf[(dst_x, dst_y)];

        if let Some(source_cell) = screen.cell(c_row, c_col) {
            if source_cell.has_contents() {
                dst.set_style(term.cursor.focused_style);
            } else {
                dst.set_symbol(&term.cursor.unfocused_symbol);
                dst.set_style(term.cursor.unfocused_style);
            }
        }
    }
}
