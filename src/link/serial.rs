/// HW I/O & Serial configuration
/// Using the 'serial2' crate
use std::{
    fmt::{self, Display, Write},
    io::ErrorKind,
    str::FromStr,
    time::Duration,
};

use super::LinkError;
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
}

/// What the directory's test found for a port (MAX-181)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortState {
    Free,
    /// Held by another program, named when it's known: `minicom (1234)`
    Busy(Option<String>),
    NoAccess,
    Error(String),
}

// TODO: redo the whole lock system by adding maxicom's (new module probably)

/// The lock folders of minicom, picocom and the other UUCP-style programs
#[cfg(unix)]
const LOCK_DIRS: &[&str] = &["/var/lock", "/run/lock", "/var/spool/lock"];

/// Whether `port` can be used (MAX-181), without opening it where the system tells otherwise:
/// an open raises DTR and RTS, which resets a board with an auto-reset (ESP32, Arduino). On
/// Linux a live UUCP lock file (minicom's), then a `flock` / `fcntl` lock in `/proc/locks`
/// (picocom's, tio's, pyserial's): both seen whoever the user; then a process of the user's
/// own holding it (its open files in `/proc`), then the permissions; on macOS the lock file and the permissions, a port held without a lock
/// passing as free. On Windows only an open tells: one with the port's settings kept, closed
/// at once
pub fn probe_port(port: &str) -> PortState {
    #[cfg(unix)]
    {
        if let Some(pid) = locked_by(port, LOCK_DIRS) {
            return PortState::Busy(Some(process_label(pid)));
        }
        #[cfg(target_os = "linux")]
        if let Some(pid) = lock_holder(port, "/proc/locks") {
            return PortState::Busy(Some(process_label(pid)));
        }
        #[cfg(target_os = "linux")]
        if let Some(holder) = holder_of(port, "/proc", std::process::id()) {
            return PortState::Busy(Some(holder));
        }
        let Ok(path) = std::ffi::CString::new(port) else {
            return PortState::Error("not a path".to_string());
        };
        if unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK) } == 0 {
            PortState::Free
        } else {
            port_state_of(&std::io::Error::last_os_error())
        }
    }
    #[cfg(not(unix))]
    match SerialPort::open(port, serial2::KeepSettings) {
        Ok(_) => PortState::Free,
        Err(err) => port_state_of(&err),
    }
}

/// An open's or an access check's error: Windows refuses a port in use with access denied, a
/// Unix port held with `TIOCEXCL` answers busy
pub fn port_state_of(err: &std::io::Error) -> PortState {
    #[cfg(unix)]
    if err.raw_os_error() == Some(libc::EBUSY) {
        return PortState::Busy(None);
    }
    match err.kind() {
        ErrorKind::PermissionDenied if cfg!(windows) => PortState::Busy(None),
        ErrorKind::PermissionDenied => PortState::NoAccess,
        _ => PortState::Error(err.to_string()),
    }
}

/// The running process of a UUCP lock file for `port`: `LCK..ttyUSB0` in one of the lock
/// folders `dirs`, the PID in it written as text
#[cfg(unix)]
fn locked_by(port: &str, dirs: &[&str]) -> Option<libc::pid_t> {
    let name = std::path::Path::new(port).file_name()?;
    let lock = format!("LCK..{}", name.to_string_lossy());
    let pid_in = |dir: &&str| {
        let text = std::fs::read_to_string(std::path::Path::new(dir).join(&lock)).ok()?;
        text.trim()
            .parse::<libc::pid_t>()
            .ok()
            .filter(|&pid| pid > 0)
    };
    dirs.iter().filter_map(pid_in).find(|&pid| {
        // signal 0 only asks whether the process exists; EPERM: it does, someone else's
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    })
}

/// The process with a `flock` or `fcntl` lock on `port` in `locks` (`/proc/locks`, which every
/// user can read): its lines name the file by its filesystem's `major:minor` (hex) and inode
#[cfg(target_os = "linux")]
fn lock_holder(port: &str, locks: &str) -> Option<libc::pid_t> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(port).ok()?;
    let dev = meta.dev();
    let file = format!(
        "{:02x}:{:02x}:{}",
        libc::major(dev),
        libc::minor(dev),
        meta.ino()
    );
    let text = std::fs::read_to_string(locks).ok()?;
    text.lines().find_map(|line| {
        // `1: FLOCK  ADVISORY  WRITE 1234 00:05:567 0 EOF`; a waiter's line has `->` second
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [_, kind, _, _, pid, id, ..] if *kind != "->" && *id == file => pid.parse().ok(),
            _ => None,
        }
    })
}

/// The first process but `skip` with `port` open, read from `proc`'s `<pid>/fd` links: the
/// ones of the user's processes, the others' can't be read
#[cfg(target_os = "linux")]
fn holder_of(port: &str, proc: &str, skip: u32) -> Option<String> {
    let target = std::fs::canonicalize(port).ok()?;
    let pids = std::fs::read_dir(proc).ok()?.flatten().filter_map(|entry| {
        let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
        (pid != skip).then_some((pid, entry.path()))
    });
    for (pid, dir) in pids {
        let Ok(fds) = std::fs::read_dir(dir.join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|link| link == target));
        if holds {
            return Some(process_label(pid as libc::pid_t));
        }
    }
    None
}

/// A process as the directory names it: `minicom (1234)`, the PID alone where its name can't
/// be read
#[cfg(unix)]
fn process_label(pid: libc::pid_t) -> String {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok();
    match name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => format!("{name} ({pid})"),
        _ => format!("pid {pid}"),
    }
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

impl SerialDevice {
    /// Opens `device` with the line settings `line`
    pub fn open(device: &str, line: &LineSettings) -> Result<Self, LinkError> {
        let mut port =
            SerialPort::open(device, line.baud).map_err(|err| LinkError::OpenFailed {
                port: device.to_string(),
                baudrate: line.baud,
                source: err,
            })?;

        // Platform-specific exclusivity lock
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = port.as_raw_fd();

            // TIOCEXCL tells the kernel to return EBUSY for any following open()
            let res = unsafe { libc::ioctl(fd, libc::TIOCEXCL) };
            if res < 0 {
                let err = std::io::Error::last_os_error();
                return Err(LinkError::OpenFailed {
                    port: device.to_string(),
                    baudrate: line.baud,
                    source: err,
                });
            }
        }

        // short: the main loop idles in the input poll, a read only waits for late bytes
        port.set_read_timeout(Duration::from_millis(1))
            .map_err(LinkError::ConfigFailed)?;

        let mut dev = Self { port };
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

    /// MAX-181: an open's or an access check's error says how the port is held: EBUSY (a
    /// `TIOCEXCL` holder) is busy, access denied is busy on Windows and no access elsewhere,
    /// anything else its text; a port that doesn't exist is an error.
    #[test]
    fn port_states_of_open_errors() {
        use std::io::Error;
        #[cfg(unix)]
        assert_eq!(
            port_state_of(&Error::from_raw_os_error(libc::EBUSY)),
            PortState::Busy(None)
        );
        let denied = port_state_of(&Error::from(ErrorKind::PermissionDenied));
        let expected = if cfg!(windows) {
            PortState::Busy(None)
        } else {
            PortState::NoAccess
        };
        assert_eq!(denied, expected);
        let missing = port_state_of(&Error::new(ErrorKind::NotFound, "no such device"));
        assert_eq!(missing, PortState::Error("no such device".to_string()));
        assert!(matches!(
            probe_port("/nonexistent/ttyX"),
            PortState::Error(_)
        ));
    }

    /// MAX-181: a lock file names the port by its file name and holds a PID: the test's own
    /// process locks it, a PID of no process or text that isn't one doesn't, nor a lock for
    /// another port.
    /// How: a temp lock folder, `LCK..ttyUSB7` written three ways.
    #[cfg(unix)]
    #[test]
    fn lock_files() {
        let dir = std::env::temp_dir().join(format!("maxicom_locks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = [dir.to_str().unwrap()];
        let lock = dir.join("LCK..ttyUSB7");
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        std::fs::write(&lock, format!("{:>10}\n", std::process::id())).unwrap();
        let me = Some(std::process::id() as libc::pid_t);
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), me);
        assert_eq!(locked_by("/dev/ttyUSB8", &dirs), None);
        // past the largest PID Linux and macOS give
        std::fs::write(&lock, "  99999999\n").unwrap();
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        std::fs::write(&lock, "garbage").unwrap();
        assert_eq!(locked_by("/dev/ttyUSB7", &dirs), None);
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(probe_port(free.to_str().unwrap()), PortState::Free);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-181: a `flock` on a file shows in `/proc/locks` under the locking process, for
    /// every user; a file nobody locks isn't there.
    /// How: a temp file locked by the test, another not.
    #[cfg(target_os = "linux")]
    #[test]
    fn flock_holders() {
        use std::os::fd::AsRawFd;
        let dir = std::env::temp_dir().join(format!("maxicom_flock_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let locked = dir.join("ttyFAKE0");
        let file = std::fs::File::create(&locked).unwrap();
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0);
        let free = dir.join("ttyFAKE1");
        std::fs::write(&free, "").unwrap();

        let me = std::process::id() as libc::pid_t;
        assert_eq!(
            lock_holder(locked.to_str().unwrap(), "/proc/locks"),
            Some(me)
        );
        assert_eq!(lock_holder(free.to_str().unwrap(), "/proc/locks"), None);
        drop(file);
        assert_eq!(lock_holder(locked.to_str().unwrap(), "/proc/locks"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
