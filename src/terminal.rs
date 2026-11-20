/// Terminal widget drawing a vt100 screen, custom from tui-term (which pins vt100 0.16, #29)
use std::num::NonZeroU16;

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Clear, Widget},
};

use crate::app::term_answers::{
    Answer, SIZE_REPORTS, answer_control, answer_csi, answer_escape, size_report,
};
use crate::configs::settings::UnhandledGroup;
use crate::glyphs::{UNSURE_MARKER, drawn, invalid_bytes, unsure_width::is_unsure};

/// What the parser reports besides the screen. `evicted` counts the main screen rows vt100 drops
/// from the top of scrollback (`on_scroll` is only called for those, not for rows entering it):
/// with the change of `scrollback_len` it gives how many rows a chunk pushed into scrollback
/// (#13). `answers` are the replies to the remote's queries, in order, for the settings to send
/// or drop (MAX-151). `size_reports` is mode 2048, which vt100 doesn't know: the remote app
/// asked for its size in the stream on each change (MAX-159). With `collect`, `dropped` lists
/// what vt100 couldn't use, as its callbacks report it (MAX-264). `title` and `icon_name` are
/// the last ones the device set, OSC 2 and 1, both with OSC 0; `clipboard` the base64 text of
/// its last OSC 52 copy, until taken (MAX-275)
#[derive(Default)]
pub struct ParserEvents {
    pub evicted: usize,
    pub answers: Vec<Answer>,
    pub size_reports: bool,
    pub collect: bool,
    pub dropped: Vec<Dropped>,
    pub title: Option<String>,
    pub icon_name: Option<String>,
    pub clipboard: Option<Vec<u8>>,
}

/// The longest OSC 52 copy taken, in base64: about 768 KB of text
const CLIPBOARD_MAX: usize = 1 << 20;

/// The most chars of a title kept: a binary dump can send a long one
const TITLE_MAX: usize = 200;

/// A title as kept: invalid bytes as `�` (their stand-ins, MAX-249), control chars left out,
/// at most `TITLE_MAX` chars, none when empty (a device clears its title with an empty one)
fn device_text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let text: String = invalid_bytes::shown(&text)
        .chars()
        .filter(|c| !c.is_control())
        .take(TITLE_MAX)
        .collect();
    (!text.is_empty()).then_some(text)
}

/// Something received that vt100 didn't use (MAX-264), with its bytes: as received for the
/// ones the tokenizer finds, rebuilt from the callback's arguments for the others
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub kind: DropKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    /// A C0 control byte vt100 doesn't use, SO and SI included, or DEL
    Control,
    /// A C1 control, U+0080 to U+009F
    C1,
    Escape,
    Csi,
    Osc,
    /// A DCS, SOS, PM or APC string
    String,
}

/// The C0 controls' names, by byte
const C0_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US",
];

/// The longest sequence written whole in a label: a binary dump can send long ones
const LABEL_MAX: usize = 24;

impl Dropped {
    /// How the notification names it: `NUL`, `DEL`, `C1 85`, `ESC #3`, `CSI ?2026h`, `OSC 7`,
    /// `DCS` (MAX-264)
    pub fn label(&self) -> String {
        let text = |bytes: &[u8]| {
            let text: String = String::from_utf8_lossy(bytes)
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            match text.char_indices().nth(LABEL_MAX) {
                Some((at, _)) => format!("{}...", &text[..at]),
                None => text,
            }
        };
        let after = |n: usize| self.bytes.get(n..).unwrap_or_default();
        match self.kind {
            DropKind::Control => match self.bytes.first() {
                Some(&b) if b < 0x20 => C0_NAMES[b as usize].to_string(),
                _ => "DEL".to_string(),
            },
            DropKind::C1 => {
                let code = String::from_utf8_lossy(&self.bytes).chars().next();
                format!("C1 {:02X}", code.map_or(0, u32::from))
            }
            DropKind::Escape => format!("ESC {}", text(after(1))),
            DropKind::Csi => format!("CSI {}", text(after(2))),
            DropKind::Osc => {
                let number = after(2).split(|&b| b == b';').next().unwrap_or_default();
                format!("OSC {}", text(number))
            }
            DropKind::String => match self.bytes.get(1) {
                Some(b'P') => "DCS",
                Some(b'X') => "SOS",
                Some(b'^') => "PM",
                _ => "APC",
            }
            .to_string(),
        }
    }

    /// Its group in `notify_unhandled`: by kind, but the harmless ones in `Shell`
    pub fn group(&self) -> UnhandledGroup {
        match self.kind {
            _ if self.harmless() => UnhandledGroup::Shell,
            DropKind::Control => UnhandledGroup::Controls,
            DropKind::C1 => UnhandledGroup::C1,
            DropKind::Escape => UnhandledGroup::Escape,
            DropKind::Csi => UnhandledGroup::Csi,
            DropKind::Osc => UnhandledGroup::Osc,
            DropKind::String => UnhandledGroup::Strings,
        }
    }

    /// What modern shells and programs send and a terminal may ignore without harm: the
    /// current directory (OSC 7), hyperlinks (OSC 8), prompt marks (OSC 133), focus reports and
    /// synchronized output turned on or off (`?1004`, `?2026`), cursor blinking (`?12`),
    /// autowrap turned on (`?7h`: always on), the title saved and restored (`22 t`, `23 t`),
    /// ASCII chosen as the character set (`ESC ( B`: already in use)
    /// TODO: implement some of them
    pub fn harmless(&self) -> bool {
        let bytes = self.bytes.as_slice();
        match self.kind {
            DropKind::Osc => {
                let number = bytes.get(2..).unwrap_or_default();
                let number = number.split(|&b| b == b';').next().unwrap_or_default();
                matches!(number, b"7" | b"8" | b"133")
            }
            DropKind::Csi => {
                let title_stack = [&b"\x1b[22"[..], b"\x1b[23"].iter().any(|op| {
                    bytes.strip_prefix(*op).is_some_and(|rest| {
                        rest == b"t" || (rest.starts_with(b";") && rest.ends_with(b"t"))
                    })
                });
                title_stack
                    || matches!(
                        bytes,
                        b"\x1b[?1004h"
                            | b"\x1b[?1004l"
                            | b"\x1b[?2026h"
                            | b"\x1b[?2026l"
                            | b"\x1b[?12h"
                            | b"\x1b[?12l"
                            | b"\x1b[?7h"
                    )
            }
            DropKind::Escape => bytes == b"\x1b(B",
            _ => false,
        }
    }
}

impl ParserEvents {
    fn drop(&mut self, kind: DropKind, bytes: Vec<u8>) {
        if self.collect {
            self.dropped.push(Dropped { kind, bytes });
        }
    }
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
        // a DECSET / DECRST comes here once per mode vt100 doesn't know, with all the modes of
        // the sequence (`?1049;2048h`): the report goes out once, when the mode turns on
        let decset = i1 == Some(b'?') && i2.is_none() && matches!(c, 'h' | 'l');
        if decset && params.iter().any(|p| p.first() == Some(&SIZE_REPORTS)) {
            let on = c == 'h';
            if on && !self.size_reports {
                let (rows, cols) = vt100_size(screen);
                self.answers.push(size_report(rows, cols));
            }
            self.size_reports = on;
        }
        match answer_csi(screen, i1, i2, params, c, self.size_reports) {
            Some(answer) => self.answers.push(answer),
            None if decset && params.iter().any(|p| p.first() == Some(&SIZE_REPORTS)) => {}
            None => self.drop(DropKind::Csi, csi_bytes(i1, i2, params, c)),
        }
    }

    // a C1 control comes here too, as its code point (`0x85` for U+0085)
    fn unhandled_control(&mut self, _: &mut vt100::Screen, b: u8) {
        match answer_control(b) {
            Some(answer) => self.answers.push(answer),
            None if b >= 0x80 => self.drop(DropKind::C1, char::from(b).to_string().into_bytes()),
            None => self.drop(DropKind::Control, vec![b]),
        }
    }

    fn unhandled_escape(&mut self, _: &mut vt100::Screen, i1: Option<u8>, i2: Option<u8>, b: u8) {
        match answer_escape(i1, i2, b) {
            Some(answer) => self.answers.push(answer),
            // ST: the end of an OSC or a string vte read, not a sequence of its own
            None if (i1, i2, b) == (None, None, b'\\') => {}
            None => {
                let bytes = [Some(0x1b), i1, i2, Some(b)]
                    .into_iter()
                    .flatten()
                    .collect();
                self.drop(DropKind::Escape, bytes);
            }
        }
    }

    // vt100 also gives the U+FFFD it prints here: maxicom's stand-ins never reach it (MAX-249)
    fn unhandled_char(&mut self, _: &mut vt100::Screen, c: char) {
        if ('\u{80}'..'\u{A0}').contains(&c) {
            self.drop(DropKind::C1, c.to_string().into_bytes());
        }
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = device_text(title);
    }

    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, icon_name: &[u8]) {
        self.icon_name = device_text(icon_name);
    }

    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        if data.len() <= CLIPBOARD_MAX {
            self.clipboard = Some(data.to_vec());
        } else {
            self.drop(DropKind::Osc, osc52_bytes(ty, b"..."));
        }
    }

    // never answered: maxicom can't read the host's clipboard, and it isn't the device's
    fn paste_from_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8]) {
        self.drop(DropKind::Osc, osc52_bytes(ty, b"?"));
    }

    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        // vt100 only takes a title without `;`: the parts after the number are the title
        if let [kind @ (b"0" | b"1" | b"2"), rest @ ..] = params
            && rest.len() > 1
        {
            let text = rest.join(&b';');
            if *kind != b"1" {
                self.set_window_title(screen, &text);
            }
            if *kind != b"2" {
                self.set_window_icon_name(screen, &text);
            }
            return;
        }
        let mut bytes = b"\x1b]".to_vec();
        bytes.extend(params.join(&b';'));
        bytes.push(0x07);
        self.drop(DropKind::Osc, bytes);
    }
}

/// An OSC 52 written back, its data given
fn osc52_bytes(ty: &[u8], data: &[u8]) -> Vec<u8> {
    [b"\x1b]52;", ty, b";", data, b"\x07"].concat()
}

/// A CSI written back from what vt100 parsed: the private marker (`?`, `>`...) before the
/// parameters, `:` between sub-parameters, the other intermediates before the final byte
fn csi_bytes(i1: Option<u8>, i2: Option<u8>, params: &[&[u16]], c: char) -> Vec<u8> {
    let marks = [i1, i2].into_iter().flatten();
    let (private, intermediates): (Vec<u8>, Vec<u8>) =
        marks.partition(|b| (b'<'..=b'?').contains(b));
    let numbers: Vec<String> = params
        .iter()
        .map(|p| p.iter().map(u16::to_string).collect::<Vec<_>>().join(":"))
        .collect();
    let mut bytes = b"\x1b[".to_vec();
    bytes.extend(private);
    bytes.extend(numbers.join(";").into_bytes());
    bytes.extend(intermediates);
    bytes.extend(c.to_string().into_bytes());
    bytes
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
    zero_width: bool,
    unsure_width: bool,
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
            zero_width: false,
            unsure_width: false,
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

    /// Write the cells' zero-width chars too, see `drawn` (MAX-270)
    #[inline]
    pub const fn zero_width(mut self, zero_width: bool) -> Self {
        self.zero_width = zero_width;
        self
    }

    /// Write the chars hosts may draw at another width as received, not as `UNSURE_MARKER`
    /// (MAX-271)
    #[inline]
    pub const fn unsure_width(mut self, unsure_width: bool) -> Self {
        self.unsure_width = unsure_width;
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
                fill_buf_cell(screen_cell, cell, term.zero_width, term.unsure_width);
                // a wide char in the area's last column would spill over its right border
                if screen_cell.is_wide() && buf_col + 1 >= area_cols {
                    cell.set_symbol(" ");
                }

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

    // fake cursor when not focused, or scrolled into the history where the real one isn't
    // placed: it marks where the remote's cursor is
    if (!term.focused || screen.scrollback() > 0) && !screen.hide_cursor() && term.cursor.show {
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
/// keeps the symbol already in the buffer, its style is always replaced. A stand-in for
/// invalid bytes is drawn `�` (MAX-249), a first char of unsure width as `UNSURE_MARKER`
/// unless `unsure_width`, the chars after it as `drawn` keeps them
fn fill_buf_cell(
    screen_cell: &vt100::Cell,
    buf_cell: &mut Cell,
    zero_width: bool,
    unsure_width: bool,
) {
    let mut modifier = Modifier::empty();
    if screen_cell.has_contents() {
        let shown = invalid_bytes::shown(screen_cell.contents());
        let first_unsure = shown.chars().next().is_some_and(is_unsure);
        if first_unsure && !unsure_width {
            buf_cell.set_symbol(UNSURE_MARKER);
            modifier |= Modifier::DIM;
        } else {
            buf_cell.set_symbol(&drawn(&shown, zero_width, unsure_width));
        }
    }

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
#[path = "tests/terminal.rs"]
mod tests;
