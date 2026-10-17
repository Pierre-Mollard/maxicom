use std::{
    fs::File,
    io::{self, ErrorKind, stdout},
    process::ExitCode,
    time::Duration,
};

use crossterm::{
    event::{DisableFocusChange, EnableFocusChange},
    execute,
};
use ratatui::DefaultTerminal;

use crate::{
    app::{App, UpdateEffect, selection::copy_via_osc52},
    args::{ParseOutcome, RunMode, parse_args, print_help},
    bytesource::ByteSource,
    configs::{AppConfig, KeyResolver, Mode},
    overlays::{ErrorDialog, Overlay},
    serial::{SerialConfig, SerialDevice, SerialError},
    ui::{PostRenderEffect, layout::LayoutWarning, render},
};

mod app;
mod args;
mod bytesource;
mod configs;
mod inputs;
mod logger;
mod notification;
mod overlays;
mod serial;
mod terminal;
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
    let highlights = configs.highlights;

    let key_resolver = KeyResolver::try_new(&keybindings)
        .expect("keybindings were already validated during config loading");
    let highlight_rules = highlights.compile_rules();

    let run_mode = match parse_args() {
        Ok(ParseOutcome::Run(mode)) => mode,
        Ok(ParseOutcome::Help) => {
            print_help();
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => {
            eprintln!("Argument error: {error}");
            return Ok(ExitCode::from(EXIT_USAGE));
        }
    };

    let mut device: Option<ByteSource> = match &run_mode {
        RunMode::Live(cfg) => match SerialDevice::open(cfg) {
            Ok(dev) => Some(ByteSource::Live(dev)),
            Err(SerialError::OpenFailed { ref source, .. })
                if source.kind() == ErrorKind::NotFound =>
            {
                None
            }
            Err(SerialError::InputClosed) => None,
            Err(err) => {
                eprintln!("maxicom: could not open serial device: {err}");
                return Ok(ExitCode::FAILURE);
            }
        },
        RunMode::Replay {
            file_path,
            delay_ms,
        } => match File::open(file_path) {
            Ok(file) => Some(ByteSource::Replay {
                file,
                delay: Duration::from_millis(*delay_ms),
                last_chunk: std::time::Instant::now(),
                finished: false,
            }),
            Err(err) => {
                eprintln!(
                    "maxicom: could not open replay file {}: {err}",
                    file_path.display()
                );
                return Ok(ExitCode::FAILURE);
            }
        },
    };

    let mut guard = TerminalGuard::init()?;

    let mut running: bool = true;
    let is_replay = matches!(run_mode, RunMode::Replay { .. });
    let buffer_size = settings.rx_buffer_size;
    let inactivity_timeout = settings.inactivity_detach_timeout;
    let mut input_buffer = vec![0u8; buffer_size];

    let serial_config = match &run_mode {
        RunMode::Live(cfg) => cfg.clone(),
        RunMode::Replay { file_path, .. } => SerialConfig {
            port: format!("file://{}", file_path.display()),
            ..SerialConfig::default()
        },
    };
    let mut app = App::new(settings, key_resolver, serial_config);
    if is_replay {
        app.settings_mut().mode = Mode::Terminal;
    }

    if device.is_none() {
        app.open_overlay(Overlay::DeviceDisconnected(app.serial_config.port.clone()));
    }

    let mut last_reconnect_attempt = std::time::Instant::now();
    let mut last_layout_warnings: Vec<LayoutWarning> = Vec::default();
    let mut last_rx_activity = std::time::Instant::now();

    while running {
        if let Some(ref mut dev) = device {
            match dev.read(&mut input_buffer) {
                Ok(0) => {}
                Ok(bytes_read) => {
                    last_rx_activity = std::time::Instant::now();
                    app.stats.update_rx(bytes_read);
                    match app.settings().mode {
                        Mode::Line => {
                            app.line_mode.feed_rx(
                                app.settings().line_mode.rx_eol,
                                app.settings().encoding,
                                &app.settings().timestamp_format.to_string(),
                                &input_buffer[..bytes_read],
                            );
                        }
                        Mode::Terminal => {
                            app.terminal_mode
                                .process_rx(app.settings().encoding, &input_buffer[..bytes_read]);
                        }
                    };
                }
                Err(SerialError::InputClosed) => {
                    device = None;
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

            if !is_replay
                && !inactivity_timeout.is_zero()
                && last_rx_activity.elapsed() >= inactivity_timeout
            {
                device = None;
                app.open_overlay(Overlay::DeviceDetached(app.serial_config.port.clone()));
                app.notify_warn(format!(
                    "Device idle for {}s: port detached",
                    inactivity_timeout.as_secs()
                ));
            }
        } else {
            let is_detached_by_user = matches!(app.overlay(), Some(Overlay::DeviceDetached(_)));
            if !is_replay
                && !is_detached_by_user
                && last_reconnect_attempt.elapsed() >= Duration::from_millis(500)
            {
                last_reconnect_attempt = std::time::Instant::now();
                if let Ok(new_dev) = SerialDevice::open(&app.serial_config) {
                    device = Some(ByteSource::Live(new_dev));
                    last_rx_activity = std::time::Instant::now();
                    if let Some(Overlay::DeviceDisconnected(_)) = app.overlay() {
                        app.close_overlay();
                    }
                    app.notify_success(format!("Device Reconnected: {}", app.serial_config.port));
                }
            }
        }

        if let Ok(Some(event)) = inputs::poll_event(Duration::from_millis(10)) {
            let action = inputs::handle_event(event, &app);
            match app.update(action) {
                UpdateEffect::Quit => running = false,
                UpdateEffect::ToggleAttachSerial => {
                    if !is_replay {
                        if device.is_some() {
                            device = None;
                            app.open_overlay(Overlay::DeviceDetached(
                                app.serial_config.port.clone(),
                            ));
                            app.notify_warn("Serial Port Released (Detached)");
                        } else {
                            match SerialDevice::open(&app.serial_config) {
                                Ok(dev) => {
                                    device = Some(ByteSource::Live(dev));
                                    last_rx_activity = std::time::Instant::now();
                                    app.close_overlay();
                                    app.notify_success(format!(
                                        "Reconnected to {}",
                                        app.serial_config.port
                                    ));
                                }
                                Err(e) => {
                                    app.notify_error(format!("Failed to reattach: {e}"));
                                }
                            }
                        }
                    }
                }
                UpdateEffect::Transmit(bytes) => {
                    if let Some(ref mut dev) = device {
                        match dev.write(&bytes) {
                            Ok(()) => {
                                last_rx_activity = std::time::Instant::now();
                                if !is_replay {
                                    app.stats.update_tx(bytes.len());
                                }
                            }
                            Err(e) => {
                                app.open_overlay(Overlay::Error(
                                    ErrorDialog::new(
                                        "Serial Write Failed",
                                        "Failed to transmit bytes to serial port",
                                    )
                                    .with_details(e),
                                ));
                            }
                        }
                    }
                }
                UpdateEffect::Reconfigure(serial_config) => {
                    if !is_replay && let Some(ByteSource::Live(ref mut dev)) = device {
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
                UpdateEffect::Screenshot(filename) => match filename {
                    Ok(name) => {
                        app.notify_success(format!("Screenshot saved to {name}"));
                    }
                    Err(err) => {
                        app.notify_error(format!("Failed to save screenshot: {err}"));
                    }
                },
            }
        }

        let mut post_render_effect = PostRenderEffect::None;
        guard.terminal.draw(|frame| {
            post_render_effect = render(&mut app, &theme, frame, &highlight_rules);
        })?;

        match post_render_effect {
            PostRenderEffect::None => {
                last_layout_warnings.clear();
            }
            PostRenderEffect::_CopyToClipboard(text) => execute_clipboard_copy(&mut app, &text),
            PostRenderEffect::LayoutWarnings(current_warnings) => {
                for warn in &current_warnings {
                    if !last_layout_warnings.contains(warn) {
                        let msg = match warn {
                            LayoutWarning::SidePanelTooSmall {
                                required,
                                actual,
                                orientation,
                            } => {
                                format!(
                                    "Side panel hidden: requires at least {} {:?}, but screen has only {}",
                                    required, orientation, actual
                                )
                            }
                            LayoutWarning::WindowTooSmall {
                                min_cols,
                                min_rows,
                                actual_cols,
                                actual_rows,
                            } => {
                                format!(
                                    "Terminal window too small ({}x{}). Minimum is {}x{}",
                                    actual_cols, actual_rows, min_cols, min_rows
                                )
                            }
                        };
                        app.notify_warn(msg);
                    }
                }
                last_layout_warnings = current_warnings;
            }
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
