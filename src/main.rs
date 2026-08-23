use std::io::Write;
use std::io::stdout;
use std::process::ExitCode;

use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
use crate::terminal::TerminalGuard;

mod commands;
mod serial;
mod terminal;

const EXIT_USAGE: u8 = 2;
// 0 -- Success
// 1 -- General failure
// 2 -- Invalid argument

fn main() -> ExitCode {
    println!("=== MaxiCom ===");

    let config = match parse_args() {
        Ok(serial::ParseOutcome::Config(config)) => config,
        Ok(serial::ParseOutcome::Help) => {
            print_help();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("Argument error: {error}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    println!("Configuration: {config:#?}");

    let mut guard = TerminalGuard::enter().unwrap();

    let device = match SerialDevice::open(config) {
        Ok(device) => device,
        Err(error) => {
            eprintln!("Could not open serial device: {error:?}");
            return ExitCode::FAILURE;
        }
    };

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 1024];

    print!("> starting receiving:\r\n");
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
    }

    println!("Exiting");
    guard.cleanup();

    ExitCode::SUCCESS
}

// notes and todos for way later:
//
// TODO: handle stderr stdin and stdout differently in TUI
// TODO: update with states in mind (render, input, serial data rx)
// TODO: event loop for keyboard_input, serial_rx, timeouts separates
