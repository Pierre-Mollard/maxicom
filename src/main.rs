use std::{
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
    configs::{
        AppConfig, ConfigsError, KeyResolver, KeywordHighlight, Mode, SettingsConfig, ThemeConfig,
    },
    link::{
        ByteSource, Opener,
        replay::Replay,
        serial::{SerialConfig, SerialError},
        tcp_address,
    },
    overlays::{ErrorDialog, Overlay},
    ui::{PostRenderEffect, layout::LayoutWarning, render, status_bar::CompiledStatusBar},
};

mod app;
mod args;
mod capture;
mod commands;
mod configs;
mod inputs;
mod link;
mod notification;
mod overlays;
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

    let (run_mode, restore) = match parse_args() {
        Ok(ParseOutcome::Run { mode, restore }) => (mode, restore),
        Ok(ParseOutcome::Help) => {
            print_help();
            return Ok(ExitCode::SUCCESS);
        }
        // the folder to look into the backups out of the app: `cd $(maxicom --cache-dir)`
        Ok(ParseOutcome::CacheDir) => match AppConfig::default_journal_dir() {
            Some(dir) => {
                println!("{}", dir.display());
                return Ok(ExitCode::SUCCESS);
            }
            None => {
                eprintln!("maxicom: no cache folder on this system");
                return Ok(ExitCode::FAILURE);
            }
        },
        Err(error) => {
            eprintln!("Argument error: {error}");
            return Ok(ExitCode::from(EXIT_USAGE));
        }
    };

    let mut device: Option<ByteSource> = match &run_mode {
        // connected in the loop, like a reconnect, so the UI is up and says it's connecting.
        // The arguments refused a malformed address
        RunMode::Live(cfg) if tcp_address(&cfg.port).is_some() => None,
        // serial without tcp for sure here
        RunMode::Live(cfg) => match link::open(cfg) {
            Ok(dev) => Some(dev),
            Err(SerialError::OpenFailed { ref source, .. })
                if source.kind() == ErrorKind::NotFound =>
            {
                None
            }
            Err(SerialError::InputClosed) => None,
            Err(err) => {
                eprintln!("maxicom: could not open the device: {err}");
                return Ok(ExitCode::FAILURE);
            }
        },
        RunMode::Replay {
            file_path,
            delay_ms,
        } => match Replay::open(file_path, Duration::from_millis(*delay_ms)) {
            Ok(replay) => Some(ByteSource::Replay(replay)),
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
    // a replay is a file already: no backup of it, nothing to restore
    if is_replay {
        app.settings_mut().mode = Mode::Terminal;
    } else {
        // before the new journal, whose pruning could remove the last session (MAX-148)
        let journal_dir = AppConfig::default_journal_dir();
        app.restore_last_session(journal_dir.as_deref(), restore);
        app.start_journal(journal_dir);
    }

    let mut last_reconnect_attempt = Instant::now();
    let mut opener = Opener::default();
    let mut open_for = OpenFor::Retry;
    // the restore question comes first, the missing device's popup once it's answered (MAX-148)
    let mut disconnected_after_restore = false;
    if device.is_none() && tcp_address(&app.serial_config.port).is_some() {
        opener.start(&app.serial_config);
        open_for = OpenFor::Start;
        app.notify_info(format!("Connecting to {}", app.serial_config.port));
    } else if device.is_none() {
        if matches!(app.overlay(), Some(Overlay::RestoreSession(_))) {
            disconnected_after_restore = true;
        } else {
            app.open_overlay(Overlay::DeviceDisconnected(app.serial_config.port.clone()));
        }
    }

    let mut last_layout_warnings: Vec<LayoutWarning> = Vec::default();
    let mut last_rx_activity = Instant::now();
    let mut needs_redraw = true;
    let mut last_draw: Option<Instant> = None;

    'main: while running {
        let mut rx_received = false;

        if disconnected_after_restore && app.overlay().is_none() {
            disconnected_after_restore = false;
            if device.is_none() {
                app.open_overlay(Overlay::DeviceDisconnected(app.serial_config.port.clone()));
                needs_redraw = true;
            }
        }

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

            // the replies to the remote's queries, never in a replay (MAX-151)
            let answers = app.take_query_answers();
            if !answers.is_empty()
                && !is_replay
                && let Some(ref mut dev) = device
            {
                transmit(&mut app, dev, &answers, is_replay);
            }
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
                opener.start(&app.serial_config);
            }
            match opener.finished() {
                Some(Ok(new_dev)) => {
                    device = Some(new_dev);
                    last_rx_activity = Instant::now();
                    let port = &app.serial_config.port;
                    match open_for {
                        OpenFor::Start => app.notify_success(format!("Connected to {port}")),
                        OpenFor::Attach => {
                            app.notify_success(format!("Reconnected to {port}"));
                            app.close_overlay();
                        }
                        OpenFor::Retry => {
                            app.notify_success(format!("Device Reconnected: {port}"));
                            if let Some(Overlay::DeviceDisconnected(_)) = app.overlay() {
                                app.close_overlay();
                            }
                        }
                    }
                    open_for = OpenFor::Retry;
                    needs_redraw = true;
                }
                Some(Err(e)) => {
                    match open_for {
                        // what a missing port does at start, after the restore question
                        OpenFor::Start => {
                            app.notify_warn(e.to_string());
                            if matches!(app.overlay(), Some(Overlay::RestoreSession(_))) {
                                disconnected_after_restore = true;
                            } else {
                                app.open_overlay(Overlay::DeviceDisconnected(
                                    app.serial_config.port.clone(),
                                ));
                            }
                        }
                        OpenFor::Attach => app.notify_error(format!("Failed to reattach: {e}")),
                        OpenFor::Retry => {}
                    }
                    open_for = OpenFor::Retry;
                    needs_redraw = true;
                }
                None => {}
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
            // the event's action, then the queued commands of a command line chain one at a
            // time, each effect applied before the next is built (MAX-12)
            let mut next = Some(action);
            while let Some(action) = next.take() {
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
                                // the result comes with the reconnect's, next frame for a port
                                open_for = OpenFor::Attach;
                                opener.start(&app.serial_config);
                                if tcp_address(&app.serial_config.port).is_some() {
                                    app.notify_info(format!(
                                        "Connecting to {}",
                                        app.serial_config.port
                                    ));
                                }
                            }
                        }
                    }
                    UpdateEffect::Transmit(bytes) => {
                        if let Some(ref mut dev) = device
                            && transmit(&mut app, dev, &bytes, is_replay)
                        {
                            last_rx_activity = Instant::now();
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
                        execute_clipboard_copy(&mut app, &text, None);
                    }
                    UpdateEffect::CopyToClipboardAs { text, what } => {
                        execute_clipboard_copy(&mut app, &text, Some(&what));
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
                }
                if running {
                    next = app.next_command_action();
                }
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
            PostRenderEffect::_CopyToClipboard(text) => {
                execute_clipboard_copy(&mut app, &text, None)
            }
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

/// Who started the open that runs, which decides what its result shows: a retry stays quiet
/// until it connects, the user's attach reports a failure, the start shows the popup
enum OpenFor {
    Start,
    Attach,
    Retry,
}

/// Writes `bytes` to the device, counted and recorded (not in a replay), or opens the error
/// popup. True when written
fn transmit(app: &mut App, dev: &mut ByteSource, bytes: &[u8], is_replay: bool) -> bool {
    match dev.write(bytes) {
        Ok(()) => {
            if !is_replay {
                app.stats.update_tx(bytes.len());
                app.record_tx(bytes);
            }
            true
        }
        Err(e) => {
            app.open_overlay(Overlay::Error(
                ErrorDialog::new(
                    "Serial Write Failed",
                    "Failed to transmit bytes to serial port",
                )
                .with_details(e),
            ));
            false
        }
    }
}

/// Copies `text` and says so: the copy of `what` with the text itself, else its char count
fn execute_clipboard_copy(app: &mut App, text: &str, what: Option<&str>) {
    if let Err(e) = copy_via_osc52(text) {
        app.notify_error(format!("Clipboard error: {e}"));
    } else if let Some(what) = what {
        app.notify_success(format!("Copied the {what}: {text}"));
    } else {
        let count = text.chars().count();
        app.notify_success(format!("Copied {count} chars to clipboard"));
    }
}
