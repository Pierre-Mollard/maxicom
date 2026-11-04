//! The directory's test of a link (MAX-181, MAX-186): whether it can be used, and who holds
//! it, without opening a port where the system tells otherwise; a TCP link by a connect
use std::{
    io::ErrorKind,
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

#[cfg(not(unix))]
use serial2::SerialPort;

use super::{
    Address,
    lock::{HeldBy, LockInfo, LockState, lock_state},
};

#[cfg(target_os = "linux")]
use super::lock::lock_holder;
#[cfg(unix)]
use super::lock::{LOCK_DIRS, locked_by};

/// A TCP link's test gives up after this, its addresses all tried: within the directory's
/// own timeout. Longer on Windows: a refused port there is only told after about 2 s of
/// retries
const TCP_PROBE_TIMEOUT: Duration = if cfg!(windows) {
    Duration::from_millis(2500)
} else {
    Duration::from_millis(1500)
};

/// What the directory's test found for a link (MAX-181, MAX-186): `InUse` is this maxicom,
/// under another name of the device; `Busy` another program, named when it's known (`minicom
/// (1234)`, `maxicom 1234 (pierre)`); `NoAccess` the group to join when it's one; `Missing` no
/// such device (unplugged, a saved port of another setup); `NoLock` on macOS, where a program
/// holding a port without a lock can't be seen; then a TCP link's answer (`Unreachable`: no
/// answer, or no route); `NoAnswer` the test's time up
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeState {
    Free,
    InUse,
    Busy(Option<String>),
    NoAccess(Option<String>),
    Missing,
    #[cfg_attr(not(unix), allow(dead_code))]
    NoLock,
    Reachable,
    Refused,
    Unreachable,
    UnknownHost,
    NoAnswer,
    Error(String),
}

/// What `address` gives: a serial port tested without opening it, a TCP link by a connect
/// closed at once, a replay's file by whether it's there
pub fn probe(address: &Address) -> ProbeState {
    match address {
        Address::Serial(port) => probe_port(port),
        Address::Tcp(addr) => probe_tcp(addr),
        Address::Replay(path) if path.is_file() => ProbeState::Free,
        Address::Replay(_) => ProbeState::Missing,
    }
}

/// Whether `port` can be used (MAX-181), without opening it where the system tells otherwise:
/// an open raises DTR and RTS, which resets a board with an auto-reset (ESP32, Arduino). First
/// a maxicom's lock (MAX-186), on every system. On Linux a live UUCP lock file (minicom's),
/// then a `flock` / `fcntl` lock in `/proc/locks` (picocom's, tio's, pyserial's, maxicom's):
/// both seen whoever the user; then a process of the user's own holding it (its open files in
/// `/proc`), then the permissions. On macOS the lock file and the permissions, a port with no
/// lock said as such. On Windows only an open tells: one with the port's settings kept, closed
/// at once
pub fn probe_port(port: &str) -> ProbeState {
    if let LockState::Held(info) = lock_state(&Address::Serial(port.to_string())) {
        return held_state(info.as_ref());
    }
    #[cfg(unix)]
    {
        if let Some(pid) = locked_by(port, LOCK_DIRS) {
            return pid_state(pid);
        }
        #[cfg(target_os = "linux")]
        if let Some(pid) = lock_holder(port, "/proc/locks") {
            return pid_state(pid);
        }
        #[cfg(target_os = "linux")]
        if let Some(holder) = open_holder(port, std::process::id()) {
            return ProbeState::Busy(Some(holder));
        }
        let Ok(path) = std::ffi::CString::new(port) else {
            return ProbeState::Error("not a path".to_string());
        };
        if unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK) } != 0 {
            return match port_state_of(&std::io::Error::last_os_error()) {
                ProbeState::NoAccess(_) => ProbeState::NoAccess(group_of(port)),
                state => state,
            };
        }
        if cfg!(target_os = "macos") {
            ProbeState::NoLock
        } else {
            ProbeState::Free
        }
    }
    #[cfg(not(unix))]
    match SerialPort::open(port, serial2::KeepSettings) {
        Ok(_) => ProbeState::Free,
        Err(err) => port_state_of(&err),
    }
}

/// Whether the TCP link `addr` (`host:port`) answers: a maxicom's lock first, then a connect
/// to each address the name gives, closed at once. A one-client bridge may drop the client it
/// has for it, and the device sees a connection: only on the directory's `t`
pub fn probe_tcp(addr: &str) -> ProbeState {
    if let LockState::Held(info) = lock_state(&Address::Tcp(addr.to_string())) {
        return held_state(info.as_ref());
    }
    let Ok(sockets) = addr.to_socket_addrs() else {
        return ProbeState::UnknownHost;
    };
    let deadline = Instant::now() + TCP_PROBE_TIMEOUT;
    let mut state = ProbeState::UnknownHost;
    for socket in sockets {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&socket, left) {
            Ok(_) => return ProbeState::Reachable,
            Err(err) if err.kind() == ErrorKind::ConnectionRefused => state = ProbeState::Refused,
            Err(_) if state == ProbeState::Refused => {}
            Err(_) => state = ProbeState::Unreachable,
        }
    }
    state
}

/// A maxicom's lock: this one's, or another's named
fn held_state(info: Option<&LockInfo>) -> ProbeState {
    match info {
        Some(info) if info.pid == std::process::id() => ProbeState::InUse,
        info => ProbeState::Busy(Some(maxicom_label(info))),
    }
}

/// A lock's process: this one, or another named
#[cfg(unix)]
fn pid_state(pid: libc::pid_t) -> ProbeState {
    if pid as u32 == std::process::id() {
        ProbeState::InUse
    } else {
        ProbeState::Busy(Some(process_label(pid)))
    }
}

/// Another maxicom as the directory names it: `maxicom 1234 (pierre)`, `maxicom` when its
/// lock's info can't be read
pub fn maxicom_label(info: Option<&LockInfo>) -> String {
    match info {
        Some(info) if info.user.is_empty() => format!("maxicom {}", info.pid),
        Some(info) => format!("maxicom {} ({})", info.pid, info.user),
        None => "maxicom".to_string(),
    }
}

/// An open's or an access check's error: a lock maxicom found names its holder (MAX-186);
/// Windows refuses a port in use with access denied, a Unix port held with `TIOCEXCL` answers
/// busy; a device that isn't there is missing
pub fn port_state_of(err: &std::io::Error) -> ProbeState {
    if let Some(HeldBy(holder)) = err.get_ref().and_then(|err| err.downcast_ref::<HeldBy>()) {
        return ProbeState::Busy(Some(holder.clone()));
    }
    #[cfg(unix)]
    if err.raw_os_error() == Some(libc::EBUSY) {
        return ProbeState::Busy(None);
    }
    match err.kind() {
        ErrorKind::PermissionDenied if cfg!(windows) => ProbeState::Busy(None),
        ErrorKind::PermissionDenied => ProbeState::NoAccess(None),
        ErrorKind::NotFound => ProbeState::Missing,
        _ => ProbeState::Error(err.to_string()),
    }
}

/// The group owning `port`, which the user joins to use it (`dialout`, `uucp`); none for
/// root's, which no one joins
#[cfg(unix)]
fn group_of(port: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let gid = std::fs::metadata(port).ok()?.gid();
    if gid == 0 {
        return None;
    }
    let mut group = unsafe { std::mem::zeroed::<libc::group>() };
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut found = std::ptr::null_mut();
    // the buffer is valid for its length, which bounds what is written; `found` points at
    // `group` when the group is known
    let res = unsafe { libc::getgrgid_r(gid, &mut group, buf.as_mut_ptr(), buf.len(), &mut found) };
    if res != 0 || found.is_null() {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(group.gr_name) };
    Some(name.to_string_lossy().into_owned())
}

/// A process of the user's own but `skip` with `port` open, none for a pty: the program that
/// made it (socat, a test's pty) holds it open too, and isn't using it (MAX-186)
#[cfg(target_os = "linux")]
pub fn open_holder(port: &str, skip: u32) -> Option<String> {
    if is_pty(port) {
        return None;
    }
    holder_of(port, "/proc", skip)
}

/// Whether `port` is the slave end of a pty, through a link too: majors 136 to 143 on Linux
#[cfg(target_os = "linux")]
fn is_pty(port: &str) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    std::fs::metadata(port).is_ok_and(|meta| {
        meta.file_type().is_char_device() && (136..=143).contains(&libc::major(meta.rdev()))
    })
}

/// The first process but `skip` with `port` open, read from `proc`'s `<pid>/fd` links: the
/// ones of the user's processes, the others' can't be read
#[cfg(target_os = "linux")]
fn holder_of(port: &str, proc: &str, skip: u32) -> Option<String> {
    let target = std::fs::canonicalize(port).ok()?;
    let pids = std::fs::read_dir(proc).ok()?.flatten().filter_map(|entry| {
        let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
        (pid != skip).then_some((pid, entry.path()))
    });
    for (pid, dir) in pids {
        let Ok(fds) = std::fs::read_dir(dir.join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|link| link == target));
        if holds {
            return Some(process_label(pid as libc::pid_t));
        }
    }
    None
}

/// A process as the directory names it: `minicom (1234)`, the PID alone where its name can't
/// be read
#[cfg(unix)]
pub fn process_label(pid: libc::pid_t) -> String {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok();
    match name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => format!("{name} ({pid})"),
        _ => format!("pid {pid}"),
    }
}

#[cfg(test)]
#[path = "../tests/link/probe.rs"]
mod tests;
