use chrono::{DateTime, Local};
use ratatui::{layout::Rect, style::Style};

use crate::{
    app::selection::{CopySide, LogicalPosition},
    configs::{KeyBindingAction, KeyBindingSearchAction, settings::RawSideOrientation},
    overlays::PopupTarget,
};

/// Received bytes cut into lines the way line mode shows them: LF ends a line, and a CR
/// followed by anything else starts it over (add LF makes a lone CR end it, MAX-181). Shared
/// by line mode and the recorder, so a recording has the same lines whatever the mode
/// (MAX-147)
#[derive(Debug, Default)]
pub struct RxLineSplitter {
    line: Vec<u8>,
    timestamp: Option<DateTime<Local>>,
    pending_carriage_return: bool,
}

impl RxLineSplitter {
    pub fn push(&mut self, byte: u8, now: DateTime<Local>) -> bool {
        self.push_at(byte, Some(now))
    }

    /// `push` for bytes received at `time`, none when it isn't known (a restored session's
    /// bytes without their times, MAX-175)
    pub fn push_at(&mut self, byte: u8, time: Option<DateTime<Local>>) -> bool {
        if self.line.is_empty() {
            self.timestamp = time;
        }

        match byte {
            b'\r' => {
                self.pending_carriage_return = true;
                self.line.push(b'\r');
            }
            b'\n' => {
                self.line.push(b'\n');
                self.pending_carriage_return = false;
                return true;
            }
            byte => {
                if self.pending_carriage_return {
                    self.pending_carriage_return = false;
                    self.line.clear();
                    self.timestamp = time;
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

    /// With add LF, a CR not followed by LF ends its line instead of starting it over: true
    /// when `byte` comes after such a CR, the line to take before `byte` is pushed (MAX-181)
    pub fn ends_at_cr(&self, byte: u8, add_lf: bool) -> bool {
        add_lf && self.pending_carriage_return && byte != b'\n'
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
    /// None for a line restored from a session without a time to read back (MAX-148)
    pub timestamp: Option<DateTime<Local>>,
}

impl LogEntry {
    /// A line typed or echoed locally: its bytes are its text
    pub fn new(text: String, timestamp: DateTime<Local>) -> Self {
        Self {
            text,
            wire: None,
            timestamp: Some(timestamp),
        }
    }

    /// A line restored from a session, with its time when it could be read back
    /// (MAX-148)
    pub fn restored(text: String, timestamp: Option<DateTime<Local>>) -> Self {
        Self {
            text,
            wire: None,
            timestamp,
        }
    }

    /// A line decoded from the bytes received, at `timestamp` when it is known
    pub fn received(text: String, wire: &[u8], timestamp: Option<DateTime<Local>>) -> Self {
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
    /// The command line's text, as drawn (MAX-12)
    pub command_line: Option<PromptMap>,
    /// The history's scrollbar, as drawn
    pub scrollbar: Option<ScrollbarMap>,
    pub side_handle: Option<SideHandle>,
    /// The status bar blocks, border buttons and search bar hints a click runs, in drawing
    /// order (MAX-125, MAX-128)
    pub click_targets: Vec<ClickTarget>,
}

/// The raw side panel's border, as drawn (MAX-189): a drag on it resizes the panel, its share
/// of `split` taken from the pointer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideHandle {
    pub area: Rect,
    pub split: Rect,
    pub orientation: RawSideOrientation,
}

impl SideHandle {
    pub fn new(panel: Rect, split: Rect, orientation: RawSideOrientation) -> Self {
        let area = match orientation {
            RawSideOrientation::Right => Rect::new(panel.x, panel.y, 1, panel.height),
            RawSideOrientation::Bottom => Rect::new(panel.x, panel.y, panel.width, 1),
        };
        Self {
            area: area.intersection(panel),
            split,
            orientation,
        }
    }

    /// Its cells when `(x, y)` is on it
    pub fn at(&self, x: u16, y: u16) -> Option<Rect> {
        self.area
            .contains(ratatui::layout::Position::new(x, y))
            .then_some(self.area)
    }

    /// The panel's share of the split, in percent, with its border under the pointer
    pub fn percent_at(&self, x: u16, y: u16) -> u16 {
        let (panel, whole) = match self.orientation {
            RawSideOrientation::Right => (self.split.right().saturating_sub(x), self.split.width),
            RawSideOrientation::Bottom => {
                (self.split.bottom().saturating_sub(y), self.split.height)
            }
        };
        if whole == 0 {
            return 0;
        }
        ((u32::from(panel) * 100 + u32::from(whole) / 2) / u32::from(whole)) as u16
    }
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

impl PromptMap {
    /// The column (counted from the text's start, scroll included) under a cell of its row
    pub fn column(&self, x: u16, y: u16) -> Option<usize> {
        let area = self.text_area;
        if y < area.top() || y >= area.bottom() || x >= area.right() {
            return None;
        }
        // a click on the prompt prefix goes to the text's start
        Some(x.saturating_sub(area.left()) as usize + self.scroll)
    }
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
        self.command_line = None;
        self.scrollbar = None;
        self.side_handle = None;
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
            .or_else(|| self.side_handle?.at(x, y))
    }

    /// The TX input column (counted from the text's start, scroll included) under a cell
    pub fn prompt_column(&self, x: u16, y: u16) -> Option<usize> {
        self.prompt?.column(x, y)
    }

    /// The command line's column under a cell, in its popup only (MAX-12)
    pub fn command_line_column(&self, x: u16, y: u16) -> Option<usize> {
        let in_popup = self
            .popup
            .is_some_and(|area| area.contains(ratatui::layout::Position::new(x, y)));
        self.command_line?.column(x, y).filter(|_| in_popup)
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
#[path = "../tests/app/types.rs"]
mod tests;
