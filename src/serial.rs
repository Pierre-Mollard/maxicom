use std::{env, io::ErrorKind, time::Duration};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    OpenFailed,
    InputClosed,
    ReadFailed,
}

// TODO: check and handle more error types
impl SerialDevice {
    pub fn open(config: SerialConfig) -> Result<Self, SerialError> {
        let mut port =
            SerialPort::open(config.port, config.baudrate).map_err(|_| SerialError::OpenFailed)?;

        port.set_read_timeout(Duration::from_millis(10))
            .map_err(|_| SerialError::OpenFailed)?;

        Ok(Self { port })
    }

    pub fn read_into(&self, buffer: &mut [u8]) -> Result<usize, SerialError> {
        match self.port.read(buffer) {
            Ok(0) => Err(SerialError::InputClosed),

            Ok(bytes_read) => Ok(bytes_read),

            Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => Ok(0),

            Err(e) if e.kind() == ErrorKind::Interrupted => Ok(0),

            Err(_) => Err(SerialError::ReadFailed),
        }
    }

    pub fn write_all(&self, buffer: &[u8]) {
        self.port.write_all(buffer);
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
