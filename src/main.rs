use std::io;
use std::io::ErrorKind;
use std::io::stdout;
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::DisableFocusChange;
use crossterm::event::EnableFocusChange;
use crossterm::execute;
use ratatui::DefaultTerminal;

use crate::app::App;
use crate::app::UpdateEffect;
use crate::config::AppConfig;
use crate::config::InputMode;
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

// 0 -- Success
// 1 -- General failure
// 2 -- Invalid argument
const EXIT_USAGE: u8 = 2;

pub struct TerminalGuard {
    pub terminal: DefaultTerminal,
}

impl TerminalGuard {
    pub fn init() -> io::Result<Self> {
        let terminal = ratatui::try_init()?;
        execute!(stdout(), EnableFocusChange)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableFocusChange);
        ratatui::restore();
    }
}

// TODO: handle maxicom program stderr in red special output in log
fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            // Because TerminalGuard dropped BEFORE this point,
            // stderr is back to standard terminal mode and readable!
            eprintln!("maxicom: fatal error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let config_path = AppConfig::default_path();
    let app_config = match AppConfig::load_or_create_default(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "maxicom: failed to load config at {}: {e}",
                config_path.display()
            );
            return Ok(ExitCode::FAILURE);
        }
    };

    let config = match parse_args() {
        Ok(serial::ParseOutcome::Config(config)) => config,
        Ok(serial::ParseOutcome::Help) => {
            print_help();
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => {
            eprintln!("Argument error: {error}");
            return Ok(ExitCode::from(EXIT_USAGE));
        }
    };

    println!("Configuration: {config:#?}");

    let mut device = match SerialDevice::open(&config) {
        Ok(dev) => Some(dev),
        Err(SerialError::OpenFailed { ref source, .. }) if source.kind() == ErrorKind::NotFound => {
            None
        }
        Err(SerialError::InputClosed) => None,
        Err(SerialError::OpenFailed {
            ref port,
            ref source,
            ..
        }) if source.kind() == ErrorKind::PermissionDenied => {
            eprintln!("maxicom: permission denied opening serial port '{port}'");
            return Ok(ExitCode::FAILURE);
        }

        Err(SerialError::OpenFailed {
            ref port,
            ref source,
            ..
        }) if source.kind() == ErrorKind::ResourceBusy => {
            eprintln!("maxicom: serial port '{port}' is busy (already in use)");
            return Ok(ExitCode::FAILURE);
        }

        Err(SerialError::OpenFailed {
            ref port,
            baudrate,
            ref source,
        }) => {
            eprintln!("maxicom: failed to open '{port}' at {baudrate} baud: {source}");
            if let Some(code) = source.raw_os_error() {
                eprintln!("OS errno: {code}");
            }
            return Ok(ExitCode::FAILURE);
        }
        Err(SerialError::ConfigFailed(ref source)) => {
            eprintln!(
                "maxicom: failed to configure UART settings for '{}'",
                config.port
            );
            eprintln!();
            eprintln!("Driver Error: {source}");
            return Ok(ExitCode::FAILURE);
        }
        Err(SerialError::ReadFailed(ref source)) => {
            eprintln!(
                "maxicom: unexpected read error while probing '{}': {source}",
                config.port
            );
            return Ok(ExitCode::FAILURE);
        }
        Err(SerialError::WriteFailed {
            bytes_attempted,
            ref source,
        }) => {
            eprintln!(
                "maxicom: failed to send probe bytes ({bytes_attempted} attempted) to '{}': {source}",
                config.port
            );
            return Ok(ExitCode::FAILURE);
        }
    };

    let mut guard = TerminalGuard::init()?;

    let mut running: bool = true;
    let mut input_buffer = [0_u8; 4096]; // TODO: configurable
    let mut app = App::new(app_config);

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

        guard.terminal.draw(|frame| {
            render(&mut app, frame);
        })?;
    }

    Ok(ExitCode::SUCCESS)
}
