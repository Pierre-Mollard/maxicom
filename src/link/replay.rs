//! A raw capture played back as if a device sent it: one buffer every `delay`, then nothing
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    time::{Duration, Instant},
};

use super::serial::SerialError;

pub struct Replay {
    file: File,
    delay: Duration,
    last_chunk: Instant,
    finished: bool,
}

impl Replay {
    pub fn open(path: &Path, delay: Duration) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            delay,
            last_chunk: Instant::now(),
            finished: false,
        })
    }

    /// Whether the whole file was read (MAX-166)
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, SerialError> {
        if self.finished || self.last_chunk.elapsed() < self.delay {
            return Ok(0);
        }

        if buf.is_empty() {
            return Ok(0);
        }

        match self.file.read(buf) {
            Ok(0) => {
                self.finished = true;
                Ok(0)
            }
            Ok(n) => {
                self.last_chunk = Instant::now();
                Ok(n)
            }
            Err(e) => Err(SerialError::ReadFailed(e)),
        }
    }
}
