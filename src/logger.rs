use crate::app::types::LogEntry;
use crate::configs::{LineLogConfig, TerminalLogConfig};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Buffered log file: writes are batched in memory and flushed at most every `FLUSH_INTERVAL`
/// (on write or on `flush_if_due` from the main loop), on stop and on drop
struct LogFile {
    writer: BufWriter<File>,
    last_flush: Instant,
}

impl LogFile {
    fn open(path: &PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            last_flush: Instant::now(),
        })
    }

    fn flush_if_due(&mut self) -> io::Result<()> {
        if self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.last_flush = Instant::now();
        self.writer.flush()
    }
}

pub struct LineLogger {
    config: LineLogConfig,
    file: Option<LogFile>,
}

impl LineLogger {
    pub fn new(config: LineLogConfig) -> Self {
        let mut logger = Self { config, file: None };
        if logger.config.enabled {
            let _ = logger.start();
        }
        logger
    }

    pub fn start(&mut self) -> io::Result<()> {
        let path = self.config.path.clone();
        self.start_with_path(path)
    }

    pub fn start_with_path(&mut self, path: PathBuf) -> io::Result<()> {
        self.file = Some(LogFile::open(&path)?);
        self.config.path = path;
        self.config.enabled = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
        self.file = None;
        self.config.enabled = false;
    }

    pub fn is_active(&self) -> bool {
        self.file.is_some()
    }

    pub fn flush_if_due(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush_if_due(),
            None => Ok(()),
        }
    }

    pub fn active_path(&self) -> &PathBuf {
        &self.config.path
    }

    pub fn log_entry(&mut self, entry: &LogEntry, timestamp_format: &str) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            if self.config.timestamp {
                writeln!(
                    file.writer,
                    "[{}] {}",
                    entry.timestamp.format(timestamp_format),
                    entry.text
                )?;
            } else {
                writeln!(file.writer, "{}", entry.text)?;
            }
            file.flush_if_due()?;
        }
        Ok(())
    }
}

impl Drop for LineLogger {
    fn drop(&mut self) {
        if self.is_active() {
            self.stop();
        }
    }
}

pub struct TerminalLogger {
    config: TerminalLogConfig,
    file: Option<LogFile>,
}

impl TerminalLogger {
    pub fn new(config: TerminalLogConfig) -> Self {
        let mut logger = Self { config, file: None };
        if logger.config.enabled {
            let _ = logger.start();
        }
        logger
    }

    pub fn start(&mut self) -> io::Result<()> {
        let path = self.config.path.clone();
        self.start_with_path(path)
    }

    pub fn start_with_path(&mut self, path: PathBuf) -> io::Result<()> {
        self.file = Some(LogFile::open(&path)?);
        self.config.path = path;
        self.config.enabled = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
        self.file = None;
        self.config.enabled = false;
    }

    pub fn is_active(&self) -> bool {
        self.file.is_some()
    }

    pub fn flush_if_due(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush_if_due(),
            None => Ok(()),
        }
    }

    pub fn active_path(&self) -> &PathBuf {
        &self.config.path
    }

    pub fn log_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.writer.write_all(bytes)?;
            file.flush_if_due()?;
        }
        Ok(())
    }
}

impl Drop for TerminalLogger {
    fn drop(&mut self) {
        if self.is_active() {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Buffered log files: batching and final flush.

    use super::*;
    use chrono::Local;

    fn temp_log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("maxicom_test_{}_{name}.log", std::process::id()))
    }

    /// A line logged right after start stays buffered, and `stop` writes it to the file.
    /// How: reads the file before and after `stop` (well under FLUSH_INTERVAL).
    #[test]
    fn line_logger_buffers_until_stop() {
        let path = temp_log_path("line");
        let mut logger = LineLogger::new(LineLogConfig {
            enabled: false,
            path: path.clone(),
            timestamp: false,
        });
        logger.start().unwrap();
        let entry = LogEntry {
            text: "hello".to_string(),
            timestamp: Local::now(),
        };
        logger.log_entry(&entry, "%H").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");

        logger.stop();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
        let _ = std::fs::remove_file(&path);
    }

    /// Raw terminal bytes are written as is, and dropping the logger flushes them.
    /// How: writes bytes, drops the logger, reads the file.
    #[test]
    fn terminal_logger_flushes_on_drop() {
        let path = temp_log_path("term");
        let mut logger = TerminalLogger::new(TerminalLogConfig {
            enabled: false,
            path: path.clone(),
        });
        logger.start().unwrap();
        logger.log_raw_bytes(b"\x1b[31mraw\r\n").unwrap();
        drop(logger);
        assert_eq!(std::fs::read(&path).unwrap(), b"\x1b[31mraw\r\n");
        let _ = std::fs::remove_file(&path);
    }
}
