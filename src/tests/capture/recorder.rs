//! Recordings: what the text and raw files get, batching and final flush.

use super::*;
use crate::capture::find_last_session;
use crate::capture::tests::{device, record, session_paths, source, temp_dir};

/// A fresh temp path for a recording named `name`: a text file, or a full recording's
/// folder. Apart from `temp_dir(name)`, which a test may use for its session backup
fn temp_log_path(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("maxicom_test_{}_{name}_rec", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// The file a recording of `stream` at `path` holds what it received in: the text file,
/// or the full recording's `.raw`
fn data_path(stream: Stream, path: &Path) -> PathBuf {
    match stream {
        Stream::Text => path.to_path_buf(),
        Stream::Full => folder_file(path, "raw"),
    }
}

/// Starts the stream to a fresh temp file or folder, gives the file it records in.
fn start(recorder: &mut Recorder, stream: Stream, name: &str) -> PathBuf {
    let path = temp_log_path(name);
    let source = source("port");
    match stream {
        Stream::Text => recorder.start_text(&path, false, source),
        Stream::Full => recorder.start_full(&path, false, source, &CaptureFormat::default()),
    }
    .unwrap();
    data_path(stream, &path)
}

/// Reads the stream's file after stopping it, and removes it (a full recording's folder).
fn stop_and_read(recorder: &mut Recorder, stream: Stream, path: &Path) -> Vec<u8> {
    recorder.stop();
    let bytes = std::fs::read(path).unwrap();
    match stream {
        Stream::Text => std::fs::remove_file(path).unwrap(),
        Stream::Full => std::fs::remove_dir_all(path.parent().unwrap()).unwrap(),
    }
    bytes
}

/// A line recorded right after start stays buffered, and `stop` writes it to the file.
/// How: reads the file before and after `stop` (well under FLUSH_INTERVAL).
#[test]
fn text_recording_buffers_until_stop() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Text, "buffered");
    record(&mut recorder, b"hello\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Text, &path),
        b"hello\n"
    );
}

/// Raw bytes are written as is, and dropping the recorder flushes them.
/// How: records bytes, drops the recorder, reads the file.
#[test]
fn raw_recording_flushes_on_drop() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Full, "dropped");
    record(&mut recorder, b"\x1b[31mraw\r\n");
    drop(recorder);
    assert_eq!(std::fs::read(&path).unwrap(), b"\x1b[31mraw\r\n");
    let _ = std::fs::remove_file(&path);
}

/// MAX-147: a text line is written without its own EOL, so CRLF and LF lines both end
/// with a single "\n" (it used to get its EOL and another "\n").
/// How: "abc\r\n" and "def\n" in one chunk.
#[test]
fn text_line_ends_with_one_newline() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Text, "eol");
    record(&mut recorder, b"abc\r\ndef\n");
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Text, &path),
        b"abc\ndef\n"
    );
}

/// MAX-147: the timestamp is `timestamp_format` as is, with no added brackets (the
/// default format, already bracketed, used to give "[[..]]").
/// How: the format "[ts]" has no strftime field, so the output is fixed.
#[test]
fn text_timestamp_is_the_format_as_is() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Text, "ts");
    recorder.record_rx(false, TextEncoding::Utf8, Some("[ts]"), b"abc\n");
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Text, &path),
        b"[ts] abc\n"
    );
}

/// MAX-147: the parsed timestamp format follows a changed `timestamp_format` (a reload),
/// and turning it off writes lines without a time; invalid UTF-8 is replaced, as before.
/// How: "[a]" then "[b]" then none, one line each, the last with a lone 0xFF byte.
#[test]
fn timestamp_format_change_is_parsed_again() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Text, "ts_change");
    let enc = TextEncoding::Utf8;
    recorder.record_rx(false, enc, Some("[a]"), b"one\n");
    recorder.record_rx(false, enc, Some("[b]"), b"two\n");
    recorder.record_rx(false, enc, None, b"\xFF\n");
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Text, &path),
        "[a] one\n[b] two\n\u{FFFD}\n".as_bytes()
    );
}

/// Only ended lines are written as text. The text a lone CR overwrites never is, and a
/// partial line still pending at stop is lost.
/// How: "old\rnew\n" then "tail" without EOL, in Auto mode.
#[test]
fn partial_and_overwritten_text_is_not_recorded() {
    let mut recorder = Recorder::default();
    let path = start(&mut recorder, Stream::Text, "partial");
    record(&mut recorder, b"old\rnew\ntail");
    assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"new\n");
}

/// A full recording's raw gets the bytes as received, its text gets them decoded.
/// How: Latin1 E9 and an escape sequence, a full recording on.
#[test]
fn raw_is_received_bytes_and_text_is_decoded() {
    let mut recorder = Recorder::default();
    let raw = start(&mut recorder, Stream::Full, "decoded");
    recorder.record_rx(false, TextEncoding::Latin1, None, b"\xE9\x1b[31mred\r\n");
    recorder.stop();
    let text = fs::read(raw.with_extension("log")).unwrap();
    assert_eq!(text, "é\x1b[31mred\n".as_bytes());
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Full, &raw),
        b"\xE9\x1b[31mred\r\n"
    );
}

/// MAX-175: one recording at a time: a text recording started mid-line gets the whole
/// line, and a full one started next takes its place.
/// How: "ab" with nothing on, text on, "c\n", full on, "d\n".
#[test]
fn one_recording_and_text_gets_the_whole_line() {
    let mut recorder = Recorder::default();
    record(&mut recorder, b"ab");
    let text = start(&mut recorder, Stream::Text, "apart_text");
    assert_eq!(recorder.recording(), Some(Stream::Text));
    record(&mut recorder, b"c\n");
    let raw = start(&mut recorder, Stream::Full, "apart_raw");
    assert_eq!(recorder.recording(), Some(Stream::Full));
    record(&mut recorder, b"d\n");
    assert_eq!(fs::read(&text).unwrap(), b"abc\n");
    let _ = fs::remove_file(&text);
    assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"d\n");
    assert_eq!(recorder.recording(), None);
}

/// MAX-147: the session backup gets everything from its start, text and raw, whatever
/// records, and a recording can start with its content.
/// How: backup in a temp folder, "a\n" received, a text recording including the session,
/// "b\n" received, then the backup files read back.
#[test]
fn session_records_apart_and_a_recording_can_start_with_it() {
    let dir = temp_dir("session");
    let mut recorder = Recorder::default();
    recorder
        .start_session(
            &dir,
            &device("port"),
            &CaptureFormat::default(),
            SessionLimits::keep(20),
        )
        .unwrap();
    assert!(recorder.has_session() && recorder.recording().is_none());
    record(&mut recorder, b"a\n");

    let path = dir.join("mine.log");
    recorder.start_text(&path, true, source("port")).unwrap();
    record(&mut recorder, b"b\n");
    assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"a\nb\n");

    drop(recorder);
    let files = session_paths(&dir);
    assert_eq!(files.len(), 4);
    assert!(files[0].to_string_lossy().ends_with("_port.log"));
    assert_eq!(fs::read(&files[0]).unwrap(), b"a\nb\n");
    assert!(files[1].to_string_lossy().ends_with("_port.meta"));
    assert_eq!(fs::read(&files[2]).unwrap(), b"a\nb\n");
    assert!(files[3].to_string_lossy().ends_with("_port.times"));
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-166: a paused session backup records nothing, a user recording still does, and the
/// session backup goes on in the same files once resumed; the line cut at the pause is dropped.
/// How: "a\n" and half a line received, the session backup paused, a text recording started,
/// "b\n" received, the session backup resumed, "c\n" received, then the files read back.
#[test]
fn paused_session_records_nothing() {
    let dir = temp_dir("session_paused");
    let mut recorder = Recorder::default();
    recorder
        .start_session(
            &dir,
            &device("port"),
            &CaptureFormat::default(),
            SessionLimits::keep(20),
        )
        .unwrap();
    record(&mut recorder, b"a\nhal");
    let session = recorder.take_session();
    assert!(session.is_some() && !recorder.has_session());
    let path = dir.join("mine.log");
    recorder.start_text(&path, true, source("port")).unwrap();
    record(&mut recorder, b"b\n");
    recorder.set_session(session);
    assert!(recorder.has_session());
    record(&mut recorder, b"c\n");
    assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"b\nc\n");

    drop(recorder);
    let read = |extension: &str| {
        let entry = session_paths(&dir)
            .into_iter()
            .find(|path| path.to_string_lossy().ends_with(extension))
            .unwrap();
        fs::read(entry).unwrap()
    };
    assert_eq!(read("_port.log"), b"a\nc\n");
    assert_eq!(read("_port.raw"), b"a\nhalc\n");
    let _ = fs::remove_dir_all(&dir);
}

/// A full recording can start with the backup's `.raw` while the session backup holds its
/// lock (Windows blocks reads from another handle), and the backup still appends after it.
/// MAX-175: its `.times` and `.log` start with the session's too, its offsets going on
/// from there.
/// How: "a\n" received, a full recording including the session, "b\n" received.
#[test]
fn full_recording_starts_with_the_locked_backup() {
    let dir = temp_dir("session_raw");
    let mut recorder = Recorder::default();
    recorder
        .start_session(
            &dir,
            &device("port"),
            &CaptureFormat::default(),
            SessionLimits::keep(20),
        )
        .unwrap();
    record(&mut recorder, b"a\n");

    let mine = temp_log_path("mine");
    let text = CaptureFormat::default();
    recorder
        .start_full(&mine, true, source("port"), &text)
        .unwrap();
    record(&mut recorder, b"b\n");
    recorder.stop();
    let read = |extension: &str| {
        fs::read_to_string(data_path(Stream::Full, &mine).with_extension(extension))
    };
    assert_eq!(read("raw").unwrap(), "a\nb\n");
    assert_eq!(read("log").unwrap(), "a\nb\n");
    let times = read("times").unwrap();
    let offsets: Vec<&str> = times
        .lines()
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(offsets, ["0", "2"], "{times:?}");
    // no device in the source: no meta
    assert!(read("meta").is_err());
    let _ = fs::remove_dir_all(&mine);

    drop(recorder);
    let backup = session_paths(&dir)
        .into_iter()
        .find(|path| path.to_string_lossy().ends_with("_port.raw"))
        .unwrap();
    assert_eq!(fs::read(backup).unwrap(), b"a\nb\n");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-175: the session backup writes where each received chunk starts in its `.raw` and
/// when, to its `.times`; a paused session writes none.
/// How: a session backup, 2 chunks received, paused for 1, then 1 more.
#[test]
fn session_writes_the_times_of_its_chunks() {
    let dir = temp_dir("times");
    let mut recorder = Recorder::default();
    recorder
        .start_session(
            &dir,
            &device("port"),
            &CaptureFormat::default(),
            SessionLimits::keep(20),
        )
        .unwrap();
    record(&mut recorder, b"ab");
    record(&mut recorder, b"cde");
    let session = recorder.take_session();
    record(&mut recorder, b"replayed");
    recorder.set_session(session);
    record(&mut recorder, b"f");
    drop(recorder);
    let times = session_paths(&dir)
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "times"))
        .unwrap();
    let text = fs::read_to_string(times).unwrap();
    let offsets: Vec<&str> = text.lines().map(|l| l.split(' ').next().unwrap()).collect();
    assert_eq!(offsets, ["0", "2", "5"], "{text:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-147: sent lines go to the text files (a full recording's, the backup's) as
/// `> line`, raw files get none of them.
/// How: a full recording and the backup on, "rx\n" received, then "a\r\nb\n\r\n" and a
/// lone ^C sent.
#[test]
fn tx_lines_go_to_text_files_only() {
    let dir = temp_dir("tx");
    let mut recorder = Recorder::default();
    let text = CaptureFormat {
        timestamp: None,
        tx_mark: Some("> ".to_string()),
    };
    recorder
        .start_session(&dir, &device("port"), &text, SessionLimits::keep(20))
        .unwrap();
    let full = temp_log_path("tx_full");
    recorder
        .start_full(&full, false, source("port"), &text)
        .unwrap();
    let raw = data_path(Stream::Full, &full);
    record(&mut recorder, b"rx\n");
    recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"a\r\nb\n\r\n");
    recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"\x03");

    let expected = b"rx\n> a\n> b\n> \n> \x03\n";
    recorder.stop();
    assert_eq!(fs::read(raw.with_extension("log")).unwrap(), expected);
    assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"rx\n");
    drop(recorder);
    let backup = session_paths(&dir)
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "log"))
        .unwrap();
    assert_eq!(fs::read(backup).unwrap(), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-148: the session backup keeps the text format it started with, whatever the recording
/// uses later, and records sent lines only when it started with a mark.
/// How: the session backup with "[j]" and "J ", the recording with "[r]" and "R ", then a TX
/// with the recording's TX off; a second session backup started without a mark.
#[test]
fn session_keeps_its_text_format() {
    let dir = temp_dir("session_format");
    let mut recorder = Recorder::default();
    let text = CaptureFormat {
        timestamp: Some("[j]".to_string()),
        tx_mark: Some("J ".to_string()),
    };
    recorder
        .start_session(&dir, &device("a"), &text, SessionLimits::keep(20))
        .unwrap();
    let rec = start(&mut recorder, Stream::Text, "session_format");
    let enc = TextEncoding::Utf8;
    recorder.record_rx(false, enc, Some("[r]"), b"rx\n");
    recorder.record_tx(enc, Some("[r]"), Some("R "), b"tx\n");
    recorder.record_tx(enc, None, None, b"off\n");
    assert_eq!(
        stop_and_read(&mut recorder, Stream::Text, &rec),
        b"[r] rx\n[r] R tx\n"
    );
    drop(recorder);
    let session = find_last_session(&dir, &device("a"), None).unwrap();
    assert_eq!(
        fs::read(session.text_path().unwrap()).unwrap(),
        b"[j] rx\n[j] J tx\n[j] J off\n"
    );

    let mut recorder = Recorder::default();
    recorder
        .start_session(
            &dir,
            &device("b"),
            &CaptureFormat::default(),
            SessionLimits::keep(20),
        )
        .unwrap();
    record(&mut recorder, b"rx\n");
    recorder.record_tx(enc, None, Some("> "), b"tx\n");
    drop(recorder);
    let session = find_last_session(&dir, &device("b"), None).unwrap();
    assert_eq!(fs::read(session.text_path().unwrap()).unwrap(), b"rx\n");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-147: a recording into folders that don't exist creates them.
#[test]
fn recording_creates_missing_folders() {
    let dir = temp_dir("folders");
    let path = dir.join("a").join("b").join("x");
    let mut recorder = Recorder::default();
    let text = CaptureFormat::default();
    recorder
        .start_full(&path, false, source("port"), &text)
        .unwrap();
    record(&mut recorder, b"x");
    let raw = data_path(Stream::Full, &path);
    assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"x");
    let _ = fs::remove_dir_all(&dir);
}
