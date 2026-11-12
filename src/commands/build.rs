/// The actions of the commands with arguments (MAX-12), built from the arguments their spec
/// checked, or refused with the reason when the link can't do them
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};

use crate::{
    app::{App, mash::Mash},
    commands::ArgValue,
    configs::{Mode, TextEncoding, links::SavedLink},
    inputs::{Action, AppAction, MarkAction, PendingMark, SearchModeAction},
    link::{
        Address, LinkConfig, LinkKind,
        replay::ReplayControl,
        serial::{FlowControl, Framing, LineControl, LineSettings},
    },
    overlays::SettingKey,
    paths::expand_home,
    transfer::{FileTransfer, modem::Protocol},
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
    if app.link_busy() {
        return Err(TRANSFER_RUNNING.to_string());
    }
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
    can_send_file(app)?;
    let (file, name, size) = open_file(path)?;
    let transfer = FileTransfer::raw(
        Box::new(BufReader::new(file)),
        name,
        size,
        app.link_config().line.bytes_per_sec(),
        Duration::from_millis(delay_ms),
        Instant::now(),
    );
    Ok(Action::App(AppAction::StartTransfer(Box::new(transfer))))
}

/// The protocols `upload` takes, as typed
pub const UPLOAD_PROTOCOLS: &[&str] = &["xmodem", "xmodem-1k", "ymodem"];

/// `upload <protocol> <path>` (MAX-162): sends the file with a protocol the receiver drives
/// (`loady`, `loadx`, `rx`, `rb`), started before or after; the chain goes on at once
pub fn upload(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let [ArgValue::Enum(protocol), ArgValue::Path(path)] = args else {
        unreachable!("upload takes a protocol and a path");
    };
    let protocol = match *protocol {
        "xmodem" => Protocol::Xmodem,
        "xmodem-1k" => Protocol::Xmodem1k,
        _ => Protocol::Ymodem,
    };
    can_send_file(app)?;
    // its bytes would break the packets
    if app.mash().is_some() {
        return Err("a mash runs (cancel stops it)".to_string());
    }
    let (file, name, size) = open_file(path)?;
    let mtime = (file
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok())
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
    let mut transfer = FileTransfer::modem(
        Box::new(BufReader::new(file)),
        protocol,
        name,
        size,
        app.link_config().line.bytes_per_sec(),
        Instant::now(),
    );
    if let Some(mtime) = mtime {
        transfer = transfer.with_mtime(mtime.as_secs());
    }
    Ok(Action::App(AppAction::StartTransfer(Box::new(transfer))))
}

/// The protocols `download` takes, as typed: a receiver takes XMODEM-1K's packets too
pub const DOWNLOAD_PROTOCOLS: &[&str] = &["xmodem", "ymodem"];

/// `download <protocol> [path]` (MAX-162): receives what the device sends with a protocol
/// (`sb`, `sx`), into `[transfers] dir`, or the folder given. XMODEM sends no name: its file
/// is the path given when it isn't a folder, else `xmodem_name`
pub fn download(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let (protocol, path) = match args {
        [ArgValue::Enum(protocol)] => (*protocol, None),
        [ArgValue::Enum(protocol), ArgValue::Path(path)] => (*protocol, Some(path)),
        _ => unreachable!("download takes a protocol and a path"),
    };
    let protocol = match protocol {
        "xmodem" => Protocol::Xmodem,
        _ => Protocol::Ymodem,
    };
    can_send_file(app)?;
    // its bytes would break the packets
    if app.mash().is_some() {
        return Err("a mash runs (cancel stops it)".to_string());
    }
    let transfers = &app.settings().transfers;
    let xmodem_name = || {
        chrono::Local::now()
            .format(&transfers.xmodem_name)
            .to_string()
    };
    let (folder, name) = match path.map(|path| expand_home(path)) {
        None => (expand_home(&transfers.dir), xmodem_name()),
        Some(path) if path.is_dir() || path.as_os_str().to_string_lossy().ends_with('/') => {
            (path, xmodem_name())
        }
        Some(path) if protocol == Protocol::Ymodem => {
            return Err(format!(
                "{} isn't a folder: YMODEM files come with their names",
                path.display()
            ));
        }
        Some(path) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
            (folder, name.unwrap_or_else(xmodem_name))
        }
    };
    // said as `.`, not as nothing
    let folder = if folder.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        folder
    };
    // YMODEM's files bring their names
    let name = (protocol == Protocol::Xmodem).then_some(name);
    let transfer = FileTransfer::download(protocol, folder, name, Instant::now());
    Ok(Action::App(AppAction::StartTransfer(Box::new(transfer))))
}

/// Why a file can't go through now: a replay, a link not open, a file going through already
fn can_send_file(app: &App) -> Result<(), String> {
    if !app.link_kind().writes() {
        return Err("a replay has no link to send to".to_string());
    }
    link_open(app)?;
    if app.transfer().is_some() {
        return Err("a file is being sent already (cancel stops it)".to_string());
    }
    Ok(())
}

/// The file to send, its name and its size; a folder is refused
fn open_file(path: &Path) -> Result<(File, String, u64), String> {
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
    Ok((file, name, metadata.len()))
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
/// Why `send` and `mash` refuse while a protocol transfer owns the link (MAX-162)
const TRANSFER_RUNNING: &str = "a file transfer is running (cancel stops it)";

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

/// `session-name [text]` (MAX-213): the running session named, no text taking the name away
pub fn session_name(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let name = match args {
        [ArgValue::String(name)] => name.clone(),
        _ => String::new(),
    };
    Ok(Action::App(AppAction::NameSession(name)))
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
    if app.replay_group().is_some() {
        return Err("a group replay is loaded at once: it has no controls".to_string());
    }
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
    // its bytes would break the packets once the receiver starts
    if app.transfer().is_some_and(FileTransfer::takes_rx) {
        return Err(TRANSFER_RUNNING.to_string());
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

/// `plot [name]` (MAX-222): the plot named, `off`, or alone the next one in the file's order,
/// off after the last
pub fn plot(app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let plots = app.plots();
    if plots.plots.is_empty() {
        return Err("plots.toml has no plot".to_string());
    }
    let name = match args {
        [] => app.next_plot().map(String::from),
        [ArgValue::String(name)] if name == "off" => None,
        [ArgValue::String(name)] if plots.get(name).is_some() => Some(name.clone()),
        [ArgValue::String(name)] => return Err(format!("no plot named '{name}'")),
        _ => unreachable!("plot takes a name, or none"),
    };
    Ok(Action::App(AppAction::ShowPlot(name)))
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

/// The letters a mark takes (MAX-164)
pub const MARK_LETTERS: &[&str] = &[
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

/// The letter of a mark command, none when left out
fn mark_letter(args: &[ArgValue]) -> Option<char> {
    match args {
        [] => None,
        [ArgValue::Enum(letter)] => letter.chars().next(),
        _ => unreachable!("a mark command takes a letter"),
    }
}

fn mark_action(action: MarkAction) -> Result<Action, String> {
    Ok(Action::App(AppAction::Mark(action)))
}

/// `mark [letter]`: the line a mark takes; without a letter, waits for it
pub fn mark(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    mark_action(match mark_letter(args) {
        Some(letter) => MarkAction::Add(Some(letter)),
        None => MarkAction::Await(PendingMark::Add),
    })
}

/// `jump-mark [letter]`: without a letter, waits for it
pub fn jump_mark(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    mark_action(match mark_letter(args) {
        Some(letter) => MarkAction::Jump(letter),
        None => MarkAction::Await(PendingMark::Jump),
    })
}

/// `delete-mark <letter>`
pub fn delete_mark(_app: &App, args: &[ArgValue]) -> Result<Action, String> {
    let letter = mark_letter(args).expect("delete-mark takes a letter");
    mark_action(MarkAction::Delete(letter))
}
