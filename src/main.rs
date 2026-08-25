use std::io::stdout;
/// Orchestration and init of the MaxiCom executable
use std::process::ExitCode;
use std::time::Duration;

use ratatui::macros::ratatui_core::terminal;

use crate::app::App;
use crate::app::UpdateEffect;
use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
use crate::ui::render;

mod app;
mod config;
mod input;
mod serial;
mod ui;

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

    let device = match SerialDevice::open(config) {
        Ok(device) => device,
        Err(error) => {
            eprintln!("Could not open serial device: {error:?}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = ratatui::init();

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 4096]; // TODO: configurable
    let mut app = App::new(app_config);

    while running {
        // TODO: make this async with infinite delay (currently blocking for 10ms)
        match device.read_into(&mut input_buffer) {
            Ok(0) => {}
            Ok(bytes_read) => {
                app.feed_rx(&input_buffer[..bytes_read]);
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
            let action = input::handle_event(event);
            match app.update(action) {
                UpdateEffect::Quit => running = false,
                UpdateEffect::Transmit(bytes) => {
                    device.write_all(&bytes);
                }
                UpdateEffect::None => {}
            }
        }

        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
    }

    ratatui::restore();

    ExitCode::SUCCESS
}

// notes and todos for way later:
//
// TODO: handle stderr stdin and stdout differently in TUI
// TODO: update with states in mind (render, input, serial data rx)
// TODO: event loop for keyboard_input, serial_rx, timeouts separates
