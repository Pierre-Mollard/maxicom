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
}
