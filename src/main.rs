use std::io::Write;
use std::io::stdout;

use crate::serial::SerialConfig;
use crate::serial::SerialDevice;
use crate::serial::SerialError;

mod app;
mod commands;
mod serial;

fn main() {
    println!("=== maxicom ===");

    let config = SerialConfig { baudrate: 115_200 };

    let device = match SerialDevice::open("/tmp/maxicom", config) {
        Ok(device) => device,
        Err(error) => {
            eprintln!("Could not open serial device: {error:?}");
            return;
        }
    };

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 1024];

    // TODO: check read vectors directly
    println!("> starting receiving:");
    while running {
        let bytes_read = match device.read_into(&mut input_buffer) {
            Ok(bytes_read) => bytes_read,

            Err(SerialError::InputClosed) => {
                eprintln!("Serial input closed.");
                break;
            }

            Err(error) => {
                eprintln!("Serial read failed: {error:?}");
                break;
            }
        };

        let received = &input_buffer[..bytes_read];
        stdout().write_all(received).unwrap();
        stdout().flush().unwrap();

        let Some(one_char) = input_buffer.first() else {
            continue;
        };
        if *one_char == b'q' {
            running = false;
        }

        /* BROKEN
        *
                let result = commands::parse_command(&input_buffer);
                match result {
                    Ok(commands::Command::Quit) => {
                        running = false;
                    }

                    Ok(commands::Command::Restart) => {
                        println!("Restart is not implemented yet.");
                    }

                    Ok(commands::Command::Clear) => {
                        println!("Clear is not implemented yet.");
                    }

                    Err(commands::CommandError::NotFound) => {
                        println!("Unknown command: {}", input_buffer.trim());
                    }
                }
        */
    }

    println!("Exiting");
}

// notes and todos for way later:
//
// TODO: handle stderr stdin and stdout differently in TUI
// TODO: update with states in mind (render, input, serial data rx)
// TODO: event loop for keyboard_input, serial_rx, timeouts separates
