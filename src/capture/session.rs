use crate::configs::TextEncoding;
use crate::link::Address;
use crate::link::{
    LinkConfig,
    serial::{Framing, LineSettings},
};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const SESSION_META_VERSION: u32 = 1;

/// How the text lines of a session or a full recording are written (MAX-148, MAX-175), taken
/// from the capture settings when it starts and kept in its meta file (the session browser
/// shows it): the timestamp format when it is on, and the mark before sent lines when they are
/// recorded. A reload changes the text recordings only, so one session never mixes two formats
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFormat {
    pub timestamp: Option<String>,
    pub tx_mark: Option<String>,
}

/// What a session or a full recording was recorded on (MAX-148, MAX-175): a session restores
/// only on the same machine, address, line settings and encoding, as bytes from another baud
/// or encoding are garbage. `address` is the link's, which gives its kind (MAX-156), written
/// `port` in the file as before; `line` its line settings when maxicom sets them (a serial
/// port, not a TCP link): a new kind of link never changes the file. In the file the line
/// settings sit at the top of `[device]`, as before
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureDevice {
    pub host: String,
    #[serde(rename = "port")]
    pub address: Address,
    #[serde(flatten)]
    pub line: Option<CaptureLine>,
    pub encoding: TextEncoding,
}

/// What a recording records (MAX-173): the link's address, and the device its meta says when
/// it is known (a replay of a plain file has none)
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureSource {
    pub address: Address,
    pub device: Option<CaptureDevice>,
}

impl CaptureSource {
    /// The line a text recording gets where the device changed to this one, its text decoded
    /// with `encoding` from there: `── device: ttyUSB1, 115200 8N1, UTF8 ──`, the line
    /// settings left out when unknown or none (MAX-173)
    pub fn marker(&self, encoding: TextEncoding) -> String {
        let line = (self.device.as_ref())
            .and_then(|device| device.line)
            .map(|line| format!("{} {}, ", line.baud, line.framing))
            .unwrap_or_default();
        format!("── device: {}, {line}{encoding} ──", self.address.tag())
    }
}

/// The line settings that change the bytes received: the baud rate and the framing. Flow
/// control doesn't, so it isn't here. In the file the framing's fields sit next to `baud`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureLine {
    pub baud: u32,
    #[serde(flatten)]
    pub framing: Framing,
}

impl CaptureLine {
    pub fn of(line: &LineSettings) -> Self {
        Self {
            baud: line.baud,
            framing: line.framing,
        }
    }
}

impl CaptureDevice {
    pub fn new(config: &LinkConfig, encoding: TextEncoding) -> Self {
        let line = config
            .address
            .kind()
            .has_line_settings()
            .then(|| CaptureLine::of(&config.line));
        Self {
            host: hostname(),
            address: config.address.clone(),
            line,
            encoding,
        }
    }

    /// Whether the bytes received on `config` decoded with `encoding` are the ones this device
    /// says: the same line settings and encoding. The address isn't compared (MAX-173)
    pub fn same_setup(&self, config: &LinkConfig, encoding: TextEncoding) -> bool {
        let line = (self.line.is_some()).then(|| CaptureLine::of(&config.line));
        self.line == line && self.encoding == encoding
    }

    /// Whether `line` is there exactly when the kind of link has line settings. serde reads
    /// a `line` it can't read whole (a missing or unknown field) as none, which would turn a
    /// broken serial meta into a link without line settings
    fn is_whole(&self) -> bool {
        self.line.is_some() == self.address.kind().has_line_settings()
    }
}

/// The terminal mode's grid size, rows and columns (MAX-178)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

/// The `.meta` file of a session or a full recording, written when it starts, and again when
/// the terminal grid changes size: a restore replays the bytes at the size they ended on
/// (MAX-178), none in a meta written before. A file of another `version` is read as none, and
/// never restores
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingMeta {
    pub version: u32,
    pub started: i64,
    pub device: CaptureDevice,
    pub text: CaptureFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalSize>,
}

impl RecordingMeta {
    pub fn new(started: DateTime<Local>, device: CaptureDevice, text: CaptureFormat) -> Self {
        Self {
            version: SESSION_META_VERSION,
            started: started.timestamp(),
            device,
            text,
            terminal: None,
        }
    }

    /// The meta file of a `.raw`: the same name with `.meta`, as a session's or a raw
    /// recording's (MAX-163)
    pub fn path_of(raw: &Path) -> PathBuf {
        raw.with_extension("meta")
    }

    /// The meta in `path`; none when missing, broken or of another version
    pub fn read(path: &Path) -> Option<Self> {
        fs::read_to_string(path)
            .ok()
            .and_then(|meta| toml::from_str::<Self>(&meta).ok())
            .filter(|meta| meta.version == SESSION_META_VERSION && meta.device.is_whole())
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let meta = toml::to_string(self).map_err(io::Error::other)?;
        fs::write(path, meta)
    }
}

/// How many sessions the session folder keeps, and up to what size all together (MAX-163):
/// `max_size` none for no limit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    pub keep: usize,
    pub max_size: Option<u64>,
}

#[cfg(test)]
impl SessionLimits {
    /// `keep` sessions, whatever their size
    pub const fn keep(keep: usize) -> Self {
        Self {
            keep,
            max_size: None,
        }
    }
}

/// The file with `extension` of the recording in `folder` (a session's, a full recording's):
/// its files are named after the folder, `<folder>/<name>.<extension>` (MAX-175)
pub fn folder_file(folder: &Path, extension: &str) -> PathBuf {
    let name = folder.file_name().unwrap_or_default().to_string_lossy();
    folder.join(format!("{name}.{extension}"))
}

/// The file of the session `stem` in the session folder `dir` with `extension`: each session
/// has a folder of its own, named after it (MAX-175)
pub fn session_file(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    folder_file(&dir.join(stem), extension)
}

/// The `.raw` a replay plays for `path` (MAX-175): a recording's folder (a session's, a full
/// recording's) gives the `.raw` named after it, a `.meta` the `.raw` beside it, a `.raw`
/// itself, with or without a meta (another tool's capture). Anything else isn't a recording, a
/// `.log` neither: its text can't be replayed as it was received
pub fn recording_raw(path: &Path) -> io::Result<PathBuf> {
    let not_one = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a recording (a .raw, its .meta or its folder)",
        )
    };
    if path.is_dir() {
        let raw = folder_file(path, "raw");
        return raw.is_file().then_some(raw).ok_or_else(not_one);
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("raw") => Ok(path.to_path_buf()),
        Some("meta") => Ok(path.with_extension("raw")),
        _ => Err(not_one()),
    }
}

/// Removes the empty sessions in `dir`, then the oldest so `limits.keep` are left and their
/// files together fit `limits.max_size`, never `current` nor a session still running (MAX-148,
/// MAX-163): those count in the size but stay. A session is the folder of one stem with its
/// `.log`, `.raw` and `.meta` files; the stem starts with its date so the names sort by age
pub fn prune_sessions(dir: &Path, limits: SessionLimits, current: &str) -> io::Result<()> {
    let mut sessions = session_files(dir)?;
    let removable = |stem: &str| stem != current && !is_running(&dir.join(stem));
    let remove = |stem: &str| fs::remove_dir_all(dir.join(stem));
    // a launch that received nothing leaves an empty session, which would fill `keep`
    sessions.retain(|stem, _| {
        let raw = session_file(dir, stem, "raw");
        let empty = fs::metadata(raw).map_or(true, |raw| raw.len() == 0);
        if !empty || !removable(stem) {
            return true;
        }
        let _ = remove(stem);
        false
    });
    let files_size = |paths: &[PathBuf]| -> u64 {
        paths
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    };
    let mut size: u64 = sessions.values().map(|paths| files_size(paths)).sum();
    let mut left = sessions.len();
    // the oldest first
    for (stem, paths) in &sessions {
        let too_many = left > limits.keep;
        let too_big = limits.max_size.is_some_and(|max| size > max);
        if !too_many && !too_big {
            break;
        }
        if removable(stem) {
            let freed = files_size(paths);
            if remove(stem).is_ok() {
                size = size.saturating_sub(freed);
                left -= 1;
            }
        }
    }
    Ok(())
}

/// The session files in `dir` by stem, which starts with the date so they sort by age: the
/// `.log`, `.raw`, `.meta` and `.times` files in the folder of each stem. A folder with none of them, and
/// the files in `dir` itself, aren't sessions
fn session_files(dir: &Path) -> io::Result<BTreeMap<String, Vec<PathBuf>>> {
    let mut sessions: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(stem) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let files: Vec<PathBuf> = ["log", "raw", "meta", "times"]
            .iter()
            .map(|extension| session_file(dir, &stem, extension))
            .filter(|path| path.is_file())
            .collect();
        if !files.is_empty() {
            sessions.insert(stem, files);
        }
    }
    Ok(sessions)
}

/// A session in the cache folder (MAX-148), what the restore and the session browser use: its
/// folder and files (MAX-175), its meta file when it has one (none before MAX-148), when it
/// started (from the meta, else from its name) and last received (the raw file's time, right
/// even after a crash), the size of its `.raw` and of all its files, and whether it is still
/// running, here or in another maxicom
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub stem: String,
    pub folder: PathBuf,
    pub files: Vec<PathBuf>,
    pub meta: Option<RecordingMeta>,
    pub started: Option<DateTime<Local>>,
    pub ended: Option<DateTime<Local>>,
    pub raw_len: u64,
    pub size: u64,
    pub running: bool,
}

/// The part of a session's name its address gives: the address's last part, kept to the
/// characters safe in a file name. "/dev/ttyUSB0" gives "ttyUSB0"; `Session::port_tag` reads
/// it back
pub fn session_tag(address: &Address) -> String {
    address.file_tag()
}

impl Session {
    /// The port's part of the name (`ttyUSB0`)
    pub fn port_tag(&self) -> &str {
        self.stem.splitn(3, '_').nth(2).unwrap_or(&self.stem)
    }

    pub fn text_path(&self) -> Option<&Path> {
        self.file("log")
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.file("raw")
    }

    /// When each chunk of the raw bytes was received (MAX-175)
    pub fn times_path(&self) -> Option<&Path> {
        self.file("times")
    }

    fn file(&self, extension: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == extension))
            .map(PathBuf::as_path)
    }

    /// When it started and ended, the end without its date on the same day
    pub fn time_span(&self) -> String {
        let day = |time: &DateTime<Local>| time.date_naive();
        let started = self.started.map(|t| t.format("%Y-%m-%d %H:%M").to_string());
        let ended = self.ended.map(|ended| match self.started {
            Some(started) if day(&started) == day(&ended) => ended.format("%H:%M").to_string(),
            _ => ended.format("%Y-%m-%d %H:%M").to_string(),
        });
        let or_unknown = |time: Option<String>| time.unwrap_or_else(|| "?".to_string());
        format!("{} → {}", or_unknown(started), or_unknown(ended))
    }
}

/// How the session browser orders the sessions: the newest first, or by port, then the newest
/// first
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionOrder {
    #[default]
    Date,
    Device,
}

impl SessionOrder {
    pub fn other(self) -> Self {
        match self {
            Self::Date => Self::Device,
            Self::Device => Self::Date,
        }
    }
}

/// The sessions in `dir`, the newest first. A missing folder has none
pub fn list_sessions(dir: &Path) -> io::Result<Vec<Session>> {
    let sessions = match session_files(dir) {
        Ok(sessions) => sessions,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut entries: Vec<_> = sessions
        .into_iter()
        .map(|(stem, files)| read_session(dir, stem, files))
        .collect();
    sort_sessions(&mut entries, SessionOrder::Date);
    Ok(entries)
}

fn read_session(dir: &Path, stem: String, files: Vec<PathBuf>) -> Session {
    let folder = dir.join(&stem);
    let meta = RecordingMeta::read(&folder_file(&folder, "meta"));
    let started = match &meta {
        Some(meta) => DateTime::from_timestamp(meta.started, 0).map(|t| t.with_timezone(&Local)),
        None => stem
            .get(..15)
            .and_then(|date| NaiveDateTime::parse_from_str(date, "%Y%m%d_%H%M%S").ok())
            .and_then(|date| Local.from_local_datetime(&date).earliest()),
    };
    let raw = fs::metadata(folder_file(&folder, "raw")).ok();
    let raw_len = raw.as_ref().map_or(0, |raw| raw.len());
    // all its files, as pruning counts them (MAX-175)
    let size = (files.iter())
        .filter_map(|path| fs::metadata(path).ok())
        .map(|file| file.len())
        .sum();
    Session {
        running: is_running(&folder),
        ended: raw.and_then(|raw| raw.modified().ok()).map(DateTime::from),
        raw_len,
        size,
        meta,
        started,
        stem,
        folder,
        files,
    }
}

/// Sorts the sessions in `order`. The names start with the date, so they sort by age
pub fn sort_sessions(entries: &mut [Session], order: SessionOrder) {
    match order {
        SessionOrder::Date => entries.sort_by(|a, b| b.stem.cmp(&a.stem)),
        SessionOrder::Device => entries.sort_by(|a, b| {
            a.port_tag()
                .cmp(b.port_tag())
                .then_with(|| b.stem.cmp(&a.stem))
        }),
    }
}

/// Deletes the folder of `entry`, refused while it is still running
pub fn delete_session(entry: &Session) -> io::Result<()> {
    if is_running(&entry.folder) {
        return Err(io::Error::other("the session is still running"));
    }
    fs::remove_dir_all(&entry.folder)
}

/// What deleting several sessions did: the sessions deleted and their size, the running ones
/// left, and the first error
#[derive(Debug, Default)]
pub struct DeleteReport {
    pub deleted: usize,
    pub size: u64,
    pub running: usize,
    pub error: Option<io::Error>,
}

/// Deletes every session of `entries` but the running ones
pub fn delete_sessions(entries: &[Session]) -> DeleteReport {
    let mut report = DeleteReport::default();
    for entry in entries {
        match delete_session(entry) {
            Ok(()) => {
                report.deleted += 1;
                report.size += entry.size;
            }
            Err(_) if entry.running => report.running += 1,
            Err(err) => {
                report.error.get_or_insert(err);
            }
        }
    }
    report
}

/// Whether the session in `folder` is still running, here or in another maxicom: its session
/// backup holds the lock on its `.raw`
fn is_running(folder: &Path) -> bool {
    File::open(folder_file(folder, "raw"))
        .is_ok_and(|raw| matches!(raw.try_lock(), Err(fs::TryLockError::WouldBlock)))
}

/// The newest session in `dir` recorded on `device` that received something, isn't running
/// and ended within `max_age` of now when given. To call before `start_session`, whose
/// pruning could remove it
pub fn find_last_session(
    dir: &Path,
    device: &CaptureDevice,
    max_age: Option<Duration>,
) -> Option<Session> {
    let is_recent = |ended: Option<DateTime<Local>>| {
        max_age.is_none_or(|max_age| {
            ended.is_some_and(|ended| {
                SystemTime::now()
                    .duration_since(ended.into())
                    .is_ok_and(|age| age <= max_age)
            })
        })
    };
    list_sessions(dir).ok()?.into_iter().find(|session| {
        session
            .meta
            .as_ref()
            .is_some_and(|meta| meta.device == *device)
            && session.raw_len > 0
            && !session.running
            && is_recent(session.ended)
    })
}

/// The machine's name, empty when it can't be read
pub fn hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // the buffer is valid for its length, which bounds what is written
        let res = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if res != 0 {
            return String::new();
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    //! The session store: the meta file, the lock, finding, listing, pruning and deleting.

    use super::*;
    use crate::capture::Recorder;
    use crate::capture::tests::{device, record, temp_dir, write_test_session};

    /// MAX-173: the device marker of a text recording names the port, its line settings when
    /// known, and the encoding the text is decoded with from there.
    /// How: a serial port, a TCP link (no line settings), a replay of a plain file (no device).
    #[test]
    fn device_markers() {
        let source = |address: &str| {
            let device = device(address);
            CaptureSource {
                address: device.address.clone(),
                device: Some(device),
            }
        };
        let utf8 = TextEncoding::Utf8;
        assert_eq!(
            source("/dev/ttyUSB1").marker(TextEncoding::Cp437),
            "── device: ttyUSB1, 115200 8N1, CP437 ──"
        );
        assert_eq!(
            source("tcp://10.0.0.2:4000").marker(utf8),
            "── device: 10.0.0.2:4000, UTF8 ──"
        );
        let plain = crate::capture::tests::source("file:///tmp/old.raw");
        assert_eq!(plain.marker(utf8), "── device: old.raw, UTF8 ──");
    }

    /// MAX-175: a replay plays a recording's `.raw`, found from its folder, its `.meta` or
    /// itself (a `.raw` without a meta too); a `.log`, any other file and a folder without its
    /// `.raw` aren't recordings.
    /// How: a full recording's folder with its files, a stray `.raw`, a `.log`, an empty folder.
    #[test]
    fn recording_raw_from_a_folder_a_meta_or_itself() {
        let dir = temp_dir("recording_raw");
        put(&dir, "20260101_000000_x.raw", "x");
        put(&dir, "20260101_000000_x.meta", "");
        let folder = dir.join("20260101_000000_x");
        let raw = folder.join("20260101_000000_x.raw");
        assert_eq!(recording_raw(&folder).unwrap(), raw);
        assert_eq!(recording_raw(&raw.with_extension("meta")).unwrap(), raw);
        assert_eq!(recording_raw(&raw).unwrap(), raw);
        assert_eq!(
            recording_raw(&dir.join("other.raw")).unwrap(),
            dir.join("other.raw")
        );
        for not_one in [raw.with_extension("log"), dir.join("notes.txt")] {
            let err = recording_raw(&not_one).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{not_one:?}");
        }
        fs::create_dir_all(dir.join("empty")).unwrap();
        assert!(recording_raw(&dir.join("empty")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The session tag keeps the last part of the address, safe for a file name.
    /// How: an address of each kind.
    #[test]
    fn session_tags() {
        let tag = |text: &str| session_tag(&text.parse::<Address>().unwrap());
        assert_eq!(tag("/dev/ttyUSB0"), "ttyUSB0");
        assert_eq!(tag("COM3"), "COM3");
        // no ':' in a Windows file name (MAX-156)
        assert_eq!(tag("tcp://192.168.1.50:4000"), "192.168.1.50_4000");
        assert_eq!(tag("file:///tmp/my log.raw"), "my_log.raw");
    }

    /// MAX-147: pruning keeps the newest sessions (all files of each, the meta file with them,
    /// MAX-148; the folder of each, MAX-175) and leaves other files.
    /// How: 3 sessions that received something and a note, keep 2.
    #[test]
    fn prune_keeps_the_newest_sessions() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "20260101_000000_a.log",
            "20260101_000000_a.meta",
            "20260101_000000_a.raw",
            "20260102_000000_a.log",
            "20260102_000000_a.meta",
            "20260102_000000_a.raw",
            "20260103_000000_b.log",
            "20260103_000000_b.raw",
            "notes.txt",
        ] {
            put(&dir, name, "x");
        }
        prune_sessions(&dir, SessionLimits::keep(2), "20260103_000000_b").unwrap();
        assert_eq!(
            file_names(&dir),
            [
                "20260102_000000_a.log",
                "20260102_000000_a.meta",
                "20260102_000000_a.raw",
                "20260103_000000_b.log",
                "20260103_000000_b.raw",
                "notes.txt"
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-163: past `max_size`, the oldest sessions go until the rest fits, whatever `keep`;
    /// the current session counts but stays, even alone over the limit.
    /// How: 4 sessions of 100 bytes (60 raw, 40 log), the newest current; 250 bytes, then 50.
    #[test]
    fn prune_keeps_the_folder_under_its_size() {
        let dir = temp_dir("prune_size");
        fs::create_dir_all(&dir).unwrap();
        for day in 1..=4 {
            put(&dir, &format!("2026010{day}_000000_a.raw"), [b'x'; 60]);
            put(&dir, &format!("2026010{day}_000000_a.log"), [b'x'; 40]);
        }
        let limits = |max| SessionLimits {
            keep: 10,
            max_size: Some(max),
        };
        prune_sessions(&dir, limits(250), "20260104_000000_a").unwrap();
        assert_eq!(
            file_names(&dir),
            [
                "20260103_000000_a.log",
                "20260103_000000_a.raw",
                "20260104_000000_a.log",
                "20260104_000000_a.raw"
            ]
        );
        prune_sessions(&dir, limits(50), "20260104_000000_a").unwrap();
        assert_eq!(
            file_names(&dir),
            ["20260104_000000_a.log", "20260104_000000_a.raw"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The names of the files in `dir` and in its sessions' folders (MAX-175)
    fn file_names(dir: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                names.extend(file_names(&path));
            } else {
                names.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
    }

    /// Where `name` goes in the session folder `dir`: a session file (`<stem>.log|raw|meta`)
    /// in its stem's folder, anything else in `dir` (MAX-175)
    fn at(dir: &Path, name: &str) -> PathBuf {
        match name.rsplit_once('.') {
            Some((stem, extension @ ("log" | "raw" | "meta"))) => {
                session_file(dir, stem, extension)
            }
            _ => dir.join(name),
        }
    }

    fn put(dir: &Path, name: &str, content: impl AsRef<[u8]>) {
        let path = at(dir, name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// MAX-148: empty sessions (an empty or missing `.raw`) are removed before counting, so
    /// launches that received nothing don't push out the real ones; the session just started
    /// stays, empty as it is.
    /// How: 2 real sessions between 3 empty ones, keep 2, the newest empty one current.
    #[test]
    fn prune_removes_empty_sessions_first() {
        let dir = temp_dir("prune_empty");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in [
            ("20260101_000000_a.raw", "x"),
            ("20260101_000000_a.log", "x"),
            ("20260102_000000_a.raw", ""),
            ("20260102_000000_a.meta", "x"),
            ("20260103_000000_a.raw", "x"),
            ("20260104_000000_a.meta", "x"),
            ("20260104_000000_a.log", ""),
            ("20260105_000000_a.raw", ""),
        ] {
            put(&dir, name, content);
        }
        prune_sessions(&dir, SessionLimits::keep(2), "20260105_000000_a").unwrap();
        assert_eq!(
            file_names(&dir),
            ["20260103_000000_a.raw", "20260105_000000_a.raw"]
        );

        // with room for 3: both real sessions stay, the empty ones still go
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in [
            ("20260101_000000_a.raw", "x"),
            ("20260102_000000_a.raw", ""),
            ("20260103_000000_a.raw", "x"),
            ("20260104_000000_a.raw", ""),
        ] {
            put(&dir, name, content);
        }
        prune_sessions(&dir, SessionLimits::keep(3), "20260104_000000_a").unwrap();
        assert_eq!(
            file_names(&dir),
            [
                "20260101_000000_a.raw",
                "20260103_000000_a.raw",
                "20260104_000000_a.raw"
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: a session still running elsewhere (its `.raw` locked) is never pruned, empty
    /// or old; once its lock is gone, it is. A session backup holds that lock while it runs.
    /// How: an empty session locked as another maxicom would, keep 1; then a real session backup.
    #[test]
    fn prune_keeps_a_running_session() {
        let dir = temp_dir("prune_running");
        fs::create_dir_all(&dir).unwrap();
        let other = at(&dir, "20260101_000000_other.raw");
        put(&dir, "20260101_000000_other.raw", "");
        put(&dir, "20260102_000000_mine.raw", "");
        let lock = File::open(&other).unwrap();
        lock.try_lock().unwrap();
        prune_sessions(&dir, SessionLimits::keep(1), "20260102_000000_mine").unwrap();
        assert!(other.exists());
        drop(lock);
        prune_sessions(&dir, SessionLimits::keep(1), "20260102_000000_mine").unwrap();
        assert!(!other.exists());

        let mut recorder = Recorder::default();
        recorder
            .start_session(
                &dir,
                &device("x"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();
        let session = file_names(&dir)
            .into_iter()
            .find_map(|n| n.strip_suffix("_x.raw").map(|stem| format!("{stem}_x")))
            .unwrap();
        assert!(is_running(&dir.join(&session)));
        drop(recorder);
        assert!(!is_running(&dir.join(&session)));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Writes a session of `stem` in `dir` as `start_session` would, with `raw` received.
    fn write_session(dir: &Path, stem: &str, device: &CaptureDevice, raw: &[u8]) {
        fs::create_dir_all(dir).unwrap();
        let meta = RecordingMeta {
            version: SESSION_META_VERSION,
            started: 1_700_000_000,
            device: device.clone(),
            text: CaptureFormat::default(),
            terminal: None,
        };
        put(
            dir,
            &format!("{stem}.meta"),
            toml::to_string(&meta).unwrap(),
        );
        put(dir, &format!("{stem}.log"), "");
        put(dir, &format!("{stem}.raw"), raw);
    }

    /// A meta file as maxicom wrote it before MAX-156 (version 1, the line settings at the top
    /// of `[device]`), kept as text: later versions must read it the same
    const META_V1: &str = r#"version = 1
started = 1700000000

[device]
host = "board-host"
port = "/dev/ttyUSB0"
baud = 115200
char_size = "bits8"
parity = "none"
stop_bits = "one"
encoding = "utf8"

[text]
timestamp = "[%H:%M:%S%.3f]"
tx_mark = "> "
"#;

    /// MAX-156: a version 1 meta reads back as a serial session with its line settings, and a
    /// serial session still writes the same file.
    /// How: the saved v1 text, read then written again.
    #[test]
    fn meta_v1_still_reads() {
        let meta: RecordingMeta = toml::from_str(META_V1).unwrap();
        assert_eq!(meta.device.address.to_string(), "/dev/ttyUSB0");
        assert_eq!(toml::to_string(&meta).unwrap(), META_V1);
    }

    /// MAX-156: a TCP session has no line settings in its meta, reads back without them, and
    /// restores only on the same address: never on a serial port, nor on another address.
    /// How: a TCP session's meta text, then the last session for three devices.
    #[test]
    fn tcp_sessions_match_their_address_only() {
        let dir = temp_dir("tcp_sessions");
        let tcp = device("tcp://192.168.1.50:4000");
        assert_eq!(tcp.line, None);
        write_session(&dir, "20260101_000000_192.168.1.50_4000", &tcp, b"rx");
        let meta = fs::read_to_string(at(&dir, "20260101_000000_192.168.1.50_4000.meta")).unwrap();
        assert!(!meta.contains("baud") && !meta.contains("parity"), "{meta}");

        assert_eq!(
            found_stem(&dir, &tcp, None).as_deref(),
            Some("20260101_000000_192.168.1.50_4000")
        );
        assert_eq!(
            found_stem(&dir, &device("tcp://192.168.1.51:4000"), None),
            None
        );
        let serial = CaptureDevice {
            line: device("/dev/ttyUSB0").line,
            ..tcp.clone()
        };
        assert_eq!(found_stem(&dir, &serial, None), None);
        let _ = fs::remove_dir_all(&dir);

        // a real session backup names its files after the address, which reads back whole
        let dir = temp_dir("tcp_session");
        let session = crate::capture::tests::write_test_session(
            &dir,
            "tcp://192.168.1.50:4000",
            &CaptureFormat::default(),
            b"rx\n",
            b"",
        );
        assert_eq!(session.port_tag(), "192.168.1.50_4000");
        assert_eq!(session.meta.unwrap().device.line, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-156: a meta whose line settings don't fit its kind of link is broken, not a session
    /// of the other kind: a serial meta missing a line field (serde would read it as no line
    /// settings), or a TCP meta with line settings.
    /// How: the v1 text without `parity`, then with a `tcp://` port.
    #[test]
    fn meta_with_the_wrong_line_is_broken() {
        let dir = temp_dir("broken_line");
        fs::create_dir_all(&dir).unwrap();
        let cases = [
            (
                "20260101_000000_ttyUSB0",
                META_V1.replace("parity = \"none\"\n", ""),
            ),
            (
                "20260102_000000_192.168.1.50_4000",
                META_V1.replace("/dev/ttyUSB0", "tcp://192.168.1.50:4000"),
            ),
        ];
        for (stem, text) in &cases {
            put(&dir, &format!("{stem}.meta"), text);
            put(&dir, &format!("{stem}.raw"), "rx");
        }
        let sessions = list_sessions(&dir).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|session| session.meta.is_none()));
        let _ = fs::remove_dir_all(&dir);
    }

    fn found_stem(dir: &Path, device: &CaptureDevice, max_age: Option<Duration>) -> Option<String> {
        let session = find_last_session(dir, device, max_age)?;
        Some(session.raw_path()?.file_stem()?.to_str()?.to_string())
    }

    /// MAX-148: the last session found is the newest one on the same device that received
    /// something; another host, port, baud or encoding, an empty session, an unknown meta
    /// version or a broken meta file never match.
    /// How: sessions from oldest to newest, each newer one failing one check.
    #[test]
    fn last_session_matches_the_device() {
        let dir = temp_dir("last_session");
        let mine = device("/dev/ttyUSB0");
        write_session(&dir, "20260101_000000_ttyUSB0", &mine, b"old");
        write_session(&dir, "20260102_000000_ttyUSB0", &mine, b"good");
        let other_host = CaptureDevice {
            host: format!("{}-other", mine.host),
            ..mine.clone()
        };
        write_session(&dir, "20260103_000000_ttyUSB0", &other_host, b"x");
        write_session(
            &dir,
            "20260104_000000_ttyUSB1",
            &device("/dev/ttyUSB1"),
            b"x",
        );
        let other_baud = CaptureDevice {
            line: mine.line.map(|line| CaptureLine { baud: 9600, ..line }),
            ..mine.clone()
        };
        write_session(&dir, "20260105_000000_ttyUSB0", &other_baud, b"x");
        let other_encoding = CaptureDevice {
            encoding: TextEncoding::Latin1,
            ..mine.clone()
        };
        write_session(&dir, "20260106_000000_ttyUSB0", &other_encoding, b"x");
        write_session(&dir, "20260107_000000_ttyUSB0", &mine, b"");
        write_session(&dir, "20260108_000000_ttyUSB0", &mine, b"x");
        let meta = at(&dir, "20260108_000000_ttyUSB0.meta");
        let next_version = fs::read_to_string(&meta)
            .unwrap()
            .replace("version = 1", "version = 2");
        fs::write(&meta, next_version).unwrap();
        write_session(&dir, "20260109_000000_ttyUSB0", &mine, b"x");
        put(&dir, "20260109_000000_ttyUSB0.meta", "not toml [");

        let session = find_last_session(&dir, &mine, None).unwrap();
        assert_eq!(
            session.raw_path(),
            Some(at(&dir, "20260102_000000_ttyUSB0.raw").as_path())
        );
        assert_eq!(
            session.text_path(),
            Some(at(&dir, "20260102_000000_ttyUSB0.log").as_path())
        );
        assert_eq!(session.raw_len, 4);
        assert_eq!(session.started.unwrap().timestamp(), 1_700_000_000);
        assert_eq!(
            found_stem(&dir, &other_host, None).as_deref(),
            Some("20260103_000000_ttyUSB0")
        );
        assert_eq!(found_stem(&dir, &device("/dev/ttyACM0"), None), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: `max_age` is checked on when the session last received (its raw file's time),
    /// so a long session that started long ago still restores.
    /// How: the raw file's time set 2 hours back, then 1 and 3 hours of max age.
    #[test]
    fn last_session_max_age_uses_the_last_write() {
        let dir = temp_dir("last_session_age");
        let mine = device("/dev/ttyUSB0");
        write_session(&dir, "20260101_000000_ttyUSB0", &mine, b"x");
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 3600);
        File::options()
            .write(true)
            .open(at(&dir, "20260101_000000_ttyUSB0.raw"))
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        let hours = |h: u64| Some(Duration::from_secs(h * 3600));
        assert_eq!(found_stem(&dir, &mine, hours(1)), None);
        assert_eq!(
            found_stem(&dir, &mine, hours(3)).as_deref(),
            Some("20260101_000000_ttyUSB0")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the session's meta file holds the device, so its session is found again.
    /// How: a session backup started and fed, then dropped, then looked up.
    #[test]
    fn session_is_found_again() {
        let dir = temp_dir("session_meta");
        let mine = device("/dev/ttyUSB0");
        let mut recorder = Recorder::default();
        let text = CaptureFormat {
            timestamp: Some("[%H:%M]".to_string()),
            tx_mark: None,
        };
        recorder
            .start_session(&dir, &mine, &text, SessionLimits::keep(20))
            .unwrap();
        record(&mut recorder, b"hello\n");
        drop(recorder);
        let session = find_last_session(&dir, &mine, None).unwrap();
        assert_eq!(session.meta.as_ref().unwrap().text, text);
        let raw_path = session.raw_path().unwrap();
        assert!(raw_path.to_string_lossy().ends_with("_ttyUSB0.raw"));
        assert_eq!(session.raw_len, 6);
        let log = fs::read_to_string(session.text_path().unwrap()).unwrap();
        assert!(log.ends_with("] hello\n"), "{log:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: two session backups started in the same second on one port stay two sessions, the
    /// second one newer: a relaunch never appends to the last session nor overwrites its meta.
    #[test]
    fn sessions_in_one_second_stay_apart() {
        let dir = temp_dir("same_second");
        let first = write_test_session(&dir, "/dev/x", &CaptureFormat::default(), b"one\r\n", b"");
        std::thread::sleep(Duration::from_millis(2));
        let second = write_test_session(&dir, "/dev/x", &CaptureFormat::default(), b"two\r\n", b"");
        assert_ne!(first.stem, second.stem);
        assert!(second.stem > first.stem);
        let sessions = list_sessions(&dir).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].stem, second.stem);
        assert_eq!(sessions[0].port_tag(), "x");
        assert_eq!(sessions[1].raw_len, 5);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the session list has every session, the newest first, with its meta when it
    /// has one (else the start comes from its name), its size (`.log` and `.raw`), its last
    /// write and whether it runs; other files aren't sessions, and no folder is no session.
    /// How: a session with a meta, one from before MAX-148, a running session backup and a note.
    #[test]
    fn sessions_are_listed() {
        let dir = temp_dir("list_sessions");
        assert!(list_sessions(&dir).unwrap().is_empty());
        let with_meta = write_test_session(
            &dir,
            "/dev/ttyUSB0",
            &CaptureFormat::default(),
            b"abc\r\n",
            b"",
        );
        put(&dir, "20200101_120000_old.log", "old\n");
        put(&dir, "20200101_120000_old.raw", "old\r\n");
        put(&dir, "notes.txt", "x");
        let mut running = Recorder::default();
        running
            .start_session(
                &dir,
                &device("live"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();

        let entries = list_sessions(&dir).unwrap();
        // the two new sessions may share their second: only the old one's place is sure
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].port_tag(), "old");
        let by_tag = |tag| entries.iter().find(|e| e.port_tag() == tag).unwrap();
        let (live, usb, old) = (by_tag("live"), by_tag("ttyUSB0"), by_tag("old"));
        assert!(live.running && !usb.running && !old.running);

        let meta = usb.meta.as_ref().unwrap();
        assert_eq!(meta.device.address.to_string(), "/dev/ttyUSB0");
        assert_eq!(meta.text, CaptureFormat::default());
        assert_eq!(usb.started, with_meta.started);
        assert_eq!(usb.ended, with_meta.ended);
        assert_eq!(usb.raw_len, 5);
        // all its files: the `.log`, the `.raw`, the `.meta` and the `.times` (MAX-175)
        let files_size = |session: &Session| -> u64 {
            let files = session.files.iter();
            files.map(|path| fs::metadata(path).unwrap().len()).sum()
        };
        assert_eq!(usb.size, files_size(usb));
        assert!(usb.size > 4 + 5);
        assert_eq!(usb.files.len(), 4);
        assert_eq!(usb.folder, usb.files[0].parent().unwrap());
        assert_eq!(usb.text_path(), with_meta.text_path());

        assert_eq!(old.meta, None);
        let old_start = Local.with_ymd_and_hms(2020, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(old.started, Some(old_start));
        assert_eq!(old.size, 4 + 5);
        drop(running);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: by device, the sessions go by port, then the newest first.
    #[test]
    fn sessions_sort_by_device() {
        let entry = |stem: &str| Session {
            stem: stem.to_string(),
            folder: PathBuf::new(),
            files: Vec::new(),
            meta: None,
            started: None,
            ended: None,
            raw_len: 0,
            size: 0,
            running: false,
        };
        let mut entries = vec![
            entry("20260101_000000_b"),
            entry("20260103_000000_a"),
            entry("20260102_000000_b"),
            entry("20260101_000000_a"),
        ];
        let stems = |entries: &[Session]| -> Vec<String> {
            entries.iter().map(|e| e.stem.clone()).collect()
        };
        sort_sessions(&mut entries, SessionOrder::Device);
        assert_eq!(
            stems(&entries),
            [
                "20260103_000000_a",
                "20260101_000000_a",
                "20260102_000000_b",
                "20260101_000000_b"
            ]
        );
        sort_sessions(&mut entries, SessionOrder::Date);
        assert_eq!(
            stems(&entries),
            [
                "20260103_000000_a",
                "20260102_000000_b",
                "20260101_000000_b",
                "20260101_000000_a"
            ]
        );
        assert_eq!(SessionOrder::Date.other(), SessionOrder::Device);
    }

    /// MAX-148: deleting a session removes its files only, and is refused while it runs;
    /// deleting all keeps the running ones and says what it did.
    /// How: two past sessions, a running session backup and a note.
    #[test]
    fn sessions_are_deleted_but_running_ones() {
        let dir = temp_dir("delete_sessions");
        write_test_session(&dir, "/dev/a", &CaptureFormat::default(), b"aa\r\n", b"");
        write_test_session(&dir, "/dev/b", &CaptureFormat::default(), b"bbb\r\n", b"");
        put(&dir, "notes.txt", "x");
        let mut running = Recorder::default();
        running
            .start_session(
                &dir,
                &device("live"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();

        let entries = list_sessions(&dir).unwrap();
        let live = entries.iter().find(|e| e.running).unwrap();
        assert!(delete_session(live).is_err());
        assert!(live.files.iter().all(|path| path.exists()));

        let a = entries.iter().find(|e| e.port_tag() == "a").unwrap();
        delete_session(a).unwrap();
        assert!(a.files.iter().all(|path| !path.exists()));

        let entries = list_sessions(&dir).unwrap();
        let deleted: u64 = (entries.iter().filter(|e| !e.running))
            .map(|e| e.size)
            .sum();
        let report = delete_sessions(&entries);
        assert_eq!((report.deleted, report.running), (1, 1));
        assert_eq!(report.size, deleted);
        assert!(report.error.is_none());
        let left: Vec<_> = list_sessions(&dir).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].port_tag(), "live");
        assert!(dir.join("notes.txt").exists());
        drop(running);
        let _ = fs::remove_dir_all(&dir);
    }
}
