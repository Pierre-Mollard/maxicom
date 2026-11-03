//! Where the link's bytes come from (MAX-166): read once from the form the CLI, `open` and a
//! session's meta file take, and checked there (MAX-168), then matched on, never parsed again.
//! The kind, what the link can do, is taken from it and never stored next to it
use std::{borrow::Cow, fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::LinkKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// A device path, `/dev/ttyUSB0`, or a port name, `COM3`
    Serial(String),
    /// `host:port`, written `tcp://host:port`
    Tcp(String),
    /// The file a replay reads, written `file://path`
    Replay(PathBuf),
}

/// `tcp://host:port` is a TCP link, `file://path` a replay, anything else a serial device.
/// The prefixes are lowercase only: `FILE://x` is a device name. Refused: an empty device or
/// path, and a TCP address that isn't `host:port`, as it would never open
impl FromStr for Address {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(path) = text.strip_prefix("file://") {
            if path.is_empty() {
                return Err("file:// needs a path".to_string());
            }
            Ok(Address::Replay(PathBuf::from(path)))
        } else if let Some(addr) = text.strip_prefix("tcp://") {
            check_host_port(addr)?;
            Ok(Address::Tcp(addr.to_string()))
        } else if text.is_empty() {
            Err("the device is empty".to_string())
        } else {
            Ok(Address::Serial(text.to_string()))
        }
    }
}

/// Whether `addr` has the `host:port` shape, without resolving it
fn check_host_port(addr: &str) -> Result<(), String> {
    match addr.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => Ok(()),
        _ => Err(format!("'{addr}' is not host:port")),
    }
}

impl Address {
    pub fn kind(&self) -> LinkKind {
        match self {
            Address::Serial(_) => LinkKind::Serial,
            Address::Tcp(_) => LinkKind::Tcp,
            Address::Replay(_) => LinkKind::Replay,
        }
    }

    /// What the status bar shows: the whole address, but a replay's file by its name only, as
    /// its folder says nothing about the link
    pub fn label(&self) -> String {
        match self {
            Address::Replay(_) => format!("file://{}", self.tag()),
            _ => self.to_string(),
        }
    }

    /// The last part of the address: `ttyUSB0`, `COM3`, `host:port`, a replay's file name
    pub fn tag(&self) -> Cow<'_, str> {
        match self {
            Address::Serial(device) => Cow::Borrowed(last_part(device)),
            Address::Tcp(addr) => Cow::Borrowed(addr),
            Address::Replay(path) => match path.to_string_lossy() {
                Cow::Borrowed(text) => Cow::Borrowed(last_part(text)),
                Cow::Owned(text) => Cow::Owned(last_part(&text).to_string()),
            },
        }
    }

    /// The tag kept to the characters safe in a file name: `/dev/ttyUSB0` gives `ttyUSB0`,
    /// `tcp://192.168.1.50:4000` gives `192.168.1.50_4000` (no `:` on Windows). For the
    /// session names (MAX-156) and the lock files (MAX-186)
    pub fn file_tag(&self) -> String {
        (self.tag().chars())
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
}

/// After the last `/` or `\`: a Windows path splits on `\` on any platform
fn last_part(text: &str) -> &str {
    text.rsplit(['/', '\\']).next().unwrap_or(text)
}

/// The form `from_str` reads back
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Address::Serial(device) => write!(f, "{device}"),
            Address::Tcp(addr) => write!(f, "tcp://{addr}"),
            Address::Replay(path) => write!(f, "file://{}", path.display()),
        }
    }
}

/// As its text form, so a session's meta file keeps its `port = "/dev/ttyUSB0"`
impl Serialize for Address {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "../tests/link/address.rs"]
mod tests;
