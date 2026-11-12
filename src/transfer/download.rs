use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use super::modem_rx::Event;

/// The file being written
struct Current {
    /// Its name once complete
    path: PathBuf,
    /// Where it's written meanwhile: `<path>.part`
    part: PathBuf,
    writer: BufWriter<File>,
    written: u64,
    mtime: Option<u64>,
}

/// Where a download's files go (MAX-162): the receiver's events applied to the disk. Each file
/// is written as `<name>.part`, renamed when complete, and never over an existing file: a
/// name taken gets `.1`, `.2`... A failed download leaves its `.part`
pub struct Download {
    folder: PathBuf,
    /// XMODEM's file name, as it sends none
    xmodem_name: Option<String>,
    current: Option<Current>,
    /// The files complete, with their sizes, not yet taken
    received: Vec<(PathBuf, u64)>,
    count: usize,
    /// The bytes of the files complete, for the summary at the end
    total: u64,
    /// The last file complete, with its size
    last: Option<(PathBuf, u64)>,
}

impl Download {
    pub fn new(folder: PathBuf, xmodem_name: Option<String>) -> Self {
        Self {
            folder,
            xmodem_name,
            current: None,
            received: Vec::new(),
            count: 0,
            total: 0,
            last: None,
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The file being written, else the XMODEM file to write
    pub fn name(&self) -> Option<String> {
        match &self.current {
            Some(current) => current.path.file_name().map(|n| n.to_string_lossy().into()),
            None => self.xmodem_name.clone(),
        }
    }

    /// The `.part` of the file being written, kept when the download stops
    pub fn partial(&self) -> Option<&Path> {
        self.current.as_ref().map(|current| current.part.as_path())
    }

    /// How many files are complete
    pub fn count(&self) -> usize {
        self.count
    }

    /// The bytes of the files complete
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The last file complete, with its size
    pub fn last(&self) -> Option<&(PathBuf, u64)> {
        self.last.as_ref()
    }

    /// The files completed since the last call, with their sizes
    pub fn take_received(&mut self) -> Vec<(PathBuf, u64)> {
        std::mem::take(&mut self.received)
    }

    pub fn apply(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::File { name, mtime, .. } => {
                self.open(&name, mtime)?;
            }
            Event::Data(data) => {
                let current = self.current_or_xmodem()?;
                current.writer.write_all(&data)?;
                current.written += data.len() as u64;
            }
            Event::FileEnd => {
                // an XMODEM file with no data still gets its (empty) file
                self.current_or_xmodem()?;
                if let Some(current) = self.current.take() {
                    self.finish(current)?;
                }
            }
        }
        Ok(())
    }

    /// The file being written; XMODEM's, opened at its first data
    fn current_or_xmodem(&mut self) -> io::Result<&mut Current> {
        if self.current.is_none() {
            let name = self
                .xmodem_name
                .clone()
                .unwrap_or_else(|| "received.bin".into());
            self.open(&name, None)?;
        }
        Ok(self.current.as_mut().expect("opened above"))
    }

    /// `name` in the folder, or `name.1`, `name.2`... when it or its `.part` exists
    fn open(&mut self, name: &str, mtime: Option<u64>) -> io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let mut index = 0;
        loop {
            let candidate = match index {
                0 => name.to_string(),
                index => format!("{name}.{index}"),
            };
            let path = self.folder.join(&candidate);
            let part = self.folder.join(format!("{candidate}.part"));
            if !path.exists() {
                // `create_new` fails when the `.part` exists: the next name then
                match File::create_new(&part) {
                    Ok(file) => {
                        self.current = Some(Current {
                            path,
                            part,
                            writer: BufWriter::new(file),
                            written: 0,
                            mtime,
                        });
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            index += 1;
        }
    }

    /// Flushed, dated as the sender said, renamed to its name
    fn finish(&mut self, current: Current) -> io::Result<()> {
        let file = current.writer.into_inner().map_err(|e| e.into_error())?;
        if let Some(mtime) = current.mtime {
            // a date the system can't set isn't worth failing the download for
            let _ = file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(mtime));
        }
        drop(file);
        fs::rename(&current.part, &current.path)?;
        self.received.push((current.path.clone(), current.written));
        self.last = Some((current.path, current.written));
        self.count += 1;
        self.total += current.written;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/transfer/download.rs"]
mod tests;
