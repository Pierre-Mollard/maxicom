use crate::serial::{SerialDevice, SerialError};
use std::fs::File;
use std::io::Read;
use std::time::Instant;

pub enum ByteSource {
    Live(SerialDevice),
    Replay {
        file: File,
        delay: std::time::Duration,
        last_chunk: std::time::Instant,
        finished: bool,
    },
}

impl ByteSource {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, SerialError> {
        match self {
            ByteSource::Live(dev) => dev.read(buf),
            ByteSource::Replay {
                file,
                delay,
                last_chunk,
                finished,
            } => {
                if *finished || last_chunk.elapsed() < *delay {
                    return Ok(0);
                }

                if buf.is_empty() {
                    return Ok(0);
                }

                match file.read(buf) {
                    Ok(0) => {
                        *finished = true;
                        Ok(0)
                    }
                    Ok(n) => {
                        *last_chunk = Instant::now();
                        Ok(n)
                    }
                    Err(e) => Err(SerialError::ReadFailed(e)),
                }
            }
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), SerialError> {
        match self {
            ByteSource::Live(dev) => dev.write(bytes),
            ByteSource::Replay { .. } => Ok(()),
        }
    }
}
