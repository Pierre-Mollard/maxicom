use std::{
    fs::File,
    io::{self, ErrorKind, stdout},
    path::Path,
    process::ExitCode,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
};
use ratatui::DefaultTerminal;

use crate::{
    app::{App, UpdateEffect, selection::copy_via_osc52},
    args::{ParseOutcome, RunMode, parse_args, print_help},
    bytesource::ByteSource,
    configs::{
        AppConfig, ConfigsError, KeyResolver, KeywordHighlight, Mode, SettingsConfig, ThemeConfig,
    },
    overlays::{ErrorDialog, Overlay},
    serial::{SerialConfig, SerialDevice, SerialError},
    ui::{PostRenderEffect, layout::LayoutWarning, render, status_bar::CompiledStatusBar},
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

/// Input events handled before a frame is drawn, so a flood can't stall rendering
const MAX_EVENTS_PER_FRAME: usize = 512;
/// Max time spent reading serial in one loop iteration before handling input and drawing
const RX_DRAIN_BUDGET: Duration = Duration::from_millis(5);
/// Min time between two frames (~60 fps), frames are only drawn when something changed
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// Redraw at least this often even when idle (status bar clock)
const MAX_FRAME_AGE: Duration = Duration::from_secs(1);
/// Longest wait for input when nothing is happening
const IDLE_WAIT: Duration = Duration::from_millis(10);

pub struct TerminalGuard {
    pub terminal: DefaultTerminal,
}

impl TerminalGuard {
    pub fn init() -> io::Result<Self> {
        let terminal = ratatui::try_init()?;
        execute!(stdout(), EnableFocusChange, EnableBracketedPaste)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            DisableFocusChange
        );
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

/// The config files, loaded and compiled for the app and the renderer
struct RuntimeConfig {
    settings: SettingsConfig,
    theme: ThemeConfig,
    key_resolver: KeyResolver,
    highlight_rules: Vec<KeywordHighlight>,
    status_bar: CompiledStatusBar,
}

fn load_runtime_config(config_path: &Path) -> Result<RuntimeConfig, ConfigsError> {
    let configs = AppConfig::load_or_create_default(config_path)?;
    let key_resolver = KeyResolver::try_new(&configs.keybindings)
        .expect("keybindings were already validated during config loading");
    let status_bar = CompiledStatusBar::new(&configs.settings.ui.status_bar, &configs.theme);
    Ok(RuntimeConfig {
        highlight_rules: configs.highlights.compile_rules(),
        settings: configs.settings,
        theme: configs.theme,
        key_resolver,
        status_bar,
    })
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let config_path = AppConfig::default_config_path();
    let RuntimeConfig {
        settings,
        mut theme,
        key_resolver,
        mut highlight_rules,
        mut status_bar,
    } = match load_runtime_config(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("maxicom: {err}");
            return Ok(ExitCode::FAILURE);
        }
    };

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
                last_chunk: Instant::now(),
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
    // set from `ui.mouse` in the loop, so its toggle and a reload apply it too (MAX-113)
    let mut mouse_captured = false;

    let mut running: bool = true;
    let is_replay = matches!(run_mode, RunMode::Replay { .. });
    let mut input_buffer = vec![0u8; settings.rx_buffer_size];

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

    let mut last_reconnect_attempt = Instant::now();
    let mut last_layout_warnings: Vec<LayoutWarning> = Vec::default();
    let mut last_rx_activity = Instant::now();
    let mut needs_redraw = true;
    let mut last_draw: Option<Instant> = None;

    'main: while running {
        let mut rx_received = false;

        if let Some(ref mut dev) = device {
            // read everything already waiting (bounded), instead of one buffer per frame
            let drain_start = Instant::now();
            loop {
                match dev.read(&mut input_buffer) {
                    Ok(0) => break,
                    Ok(bytes_read) => {
                        let chunk = &input_buffer[..bytes_read];
                        last_rx_activity = Instant::now();
                        rx_received = true;
                        app.stats.update_rx(bytes_read);
                        app.feed_rx(chunk);
                        if drain_start.elapsed() >= RX_DRAIN_BUDGET {
                            break;
                        }
                    }
                    Err(SerialError::InputClosed) => {
                        device = None;
                        app.open_overlay(Overlay::DeviceDisconnected(
                            app.serial_config.port.clone(),
                        ));
                        app.notify_warn("Device Lost");
                        needs_redraw = true;
                        break;
                    }
                    Err(SerialError::ReadFailed(e)) => {
                        app.open_overlay(Overlay::Error(
                            ErrorDialog::new(
                                "Serial Read Error",
                                "Could not read from device buffer",
                            )
                            .with_details(e),
                        ));
                        needs_redraw = true;
                        break;
                    }
                    Err(e) => {
                        eprintln!("Serial read error: {e:?}");
                        app.notify_error("Serial read error unkown");
                        break 'main;
                    }
                }
            }
            needs_redraw |= rx_received;
        }

        if device.is_some() {
            let inactivity_timeout = app.settings().inactivity_detach_timeout;
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
                needs_redraw = true;
            }
        } else {
            let is_detached_by_user = matches!(app.overlay(), Some(Overlay::DeviceDetached(_)));
            if !is_replay
                && !is_detached_by_user
                && last_reconnect_attempt.elapsed() >= Duration::from_millis(500)
            {
                last_reconnect_attempt = Instant::now();
                if let Ok(new_dev) = SerialDevice::open(&app.serial_config) {
                    device = Some(ByteSource::Live(new_dev));
                    last_rx_activity = Instant::now();
                    if let Some(Overlay::DeviceDisconnected(_)) = app.overlay() {
                        app.close_overlay();
                    }
                    app.notify_success(format!("Device Reconnected: {}", app.serial_config.port));
                    needs_redraw = true;
                }
            }
        }

        // the input poll is the loop's only idle wait: none while RX flows, otherwise until the
        // next frame is due (if one is pending) or IDLE_WAIT. Then drain what is already pending
        // (key bursts, pastes) before drawing, instead of one event per frame
        let since_draw = last_draw.map_or(Duration::MAX, |t| t.elapsed());
        let mut poll_timeout = if rx_received {
            Duration::ZERO
        } else if needs_redraw {
            FRAME_INTERVAL.saturating_sub(since_draw).min(IDLE_WAIT)
        } else {
            IDLE_WAIT
        };
        let mut handled_events = 0;
        while running
            && handled_events < MAX_EVENTS_PER_FRAME
            && let Ok(Some(event)) = inputs::poll_event(poll_timeout)
        {
            poll_timeout = Duration::ZERO;
            handled_events += 1;
            let action = inputs::handle_event(event, &app);
            // the mouse capture reports every move: one that does nothing draws nothing
            needs_redraw |= !matches!(action, inputs::Action::Ignore);
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
                                    last_rx_activity = Instant::now();
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
                                last_rx_activity = Instant::now();
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
                UpdateEffect::ReloadConfig => match load_runtime_config(&config_path) {
                    Ok(mut config) => {
                        if is_replay {
                            config.settings.mode = Mode::Terminal;
                        }
                        input_buffer.resize(config.settings.rx_buffer_size, 0);
                        theme = config.theme;
                        highlight_rules = config.highlight_rules;
                        status_bar = config.status_bar;
                        app.apply_reload(config.settings, config.key_resolver);
                    }
                    // the running config stays
                    Err(err) => app.open_overlay(Overlay::Error(
                        ErrorDialog::new(
                            "Config Reload Failed",
                            "The config files were not applied",
                        )
                        .with_details(err),
                    )),
                },
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

        if app.settings().ui.mouse != mouse_captured {
            mouse_captured = app.settings().ui.mouse;
            let _ = if mouse_captured {
                execute!(stdout(), EnableMouseCapture)
            } else {
                execute!(stdout(), DisableMouseCapture)
            };
        }

        needs_redraw |= app.tick();

        let since_draw = last_draw.map_or(Duration::MAX, |t| t.elapsed());
        needs_redraw |= since_draw >= MAX_FRAME_AGE;
        if !needs_redraw || since_draw < FRAME_INTERVAL {
            continue;
        }

        let mut post_render_effect = PostRenderEffect::None;
        guard.terminal.draw(|frame| {
            post_render_effect = render(&mut app, &theme, frame, &highlight_rules, &status_bar);
        })?;
        last_draw = Some(Instant::now());
        needs_redraw = false;

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
                        needs_redraw = true;
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
