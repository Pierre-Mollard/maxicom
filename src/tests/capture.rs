//! The helpers the capture tests and the other modules' tests share.

use super::*;
use crate::configs::TextEncoding;
use crate::link::{Address, LinkConfig};
use std::fs;
use std::path::{Path, PathBuf};

pub fn record(recorder: &mut Recorder, bytes: &[u8]) {
    recorder.record_rx(false, TextEncoding::Utf8, None, bytes);
}

/// The files of every session in the session folder `dir`, sorted (MAX-175)
pub fn session_paths(dir: &Path) -> Vec<PathBuf> {
    let sessions = super::list_sessions(dir).unwrap();
    let mut paths: Vec<PathBuf> = sessions.into_iter().flat_map(|s| s.files).collect();
    paths.sort();
    paths
}

/// What a recording on `address` with no known device records (MAX-173)
pub fn source(address: &str) -> CaptureSource {
    CaptureSource {
        address: address.parse::<Address>().unwrap(),
        device: None,
    }
}

/// A device on `address`, on this machine.
pub fn device(address: &str) -> CaptureDevice {
    CaptureDevice::new(
        &LinkConfig {
            address: address.parse::<Address>().unwrap(),
            ..LinkConfig::default()
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

/// A session written in `dir` on `port` the way a run would: `rx` received, then `tx`
/// sent in line mode, and the session found back
pub fn write_test_session(
    dir: &Path,
    port: &str,
    text: &CaptureFormat,
    rx: &[u8],
    tx: &[u8],
) -> Session {
    let device = CaptureDevice::new(
        &LinkConfig {
            address: port.parse::<Address>().unwrap(),
            ..LinkConfig::default()
        },
        TextEncoding::Utf8,
    );
    let mut recorder = Recorder::default();
    recorder
        .start_session(dir, &device, text, SessionLimits::keep(20))
        .unwrap();
    recorder.record_rx(false, TextEncoding::Utf8, None, rx);
    if !tx.is_empty() {
        recorder.record_tx(TextEncoding::Utf8, None, None, tx);
    }
    drop(recorder);
    find_last_session(dir, &device, None).unwrap()
}
