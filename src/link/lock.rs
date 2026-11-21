//! The locks on a link (MAX-186): maxicom's own on the link it has open, in a folder every
//! user shares, and the ones other programs take, read to say who holds a port (MAX-181)
use std::{
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
};

use jiff::Timestamp;

use super::Address;
use crate::clock;

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
            clock::format(Timestamp::now(), "%Y-%m-%d %H:%M:%S"),
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

/// The lock folders of the UUCP-style programs: minicom's list, the first of them that exists
/// being its own, and `/run/lock`. Each one's `lockdev/` is searched too (`lock_folders`)
#[cfg(unix)]
pub const LOCK_DIRS: &[&str] = &[
    "/etc/locks",
    "/var/lock",
    "/run/lock",
    "/usr/spool/locks",
    "/var/spool/locks",
    "/var/spool/lock",
    "/usr/spool/uucp",
    "/var/spool/uucp",
];

/// The folders of `dirs` and their `lockdev/` (lockdev's: Fedora, RHEL) that exist, each
/// once: `/var/lock` is often a link to `/run/lock`
#[cfg(unix)]
fn lock_folders(dirs: &[&str]) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for dir in dirs {
        for folder in [PathBuf::from(dir), Path::new(dir).join("lockdev")] {
            if let Ok(folder) = fs::canonicalize(&folder)
                && !folders.contains(&folder)
            {
                folders.push(folder);
            }
        }
    }
    folders
}

/// Who holds a port by a UUCP lock file (MAX-268): its running process, or the lock file
/// itself when no PID can be read in it (empty, garbage): another program's lock being
/// written, or a broken one, which only the user can tell and remove
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UucpHolder {
    Process(libc::pid_t),
    NoPid(PathBuf),
}

/// The holder of a UUCP lock file for `port` in one of the lock folders `dirs` (MAX-268)
#[cfg(unix)]
pub fn locked_by(port: &str, dirs: &[&str]) -> Option<UucpHolder> {
    locked_by_under(port, dirs, Path::new("/dev"))
}

/// [`locked_by`], the device folder `dev` given (a temp one in tests). A program names its
/// lock file after the path it was given, an alias as well: each lock file is led back to the
/// device it names (`UucpPort::names`)
#[cfg(unix)]
fn locked_by_under(port: &str, dirs: &[&str], dev: &Path) -> Option<UucpHolder> {
    let port = UucpPort::new(port);
    (lock_folders(dirs).iter())
        .filter_map(|folder| fs::read_dir(folder).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| (entry.file_name().to_str()).is_some_and(|name| port.names(name, dev)))
        .find_map(|entry| uucp_holder(&entry.path()))
}

/// A port as lock files may name it: the names programs give it for the path maxicom was
/// given and the device it leads to, and that device
#[cfg(unix)]
struct UucpPort {
    names: Vec<String>,
    device: Option<fs::Metadata>,
}

#[cfg(unix)]
impl UucpPort {
    /// The names for `port`, path given and device: its last part (`LCK..ttyUSB0`: picocom,
    /// maxicom, minicom outside `/dev`), and the path after `/dev/` with `_` for `/`
    /// (minicom: `LCK..pts_3`) or `:` (lockdev: `LCK..pts:3`). On macOS a port is two
    /// devices, `cu.X` and `tty.X`: each one's names hold the other's
    fn new(port: &str) -> Self {
        let given = PathBuf::from(port);
        let resolved = fs::canonicalize(port).ok();
        let mut names = Vec::new();
        for path in std::iter::once(&given).chain(resolved.as_ref()) {
            let text = path.to_string_lossy();
            let under_dev = text.strip_prefix("/dev/");
            let last = (path.file_name()).map(|name| name.to_string_lossy().into_owned());
            let sibling = last.as_deref().and_then(|last| {
                (last.strip_prefix("cu.").map(|rest| format!("tty.{rest}")))
                    .or_else(|| last.strip_prefix("tty.").map(|rest| format!("cu.{rest}")))
            });
            let forms = [
                under_dev.map(|rest| rest.replace('/', "_")),
                under_dev.map(|rest| rest.replace('/', ":")),
                last,
                sibling,
            ];
            for name in forms.into_iter().flatten() {
                let name = format!("LCK..{name}");
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let device = fs::metadata(port).ok();
        Self { names, device }
    }

    /// Whether the lock file `file` is for this port: by one of its names, else by the device
    /// its name leads to under `dev`, or for lockdev's `LK.<a>.<major>.<minor>` by its
    /// numbers. lockdev's `LCK...<pid>` isn't a port's
    fn names(&self, file: &str, dev: &Path) -> bool {
        if self.names.iter().any(|name| name == file) {
            return true;
        }
        #[cfg(target_os = "linux")]
        if let Some(numbers) = file.strip_prefix("LK.") {
            return self.has_numbers(numbers);
        }
        let name = (file.strip_prefix("LCK..")).filter(|name| !name.starts_with('.'));
        let Some(name) = name else {
            return false;
        };
        let mut paths = vec![dev.join(name.replace(':', "/"))];
        underscore_paths(dev, name, &mut paths);
        (paths.iter()).any(|path| fs::metadata(path).is_ok_and(|meta| self.is(&meta)))
    }

    /// Whether `meta` is this port's device: the same device numbers, or for a file that
    /// isn't a device (tests), the same file
    fn is(&self, meta: &fs::Metadata) -> bool {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let Some(device) = &self.device else {
            return false;
        };
        if device.file_type().is_char_device() && meta.file_type().is_char_device() {
            device.rdev() == meta.rdev()
        } else {
            (device.dev(), device.ino()) == (meta.dev(), meta.ino())
        }
    }

    /// Whether `numbers`, `<a>.<major>.<minor>` of a `LK.` name, are this port's device's
    #[cfg(target_os = "linux")]
    fn has_numbers(&self, numbers: &str) -> bool {
        use std::os::unix::fs::MetadataExt;
        let Some(device) = &self.device else {
            return false;
        };
        let rdev = device.rdev();
        let parts: Vec<Option<u32>> = numbers.split('.').map(|n| n.parse().ok()).collect();
        matches!(parts.as_slice(), [Some(_), Some(major), Some(minor)]
            if *major == libc::major(rdev) && *minor == libc::minor(rdev))
    }
}

/// The paths a lock file's `name` may stand for under `dir`, minicom's `_` for `/` undone:
/// the name as a file of `dir`, and for each `_` after a folder of `dir`, the rest in that
/// folder (`serial_by-id_usb-Silicon_Labs_…` is `serial/by-id/usb-Silicon_Labs_…`)
#[cfg(unix)]
fn underscore_paths(dir: &Path, name: &str, paths: &mut Vec<PathBuf>) {
    paths.push(dir.join(name));
    for (at, _) in name.match_indices('_') {
        let folder = dir.join(&name[..at]);
        if folder.is_dir() {
            underscore_paths(&folder, &name[at + 1..], paths);
        }
    }
}

/// A UUCP lock file's name for `port`: `LCK..` and the name of the device it leads to, so a
/// `by-id` link and the device share it
#[cfg(unix)]
fn uucp_name(port: &str) -> Option<String> {
    let path = fs::canonicalize(port).unwrap_or_else(|_| PathBuf::from(port));
    Some(format!("LCK..{}", path.file_name()?.to_string_lossy()))
}

/// The PID a UUCP lock file holds, written as text first: the first word, as some programs
/// write more after it. Else 4 bytes of a binary PID (Kermit's, old UUCP's: minicom reads it)
#[cfg(unix)]
fn lock_pid(path: &Path) -> Option<libc::pid_t> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let pid = match text.split_whitespace().next().map(str::parse) {
        Some(Ok(pid)) => pid,
        _ => libc::pid_t::from_ne_bytes(bytes.try_into().ok()?),
    };
    Some(pid).filter(|&pid| pid > 0)
}

/// Who holds the UUCP lock file `path`: the process of its PID while it runs, the file when
/// it holds no PID; none when its process is gone (stale)
#[cfg(unix)]
fn uucp_holder(path: &Path) -> Option<UucpHolder> {
    let Some(pid) = lock_pid(path) else {
        return Some(UucpHolder::NoPid(path.to_path_buf()));
    };
    // signal 0 only asks whether the process exists; EPERM: it does, someone else's
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    (alive || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
        .then_some(UucpHolder::Process(pid))
}

/// maxicom's UUCP lock file on a serial port (MAX-186), for minicom, picocom and the other
/// programs that look for one: `LCK..ttyUSB0` holding this PID, in the first lock folder this
/// user can write in (Debian's; on Fedora and RHEL `lockdev/` for a member of the `lock`
/// group, none for the others). Taken before the port is opened, as the convention asks
/// (MAX-268). A stale one, its process gone (a crash), is replaced; removed when dropped while
/// it's still this process's
#[cfg(unix)]
pub struct UucpLock {
    path: PathBuf,
}

#[cfg(unix)]
impl UucpLock {
    /// The lock on `port` in the first lock folder that takes it, none when none does;
    /// refused with the holder of a lock found in one
    pub fn take(port: &str) -> Result<Option<Self>, UucpHolder> {
        let Some(name) = uucp_name(port) else {
            return Ok(None);
        };
        for folder in lock_folders(LOCK_DIRS) {
            match Self::take_in(&folder, &name) {
                Ok(lock) => return Ok(Some(lock)),
                Err(Some(holder)) => return Err(holder),
                Err(None) => {}
            }
        }
        Ok(None)
    }

    /// The lock file `name` in `dir`, refused with its holder; none when this folder can't
    /// take it. The PID is written in a file of its own, then hard-linked to the lock's name
    /// (a second name of the same file, which stays when the first is removed): the link fails
    /// when the name is taken, so two takers can't both have it, and the lock is never seen
    /// empty
    pub fn take_in(dir: &Path, name: &str) -> Result<Self, Option<UucpHolder>> {
        let path = dir.join(name);
        let temp = write_pid(dir).map_err(|_| None)?;
        let taken = link_lock(&temp, &path);
        let _ = fs::remove_file(&temp);
        taken.map(|()| Self { path })
    }
}

/// A new file in `dir` holding this PID in the HDB format (right-aligned on 10 characters),
/// named `LTMP.<pid>.<n>`: never a lock's name, and one per take of this process
#[cfg(unix)]
fn write_pid(dir: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    static TAKES: AtomicU32 = AtomicU32::new(0);
    let me = std::process::id();
    let temp = dir.join(format!(
        "LTMP.{me}.{}",
        TAKES.fetch_add(1, Ordering::Relaxed)
    ));
    // a crash's, from a process of this PID before
    let _ = fs::remove_file(&temp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o644)
        .open(&temp)?;
    match writeln!(file, "{me:>10}") {
        Ok(()) => Ok(temp),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// `temp` linked to the lock file `path`, refused with the holder of the one there; a stale
/// one, its process gone, removed and linked again, once
#[cfg(unix)]
fn link_lock(temp: &Path, path: &Path) -> Result<(), Option<UucpHolder>> {
    for _ in 0..2 {
        match fs::hard_link(temp, path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => match uucp_holder(path) {
                Some(holder) => return Err(Some(holder)),
                None => {
                    let _ = fs::remove_file(path);
                }
            },
            Err(_) => return Err(None),
        }
    }
    Err(None)
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
pub fn flock_holder(port: &str, locks: &str) -> Option<libc::pid_t> {
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
#[path = "../tests/link/lock.rs"]
mod tests;
