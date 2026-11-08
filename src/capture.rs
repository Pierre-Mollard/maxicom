//! What is captured of the serial data (MAX-152): the recordings and the session backup
//! (`recorder`), the session store (`session`), reading a session back (`restore`), the
//! terminal screenshots (`screenshot`), the session browser's tree (`tree`)

mod recorder;
mod restore;
mod screenshot;
mod session;
mod tree;

// Re-exports
pub use recorder::{Recorder, RecordingWriter, Stream};
pub use restore::{
    RESTORE_RAW_TAIL, TimedChunk, group_separator, read_tail_bytes, read_tail_lines, read_times,
    restore_separator, timed_chunks,
};
pub use screenshot::{Screenshot, ScreenshotFormat};
pub use session::{
    CaptureDevice, CaptureFormat, CaptureLine, CaptureSource, PruneReport, RecordingMeta, Session,
    SessionLimits, TerminalSize, delete_session, delete_sessions, find_last_session, list_sessions,
    recording_raw,
};
pub use tree::{DEFAULT_GROUP_BY, GroupLevel, NodeKind, SessionNode, session_list, session_tree};

#[cfg(test)]
#[path = "tests/capture.rs"]
pub mod tests;
