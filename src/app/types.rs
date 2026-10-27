use chrono::{DateTime, Local};
use ratatui::{layout::Rect, style::Style};

use crate::{
    app::selection::{CopySide, LogicalPosition},
    configs::{KeyBindingAction, KeyBindingSearchAction, RxNewlineMode},
    overlays::PopupTarget,
};

/// Received bytes cut into lines the way line mode shows them: LF ends a line, and with CrLf
/// or Auto a CR followed by anything else starts it over. Shared by line mode and the
/// recorder, so a recording has the same lines whatever the mode (MAX-147)
#[derive(Debug, Default)]
pub struct RxLineSplitter {
    line: Vec<u8>,
    timestamp: Option<DateTime<Local>>,
    pending_carriage_return: bool,
}

impl RxLineSplitter {
    pub fn push(&mut self, byte: u8, rx_eol_mode: RxNewlineMode, now: DateTime<Local>) -> bool {
        if self.line.is_empty() {
            self.timestamp = Some(now);
        }

        match byte {
            b'\r' => match rx_eol_mode {
                RxNewlineMode::CrLf | RxNewlineMode::Auto => {
                    self.pending_carriage_return = true;
                    self.line.push(b'\r');
                }
                RxNewlineMode::LfOnly => {
                    self.line.push(b'\r');
                }
            },
            b'\n' => {
                self.line.push(b'\n');
                self.pending_carriage_return = false;
                return true;
            }
            byte => {
                if self.pending_carriage_return {
                    self.pending_carriage_return = false;
                    self.line.clear();
                    self.timestamp = Some(now);
                }
                self.line.push(byte);
            }
        }
        false
    }

    /// The line received so far, with its EOL once ended
    #[inline]
    pub fn line(&self) -> &[u8] {
        &self.line
    }

    #[inline]
    pub fn timestamp(&self) -> Option<DateTime<Local>> {
        self.timestamp
    }

    #[inline]
    pub fn clear(&mut self) {
        self.line.clear();
    }

    /// When the line started, now if nothing was received
    pub fn take_timestamp(&mut self) -> DateTime<Local> {
        self.timestamp.take().unwrap_or_else(Local::now)
    }
}

// we no longer cache the timestamp string because there is less lines to render
// and it cost alloc and space when in the 10 of thousands of lines
pub struct LogEntry {
    pub text: String,
    /// Bytes received, kept only when they differ from `text`'s UTF-8 (Latin1, CP437, ASCII or
    /// invalid UTF-8): what was on the wire for the raw panel, raw search and raw yank (#20)
    pub wire: Option<Box<[u8]>>,
    pub timestamp: DateTime<Local>,
}

impl LogEntry {
    /// A line typed or echoed locally: its bytes are its text
    pub fn new(text: String, timestamp: DateTime<Local>) -> Self {
        Self {
            text,
            wire: None,
            timestamp,
        }
    }

    /// A line decoded from the bytes received
    pub fn received(text: String, wire: &[u8], timestamp: DateTime<Local>) -> Self {
        let wire = (wire != text.as_bytes()).then(|| wire.into());
        Self {
            text,
            wire,
            timestamp,
        }
    }

    /// The bytes received for this line
    pub fn wire_bytes(&self) -> &[u8] {
        self.wire.as_deref().unwrap_or(self.text.as_bytes())
    }

    /// For each byte offset of `text` (and its end), the offset of the received byte it was
    /// decoded from. One char per received byte for single byte encodings (Latin1, CP437,
    /// ASCII) and for UTF-8 made of ASCII and single invalid bytes; otherwise the received
    /// bytes are split the way `String::from_utf8_lossy` decoded them, one U+FFFD per invalid
    /// sequence
    pub fn text_to_wire_offsets(&self) -> Vec<usize> {
        let text = &self.text;
        let wire = self.wire_bytes();
        let mut offsets = Vec::with_capacity(text.len() + 1);
        if self.wire.is_none() {
            offsets.extend(0..=text.len());
        } else if text.chars().count() == wire.len() {
            for (wire_idx, ch) in text.chars().enumerate() {
                offsets.extend(std::iter::repeat_n(wire_idx, ch.len_utf8()));
            }
            offsets.push(wire.len());
        } else {
            let mut wire_idx = 0;
            for chunk in wire.utf8_chunks() {
                offsets.extend(wire_idx..wire_idx + chunk.valid().len());
                wire_idx += chunk.valid().len();
                if !chunk.invalid().is_empty() {
                    offsets.extend(std::iter::repeat_n(wire_idx, '\u{FFFD}'.len_utf8()));
                    wire_idx += chunk.invalid().len();
                }
            }
            offsets.push(wire.len());
        }
        offsets
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportSize {
    pub cols: usize,
    pub rows: usize,
}

impl ViewportSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

/// Where the last frame drew the history and the raw side panel, for the mouse (MAX-113)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameAreas {
    pub content: Rect,
    pub raw_side: Option<Rect>,
}

/// What the last frame showed where, for mouse clicks (MAX-113): per screen row of the history
/// its line and where its chars are, per row of the raw panel its line and its bytes. Rows
/// showing nothing a copy cursor can go to are `None`
#[derive(Debug, Default)]
pub struct FrameMap {
    pub text_area: Rect,
    pub text_rows: Vec<Option<TextRow>>,
    pub raw_area: Rect,
    pub raw_rows: Vec<Option<RawRow>>,
    /// Raw panel pan (columns) and width of one byte (columns), as drawn
    pub raw_pan_x: usize,
    pub raw_stride: usize,
    /// The open popup's area, and the parts of it a click picks
    pub popup: Option<Rect>,
    pub popup_targets: Vec<(Rect, PopupTarget)>,
    /// The TX input's text, as drawn
    pub prompt: Option<PromptMap>,
    /// The search bar's query, as drawn
    pub search_query: Option<PromptMap>,
    /// The history's scrollbar, as drawn
    pub scrollbar: Option<ScrollbarMap>,
    /// The status bar blocks, border buttons and search bar hints a click runs, in drawing
    /// order (MAX-125, MAX-128)
    pub click_targets: Vec<ClickTarget>,
}

/// A part a click runs, outside the popups, drawn in `hover` under the pointer (MAX-131)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickTarget {
    pub area: Rect,
    pub action: ClickAction,
    pub hover: Option<Style>,
}

/// What a click on a status bar block, a border button or a search bar hint runs (MAX-125)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    /// An action of the keymap, by its name in `click=<action>` or implied by a variable
    Run(KeyBindingAction),
    /// The copy cursor's panel (`{copy_side}`), a copy mode action
    ToggleSide,
    /// A search bar hint (MAX-131)
    Search(KeyBindingSearchAction),
}

/// The history's scrollbar as drawn (MAX-142): its column, `▲` on top and `▼` at the bottom
/// with the track between them, the positions it goes through (`0` the top, `max` the live view),
/// the one it shows and the rows the view shows. The thumb is placed as ratatui's `Scrollbar`
/// places it, so a pointer lands where it is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarMap {
    pub area: Rect,
    pub max: usize,
    pub position: usize,
    pub view: usize,
}

impl ScrollbarMap {
    pub fn track(&self) -> usize {
        self.area.height.saturating_sub(2) as usize
    }

    /// The thumb's first row in the track and its length
    pub fn thumb(&self) -> (usize, usize) {
        let track = self.track();
        let max_position = self.max.saturating_sub(1);
        let max_viewport = max_position + self.view;
        if track == 0 || max_viewport == 0 {
            return (0, track);
        }
        let round = |n: usize, d: usize| (n + d / 2) / d;
        let length = round(self.view * track, max_viewport).clamp(1, track);
        let first = round(self.position.min(max_position) * track, max_viewport);
        (first.min(track - length), length)
    }

    /// The position that draws the thumb from track row `first` (out of the track: its end);
    /// the thumb at the bottom is the live view
    pub fn position_at(&self, first: isize) -> usize {
        let track = self.track();
        let (_, length) = self.thumb();
        let first = first.clamp(0, (track - length) as isize) as usize;
        if track == 0 || first + length >= track {
            return self.max;
        }
        let max_viewport = self.max.saturating_sub(1) + self.view;
        ((first * max_viewport + track / 2) / track).min(self.max)
    }

    /// The cells of the arrow or the thumb under a screen cell, for the hover
    pub fn part_at(&self, x: u16, y: u16) -> Option<Rect> {
        if !self.area.contains(ratatui::layout::Position::new(x, y)) {
            return None;
        }
        let row = y - self.area.y;
        if row == 0 || row + 1 == self.area.height {
            return Some(Rect::new(x, y, 1, 1));
        }
        let (first, length) = self.thumb();
        let in_track = row as usize - 1;
        (first..first + length)
            .contains(&in_track)
            .then(|| Rect::new(x, self.area.y + 1 + first as u16, 1, length as u16))
    }
}

/// Where an input's text is drawn (the TX input, the search query): its area (after a prefix)
/// and how many columns of it are scrolled out on the left
#[derive(Debug, Clone, Copy)]
pub struct PromptMap {
    pub text_area: Rect,
    pub scroll: usize,
}

#[derive(Debug)]
pub struct TextRow {
    pub line_idx: usize,
    pub chars: RowChars,
}

#[derive(Debug)]
pub enum RowChars {
    /// One char per column, the first column showing char `first` (the vt100 grid)
    Grid { first: usize },
    /// Where each char shown starts (column in the area) and its index, in order; `first` is
    /// the row's first char, for a row showing none (an empty line, or panned past its end)
    Cols {
        first: usize,
        cols: Vec<(u16, usize)>,
    },
}

#[derive(Debug)]
pub struct RawRow {
    pub line_idx: usize,
    pub byte_start: usize,
    pub byte_count: usize,
}

impl FrameMap {
    pub fn clear(&mut self) {
        self.text_area = Rect::default();
        self.text_rows.clear();
        self.raw_area = Rect::default();
        self.raw_rows.clear();
        self.popup = None;
        self.popup_targets.clear();
        self.prompt = None;
        self.search_query = None;
        self.scrollbar = None;
        self.click_targets.clear();
    }

    /// The click action under a screen cell
    pub fn click_target(&self, x: u16, y: u16) -> Option<ClickAction> {
        self.click_target_at(x, y).map(|target| target.action)
    }

    /// The part a click runs under a screen cell: of the ones drawn last there, the status
    /// bar's right part over its center over its left
    pub fn click_target_at(&self, x: u16, y: u16) -> Option<ClickTarget> {
        let at = ratatui::layout::Position::new(x, y);
        self.click_targets
            .iter()
            .rev()
            .find(|target| target.area.contains(at))
            .copied()
    }

    /// The popup part under a screen cell
    pub fn popup_target(&self, x: u16, y: u16) -> Option<PopupTarget> {
        self.popup_target_at(x, y).map(|(_, target)| target)
    }

    /// The popup part under a screen cell, with its cells
    pub fn popup_target_at(&self, x: u16, y: u16) -> Option<(Rect, PopupTarget)> {
        let at = ratatui::layout::Position::new(x, y);
        self.popup_targets
            .iter()
            .find(|(area, _)| area.contains(at))
            .copied()
    }

    /// The cells of the clickable part under a screen cell (MAX-131): of the popup when one is
    /// open, else a status bar block, border button or search bar hint
    pub fn hover_area(&self, x: u16, y: u16) -> Option<Rect> {
        if self.popup.is_some() {
            return self.popup_target_at(x, y).map(|(area, _)| area);
        }
        self.click_target_at(x, y)
            .map(|target| target.area)
            .or_else(|| self.scrollbar?.part_at(x, y))
    }

    /// The TX input column (counted from the text's start, scroll included) under a cell
    pub fn prompt_column(&self, x: u16, y: u16) -> Option<usize> {
        let prompt = self.prompt?;
        let area = prompt.text_area;
        if y < area.top() || y >= area.bottom() || x >= area.right() {
            return None;
        }
        // a click on the prompt prefix goes to the text's start
        Some(x.saturating_sub(area.left()) as usize + prompt.scroll)
    }

    /// The search query's column under a cell: on the query only, the bar's labels before it
    /// aren't a prefix of it
    pub fn search_column(&self, x: u16, y: u16) -> Option<usize> {
        let area = self.search_query?.text_area;
        area.contains(ratatui::layout::Position::new(x, y))
            .then(|| (x - area.x) as usize)
    }

    /// The text position under a screen cell. `clamp` (a drag) brings a cell out of the
    /// history to the nearest row and char shown; without it such a cell has none
    pub fn text_position(&self, x: u16, y: u16, clamp: bool) -> Option<LogicalPosition> {
        let (col, row) = cell_in(self.text_area, x, y, clamp)?;
        let text_row = pick_row(&self.text_rows, row, clamp)?;
        let char_idx = match &text_row.chars {
            RowChars::Grid { first } => first + col,
            RowChars::Cols { first, cols } => {
                // the last char starting at or before the column, else the first one shown
                let shown = cols.partition_point(|&(start, _)| start as usize <= col);
                match shown
                    .checked_sub(1)
                    .and_then(|idx| cols.get(idx))
                    .or(cols.first())
                {
                    Some(&(_, char_idx)) => char_idx,
                    None => *first,
                }
            }
        };
        Some(LogicalPosition {
            line_idx: text_row.line_idx,
            char_idx,
            preferred_col: char_idx,
            byte_idx: 0,
        })
    }

    /// The position under a screen cell of the text or the raw panel, `clamp` as for the text
    pub fn copy_position(
        &self,
        side: CopySide,
        x: u16,
        y: u16,
        clamp: bool,
    ) -> Option<LogicalPosition> {
        match side {
            CopySide::Text => self.text_position(x, y, clamp),
            CopySide::Raw => self.raw_position(x, y, clamp),
        }
    }

    /// The raw position (byte) under a screen cell of the raw panel, `clamp` as for the text
    pub fn raw_position(&self, x: u16, y: u16, clamp: bool) -> Option<LogicalPosition> {
        let (col, row) = cell_in(self.raw_area, x, y, clamp)?;
        let raw_row = pick_row(&self.raw_rows, row, clamp)?;
        let stride = self.raw_stride.max(1);
        let byte_in_row = (col + self.raw_pan_x) / stride;
        let byte_idx = raw_row.byte_start + byte_in_row.min(raw_row.byte_count.saturating_sub(1));
        Some(LogicalPosition {
            line_idx: raw_row.line_idx,
            char_idx: 0,
            preferred_col: byte_idx,
            byte_idx,
        })
    }
}

/// The cell's column and row in `area`, or with `clamp` the nearest cell of it
fn cell_in(area: Rect, x: u16, y: u16, clamp: bool) -> Option<(usize, usize)> {
    if area.is_empty() {
        return None;
    }
    let inside = area.contains(ratatui::layout::Position::new(x, y));
    if !inside && !clamp {
        return None;
    }
    let col = x.clamp(area.left(), area.right() - 1) - area.left();
    let row = y.clamp(area.top(), area.bottom() - 1) - area.top();
    Some((col as usize, row as usize))
}

/// The row's map, or with `clamp` the nearest row above showing something (else below)
fn pick_row<T>(rows: &[Option<T>], row: usize, clamp: bool) -> Option<&T> {
    if let Some(Some(found)) = rows.get(row) {
        return Some(found);
    }
    if !clamp {
        return None;
    }
    let above = rows[..row.min(rows.len())].iter().rev().flatten().next();
    above.or_else(|| rows.iter().skip(row).flatten().next())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Terminal,
    LineInput,
    LineHistory,
    Overlay,
}

#[cfg(test)]
mod tests {
    //! Wire bytes kept next to the decoded text, and how text offsets map back to them (#20).

    use super::*;
    use crate::configs::TextEncoding;

    fn received(encoding: TextEncoding, wire: &[u8]) -> LogEntry {
        LogEntry::received(encoding.decode(wire), wire, Local::now())
    }

    /// Wire bytes are only stored when they differ from the text's UTF-8, and `wire_bytes`
    /// always gives what was received.
    /// How: valid UTF-8, Latin1, invalid UTF-8 and a locally echoed line.
    #[test]
    fn wire_kept_only_when_it_differs() {
        let utf8 = received(TextEncoding::Utf8, "é".as_bytes());
        assert!(utf8.wire.is_none());
        assert_eq!(utf8.wire_bytes(), "é".as_bytes());

        let latin1 = received(TextEncoding::Latin1, &[0xE9]);
        assert_eq!(latin1.text, "é");
        assert_eq!(latin1.wire_bytes(), [0xE9]);

        let invalid = received(TextEncoding::Utf8, &[0xFF]);
        assert_eq!(invalid.text, "\u{FFFD}");
        assert_eq!(invalid.wire_bytes(), [0xFF]);

        let echoed = LogEntry::new(String::from("ls"), Local::now());
        assert_eq!(echoed.wire_bytes(), b"ls");
    }

    /// Each text byte points to the received byte it was decoded from.
    /// How: exact offsets for each encoding, including multi-byte invalid UTF-8 sequences.
    #[test]
    fn text_to_wire_offsets_per_encoding() {
        let offsets = |encoding, wire: &[u8]| received(encoding, wire).text_to_wire_offsets();
        // x é y: é is 1 received byte, 2 text bytes
        assert_eq!(offsets(TextEncoding::Latin1, b"x\xE9y"), [0, 1, 1, 2, 3]);
        assert_eq!(offsets(TextEncoding::Cp437, b"x\x82y"), [0, 1, 1, 2, 3]);
        // ASCII and UTF-8: an invalid byte becomes U+FFFD (3 text bytes)
        assert_eq!(offsets(TextEncoding::Ascii, b"a\xE9b"), [0, 1, 1, 1, 2, 3]);
        assert_eq!(offsets(TextEncoding::Utf8, b"a\xFFb"), [0, 1, 1, 1, 2, 3]);
        // valid é then an invalid byte: fewer chars than bytes, split like from_utf8_lossy
        assert_eq!(
            offsets(TextEncoding::Utf8, b"\xC3\xA9\xFF"),
            [0, 1, 2, 2, 2, 3]
        );
        // a cut 3-byte sequence is one U+FFFD for 2 received bytes
        assert_eq!(offsets(TextEncoding::Utf8, b"\xE2\x82a"), [0, 0, 0, 2, 3]);
        // nothing to map for valid UTF-8
        assert_eq!(offsets(TextEncoding::Utf8, "hé".as_bytes()), [0, 1, 2, 3]);
    }

    /// For any received bytes and encoding: one offset per text byte plus the end, never
    /// decreasing, every char starting on a received byte, the end at the wire length.
    /// How: 2000 lines from a fixed-seed xorshift, decoded with every encoding.
    #[test]
    fn text_to_wire_offsets_hold_on_generated_bytes() {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2000 {
            let len = (next() % 24) as usize;
            // biased towards UTF-8 lead and continuation bytes
            let wire: Vec<u8> = (0..len)
                .map(|_| match next() % 4 {
                    0 => (next() % 0x80) as u8,
                    1 => 0x80 | (next() % 0x40) as u8,
                    _ => 0xC0 | (next() % 0x40) as u8,
                })
                .collect();
            for encoding in [
                TextEncoding::Utf8,
                TextEncoding::Ascii,
                TextEncoding::Latin1,
                TextEncoding::Cp437,
            ] {
                let entry = received(encoding, &wire);
                let offsets = entry.text_to_wire_offsets();
                let context = format!("{encoding:?} {wire:02X?}");
                assert_eq!(offsets.len(), entry.text.len() + 1, "{context}");
                assert_eq!(offsets.last(), Some(&wire.len()), "{context}");
                assert!(offsets.windows(2).all(|w| w[0] <= w[1]), "{context}");
                for (byte, _) in entry.text.char_indices() {
                    assert!(offsets[byte] < wire.len(), "{context}");
                }
            }
        }
    }
}
