//! The bytes received (MAX-252), whatever the modes fed (MAX-290)
//!
//! The terminal raw panel shows them as a hex dump, the API's `raw` reads them. Each byte has
//! an absolute offset, counted from the first byte received on this screen: dropping old bytes
//! doesn't change the others', so a dump line (offset / width) stays the same line. Up to
//! `rx_bytes`, the oldest bytes dropped past it

use std::{collections::VecDeque, ops::Range};

/// The least bytes kept, whatever `rx_bytes` says
const MIN_CAPACITY: usize = 64 * 1024;

#[derive(Debug)]
pub struct RxStream {
    bytes: VecDeque<u8>,
    /// The offset of `bytes[0]`
    start: u64,
    capacity: usize,
}

impl RxStream {
    /// A stream keeping `capacity` bytes at most (64 KiB at least)
    pub fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            start: 0,
            capacity: capacity.max(MIN_CAPACITY),
        }
    }

    /// Adds a read, the oldest bytes dropped past the ceiling; returns its first byte's offset
    pub fn push(&mut self, bytes: &[u8]) -> u64 {
        let offset = self.end();
        self.bytes.extend(bytes);
        if let Some(over) = self.bytes.len().checked_sub(self.capacity) {
            self.bytes.drain(..over);
            self.start += over as u64;
        }
        offset
    }

    pub fn clear(&mut self) {
        self.start = self.end();
        self.bytes.clear();
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

    /// The bytes of `range` still kept, joined
    pub fn bytes(&self, range: Range<u64>) -> Vec<u8> {
        let (front, back) = self.slices(range);
        [front, back].concat()
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
}

#[cfg(test)]
#[path = "tests/rx_stream.rs"]
mod tests;
