use serial2::SerialPort;
use std::env;
use std::io::ErrorKind;
use std::io::Write;
use std::io::stdout;

mod commands;

fn main() {
    println!("=== maxicom ===");

    let mut args = env::args();
    let program_name = args.next();
    println!("program name : {program_name:?}");

    for arg in args {
        println!("argument : {arg:?}");
    }

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 1024];

    let port = SerialPort::open("/tmp/maxicom", 115200).expect("fatal error opening fake device");

    // TODO: check read vectors directly
    println!("> starting receiving:");
    while running {
        let bytes_read: usize = match port.read(&mut input_buffer) {
            Ok(bytes_read) => bytes_read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                eprintln!("Input was Interrupted; trying again...");
                continue;
            }
            Err(error) => {
                eprintln!("Could not read input: {error}");
                break;
            }
        };
        //println!("Read #{bytes_read} characters");

        // hangup detection is a successful 0 char read
        // (test with Ctrl-D with empty buffer on linux)
        if bytes_read == 0 {
            eprintln!("Input closed (EOF); exiting...");
            break;
        }

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
