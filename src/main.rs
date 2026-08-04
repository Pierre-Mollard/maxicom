use std::io;
use std::io::ErrorKind;

fn main() {
    println!("Hello, world!");

    let mut running: bool = true;
    let mut input_buffer: String = String::new();

    while running {
        input_buffer.clear();

        let bytes_read: usize = match io::stdin().read_line(&mut input_buffer) {
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
        println!("Read #{bytes_read} characters");

        // hangup detection is a successful 0 char read
        // (test with Ctrl-D with empty buffer on linux)
        if bytes_read == 0 {
            eprintln!("Input closed (EOF); exiting...");
            break;
        }

        let command = input_buffer.trim();

        if command == "quit" {
            running = false;
        } else if command.is_empty() {
            continue;
        } else {
            println!("Unknown command: {command}");
        }
    }

    println!("Exiting");
}

// notes and todos for way later:
//
// NOTE: for now treat input buffer as char, later as keystrokes
// TODO: handle stderr stdin and stdout differently in TUI
// TODO: update with states in mind (render, input, serial data rx)
