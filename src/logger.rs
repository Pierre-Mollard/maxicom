use crate::app::types::RxLineSplitter;
use crate::configs::{RxNewlineMode, TextEncoding};
use chrono::{DateTime, Local};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Buffered log file: writes are batched in memory and flushed at most every `FLUSH_INTERVAL`
/// (on write or on `flush_if_due` from the main loop), on stop and on drop
struct LogFile {
    writer: BufWriter<File>,
    last_flush: Instant,
}

impl LogFile {
    fn open(path: &PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            last_flush: Instant::now(),
        })
    }

    fn flush_if_due(&mut self) -> io::Result<()> {
        if self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.last_flush = Instant::now();
        self.writer.flush()
    }
}

/// What a recording saves: the received lines as text, or the received bytes as is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Text,
    Raw,
}

impl Stream {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Raw => "raw",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Text => Self::Raw,
            Self::Raw => Self::Text,
        }
    }
}

/// Recordings of what is received, fed whatever the mode shows: text and raw are started and
/// stopped apart, each to its own file (MAX-147). The lines are cut even when nothing records
/// text, so a recording started mid-line still gets that whole line
#[derive(Default)]
pub struct Recorder {
    text: Option<LogFile>,
    raw: Option<LogFile>,
    lines: RxLineSplitter,
}

impl Recorder {
    fn slot(&mut self, stream: Stream) -> &mut Option<LogFile> {
        match stream {
            Stream::Text => &mut self.text,
            Stream::Raw => &mut self.raw,
        }
    }

    pub fn start(&mut self, stream: Stream, path: PathBuf) -> io::Result<()> {
        *self.slot(stream) = Some(LogFile::open(&path)?);
        Ok(())
    }

    pub fn stop(&mut self, stream: Stream) {
        if let Some(mut file) = self.slot(stream).take() {
            let _ = file.flush();
        }
    }

    pub fn is_active(&self, stream: Stream) -> bool {
        match stream {
            Stream::Text => self.text.is_some(),
            Stream::Raw => self.raw.is_some(),
        }
    }

    pub fn is_any_active(&self) -> bool {
        self.text.is_some() || self.raw.is_some()
    }

    pub fn flush_if_due(&mut self) {
        for file in [&mut self.text, &mut self.raw].into_iter().flatten() {
            let _ = file.flush_if_due();
        }
    }

    /// Records received bytes: as is to the raw file, and each line they end, without its
    /// EOL, to the text file, after its first byte's time when `timestamp_format` is given
    pub fn record_rx(
        &mut self,
        rx_eol_mode: RxNewlineMode,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        bytes: &[u8],
    ) {
        if let Some(file) = &mut self.raw {
            let _ = file.writer.write_all(bytes);
            let _ = file.flush_if_due();
        }

        let now = Local::now();
        for &b in bytes {
            if self.lines.push(b, rx_eol_mode, now) {
                let timestamp = self.lines.take_timestamp();
                if let Some(file) = &mut self.text {
                    let line = encoding.decode(self.lines.line());
                    let _ = write_line(file, &line, timestamp, timestamp_format);
                }
                self.lines.clear();
            }
        }
        if let Some(file) = &mut self.text {
            let _ = file.flush_if_due();
        }
    }
}

fn write_line(
    file: &mut LogFile,
    line: &str,
    timestamp: DateTime<Local>,
    timestamp_format: Option<&str>,
) -> io::Result<()> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    match timestamp_format {
        Some(format) => writeln!(file.writer, "{} {line}", timestamp.format(format)),
        None => writeln!(file.writer, "{line}"),
    }
}

#[cfg(test)]
mod tests {
    //! Recordings: what the text and raw files get, batching and final flush.

    use super::*;

    fn temp_log_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn record(recorder: &mut Recorder, bytes: &[u8]) {
        recorder.record_rx(RxNewlineMode::Auto, TextEncoding::Utf8, None, bytes);
    }

    /// Starts the stream to a fresh temp file.
    fn start(recorder: &mut Recorder, stream: Stream, name: &str) -> PathBuf {
        let path = temp_log_path(name);
        recorder.start(stream, path.clone()).unwrap();
        path
    }

    /// Reads the stream's file after stopping it, and removes it.
    fn stop_and_read(recorder: &mut Recorder, stream: Stream, path: &PathBuf) -> Vec<u8> {
        recorder.stop(stream);
        let bytes = std::fs::read(path).unwrap();
        let _ = std::fs::remove_file(path);
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
        let path = start(&mut recorder, Stream::Raw, "dropped");
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
        recorder.record_rx(
            RxNewlineMode::Auto,
            TextEncoding::Utf8,
            Some("[ts]"),
            b"abc\n",
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &path),
            b"[ts] abc\n"
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

    /// Raw gets the bytes as received, text gets them decoded.
    /// How: Latin1 E9 and an escape sequence, both streams on.
    #[test]
    fn raw_is_received_bytes_and_text_is_decoded() {
        let mut recorder = Recorder::default();
        let text = start(&mut recorder, Stream::Text, "dec_text");
        let raw = start(&mut recorder, Stream::Raw, "dec_raw");
        recorder.record_rx(
            RxNewlineMode::Auto,
            TextEncoding::Latin1,
            None,
            b"\xE9\x1b[31mred\r\n",
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Raw, &raw),
            b"\xE9\x1b[31mred\r\n"
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &text),
            "é\x1b[31mred\n".as_bytes()
        );
    }

    /// Text and raw start and stop apart, and a text recording started mid-line gets the
    /// whole line.
    /// How: "ab" with nothing on, text on, "c\n", raw on, "d\n".
    #[test]
    fn streams_start_apart_and_text_gets_the_whole_line() {
        let mut recorder = Recorder::default();
        record(&mut recorder, b"ab");
        let text = start(&mut recorder, Stream::Text, "apart_text");
        assert!(recorder.is_active(Stream::Text) && !recorder.is_active(Stream::Raw));
        record(&mut recorder, b"c\n");
        let raw = start(&mut recorder, Stream::Raw, "apart_raw");
        record(&mut recorder, b"d\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Raw, &raw), b"d\n");
        assert!(recorder.is_active(Stream::Text));
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &text),
            b"abc\nd\n"
        );
        assert!(!recorder.is_active(Stream::Text));
    }
}
