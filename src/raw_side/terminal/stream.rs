//! The bytes received in terminal mode (MAX-252)
//!
//! The terminal raw panel shows them as a hex dump, the same bytes on every line. Each byte has
//! an absolute offset, counted from the first byte ever received: dropping old bytes doesn't
//! change the others', so a dump line (offset / width) stays the same line. Kept with the
//! history: the bytes go with the rows they wrote, and a ceiling stops a program that redraws
//! without scrolling (`top`) from growing it forever

use std::{collections::VecDeque, ops::Range};

use chrono::{DateTime, Local};

/// The most bytes kept per terminal history row
pub const BYTES_PER_ROW: usize = 512;
/// The least bytes kept, for a history of a few rows
const MIN_CAPACITY: usize = 64 * 1024;

#[derive(Debug)]
pub struct RxStream {
    bytes: VecDeque<u8>,
    /// The offset of `bytes[0]`
    start: u64,
    /// Each read from the device, oldest first: the first kept one starts at `start`
    reads: VecDeque<Read>,
    capacity: usize,
}

/// A read from the device: its first byte's offset, and when it came (none when not known, a
/// restored session's bytes without their times)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    pub offset: u64,
    pub time: Option<DateTime<Local>>,
}

impl RxStream {
    /// A stream for a terminal history of `history_rows` rows
    pub fn new(history_rows: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            start: 0,
            reads: VecDeque::new(),
            capacity: history_rows.saturating_mul(BYTES_PER_ROW).max(MIN_CAPACITY),
        }
    }

    /// Adds a read received at `time`, the oldest bytes dropped past the ceiling; returns its
    /// first byte's offset
    pub fn push(&mut self, bytes: &[u8], time: Option<DateTime<Local>>) -> u64 {
        let offset = self.end();
        if bytes.is_empty() {
            return offset;
        }
        self.reads.push_back(Read { offset, time });
        self.bytes.extend(bytes);
        if let Some(over) = self.bytes.len().checked_sub(self.capacity) {
            self.drop_front(over);
        }
        offset
    }

    fn drop_front(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.bytes.drain(..count);
        self.start += count as u64;
        if self.bytes.is_empty() {
            self.reads.clear();
            return;
        }
        // the read holding the new first byte stays, starting there
        while self
            .reads
            .get(1)
            .is_some_and(|next| next.offset <= self.start)
        {
            self.reads.pop_front();
        }
        if let Some(read) = self.reads.front_mut() {
            read.offset = read.offset.max(self.start);
        }
    }

    pub fn clear(&mut self) {
        self.start = self.end();
        self.bytes.clear();
        self.reads.clear();
    }

    /// The first byte kept
    #[inline]
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Past the last byte received
    #[inline]
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    /// The bytes of `range` still kept, in one or two slices (the store is a ring)
    pub fn slices(&self, range: Range<u64>) -> (&[u8], &[u8]) {
        let local = |offset: u64| (offset.clamp(self.start, self.end()) - self.start) as usize;
        let from = local(range.start);
        let to = local(range.end).max(from);
        let (front, back) = self.bytes.as_slices();
        let split = front.len();
        (
            &front[from.min(split)..to.min(split)],
            &back[from.saturating_sub(split)..to.saturating_sub(split)],
        )
    }

    /// The reads with a byte kept, oldest first
    // TODO: (MAX-252) step 7, a mark at the read boundaries in the dump
    #[allow(dead_code)]
    #[inline]
    pub fn reads(&self) -> &VecDeque<Read> {
        &self.reads
    }
}

#[cfg(test)]
#[path = "../../tests/raw_side/terminal/stream.rs"]
mod tests;
