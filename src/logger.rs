use crate::config::{LineLogConfig, TerminalLogConfig};
use crate::types::LogEntry;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

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
        let path = self.config.path.clone();
        self.start_with_path(path)
    }

    pub fn start_with_path(&mut self, path: PathBuf) -> io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        self.file = Some(file);
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

    pub fn active_path(&self) -> &PathBuf {
        &self.config.path
    }

    pub fn log_entry(&mut self, entry: &LogEntry, timestamp_format: &str) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            if self.config.timestamp {
                writeln!(
                    file,
                    "[{}] {}",
                    entry.timestamp.format(timestamp_format),
                    entry.text
                )?;
            } else {
                writeln!(file, "{}", entry.text)?;
            }
            file.flush()?;
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
        let path = self.config.path.clone();
        self.start_with_path(path)
    }

    pub fn start_with_path(&mut self, path: PathBuf) -> io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        self.file = Some(file);
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

    pub fn active_path(&self) -> &PathBuf {
        &self.config.path
    }

    pub fn log_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.write_all(bytes)?;
            file.flush()?;
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
