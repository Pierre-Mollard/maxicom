use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

//TODO: move this module around when adding terminal stream saving feature
use crate::app::line_mode::LogEntry; // this is ugly deps i find

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
    pub fn log_entry(&mut self, entry: &LogEntry) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            if self.config.timestamp_lines {
                writeln!(file, "[{}] {}", entry.formatted_time(), entry.text)?;
            } else {
                writeln!(file, "{}", entry.text)?;
            }
            file.flush()?;
        }
        Ok(())
    }

    /// Logs raw binary bytes (for hex dump / raw logging)
    pub fn log_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        // TODO: use these and use another file format, add screenshot
        if let Some(file) = &mut self.file {
            file.write_all(bytes)?;
            file.flush()?;
        }
        Ok(())
    }
}
