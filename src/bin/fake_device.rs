use serial2::SerialPort;
use std::{io::ErrorKind, thread::sleep, time::Duration};

fn main() {
    let mut running: bool = true;
    let mut input_buffer = [0_u8; 1024];

    println!(">> Waiting");
    sleep(Duration::from_secs(3));
    println!(">> Starting");

    let port =
        SerialPort::open("/tmp/fake-device", 115200).expect("fatal error opening fake device");
    port.write(b"I am Starting").unwrap();

    while running {
        let bytes_read: usize = match port.read(&mut input_buffer) {
            Ok(bytes_read) => bytes_read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                eprintln!(">> Input was Interrupted; trying again...");
                continue;
            }
            Err(error) => {
                eprintln!(">> Could not read input: {error}");
                //break;
                continue;
            }
        };

        // hangup detection is a successful 0 char read
        // (test with Ctrl-D with empty buffer on linux)
        if bytes_read == 0 {
            eprintln!(">> Input closed (EOF); exiting...");
            break;
        }

        // FIXME: this only received 1byte at a time so never catch a quit
        let received = &input_buffer[..bytes_read];
        println!(">> RX {bytes_read} bytes: {received:02X?}");
        let quit_command = b"quit";
        let contains_quit = received
            .windows(quit_command.len())
            .any(|window| window == quit_command);

        if contains_quit {
            port.write(b"I am Quitting...").unwrap();
            running = false;
        } else {
            port.write(b"I am Echoing: ").unwrap();
            port.write(received).unwrap();
        }
    }

    println!(">> Exiting");
}
