use std::env;

use crate::serial::{SerialConfig, SerialDevice};

pub fn start() {
    let mut args = env::args();
    let program_name = args.next(); // discard program_name

    let path: String = String::from("/dev/ttytest");
    let baudrate: u32 = 0;
    let stop_bit: u8 = 0;
    let parity: u8 = 0;

    for arg in args {
        println!("argument : {arg:?}");
    }

    let serial_conf: SerialConfig = SerialConfig { baudrate };
}
