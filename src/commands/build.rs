/// The actions of the commands with arguments (MAX-12), built from the arguments their spec
/// checked, or refused with the reason when the link can't do them
use std::{
    fs::File,
    io::BufReader,
    time::{Duration, Instant},
};

use crate::{
    app::{App, transfer::FileTransfer},
    capture::expand_home,
    commands::ArgValue,
    configs::{Mode, TextEncoding},
    inputs::{Action, AppAction, SearchModeAction},
    link::{
        self,
        serial::{FlowControl, LineControl, SerialConfig, parse_framing},
    },
    overlays::SettingKey,
};

/// `send` and `send-hex`: their bytes, to the link
pub fn send(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Bytes(bytes)] = args else {
        unreachable!("send takes its bytes");
    };
    if bytes.is_empty() {
        return Err("nothing to send".to_string());
    }
    if link::is_replay(&app.serial_config.port) {
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
    if link::is_replay(&app.serial_config.port) {
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
        app.serial_config.bytes_per_sec(),
        Duration::from_millis(delay_ms),
        Instant::now(),
    );
    Ok(Action::App(AppAction::StartTransfer(Box::new(transfer))))
}

/// None when the link is a serial port, else why it has no `what` (line settings, control
/// lines): a replay, a TCP link whose bridge owns them
fn serial_only(app: &App, what: &str) -> Result<(), String> {
    let port = &app.serial_config.port;
    if link::is_replay(port) {
        Err(format!("a replay has no {what}"))
    } else if !link::has_line_settings(port) {
        Err(format!("a TCP link has no {what}, its bridge sets them"))
    } else {
        Ok(())
    }
}

/// The port's settings with `change`, for the loop to apply
fn reconfigure(app: &App, change: impl FnOnce(&mut SerialConfig)) -> Result<Action, String> {
    serial_only(app, "line settings")?;
    let mut config = app.serial_config.clone();
    change(&mut config);
    Ok(Action::App(AppAction::Reconfigure(config)))
}

/// `baud <rate>`
pub fn baud(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Int(rate)] = args else {
        unreachable!("baud takes a rate");
    };
    reconfigure(app, |config| config.baudrate = *rate as u32)
}

/// `framing <8N1>`
pub fn framing(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::String(text)] = args else {
        unreachable!("framing takes a framing");
    };
    let (char_size, parity, stop_bits) = parse_framing(text)?;
    reconfigure(app, |config| {
        config.char_size = char_size;
        config.parity = parity;
        config.stop_bits = stop_bits;
    })
}

/// `flow none|hw|sw`
pub fn flow(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let flow_control = match args {
        [ArgValue::Enum("hw")] => FlowControl::RtsCts,
        [ArgValue::Enum("sw")] => FlowControl::XonXoff,
        [ArgValue::Enum("none")] => FlowControl::None,
        _ => unreachable!("flow takes none, hw or sw"),
    };
    reconfigure(app, |config| config.flow_control = flow_control)
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
    if app.serial_config.flow_control == FlowControl::RtsCts {
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

/// `open <port | tcp://host:port>`: the link switches to it, its settings kept; the rest of
/// the line waits for it to be open
pub fn open(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Path(port)] = args else {
        unreachable!("open takes a port");
    };
    if link::is_replay(&app.serial_config.port) {
        return Err("a replay can't open a link".to_string());
    }
    let port = port.to_string_lossy().into_owned();
    if link::is_replay(&port) {
        return Err("a replay opens from the command line: maxicom --replay".to_string());
    }
    Ok(Action::App(AppAction::Open(port)))
}

/// The settings `set` turns on or off, by name
pub const ON_OFF_SETTINGS: &[&str] = &["local-echo", "timestamps", "mouse", "answer-queries"];

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
