/// The actions of the commands with arguments (MAX-12), built from the arguments their spec
/// checked, or refused with the reason when the link can't do them
use std::{
    fs::File,
    io::BufReader,
    time::{Duration, Instant},
};

use crate::{
    app::{App, mash::Mash, transfer::FileTransfer},
    commands::ArgValue,
    configs::{Mode, TextEncoding, links::SavedLink},
    inputs::{Action, AppAction, SearchModeAction},
    link::{
        Address, LinkConfig, LinkKind,
        replay::ReplayControl,
        serial::{FlowControl, Framing, LineControl, LineSettings},
    },
    overlays::SettingKey,
    paths::expand_home,
};

/// `send` and `send-hex`: their bytes, to the link
pub fn send(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Bytes(bytes)] = args else {
        unreachable!("send takes its bytes");
    };
    if bytes.is_empty() {
        return Err("nothing to send".to_string());
    }
    if !app.link_kind().writes() {
        return Err("a replay has no link to send to".to_string());
    }
    link_open(app)?;
    Ok(Action::TransmitRaw(bytes.clone()))
}

/// `send-file <path> [line-delay-ms]`: starts sending the file, paced; the chain goes on at
/// once, without waiting for its end
pub fn send_file(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let (path, delay_ms) = match args {
        [ArgValue::Path(path)] => (path, 0),
        [ArgValue::Path(path), ArgValue::Int(ms)] => (path, *ms as u64),
        _ => unreachable!("send-file takes a path and a delay"),
    };
    if !app.link_kind().writes() {
        return Err("a replay has no link to send to".to_string());
    }
    link_open(app)?;
    if app.transfer().is_some() {
        return Err("a file is being sent already (cancel stops it)".to_string());
    }
    let path = expand_home(path);
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if metadata.is_dir() {
        return Err(format!("{} is a folder", path.display()));
    }
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let transfer = FileTransfer::new(
        Box::new(BufReader::new(file)),
        name,
        metadata.len(),
        app.link_config().line.bytes_per_sec(),
        Duration::from_millis(delay_ms),
        Instant::now(),
    );
    Ok(Action::App(AppAction::StartTransfer(Box::new(transfer))))
}

/// None when the link is a serial port, else why it has no `what` (line settings, control
/// lines): a replay, a TCP link whose bridge owns them
fn serial_only(app: &App, what: &str) -> Result<(), String> {
    match app.link_kind() {
        LinkKind::Serial => Ok(()),
        LinkKind::Tcp => Err(format!("a TCP link has no {what}, its bridge sets them")),
        LinkKind::Replay => Err(format!("a replay has no {what}")),
    }
}

/// The port's line settings with `change`, for the link to take
fn reconfigure(app: &App, change: impl FnOnce(&mut LineSettings)) -> Result<Action, String> {
    serial_only(app, "line settings")?;
    let mut line = app.link_config().line;
    change(&mut line);
    Ok(Action::App(AppAction::Reconfigure(line)))
}

/// `baud <rate>`
pub fn baud(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Int(rate)] = args else {
        unreachable!("baud takes a rate");
    };
    reconfigure(app, |line| line.baud = *rate as u32)
}

/// `framing <8N1>`
pub fn framing(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::String(text)] = args else {
        unreachable!("framing takes a framing");
    };
    let framing: Framing = text.parse()?;
    reconfigure(app, |line| line.framing = framing)
}

/// `flow none|hw|sw`
pub fn flow(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let flow_control = match args {
        [ArgValue::Enum("hw")] => FlowControl::RtsCts,
        [ArgValue::Enum("sw")] => FlowControl::XonXoff,
        [ArgValue::Enum("none")] => FlowControl::None,
        _ => unreachable!("flow takes none, hw or sw"),
    };
    reconfigure(app, |line| line.flow_control = flow_control)
}

fn control(app: &App, control: LineControl) -> Result<Action, String> {
    serial_only(app, "control lines")?;
    link_open(app)?;
    Ok(Action::App(AppAction::LineControl(control)))
}

/// `dtr on|off`
pub fn dtr(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Bool(on)] = args else {
        unreachable!("dtr takes on or off");
    };
    control(app, LineControl::Dtr(*on))
}

/// `rts on|off`, refused with hardware flow control: the driver drives RTS then
pub fn rts(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Bool(on)] = args else {
        unreachable!("rts takes on or off");
    };
    if app.link_config().line.flow_control == FlowControl::RtsCts {
        return Err("hardware flow control drives RTS (flow none first)".to_string());
    }
    control(app, LineControl::Rts(*on))
}

/// `break [ms]`: the line held in break that long, 250 ms by default; the rest of the line
/// waits for its end
pub fn line_break(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let ms = match args {
        [] => 250,
        [ArgValue::Int(ms)] => *ms as u64,
        _ => unreachable!("break takes a duration"),
    };
    control(app, LineControl::Break(Duration::from_millis(ms)))
}

/// `sleep <ms>`: the commands pause, the rest of the line waits that long; the screen doesn't
pub fn pause_commands(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Int(ms)] = args else {
        unreachable!("sleep takes a duration");
    };
    Ok(Action::App(AppAction::PauseCommands(
        Duration::from_millis(*ms as u64),
    )))
}

/// None when the link is open, else that it isn't: detached, lost, still opening
fn link_open(app: &App) -> Result<(), String> {
    if app.is_link_open() {
        Ok(())
    } else {
        Err("the port isn't open (detached or reconnecting)".to_string())
    }
}

/// `open <name | port | tcp://host:port | file://path>`: the link switches to it; the rest of
/// the line waits for it to be open. A saved link's name first (MAX-185), which opens with its
/// line settings; a port keeps the ones in use. A `file://` is a replay, without delay
pub fn open(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Address(address)] = args else {
        unreachable!("open takes an address");
    };
    let saved = match address {
        Address::Serial(name) => app.saved_link(name).and_then(|link| app.resolve(link)),
        _ => None,
    };
    let (address, line) = match saved {
        Some(config) => (config.address, Some(config.line)),
        None => (address.clone(), None),
    };
    match address {
        Address::Replay(path) => Ok(Action::App(AppAction::Replay {
            path: expand_home(&path),
            delay: Duration::ZERO,
        })),
        _ if app.link_kind() == LinkKind::Replay => {
            Err("a replay can't open a link (replay-stop ends it)".to_string())
        }
        // a TCP link has no line: its bridge sets it
        address
            if let Some(line) = line
                && address.kind() != LinkKind::Tcp =>
        {
            Ok(Action::App(AppAction::OpenLink(LinkConfig {
                address,
                line,
            })))
        }
        address => Ok(Action::App(AppAction::Open(address))),
    }
}

/// `link-save [name]` (MAX-185): the link in use saved to `links.toml`, under `name` or its
/// address's; refused as the directory's `s` is
pub fn link_save(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let link = app.link_to_save(None)?;
    let link = match args {
        [ArgValue::String(name)] => SavedLink {
            name: name.clone(),
            ..link
        },
        _ => link,
    };
    Ok(Action::App(AppAction::SaveLink(link)))
}

/// `link-delete <name>` (MAX-185): this machine's saved link of that name taken out of
/// `links.toml`
pub fn link_delete(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::String(name)] = args else {
        unreachable!("link-delete takes a name");
    };
    let link = (app.saved_link(name)).ok_or_else(|| format!("no saved link '{name}'"))?;
    Ok(Action::App(AppAction::DeleteLink(link.clone())))
}

/// `replay <path> [delay-ms]`: the file's bytes in place of the link's, a buffer every delay
/// (MAX-166)
pub fn replay(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let (path, delay_ms) = match args {
        [ArgValue::Path(path)] => (path, 0),
        [ArgValue::Path(path), ArgValue::Int(ms)] => (path, *ms as u64),
        _ => unreachable!("replay takes a path and a delay"),
    };
    Ok(Action::App(AppAction::Replay {
        path: expand_home(path),
        delay: Duration::from_millis(delay_ms),
    }))
}

/// `replay-stop`: the replay ends, back to the link it replaced if any (MAX-166)
pub fn replay_stop(app: &App, _args: &[ArgValue]) -> Result<Action, String> {
    if app.link_kind() != LinkKind::Replay {
        return Err("no replay runs".to_string());
    }
    Ok(Action::App(AppAction::StopReplay))
}

/// `replay-pause`, `replay-resume`, `replay-restart` and `replay-speed`: refused without a
/// replay reading, and when they would change nothing (MAX-166)
fn replay_control(app: &App, control: ReplayControl) -> Result<Action, String> {
    let Some(replay) = app.replay() else {
        return Err(if app.link_kind() == LinkKind::Replay {
            "the replay was stopped".to_string()
        } else {
            "no replay runs".to_string()
        });
    };
    match control {
        ReplayControl::Pause if replay.is_paused() => {
            Err("the replay is paused already".to_string())
        }
        ReplayControl::Pause if app.is_replay_ended() => {
            Err("the replay is finished (replay-restart plays it again)".to_string())
        }
        ReplayControl::Resume if !replay.is_paused() => Err("the replay isn't paused".to_string()),
        _ => Ok(Action::App(AppAction::ControlReplay(control))),
    }
}

pub fn replay_pause(app: &App, _args: &[ArgValue]) -> Result<Action, String> {
    replay_control(app, ReplayControl::Pause)
}

pub fn replay_resume(app: &App, _args: &[ArgValue]) -> Result<Action, String> {
    replay_control(app, ReplayControl::Resume)
}

pub fn replay_restart(app: &App, _args: &[ArgValue]) -> Result<Action, String> {
    replay_control(app, ReplayControl::Restart)
}

/// `replay-speed <delay-ms>`: the wait between two buffers from now on
pub fn replay_speed(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Int(ms)] = args else {
        unreachable!("replay-speed takes a delay");
    };
    let delay = Duration::from_millis(*ms as u64);
    replay_control(app, ReplayControl::Delay(delay))
}

/// The settings `set` turns on or off, by name
pub const ON_OFF_SETTINGS: &[&str] = &[
    "local-echo",
    "timestamps",
    "mouse",
    "answer-queries",
    "add-cr",
    "add-lf",
];

/// `set <setting> on|off`: nothing when it is so already
pub fn set(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Enum(name), ArgValue::Bool(on)] = args else {
        unreachable!("set takes a setting and on or off");
    };
    let key = match *name {
        "local-echo" => SettingKey::LocalEcho,
        "timestamps" => SettingKey::DisplayTimestamp,
        "mouse" => SettingKey::Mouse,
        "answer-queries" => SettingKey::AnswerQueries,
        "add-cr" => SettingKey::AddCr,
        "add-lf" => SettingKey::AddLf,
        _ => unreachable!("set takes one of ON_OFF_SETTINGS"),
    };
    let is_on = app.setting_is_on(key).expect("an on/off setting");
    Ok(if is_on == *on {
        Action::Ignore
    } else {
        Action::App(AppAction::ToggleSetting(key))
    })
}

/// `encoding utf8|ascii|cp437|latin1`: how the received bytes are read as text
pub fn encoding(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let encoding = match args {
        [ArgValue::Enum("utf8")] => TextEncoding::Utf8,
        [ArgValue::Enum("ascii")] => TextEncoding::Ascii,
        [ArgValue::Enum("cp437")] => TextEncoding::Cp437,
        [ArgValue::Enum("latin1")] => TextEncoding::Latin1,
        _ => unreachable!("encoding takes utf8, ascii, cp437 or latin1"),
    };
    Ok(Action::App(AppAction::SetEncoding(encoding)))
}

/// `search <text>`: the search bar open on it, its matches shown
pub fn search(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::String(text)] = args else {
        unreachable!("search takes its text");
    };
    Ok(Action::Search(SearchModeAction::Query(text.clone())))
}

/// `mash <text> <until> [every-ms] [timeout-ms]` (MAX-6): sends the text at once, then every
/// `every-ms` (50 by default), until `until` comes in or `timeout-ms` (15 s) is up; the rest
/// of the line waits for `until`
pub fn mash(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let (text, until, every, timeout) = match args {
        [ArgValue::Bytes(text), ArgValue::Bytes(until), rest @ ..] => {
            let ms = |i: usize, default| match rest.get(i) {
                Some(ArgValue::Int(ms)) => Duration::from_millis(*ms as u64),
                _ => Duration::from_millis(default),
            };
            (text, until, ms(0, 50), ms(1, 15_000))
        }
        _ => unreachable!("mash takes a text, a text to wait for, then times"),
    };
    if text.is_empty() {
        return Err("nothing to send".to_string());
    }
    if !app.link_kind().writes() {
        return Err("a replay has no link to send to".to_string());
    }
    link_open(app)?;
    if app.mash().is_some() {
        return Err("a mash runs already (cancel stops it)".to_string());
    }
    let mash = Mash::new(text.clone(), until, every, timeout, Instant::now())
        .ok_or_else(|| "nothing to wait for".to_string())?;
    Ok(Action::App(AppAction::StartMash(Box::new(mash))))
}

/// `trigger on|off|toggle [name]` (MAX-6): all the triggers, or the one named. Toggled, all
/// of them are armed when none is, else disarmed
pub fn trigger(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let (state, name) = match args {
        [ArgValue::Enum(state)] => (*state, None),
        [ArgValue::Enum(state), ArgValue::String(name)] => (*state, Some(name.clone())),
        _ => unreachable!("trigger takes on, off or toggle, then a name"),
    };
    let triggers = app.triggers();
    if triggers.is_empty() {
        return Err("triggers.toml has no trigger".to_string());
    }
    let named =
        (triggers.iter()).filter(|trigger| name.as_ref().is_none_or(|n| *n == trigger.name));
    let on = match state {
        "on" => true,
        "off" => false,
        _ => !named.clone().any(|trigger| trigger.is_armed()),
    };
    match &name {
        Some(name) if named.count() == 0 => Err(format!("no trigger named '{name}'")),
        _ => Ok(Action::App(AppAction::ArmTriggers { on, name })),
    }
}

/// `screenshot <name>`: the terminal saved at once, without the dialog
pub fn screenshot(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Path(name)] = args else {
        unreachable!("screenshot takes a name");
    };
    if app.settings().mode != Mode::Terminal {
        return Err("screenshots are in terminal mode".to_string());
    }
    Ok(Action::App(AppAction::SaveScreenshot(name.clone())))
}
