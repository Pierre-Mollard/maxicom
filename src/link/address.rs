//! Where the link's bytes come from (MAX-166): read once from the form the CLI, `open` and the
//! settings dialog take, then matched on, never parsed again. The kind, what the link can do,
//! is taken from it and never stored next to it
use std::{borrow::Cow, fmt, path::PathBuf};

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

impl Address {
    /// `tcp://host:port` is a TCP link, `file://path` a replay, anything else a serial device.
    /// The prefixes are lowercase only: `FILE://x` is a device name
    pub fn parse(text: &str) -> Self {
        if let Some(path) = text.strip_prefix("file://") {
            Address::Replay(PathBuf::from(path))
        } else if let Some(addr) = text.strip_prefix("tcp://") {
            Address::Tcp(addr.to_string())
        } else {
            Address::Serial(text.to_string())
        }
    }

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
}

/// After the last `/` or `\`: a Windows path splits on `\` on any platform
fn last_part(text: &str) -> &str {
    text.rsplit(['/', '\\']).next().unwrap_or(text)
}

/// The form `parse` reads back
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
        Ok(Address::parse(&text))
    }
}

#[cfg(test)]
mod tests {
    //! The forms an address is read from and written back as.
    use super::*;

    /// Each form gives its kind, writes back the same text, and shows its label and tag.
    /// How: a table of addresses of each kind, a Windows replay path among them.
    #[test]
    fn address_forms() {
        let table = [
            ("/dev/ttyUSB0", LinkKind::Serial, "/dev/ttyUSB0", "ttyUSB0"),
            ("COM3", LinkKind::Serial, "COM3", "COM3"),
            (
                "FILE:///tmp/x.raw",
                LinkKind::Serial,
                "FILE:///tmp/x.raw",
                "x.raw",
            ),
            ("TCP://host:1", LinkKind::Serial, "TCP://host:1", "host:1"),
            (
                "tcp://192.168.1.50:4000",
                LinkKind::Tcp,
                "tcp://192.168.1.50:4000",
                "192.168.1.50:4000",
            ),
            (
                "tcp://localhost:23",
                LinkKind::Tcp,
                "tcp://localhost:23",
                "localhost:23",
            ),
            (
                "file:///tmp/logs/x.raw",
                LinkKind::Replay,
                "file://x.raw",
                "x.raw",
            ),
            (
                "file://C:\\logs\\x.raw",
                LinkKind::Replay,
                "file://x.raw",
                "x.raw",
            ),
        ];
        for (text, kind, label, tag) in table {
            let address = Address::parse(text);
            assert_eq!(address.kind(), kind, "{text}");
            assert_eq!(address.to_string(), text);
            assert_eq!(address.label(), label, "{text}");
            assert_eq!(address.tag(), tag, "{text}");
        }
        assert_eq!(
            Address::parse("tcp://192.168.1.50:4000"),
            Address::Tcp("192.168.1.50:4000".to_string())
        );
        assert_eq!(
            Address::parse("file:///tmp/x.raw"),
            Address::Replay(PathBuf::from("/tmp/x.raw"))
        );
    }
}
