use std::process::ExitCode;
use std::time::Duration;

use crate::app::App;
use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
use crate::terminal::TerminalGuard;

mod app;
mod commands;
mod config;
mod input;
mod serial;
mod terminal;

const EXIT_USAGE: u8 = 2;
// 0 -- Success
// 1 -- General failure
// 2 -- Invalid argument

fn main() -> ExitCode {
    println!("=== MaxiCom ===");

    let app_config = config::AppConfig::default();

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

    // LOOP (TODO: move in app.rs)

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 1024];
    let mut app = App::new(100);

    app.render();

    while running {
        let mut needs_render = false;

        // TODO: make this async (currently blocking for 10ms)
        match device.read_into(&mut input_buffer) {
            Ok(0) => {}
            Ok(bytes_read) => {
                app.feed_rx(&input_buffer[..bytes_read]);
                needs_render = true;
            }
            Err(SerialError::InputClosed) => {
                break;
            }
            Err(e) => {
                eprintln!("Serial read error: {e:?}");
                break;
            }
        }

        // TODO: handle unwrap better
        if let Some(event) = input::poll_event(Duration::from_millis(10)).unwrap() {
            match input::handle_event(event) {
                input::Action::InsertChar(c) => {
                    app.append_input_char(c);
                    needs_render = true;
                }
                input::Action::DeleteBack => {
                    app.delete_last_input_char();
                    needs_render = true;
                }
                input::Action::Submit => {
                    let line = app.commit_input_line();

                    if app_config.local_echo {
                        app.push_history(&line);
                    }

                    device.write_all(line.as_bytes());
                    device.write_all(app_config.eol_mode.as_bytes());
                    needs_render = true;
                }
                input::Action::Quit => {
                    running = false;
                }
                input::Action::Ignore => {}
            }
        }

        if needs_render {
            app.render();
        }
    }

    guard.cleanup();
    println!("Exiting");

    ExitCode::SUCCESS
}

// notes and todos for way later:
//
// TODO: handle stderr stdin and stdout differently in TUI
// TODO: update with states in mind (render, input, serial data rx)
// TODO: event loop for keyboard_input, serial_rx, timeouts separates
