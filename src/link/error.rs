//! What can go wrong on a link, whatever its kind (MAX-168): opening, connecting, setting the
//! line, reading, writing, the control lines
use std::{
    error::Error,
    fmt,
    io::{self, ErrorKind},
};

#[derive(Debug)]
pub enum LinkError {
    /// Failed to open port (missing device, permissions, etc.)
    OpenFailed {
        port: String,
        baudrate: u32,
        source: io::Error,
    },
    /// Failed to connect to a network link (refused, unreachable, bad address)
    ConnectFailed { addr: String, source: io::Error },
    /// Failed to configure port options (read timeout, termios flags)
    ConfigFailed(io::Error),
    ReconfigureFailed {
        summary: String,
        rejected_fields: Vec<String>,
        source: io::Error,
    },
    /// The link was closed on the other side, or the USB unplugged: EOF on read, or a read or
    /// write error that says the device is gone (MAX-199)
    InputClosed,
    /// Unexpected error while reading bytes
    ReadFailed(io::Error),
    /// Failed to transmit bytes
    WriteFailed {
        bytes_attempted: usize,
        source: io::Error,
    },
    /// Failed to set a control line (DTR, RTS, break)
    ControlFailed(io::Error),
    /// The link has no control lines: a TCP link, a replay
    NoControlLines,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenFailed {
                port,
                baudrate,
                source,
            } => match source.kind() {
                ErrorKind::PermissionDenied => {
                    write!(f, "permission denied opening serial port '{port}'")
                }
                // a holder maxicom found names it (MAX-186)
                ErrorKind::ResourceBusy => match source.get_ref() {
                    Some(held) if held.is::<crate::link::lock::HeldBy>() => {
                        write!(f, "serial port '{port}' is {held}")
                    }
                    _ => write!(
                        f,
                        "serial port '{port}' is busy (already in use by another application)"
                    ),
                },
                _ => {
                    write!(
                        f,
                        "failed to open serial port '{port}' at {baudrate} baud: {source}"
                    )
                }
            },
            Self::ConnectFailed { addr, source } => {
                write!(f, "could not connect to '{addr}': {source}")
            }
            Self::ConfigFailed(source) => {
                write!(f, "failed to configure port settings: {source}")
            }
            Self::ReconfigureFailed {
                summary,
                rejected_fields,
                source,
            } => {
                if !rejected_fields.is_empty() {
                    write!(
                        f,
                        "Hardware driver rejected: {}.\n\
                         Requested [{summary}], but device kept previous attributes.",
                        rejected_fields.join(", ")
                    )
                } else {
                    write!(f, "Driver error applying [{summary}]: {source}")
                }
            }
            Self::InputClosed => {
                write!(
                    f,
                    "the link closed (unplugged, or closed on the other side)"
                )
            }
            Self::ReadFailed(source) => {
                write!(f, "failed to read from the link: {source}")
            }
            Self::WriteFailed {
                bytes_attempted,
                source,
            } => {
                write!(
                    f,
                    "failed to write {bytes_attempted} bytes to the link: {source}"
                )
            }
            Self::ControlFailed(source) => write!(f, "failed to set the line: {source}"),
            Self::NoControlLines => write!(f, "the link has no control lines"),
        }
    }
}

impl Error for LinkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::OpenFailed { source, .. } => Some(source),
            Self::ConnectFailed { source, .. } => Some(source),
            Self::ConfigFailed(source) => Some(source),
            Self::ReconfigureFailed { source, .. } => Some(source),
            Self::ReadFailed(source) => Some(source),
            Self::WriteFailed { source, .. } => Some(source),
            Self::ControlFailed(source) => Some(source),
            Self::InputClosed | Self::NoControlLines => None,
        }
    }
}

/// A read or write error that means the device is gone, unplugged or its connection dropped
/// (MAX-199): the link closes as for a read of 0, with the lost popup
/// the codes come from the OS docs and drivers; only Windows' 22 was seen (MAX-194)
pub fn is_gone(err: &io::Error) -> bool {
    if matches!(
        err.kind(),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe
    ) {
        return true;
    }
    let Some(code) = err.raw_os_error() else {
        return false;
    };
    // EIO: Linux, macOS; ENXIO: macOS's unplugged /dev/cu.*; ENODEV: Linux
    #[cfg(unix)]
    let gone = matches!(code, libc::EIO | libc::ENXIO | libc::ENODEV);
    // ERROR_BAD_COMMAND, ERROR_GEN_FAILURE, ERROR_OPERATION_ABORTED,
    // ERROR_DEVICE_NOT_CONNECTED, ERROR_DEVICE_REMOVED, ERROR_INVALID_HANDLE, and
    // ERROR_ACCESS_DENIED, what some drivers give after a removal
    #[cfg(windows)]
    let gone = matches!(code, 22 | 31 | 995 | 1167 | 1617 | 6 | 5);
    #[cfg(not(any(unix, windows)))]
    let gone = false;
    gone
}

#[cfg(test)]
#[path = "../tests/link/error.rs"]
mod tests;
