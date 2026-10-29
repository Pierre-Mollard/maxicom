use crate::configs::TextEncoding;
use crate::serial::{CharSize, Parity, SerialConfig, StopBits};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const SESSION_META_VERSION: u32 = 1;

/// How a journal's text lines are written, taken from the capture settings when it starts and
/// kept in its meta file, so it can be read back (MAX-148): the timestamp format when it is
/// on, and the mark before sent lines when they are recorded. A reload changes the user
/// recordings only, so one journal never mixes two formats
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalText {
    pub timestamp: Option<String>,
    pub tx_mark: Option<String>,
}

/// What a session was recorded on (MAX-148): a session restores only on the same machine,
/// port, line settings and encoding, as bytes from another baud or encoding are garbage. Flow
/// control doesn't change the bytes, so it isn't part of it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDevice {
    pub host: String,
    pub port: String,
    pub baud: u32,
    pub char_size: CharSize,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub encoding: TextEncoding,
}

impl SessionDevice {
    pub fn new(serial: &SerialConfig, encoding: TextEncoding) -> Self {
        Self {
            host: hostname(),
            port: serial.port.clone(),
            baud: serial.baudrate,
            char_size: serial.char_size,
            parity: serial.parity,
            stop_bits: serial.stop_bits,
            encoding,
        }
    }
}

/// The `<stem>.meta` file of a session, written when it starts. A file of another `version`
/// is read as none, and never restores
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub version: u32,
    pub started: i64,
    pub device: SessionDevice,
    pub text: JournalText,
}

/// Removes the empty sessions in `dir`, then the oldest so `keep` are left, never `current`
/// nor a session still running (MAX-148). A session is the `.log`, `.raw` and `.meta` files of
/// one stem, which starts with its date so the names sort by age
pub fn prune_sessions(dir: &Path, keep: usize, current: &str) -> io::Result<()> {
    let mut sessions = session_files(dir)?;
    let removable = |stem: &str| stem != current && !is_running(dir, stem);
    // a launch that received nothing leaves an empty session, which would fill `keep`
    sessions.retain(|stem, paths| {
        let empty =
            fs::metadata(dir.join(format!("{stem}.raw"))).map_or(true, |raw| raw.len() == 0);
        if !empty || !removable(stem) {
            return true;
        }
        let _ = remove_files(paths);
        false
    });
    let old = sessions.len().saturating_sub(keep);
    for (stem, paths) in sessions.iter().take(old) {
        if removable(stem) {
            let _ = remove_files(paths);
        }
    }
    Ok(())
}

/// The session files in `dir` by stem, which starts with the date so they sort by age
fn session_files(dir: &Path) -> io::Result<BTreeMap<String, Vec<PathBuf>>> {
    let mut sessions: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_session = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("log" | "raw" | "meta")
        );
        if is_session && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            sessions.entry(stem.to_string()).or_default().push(path);
        }
    }
    Ok(sessions)
}

/// Removes every file of `paths`, even after one fails, and gives the first error
fn remove_files(paths: &[PathBuf]) -> io::Result<()> {
    let mut result = Ok(());
    for path in paths {
        if let Err(err) = fs::remove_file(path)
            && result.is_ok()
        {
            result = Err(err);
        }
    }
    result
}

/// A session in the cache folder (MAX-148), what the restore and the session browser use: its
/// meta file when it has one (none before MAX-148), when it started (from the meta, else from
/// its name) and last received (the raw file's time, right even after a crash), the size of
/// its `.raw` and of its `.log` and `.raw` together, and whether it is still running, here or
/// in another maxicom
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub stem: String,
    pub files: Vec<PathBuf>,
    pub meta: Option<SessionMeta>,
    pub started: Option<DateTime<Local>>,
    pub ended: Option<DateTime<Local>>,
    pub raw_len: u64,
    pub size: u64,
    pub running: bool,
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
    let meta = fs::read_to_string(dir.join(format!("{stem}.meta")))
        .ok()
        .and_then(|meta| toml::from_str::<SessionMeta>(&meta).ok())
        .filter(|meta| meta.version == SESSION_META_VERSION);
    let started = match &meta {
        Some(meta) => DateTime::from_timestamp(meta.started, 0).map(|t| t.with_timezone(&Local)),
        None => stem
            .get(..15)
            .and_then(|date| NaiveDateTime::parse_from_str(date, "%Y%m%d_%H%M%S").ok())
            .and_then(|date| Local.from_local_datetime(&date).earliest()),
    };
    let raw = fs::metadata(dir.join(format!("{stem}.raw"))).ok();
    let log_len = fs::metadata(dir.join(format!("{stem}.log"))).map_or(0, |log| log.len());
    let raw_len = raw.as_ref().map_or(0, |raw| raw.len());
    Session {
        running: is_running(dir, &stem),
        ended: raw.and_then(|raw| raw.modified().ok()).map(DateTime::from),
        raw_len,
        size: log_len + raw_len,
        meta,
        started,
        stem,
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

/// Deletes the files of `entry`, refused while it is still running
pub fn delete_session(entry: &Session) -> io::Result<()> {
    let dir = entry.files.first().and_then(|path| path.parent());
    if dir.is_some_and(|dir| is_running(dir, &entry.stem)) {
        return Err(io::Error::other("the session is still running"));
    }
    remove_files(&entry.files)
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

/// Whether the session of `stem` is still running, here or in another maxicom: its journal
/// holds the lock on its `.raw`
fn is_running(dir: &Path, stem: &str) -> bool {
    File::open(dir.join(format!("{stem}.raw")))
        .is_ok_and(|raw| matches!(raw.try_lock(), Err(fs::TryLockError::WouldBlock)))
}

/// The newest session in `dir` recorded on `device` that received something, isn't running
/// and ended within `max_age` of now when given. To call before `start_journal`, whose
/// pruning could remove it
pub fn find_last_session(
    dir: &Path,
    device: &SessionDevice,
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

    /// MAX-147: pruning keeps the newest sessions (all files of each, the meta file with them,
    /// MAX-148) and leaves other files.
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
            fs::write(dir.join(name), "x").unwrap();
        }
        prune_sessions(&dir, 2, "20260103_000000_b").unwrap();
        let mut left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
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

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
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
            fs::write(dir.join(name), content).unwrap();
        }
        prune_sessions(&dir, 2, "20260105_000000_a").unwrap();
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
            fs::write(dir.join(name), content).unwrap();
        }
        prune_sessions(&dir, 3, "20260104_000000_a").unwrap();
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
    /// or old; once its lock is gone, it is. A journal holds that lock while it runs.
    /// How: an empty session locked as another maxicom would, keep 1; then a real journal.
    #[test]
    fn prune_keeps_a_running_session() {
        let dir = temp_dir("prune_running");
        fs::create_dir_all(&dir).unwrap();
        let other = dir.join("20260101_000000_other.raw");
        fs::write(&other, "").unwrap();
        fs::write(dir.join("20260102_000000_mine.raw"), "").unwrap();
        let lock = File::open(&other).unwrap();
        lock.try_lock().unwrap();
        prune_sessions(&dir, 1, "20260102_000000_mine").unwrap();
        assert!(other.exists());
        drop(lock);
        prune_sessions(&dir, 1, "20260102_000000_mine").unwrap();
        assert!(!other.exists());

        let mut recorder = Recorder::default();
        recorder
            .start_journal(&dir, &device("x"), &JournalText::default(), 20)
            .unwrap();
        let session = file_names(&dir)
            .into_iter()
            .find_map(|n| n.strip_suffix("_x.raw").map(|stem| format!("{stem}_x")))
            .unwrap();
        assert!(is_running(&dir, &session));
        drop(recorder);
        assert!(!is_running(&dir, &session));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Writes a session of `stem` in `dir` as `start_journal` would, with `raw` received.
    fn write_session(dir: &Path, stem: &str, device: &SessionDevice, raw: &[u8]) {
        fs::create_dir_all(dir).unwrap();
        let meta = SessionMeta {
            version: SESSION_META_VERSION,
            started: 1_700_000_000,
            device: device.clone(),
            text: JournalText::default(),
        };
        fs::write(
            dir.join(format!("{stem}.meta")),
            toml::to_string(&meta).unwrap(),
        )
        .unwrap();
        fs::write(dir.join(format!("{stem}.log")), "").unwrap();
        fs::write(dir.join(format!("{stem}.raw")), raw).unwrap();
    }

    fn found_stem(dir: &Path, device: &SessionDevice, max_age: Option<Duration>) -> Option<String> {
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
        let other_host = SessionDevice {
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
        let other_baud = SessionDevice {
            baud: 9600,
            ..mine.clone()
        };
        write_session(&dir, "20260105_000000_ttyUSB0", &other_baud, b"x");
        let other_encoding = SessionDevice {
            encoding: TextEncoding::Latin1,
            ..mine.clone()
        };
        write_session(&dir, "20260106_000000_ttyUSB0", &other_encoding, b"x");
        write_session(&dir, "20260107_000000_ttyUSB0", &mine, b"");
        write_session(&dir, "20260108_000000_ttyUSB0", &mine, b"x");
        let meta = dir.join("20260108_000000_ttyUSB0.meta");
        let next_version = fs::read_to_string(&meta)
            .unwrap()
            .replace("version = 1", "version = 2");
        fs::write(&meta, next_version).unwrap();
        write_session(&dir, "20260109_000000_ttyUSB0", &mine, b"x");
        fs::write(dir.join("20260109_000000_ttyUSB0.meta"), "not toml [").unwrap();

        let session = find_last_session(&dir, &mine, None).unwrap();
        assert_eq!(
            session.raw_path(),
            Some(dir.join("20260102_000000_ttyUSB0.raw").as_path())
        );
        assert_eq!(
            session.text_path(),
            Some(dir.join("20260102_000000_ttyUSB0.log").as_path())
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
            .open(dir.join("20260101_000000_ttyUSB0.raw"))
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

    /// MAX-148: the journal's meta file holds the device, so its session is found again.
    /// How: a journal started and fed, then dropped, then looked up.
    #[test]
    fn journal_session_is_found_again() {
        let dir = temp_dir("journal_meta");
        let mine = device("/dev/ttyUSB0");
        let mut recorder = Recorder::default();
        let text = JournalText {
            timestamp: Some("[%H:%M]".to_string()),
            tx_mark: None,
        };
        recorder.start_journal(&dir, &mine, &text, 20).unwrap();
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

    /// MAX-148: two journals started in the same second on one port stay two sessions, the
    /// second one newer: a relaunch never appends to the last session nor overwrites its meta.
    #[test]
    fn journals_in_one_second_stay_apart() {
        let dir = temp_dir("same_second");
        let first = write_test_session(&dir, "/dev/x", &JournalText::default(), b"one\r\n", b"");
        std::thread::sleep(Duration::from_millis(2));
        let second = write_test_session(&dir, "/dev/x", &JournalText::default(), b"two\r\n", b"");
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
    /// How: a session with a meta, one from before MAX-148, a running journal and a note.
    #[test]
    fn sessions_are_listed() {
        let dir = temp_dir("list_sessions");
        assert!(list_sessions(&dir).unwrap().is_empty());
        let with_meta = write_test_session(
            &dir,
            "/dev/ttyUSB0",
            &JournalText::default(),
            b"abc\r\n",
            b"",
        );
        fs::write(dir.join("20200101_120000_old.log"), "old\n").unwrap();
        fs::write(dir.join("20200101_120000_old.raw"), "old\r\n").unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        let mut running = Recorder::default();
        running
            .start_journal(&dir, &device("live"), &JournalText::default(), 20)
            .unwrap();

        let entries = list_sessions(&dir).unwrap();
        // the two new sessions may share their second: only the old one's place is sure
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].port_tag(), "old");
        let by_tag = |tag| entries.iter().find(|e| e.port_tag() == tag).unwrap();
        let (live, usb, old) = (by_tag("live"), by_tag("ttyUSB0"), by_tag("old"));
        assert!(live.running && !usb.running && !old.running);

        let meta = usb.meta.as_ref().unwrap();
        assert_eq!(meta.device.port, "/dev/ttyUSB0");
        assert_eq!(meta.text, JournalText::default());
        assert_eq!(usb.started, with_meta.started);
        assert_eq!(usb.ended, with_meta.ended);
        assert_eq!(usb.raw_len, 5);
        assert_eq!(usb.size, 4 + 5);
        assert_eq!(usb.files.len(), 3);
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
    /// How: two past sessions, a running journal and a note.
    #[test]
    fn sessions_are_deleted_but_running_ones() {
        let dir = temp_dir("delete_sessions");
        write_test_session(&dir, "/dev/a", &JournalText::default(), b"aa\r\n", b"");
        write_test_session(&dir, "/dev/b", &JournalText::default(), b"bbb\r\n", b"");
        fs::write(dir.join("notes.txt"), "x").unwrap();
        let mut running = Recorder::default();
        running
            .start_journal(&dir, &device("live"), &JournalText::default(), 20)
            .unwrap();

        let entries = list_sessions(&dir).unwrap();
        let live = entries.iter().find(|e| e.running).unwrap();
        assert!(delete_session(live).is_err());
        assert!(live.files.iter().all(|path| path.exists()));

        let a = entries.iter().find(|e| e.port_tag() == "a").unwrap();
        delete_session(a).unwrap();
        assert!(a.files.iter().all(|path| !path.exists()));

        let report = delete_sessions(&list_sessions(&dir).unwrap());
        assert_eq!((report.deleted, report.running), (1, 1));
        assert_eq!(report.size, 4 + 5);
        assert!(report.error.is_none());
        let left: Vec<_> = list_sessions(&dir).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].port_tag(), "live");
        assert!(dir.join("notes.txt").exists());
        drop(running);
        let _ = fs::remove_dir_all(&dir);
    }
}
