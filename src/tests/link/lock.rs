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
