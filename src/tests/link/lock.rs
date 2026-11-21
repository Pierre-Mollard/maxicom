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
/// one with more text after its PID is live; MAX-268: one with no PID that can be read holds
/// it, named by its path, and is left.
/// MAX-268: the file the PID is first written in is gone after each take.
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
    assert_eq!(
        locked_by("/dev/ttyFAKE", &dirs),
        Some(UucpHolder::Process(me as libc::pid_t))
    );
    drop(lock);
    assert!(!path.exists());

    fs::write(&path, "  99999999\n").unwrap();
    let _stale = UucpLock::take_in(&dir, "LCK..ttyFAKE").ok().unwrap();
    assert_eq!(
        locked_by("/dev/ttyFAKE", &dirs),
        Some(UucpHolder::Process(me as libc::pid_t))
    );
    drop(_stale);

    fs::write(&path, "         1\n").unwrap();
    assert!(matches!(
        UucpLock::take_in(&dir, "LCK..ttyFAKE"),
        Err(Some(UucpHolder::Process(1)))
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
    assert_eq!(
        UucpLock::take_in(&dir, "LCK..ttyFAKE").err(),
        Some(Some(UucpHolder::NoPid(path.clone())))
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "garbage");
    let names: Vec<_> = (fs::read_dir(&dir).unwrap().flatten())
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, ["LCK..ttyFAKE"]);
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-186: an open port writes maxicom's UUCP lock where the system lets it, removed on
/// close; a port with a live UUCP lock of another process is refused before it's opened,
/// that process named. MAX-268: an empty one too, its path in the error; the lock file is
/// taken before the open, and goes when a later step refuses the port.
/// How: a pty opened and closed; then a `LCK..` of PID 1 written for it, then an empty one;
/// then the pty
/// locked with a `flock` on another fd of the test's, which the check before the open lets
/// by as this process's. Skipped where no lock folder can be written.
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

    // MAX-268: an empty lock file holds it too, the error naming the file to remove
    fs::write(&lock, "").unwrap();
    let refused = SerialDevice::open(&pty.path, &line);
    let _ = fs::remove_file(&lock);
    let Err(err) = refused else {
        panic!("opened a port an empty UUCP lock holds");
    };
    // named in the folder it was read in, `/var/lock` by its real path `/run/lock`
    let read = fs::canonicalize(dir).unwrap().join(&name);
    let named = format!(
        "held by a lock with no PID, {} (remove it if stale)",
        read.display()
    );
    assert!(err.to_string().ends_with(&named), "{err}");

    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    let other = (fs::File::options().read(true).write(true))
        .custom_flags(libc::O_NOCTTY)
        .open(&pty.path)
        .unwrap();
    assert_eq!(unsafe { libc::flock(other.as_raw_fd(), libc::LOCK_EX) }, 0);
    assert!(SerialDevice::open(&pty.path, &line).is_err());
    assert!(!lock.exists(), "lock file left by a refused open");
}

/// MAX-181: a lock file names the port by its file name and holds a PID: the test's own
/// process locks it, a PID of no process doesn't, nor a lock for another port; MAX-268: text
/// that isn't a PID or an empty file holds it, by the lock file. MAX-186: one with more text
/// after the PID does, and so does minicom's name for a port under a subfolder of `/dev`.
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
    let me = Some(UucpHolder::Process(std::process::id() as libc::pid_t));
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), me);
    assert_eq!(locked_by("/dev/ttyUSB8", &dirs), None);
    // past the largest PID Linux and macOS give
    std::fs::write(&lock, "  99999999\n").unwrap();
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
    // named in the folder it was read in, by its real path: the temp folder is under a link
    // on macOS (`/var` to `/private/var`)
    let read = fs::canonicalize(&dir).unwrap().join("LCK..ttyUSB7");
    let no_pid = Some(UucpHolder::NoPid(read));
    std::fs::write(&lock, "garbage").unwrap();
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), no_pid);
    std::fs::write(&lock, "").unwrap();
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), no_pid);
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

/// MAX-268: Fedora's and RHEL's minicom locks through lockdev, in a `lockdev/` subfolder of
/// the lock folder (`/run/lock/lockdev`), naming a port in a subfolder of `/dev` with `:` for
/// `/` (`LCK..pts:7`). A live one there holds the port, one of no process doesn't.
/// How: a temp lock folder with a `lockdev/` subfolder, `LCK..ttyUSB7` then `LCK..pts:7`
/// written in it, as `scripts/test_lock_minicom.sh` saw another user's minicom leave them.
#[cfg(unix)]
#[test]
fn lockdev_lock_files() {
    let dir = temp_dir("lockdev");
    let lockdev = dir.join("lockdev");
    fs::create_dir(&lockdev).unwrap();
    let dirs = [dir.to_str().unwrap()];
    let me = Some(UucpHolder::Process(std::process::id() as libc::pid_t));
    let pid = format!("{:>10}\n", std::process::id());
    fs::write(lockdev.join("LCK..ttyUSB7"), &pid).unwrap();
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), me);
    fs::write(lockdev.join("LCK..ttyUSB7"), "  99999999\n").unwrap();
    assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
    fs::write(lockdev.join("LCK..pts:7"), &pid).unwrap();
    assert_eq!(locked_by("/dev/pts/7", &dirs), me);
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-268: a lock file is named after the path its program was given, an alias too: it's
/// led back to the device it names, so a port is held whatever path each side uses. A udev
/// alias (`LCK..board`), minicom's `by-id` name with `_` for `/` and `_` of its own, lockdev's
/// with `:`; a name for another device doesn't hold it, nor lockdev's `LCK...<pid>`.
/// How: a temp `dev` folder, `ttyFAKE0` (a plain file) with links to it, and one lock file at
/// a time with this PID, the port asked for as `ttyFAKE0`.
#[cfg(unix)]
#[test]
fn lock_files_of_aliases() {
    use std::os::unix::fs::symlink;
    let dir = temp_dir("alias_locks");
    let dev = temp_dir("alias_dev");
    let dirs = [dir.to_str().unwrap()];
    let device = dev.join("ttyFAKE0");
    fs::write(&device, "").unwrap();
    fs::write(dev.join("ttyFAKE1"), "").unwrap();
    symlink(&device, dev.join("board")).unwrap();
    fs::create_dir_all(dev.join("serial/by-id")).unwrap();
    symlink(
        &device,
        dev.join("serial/by-id/usb-Silicon_Labs_CP2102-if00"),
    )
    .unwrap();

    let port = device.to_str().unwrap();
    let me = Some(UucpHolder::Process(std::process::id() as libc::pid_t));
    let pid = format!("{:>10}\n", std::process::id());
    let held_by = |name: &str| {
        fs::write(dir.join(name), &pid).unwrap();
        let holder = locked_by_under(port, &dirs, &dev);
        fs::remove_file(dir.join(name)).unwrap();
        holder
    };
    assert_eq!(held_by("LCK..board"), me);
    assert_eq!(
        held_by("LCK..serial_by-id_usb-Silicon_Labs_CP2102-if00"),
        me
    );
    assert_eq!(
        held_by("LCK..serial:by-id:usb-Silicon_Labs_CP2102-if00"),
        me
    );
    assert_eq!(held_by("LCK..ttyFAKE1"), None);
    assert_eq!(held_by(&format!("LCK...{}", std::process::id())), None);
    // the other way: maxicom through the alias, the lock file for the device
    let alias = dev.join("board");
    fs::write(dir.join("LCK..ttyFAKE0"), &pid).unwrap();
    assert_eq!(locked_by_under(alias.to_str().unwrap(), &dirs, &dev), me);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&dev);
}

/// MAX-268: lockdev's `LK.<a>.<major>.<minor>` lock file names the device by its numbers,
/// whatever path its program used; another device's numbers don't hold it.
/// How: a pty's numbers read, `LK.` files written with them and with another minor.
#[cfg(target_os = "linux")]
#[test]
fn lockdev_device_numbers() {
    use std::os::unix::fs::MetadataExt;
    let dir = temp_dir("lk_locks");
    let dirs = [dir.to_str().unwrap()];
    let pty = crate::app::tests::Pty::open();
    let rdev = fs::metadata(&pty.path).unwrap().rdev();
    let (major, minor) = (libc::major(rdev), libc::minor(rdev));
    let pid = format!("{:>10}\n", std::process::id());
    let other = dir.join(format!("LK.000.{major:03}.{:03}", minor + 1));
    fs::write(&other, &pid).unwrap();
    assert_eq!(locked_by(&pty.path, &dirs), None);
    fs::write(dir.join(format!("LK.000.{major:03}.{minor:03}")), &pid).unwrap();
    assert_eq!(
        locked_by(&pty.path, &dirs),
        Some(UucpHolder::Process(std::process::id() as libc::pid_t))
    );
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-268: what minicom reads besides a text PID, a binary one of 4 bytes (Kermit's lock
/// files), holds the port; on macOS a port is two devices, `cu.X` and `tty.X`, and a lock on
/// either holds both.
/// How: `LCK..ttyFAKE` with this PID's bytes; `LCK..tty.usbserial-1` asked for as
/// `/dev/cu.usbserial-1`, then the other way.
#[cfg(unix)]
#[test]
fn binary_pids_and_mac_pairs() {
    let dir = temp_dir("binary_locks");
    let dirs = [dir.to_str().unwrap()];
    let me = Some(UucpHolder::Process(std::process::id() as libc::pid_t));
    fs::write(
        dir.join("LCK..ttyFAKE"),
        (std::process::id() as libc::pid_t).to_ne_bytes(),
    )
    .unwrap();
    assert_eq!(locked_by("/dev/ttyFAKE", &dirs), me);
    let pid = format!("{:>10}\n", std::process::id());
    fs::write(dir.join("LCK..tty.usbserial-1"), &pid).unwrap();
    assert_eq!(locked_by("/dev/cu.usbserial-1", &dirs), me);
    fs::remove_file(dir.join("LCK..tty.usbserial-1")).unwrap();
    fs::write(dir.join("LCK..cu.usbserial-1"), &pid).unwrap();
    assert_eq!(locked_by("/dev/tty.usbserial-1", &dirs), me);
    assert_eq!(locked_by("/dev/tty.usbserial-2", &dirs), None);
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-268: a port another user's minicom holds first is refused, its holder named, as
/// minicom refuses one maxicom holds first. Needs that minicom running: ignored, run by
/// `scripts/test_lock_minicom.sh` in a Fedora or Debian container, the port in `MAXICOM_HELD_PORT`.
/// How: the port probed, then opened; both say busy, held by minicom.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs another user's minicom: scripts/test_lock_minicom.sh"]
fn other_users_minicom_refused() {
    use crate::link::{
        probe::{ProbeState, port_state_of, probe_port},
        serial::{LineSettings, SerialDevice},
    };
    let port = std::env::var("MAXICOM_HELD_PORT").expect("MAXICOM_HELD_PORT");
    let held = |state: &ProbeState| matches!(state, ProbeState::Busy(Some(holder)) if holder.starts_with("minicom"));
    let probed = probe_port(&port);
    assert!(held(&probed), "probe: {probed:?}");
    let Err(crate::link::LinkError::OpenFailed { source, .. }) =
        SerialDevice::open(&port, &LineSettings::default())
    else {
        panic!("opened a port another user's minicom holds");
    };
    let refused = port_state_of(&source);
    assert!(held(&refused), "open: {refused:?}");
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
        flock_holder(locked.to_str().unwrap(), "/proc/locks"),
        Some(me)
    );
    assert_eq!(flock_holder(free.to_str().unwrap(), "/proc/locks"), None);
    drop(file);
    assert_eq!(flock_holder(locked.to_str().unwrap(), "/proc/locks"), None);
    let _ = std::fs::remove_dir_all(&dir);
}
