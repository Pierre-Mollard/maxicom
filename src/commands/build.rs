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
    inputs::{Action, AppAction},
    link,
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
    if app.transfer().is_some() {
        return Err("a file is being sent already (send-cancel stops it)".to_string());
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
