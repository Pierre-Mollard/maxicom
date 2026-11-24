//! The queries (MAX-165): commands that read the app's state and change nothing, run at once,
//! their `Value` shown in a popup for the user and later given to a client (MAX-284). A
//! trigger's or a profile's query does nothing: nobody waits for its answer
use super::{ArgValue, value::Value};
use crate::{app::App, link::LinkKind};

/// `status`: the link (its name, kind and state, its line settings), the mode, the encoding,
/// the bytes received and sent, the recording, the file going through and the profiles laid
pub fn status(app: &App, _args: &[ArgValue]) -> Result<Value, String> {
    let kind = app.link_kind();
    let state = match (kind, app.is_link_open()) {
        (LinkKind::Replay, _) => "replay",
        (_, true) => "open",
        (_, false) => "closed",
    };
    let mut entries = vec![
        ("link".to_string(), Value::text(app.link_label())),
        ("kind".to_string(), Value::text(kind.name())),
        ("state".to_string(), Value::text(state)),
    ];
    if let Some(line) = app.line_settings() {
        let line = format!("{} {}", line.baud, line.framing);
        entries.push(("line".to_string(), Value::text(line)));
    }
    let settings = app.settings();
    let recording = app.recording().map_or("none", |stream| stream.as_str());
    let transfer = match app.transfer() {
        Some(transfer) => format!("{} at {}%", transfer.name(), transfer.percent()),
        None => "none".to_string(),
    };
    let profiles = app.profiles().iter().map(Value::text).collect();
    entries.extend([
        ("mode".to_string(), Value::text(settings.mode.as_str())),
        (
            "encoding".to_string(),
            Value::text(settings.encoding.as_str()),
        ),
        ("rx".to_string(), Value::Int(app.stats.rx() as i64)),
        ("tx".to_string(), Value::Int(app.stats.tx() as i64)),
        ("recording".to_string(), Value::text(recording)),
        ("transfer".to_string(), Value::text(transfer)),
        ("profiles".to_string(), Value::List(profiles)),
    ]);
    Ok(Value::Map(entries))
}
