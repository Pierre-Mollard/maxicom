use crate::config::{LineLogConfig, TerminalLogConfig};
use crate::types::LogEntry;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};

pub struct LineLogger {
    config: LineLogConfig,
    file: Option<File>,
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
}

pub struct TerminalLogger {
    config: TerminalLogConfig,
    file: Option<File>,
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

    pub fn log_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.write_all(bytes)?;
            file.flush()?;
        }
        Ok(())
    }
}
