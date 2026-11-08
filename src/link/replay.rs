//! A raw capture played back as if a device sent it: one buffer every `delay`, then nothing.
//! It can pause, change its delay and start again from the top, and says how far it read
//! (MAX-166)
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};

use super::LinkError;

/// What the replay controls do to the replay playing (MAX-166)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayControl {
    Pause,
    Resume,
    Delay(Duration),
    Restart,
}

pub struct Replay {
    file: File,
    delay: Duration,
    last_chunk: Instant,
    finished: bool,
    paused: bool,
    read: u64,
    size: u64,
}

impl Replay {
    pub fn open(path: &Path, delay: Duration) -> io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            file,
            delay,
            last_chunk: Instant::now(),
            finished: false,
            paused: false,
            read: 0,
            size,
        })
    }

    /// A replay with nothing left to read: what it plays was loaded at once (MAX-202)
    pub fn loaded(path: &Path) -> io::Result<Self> {
        let mut replay = Self::open(path, Duration::ZERO)?;
        replay.finished = true;
        replay.read = replay.size;
        Ok(replay)
    }

    /// Whether the whole file was read (MAX-166)
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Nothing more is read until `resume`
    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// The wait between two buffers from now on
    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = delay;
    }

    /// Back to the top of the file, not finished, its pause and delay kept
    pub fn restart(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.read = 0;
        self.finished = false;
        self.last_chunk = Instant::now();
        Ok(())
    }

    /// The bytes read so far, and the file's size when it was opened
    pub fn progress(&self) -> (u64, u64) {
        (self.read, self.size)
    }

    /// How much of the file was read, in percent; a whole empty file is 100
    pub fn percent(&self) -> u64 {
        if self.size == 0 {
            return if self.finished { 100 } else { 0 };
        }
        (self.read.min(self.size) * 100) / self.size
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, LinkError> {
        if self.finished || self.paused || self.last_chunk.elapsed() < self.delay {
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
                self.read += n as u64;
                Ok(n)
            }
            Err(e) => Err(LinkError::ReadFailed(e)),
        }
    }
}

#[cfg(test)]
#[path = "../tests/link/replay.rs"]
mod tests;
