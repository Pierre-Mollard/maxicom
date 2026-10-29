//! What is captured of the serial data (MAX-152): the recordings and the session backup
//! (`recorder`), the session store (`session`), reading a session back (`restore`), the
//! terminal screenshots (`screenshot`), and the path helpers they share

mod recorder;
mod restore;
mod screenshot;
mod session;

// Re-exports
pub use recorder::{Recorder, Stream};
pub use restore::{
    JournalReader, RESTORE_RAW_TAIL, read_tail_bytes, read_tail_lines, restore_separator,
};
pub use screenshot::{Screenshot, ScreenshotFormat};
pub use session::{
    JournalText, Session, SessionDevice, SessionOrder, delete_session, delete_sessions,
    find_last_session, list_sessions, sort_sessions,
};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Creates the missing folders of `path`, so a file can be written there
pub fn create_parent_dirs(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// `~` at the start of a path is the home folder, as in a shell
pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

/// The port's last part, kept to the characters safe in a file name: "/dev/ttyUSB0" gives
/// "ttyUSB0"
pub fn session_tag(port: &str) -> String {
    let name = port.rsplit(['/', '\\']).next().unwrap_or(port);
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    //! The path helpers, and the helpers the capture tests and the other modules' tests share.

    use super::*;
    use crate::configs::{RxNewlineMode, TextEncoding};
    use crate::link::serial::SerialConfig;

    pub fn record(recorder: &mut Recorder, bytes: &[u8]) {
        recorder.record_rx(RxNewlineMode::Auto, TextEncoding::Utf8, None, bytes);
    }

    /// A device on `port`, on this machine.
    pub fn device(port: &str) -> SessionDevice {
        SessionDevice::new(
            &SerialConfig {
                port: port.to_string(),
                ..SerialConfig::default()
            },
            TextEncoding::Utf8,
        )
    }

    /// A fresh temp folder.
    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// A session journal written in `dir` on `port` the way a run would: `rx` received, then `tx`
    /// sent in line mode, and the session found back
    pub fn write_test_session(
        dir: &Path,
        port: &str,
        text: &JournalText,
        rx: &[u8],
        tx: &[u8],
    ) -> Session {
        let device = SessionDevice::new(
            &SerialConfig {
                port: port.to_string(),
                ..SerialConfig::default()
            },
            TextEncoding::Utf8,
        );
        let mut recorder = Recorder::default();
        recorder.start_journal(dir, &device, text, 20).unwrap();
        recorder.record_rx(RxNewlineMode::Auto, TextEncoding::Utf8, None, rx);
        if !tx.is_empty() {
            recorder.record_tx(TextEncoding::Utf8, None, None, tx);
        }
        drop(recorder);
        find_last_session(dir, &device, None).unwrap()
    }

    /// `~` is the home folder only as the first part of a path; the port tag keeps the last
    /// part of the port, safe for a file name.
    #[test]
    fn home_expansion_and_session_tag() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/logs/x.log")),
            home.join("logs/x.log")
        );
        assert_eq!(expand_home(Path::new("a/~/x")), Path::new("a/~/x"));
        assert_eq!(expand_home(Path::new("~x")), Path::new("~x"));
        assert_eq!(session_tag("/dev/ttyUSB0"), "ttyUSB0");
        assert_eq!(session_tag("COM3"), "COM3");
        // no ':' in a Windows file name (MAX-156)
        assert_eq!(session_tag("tcp://192.168.1.50:4000"), "192.168.1.50_4000");
        assert_eq!(session_tag("file:///tmp/my log.raw"), "my_log.raw");
    }
}
