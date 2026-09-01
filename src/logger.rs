use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::Local;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    pub enabled: bool,
    pub path: PathBuf,
    pub timestamp_lines: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: PathBuf::from("maxicom.log"),
            timestamp_lines: true,
        }
    }
}

pub struct FileLogger {
    config: LogConfig,
    file: Option<File>,
}

impl FileLogger {
    pub fn new(config: LogConfig) -> Self {
        let mut logger = Self { config, file: None };
        if logger.config.enabled {
            let _ = logger.start();
        }
        logger
    }

    pub fn start(&mut self) -> io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.path)?;

        self.file = Some(file);
        self.config.enabled = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.file = None;
        self.config.enabled = false;
    }

    pub fn is_active(&self) -> bool {
        self.file.is_some()
    }

    /// Logs a completed line of text received from the device
    pub fn log_line(&mut self, line: &str) -> io::Result<()> {
        let Some(file) = &mut self.file else {
            return Ok(());
        };

        if self.config.timestamp_lines {
            let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(file, "[{now}] {line}")?;
        } else {
            writeln!(file, "{line}")?;
        }

        // Flush immediately so crashing devices don't lose the final panic log in buffer
        file.flush()?;
        Ok(())
    }

    /// Logs raw binary bytes (for hex dump / raw logging)
    pub fn log_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.write_all(bytes)?;
            file.flush()?;
        }
        Ok(())
    }
}
