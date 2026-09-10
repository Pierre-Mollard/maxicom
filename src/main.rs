use std::path::PathBuf;
/// Orchestration and init of the MaxiCom executable
use std::process::ExitCode;
use std::time::Duration;

use crate::app::App;
use crate::app::UpdateEffect;
use crate::config::AppConfig;
use crate::config::InputMode;
use crate::logger::LogConfig;
use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
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

    let mut device = match SerialDevice::open(&config) {
        Ok(device) => device,
        Err(error) => {
            eprintln!("Could not open serial device: {error:?}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = ratatui::init();

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 4096]; // TODO: configurable
    let mut app = App::new(app_config, log_config);

    while running {
        // TODO: make this async with infinite delay (currently blocking for 10ms)
        match device.read(&mut input_buffer) {
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
                break;
            }
            Err(e) => {
                eprintln!("Serial read error: {e:?}");
                break;
            }
        }

        if let Ok(Some(event)) = input::poll_event(Duration::from_millis(10)) {
            let action = input::handle_event(event, &app);
            match app.update(action) {
                UpdateEffect::Quit => running = false,
                UpdateEffect::Transmit(bytes) => match device.write(&bytes) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Serial write error: {e:?}");
                        break;
                    }
                },
                UpdateEffect::None => {}
            }
        }

        terminal
            .draw(|frame| {
                render(&mut app, frame);
            })
            .unwrap();
    }

    ratatui::restore();

    ExitCode::SUCCESS
}
// TODO: handle maxicom program stderr in red special output in log
