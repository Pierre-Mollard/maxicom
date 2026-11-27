//! `maxicom cli`: the client of a running maxicom, one action per call (MAX-285). It finds the
//! maxicoms by their sockets, says `hello` to each, picks one, sends its request and waits for
//! the reply: a `run`'s comes when its job ends. Ctrl-C closes the connection, which cancels
//! the job
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use base64::prelude::*;
use interprocess::local_socket::{Stream, prelude::*};
use jiff::Timestamp;
use serde_json::{Map, Value as Json, json};

use crate::{
    api::{
        self, Running,
        rpc::{self, RpcError},
    },
    args::ParseError,
    commands::value::Value,
};

/// The name the client gives itself in `hello`
const CLIENT: &str = "maxicom-cli";

/// The variable naming the socket to reach, as `--socket` does (MAX-292)
const SOCKET_VAR: &str = "MAXICOM_SOCKET";

/// How long a maxicom has to answer `hello`: one that doesn't is left out
const HELLO_WAIT: Duration = Duration::from_secs(1);

/// How a call ends, its exit code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    /// done
    Done = 0,
    /// the command failed, a `wait-for` that timed out too
    Failed = 1,
    /// the call doesn't parse
    Usage = 2,
    /// no maxicom to reach: none running, none matching, or several
    NoInstance = 3,
    /// refused by the `[api]` levels or by the user
    Refused = 4,
    /// the job was cancelled
    Cancelled = 5,
    /// the maxicom quit before its reply
    Gone = 6,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit as u8)
    }
}

/// What `maxicom cli` is asked: the instance to reach (`--instance`: a pid, a saved link's
/// name or an address; else the only one running), the socket to reach it at in place of the
/// ones found (`--socket`, MAX-292), the reply as the wire gives it (`--json`), and what to do
#[derive(Debug, PartialEq, Eq)]
pub struct CliArgs {
    pub instance: Option<String>,
    pub socket: Option<String>,
    pub json: bool,
    pub action: CliAction,
}

/// What the client does
#[derive(Debug, PartialEq, Eq)]
pub enum CliAction {
    /// a command line run in the instance, as the command line takes it
    Line(String),
    /// the instances running
    List,
    /// the events of these kinds, every kind when none, one JSON line each
    Events(Vec<String>),
    /// the bytes received from offset `since` (the oldest kept when none), in hex or as they
    /// came, then live with `follow`
    Rx {
        since: Option<u64>,
        follow: bool,
        hex: bool,
    },
    /// the help page
    Help,
}

/// `maxicom cli` with `args`, the words after `cli`
pub fn main(args: Vec<String>) -> ExitCode {
    let args = match parse(args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("maxicom cli: {err}");
            eprintln!("See maxicom cli --help.");
            return Exit::Usage.into();
        }
    };
    // a socket given, else in the environment: a forwarded one (MAX-292)
    let socket = (args.socket.clone()).or_else(|| {
        std::env::var(SOCKET_VAR)
            .ok()
            .filter(|path| !path.is_empty())
    });
    let found = || found(socket.as_deref());
    let exit = match args.action {
        CliAction::Help => {
            print_api_help();
            Exit::Done
        }
        CliAction::List => match found() {
            Ok(found) => list(found, args.instance.as_deref(), args.json),
            Err(message) => {
                eprintln!("maxicom cli: {message}");
                Exit::NoInstance
            }
        },
        action => match found().and_then(|found| pick(args.instance.as_deref(), found.0)) {
            Ok(mut maxicom) => {
                let connection = &mut maxicom.connection;
                match action {
                    CliAction::Line(line) => run(connection, &line, args.json),
                    CliAction::Events(kinds) => events(connection, kinds),
                    CliAction::Rx { since, follow, hex } => {
                        let output = match (args.json, hex) {
                            (true, _) => RxOutput::Json,
                            (false, true) => RxOutput::Hex,
                            (false, false) => RxOutput::Raw,
                        };
                        rx(connection, since, follow, output)
                    }
                    CliAction::Help | CliAction::List => unreachable!("answered above"),
                }
            }
            Err(message) => {
                eprintln!("maxicom cli: {message}");
                Exit::NoInstance
            }
        },
    };
    exit.into()
}

/// A connection to a maxicom: a request at a time, its reply read past the notifications
struct Connection {
    reader: BufReader<Stream>,
    last_id: u64,
}

impl Connection {
    fn open(location: &str) -> io::Result<Self> {
        Ok(Self {
            reader: BufReader::new(api::connect(location)?),
            last_id: 0,
        })
    }

    /// How long a read waits, none for ever (not on Windows pipes, which wait for ever)
    fn wait_at_most(&self, wait: Option<Duration>) {
        let _ = self.reader.get_ref().set_recv_timeout(wait);
    }

    /// `method` called with `params`: its reply's result, or its error. The connection ending
    /// first is `UnexpectedEof`
    fn call(&mut self, method: &str, params: Json) -> io::Result<Result<Json, RpcError>> {
        self.last_id += 1;
        let id = json!(self.last_id);
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        (self.reader.get_mut()).write_all(format!("{request}\n").as_bytes())?;
        loop {
            let mut message = self.next()?;
            if message.get("id") != Some(&id) {
                continue;
            }
            return Ok(match message.remove("error") {
                Some(error) => Err(RpcError {
                    code: error["code"].as_i64().unwrap_or_default(),
                    message: error["message"].as_str().unwrap_or_default().to_string(),
                }),
                None => Ok(message.remove("result").unwrap_or_default()),
            });
        }
    }

    /// The next message: a reply or a notification, a line that isn't one skipped. The
    /// connection ending first is `UnexpectedEof`
    fn next(&mut self) -> io::Result<Map<String, Json>> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            if let Ok(Json::Object(message)) = serde_json::from_str(&line) {
                return Ok(message);
            }
        }
    }
}

/// A maxicom that answered `hello`: its pid, its `hello`, its socket and what reaches it
struct Found<C> {
    pid: u32,
    hello: Json,
    location: String,
    connection: C,
}

impl<C> Found<C> {
    /// Whether `id` names it: its pid, its saved link's name or its address
    fn is(&self, id: &str) -> bool {
        self.pid.to_string() == id
            || self.hello["name"].as_str() == Some(id)
            || self.hello["address"].as_str() == Some(id)
    }

    /// Its line in a list: its pid and its link
    fn line(&self) -> String {
        let link = self.hello["link"].as_str().unwrap_or("?");
        format!("  {:>7}  {link}", self.pid)
    }
}

/// The maxicoms running, by pid: those that answer `hello`, each with its connection, and
/// those that take the connection but don't answer in `HELLO_WAIT` (busy, or stuck). One that
/// refuses the connect is left out: gone, or another user's pipe (Windows). With `socket`,
/// the one at that path only, its pid from its `hello` (MAX-292): an error when nothing is
/// there
fn found(socket: Option<&str>) -> Result<(Vec<Found<Connection>>, Vec<Running>), String> {
    let running = match socket {
        Some(path) => vec![Running {
            pid: 0,
            location: path.to_string(),
            link: None,
        }],
        None => api::running(
            api::socket_dir().as_deref(),
            crate::link::lock::lock_dir().as_deref(),
        ),
    };
    let (mut answering, mut silent) = (Vec::new(), Vec::new());
    for running in running {
        let Ok(mut connection) = Connection::open(&running.location) else {
            continue;
        };
        connection.wait_at_most(Some(HELLO_WAIT));
        let hello = connection.call("hello", json!({ "client": CLIENT }));
        connection.wait_at_most(None);
        match hello {
            Ok(Ok(hello)) => answering.push(Found {
                pid: (hello["pid"].as_u64()).map_or(running.pid, |pid| pid as u32),
                hello,
                location: running.location,
                connection,
            }),
            _ => silent.push(running),
        }
    }
    match socket {
        Some(path) if answering.is_empty() && silent.is_empty() => {
            Err(format!("no maxicom answers at {path}"))
        }
        _ => Ok((answering, silent)),
    }
}

/// The maxicom `id` names (`Found::is`), else the only one running; why there's none to pick
/// else, the ones to pick from listed
fn pick<C>(id: Option<&str>, found: Vec<Found<C>>) -> Result<Found<C>, String> {
    let listed = |found: &[Found<C>]| {
        let lines: Vec<String> = found.iter().map(Found::line).collect();
        lines.join("\n")
    };
    let (mut matching, others): (Vec<_>, Vec<_>) =
        (found.into_iter()).partition(|found| id.is_none_or(|id| found.is(id)));
    match (matching.len(), id) {
        (1, _) => Ok(matching.remove(0)),
        (0, None) => Err("no maxicom is running, or its [api] isn't enabled".to_string()),
        (0, Some(id)) if others.is_empty() => Err(format!(
            "no maxicom is running, so none is '{id}', or its [api] isn't enabled"
        )),
        (0, Some(id)) => Err(format!(
            "no maxicom running is '{id}': pick one by its pid, its link's name or its \
             address:\n{}",
            listed(&others)
        )),
        (_, _) => Err(format!(
            "several maxicoms match: pick one with --instance, by its pid, its link's name or \
             its address:\n{}",
            listed(&matching)
        )),
    }
}

/// `list`: the maxicoms `found`, those `id` names when given, as a table of their pid, link
/// and start, or as JSON: a list of their `hello`s with their socket's `location` (MAX-292),
/// `{pid, link, location, answering: false}` for one that doesn't answer. None running is no
/// error: the table is empty
fn list(found: (Vec<Found<Connection>>, Vec<Running>), id: Option<&str>, json: bool) -> Exit {
    let (answering, silent) = found;
    let answering: Vec<_> = (answering.into_iter())
        .filter(|found| id.is_none_or(|id| found.is(id)))
        .collect();
    let silent: Vec<_> = (silent.into_iter())
        .filter(|running| id.is_none_or(|id| running.pid.to_string() == id))
        .collect();
    if json {
        let silent = (silent.iter()).map(|running| {
            let Running {
                pid,
                location,
                link,
            } = running;
            json!({ "pid": pid, "link": link, "location": location, "answering": false })
        });
        let hellos = (answering.into_iter()).map(|found| {
            let mut hello = found.hello;
            hello["location"] = json!(found.location);
            hello
        });
        print_lines([Json::Array(hellos.chain(silent).collect()).to_string()]);
        return Exit::Done;
    }
    let mut rows: Vec<[String; 3]> = (answering.iter())
        .map(|found| {
            let link = found.hello["link"].as_str().unwrap_or("?").to_string();
            let since = (found.hello["since"].as_str())
                .and_then(|since| since.parse::<Timestamp>().ok())
                .map(|since| crate::clock::format(since, "%Y-%m-%d %H:%M:%S"))
                .unwrap_or_default();
            [found.pid.to_string(), link, since]
        })
        .collect();
    rows.extend(silent.into_iter().map(|running| {
        let link = running.link.unwrap_or_else(|| "?".to_string());
        [running.pid.to_string(), link, "not answering".to_string()]
    }));
    rows.sort_by_key(|row| row[0].parse::<u32>().unwrap_or_default());
    if !rows.is_empty() {
        print_lines(table(&rows));
    }
    Exit::Done
}

/// `list`'s table: a header, then one line per maxicom, the pids to the right, the links to
/// the left in a column as wide as the widest
fn table(rows: &[[String; 3]]) -> Vec<String> {
    let header = ["PID", "LINK", "SINCE"].map(str::to_string);
    let pid = (rows.iter())
        .map(|row| row[0].len())
        .chain([3])
        .max()
        .unwrap_or_default();
    let link = (rows.iter())
        .map(|row| row[1].chars().count())
        .chain([4])
        .max();
    let link = link.unwrap_or_default();
    (std::iter::once(&header).chain(rows))
        .map(|[p, l, since]| {
            format!("{p:>pid$}  {l:<link$}  {since}")
                .trim_end()
                .to_string()
        })
        .collect()
}

/// `run {command: line}`: its notes and values, or its error and the exit code of its kind;
/// with `json`, its result or error as the wire gives them
fn run(connection: &mut Connection, line: &str, json: bool) -> Exit {
    let reply = match connection.call("run", json!({ "command": line })) {
        Ok(reply) => reply,
        Err(_) => return gone(),
    };
    let exit = reply.as_ref().map_or_else(exit_of, |_| Exit::Done);
    match (reply, json) {
        (Ok(result), true) => print_lines([result.to_string()]),
        (Ok(result), false) => print_lines(result_lines(&result)),
        (Err(error), true) => {
            print_lines([json!({ "code": error.code, "message": error.message }).to_string()]);
        }
        (Err(error), false) => eprintln!("maxicom cli: {}", error.message),
    }
    exit
}

/// `events`: subscribed to `kinds`, every kind when none, then each event on a line of JSON
/// (the `event` notification's params, the streams' notifications skipped), until maxicom
/// quits. A kind unknown is a usage error; the reader gone (`| head`) ends it quietly
fn events(connection: &mut Connection, kinds: Vec<String>) -> Exit {
    let params = match kinds.is_empty() {
        true => json!({}),
        false => json!({ "events": kinds }),
    };
    match connection.call("subscribe", params) {
        Ok(Ok(_)) => {}
        Ok(Err(error)) if error.code == rpc::INVALID_PARAMS => {
            eprintln!("maxicom cli: {}", error.message);
            return Exit::Usage;
        }
        Ok(Err(error)) => return refused(&error),
        Err(_) => return gone(),
    }
    let mut stdout = io::stdout().lock();
    loop {
        let mut message = match connection.next() {
            Ok(message) => message,
            Err(_) => return gone(),
        };
        if message.get("method").and_then(Json::as_str) != Some("event") {
            continue;
        }
        let event = message.remove("params").unwrap_or_default();
        if writeln!(stdout, "{event}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return Exit::Done;
        }
    }
}

/// How `rx` writes the bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RxOutput {
    /// as they came
    Raw,
    /// a hex dump, each line its offset first
    Hex,
    /// each `raw-since` value as a line of JSON, as the API gives it
    Json,
}

/// `rx`: the bytes received from offset `since` (the oldest kept when none) to the newest, by
/// pages of `raw-since`; with `follow`, then the new ones on each `rx` notification until
/// maxicom quits, subscribed first so none is missed. Bytes already gone past `since` are
/// said on stderr, so is the next offset without `follow` (for the next `--since`)
fn rx(connection: &mut Connection, since: Option<u64>, follow: bool, output: RxOutput) -> Exit {
    if follow {
        match connection.call("subscribe", json!({ "events": ["rx"] })) {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return refused(&error),
            Err(_) => return gone(),
        }
    }
    let mut next = since.unwrap_or_default();
    let mut stdout = io::stdout().lock();
    // the first page: the bytes gone are said for an offset asked, not for the oldest's, and
    // it's written in JSON even empty, so a poll gets the newest offset
    let mut first = true;
    loop {
        // up to the newest, by pages
        loop {
            let command = format!("raw-since {next} {}", crate::commands::query::MAX_RAW);
            let page = match connection.call("query", json!({ "command": command })) {
                Ok(Ok(page)) => page,
                Ok(Err(error)) => return refused(&error),
                Err(_) => return gone(),
            };
            let start = page["start"].as_u64().unwrap_or(next);
            if first && since.is_some() && page["cut"].as_bool() == Some(true) {
                eprintln!("maxicom cli: the bytes from {next} to {start} are gone");
            }
            let encoded = page["bytes"].as_str().unwrap_or_default();
            let bytes = BASE64_STANDARD.decode(encoded).unwrap_or_default();
            let written = match output {
                RxOutput::Raw => stdout.write_all(&bytes),
                RxOutput::Hex => (hex_lines(start, &bytes).iter())
                    .try_for_each(|line| writeln!(stdout, "{line}")),
                RxOutput::Json if bytes.is_empty() && !first => Ok(()),
                RxOutput::Json => writeln!(stdout, "{page}"),
            };
            if written.and_then(|()| stdout.flush()).is_err() {
                return Exit::Done;
            }
            first = false;
            next = start + bytes.len() as u64;
            if bytes.is_empty() || next >= page["newest"].as_u64().unwrap_or_default() {
                break;
            }
        }
        if !follow {
            if output != RxOutput::Json {
                eprintln!("maxicom cli: next offset {next}");
            }
            return Exit::Done;
        }
        // the next pages when maxicom says more came
        loop {
            match connection.next() {
                Ok(message) if message.get("method").and_then(Json::as_str) == Some("rx") => {
                    break;
                }
                Ok(_) => {}
                Err(_) => return gone(),
            }
        }
    }
}

/// Bytes as a hex dump: 16 a line, each line the offset of its first byte, the bytes in hex,
/// then their printable ASCII (`.` for the others)
fn hex_lines(start: u64, bytes: &[u8]) -> Vec<String> {
    (bytes.chunks(16).enumerate())
        .map(|(row, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
            let text: String = (chunk.iter())
                .map(|&byte| match byte {
                    0x20..=0x7e => byte as char,
                    _ => '.',
                })
                .collect();
            let offset = start + row as u64 * 16;
            format!("{offset:>10}  {:<47}  {text}", hex.join(" "))
        })
        .collect()
}

/// A request refused or failed: its message on stderr, its exit code
fn refused(error: &RpcError) -> Exit {
    eprintln!("maxicom cli: {}", error.message);
    exit_of(error)
}

/// maxicom quit, or the connection broke, before the end of the call
fn gone() -> Exit {
    eprintln!("maxicom cli: maxicom quit");
    Exit::Gone
}

/// The exit code of a refused or stopped request
fn exit_of(error: &RpcError) -> Exit {
    match error.code {
        rpc::REFUSED | rpc::REFUSED_BY_USER => Exit::Refused,
        rpc::CANCELLED => Exit::Cancelled,
        _ => Exit::Failed,
    }
}

/// A `run`'s result as text: its notes, then its values as the TUI shows them, each under its
/// command as it ran when there are several (`get local-echo`)
fn result_lines(result: &Json) -> Vec<String> {
    let notes = (result["notes"].as_array().into_iter().flatten()).filter_map(Json::as_str);
    let mut lines: Vec<String> = notes.map(str::to_string).collect();
    let values: Vec<(String, Value)> = (result["values"].as_array().into_iter().flatten())
        .map(|answer| {
            let command = answer["command"].as_str().or(answer["query"].as_str());
            (
                command.unwrap_or_default().to_string(),
                value_of(&answer["value"]),
            )
        })
        .collect();
    match <[_; 1]>::try_from(values) {
        Ok([(_, value)]) => lines.extend(value.lines()),
        Err(values) if values.is_empty() => {}
        Err(values) => lines.extend(Value::Map(values).lines()),
    }
    lines
}

/// A value as the API gave it, back as a query's value for its text: bytes stay base64, a
/// number that isn't whole stays text
fn value_of(json: &Json) -> Value {
    match json {
        Json::Null => Value::text("none"),
        Json::Bool(yes) => Value::Bool(*yes),
        Json::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::text(n.to_string()), Value::Int),
        Json::String(text) => Value::text(text),
        Json::Array(items) => Value::List(items.iter().map(value_of).collect()),
        Json::Object(entries) => Value::Map(
            (entries.iter())
                .map(|(name, value)| (name.clone(), value_of(value)))
                .collect(),
        ),
    }
}

/// Lines on stdout, a reader gone (`| head`) not being an error
fn print_lines(lines: impl IntoIterator<Item = String>) {
    let mut stdout = io::stdout().lock();
    for line in lines {
        if writeln!(stdout, "{line}").is_err() {
            return;
        }
    }
}

/// The options first, then the action: one of the client's own verbs (`list`, `events`,
/// `rx`) with its own options, else a command line
fn parse(args: impl IntoIterator<Item = String>) -> Result<CliArgs, ParseError> {
    let mut args = args.into_iter().peekable();
    let mut instance = None;
    let mut socket = None;
    let mut json = false;
    while let Some(arg) = args.next_if(|arg| arg.starts_with('-')) {
        let (name, glued) = split_glued(&arg);
        match name {
            "--instance" => instance = Some(value("--instance", glued, &mut args)?),
            "--socket" => socket = Some(value("--socket", glued, &mut args)?),
            "--json" if glued.is_none() => json = true,
            "--help" | "-h" if glued.is_none() => {
                let action = CliAction::Help;
                return Ok(CliArgs {
                    instance,
                    socket,
                    json,
                    action,
                });
            }
            _ => return Err(ParseError::UnknownArgument(arg)),
        }
    }
    let words: Vec<String> = args.collect();
    let action = match words.first().map(String::as_str) {
        None => {
            return Err(ParseError::BadFormat(
                "No command given: maxicom cli <command line>.".to_string(),
            ));
        }
        Some("list") => match words.get(1) {
            Some(arg) => return Err(ParseError::UnknownArgument(arg.clone())),
            None => CliAction::List,
        },
        Some("events") => match words[1..].iter().find(|word| word.starts_with('-')) {
            Some(arg) => return Err(ParseError::UnknownArgument(arg.clone())),
            None => CliAction::Events(words[1..].to_vec()),
        },
        Some("rx") => rx_args(words[1..].iter().cloned())?,
        Some(_) => CliAction::Line(line(&words)),
    };
    Ok(CliArgs {
        instance,
        socket,
        json,
        action,
    })
}

/// `rx`'s options: `--since <offset>`, `--follow`, `--hex`
fn rx_args(args: impl IntoIterator<Item = String>) -> Result<CliAction, ParseError> {
    let mut args = args.into_iter();
    let (mut since, mut follow, mut hex) = (None, false, false);
    while let Some(arg) = args.next() {
        let (name, glued) = split_glued(&arg);
        match name {
            "--since" => {
                let offset = value("--since", glued, &mut args)?;
                let bad = |_| ParseError::BadFormat(format!("Invalid offset '{offset}'."));
                since = Some(offset.parse().map_err(bad)?);
            }
            "--follow" if glued.is_none() => follow = true,
            "--hex" if glued.is_none() => hex = true,
            _ => return Err(ParseError::UnknownArgument(arg)),
        }
    }
    Ok(CliAction::Rx { since, follow, hex })
}

/// `--name=value` split at its `=`, as the other options take it
fn split_glued(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((name, value)) if name.starts_with("--") => (name, Some(value)),
        _ => (arg, None),
    }
}

/// The value of option `name`: glued to it, else the next argument
fn value(
    name: &'static str,
    glued: Option<&str>,
    args: &mut impl Iterator<Item = String>,
) -> Result<String, ParseError> {
    match glued {
        Some(value) => Ok(value.to_string()),
        None => args.next().ok_or(ParseError::MissingValue(name)),
    }
}

/// The command line in `words`: one word is the line as typed (`'status; get baud'`), as
/// `sh -c` takes it; several are the words the shell split, each quoted again when it holds
/// a space, a quote or a `;` so it stays one word, and a lone `;` still ends a command
fn line(words: &[String]) -> String {
    if let [line] = words {
        return line.clone();
    }
    let quoted = |word: &String| {
        let plain = word == ";"
            || !word.is_empty()
                && !(word.chars()).any(|c| c.is_whitespace() || matches!(c, ';' | '"' | '\''));
        match plain {
            true => word.clone(),
            false => format!("\"{}\"", word.replace('"', "\\\"")),
        }
    };
    words.iter().map(quoted).collect::<Vec<_>>().join(" ")
}

/// What `maxicom api` is asked (MAX-292): the instance to reach and the socket to reach it at,
/// as `maxicom cli` takes them, or its help page
#[derive(Debug, PartialEq, Eq)]
pub struct BridgeArgs {
    pub instance: Option<String>,
    pub socket: Option<String>,
    pub help: bool,
}

/// `maxicom api`'s options: `--instance`, `--socket`, `--help`, nothing else
fn parse_bridge(args: impl IntoIterator<Item = String>) -> Result<BridgeArgs, ParseError> {
    let mut args = args.into_iter();
    let mut parsed = BridgeArgs {
        instance: None,
        socket: None,
        help: false,
    };
    while let Some(arg) = args.next() {
        let (name, glued) = split_glued(&arg);
        match name {
            "--instance" => parsed.instance = Some(value("--instance", glued, &mut args)?),
            "--socket" => parsed.socket = Some(value("--socket", glued, &mut args)?),
            "--help" | "-h" if glued.is_none() => parsed.help = true,
            _ => return Err(ParseError::UnknownArgument(arg)),
        }
    }
    Ok(parsed)
}

/// `maxicom api` (MAX-292): a session with a running maxicom over stdio, for a program that
/// reaches it through `ssh bench maxicom api`. The bytes are copied both ways as they are,
/// JSON-RPC lines unread: stdin to the socket, the socket to stdout. stdin closing ends the
/// session, the connection with it, so its jobs are cancelled as for a program on the socket;
/// maxicom quitting ends it too
pub fn bridge(args: Vec<String>) -> ExitCode {
    let args = match parse_bridge(args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("maxicom api: {err}");
            eprintln!("See maxicom api --help.");
            return Exit::Usage.into();
        }
    };
    if args.help {
        print_bridge_help();
        return Exit::Done.into();
    }
    let socket = (args.socket.clone()).or_else(|| {
        std::env::var(SOCKET_VAR)
            .ok()
            .filter(|path| !path.is_empty())
    });
    let found = found(socket.as_deref());
    let maxicom = match found.and_then(|found| pick(args.instance.as_deref(), found.0)) {
        Ok(maxicom) => maxicom,
        Err(message) => {
            eprintln!("maxicom api: {message}");
            return Exit::NoInstance.into();
        }
    };
    // the bytes read past `hello`'s reply are the program's: written first
    let reader = maxicom.connection.reader;
    let early = reader.buffer().to_vec();
    let stream = Arc::new(reader.into_inner());
    let sending = Arc::clone(&stream);
    // stdin's end is the session's: the process ends there, the connection with it
    let spawned = std::thread::Builder::new()
        .name("api-stdin".to_string())
        .spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut &*sending);
            std::process::exit(Exit::Done as i32);
        });
    if spawned.is_err() {
        eprintln!("maxicom api: no thread for stdin");
        return Exit::Failed.into();
    }
    let mut stdout = io::stdout().lock();
    // false once the program reading is gone
    let mut write = |bytes: &[u8]| {
        stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .is_ok()
    };
    if !write(&early) {
        return Exit::Done.into();
    }
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match (&*stream).read(&mut buffer) {
            Ok(0) | Err(_) => {
                eprintln!("maxicom api: maxicom quit");
                return Exit::Gone.into();
            }
            Ok(read) if !write(&buffer[..read]) => return Exit::Done.into(),
            Ok(_) => {}
        }
    }
}

fn print_bridge_help() {
    println!(
        "\
maxicom api — a session with a running maxicom over stdio, for SSH

Usage:
  maxicom api [OPTIONS]

  Copies the API's JSON-RPC lines both ways: stdin to the maxicom, its replies
  and notifications to stdout. A program on another computer reaches it with
  ssh bench maxicom api. Closing stdin ends the session, and maxicom cancels its
  jobs; for one action at a time, use maxicom cli.

Options:
      --instance <ID>      The maxicom to reach: its pid, its saved link's name or
                           its address [default: the only one running]
      --socket <PATH>      The socket to reach it at, in place of the ones running
                           here [default: $MAXICOM_SOCKET when set, else the
                           maxicoms running on this computer]
  -h, --help               Print help
"
    );
}

fn print_api_help() {
    println!(
        "\
maxicom cli — the client of a running maxicom, one action per call

Usage:
  maxicom cli [OPTIONS] <COMMAND LINE>
  maxicom cli [OPTIONS] list
  maxicom cli [OPTIONS] events [KIND...]
  maxicom cli [OPTIONS] rx [--since <OFFSET>] [--follow] [--hex]

  COMMAND LINE             Run in maxicom as typed on its command line, waiting
                           for it to end: maxicom cli 'send \"reboot\\r\";
                           wait-for-ms 5000 login:'. One argument is the line as
                           is; several are its words, each one kept whole, a
                           lone ';' between two commands
  list                     The maxicoms running
  events [KIND...]         The events of these kinds (all without any), one JSON
                           object per line, until maxicom quits
  rx                       The bytes received, as they came
      --since <OFFSET>     From this offset on [default: the oldest kept]
      --follow             Then the bytes as they come, until maxicom quits
      --hex                In hex

Options, before the action:
      --instance <ID>      The maxicom to reach: its pid, its saved link's name or
                           its address [default: the only one running]
      --socket <PATH>      The socket to reach it at, a forwarded one, in place of
                           the ones running here [default: $MAXICOM_SOCKET when
                           set, else the maxicoms running on this computer]
      --json               The reply as JSON, as the API gives it
  -h, --help               Print help
"
    );
}

#[cfg(test)]
#[path = "tests/cli.rs"]
mod tests;
