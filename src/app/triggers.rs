//! Triggers: a chain run when a text comes in (MAX-6)
//!
//! The text is matched on the received bytes as they come, not on finished lines: U-Boot
//! redraws its countdown with `\b` and only ends the line when it runs out, a line matcher
//! would see it too late.

use memchr::memmem::Finder;

use crate::commands::Invocation;

/// A rule of `triggers.toml`, compiled: while it is armed, its chain runs when its text
/// comes in. Unless it rearms, it disarms when it fires
#[derive(Debug, Clone)]
pub struct Trigger {
    pub name: String,
    pub text: String,
    pub run: String,
    pub invocations: Vec<Invocation>,
    pub rearm: bool,
    pub silent: bool,
    /// The mark put on the line the text ended on (MAX-220)
    pub mark: Option<char>,
    /// How many times it fired, since it was loaded or reset (MAX-220)
    pub hits: u32,
    armed: bool,
    matcher: StreamMatcher,
}

impl Trigger {
    pub fn new(
        name: String,
        text: String,
        matcher: StreamMatcher,
        run: String,
        invocations: Vec<Invocation>,
        rearm: bool,
        armed: bool,
    ) -> Self {
        Self {
            name,
            text,
            run,
            invocations,
            rearm,
            silent: false,
            mark: None,
            hits: 0,
            armed,
            matcher,
        }
    }

    pub fn with_silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }

    pub fn with_mark(mut self, mark: Option<char>) -> Self {
        self.mark = mark;
        self
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Armed again, it starts from the next bytes: a text half seen before doesn't complete
    pub fn arm(&mut self, on: bool) {
        if on && !self.armed {
            self.matcher.reset();
        }
        self.armed = on;
    }

    /// Where in this chunk its text ends when it fires on it, past the text's last byte,
    /// counted; a disarmed one doesn't look at it
    pub fn fires(&mut self, chunk: &[u8]) -> Option<usize> {
        if !self.armed {
            return None;
        }
        let end = self.matcher.feed(chunk)?;
        self.hits = self.hits.saturating_add(1);
        if !self.rearm {
            self.armed = false;
            self.matcher.reset();
        }
        Some(end)
    }
}

/// Finds a text in a stream of chunks, one search per chunk. The last bytes of a chunk are
/// kept, one short of the text, for a text cut between two chunks: an appearance is only ever
/// found once, in the chunk where it ends
#[derive(Debug, Clone)]
pub struct StreamMatcher {
    finder: Finder<'static>,
    /// The end of the bytes seen, at most one short of the text
    tail: Vec<u8>,
}

impl StreamMatcher {
    /// `None` for an empty text, which would match anything
    pub fn new(text: &[u8]) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        Some(Self {
            finder: Finder::new(text).into_owned(),
            tail: Vec::with_capacity(text.len() - 1),
        })
    }

    /// Where the text ends in this chunk, past its last byte: its first end, once however
    /// many times it does
    pub fn feed(&mut self, chunk: &[u8]) -> Option<usize> {
        let len = self.finder.needle().len();
        let keep = len - 1;
        // across the cut: the tail and the start of the chunk, short of a whole text, so a
        // text found there ends in the chunk
        let seen = self.tail.len();
        self.tail.extend_from_slice(&chunk[..chunk.len().min(keep)]);
        let found = (self.finder.find(&self.tail).map(|at| at + len - seen))
            .or_else(|| self.finder.find(chunk).map(|at| at + len));
        if chunk.len() >= keep {
            self.tail.clear();
            self.tail.extend_from_slice(&chunk[chunk.len() - keep..]);
        } else {
            let extra = self.tail.len().saturating_sub(keep);
            self.tail.drain(..extra);
        }
        found
    }

    /// Forgets the bytes seen: a text half seen before doesn't complete with the next chunk
    pub fn reset(&mut self) {
        self.tail.clear();
    }
}

#[cfg(test)]
#[path = "../tests/app/triggers.rs"]
mod tests;
