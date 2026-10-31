//! What the link is (MAX-168): its address, and the line settings a serial port opens with,
//! which a TCP link or a replay keeps for the port it goes back to
use super::{Address, serial::LineSettings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkConfig {
    pub address: Address,
    pub line: LineSettings,
}

/// `/dev/ttyUSB0` at 115200 8N1
impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            address: Address::Serial(String::from("/dev/ttyUSB0")),
            line: LineSettings::default(),
        }
    }
}
