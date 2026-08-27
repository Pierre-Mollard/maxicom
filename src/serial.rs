/// HW I/O & Serial configuration
/// Using the 'serial2' crate
use std::{
    env,
    error::Error,
    fmt,
    io::{self, ErrorKind},
    time::Duration,
};

use serial2::SerialPort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialConfig {
    pub port: String,
    pub baudrate: u32,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::from("/dev/ttyUSB0"),
            baudrate: 115_200,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    MissingValue(&'static str),
    InvalidBaudRate(String),
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

// 1. User-facing formatting (used by println!("{}", err))
impl fmt::Display for SerialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenFailed {
                port,
                baudrate,
                source,
            } => {
                write!(
                    f,
                    "failed to open serial port '{port}' at {baudrate} baud: {source}"
                )
            }
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

        port.set_read_timeout(Duration::from_millis(10))
            .map_err(SerialError::ConfigFailed)?;

        Ok(Self { port })
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
                let value = args.next().ok_or(ConfigError::MissingValue("--baud"))?;

                config.baudrate = value
                    .parse::<u32>()
                    .map_err(|_| ConfigError::InvalidBaudRate(value))?;
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

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingValue(option) => {
                write!(formatter, "Missing value for option '{option}'.")
            }

            ConfigError::UnknownArgument(argument) => {
                write!(formatter, "Unknown argument '{argument}'.")
            }

            ConfigError::InvalidBaudRate(value) => {
                write!(
                    formatter,
                    "Invalid baud rate '{value}': expected an unsigned 32-bit integer."
                )
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
  -p, --port <DEVICE>  Serial device path
  -b, --baud <RATE>    Baud rate [default: 115200]
  -h, --help           Print help
"
    );
}
