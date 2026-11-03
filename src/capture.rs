//! What is captured of the serial data (MAX-152): the recordings and the session backup
//! (`recorder`), the session store (`session`), reading a session back (`restore`), the
//! terminal screenshots (`screenshot`)

mod recorder;
mod restore;
mod screenshot;
mod session;

// Re-exports
pub use recorder::{Recorder, RecordingWriter, Stream};
pub use restore::{
    RESTORE_RAW_TAIL, TimedChunk, read_tail_bytes, read_tail_lines, read_times, restore_separator,
    timed_chunks,
};
pub use screenshot::{Screenshot, ScreenshotFormat};
pub use session::{
    CaptureDevice, CaptureFormat, CaptureLine, CaptureSource, RecordingMeta, Session,
    SessionLimits, SessionOrder, TerminalSize, delete_session, delete_sessions, find_last_session,
    list_sessions, recording_raw, sort_sessions,
};

#[cfg(test)]
#[path = "tests/capture.rs"]
pub mod tests;
