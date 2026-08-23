use std::io::ErrorKind;

use serial2::SerialPort;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialConfig {
    pub baudrate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    OpenFailed,
    InputClosed,
    ReadFailed,
}

pub struct SerialDevice {
    port: SerialPort,
}

// TODO: check and handle more error types
impl SerialDevice {
    pub fn open(device_name: &str, config: SerialConfig) -> Result<Self, SerialError> {
        let port =
            SerialPort::open(device_name, config.baudrate).map_err(|_| SerialError::OpenFailed)?;

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
