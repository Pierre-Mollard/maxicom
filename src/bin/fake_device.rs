use std::io::{self, ErrorKind};

fn main() {
    let mut running: bool = true;
    let mut input_buffer: String = String::new();

    println!("Starting");

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

        if input_buffer.contains("quit") {
            println!("quitting...");
            running = false;
        } else {
            println!("Echoing: {}", input_buffer);
        }
    }

    println!("Exiting");
}
