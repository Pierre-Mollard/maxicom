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
mod tests {
    use super::*;

    /// A space every 50 ms, at once first, never two for a late turn, until 200 ms.
    #[test]
    fn paced_until_the_timeout() {
        let start = Instant::now();
        let ms = |ms| start + Duration::from_millis(ms);
        let every = Duration::from_millis(50);
        let timeout = Duration::from_millis(200);
        let mut mash = Mash::new(b" ".to_vec(), b"=> ", every, timeout, start).unwrap();
        assert_eq!(mash.step(start), MashStep::Send(b" "));
        assert_eq!(mash.step(ms(49)), MashStep::Wait);
        assert_eq!(mash.step(ms(50)), MashStep::Send(b" "));
        // a late turn: one send, the next 50 ms after it
        assert_eq!(mash.step(ms(170)), MashStep::Send(b" "));
        assert_eq!(mash.step(ms(199)), MashStep::Wait);
        assert_eq!(mash.step(ms(200)), MashStep::TimedOut);
        assert_eq!(mash.deadline(), ms(200));
    }

    /// The text waited for is matched across chunks, and shown printable.
    #[test]
    fn until_seen() {
        let now = Instant::now();
        let second = Duration::from_secs(1);
        assert!(Mash::new(b" ".to_vec(), b"", second, second, now).is_none());
        let mut mash = Mash::new(b"\x03".to_vec(), b"=> ", second, second, now).unwrap();
        assert!(!mash.until_seen(b"autoboot in 1\r\n="));
        assert!(mash.until_seen(b"> "));
        let mash = Mash::new(b" ".to_vec(), b"\r\n#", second, second, now).unwrap();
        assert_eq!(mash.until_text(), "\\r\\n#");
    }
}
