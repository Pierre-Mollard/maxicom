use std::time::{Duration, Instant};

use crate::app::triggers::StreamMatcher;

/// `mash` (MAX-6): a text sent again and again, every so often, until another one comes in or
/// the time is up, for a bootloader that only listens for a moment after a reset
#[derive(Debug, Clone)]
pub struct Mash {
    bytes: Vec<u8>,
    until: StreamMatcher,
    until_text: String,
    every: Duration,
    next_at: Instant,
    deadline: Instant,
}

/// What a mash does at a given time
#[derive(Debug, PartialEq, Eq)]
pub enum MashStep<'a> {
    Wait,
    Send(&'a [u8]),
    TimedOut,
}

impl Mash {
    /// The first send is at once. `None` for an empty `until`, which would match anything
    pub fn new(
        bytes: Vec<u8>,
        until: &[u8],
        every: Duration,
        timeout: Duration,
        now: Instant,
    ) -> Option<Self> {
        Some(Self {
            bytes,
            until: StreamMatcher::new(until)?,
            until_text: String::from_utf8_lossy(until).escape_debug().to_string(),
            every,
            next_at: now,
            deadline: now + timeout,
        })
    }

    /// The text waited for, printable, for the notices
    pub fn until_text(&self) -> &str {
        &self.until_text
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn step(&mut self, now: Instant) -> MashStep<'_> {
        if now >= self.deadline {
            return MashStep::TimedOut;
        }
        if now < self.next_at {
            return MashStep::Wait;
        }
        // the next one from now: a late turn of the loop doesn't send a burst
        self.next_at = now + self.every;
        MashStep::Send(&self.bytes)
    }

    /// Whether the text waited for ends in these received bytes
    pub fn until_seen(&mut self, chunk: &[u8]) -> bool {
        self.until.feed(chunk)
    }
}

#[cfg(test)]
#[path = "../tests/app/mash.rs"]
mod tests;
