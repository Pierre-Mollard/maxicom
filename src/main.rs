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
    app::{App, UpdateEffect, selection::copy_via_osc52, triggers::Trigger},
    args::{ParseOutcome, parse_args, print_help, print_version},
    configs::{
        AppConfig, ConfigsError, KeyResolver, KeywordHighlight, LinksConfig, RawSideView,
        SettingsConfig, ThemeConfig,
    },
    link::{Address, ByteSource, LinkConfig, LinkError},
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
mod paths;
mod terminal;
mod ui;

// 0 -- Success
// 1 -- General failure
// 2 -- Invalid argument
const EXIT_USAGE: u8 = 2;

/// Input events handled before a frame is drawn, so a flood can't stall rendering
const MAX_EVENTS_PER_FRAME: usize = 512;
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
    triggers: Vec<Trigger>,
    links: Result<LinksConfig, String>,
    warnings: Vec<String>,
    status_bar: CompiledStatusBar,
}

fn load_runtime_config(config_path: &Path) -> Result<RuntimeConfig, ConfigsError> {
    let configs = AppConfig::load_or_create_default(config_path)?;
    let key_resolver = KeyResolver::try_new(&configs.keybindings)
        .expect("keybindings were already validated during config loading");
    let status_bar = CompiledStatusBar::new(&configs.settings.ui.status_bar, &configs.theme);
    let triggers = (configs.triggers.compile())
        .expect("triggers were already validated during config loading");
    Ok(RuntimeConfig {
        highlight_rules: configs.highlights.compile_rules(),
        triggers,
        links: configs.links,
        warnings: configs.warnings,
        settings: configs.settings,
        theme: configs.theme,
        key_resolver,
        status_bar,
    })
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    // `--config` first: the other options need the settings it gives (MAX-206)
    if let Some(path) = args::config_file() {
        AppConfig::use_config_file(path);
    }
    let config_path = AppConfig::config_path();
    let RuntimeConfig {
        mut settings,
        mut theme,
        key_resolver,
        mut highlight_rules,
        triggers,
        links,
        warnings,
        mut status_bar,
    } = match load_runtime_config(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("maxicom: {err}");
            return Ok(ExitCode::FAILURE);
        }
    };

    // this machine's saved links, which a name given or `[link] device` may be (MAX-185)
    let host = configs::links::this_host();
    let saved: Vec<_> = (links.as_ref().ok())
        .map(|config| {
            (config.links.iter())
                .filter(|link| link.is_for(host.as_deref()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let (address, line, replay_delay, restore, capture_file) =
        match parse_args(&settings.link, &saved) {
            Ok(ParseOutcome::Run {
                address,
                line,
                replay_delay,
                restore,
                capture_file,
                display_hex,
                encoding,
            }) => {
                // minicom's `-H` and `-R`, over the settings until a reload (MAX-181)
                if display_hex {
                    settings.raw_side.view = RawSideView::Hex;
                }
                if let Some(encoding) = encoding {
                    settings.encoding = encoding;
                }
                (address, line, replay_delay, restore, capture_file)
            }
            Ok(ParseOutcome::Help) => {
                print_help();
                return Ok(ExitCode::SUCCESS);
            }
            Ok(ParseOutcome::Version) => {
                print_version();
                return Ok(ExitCode::SUCCESS);
            }
            // the folder to look into the backups out of the app: `cd $(maxicom --cache-dir)`
            Ok(ParseOutcome::CacheDir) => match AppConfig::default_sessions_dir() {
                Some(dir) => {
                    println!("{}", dir.display());
                    return Ok(ExitCode::SUCCESS);
                }
                None => {
                    eprintln!("maxicom: no cache folder on this system");
                    return Ok(ExitCode::FAILURE);
                }
            },
            // the settings just loaded, so a file that doesn't load has already stopped it
            Ok(ParseOutcome::WriteConfig) => match configs::update::update_files(&config_path) {
                Ok(report) => {
                    report.iter().for_each(|line| println!("{line}"));
                    return Ok(ExitCode::SUCCESS);
                }
                Err(err) => {
                    eprintln!("maxicom: {err}");
                    return Ok(ExitCode::FAILURE);
                }
            },
            Err(error) => {
                eprintln!("Argument error: {error}");
                return Ok(ExitCode::from(EXIT_USAGE));
            }
        };

    // no device named: the directory on the ports the system lists and the saved links, the
    // link waiting on the first port, else the first link, until one is picked; with none,
    // how to give one (MAX-181, MAX-185)
    let (link_config, pick_from) = match address {
        Some(address) => (LinkConfig { address, line }, None),
        None => {
            let ports = link::serial::port_names();
            let first_link = saved.first().map(|link| link.address.as_str());
            let first = ports.first().map(String::as_str).or(first_link);
            let Some(Ok(address)) = first.map(str::parse) else {
                eprintln!(
                    "maxicom: No device: give one (maxicom -D <device>), or set `device` in \
                     [link] of config.toml.\nNo serial port found."
                );
                return Ok(ExitCode::from(EXIT_USAGE));
            };
            (LinkConfig { address, line }, Some(ports))
        }
    };

    let device: Option<ByteSource> = match &link_config.address {
        // nothing opens before the user picks a port (MAX-181)
        _ if pick_from.is_some() => None,
        // connected in the loop, like a reconnect, so the UI is up and says it's connecting.
        // The arguments refused a malformed address
        Address::Tcp(_) => None,
        // opened by the app below, as the `replay` command does (MAX-168)
        Address::Replay(_) => None,
        Address::Serial(_) => match link::open(&link_config) {
            Ok(dev) => Some(dev),
            Err(LinkError::OpenFailed { ref source, .. })
                if source.kind() == ErrorKind::NotFound =>
            {
                None
            }
            Err(LinkError::InputClosed) => None,
            Err(err) => {
                eprintln!("maxicom: could not open the device: {err}");
                return Ok(ExitCode::FAILURE);
            }
        },
    };

    let mut running: bool = true;
    let mut input_buffer = vec![0u8; settings.rx_buffer_size];

    let mut app = App::new(settings, key_resolver, link_config);
    app.set_triggers(triggers);
    app.set_links(links);
    warnings.into_iter().for_each(|w| app.notify_warn(w));
    app.set_restore_flag(restore);
    if let Some(ports) = pick_from {
        app.start_in_directory(ports, AppConfig::default_sessions_dir());
    } else if app.link_kind().keeps_sessions() {
        // before the new session backup, whose pruning could remove the last session (MAX-148)
        let sessions_dir = AppConfig::default_sessions_dir();
        app.restore_last_session(sessions_dir.as_deref(), restore);
        app.start_session(sessions_dir);
    }

    // before the UI, so a file that doesn't open is said on the shell
    let picking = app.is_device_unpicked();
    if let (false, Address::Replay(path)) = (picking, &app.link_config().address) {
        let path = path.clone();
        if let Err(err) = app.open_replay(path.clone(), replay_delay, None) {
            eprintln!(
                "maxicom: could not open replay file {}: {err}",
                path.display()
            );
            return Ok(ExitCode::FAILURE);
        }
    } else if !picking {
        app.start_link(device);
    }
    if let Some(file) = capture_file {
        app.start_text_capture(&file, false);
    }

    let mut guard = TerminalGuard::init()?;
    // set from `ui.mouse` in the loop, so its toggle and a reload apply it too (MAX-113)
    let mut mouse_captured = false;

    let mut last_layout_warnings: Vec<LayoutWarning> = Vec::default();
    let mut needs_redraw = true;
    let mut last_draw: Option<Instant> = None;

    while running {
        let (rx_received, link_redraw) = match app.poll_link(&mut input_buffer) {
            Ok(turn) => (turn.rx_received, turn.redraw),
            Err(e) => {
                eprintln!("Serial read error: {e:?}");
                break;
            }
        };
        needs_redraw |= link_redraw;

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
        while running && handled_events < MAX_EVENTS_PER_FRAME {
            let action = match inputs::poll_event(poll_timeout) {
                Ok(Some(event)) => {
                    let action = inputs::handle_event(event, &app);
                    // the mouse capture reports every move: one that does nothing draws nothing
                    needs_redraw |= !matches!(action, inputs::Action::Ignore);
                    action
                }
                // no event: a command line chain paused by a `sleep` or a `break` goes on
                // once its time is up (MAX-12)
                _ => match app.next_command_action(Instant::now()) {
                    Some(action) => {
                        needs_redraw = true;
                        action
                    }
                    None => break,
                },
            };
            poll_timeout = Duration::ZERO;
            handled_events += 1;
            // the action, then the queued commands of a command line chain one at a time, each
            // effect applied before the next is built (MAX-12)
            let mut next = Some(action);
            while let Some(action) = next.take() {
                match app.dispatch(action) {
                    UpdateEffect::Quit => running = false,
                    UpdateEffect::CopyToClipboard(text) => {
                        execute_clipboard_copy(&mut app, &text, None);
                    }
                    UpdateEffect::CopyToClipboardAs { text, what } => {
                        execute_clipboard_copy(&mut app, &text, Some(&what));
                    }
                    UpdateEffect::ReloadConfig => match load_runtime_config(&config_path) {
                        Ok(config) => {
                            input_buffer.resize(config.settings.rx_buffer_size, 0);
                            theme = config.theme;
                            highlight_rules = config.highlight_rules;
                            app.set_triggers(config.triggers);
                            app.set_links(config.links);
                            status_bar = config.status_bar;
                            app.apply_reload(config.settings, config.key_resolver);
                            config.warnings.into_iter().for_each(|w| app.notify_warn(w));
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
                    _ => {}
                }
                if running {
                    next = app.next_command_action(Instant::now());
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
