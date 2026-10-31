use std::{
    io::{self, BufRead},
    time::{Duration, Instant},
};

/// The most a chunk carries after a pause (a slow loop turn, the end of a line delay): what the
/// line drains in this long, so the link's buffers never fill
const MAX_BURST: Duration = Duration::from_millis(100);

/// A file sent paced (MAX-12): one chunk per loop turn, what the line drains at its rate since
/// the last one (`LinkConfig::bytes_per_sec`), read from the file as it goes, so the UI stays
/// live and the file can be any size. With a line delay it waits that long after each `\n`,
/// for a device without flow control that handles a line at a time
pub struct FileTransfer {
    reader: Box<dyn BufRead + Send>,
    name: String,
    size: u64,
    sent: u64,
    bytes_per_sec: f64,
    line_delay: Duration,
    /// When the last chunk went, or the start
    last: Instant,
    /// The end of the line delay running
    resume_at: Option<Instant>,
}

impl std::fmt::Debug for FileTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTransfer")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("sent", &self.sent)
            .finish_non_exhaustive()
    }
}

impl FileTransfer {
    pub fn new(
        reader: Box<dyn BufRead + Send>,
        name: String,
        size: u64,
        bytes_per_sec: f64,
        line_delay: Duration,
        now: Instant,
    ) -> Self {
        Self {
            reader,
            name,
            size,
            sent: 0,
            bytes_per_sec,
            line_delay,
            last: now,
            resume_at: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// How much is sent, in percent; 100 for an empty file
    pub fn percent(&self) -> u64 {
        (self.sent * 100).checked_div(self.size).unwrap_or(100)
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
mod tests {
    use super::*;

    /// At `baudrate` in 8N1: 10 bits a byte
    fn transfer(content: &[u8], baudrate: u32, line_delay_ms: u64, start: Instant) -> FileTransfer {
        FileTransfer::new(
            Box::new(io::Cursor::new(content.to_vec())),
            "test.txt".to_string(),
            content.len() as u64,
            f64::from(baudrate) / 10.0,
            Duration::from_millis(line_delay_ms),
            start,
        )
    }

    fn chunk(transfer: &mut FileTransfer, at: Instant) -> Option<Vec<u8>> {
        transfer.next_chunk(at).unwrap()
    }

    /// At 9600 baud (960 bytes a second) a chunk is what drained since the last one, at most
    /// 100 ms of it after a pause; the end of the file ends the transfer.
    #[test]
    fn paced_to_the_baud_rate() {
        let start = Instant::now();
        let ms = |ms| start + Duration::from_millis(ms);
        let mut transfer = transfer(&[b'x'; 200], 9600, 0, start);
        assert_eq!(chunk(&mut transfer, start), Some(vec![]));
        assert_eq!(chunk(&mut transfer, ms(50)).unwrap().len(), 48);
        assert_eq!(chunk(&mut transfer, ms(51)), Some(vec![]));
        assert_eq!(chunk(&mut transfer, ms(10_000)).unwrap().len(), 96);
        assert_eq!(transfer.percent(), 72);
        assert_eq!(chunk(&mut transfer, ms(20_000)).unwrap().len(), 56);
        assert_eq!(transfer.percent(), 100);
        assert_eq!(chunk(&mut transfer, ms(30_000)), None);
    }

    /// With a line delay, a chunk ends after a `\n` and the next waits the delay.
    #[test]
    fn line_delay() {
        let start = Instant::now();
        let ms = |ms| start + Duration::from_millis(ms);
        let mut transfer = transfer(b"ab\ncd\nef", 115_200, 100, start);
        assert_eq!(chunk(&mut transfer, ms(50)), Some(b"ab\n".to_vec()));
        assert_eq!(chunk(&mut transfer, ms(149)), Some(vec![]));
        assert_eq!(chunk(&mut transfer, ms(150)), Some(vec![]));
        assert_eq!(chunk(&mut transfer, ms(200)), Some(b"cd\n".to_vec()));
        assert_eq!(chunk(&mut transfer, ms(400)), Some(b"ef".to_vec()));
        assert_eq!(chunk(&mut transfer, ms(500)), None);
    }

    #[test]
    fn empty_file() {
        let start = Instant::now();
        let mut transfer = transfer(b"", 9600, 0, start);
        assert_eq!(transfer.percent(), 100);
        assert_eq!(chunk(&mut transfer, start + Duration::from_secs(1)), None);
    }
}
