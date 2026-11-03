
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
