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
    /// Port was closed on the other side or USB unplugged (EOF on read)
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
                ErrorKind::ResourceBusy => {
                    write!(
                        f,
                        "serial port '{port}' is busy (already in use by another application)"
                    )
                }
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
                write!(f, "serial device input closed (EOF / cable unplugged)")
            }
            Self::ReadFailed(source) => {
                write!(f, "failed to read from serial port: {source}")
            }
            Self::WriteFailed {
                bytes_attempted,
                source,
            } => {
                write!(
                    f,
                    "failed to write {bytes_attempted} bytes to serial port: {source}"
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
