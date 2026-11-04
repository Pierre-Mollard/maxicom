//! Which io errors mean the device is gone (MAX-199).
use super::*;

/// A dropped connection is gone on every OS; a timeout, a bad argument or an error with no
/// OS code isn't.
/// How: io errors built from their kind.
#[test]
fn dropped_connections_are_gone() {
    for kind in [
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
    ] {
        assert!(is_gone(&io::Error::from(kind)), "{kind:?}");
    }
    for kind in [
        ErrorKind::TimedOut,
        ErrorKind::InvalidInput,
        ErrorKind::PermissionDenied,
        ErrorKind::Other,
    ] {
        assert!(!is_gone(&io::Error::from(kind)), "{kind:?}");
    }
}

/// The unplug errors of Linux and macOS are gone; EINVAL and EISDIR aren't.
/// How: io errors built from their errno.
#[cfg(unix)]
#[test]
fn unix_unplug_codes_are_gone() {
    for code in [libc::EIO, libc::ENXIO, libc::ENODEV] {
        assert!(is_gone(&io::Error::from_raw_os_error(code)), "{code}");
    }
    for code in [libc::EINVAL, libc::EISDIR] {
        assert!(!is_gone(&io::Error::from_raw_os_error(code)), "{code}");
    }
}

/// The removal errors of Windows' serial drivers are gone, 22 the one seen in MAX-194;
/// ERROR_INVALID_PARAMETER (87) isn't.
/// How: io errors built from their Windows code.
#[cfg(windows)]
#[test]
fn windows_removal_codes_are_gone() {
    for code in [22, 31, 995, 1167, 1617, 6, 5] {
        assert!(is_gone(&io::Error::from_raw_os_error(code)), "{code}");
    }
    assert!(!is_gone(&io::Error::from_raw_os_error(87)));
}
