//! Terminal mode's raw panel (MAX-252): every byte received, shown as a dump like `xxd`'s.
//! Line `n` holds the bytes `n * width..(n + 1) * width` by their absolute offset, so the lines
//! are only arithmetic and stay put when the oldest bytes are dropped

mod copy;
mod scroll;
mod stream;
pub mod tokens;
pub mod writes;

use std::{
    collections::HashMap,
    ops::{Range, RangeInclusive},
};

pub use copy::{DumpCopy, DumpResult};
use scroll::DumpScroller;
pub use stream::RxStream;
use tokens::{Token, Tokenizer};
use writes::{Run, Writes};

use crate::{
    configs::{RawSideView, TextEncoding, settings::raw_view_to_display_mode},
    inputs::CopyModeAction,
    raw_side::RawSideScroller,
    ui::raw_side::format_raw_bytes_to_plain_string,
};

/// The most runs kept per history row: about 25 MB for the default 100000 rows (MAX-256)
const RUNS_PER_ROW: usize = 16;

/// A cell of the terminal (MAX-256): an absolute row (a grid row on the alternate screen) and a
/// column
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub row: u64,
    pub col: u16,
    pub alternate: bool,
}

/// A char received: its bytes and the cell it went to
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub bytes: Range<u64>,
    pub ch: char,
    pub cell: Cell,
}

/// The cells a char takes, as vt100 draws it: 0 for a combining char, 2 at most
fn width(ch: char) -> u64 {
    use unicode_width::UnicodeWidthChar;
    ch.width().unwrap_or(1).min(2) as u64
}

/// The dump: the bytes kept, its own scroll and pan, and copy mode's cursor in it
#[derive(Debug)]
pub struct Dump {
    stream: RxStream,
    scroller: DumpScroller,
    pan: RawSideScroller,
    copy: DumpCopy,
    tokenizer: Tokenizer,
    writes: Writes,
}

impl Dump {
    /// The dump of a terminal history of `history_rows` rows
    pub fn new(history_rows: usize) -> Self {
        Self {
            stream: RxStream::new(history_rows),
            scroller: DumpScroller::default(),
            pan: RawSideScroller::default(),
            copy: DumpCopy::default(),
            tokenizer: Tokenizer::new(TextEncoding::Utf8),
            writes: Writes::new(history_rows.saturating_mul(RUNS_PER_ROW).max(4096)),
        }
    }

    /// Adds a read; returns its first byte's offset
    pub fn push(&mut self, bytes: &[u8]) -> u64 {
        let offset = self.stream.push(bytes);
        self.writes.trim(self.stream.start());
        offset
    }

    /// Every byte dropped, the offsets still counting on; the dump follows the live end
    pub fn clear(&mut self) {
        self.stream.clear();
        self.writes.clear();
        self.scroller.follow();
    }

    /// The tokens of a read pushed at `offset`, read in `encoding` (MAX-256)
    pub fn tokens(&mut self, encoding: TextEncoding, bytes: &[u8], offset: u64) -> Vec<Token> {
        if self.tokenizer.encoding() != encoding {
            self.tokenizer = Tokenizer::new(encoding);
        }
        let mut tokens = Vec::new();
        self.tokenizer.feed(bytes, offset, &mut tokens);
        tokens
    }

    /// Back to following the bytes as they come
    pub fn follow(&mut self) {
        self.scroller.follow();
    }

    /// The byte trace dropped, its memory given back (MAX-256)
    pub fn forget_trace(&mut self) {
        self.writes.release();
    }

    #[inline]
    #[cfg(test)]
    pub fn writes(&self) -> &Writes {
        &self.writes
    }

    /// Records a printable run on a screen `cols` wide, its cells counted from its bytes kept (a
    /// char cut between two reads included); returns them
    pub fn record(&mut self, run: Run, encoding: TextEncoding, cols: u16) -> u32 {
        let (front, back) = self.stream.slices(run.start..run.end);
        let cells = if front.is_ascii() && back.is_ascii() {
            (front.len() + back.len()) as u32
        } else {
            (self.chars(run.start..run.end, encoding).iter())
                .map(|&(_, ch)| width(ch))
                .sum::<u64>() as u32
        };
        self.writes.push(run, cells, cols);
        cells
    }

    /// What byte `offset` wrote (MAX-256): its char and the cell it went to; none for a byte
    /// that draws nothing (a control byte, a sequence) or no longer kept
    pub fn written(&self, offset: u64, encoding: TextEncoding) -> Option<Write> {
        let run = self.writes.at(offset)?;
        (self.writes_of(run, encoding).into_iter()).find(|write| write.bytes.contains(&offset))
    }

    /// The writes to `cell`, newest first, `max` at most
    pub fn writes_to(&self, cell: Cell, encoding: TextEncoding, max: usize) -> Vec<Write> {
        let mut found = Vec::new();
        for run in self.writes.newest_first() {
            if run.alternate != cell.alternate || cell.row < run.row {
                continue;
            }
            if cell.row > run.row + self.rows_reached(run) {
                continue;
            }
            let write = (self.writes_of(run, encoding).into_iter()).find(|write| {
                let width = width(write.ch).max(1);
                write.cell.row == cell.row
                    && (u64::from(write.cell.col)..u64::from(write.cell.col) + width)
                        .contains(&u64::from(cell.col))
            });
            found.extend(write);
            if found.len() >= max {
                break;
            }
        }
        found
    }

    /// The chars the bytes of `range` wrote, in byte order, a char cut by its ends included
    pub fn written_in(&self, range: Range<u64>, encoding: TextEncoding) -> Vec<Write> {
        let mut found = Vec::new();
        for run in self.writes.within(range.clone()) {
            let writes = self.writes_of(run, encoding).into_iter();
            found.extend(writes.filter(|w| w.bytes.start < range.end && w.bytes.end > range.start));
        }
        found
    }

    /// The newest write on each cell of `rows` on the main screen or the alternate one (a wide
    /// char on both its cells), as `writes_to` finds each, in one pass over the runs
    pub fn newest_writes_on(
        &self,
        rows: RangeInclusive<u64>,
        alternate: bool,
        encoding: TextEncoding,
    ) -> HashMap<(u64, u16), Write> {
        let mut found = HashMap::new();
        for run in self.writes.newest_first() {
            if run.alternate != alternate || run.row > *rows.end() {
                continue;
            }
            if run.row + self.rows_reached(run) < *rows.start() {
                continue;
            }
            for write in self.writes_of(run, encoding) {
                if !rows.contains(&write.cell.row) {
                    continue;
                }
                let (row, col) = (write.cell.row, write.cell.col);
                for col in col..col + width(write.ch).max(1) as u16 {
                    found.entry((row, col)).or_insert_with(|| write.clone());
                }
            }
        }
        found
    }

    /// Whether bytes of `range` drew anything: false for escape sequences and line ends only
    pub fn draws_in(&self, range: Range<u64>) -> bool {
        self.writes.within(range).next().is_some()
    }

    /// The rows after its first one a run can reach: a char takes a byte at least, a cell at
    /// most two, on the screen as wide as it was then
    fn rows_reached(&self, run: Run) -> u64 {
        let wide = u64::from(self.writes.cols_at(run.start).max(1));
        (u64::from(run.col) + 2 * (run.end - run.start)) / wide
    }

    /// The chars a run wrote and their cells: from its first cell on, a char wrapping to the next
    /// row when it doesn't fit, as the screen was wide then (MAX-282), a combining char on the
    /// cell before it
    fn writes_of(&self, run: Run, encoding: TextEncoding) -> Vec<Write> {
        let cols = u64::from(self.writes.cols_at(run.start).max(1));
        let (mut row, mut col) = (run.row, u64::from(run.col));
        let mut last = None;
        let mut out = Vec::new();
        for (bytes, ch) in self.chars(run.start..run.end, encoding) {
            let width = width(ch);
            let cell = if width == 0 {
                match last {
                    Some(cell) => cell,
                    None => continue,
                }
            } else {
                if col + width > cols {
                    row += 1;
                    col = 0;
                }
                let cell = Cell {
                    row,
                    col: col as u16,
                    alternate: run.alternate,
                };
                col += width;
                cell
            };
            last = Some(cell);
            out.push(Write { bytes, ch, cell });
        }
        out
    }

    /// The chars of the bytes `range` kept, read in `encoding`, each with its bytes
    fn chars(
        &self,
        range: std::ops::Range<u64>,
        encoding: TextEncoding,
    ) -> Vec<(Range<u64>, char)> {
        let bytes = self.bytes(range.clone());
        let at = |i: usize| range.start + i as u64;
        match encoding {
            TextEncoding::Utf8 => String::from_utf8_lossy(&bytes)
                .char_indices()
                .map(|(i, ch)| (at(i)..at(i + ch.len_utf8()), ch))
                .collect(),
            _ => (bytes.iter().enumerate())
                .filter_map(|(i, &byte)| {
                    let ch = encoding.decode(&[byte]).chars().next()?;
                    Some((at(i)..at(i + 1), ch))
                })
                .collect(),
        }
    }

    #[inline]
    pub fn stream(&self) -> &RxStream {
        &self.stream
    }

    /// The bytes of `range` still kept
    pub fn bytes(&self, range: Range<u64>) -> Vec<u8> {
        let (front, back) = self.stream.slices(range);
        [front, back].concat()
    }

    pub fn bounds(&self) -> DumpBounds {
        DumpBounds {
            start: self.stream.start(),
            end: self.stream.end(),
            width: self.scroller.width,
            rows: self.scroller.rows,
        }
    }

    /// The top line once drawn `width` bytes a line on `rows` rows
    pub fn top_line(&mut self, width: usize, rows: usize) -> u64 {
        self.scroller.width = width;
        self.scroller.rows = rows;
        let (start, end) = self.span();
        self.scroller.top_line(start, end)
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let (start, end) = self.span();
        self.scroller.scroll_up(lines, start, end);
    }

    /// The lines a page scrolls: the rows shown but one
    pub fn page(&self) -> usize {
        self.scroller.rows.saturating_sub(1).max(1)
    }

    pub fn scroll_down(&mut self, lines: usize) {
        let (start, end) = self.span();
        self.scroller.scroll_down(lines, start, end);
    }

    /// The top line at `position` of the scrollbar, 0 the first line kept
    pub fn scroll_to(&mut self, position: usize) {
        let (start, end) = self.span();
        let (first, _) = self.scroller.bounds(start, end);
        self.scroller.scroll_to(first + position as u64, start, end);
    }

    /// The scrollbar once drawn: its positions past the first (the live view's) and the one
    /// shown, both from the first line kept
    pub fn scroll_position(&self) -> (usize, usize) {
        let (start, end) = self.span();
        let (first, live) = self.scroller.bounds(start, end);
        let top = self.scroller.top_line(start, end);
        ((live - first) as usize, (top - first) as usize)
    }

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

    /// The pan showing the cursor's byte in a body `cols` wide, when it's off the panel
    pub fn pan_to_cursor(&mut self, view: RawSideView, cols: usize) {
        self.pan_to(self.copy.cursor, view, cols);
    }

    /// The pan showing byte `offset` in a body `cols` wide, when it's off the panel
    pub fn pan_to(&mut self, offset: u64, view: RawSideView, cols: usize) {
        let width = self.scroller.width.max(1);
        let byte = (offset % width as u64) as usize;
        self.pan.sync_with_cursor(view, cols, byte, width);
    }

    #[inline]
    pub fn copy(&self) -> &DumpCopy {
        &self.copy
    }

    /// Copy mode as it was, given back (a search cancelled)
    pub fn set_copy(&mut self, copy: DumpCopy) {
        self.copy = copy;
    }

    /// Copy mode coming in: its cursor on the last byte of the bottom line shown; false
    /// without bytes
    pub fn start_copy(&mut self) -> bool {
        let bounds = self.bounds();
        if bounds.end == bounds.start {
            return false;
        }
        let top = self.scroller.top_line(bounds.start, bounds.end);
        let bottom = (top + bounds.rows.max(1) as u64) * bounds.width.max(1) as u64;
        self.copy = DumpCopy::at(bottom.min(bounds.end) - 1);
        true
    }

    /// The cursor on `offset` (a search's match), the selection kept, the dump scrolled to it
    pub fn cursor_to(&mut self, offset: u64) {
        self.copy.cursor = offset;
        self.reveal_cursor();
    }

    /// A copy mode action, the mouse's included; a move scrolls the dump to the cursor
    pub fn copy_action(&mut self, action: CopyModeAction) -> DumpResult {
        let bounds = self.bounds();
        let result = match action {
            CopyModeAction::MousePress(pos, _, _) => {
                self.copy.press(pos.byte_idx as u64, &bounds);
                DumpResult::None
            }
            CopyModeAction::MouseDrag(pos, _) => {
                self.copy.drag(pos.byte_idx as u64, &bounds);
                DumpResult::None
            }
            CopyModeAction::MouseRelease => self.copy.release(&bounds),
            action => self.copy.apply(action, &bounds),
        };
        if result == DumpResult::Moved {
            self.reveal_cursor();
        }
        result
    }

    /// The text a yank of `bytes` copies: the bytes `as_received` (read as UTF-8), else as the
    /// panel shows them in `view`
    pub fn yank_text(&self, bytes: Range<u64>, as_received: bool, view: RawSideView) -> String {
        let bytes = self.bytes(bytes);
        if as_received {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            format_raw_bytes_to_plain_string(&bytes, raw_view_to_display_mode(view))
        }
    }

    /// Scrolls the fewest lines that show the cursor
    fn reveal_cursor(&mut self) {
        self.reveal(self.copy.cursor);
    }

    /// Keeps byte `offset` in the middle half of the lines shown: nothing moves while it's there,
    /// else the dump centers it (MAX-256: following the other panel's cursor, calmer than the
    /// fewest lines, which leave it on an edge)
    pub fn keep_in_middle(&mut self, offset: u64) {
        let (start, end) = self.span();
        let line = offset / self.scroller.width.max(1) as u64;
        let rows = self.scroller.rows.max(1) as u64;
        let top = self.scroller.top_line(start, end);
        let margin = rows / 4;
        if !(top + margin..top + rows - margin).contains(&line) {
            (self.scroller).scroll_to(line.saturating_sub(rows / 2), start, end);
        }
    }

    /// Scrolls the fewest lines that show byte `offset`
    pub fn reveal(&mut self, offset: u64) {
        let (start, end) = self.span();
        let line = offset / self.scroller.width.max(1) as u64;
        self.scroller.reveal(line, start, end);
    }

    fn span(&self) -> (u64, u64) {
        (self.stream.start(), self.stream.end())
    }
}

/// The bytes kept (`start..end`) and the dump's shape as last drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpBounds {
    pub start: u64,
    pub end: u64,
    pub width: usize,
    pub rows: usize,
}

impl DumpBounds {
    fn width(&self) -> u64 {
        self.width.max(1) as u64
    }

    /// The offset brought onto a byte kept
    fn clamp(&self, offset: u64) -> u64 {
        offset.clamp(self.start, self.end.saturating_sub(1).max(self.start))
    }

    /// The bytes of `offset`'s line still kept
    fn line_of(&self, offset: u64) -> Range<u64> {
        let from = offset - offset % self.width();
        self.clamp(from)..self.clamp(from + self.width() - 1) + 1
    }
}
