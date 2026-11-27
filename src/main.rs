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
    app::{
        App, UpdateEffect,
        selection::{copy_base64_via_osc52, copy_via_osc52},
        triggers::Trigger,
        types::SidePanel,
    },
    args::{ParseOutcome, parse_args, print_help, print_version},
    commands::{
        jobs::Caller,
        report::{Note, Report},
    },
    configs::{
        AppConfig, ConfigsError, KeyResolver, KeywordHighlight, LinksConfig, PlotsConfig,
        RawSideView, SettingsConfig, ThemeConfig, profiles::with_profiles,
    },
    event::Event,
    link::{Address, ByteSource, LinkConfig, LinkError},
    notification::ToastLevel,
    overlays::{ErrorDialog, Overlay},
    ui::{PostRenderEffect, layout::LayoutWarning, render, status_bar::CompiledStatusBar},
};

mod api;
mod app;
mod args;
mod capture;
mod cli;
mod clock;
mod commands;
mod configs;
mod event;
mod glyphs;
mod inputs;
mod link;
mod notification;
mod overlays;
mod paths;
mod raw_side;
mod rx_stream;
mod terminal;
mod transfer;
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
    plots: PlotsConfig,
    warnings: Vec<String>,
    status_bar: CompiledStatusBar,
}

/// The config files, kept to lay another stack over them, and the runtime config with the
/// link's `profiles` laid over them (MAX-240)
fn load_runtime_config(
    config_path: &Path,
    profiles: &[String],
) -> Result<(AppConfig, RuntimeConfig), ConfigsError> {
    let global = AppConfig::load_or_create_default(config_path)?;
    let laid = with_profiles(&global, &AppConfig::profiles_dir(), profiles)
        .map_err(ConfigsError::Profile)?;
    Ok((global, RuntimeConfig::new(laid)))
}

impl RuntimeConfig {
    fn new(configs: AppConfig) -> Self {
        let key_resolver = KeyResolver::try_new(&configs.keybindings)
            .expect("keybindings were already validated during config loading");
        let status_bar = CompiledStatusBar::new(&configs.settings.ui.status_bar, &configs.theme);
        let triggers = (configs.triggers.compile())
            .expect("triggers were already validated during config loading");
        Self {
            highlight_rules: configs.highlights.compile_rules(),
            triggers,
            links: configs.links,
            plots: configs.plots,
            warnings: configs.warnings,
            settings: configs.settings,
            theme: configs.theme,
            key_resolver,
            status_bar,
        }
    }
}

/// `--reset-config` (MAX-231): the files it resets, a confirmation, then what it did
fn reset_config(config_path: &std::path::Path) -> ExitCode {
    let files = configs::update::reset_targets(config_path);
    println!("These files go back to the defaults, each one kept as .bak first:");
    for file in &files {
        println!("  {}", file.display());
    }
    println!("links.toml, your saved links, and your own profiles are left alone.");
    print!("Reset these files to the defaults? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    // a piped answer has no Enter typed after the question
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        println!();
    }
    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        println!("Nothing reset.");
        return ExitCode::SUCCESS;
    }
    match configs::update::reset_files(&files) {
        Ok(report) => {
            report.iter().for_each(|line| println!("{line}"));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("maxicom: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    // the client of a running maxicom, which needs no settings (MAX-285)
    if let Some(args) = args::cli() {
        return Ok(cli::main(args));
    }

    // `--config` first: the other options need the settings it gives (MAX-206)
    if let Some(path) = args::config_file() {
        AppConfig::use_config_file(path);
    }
    let config_path = AppConfig::config_path();
    // before the settings load: a file that doesn't load mustn't stop it (MAX-231)
    if args::reset_config() {
        return Ok(reset_config(&config_path));
    }
    let mut global = match AppConfig::load_or_create_default(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("maxicom: {err}");
            return Ok(ExitCode::FAILURE);
        }
    };

    // this machine's saved links, which a name given or `[link] device` may be (MAX-185)
    let host = configs::links::this_host();
    let saved: Vec<_> = (global.links.as_ref().ok())
        .map(|config| {
            (config.links.iter())
                .filter(|link| link.is_for(host.as_deref()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let outcome = parse_args(&global.settings.link, &saved);

    // the link's profiles laid over the config, kept by the app (MAX-240)
    let profiles = match &outcome {
        Ok(ParseOutcome::Run { profiles, .. }) => profiles.clone(),
        _ => Vec::new(),
    };
    let laid = match with_profiles(&global, &AppConfig::profiles_dir(), &profiles) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("maxicom: {err}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let RuntimeConfig {
        mut settings,
        mut theme,
        key_resolver,
        mut highlight_rules,
        triggers,
        links,
        plots,
        warnings,
        mut status_bar,
    } = RuntimeConfig::new(laid);

    let (address, line, replay_delay, restore, capture_file) = match outcome {
        Ok(ParseOutcome::Run {
            address,
            line,
            replay_delay,
            restore,
            capture_file,
            display_hex,
            encoding,
            plot,
            profiles: _,
        }) => {
            // minicom's `-H` and `-R`, over the settings until a reload (MAX-181)
            if display_hex {
                settings.raw_side.view = RawSideView::Hex;
            }
            if let Some(encoding) = encoding {
                settings.encoding = encoding;
            }
            if plot.is_some() {
                settings.plot = plot;
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
        Address::Tcp(..) => None,
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
    app.set_profiles(profiles);
    app.set_triggers(triggers);
    app.set_links(links);
    app.set_plots(plots);
    app.show_start_plot();
    for text in warnings {
        app.emit(Event::ConfigWarning { text });
    }
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
        // `--profile`'s over the session's own (MAX-240)
        let extra = app.profiles().to_vec();
        match app.open_replay(path.clone(), replay_delay, None, &extra) {
            Ok(notes) => notes.into_iter().for_each(|note| app.show_note(note)),
            Err(err) => {
                eprintln!(
                    "maxicom: could not open replay file {}: {err}",
                    path.display()
                );
                return Ok(ExitCode::FAILURE);
            }
        }
    } else if !picking {
        app.start_link(device);
    }
    // a failure has its popup already
    if let Some(file) = capture_file {
        let started = app.start_text_capture(&file, false);
        started
            .notes
            .into_iter()
            .for_each(|note| app.show_note(note));
    }

    let mut guard = TerminalGuard::init()?;
    // set from `ui.mouse` in the loop, so its toggle and a reload apply it too (MAX-113)
    let mut mouse_captured = false;

    // the local API (MAX-74), as `[api] enabled` says, checked again after each reload
    let instance = api::Instance::now();
    let mut api_server: Option<api::Server> = None;
    sync_api(&mut api_server, &instance, &mut app);

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
        if let Some(copy) = app.take_device_copy() {
            execute_device_copy(&mut app, &copy);
            needs_redraw = true;
        }

        // a link opened at runtime with another stack: laid over the config in use, its
        // files not read again (MAX-240)
        if let Some(profiles) = app.take_profiles_due() {
            // its failure has its dialog: no command waits for it
            let _ = lay_profiles(&mut app, &global, &mut highlight_rules, profiles);
            needs_redraw = true;
        }
        // once the link's stack is laid, so they're that stack's (MAX-9, MAX-247)
        app.run_profile_hooks();

        // the input poll is the loop's only idle wait: none while RX flows, otherwise until the
        // next frame is due (if one is pending) or IDLE_WAIT. Then drain what is already pending
        // (key bursts, pastes) before drawing, instead of one event per frame
        let since_draw = last_draw.map_or(Duration::MAX, |t| t.elapsed());
        // a group replay loading, or line heights being counted, draw a frame per slice, no
        // wait between (MAX-272)
        let busy = app.group_load().is_some() || app.busy().is_some();
        let mut poll_timeout = if rx_received || busy {
            Duration::ZERO
        } else if needs_redraw {
            FRAME_INTERVAL.saturating_sub(since_draw).min(IDLE_WAIT)
        } else {
            IDLE_WAIT
        };
        let mut handled_events = 0;
        while running && handled_events < MAX_EVENTS_PER_FRAME {
            let next_action = match inputs::poll_event(poll_timeout) {
                Ok(Some(event)) => {
                    let action = inputs::handle_event(event, &app);
                    // the mouse capture reports every move: one that does nothing draws nothing
                    needs_redraw |= !matches!(action, inputs::Action::Ignore);
                    (Caller::input(), action)
                }
                // no event: a chain paused by a `sleep` or a `break` goes on once its time is
                // up (MAX-12)
                _ => match app.next_command_action(Instant::now()) {
                    Some(next_action) => {
                        needs_redraw = true;
                        next_action
                    }
                    None => break,
                },
            };
            poll_timeout = Duration::ZERO;
            handled_events += 1;
            // the action, then the queued commands of a chain one at a time, each effect
            // applied before the next is built (MAX-12)
            let mut next = Some(next_action);
            while let Some((caller, action)) = next.take() {
                match app.dispatch_from(&caller, action) {
                    UpdateEffect::Quit => running = false,
                    // what an effect gives is reported to who started its action (MAX-165)
                    UpdateEffect::CopyToClipboard(text) => {
                        let copied = execute_clipboard_copy(&app, &text, None);
                        app.present(&caller, copied);
                    }
                    UpdateEffect::CopyToClipboardAs { text, what } => {
                        let copied = execute_clipboard_copy(&app, &text, Some(&what));
                        app.present(&caller, copied);
                    }
                    // `profile load` / `unload`, at once: the next command of the chain sees it
                    UpdateEffect::LayProfiles(profiles) => {
                        let laid = lay_profiles(&mut app, &global, &mut highlight_rules, profiles);
                        let report = match laid {
                            Ok(()) => Report::from(app.profiles_note()),
                            Err(message) => Report::shown(message),
                        };
                        app.present(&caller, report);
                    }
                    // ratatui forgets what it drew: the next frame writes every cell (MAX-271)
                    UpdateEffect::Redraw => guard.terminal.clear()?,
                    // the clients dropped, the server still listening (MAX-287)
                    UpdateEffect::DisconnectClients => {
                        let count = api_server.as_mut().map_or(0, api::Server::disconnect_all);
                        let note = match count {
                            1 => "Disconnected 1 API client".to_string(),
                            n => format!("Disconnected {n} API clients"),
                        };
                        app.present(&caller, Report::from(vec![Note::info(note)]));
                    }
                    UpdateEffect::ReloadConfig => {
                        match load_runtime_config(&config_path, app.profiles()) {
                            Ok((files, config)) => {
                                global = files;
                                input_buffer.resize(config.settings.rx_buffer_size, 0);
                                theme = config.theme;
                                highlight_rules = config.highlight_rules;
                                app.set_triggers(config.triggers);
                                app.set_links(config.links);
                                app.set_plots(config.plots);
                                status_bar = config.status_bar;
                                let reloaded =
                                    app.apply_reload(config.settings, config.key_resolver);
                                app.present(&caller, Report::from(reloaded));
                                sync_api(&mut api_server, &instance, &mut app);
                                app.warn_shadowed_keys();
                                for text in config.warnings {
                                    app.emit(Event::ConfigWarning { text });
                                }
                            }
                            // the running config stays
                            Err(err) => {
                                let message = format!("The config files were not applied: {err}");
                                app.open_overlay(Overlay::Error(
                                    ErrorDialog::new(
                                        "Config Reload Failed",
                                        "The config files were not applied",
                                    )
                                    .with_details(err),
                                ));
                                let failed = Report::shown(message);
                                app.present(&caller, failed);
                            }
                        }
                    }
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

        // the API's requests, answered with the app in hand (MAX-74), and its subscribers told
        // what happened (MAX-284)
        if let Some(server) = &mut api_server {
            let _ = server.serve(&mut app, &instance);
        }

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
                let copied = execute_clipboard_copy(&app, &text, None);
                app.present(&Caller::input(), copied);
            }
            PostRenderEffect::LayoutWarnings(current_warnings) => {
                for warn in &current_warnings {
                    if !last_layout_warnings.contains(warn) {
                        let msg = match warn {
                            LayoutWarning::SidePanelTooSmall {
                                panel,
                                required,
                                actual,
                                orientation,
                            } => {
                                let name = match panel {
                                    SidePanel::Raw => "Raw side panel",
                                    SidePanel::Plot => "Plot",
                                };
                                format!(
                                    "{} hidden: requires at least {} {:?}, but screen has only {}",
                                    name, required, orientation, actual
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
                        app.emit(Event::ScreenWarning { text: msg });
                        needs_redraw = true;
                    }
                }
                last_layout_warnings = current_warnings;
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// `profiles` laid over the config files `global`, which aren't read again (MAX-240): the
/// parts a profile changes given to the app, the highlight rules here, and the profiles'
/// warnings said. When one doesn't load, its error in a dialog and the stack in use stays,
/// the error given back too
fn lay_profiles(
    app: &mut App,
    global: &AppConfig,
    highlight_rules: &mut Vec<KeywordHighlight>,
    profiles: Vec<String>,
) -> Result<(), String> {
    match with_profiles(global, &AppConfig::profiles_dir(), &profiles) {
        Ok(laid) => {
            let known = global.warnings.len();
            let config = RuntimeConfig::new(laid);
            *highlight_rules = config.highlight_rules;
            let (keys, triggers) = (config.key_resolver, config.triggers);
            app.apply_profiles(profiles, keys, triggers, config.plots);
            for text in config.warnings.into_iter().skip(known) {
                app.emit(Event::ConfigWarning { text });
            }
            Ok(())
        }
        Err(err) => {
            let message = format!("A profile didn't load: {err}");
            app.open_overlay(Overlay::Error(
                ErrorDialog::new("Profiles Not Applied", "A profile didn't load").with_details(err),
            ));
            Err(message)
        }
    }
}

/// The device's OSC 52 copy sent on as it came, base64 checked by vt100 (MAX-275)
fn execute_device_copy(app: &mut App, base64: &[u8]) {
    let sent = std::str::from_utf8(base64)
        .map_err(std::io::Error::other)
        .and_then(copy_base64_via_osc52);
    let copied = match sent {
        Err(e) => Event::DeviceCopyFailed {
            error: e.to_string(),
        },
        Ok(()) => Event::DeviceCopied {
            base64_len: base64.len(),
        },
    };
    app.emit(copied);
}

/// Copies `text`, with what to say: the copy of `what` with the text itself, else its char
/// count, copies one after the other in one box (MAX-234); a clipboard error in that box too
fn execute_clipboard_copy(app: &App, text: &str, what: Option<&str>) -> Report {
    match copy_via_osc52(text) {
        Err(e) => {
            let error = format!("Clipboard error: {e}");
            Report::from(Note::new(ToastLevel::Error, error).in_slot("copy"))
        }
        Ok(()) => Report::from(Vec::from_iter(app.copied_note(text, what))),
    }
}

/// Starts or stops the API server as `[api] enabled` says (MAX-74): at start and after a
/// reload, so a failure is said once, not retried on every pass
fn sync_api(server: &mut Option<api::Server>, instance: &api::Instance, app: &mut App) {
    match (app.settings().api.enabled, server.is_some()) {
        (true, false) => {
            let dir = api::socket_dir();
            match api::Server::start(dir.as_deref(), &instance.name()) {
                Ok(started) => {
                    let location = started.location().to_string();
                    *server = Some(started);
                    app.show_note(Note::info(format!("API listening on {location}")));
                }
                Err(err) => app.show_note(Note::warn(format!("API not started: {err}"))),
            }
        }
        (false, true) => {
            *server = None;
            app.show_note(Note::info("API stopped"));
        }
        (true, true) | (false, false) => {}
    }
}
