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
use crate::app::search::copy_via_osc52;
use crate::configs::AppConfig;
use crate::configs::InputMode;
use crate::configs::KeyResolver;
use crate::serial::SerialDevice;
use crate::serial::SerialError;
use crate::serial::parse_args;
use crate::serial::print_help;
use crate::types::ErrorDialog;
use crate::types::Overlay;
use crate::ui::PostRenderEffect;
use crate::ui::render;

mod app;
mod configs;
mod input;
mod logger;
mod notification;
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
    let config_path = AppConfig::default_config_path();
    let configs = match AppConfig::load_or_create_default(&config_path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("maxicom: {err}");
            return Ok(ExitCode::FAILURE);
        }
    };

    let settings = configs.settings;
    let theme = configs.theme;
    let keybindings = configs.keybindings;

    let key_resolver = KeyResolver::try_new(&keybindings)
        .expect("keybindings were already validated during config loading");

    let console_args = match parse_args() {
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

    let mut device = match SerialDevice::open(&console_args) {
        Ok(dev) => Some(dev),
        Err(SerialError::OpenFailed { ref source, .. }) if source.kind() == ErrorKind::NotFound => {
            None
        }
        Err(SerialError::InputClosed) => None,
        Err(err) => {
            eprintln!("maxicom: {err}"); // Display is implemented in serial.rs
            return Ok(ExitCode::FAILURE);
        }
    };

    let mut guard = TerminalGuard::init()?;

    let mut running: bool = true;
    let buffer_size = settings.rx_buffer_size;
    let mut input_buffer = vec![0u8; buffer_size];
    let mut app = App::new(settings, key_resolver, console_args);

    if device.is_none() {
        app.open_overlay(Overlay::DeviceDisconnected(app.serial_config.port.clone()));
    }

    let mut last_reconnect_attempt = std::time::Instant::now();

    while running {
        if let Some(ref mut dev) = device {
            match dev.read(&mut input_buffer) {
                Ok(0) => {}
                Ok(bytes_read) => match app.settings().input_mode {
                    InputMode::Line => {
                        app.line_mode.feed_rx(
                            app.settings().line_mode.rx_eol,
                            app.settings().encoding,
                            &app.settings().timestamp_format.to_string(),
                            &input_buffer[..bytes_read],
                        );
                    }
                    InputMode::Terminal => {
                        app.terminal_mode
                            .process_rx(app.settings().encoding, &input_buffer[..bytes_read]);
                    }
                },
                Err(SerialError::InputClosed) => {
                    device = None; // Drops closed handle
                    app.open_overlay(Overlay::DeviceDisconnected(app.serial_config.port.clone()));
                    app.notify_warn("Device Lost");
                }
                Err(SerialError::ReadFailed(e)) => {
                    app.open_overlay(Overlay::Error(
                        ErrorDialog::new("Serial Read Error", "Could not read from device buffer")
                            .with_details(e),
                    ));
                }
                Err(e) => {
                    eprintln!("Serial read error: {e:?}");
                    app.notify_error("Serial read error unkown");
                    break;
                }
            }
        } else {
            if last_reconnect_attempt.elapsed() >= Duration::from_millis(500) {
                last_reconnect_attempt = std::time::Instant::now();
                if let Ok(new_dev) = SerialDevice::open(&app.serial_config) {
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
                UpdateEffect::Reconfigure(serial_config) => {
                    if let Some(ref mut dev) = device {
                        if let Err(e) = dev.reconfigure(&serial_config) {
                            app.open_overlay(Overlay::Error(
                                ErrorDialog::new(
                                    "Serial Conf Rejected",
                                    "Failed to reconfigure serial",
                                )
                                .with_details(e),
                            ));
                        } else {
                            app.serial_config = serial_config;
                        }
                    }
                }
                UpdateEffect::CopyToClipboard(text) => {
                    execute_clipboard_copy(&mut app, &text);
                }
                UpdateEffect::None => {}
            }
        }

        let mut post_render_effect = PostRenderEffect::None;
        guard.terminal.draw(|frame| {
            post_render_effect = render(&mut app, &theme, frame);
        })?;

        match post_render_effect {
            PostRenderEffect::None => (),
            PostRenderEffect::CopyToClipboard(text) => execute_clipboard_copy(&mut app, &text),
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn execute_clipboard_copy(app: &mut App, text: &str) {
    if let Err(e) = copy_via_osc52(text) {
        app.notify_error(format!("Clipboard error: {e}"));
    } else {
        let count = text.chars().count();
        app.notify_success(format!("Copied {count} chars to clipboard"));
    }
}
