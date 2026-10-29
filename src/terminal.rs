/// Terminal widget drawing a vt100 screen, custom from tui-term (which pins vt100 0.16, #29)
use std::num::NonZeroU16;

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Clear, Widget},
};

use crate::app::term_answers::{Answer, answer_control, answer_csi, answer_escape};

/// What the parser reports besides the screen. `evicted` counts the main screen rows vt100 drops
/// from the top of scrollback (`on_scroll` is only called for those, not for rows entering it):
/// with the change of `scrollback_len` it gives how many rows a chunk pushed into scrollback
/// (#13). `answers` are the replies to the remote's queries, in order, for the settings to send
/// or drop (MAX-151)
#[derive(Default)]
pub struct ParserEvents {
    pub evicted: usize,
    pub answers: Vec<Answer>,
}

impl vt100::Callbacks for ParserEvents {
    fn on_scroll(&mut self, _: vt100::capture::RowContents<'_>, alternate_screen: bool) {
        if !alternate_screen {
            self.evicted += 1;
        }
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if let Some(answer) = answer_csi(screen, i1, i2, params, c) {
            self.answers.push(answer);
        }
    }

    fn unhandled_control(&mut self, _: &mut vt100::Screen, b: u8) {
        if let Some(answer) = answer_control(b) {
            self.answers.push(answer);
        }
    }

    fn unhandled_escape(&mut self, _: &mut vt100::Screen, i1: Option<u8>, i2: Option<u8>, b: u8) {
        if let Some(answer) = answer_escape(i1, i2, b) {
            self.answers.push(answer);
        }
    }
}

pub type TerminalParser = vt100::Parser<ParserEvents>;

pub fn new_parser(rows: u16, cols: u16, max_history_lines: usize) -> TerminalParser {
    TerminalParser::new_with_callbacks(
        vt100_dim(rows),
        vt100_dim(cols),
        max_history_lines,
        ParserEvents::default(),
    )
}

/// vt100 sizes are `NonZeroU16`, maxicom uses `u16`: 0 (never a valid size) becomes 1
pub fn vt100_dim(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

/// Screen size as `(rows, cols)` in `u16`, like the rest of maxicom
pub fn vt100_size(screen: &vt100::Screen) -> (u16, u16) {
    let (rows, cols) = screen.size();
    (rows.get(), cols.get())
}

#[derive(Debug, Clone)]
pub struct MaxicomCursor<'a> {
    pub show: bool,
    pub unfocused_symbol: &'a str,
    pub unfocused_style: Style,
}

impl Default for MaxicomCursor<'_> {
    fn default() -> Self {
        Self {
            show: true,
            unfocused_symbol: "█",
            unfocused_style: Style::default(),
        }
    }
}

impl<'a> MaxicomCursor<'a> {
    pub fn from_config(unfocused: Style, symbol: &'a str) -> Self {
        Self {
            show: true,
            unfocused_symbol: symbol,
            unfocused_style: unfocused,
        }
    }
}

pub struct MaxicomTerminal<'a> {
    screen: &'a vt100::Screen,
    x_offset: u16,
    y_offset: u16,
    block: Option<Block<'a>>,
    style: Option<Style>,
    cursor: MaxicomCursor<'a>,
    empty_filling_char: &'a str,
    outside_filling_style: Style,
    focused: bool,
}

impl<'a> MaxicomTerminal<'a> {
    #[inline]
    #[must_use]
    pub fn new(screen: &'a vt100::Screen) -> Self {
        MaxicomTerminal {
            screen,
            x_offset: 0,
            y_offset: 0,
            block: None,
            style: None,
            cursor: MaxicomCursor::default(),
            empty_filling_char: " ",
            outside_filling_style: Style::default().fg(Color::DarkGray),
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
    pub fn cursor(mut self, cursor: MaxicomCursor<'a>) -> Self {
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
    pub const fn filling_char_style(mut self, style: Style) -> Self {
        self.outside_filling_style = style;
        self
    }

    #[inline]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    #[inline]
    #[must_use]
    pub const fn screen(&self) -> &vt100::Screen {
        self.screen
    }
}

impl Widget for &MaxicomTerminal<'_> {
    #[inline]
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let area = self.block.as_ref().map_or(area, |b| {
            let inner_area = b.inner(area);
            b.clone().render(area, buf);
            inner_area
        });

        if let Some(style) = self.style {
            buf.set_style(area, style);
        }

        handle(self, area, buf);
    }
}

impl Widget for MaxicomTerminal<'_> {
    #[inline]
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// Draw the vt100 screen to the [`Buffer`],
/// area is the designated area that the consumer provides
fn handle(term: &MaxicomTerminal, area: Rect, buf: &mut Buffer) {
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
                fill_buf_cell(screen_cell, cell);

                // Prevent loosing fg or bg if existing from the real data CSI
                if let Some(base_style) = term.style {
                    if let Some(bg) = base_style.bg
                        && (cell.style().bg.is_none() || cell.style().bg == Some(Color::Reset))
                    {
                        cell.set_bg(bg);
                    }
                    if let Some(fg) = base_style.fg
                        && (cell.style().fg.is_none() || cell.style().fg == Some(Color::Reset))
                    {
                        cell.set_fg(fg);
                    }
                }
            } else {
                // Outside the terminal canvas (viewer area padding)
                cell.set_symbol(term.empty_filling_char);
                cell.set_style(term.outside_filling_style);
            }
        }
    }

    // fake cursor when not focused
    if !term.focused && !screen.hide_cursor() && term.cursor.show {
        let (c_row, c_col) = visible_cursor_position(screen);

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
                dst.set_style(term.cursor.unfocused_style);
            } else {
                dst.set_symbol(term.cursor.unfocused_symbol);
                dst.set_style(term.cursor.unfocused_style);
            }
        }
    }
}

// The 3 functions below are adapted from tui-term 0.3.4 (`src/vt100_imp.rs`, MIT license)

/// Cursor row counted in the scrolled view: vt100 gives it on the live screen, so scrolling back
/// by N rows moves it N rows down, out of view once past the bottom
fn visible_cursor_position(screen: &vt100::Screen) -> (u16, u16) {
    let (row, col) = screen.cursor_position();
    let scrollback = u16::try_from(screen.scrollback()).unwrap_or(u16::MAX);
    (row.saturating_add(scrollback), col)
}

/// Copies a vt100 cell (contents, colors, modifiers) into a ratatui cell. An empty vt100 cell
/// keeps the symbol already in the buffer, its style is always replaced
fn fill_buf_cell(screen_cell: &vt100::Cell, buf_cell: &mut Cell) {
    if screen_cell.has_contents() {
        buf_cell.set_symbol(screen_cell.contents());
    }

    let mut modifier = Modifier::empty();
    if screen_cell.bold() {
        modifier |= Modifier::BOLD;
    }
    if screen_cell.italic() {
        modifier |= Modifier::ITALIC;
    }
    if screen_cell.underline() {
        modifier |= Modifier::UNDERLINED;
    }
    if screen_cell.inverse() {
        modifier |= Modifier::REVERSED;
    }
    if screen_cell.dim() {
        modifier |= Modifier::DIM;
    }

    let fg = map_color(screen_cell.fgcolor());
    let bg = map_color(screen_cell.bgcolor());

    buf_cell.set_style(Style::reset().fg(fg).bg(bg).add_modifier(modifier));
}

fn map_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    //! Golden output of the terminal widget, locked before replacing tui-term (#29).

    use super::*;
    use std::fmt::Write;

    /// Renders the widget into a `w`x`h` buffer and dumps each row's symbols (spaces as `.`, so
    /// editors don't strip them), then the cells that have a color or a modifier.
    fn dump(term: MaxicomTerminal<'_>, w: u16, h: u16) -> String {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        term.render(area, &mut buf);
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(&buf[(x, y)].symbol().replace(' ', "."));
            }
            out.push('\n');
        }
        // runs of same style cells per row, as `row cols: fg bg modifiers`
        for y in 0..h {
            let mut x = 0;
            while x < w {
                let style = buf[(x, y)].style();
                let start = x;
                while x < w && buf[(x, y)].style() == style {
                    x += 1;
                }
                let plain = style.fg == Some(Color::Reset)
                    && style.bg == Some(Color::Reset)
                    && style.add_modifier.is_empty();
                if !plain {
                    let (fg, bg, mods) = (style.fg, style.bg, style.add_modifier);
                    writeln!(out, "{y} {start}..{x}: {fg:?} {bg:?} {mods:?}").unwrap();
                }
            }
        }
        out
    }

    fn styled_parser() -> vt100::Parser {
        let mut parser = vt100::Parser::new(vt100_dim(3), vt100_dim(8), 10);
        parser.process(b"old1\r\nold2\r\n");
        parser.process(b"\x1b[1;31mred\x1b[0m \x1b[2;3;4;7mx\x1b[0m\r\n");
        parser.process(b"\x1b[38;2;1;2;3;48;5;200mrgb\x1b[0m\r\nend");
        parser
    }

    /// Colors (indexed, RGB, default) and every modifier map to the ratatui cell, a base style
    /// only fills default colors, and cells outside the screen get the filling char.
    /// How: a 3x8 screen rendered into a 10x4 buffer with a base style.
    #[test]
    fn golden_styled_cells() {
        let parser = styled_parser();
        let term = MaxicomTerminal::new(parser.screen())
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .filling_char("·");
        insta_like(dump(term, 10, 4), GOLDEN_STYLED);
    }

    /// The unfocused fake cursor follows the scrollback offset, on an empty cell (symbol) and
    /// once scrolled back so it is out of view.
    /// How: same screen unfocused, at scrollback 0 then 1 and 3.
    #[test]
    fn golden_unfocused_cursor() {
        let mut parser = styled_parser();
        let cursor = MaxicomCursor::from_config(Style::default().fg(Color::Yellow), "▒");
        let mut out = String::new();
        for scrollback in [0, 1, 3] {
            parser.screen_mut().set_scrollback(scrollback);
            let term = MaxicomTerminal::new(parser.screen())
                .cursor(cursor.clone())
                .focused(false);
            writeln!(out, "-- scrollback {scrollback}").unwrap();
            out.push_str(&dump(term, 8, 3));
        }
        insta_like(out, GOLDEN_CURSOR);
    }

    /// x/y offsets pan the screen inside the widget.
    /// How: offsets 1,1 on the styled screen.
    #[test]
    fn golden_offsets() {
        let parser = styled_parser();
        let term = MaxicomTerminal::new(parser.screen())
            .x_offset(1)
            .y_offset(1);
        insta_like(dump(term, 8, 3), GOLDEN_OFFSETS);
    }

    /// Compares with a golden string; `GOLDEN_PRINT=1` prints the actual output to recapture it.
    fn insta_like(actual: String, expected: &str) {
        if std::env::var_os("GOLDEN_PRINT").is_some() {
            println!("{actual}");
        }
        // the constants start with a line break so their first row lines up with the others
        assert_eq!(actual, expected.strip_prefix('\n').unwrap());
    }

    const GOLDEN_STYLED: &str = r#"
red.x...··
rgb.....··
end.....··
··········
0 0..3: Some(Indexed(1)) Some(Black) BOLD
0 3..4: Some(White) Some(Black) NONE
0 4..5: Some(White) Some(Black) DIM | ITALIC | UNDERLINED | REVERSED
0 5..8: Some(White) Some(Black) NONE
0 8..10: Some(DarkGray) Some(Black) NONE
1 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
1 3..8: Some(White) Some(Black) NONE
1 8..10: Some(DarkGray) Some(Black) NONE
2 0..8: Some(White) Some(Black) NONE
2 8..10: Some(DarkGray) Some(Black) NONE
3 0..10: Some(DarkGray) Some(Black) NONE
"#;
    const GOLDEN_CURSOR: &str = r#"
-- scrollback 0
red.x...
rgb.....
end▒....
0 0..3: Some(Indexed(1)) Some(Reset) BOLD
0 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
1 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
2 3..4: Some(Yellow) Some(Reset) NONE
-- scrollback 1
old2....
red.x...
rgb.....
1 0..3: Some(Indexed(1)) Some(Reset) BOLD
1 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
2 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
-- scrollback 3
old1....
old2....
red.x...
2 0..3: Some(Indexed(1)) Some(Reset) BOLD
2 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
"#;
    const GOLDEN_OFFSETS: &str = r#"
gb......
nd......
........
0 0..2: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
0 7..8: Some(DarkGray) Some(Reset) NONE
1 7..8: Some(DarkGray) Some(Reset) NONE
2 0..8: Some(DarkGray) Some(Reset) NONE
"#;
}
