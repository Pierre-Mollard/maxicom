use std::{
    env::{self, args},
    io::ErrorKind,
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
        let port =
            SerialPort::open(config.port, config.baudrate).map_err(|_| SerialError::OpenFailed)?;

        Ok(Self { port })
    }

    pub fn read_into(&self, buffer: &mut [u8]) -> Result<usize, SerialError> {
        loop {
            match self.port.read(buffer) {
                Ok(0) => return Err(SerialError::InputClosed),

                Ok(bytes_read) => return Ok(bytes_read),

                Err(error) if error.kind() == ErrorKind::Interrupted => {
                    continue;
                }

                Err(_) => return Err(SerialError::ReadFailed),
            }
        }
    }
}

pub fn parse_args() -> Result<SerialConfig, ConfigError> {
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
                return Err(ConfigError::UnknownArgument(
                    "Usage: maxicom [--port DEVICE] [--baud RATE]".to_string(),
                ));
            }

            unknown => {
                return Err(ConfigError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    Ok(config)
}
