use std::{
    io::{self, BufRead},
    time::{Duration, Instant},
};

/// The most a chunk carries after a pause (a slow loop turn, the end of a line delay): what the
/// line drains in this long, so the link's buffers never fill
const MAX_BURST: Duration = Duration::from_millis(100);

/// A file sent as is, paced (MAX-12): one chunk per loop turn, what the line drains at its rate
/// since the last one (`LinkConfig::bytes_per_sec`), read from the file as it goes, so the UI
/// stays live and the file can be any size. With a line delay it waits that long after each
/// `\n`, for a device without flow control that handles a line at a time
pub struct RawSender {
    reader: Box<dyn BufRead + Send>,
    sent: u64,
    bytes_per_sec: f64,
    line_delay: Duration,
    /// When the last chunk went, or the start
    last: Instant,
    /// The end of the line delay running
    resume_at: Option<Instant>,
}

impl RawSender {
    pub fn new(
        reader: Box<dyn BufRead + Send>,
        bytes_per_sec: f64,
        line_delay: Duration,
        now: Instant,
    ) -> Self {
        Self {
            reader,
            sent: 0,
            bytes_per_sec,
            line_delay,
            last: now,
            resume_at: None,
        }
    }

    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// The bytes to send now: empty while it isn't time (the line hasn't drained enough, a line
    /// delay runs), `None` once the file is all sent
    pub fn next_chunk(&mut self, now: Instant) -> io::Result<Option<Vec<u8>>> {
        if let Some(resume_at) = self.resume_at {
            if now < resume_at {
                return Ok(Some(Vec::new()));
            }
            self.resume_at = None;
            self.last = resume_at;
        }
        let elapsed = now.saturating_duration_since(self.last).min(MAX_BURST);
        let budget = (elapsed.as_secs_f64() * self.bytes_per_sec) as usize;
        if budget == 0 {
            return Ok(Some(Vec::new()));
        }

        let mut chunk = Vec::with_capacity(budget);
        while chunk.len() < budget {
            let buf = self.reader.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            let mut part = &buf[..buf.len().min(budget - chunk.len())];
            let line_end = if self.line_delay.is_zero() {
                None
            } else {
                part.iter().position(|&byte| byte == b'\n')
            };
            if let Some(end) = line_end {
                part = &part[..=end];
            }
            chunk.extend_from_slice(part);
            let taken = part.len();
            self.reader.consume(taken);
            if line_end.is_some() {
                self.resume_at = Some(now + self.line_delay);
                break;
            }
        }
        if chunk.is_empty() {
            return Ok(None);
        }
        self.sent += chunk.len() as u64;
        self.last = now;
        Ok(Some(chunk))
    }
}

#[cfg(test)]
#[path = "../tests/transfer/raw.rs"]
mod tests;
