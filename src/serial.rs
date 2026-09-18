/// HW I/O & Serial configuration
/// Using the 'serial2' crate
use std::{
    env,
    error::Error,
    fmt,
    io::{self, ErrorKind},
    str::FromStr,
    time::Duration,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialConfig {
    pub port: String,
    pub baudrate: u32,
    pub char_size: CharSize,
    pub stop_bits: StopBits,
    pub parity: Parity,
    pub flow_control: FlowControl,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::from("/dev/ttyUSB0"),
            baudrate: 115_200,
            char_size: CharSize::Bits8,
            stop_bits: StopBits::One,
            parity: Parity::None,
            flow_control: FlowControl::None,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    MissingValue(&'static str),
    ParseError(String),
    UnknownArgument(String),
}

#[derive(Debug)]
pub enum ParseOutcome {
    Config(SerialConfig),
    Help,
}

pub struct SerialDevice {
    port: SerialPort,
}

#[derive(Debug)]
pub enum SerialError {
    /// Failed to open port (missing device, permissions, etc.)
    OpenFailed {
        port: String,
        baudrate: u32,
        source: io::Error,
    },
    /// Failed to configure port options (read timeout, termios flags)
    ConfigFailed(io::Error),
    /// Port was closed on the other side or USB unplugged (EOF on read)
    InputClosed,
    /// Unexpected error while reading bytes
    ReadFailed(io::Error),
    /// Failed to transmit bytes
    WriteFailed {
        bytes_attempted: usize,
        source: io::Error,
    },
}

impl fmt::Display for SerialError {
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
            Self::ConfigFailed(source) => {
                write!(f, "failed to configure port settings: {source}")
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
        }
    }
}

// 2. Standard library error trait implementation (allows error chaining)
impl Error for SerialError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::OpenFailed { source, .. } => Some(source),
            Self::ConfigFailed(source) => Some(source),
            Self::ReadFailed(source) => Some(source),
            Self::WriteFailed { source, .. } => Some(source),
            Self::InputClosed => None,
        }
    }
}

impl SerialDevice {
    pub fn open(config: &SerialConfig) -> Result<Self, SerialError> {
        let mut port = SerialPort::open(&config.port, config.baudrate).map_err(|err| {
            SerialError::OpenFailed {
                port: config.port.clone(),
                baudrate: config.baudrate,
                source: err,
            }
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
                return Err(SerialError::OpenFailed {
                    port: config.port.clone(),
                    baudrate: config.baudrate,
                    source: err,
                });
            }
        }

        port.set_read_timeout(Duration::from_millis(10))
            .map_err(SerialError::ConfigFailed)?;

        let mut dev = Self { port };
        dev.reconfigure(config)?;
        Ok(dev)
    }

    // TODO: make it accessible for user
    pub fn reconfigure(&mut self, config: &SerialConfig) -> Result<(), SerialError> {
        let mut settings = self
            .port
            .get_configuration()
            .map_err(SerialError::ConfigFailed)?;
        settings.set_raw();
        settings
            .set_baud_rate(config.baudrate)
            .map_err(SerialError::ConfigFailed)?;
        settings.set_char_size(config.char_size.into());
        settings.set_stop_bits(config.stop_bits.into());
        settings.set_parity(config.parity.into());
        settings.set_flow_control(config.flow_control.into());

        self.port
            .set_configuration(&settings)
            .map_err(SerialError::ConfigFailed)
    }

    pub fn read(&self, buffer: &mut [u8]) -> Result<usize, SerialError> {
        match self.port.read(buffer) {
            // 0 bytes on Unix TTY indicates EOF / disconnect
            Ok(0) => Err(SerialError::InputClosed),

            Ok(bytes_read) => Ok(bytes_read),

            // Recoverable conditions -> return 0 bytes read
            Err(e) => match e.kind() {
                ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted => Ok(0),
                _ => Err(SerialError::ReadFailed(e)),
            },
        }
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<(), SerialError> {
        self.port
            .write_all(buffer)
            .map_err(|err| SerialError::WriteFailed {
                bytes_attempted: buffer.len(),
                source: err,
            })
    }
}

pub fn parse_args() -> Result<ParseOutcome, ConfigError> {
    let mut config = SerialConfig::default();
    let mut args = env::args().skip(1); // skip program name

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                config.port = args.next().ok_or(ConfigError::MissingValue("--port"))?;
            }
            "--baud" | "-b" => {
                let val = args.next().ok_or(ConfigError::MissingValue("--baud"))?;
                config.baudrate = val
                    .parse::<u32>()
                    .map_err(|_| ConfigError::ParseError(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                let val = args.next().ok_or(ConfigError::MissingValue("--bits"))?;
                config.char_size = val.parse::<CharSize>().map_err(ConfigError::ParseError)?;
            }
            "--stop-bits" => {
                let val = args
                    .next()
                    .ok_or(ConfigError::MissingValue("--stop-bits"))?;
                config.stop_bits = val.parse::<StopBits>().map_err(ConfigError::ParseError)?;
            }
            "--parity" => {
                let val = args.next().ok_or(ConfigError::MissingValue("--parity"))?;
                config.parity = val.parse::<Parity>().map_err(ConfigError::ParseError)?;
            }
            "--flow" => {
                let val = args.next().ok_or(ConfigError::MissingValue("--flow"))?;
                config.flow_control = val
                    .parse::<FlowControl>()
                    .map_err(ConfigError::ParseError)?;
            }
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }

            unknown => {
                return Err(ConfigError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    Ok(ParseOutcome::Config(config))
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

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingValue(option) => {
                write!(formatter, "Missing value for option '{option}'.")
            }

            ConfigError::UnknownArgument(argument) => {
                write!(formatter, "Unknown argument '{argument}'.")
            }

            ConfigError::ParseError(msg) => {
                write!(formatter, "{msg}")
            }
        }
    }
}

pub fn print_help() {
    println!(
        "\
MaxiCom — serial terminal

Usage:
  maxicom [OPTIONS]

Options:
  -p, --port <DEVICE>      Serial device path [default: /dev/ttyUSB0]
  -b, --baud <RATE>        Baud rate [default: 115200]
      --bits <5|6|7|8>     Character data bits [default: 8]
      --stop-bits <1|2>    Stop bits [default: 1]
      --parity <n|e|o>     Parity: none, even, odd [default: none]
      --flow <none|hw|sw>  Flow control: none, rtscts (hw), xonxoff (sw) [default: none]
  -h, --help               Print help
"
    );
}
