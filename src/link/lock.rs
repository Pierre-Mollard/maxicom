//! The locks on a link (MAX-186): maxicom's own on the link it has open, in a folder every
//! user shares, and the ones other programs take, read to say who holds a port (MAX-181)
use std::{
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::Address;

/// Why an open is refused: another program holds the port, named (MAX-186). The error of an
/// `io::Error`, which the probe reads back
#[derive(Debug)]
pub struct HeldBy(pub String);

impl fmt::Display for HeldBy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "held by {}", self.0)
    }
}

impl std::error::Error for HeldBy {}

/// An open refused because `holder` holds the port
pub fn held_by(holder: String) -> io::Error {
    io::Error::new(io::ErrorKind::ResourceBusy, HeldBy(holder))
}

/// maxicom's lock on a link it has open (MAX-186), released when dropped: `<tag>.lock` held
/// with `File::try_lock` in the shared folder, and `<tag>.info` beside it saying who holds it
/// (a Windows lock keeps the others from reading the locked file). The OS drops the lock with
/// the process, a crash included: a lock file nobody holds is stale, and taken over
pub struct LinkLock {
    file: File,
    // removed on release on Unix only (see `drop`)
    #[cfg_attr(not(unix), allow(dead_code))]
    lock_path: PathBuf,
    info_path: PathBuf,
}

/// Who holds a link's lock, as its `.info` says
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockInfo {
    pub pid: u32,
    pub user: String,
    pub host: String,
    pub address: String,
    pub since: String,
}

/// Whether a maxicom holds a link: its info when it could be read
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    Free,
    Held(Option<LockInfo>),
}

impl LinkLock {
    /// The lock on `address` in the shared folder, none when there's no folder to use or
    /// another maxicom holds it
    pub fn take(address: &Address) -> Option<Self> {
        Self::take_in(&lock_dir()?, address).ok()
    }

    /// The lock on `address` in `dir`: refused with the holder's info when held
    pub fn take_in(dir: &Path, address: &Address) -> Result<Self, LockState> {
        let (lock_path, info_path) = lock_paths(dir, address);
        let other = |_| LockState::Held(None);
        // a holder that releases removes the file: the one locked here may be gone, a new one
        // in its place, so it's checked and taken again
        for _ in 0..3 {
            let file = open_lock(&lock_path, true).map_err(other)?;
            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => {
                    return Err(LockState::Held(read_info(&info_path)));
                }
                Err(TryLockError::Error(err)) => return Err(other(err)),
            }
            if !is_same_file(&file, &lock_path) {
                continue;
            }
            let lock = Self {
                file,
                lock_path,
                info_path,
            };
            // the lock holds without its info: a reader then says held, by whom unknown
            let _ = lock.write_info(address);
            return Ok(lock);
        }
        Err(LockState::Held(None))
    }

    fn write_info(&self, address: &Address) -> io::Result<()> {
        let user = std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" });
        let text = format!(
            "pid={}\nuser={}\nhost={}\naddress={address}\nsince={}\n",
            std::process::id(),
            user.unwrap_or_default(),
            crate::configs::links::this_host().unwrap_or_default(),
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        );
        let mut info = open_shared(&self.info_path, true, true)?;
        info.set_len(0)?;
        info.write_all(text.as_bytes())
    }
}

impl Drop for LinkLock {
    /// The info, then the lock file while still held, removed. Windows keeps the lock file:
    /// removing a file someone has open fails there, and its next taker locks it as it is
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.info_path);
        #[cfg(unix)]
        let _ = fs::remove_file(&self.lock_path);
        let _ = self.file.unlock();
    }
}

/// Whether a maxicom holds `address`'s lock in the shared folder
pub fn lock_state(address: &Address) -> LockState {
    lock_dir().map_or(LockState::Free, |dir| lock_state_in(&dir, address))
}

/// Whether a maxicom holds `address`'s lock in `dir`: a shared lock taken and dropped at once
/// tells, without holding it up
pub fn lock_state_in(dir: &Path, address: &Address) -> LockState {
    let (lock_path, info_path) = lock_paths(dir, address);
    let Ok(file) = open_lock(&lock_path, false) else {
        return LockState::Free;
    };
    match file.try_lock_shared() {
        Err(TryLockError::WouldBlock) => LockState::Held(read_info(&info_path)),
        _ => LockState::Free,
    }
}

/// The folder every user's maxicom shares for its locks: `/run/lock/maxicom` where the system
/// lets users make it (Debian, Ubuntu), else `/tmp/maxicom-locks`, made writable by all with the
/// sticky bit as `/tmp` is; `%ProgramData%\maxicom\locks` on Windows. None when none can be used
pub fn lock_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        (["/run/lock/maxicom", "/tmp/maxicom-locks"].into_iter())
            .map(PathBuf::from)
            .find(|dir| shared_dir(dir).is_ok())
    }
    #[cfg(windows)]
    {
        let data = std::env::var_os("ProgramData")?;
        let dir = Path::new(&data).join("maxicom").join("locks");
        fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }
    #[cfg(not(any(unix, windows)))]
    None
}

/// `dir` made if missing, open to every user (`1777`), and checked: a real folder, not a
/// link, this user can write in
#[cfg(unix)]
fn shared_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match fs::DirBuilder::new().mode(0o1777).create(dir) {
        // the umask took bits off
        Ok(()) => fs::set_permissions(dir, fs::Permissions::from_mode(0o1777))?,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(io::Error::other("not a folder"));
    }
    let path = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes())?;
    if unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// `<tag>.lock` and `<tag>.info` in `dir`; a serial port by the device it leads to, so
/// `/dev/serial/by-id/...` and `/dev/ttyUSB0` share one lock
fn lock_paths(dir: &Path, address: &Address) -> (PathBuf, PathBuf) {
    let resolved = match address {
        Address::Serial(device) => fs::canonicalize(device)
            .ok()
            .and_then(|path| path.into_os_string().into_string().ok())
            .map(Address::Serial),
        _ => None,
    };
    let tag = resolved.as_ref().unwrap_or(address).file_tag();
    (
        dir.join(format!("{tag}.lock")),
        dir.join(format!("{tag}.info")),
    )
}

/// The lock file, made when `create` (then writable by every user, so any maxicom can take
/// it over once stale); read only when this user can't write it, which is enough to lock it
fn open_lock(path: &Path, create: bool) -> io::Result<File> {
    match open_shared(path, create, create) {
        Err(err) if create && err.kind() == io::ErrorKind::PermissionDenied => {
            open_shared(path, false, false)
        }
        result => result,
    }
}

/// A file of the shared folder, never through a link and only a plain file, as anyone may
/// put one there. Made `0666` when this user made it
fn open_shared(path: &Path, write: bool, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o666);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if create && meta.uid() == unsafe { libc::geteuid() } {
            let _ = file.set_permissions(fs::Permissions::from_mode(0o666));
        }
    }
    Ok(file)
}

/// Whether `file` is still the one at `path`: a holder releasing removes its lock file (Unix)
fn is_same_file(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(open), Ok(named)) = (file.metadata(), fs::symlink_metadata(path)) else {
            return false;
        };
        (open.dev(), open.ino()) == (named.dev(), named.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path);
        true
    }
}

/// A lock's `.info`, `key=value` lines; none without a PID
fn read_info(path: &Path) -> Option<LockInfo> {
    let mut file = open_shared(path, false, false).ok()?;
    let mut text = String::new();
    io::Read::read_to_string(&mut file, &mut text).ok()?;
    let value = |key: &str| {
        (text.lines())
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_default()
            .to_string()
    };
    Some(LockInfo {
        pid: value("pid").parse().ok()?,
        user: value("user"),
        host: value("host"),
        address: value("address"),
        since: value("since"),
    })
}

/// The lock folders of minicom, picocom and the other UUCP-style programs
#[cfg(unix)]
pub const LOCK_DIRS: &[&str] = &["/var/lock", "/run/lock", "/var/spool/lock"];

/// The running process of a UUCP lock file for `port` in one of the lock folders `dirs`, the
/// PID in it written as text, under any name a program gives it (`uucp_names`)
#[cfg(unix)]
pub fn locked_by(port: &str, dirs: &[&str]) -> Option<libc::pid_t> {
    let names = uucp_names(port);
    (dirs.iter())
        .flat_map(|dir| names.iter().map(move |name| Path::new(dir).join(name)))
        .find_map(|path| live_pid(&path))
}

/// The names programs give a UUCP lock file for `port`, for the path given and the device it
/// leads to: the last part (`LCK..ttyUSB0`: picocom, lockdev, maxicom), and minicom's, the
/// path after `/dev/` with `_` for `/` (`LCK..pts_3`, `LCK..serial_by-id_usb-…`). The same for
/// a plain `/dev/ttyUSB0`
#[cfg(unix)]
fn uucp_names(port: &str) -> Vec<String> {
    let given = PathBuf::from(port);
    let resolved = fs::canonicalize(port).ok();
    let mut names = Vec::new();
    for path in std::iter::once(&given).chain(resolved.as_ref()) {
        let text = path.to_string_lossy();
        let minicom = text
            .strip_prefix("/dev/")
            .map(|rest| rest.replace('/', "_"));
        let last = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        for name in minicom.into_iter().chain(last) {
            let name = format!("LCK..{name}");
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// A UUCP lock file's name for `port`: `LCK..` and the name of the device it leads to, so a
/// `by-id` link and the device share it
#[cfg(unix)]
fn uucp_name(port: &str) -> Option<String> {
    let path = fs::canonicalize(port).unwrap_or_else(|_| PathBuf::from(port));
    Some(format!("LCK..{}", path.file_name()?.to_string_lossy()))
}

/// The PID a UUCP lock file holds, written as text first: the first word, as some programs
/// write more after it
#[cfg(unix)]
fn lock_pid(path: &Path) -> Option<libc::pid_t> {
    let text = fs::read_to_string(path).ok()?;
    let first = text.split_whitespace().next()?;
    first.parse::<libc::pid_t>().ok().filter(|&pid| pid > 0)
}

/// The PID a UUCP lock file holds, when that process runs
#[cfg(unix)]
fn live_pid(path: &Path) -> Option<libc::pid_t> {
    let pid = lock_pid(path)?;
    // signal 0 only asks whether the process exists; EPERM: it does, someone else's
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    (alive || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)).then_some(pid)
}

/// maxicom's UUCP lock file on a serial port it has open (MAX-186), for minicom, picocom and
/// the other programs that look for one: `LCK..ttyUSB0` holding this PID, in the first lock
/// folder this user can write in (Debian's and Ubuntu's; not Fedora's, skipped there). A stale
/// one, its process gone (a crash), is replaced; removed when dropped while it's still this
/// process's
#[cfg(unix)]
pub struct UucpLock {
    path: PathBuf,
}

#[cfg(unix)]
impl UucpLock {
    /// The lock on `port` in the first lock folder that takes it
    pub fn take(port: &str) -> Option<Self> {
        let name = uucp_name(port)?;
        (LOCK_DIRS.iter()).find_map(|dir| Self::take_in(Path::new(dir), &name).ok())
    }

    /// The lock file `name` in `dir`, refused with the PID of a live holder
    pub fn take_in(dir: &Path, name: &str) -> Result<Self, Option<libc::pid_t>> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = dir.join(name);
        // a stale one removed, then made again: once
        for _ in 0..2 {
            let created = OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(libc::O_NOFOLLOW)
                .mode(0o644)
                .open(&path);
            match created {
                Ok(mut file) => {
                    // the HDB format: the PID right-aligned on 10 characters
                    let written = writeln!(file, "{:>10}", std::process::id());
                    return match written {
                        Ok(()) => Ok(Self { path }),
                        Err(_) => {
                            let _ = fs::remove_file(&path);
                            Err(None)
                        }
                    };
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    if let Some(pid) = live_pid(&path) {
                        return Err(Some(pid));
                    }
                    // only a lock whose process is gone is stale: one that can't be read is
                    // another program's, left
                    if lock_pid(&path).is_none() {
                        return Err(None);
                    }
                    let _ = fs::remove_file(&path);
                }
                Err(_) => return Err(None),
            }
        }
        Err(None)
    }
}

#[cfg(unix)]
impl Drop for UucpLock {
    fn drop(&mut self) {
        let mine = fs::read_to_string(&self.path)
            .is_ok_and(|text| text.trim() == std::process::id().to_string());
        if mine {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The process with a `flock` or `fcntl` lock on `port` in `locks` (`/proc/locks`, which every
/// user can read): its lines name the file by its filesystem's `major:minor` (hex) and inode
#[cfg(target_os = "linux")]
pub fn lock_holder(port: &str, locks: &str) -> Option<libc::pid_t> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(port).ok()?;
    let dev = meta.dev();
    let file = format!(
        "{:02x}:{:02x}:{}",
        libc::major(dev),
        libc::minor(dev),
        meta.ino()
    );
    let text = std::fs::read_to_string(locks).ok()?;
    text.lines().find_map(|line| {
        // `1: FLOCK  ADVISORY  WRITE 1234 00:05:567 0 EOF`; a waiter's line has `->` second
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [_, kind, _, _, pid, id, ..] if *kind != "->" && *id == file => pid.parse().ok(),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temp folder for `name`
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maxicom_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// MAX-186: maxicom's lock on a link: taken, it says who holds it (this process); a second
    /// take is refused with the holder; released, its files are gone and it's free to take
    /// again.
    /// How: a TCP address locked in a temp folder, read back, taken twice, dropped.
    #[test]
    fn link_lock_taken_read_and_released() {
        let dir = temp_dir("link_lock");
        let address: Address = "tcp://10.0.0.2:4000".parse().unwrap();
        assert_eq!(lock_state_in(&dir, &address), LockState::Free);
        let lock = LinkLock::take_in(&dir, &address).ok().unwrap();
        assert!(dir.join("10.0.0.2_4000.lock").exists());
        let LockState::Held(Some(info)) = lock_state_in(&dir, &address) else {
            panic!("not held");
        };
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.address, "tcp://10.0.0.2:4000");
        assert!(!info.since.is_empty());
        let Err(LockState::Held(Some(holder))) = LinkLock::take_in(&dir, &address) else {
            panic!("taken twice");
        };
        assert_eq!(holder, info);

        drop(lock);
        assert!(!dir.join("10.0.0.2_4000.info").exists());
        #[cfg(unix)]
        assert!(!dir.join("10.0.0.2_4000.lock").exists());
        assert_eq!(lock_state_in(&dir, &address), LockState::Free);
        assert!(LinkLock::take_in(&dir, &address).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-186: what a crash leaves, a lock file and its info that nobody holds, is free and
    /// taken over, the info then this process's.
    /// How: both files written by hand with a PID of no process.
    #[test]
    fn stale_lock_taken_over() {
        let dir = temp_dir("stale_lock");
        let address: Address = "tcp://10.0.0.2:4000".parse().unwrap();
        fs::write(dir.join("10.0.0.2_4000.lock"), "").unwrap();
        fs::write(dir.join("10.0.0.2_4000.info"), "pid=99999999\nuser=gone\n").unwrap();
        assert_eq!(lock_state_in(&dir, &address), LockState::Free);
        let _lock = LinkLock::take_in(&dir, &address).ok().unwrap();
        let LockState::Held(Some(info)) = lock_state_in(&dir, &address) else {
            panic!("not held");
        };
        assert_eq!(info.pid, std::process::id());
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-186: the shared folder is anyone's: a lock file that is a link is neither followed
    /// nor taken, its target untouched; a serial port is locked by the device it leads to, so
    /// a `by-id` link and the device share one lock.
    /// How: a `.lock` symlinked to a file; then a fake device and a link to it.
    #[cfg(unix)]
    #[test]
    fn links_in_the_lock_folder() {
        let dir = temp_dir("lock_links");
        let address: Address = "tcp://10.0.0.2:4000".parse().unwrap();
        let target = dir.join("target");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("10.0.0.2_4000.lock")).unwrap();
        assert!(LinkLock::take_in(&dir, &address).is_err());
        assert_eq!(lock_state_in(&dir, &address), LockState::Free);
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");

        let devices = temp_dir("lock_devices");
        let device = devices.join("ttyFAKE0");
        fs::write(&device, "").unwrap();
        let by_id = devices.join("usb-board-if00");
        std::os::unix::fs::symlink(&device, &by_id).unwrap();
        let serial = |path: &Path| Address::Serial(path.to_str().unwrap().to_string());
        let _lock = LinkLock::take_in(&dir, &serial(&by_id)).ok().unwrap();
        assert!(matches!(
            lock_state_in(&dir, &serial(&device)),
            LockState::Held(Some(_))
        ));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&devices);
    }

    /// MAX-186: the shared folder is made open to every user with the sticky bit, whatever
    /// the umask; a link in its place is refused.
    /// How: a folder made in a temp folder, then a symlink to a folder.
    #[cfg(unix)]
    #[test]
    fn shared_lock_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_dir("lock_folder");
        let dir = root.join("locks");
        shared_dir(&dir).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o1777);
        shared_dir(&dir).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(shared_dir(&link).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    /// MAX-186: a TCP link holds maxicom's lock in the shared folder while connected, gone
    /// once closed. A serial port is in use while this maxicom has it open; another maxicom's
    /// is busy, named, and refused before it's opened.
    /// How: a TCP device connected to a local listener; a pty opened as a port, then locked
    /// with another PID in its info (Linux).
    #[test]
    fn open_links_hold_the_lock() {
        if lock_dir().is_none() {
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("tcp://{}", listener.local_addr().unwrap());
        let address: Address = address.parse().unwrap();
        let Address::Tcp(addr) = &address else {
            unreachable!()
        };
        let device =
            super::super::tcp::TcpDevice::connect(addr, std::time::Duration::from_secs(1)).unwrap();
        assert!(matches!(lock_state(&address), LockState::Held(Some(_))));
        drop(device);
        assert_eq!(lock_state(&address), LockState::Free);

        #[cfg(target_os = "linux")]
        {
            use crate::link::probe::{ProbeState, probe_port};
            let pty = crate::app::tests::Pty::open();
            let line = crate::link::serial::LineSettings::default();
            let port = crate::link::serial::SerialDevice::open(&pty.path, &line).unwrap();
            assert_eq!(probe_port(&pty.path), ProbeState::InUse);
            drop(port);
            assert_eq!(probe_port(&pty.path), ProbeState::Free);

            // another maxicom: the lock held, its info another PID's
            let serial = Address::Serial(pty.path.clone());
            let lock = LinkLock::take(&serial).unwrap();
            let other = "pid=4242\nuser=other\n";
            fs::write(lock_paths(&lock_dir().unwrap(), &serial).1, other).unwrap();
            let held = ProbeState::Busy(Some("maxicom 4242 (other)".to_string()));
            assert_eq!(probe_port(&pty.path), held);
            let Err(crate::link::LinkError::OpenFailed { source, .. }) =
                crate::link::serial::SerialDevice::open(&pty.path, &line)
            else {
                panic!("opened a port another maxicom holds");
            };
            assert_eq!(crate::link::probe::port_state_of(&source), held);
            drop(lock);
        }
    }

    /// MAX-186: maxicom's UUCP lock file holds its PID in the HDB format, is found by
    /// `locked_by`, and goes when dropped; a stale one (no such process) is replaced; a live
    /// one is refused with its PID and left; one another process wrote meanwhile isn't removed;
    /// one with more text after its PID is live, one that can't be read is left, not stale.
    /// How: a temp lock folder; `LCK..ttyFAKE` taken, then written with a dead PID, then with
    /// PID 1 (init, always alive), then rewritten under a held lock, then with more, garbage.
    #[cfg(unix)]
    #[test]
    fn uucp_lock_written() {
        let dir = temp_dir("uucp_lock");
        let dirs = [dir.to_str().unwrap()];
        let path = dir.join("LCK..ttyFAKE");
        let me = std::process::id();
        let lock = UucpLock::take_in(&dir, "LCK..ttyFAKE").ok().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("{me:>10}\n"));
        assert_eq!(locked_by("/dev/ttyFAKE", &dirs), Some(me as libc::pid_t));
        drop(lock);
        assert!(!path.exists());

        fs::write(&path, "  99999999\n").unwrap();
        let _stale = UucpLock::take_in(&dir, "LCK..ttyFAKE").ok().unwrap();
        assert_eq!(locked_by("/dev/ttyFAKE", &dirs), Some(me as libc::pid_t));
        drop(_stale);

        fs::write(&path, "         1\n").unwrap();
        assert!(matches!(
            UucpLock::take_in(&dir, "LCK..ttyFAKE"),
            Err(Some(1))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "         1\n");

        fs::remove_file(&path).unwrap();
        let lock = UucpLock::take_in(&dir, "LCK..ttyFAKE").ok().unwrap();
        fs::write(&path, "         1\n").unwrap();
        drop(lock);
        assert!(path.exists(), "removed another's lock");

        // a lock with more after its PID is live, and one that can't be read is left
        let more = format!("{me:>10} prog user\n");
        fs::write(&path, &more).unwrap();
        assert!(matches!(
            UucpLock::take_in(&dir, "LCK..ttyFAKE"),
            Err(Some(_))
        ));
        fs::write(&path, "garbage").unwrap();
        assert!(matches!(UucpLock::take_in(&dir, "LCK..ttyFAKE"), Err(None)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "garbage");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-186: an open port writes maxicom's UUCP lock where the system lets it, removed on
    /// close; a port with a live UUCP lock of another process is refused before it's opened,
    /// that process named.
    /// How: a pty opened and closed; then a `LCK..` of PID 1 written for it. Skipped where no
    /// lock folder can be written.
    #[cfg(target_os = "linux")]
    #[test]
    fn open_and_uucp_locks() {
        use crate::link::{
            probe::{ProbeState, port_state_of},
            serial::{LineSettings, SerialDevice},
        };
        let pty = crate::app::tests::Pty::open();
        let name = uucp_name(&pty.path).unwrap();
        let writable = (LOCK_DIRS.iter()).find(|dir| {
            let path = std::ffi::CString::new(**dir).unwrap();
            (unsafe { libc::access(path.as_ptr(), libc::W_OK) }) == 0
        });
        let Some(dir) = writable else {
            return;
        };
        let lock = Path::new(dir).join(&name);
        let line = LineSettings::default();
        let port = SerialDevice::open(&pty.path, &line).unwrap();
        let me = std::process::id().to_string();
        assert_eq!(fs::read_to_string(&lock).unwrap().trim(), me);
        drop(port);
        assert!(!lock.exists());

        fs::write(&lock, "         1\n").unwrap();
        let refused = SerialDevice::open(&pty.path, &line);
        let _ = fs::remove_file(&lock);
        let Err(crate::link::LinkError::OpenFailed { source, .. }) = refused else {
            panic!("opened a port a UUCP lock holds");
        };
        match port_state_of(&source) {
            ProbeState::Busy(Some(holder)) => assert!(holder.ends_with("(1)"), "{holder}"),
            state => panic!("{state:?}"),
        }
    }

    /// MAX-181: a lock file names the port by its file name and holds a PID: the test's own
    /// process locks it, a PID of no process or text that isn't one doesn't, nor a lock for
    /// another port. MAX-186: one with more text after the PID does, and so does minicom's name
    /// for a port under a subfolder of `/dev`.
    /// How: a temp lock folder, `LCK..ttyUSB7` written four ways, then `LCK..pts_7`.
    #[cfg(unix)]
    #[test]
    fn lock_files() {
        let dir = std::env::temp_dir().join(format!("maxicom_locks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = [dir.to_str().unwrap()];
        let lock = dir.join("LCK..ttyUSB7");
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        std::fs::write(&lock, format!("{:>10}\n", std::process::id())).unwrap();
        let me = Some(std::process::id() as libc::pid_t);
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), me);
        assert_eq!(locked_by("/dev/ttyUSB8", &dirs), None);
        // past the largest PID Linux and macOS give
        std::fs::write(&lock, "  99999999\n").unwrap();
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        std::fs::write(&lock, "garbage").unwrap();
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        // MAX-186: more text after the PID
        let more = format!("{:>10} prog user\n", std::process::id());
        std::fs::write(&lock, more).unwrap();
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), me);
        // minicom's name for a port in a subfolder of /dev: `pts_7` for `/dev/pts/7`
        let _ = std::fs::remove_file(&lock);
        std::fs::write(
            dir.join("LCK..pts_7"),
            format!("{:>10}\n", std::process::id()),
        )
        .unwrap();
        assert_eq!(locked_by("/dev/pts/7", &dirs), me);
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-181: a `flock` on a file shows in `/proc/locks` under the locking process, for
    /// every user; a file nobody locks isn't there.
    /// How: a temp file locked by the test, another not.
    #[cfg(target_os = "linux")]
    #[test]
    fn flock_holders() {
        use std::os::fd::AsRawFd;
        let dir = std::env::temp_dir().join(format!("maxicom_flock_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let locked = dir.join("ttyFAKE0");
        let file = std::fs::File::create(&locked).unwrap();
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0);
        let free = dir.join("ttyFAKE1");
        std::fs::write(&free, "").unwrap();

        let me = std::process::id() as libc::pid_t;
        assert_eq!(
            lock_holder(locked.to_str().unwrap(), "/proc/locks"),
            Some(me)
        );
        assert_eq!(lock_holder(free.to_str().unwrap(), "/proc/locks"), None);
        drop(file);
        assert_eq!(lock_holder(locked.to_str().unwrap(), "/proc/locks"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
