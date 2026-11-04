use super::*;

/// MAX-12: the framing's bits per character, what a paced `send-file` divides the baud
/// rate by
#[test]
fn bits_per_char() {
    let framing = |char_size, parity, stop_bits| Framing {
        char_size,
        parity,
        stop_bits,
    };
    let cases = [
        (framing(CharSize::Bits8, Parity::None, StopBits::One), 10),
        (framing(CharSize::Bits7, Parity::Even, StopBits::One), 10),
        (framing(CharSize::Bits8, Parity::Odd, StopBits::Two), 12),
        (framing(CharSize::Bits5, Parity::None, StopBits::One), 7),
    ];
    for (framing, bits) in cases {
        assert_eq!(framing.bits_per_char(), bits, "{framing}");
    }
    let line = LineSettings {
        baud: 9600,
        framing: framing(CharSize::Bits8, Parity::Even, StopBits::One),
        ..LineSettings::default()
    };
    assert!((line.bytes_per_sec() - 9600.0 / 11.0).abs() < 1e-9);
}

/// MAX-12: `framing 8N1` and the like, any case, written back in capitals
#[test]
fn framings() {
    let parsed = |text: &str| text.parse::<Framing>();
    assert_eq!(
        parsed("8N1"),
        Ok(Framing {
            char_size: CharSize::Bits8,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
    );
    assert_eq!(parsed("7e2").unwrap().to_string(), "7E2");
    for bad in ["", "8N", "8N1 ", "9N1", "8X1", "8N3", "8é1"] {
        assert!(parsed(bad).is_err(), "{bad}");
    }
}

/// MAX-186: an open port holds a `flock`, seen in `/proc/locks` under maxicom's PID and
/// gone once closed; a port another program holds a `flock` on is refused as busy, and
/// left without TIOCEXCL, so that program can still open it again.
/// How: a pty opened as the port then closed; then its other end locked by the test
/// first, maxicom's open refused, a plain open after it.
/// The close is waited for: a process another test starts (`sleep`) holds a copy of every
/// fd until it execs, this port's and its lock with it (MAX-199, was flaky).
#[cfg(target_os = "linux")]
#[test]
fn open_takes_a_flock() {
    use crate::link::{lock::lock_holder, probe::port_state_of};
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    use std::time::Instant;
    let pty = crate::app::tests::Pty::open();
    let me = std::process::id() as libc::pid_t;
    let line = LineSettings::default();
    let port = SerialDevice::open(&pty.path, &line).unwrap();
    assert_eq!(lock_holder(&pty.path, "/proc/locks"), Some(me));
    drop(port);
    let start = Instant::now();
    while lock_holder(&pty.path, "/proc/locks").is_some() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "flock kept after the close"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&pty.path)
    };
    let other = open().unwrap();
    assert_eq!(unsafe { libc::flock(other.as_raw_fd(), libc::LOCK_EX) }, 0);
    let Err(LinkError::OpenFailed { source, .. }) = SerialDevice::open(&pty.path, &line) else {
        panic!("opened a locked port");
    };
    assert_eq!(
        port_state_of(&source),
        crate::link::probe::ProbeState::Busy(None)
    );
    open().expect("TIOCEXCL left on the port");
}

/// MAX-186: a port another process of the user's has open without any lock (`minicom -o`,
/// screen, cu) is refused before it's opened, that process named in the error; a pty is
/// opened though its maker holds it too (socat, as the test scripts make one).
/// How: a `sleep` started with a plain file as its input (refused before any open); then
/// with a pty's port end.
#[cfg(target_os = "linux")]
#[test]
fn open_refuses_a_port_held_without_a_lock() {
    let held_by_sleep = |path: &str| {
        let input = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        std::process::Command::new("sleep")
            .arg("10")
            .stdin(input)
            .spawn()
            .unwrap()
    };
    let line = LineSettings::default();
    let dir = std::env::temp_dir().join(format!("maxicom_held_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let port = dir.join("ttyHELD");
    std::fs::write(&port, "").unwrap();
    let port = port.to_str().unwrap();
    let mut holder = held_by_sleep(port);
    let refused = SerialDevice::open(port, &line);
    let _ = holder.kill();
    let _ = holder.wait();
    let Err(err) = refused else {
        panic!("opened a port another process holds");
    };
    let held = format!("is held by sleep ({})", holder.id());
    assert!(err.to_string().ends_with(&held), "{err}");
    let _ = std::fs::remove_dir_all(&dir);

    let pty = crate::app::tests::Pty::open();
    let mut maker = held_by_sleep(&pty.path);
    let opened = SerialDevice::open(&pty.path, &line);
    let _ = maker.kill();
    let _ = maker.wait();
    assert!(opened.is_ok(), "a pty refused for its maker");
}

/// MAX-188: a port's maker, product and `vid:pid` from the USB device above its sysfs
/// `device` link, two folders up for a ttyUSB, one for a ttyACM; the maker said once
/// when the product names it; none for a port with no USB device.
/// How: a fake sysfs tree of each kind, links as the kernel makes them.
#[cfg(target_os = "linux")]
#[test]
fn sysfs_port_details() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!("maxicom_sysfs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (root, devices) = (dir.join("class/tty"), dir.join("devices"));
    let usb = |path: &str, files: &[(&str, &str)]| {
        let usb = devices.join(path);
        std::fs::create_dir_all(&usb).unwrap();
        for (file, text) in files {
            std::fs::write(usb.join(file), format!("{text}\n")).unwrap();
        }
        usb
    };
    let tty = |name: &str, device: &std::path::Path| {
        std::fs::create_dir_all(root.join(name)).unwrap();
        symlink(device, root.join(name).join("device")).unwrap();
    };
    let cp2102 = usb(
        "usb1/1-1",
        &[
            ("manufacturer", "Silicon Labs"),
            ("product", "CP2102 USB to UART Bridge Controller"),
            ("idVendor", "10c4"),
            ("idProduct", "ea60"),
        ],
    );
    let port = cp2102.join("1-1:1.0/ttyUSB0");
    std::fs::create_dir_all(&port).unwrap();
    tty("ttyUSB0", &port);
    let acm = usb(
        "usb1/1-2",
        &[
            ("manufacturer", "Espressif"),
            ("product", "Espressif USB JTAG/serial debug unit"),
            ("idVendor", "303a"),
            ("idProduct", "1001"),
        ],
    );
    let interface = acm.join("1-2:1.0");
    std::fs::create_dir_all(&interface).unwrap();
    tty("ttyACM0", &interface);
    let serial8250 = devices.join("platform/serial8250/tty/ttyS0");
    std::fs::create_dir_all(&serial8250).unwrap();
    tty("ttyS0", &serial8250);

    assert_eq!(
        sysfs_details(&root, "/dev/ttyUSB0"),
        Some(PortDetails {
            device: Some("Silicon Labs CP2102 USB to UART Bridge Controller".into()),
            id: Some("10c4:ea60".into()),
        })
    );
    assert_eq!(
        sysfs_details(&root, "/dev/ttyACM0"),
        Some(PortDetails {
            device: Some("Espressif USB JTAG/serial debug unit".into()),
            id: Some("303a:1001".into()),
        })
    );
    assert_eq!(sysfs_details(&root, "/dev/ttyS0"), None);
    assert_eq!(sysfs_details(&root, "/dev/ttyUSB7"), None);
    let _ = std::fs::remove_dir_all(&dir);
}
