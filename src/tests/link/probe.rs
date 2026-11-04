use super::*;

/// MAX-181: an open's or an access check's error says how the port is held: EBUSY (a
/// `TIOCEXCL` holder) is busy, access denied is busy on Windows and no access elsewhere,
/// anything else its text. MAX-186: a port that doesn't exist is missing; an open maxicom
/// refused for a lock names its holder.
#[test]
fn port_states_of_open_errors() {
    use std::io::Error;
    let held = crate::link::lock::held_by("minicom (12)".to_string());
    assert_eq!(held.to_string(), "held by minicom (12)");
    assert_eq!(
        port_state_of(&held),
        ProbeState::Busy(Some("minicom (12)".to_string()))
    );
    #[cfg(unix)]
    assert_eq!(
        port_state_of(&Error::from_raw_os_error(libc::EBUSY)),
        ProbeState::Busy(None)
    );
    let denied = port_state_of(&Error::from(ErrorKind::PermissionDenied));
    let expected = if cfg!(windows) {
        ProbeState::Busy(None)
    } else {
        ProbeState::NoAccess(None)
    };
    assert_eq!(denied, expected);
    let missing = port_state_of(&Error::new(ErrorKind::NotFound, "no such device"));
    assert_eq!(missing, ProbeState::Missing);
    assert_eq!(probe_port("/nonexistent/ttyX"), ProbeState::Missing);
    let other = port_state_of(&Error::other("broken"));
    assert_eq!(other, ProbeState::Error("broken".to_string()));
}

/// MAX-181: on Linux, a file some process holds open is found in `/proc` with that
/// process's name and PID, through a symlink too; the skipped process (maxicom itself)
/// isn't, nor a file nobody holds; a readable port nobody holds is free.
/// How: a temp file this test holds open, a symlink to it, a second file closed.
#[cfg(target_os = "linux")]
#[test]
fn holders_in_proc() {
    let dir = std::env::temp_dir().join(format!("maxicom_holder_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let held = dir.join("ttyFAKE0");
    let _open = std::fs::File::create(&held).unwrap();
    let link = dir.join("by-id");
    let _ = std::os::unix::fs::symlink(&held, &link);
    let free = dir.join("ttyFAKE1");
    std::fs::write(&free, "").unwrap();

    let me = std::process::id();
    let holder = holder_of(held.to_str().unwrap(), "/proc", 0).expect("held");
    assert!(holder.ends_with(&format!("({me})")), "{holder}");
    assert_eq!(holder_of(link.to_str().unwrap(), "/proc", 0), Some(holder));
    assert_eq!(holder_of(held.to_str().unwrap(), "/proc", me), None);
    assert_eq!(holder_of(free.to_str().unwrap(), "/proc", 0), None);
    assert_eq!(probe_port(free.to_str().unwrap()), ProbeState::Free);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-186: a port this maxicom holds a `flock` on is in use (another process's is busy,
/// named); a port maxicom only holds open is free; one the user can't read or write is no
/// access, with the group owning it when it isn't root's.
/// How: three temp files: one locked, one held open, one with no permissions (skipped as
/// root, who passes any permission check).
#[cfg(target_os = "linux")]
#[test]
fn probe_states_of_ports() {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    let dir = std::env::temp_dir().join(format!("maxicom_probe_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_string();

    let locked = std::fs::File::create(path("ttyLOCKED")).unwrap();
    assert_eq!(unsafe { libc::flock(locked.as_raw_fd(), libc::LOCK_EX) }, 0);
    assert_eq!(probe_port(&path("ttyLOCKED")), ProbeState::InUse);
    drop(locked);
    assert_eq!(probe_port(&path("ttyLOCKED")), ProbeState::Free);

    let _open = std::fs::File::create(path("ttyOPEN")).unwrap();
    assert_eq!(probe_port(&path("ttyOPEN")), ProbeState::Free);

    std::fs::write(path("ttyDENIED"), "").unwrap();
    let none = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(path("ttyDENIED"), none).unwrap();
    if unsafe { libc::geteuid() } != 0 {
        let group = group_of(&path("ttyDENIED"));
        // the user's own group, named
        assert!(group.is_some() || unsafe { libc::getegid() } == 0);
        assert_eq!(probe_port(&path("ttyDENIED")), ProbeState::NoAccess(group));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
