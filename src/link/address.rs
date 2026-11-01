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
            let address: Address = text.parse().unwrap();
            assert_eq!(address.kind(), kind, "{text}");
            assert_eq!(address.to_string(), text);
            assert_eq!(address.label(), label, "{text}");
            assert_eq!(address.tag(), tag, "{text}");
        }
        assert_eq!(
            "tcp://192.168.1.50:4000".parse(),
            Ok(Address::Tcp("192.168.1.50:4000".to_string()))
        );
        assert_eq!(
            "file:///tmp/x.raw".parse(),
            Ok(Address::Replay(PathBuf::from("/tmp/x.raw")))
        );
    }

    /// An address that could never open is refused when it is read: a TCP address that isn't
    /// `host:port` (checked without a name lookup), an empty device, an empty file path.
    /// How: good and bad TCP addresses, then the empty forms.
    #[test]
    fn bad_addresses_refused() {
        for good in [
            "192.168.1.50:4000",
            "localhost:23",
            "esp32.local:4000",
            "[::1]:4000",
        ] {
            assert!(format!("tcp://{good}").parse::<Address>().is_ok(), "{good}");
        }
        for bad in [
            "192.168.1.50",
            ":4000",
            "host:",
            "host:notaport",
            "host:70000",
            "",
        ] {
            assert_eq!(
                format!("tcp://{bad}").parse::<Address>(),
                Err(format!("'{bad}' is not host:port"))
            );
        }
        assert!("".parse::<Address>().is_err());
        assert!("file://".parse::<Address>().is_err());
    }

    /// A meta file's bad address fails its read instead of giving a link that never opens.
    /// How: a struct holding an address, read from TOML.
    #[test]
    fn bad_address_fails_deserialize() {
        #[derive(Deserialize)]
        struct Holder {
            #[allow(dead_code)]
            port: Address,
        }
        assert!(toml::from_str::<Holder>("port = \"tcp://host:1\"").is_ok());
        assert!(toml::from_str::<Holder>("port = \"tcp://host\"").is_err());
    }
}
