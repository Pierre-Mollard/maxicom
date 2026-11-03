/// HW I/O & Serial configuration
/// Using the 'serial2' crate
use std::{
    fmt::{self, Display, Write},
    io::ErrorKind,
    str::FromStr,
    time::Duration,
};

#[cfg(target_os = "linux")]
use super::probe::open_holder;
use super::{
    Address, LinkError,
    lock::{LinkLock, LockState, held_by, lock_state},
    probe::maxicom_label,
};
#[cfg(unix)]
use super::{
    lock::{LOCK_DIRS, UucpLock, locked_by},
    probe::process_label,
};
use serde::{Deserialize, Serialize};
use serial2::{
    CharSize as S2CharSize, FlowControl as S2FlowControl, Parity as S2Parity, SerialPort,
    StopBits as S2StopBits,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CharSize {
    Bits5,
    Bits6,
    Bits7,
    #[default]
    Bits8,
}

impl FromStr for CharSize {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "5" => Ok(Self::Bits5),
            "6" => Ok(Self::Bits6),
            "7" => Ok(Self::Bits7),
            "8" => Ok(Self::Bits8),
            _ => Err(format!("invalid char size '{s}': expected 5, 6, 7, or 8")),
        }
    }
}

impl From<CharSize> for S2CharSize {
    fn from(c: CharSize) -> Self {
        match c {
            CharSize::Bits5 => S2CharSize::Bits5,
            CharSize::Bits6 => S2CharSize::Bits6,
            CharSize::Bits7 => S2CharSize::Bits7,
            CharSize::Bits8 => S2CharSize::Bits8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StopBits {
    #[default]
    One,
    Two,
}

impl FromStr for StopBits {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1" => Ok(Self::One),
            "2" => Ok(Self::Two),
            _ => Err(format!("invalid stop bits '{s}': expected 1 or 2")),
        }
    }
}

impl From<StopBits> for S2StopBits {
    fn from(s: StopBits) -> Self {
        match s {
            StopBits::One => S2StopBits::One,
            StopBits::Two => S2StopBits::Two,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
}

impl FromStr for Parity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "none" | "n" => Ok(Self::None),
            "even" | "e" => Ok(Self::Even),
            "odd" | "o" => Ok(Self::Odd),
            _ => Err(format!(
                "invalid parity '{s}': expected none (n), even (e), or odd (o)"
            )),
        }
    }
}

impl From<Parity> for S2Parity {
    fn from(p: Parity) -> Self {
        match p {
            Parity::None => S2Parity::None,
            Parity::Even => S2Parity::Even,
            Parity::Odd => S2Parity::Odd,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowControl {
    #[default]
    None,
    RtsCts,  // Hardware flow control
    XonXoff, // Software flow control
}

impl FromStr for FlowControl {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "rtscts" | "hardware" | "hw" => Ok(Self::RtsCts),
            "xonxoff" | "software" | "sw" => Ok(Self::XonXoff),
            _ => Err(format!(
                "invalid flow control '{s}': expected none, rtscts, or xonxoff"
            )),
        }
    }
}

impl From<FlowControl> for S2FlowControl {
    fn from(f: FlowControl) -> Self {
        match f {
            FlowControl::None => S2FlowControl::None,
            FlowControl::RtsCts => S2FlowControl::RtsCts,
            FlowControl::XonXoff => S2FlowControl::XonXoff,
        }
    }
}

/// The bits of one character on the line, written `8N1`: the data bits (5 to 8), the parity
/// (`N`, `E`, `O`) and the stop bits (1 or 2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Framing {
    pub char_size: CharSize,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl Framing {
    /// The bits on the wire for one character: the start bit, the data bits, the parity bit if
    /// any and the stop bits; 10 for 8N1
    pub fn bits_per_char(&self) -> u32 {
        let data = match self.char_size {
            CharSize::Bits5 => 5,
            CharSize::Bits6 => 6,
            CharSize::Bits7 => 7,
            CharSize::Bits8 => 8,
        };
        let parity = match self.parity {
            Parity::None => 0,
            Parity::Odd | Parity::Even => 1,
        };
        let stop = match self.stop_bits {
            StopBits::One => 1,
            StopBits::Two => 2,
        };
        1 + data + parity + stop
    }

    pub fn compact_notation(&self) -> [char; 3] {
        let d = match self.char_size {
            CharSize::Bits5 => '5',
            CharSize::Bits6 => '6',
            CharSize::Bits7 => '7',
            CharSize::Bits8 => '8',
        };

        let p = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };

        let s = match self.stop_bits {
            StopBits::One => '1',
            StopBits::Two => '2',
        };

        [d, p, s]
    }
}

impl Display for Framing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let compact_bytes = self.compact_notation();
        f.write_char(compact_bytes[0])?;
        f.write_char(compact_bytes[1])?;
        f.write_char(compact_bytes[2])
    }
}

/// `8N1` and the like, in any case
impl FromStr for Framing {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid =
            || format!("'{text}' isn't a framing like 8N1 (5-8 data bits, N/E/O, 1-2 stop)");
        let [data, parity, stop] = text.as_bytes() else {
            return Err(invalid());
        };
        let part = |byte: &u8| (*byte as char).to_string();
        let char_size = part(data).parse().map_err(|_| invalid())?;
        let parity = part(parity).parse().map_err(|_| invalid())?;
        let stop_bits = match stop {
            b'1' => StopBits::One,
            b'2' => StopBits::Two,
            _ => return Err(invalid()),
        };
        Ok(Self {
            char_size,
            parity,
            stop_bits,
        })
    }
}

/// What maxicom sets on a serial port: its baud rate, framing and flow control (MAX-168)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSettings {
    pub baud: u32,
    pub framing: Framing,
    pub flow_control: FlowControl,
}

impl LineSettings {
    /// The characters the line carries a second at its baud rate and framing
    pub fn bytes_per_sec(&self) -> f64 {
        f64::from(self.baud) / f64::from(self.framing.bits_per_char())
    }
}

/// 115200 8N1, no flow control
impl Default for LineSettings {
    fn default() -> Self {
        Self {
            baud: 115_200,
            framing: Framing::default(),
            flow_control: FlowControl::None,
        }
    }
}

/// A control line of a serial port, set by hand (MAX-12): DTR, RTS, or a break held this long
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineControl {
    Dtr(bool),
    Rts(bool),
    Break(Duration),
}

impl Display for LineControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on_off = |on| if on { "on" } else { "off" };
        match self {
            Self::Dtr(on) => write!(f, "DTR {}", on_off(*on)),
            Self::Rts(on) => write!(f, "RTS {}", on_off(*on)),
            Self::Break(duration) => write!(f, "break of {} ms", duration.as_millis()),
        }
    }
}

pub struct SerialDevice {
    port: SerialPort,
    // maxicom's locks on the port while open, when they could be taken (MAX-186)
    _lock: Option<LinkLock>,
    #[cfg(unix)]
    _uucp: Option<UucpLock>,
}

/// Who holds `device`, this process aside: another maxicom's lock in the shared folder, a UUCP
/// lock file on Unix (minicom's), and on Linux a process of the user's own with a real port
/// open, with no lock (screen, cu, `minicom -o`), a pty's maker aside (MAX-186). A `flock` holder is found by the open. Another user's
/// program holding it without a lock can't be seen
fn lock_holder(device: &str) -> Option<String> {
    let me = std::process::id();
    match lock_state(&Address::Serial(device.to_string())) {
        LockState::Held(info) if info.as_ref().is_none_or(|info| info.pid != me) => {
            return Some(maxicom_label(info.as_ref()));
        }
        _ => {}
    }
    #[cfg(unix)]
    if let Some(pid) = locked_by(device, LOCK_DIRS).filter(|&pid| pid as u32 != me) {
        return Some(process_label(pid));
    }
    #[cfg(target_os = "linux")]
    if let Some(holder) = open_holder(device, me) {
        return Some(holder);
    }
    None
}

/// The serial ports the system lists, sorted, to complete `open`'s address (MAX-168); none
/// where it can't list them, or for a name that isn't UTF-8
pub fn port_names() -> Vec<String> {
    let ports = SerialPort::available_ports().unwrap_or_default();
    let mut names: Vec<String> = (ports.into_iter())
        .filter_map(|port| port.into_os_string().into_string().ok())
        .collect();
    names.sort_unstable();
    names
}

/// What the system tells of a port besides its name (MAX-188): its USB maker and product,
/// its `vid:pid`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortDetails {
    pub device: Option<String>,
    pub id: Option<String>,
}

/// The details of each of `ports` that has some, by name: Linux reads them in sysfs, the other
/// systems have none. Tests read none, so the boards plugged in don't change what they see
pub fn port_details(ports: &[String]) -> std::collections::HashMap<String, PortDetails> {
    #[cfg(target_os = "linux")]
    if !cfg!(test) {
        let root = std::path::Path::new("/sys/class/tty");
        return (ports.iter())
            .filter_map(|port| Some((port.clone(), sysfs_details(root, port)?)))
            .collect();
    }
    let _ = ports;
    std::collections::HashMap::new()
}

/// A port's details under `root` (`/sys/class/tty`): its `device` link resolved, then the
/// first folder above with an `idVendor`, the USB device (two up for a ttyUSB, one for a
/// ttyACM); none for a port with no USB device (ttyS, a pty)
#[cfg(target_os = "linux")]
fn sysfs_details(root: &std::path::Path, port: &str) -> Option<PortDetails> {
    let name = std::path::Path::new(port).file_name()?;
    let device = std::fs::canonicalize(root.join(name).join("device")).ok()?;
    let usb = device
        .ancestors()
        .take(4)
        .find(|dir| dir.join("idVendor").is_file())?;
    let read = |file: &str| {
        let text = std::fs::read_to_string(usb.join(file)).ok()?;
        Some(text.trim().to_string()).filter(|text| !text.is_empty())
    };
    let product = read("product");
    // the product often starts with its maker's name already
    let device = match (read("manufacturer"), product) {
        (Some(maker), Some(product)) if product.starts_with(&maker) => Some(product),
        (Some(maker), Some(product)) => Some(format!("{maker} {product}")),
        (maker, product) => maker.or(product),
    };
    let id = read("idVendor").map(|vid| match read("idProduct") {
        Some(pid) => format!("{vid}:{pid}"),
        None => vid,
    });
    Some(PortDetails { device, id })
}

impl SerialDevice {
    /// Opens `device` with the line settings `line`
    pub fn open(device: &str, line: &LineSettings) -> Result<Self, LinkError> {
        let failed = |source| LinkError::OpenFailed {
            port: device.to_string(),
            baudrate: line.baud,
            source,
        };
        // a port another program holds by a lock is refused before the open, which would
        // raise its lines and may reset the board (MAX-186)
        if let Some(holder) = lock_holder(device) {
            return Err(failed(held_by(holder)));
        }
        let mut port = SerialPort::open(device, line.baud).map_err(failed)?;

        // Platform-specific exclusivity lock
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = port.as_raw_fd();

            // a `flock` too, the one picocom, tio and pyserial take (MAX-186): every user sees
            // it in `/proc/locks`, those programs keep off the port, and it goes with the fd,
            // a crash included. Taken first: a port refused here is left without TIOCEXCL. A
            // file system without locks only loses that guard
            if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } < 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
            {
                return Err(failed(std::io::Error::from_raw_os_error(libc::EBUSY)));
            }

            // TIOCEXCL tells the kernel to return EBUSY for any following open()
            let res = unsafe { libc::ioctl(fd, libc::TIOCEXCL) };
            if res < 0 {
                return Err(failed(std::io::Error::last_os_error()));
            }
        }

        // short: the main loop idles in the input poll, a read only waits for late bytes
        port.set_read_timeout(Duration::from_millis(1))
            .map_err(LinkError::ConfigFailed)?;

        let mut dev = Self {
            port,
            _lock: LinkLock::take(&Address::Serial(device.to_string())),
            #[cfg(unix)]
            _uucp: UucpLock::take(device),
        };
        dev.reconfigure(line)?;
        Ok(dev)
    }

    pub fn reconfigure(&mut self, line: &LineSettings) -> Result<(), LinkError> {
        let summary = format!(
            "{} baud, {:?}, {:?} parity, {:?} stop, {:?} flow",
            line.baud,
            line.framing.char_size,
            line.framing.parity,
            line.framing.stop_bits,
            line.flow_control
        );
        let mut settings = self
            .port
            .get_configuration()
            .map_err(LinkError::ConfigFailed)?;

        settings.set_raw();
        settings
            .set_baud_rate(line.baud)
            .map_err(|e| LinkError::ReconfigureFailed {
                summary: summary.clone(),
                rejected_fields: vec![format!("Baud rate {}", line.baud)],
                source: e,
            })?;

        settings.set_char_size(line.framing.char_size.into());
        settings.set_stop_bits(line.framing.stop_bits.into());
        settings.set_parity(line.framing.parity.into());
        settings.set_flow_control(line.flow_control.into());

        if let Err(err) = self.port.set_configuration(&settings) {
            // Read back what the hardware actually kept to find the culprit!
            let mut rejected = Vec::new();

            if let Ok(actual) = self.port.get_configuration() {
                let expected_char_size: serial2::CharSize = line.framing.char_size.into();
                let expected_stop_bits: serial2::StopBits = line.framing.stop_bits.into();
                let expected_parity: serial2::Parity = line.framing.parity.into();
                let expected_flow: serial2::FlowControl = line.flow_control.into();

                if actual.get_char_size().ok() != Some(expected_char_size) {
                    rejected.push(format!("Data bits ({:?})", line.framing.char_size));
                }
                if actual.get_stop_bits().ok() != Some(expected_stop_bits) {
                    rejected.push(format!("Stop bits ({:?})", line.framing.stop_bits));
                }
                if actual.get_parity().ok() != Some(expected_parity) {
                    rejected.push(format!("Parity ({:?})", line.framing.parity));
                }
                if actual.get_flow_control().ok() != Some(expected_flow) {
                    rejected.push(format!("Flow control ({:?})", line.flow_control));
                }
            }

            if rejected.is_empty() {
                rejected.push("Unsupported configuration combination".to_string());
            }

            return Err(LinkError::ReconfigureFailed {
                summary,
                rejected_fields: rejected,
                source: err,
            });
        }

        Ok(())
    }

    pub fn read(&self, buffer: &mut [u8]) -> Result<usize, LinkError> {
        match self.port.read(buffer) {
            // 0 bytes on Unix TTY indicates EOF / disconnect
            Ok(0) => Err(LinkError::InputClosed),

            Ok(bytes_read) => Ok(bytes_read),

            // Recoverable conditions -> return 0 bytes read
            Err(e) => match e.kind() {
                ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted => Ok(0),
                _ => Err(LinkError::ReadFailed(e)),
            },
        }
    }

    /// Sets DTR or RTS, or starts a break: ending it is the caller's, `set_break(false)`
    pub fn control(&mut self, control: LineControl) -> Result<(), LinkError> {
        match control {
            LineControl::Dtr(on) => self.port.set_dtr(on),
            LineControl::Rts(on) => self.port.set_rts(on),
            LineControl::Break(_) => self.port.set_break(true),
        }
        .map_err(LinkError::ControlFailed)
    }

    pub fn end_break(&mut self) -> Result<(), LinkError> {
        self.port.set_break(false).map_err(LinkError::ControlFailed)
    }

    /// The line's settings as the port has them, for the tests on a pty
    #[cfg(test)]
    pub fn settings(&self) -> std::io::Result<serial2::Settings> {
        self.port.get_configuration()
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<(), LinkError> {
        self.port
            .write_all(buffer)
            .map_err(|err| LinkError::WriteFailed {
                bytes_attempted: buffer.len(),
                source: err,
            })
    }
}

#[cfg(unix)]
impl Drop for SerialDevice {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        let fd = self.port.as_raw_fd();
        // Clear exclusive mode
        unsafe {
            libc::ioctl(fd, libc::TIOCNXCL);
        }
    }
}

#[cfg(test)]
mod tests {
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
    #[cfg(target_os = "linux")]
    #[test]
    fn open_takes_a_flock() {
        use crate::link::{lock::lock_holder, probe::port_state_of};
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let pty = crate::app::tests::Pty::open();
        let me = std::process::id() as libc::pid_t;
        let line = LineSettings::default();
        let port = SerialDevice::open(&pty.path, &line).unwrap();
        assert_eq!(lock_holder(&pty.path, "/proc/locks"), Some(me));
        drop(port);
        assert_eq!(lock_holder(&pty.path, "/proc/locks"), None);

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
}
