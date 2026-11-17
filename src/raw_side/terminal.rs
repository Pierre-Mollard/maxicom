//! Terminal mode's raw panel (MAX-252): every byte received, shown as a dump like `xxd`'s.
//! Line `n` holds the bytes `n * width..(n + 1) * width` by their absolute offset, so the lines
//! are only arithmetic and stay put when the oldest bytes are dropped

mod copy;
mod scroll;
mod stream;

use std::ops::Range;

use chrono::{DateTime, Local};

pub use copy::{DumpCopy, DumpResult};
use scroll::DumpScroller;
pub use stream::RxStream;

use crate::{
    configs::{RawSideView, settings::raw_view_to_display_mode},
    inputs::CopyModeAction,
    raw_side::RawSideScroller,
    ui::raw_side::format_raw_bytes_to_plain_string,
};

/// The dump: the bytes kept, its own scroll and pan, and copy mode's cursor in it
#[derive(Debug)]
pub struct Dump {
    stream: RxStream,
    scroller: DumpScroller,
    pan: RawSideScroller,
    copy: DumpCopy,
}

impl Dump {
    /// The dump of a terminal history of `history_rows` rows
    pub fn new(history_rows: usize) -> Self {
        Self {
            stream: RxStream::new(history_rows),
            scroller: DumpScroller::default(),
            pan: RawSideScroller::default(),
            copy: DumpCopy::default(),
        }
    }

    pub fn push(&mut self, bytes: &[u8], time: Option<DateTime<Local>>) {
        self.stream.push(bytes, time);
    }

    /// Every byte dropped, the offsets still counting on; the dump follows the live end
    pub fn clear(&mut self) {
        self.stream.clear();
        self.scroller.follow();
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
        let width = self.scroller.width.max(1);
        let byte = (self.copy.cursor % width as u64) as usize;
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
        let (start, end) = self.span();
        let line = self.copy.cursor / self.scroller.width.max(1) as u64;
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
