use std::path::PathBuf;
/// Orchestration and init of the MaxiCom executable
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::DisableFocusChange;
use crossterm::event::EnableFocusChange;
use crossterm::execute;

use crate::app::App;
use crate::app::UpdateEffect;
use crate::config::AppConfig;
use crate::config::InputMode;
use crate::logger::LogConfig;
use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
use crate::types::ErrorDialog;
use crate::types::Overlay;
use crate::ui::render;

mod app;
mod config;
mod input;
mod logger;
mod serial;
mod terminal;
mod types;
mod ui;

const EXIT_USAGE: u8 = 2;
// 0 -- Success
// 1 -- General failure
// 2 -- Invalid argument

fn main() -> ExitCode {
    println!("=== MaxiCom ===");

    // TODO: handle warning error
    let config_path = AppConfig::default_path();
    let app_config = AppConfig::load_or_create_default(&config_path).unwrap();

    let log_config = LogConfig {
        enabled: true,
        path: PathBuf::from("maxicom_test_file.log"),
        timestamp_lines: true,
    };

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

    // TODO: handle all error case and display popups/retries (ConfigFailed & OpenFailed)
    let mut device = match SerialDevice::open(&config) {
        Ok(device) => Some(device),
        Err(error) => {
            eprintln!("Could not open serial device: {error:?}");
            None
        }
    };

    let mut terminal = ratatui::init();
    _ = execute!(std::io::stdout(), EnableFocusChange);

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 4096]; // TODO: configurable
    let mut app = App::new(app_config, log_config);

    if device.is_none() {
        app.open_overlay(Overlay::DeviceDisconnected(config.port.clone()));
    }

    let mut last_reconnect_attempt = std::time::Instant::now();

    while running {
        if let Some(ref mut dev) = device {
            match dev.read(&mut input_buffer) {
                Ok(0) => {}
                Ok(bytes_read) => match app.config().input_mode {
                    InputMode::Line => {
                        app.line_mode
                            .feed_rx(app.config().rx_eol_mode, &input_buffer[..bytes_read]);
                    }
                    InputMode::Terminal => {
                        app.terminal_mode.process_rx(&input_buffer[..bytes_read]);
                    }
                },
                Err(SerialError::InputClosed) => {
                    device = None; // Drops closed handle
                    app.open_overlay(Overlay::DeviceDisconnected(config.port.clone()));
                }
                Err(SerialError::ReadFailed(e)) => {
                    app.open_overlay(Overlay::Error(
                        ErrorDialog::new("Serial Read Error", "Could not read from device buffer")
                            .with_details(e),
                    ));
                }
                Err(e) => {
                    eprintln!("Serial read error: {e:?}");
                    break;
                }
            }
        } else {
            if last_reconnect_attempt.elapsed() >= Duration::from_millis(500) {
                last_reconnect_attempt = std::time::Instant::now();
                if let Ok(new_dev) = SerialDevice::open(&config) {
                    device = Some(new_dev);
                    if let Some(Overlay::DeviceDisconnected(_)) = app.overlay() {
                        app.close_overlay();
                    }
                }
            }
        }

        if let Ok(Some(event)) = input::poll_event(Duration::from_millis(10)) {
            let action = input::handle_event(event, &app);
            match app.update(action) {
                UpdateEffect::Quit => running = false,
                UpdateEffect::Transmit(bytes) => {
                    if let Some(ref mut dev) = device
                        && let Err(e) = dev.write(&bytes)
                    {
                        app.open_overlay(Overlay::Error(
                            ErrorDialog::new(
                                "Serial Write Failed",
                                "Failed to transmit bytes to serial port",
                            )
                            .with_details(e),
                        ));
                    }
                }
                UpdateEffect::None => {}
            }
        }

        terminal
            .draw(|frame| {
                render(&mut app, frame);
            })
            .unwrap();
    }

    _ = crossterm::execute!(std::io::stdout(), DisableFocusChange);
    ratatui::restore();

    ExitCode::SUCCESS
}
// TODO: handle maxicom program stderr in red special output in log
