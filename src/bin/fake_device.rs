use serial2::SerialPort;
use std::{
    io::ErrorKind,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, sleep},
    time::Duration,
};

fn main() {
    let quit_command = b"quit";
    let spam_command = b"spam";
    let askspam_command = b"spam?";

    let is_spamming = Arc::new(AtomicBool::new(false));
    let is_running = Arc::new(AtomicBool::new(true));

    let mut input_buffer = [0_u8; 1024];

    println!(">> Waiting");
    sleep(Duration::from_secs(3));
    println!(">> Starting");

    let port =
        SerialPort::open("/tmp/fake-device", 115200).expect("fatal error opening fake device");
    port.write(b"I am Starting\r\n").unwrap();

    let tx_port = port.try_clone().expect("failed to clone serial port");
    let spam_flag = Arc::clone(&is_spamming);
    let run_flag = Arc::clone(&is_running);
    let spam_handle = thread::spawn(move || {
        let mut count: u32 = 0;
        while run_flag.load(Ordering::Relaxed) {
            if spam_flag.load(Ordering::Relaxed) {
                let msg = format!("SPAM #{count}: sensor_val={}\r\n", count * 10);
                if tx_port.write(msg.as_bytes()).is_err() {
                    break;
                }
                count += 1;
                sleep(Duration::from_millis(200)); // 5 messages/sec
            } else {
                sleep(Duration::from_millis(50));
            }
        }
    });

    while is_running.load(std::sync::atomic::Ordering::Relaxed) {
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

        let received = &input_buffer[..bytes_read];
        println!(">> RX {bytes_read} bytes: {received:02X?}");
        let contains_quit = received
            .windows(quit_command.len())
            .any(|window| window == quit_command);
        let contains_spam = received
            .windows(spam_command.len())
            .any(|window| window == spam_command);
        let contains_askspam = received
            .windows(askspam_command.len())
            .any(|window| window == askspam_command);

        // a serial is always (often...) echoing what it received, if not use local_echo on client
        port.write(received).unwrap();

        if contains_quit {
            port.write(b"I am Quitting...\r\n").unwrap();
            is_running.store(false, std::sync::atomic::Ordering::Relaxed);
        } else if contains_askspam {
            let current = is_spamming.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
            port.write(format!("{current:#?}\r\n").as_bytes()).unwrap();
        } else if contains_spam {
            let prev = is_spamming.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
            is_spamming.store(!prev, std::sync::atomic::Ordering::Relaxed);
            if !prev {
                port.write(b"Spamming...\r\n").unwrap();
            } else {
                port.write(b"Stop Spamming...\r\n").unwrap();
            }
        }
    }

    is_running.store(false, Ordering::Relaxed);
    let _ = spam_handle.join();
    println!(">> Exiting");
}
