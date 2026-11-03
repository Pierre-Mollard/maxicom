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
#[path = "../tests/capture/session.rs"]
mod tests;
