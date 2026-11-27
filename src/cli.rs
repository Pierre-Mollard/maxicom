//! `maxicom cli`: the client of a running maxicom, one action per call (MAX-285). It finds the
//! maxicoms by their sockets, says `hello` to each, picks one, sends its request and waits for
//! the reply: a `run`'s comes when its job ends. Ctrl-C closes the connection, which cancels
//! the job
use std::{
    io::{self, BufRead, BufReader, Write},
    process::ExitCode,
    time::Duration,
};

use interprocess::local_socket::{Stream, prelude::*};
use jiff::Timestamp;
use serde_json::{Value as Json, json};

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
/// name or an address; else the only one running), the reply as the wire gives it (`--json`),
/// and what to do
#[derive(Debug, PartialEq, Eq)]
pub struct CliArgs {
    pub instance: Option<String>,
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
    let exit = match args.action {
        CliAction::Help => {
            print_help();
            Exit::Done
        }
        CliAction::List => list(args.instance.as_deref(), args.json),
        CliAction::Line(line) => match pick(args.instance.as_deref(), found().0) {
            Ok(mut maxicom) => run(&mut maxicom.connection, &line, args.json),
            Err(message) => {
                eprintln!("maxicom cli: {message}");
                Exit::NoInstance
            }
        },
        // TODO: (MAX-285) events and rx, steps 4-5
        _ => {
            eprintln!("maxicom cli: not built yet");
            Exit::Failed
        }
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
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let Ok(Json::Object(mut message)) = serde_json::from_str(&line) else {
                continue;
            };
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
}

/// A maxicom that answered `hello`: its pid, its `hello`, and what reaches it
struct Found<C> {
    pid: u32,
    hello: Json,
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
/// refuses the connect is left out: gone, or another user's pipe (Windows)
fn found() -> (Vec<Found<Connection>>, Vec<Running>) {
    let running = api::running(
        api::socket_dir().as_deref(),
        crate::link::lock::lock_dir().as_deref(),
    );
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
                pid: running.pid,
                hello,
                connection,
            }),
            _ => silent.push(running),
        }
    }
    (answering, silent)
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

/// `list`: the maxicoms running, those `id` names when given, as a table of their pid, link
/// and start, or as JSON: a list of their `hello`s, `{pid, link, answering: false}` for one
/// that doesn't answer. None running is no error: the table is empty
fn list(id: Option<&str>, json: bool) -> Exit {
    let (answering, silent) = found();
    let answering: Vec<_> = (answering.into_iter())
        .filter(|found| id.is_none_or(|id| found.is(id)))
        .collect();
    let silent: Vec<_> = (silent.into_iter())
        .filter(|running| id.is_none_or(|id| running.pid.to_string() == id))
        .collect();
    if json {
        let silent = (silent.iter())
            .map(|running| json!({ "pid": running.pid, "link": running.link, "answering": false }));
        let hellos = answering.into_iter().map(|found| found.hello);
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
        Err(_) => {
            eprintln!("maxicom cli: maxicom quit before its reply");
            return Exit::Gone;
        }
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

/// The exit code of a refused or stopped `run`
fn exit_of(error: &RpcError) -> Exit {
    match error.code {
        rpc::REFUSED | rpc::REFUSED_BY_USER => Exit::Refused,
        rpc::CANCELLED => Exit::Cancelled,
        _ => Exit::Failed,
    }
}

/// A `run`'s result as text: its notes, then its values as the TUI shows them, each under its
/// query when there are several
fn result_lines(result: &Json) -> Vec<String> {
    let notes = (result["notes"].as_array().into_iter().flatten()).filter_map(Json::as_str);
    let mut lines: Vec<String> = notes.map(str::to_string).collect();
    let values: Vec<(String, Value)> = (result["values"].as_array().into_iter().flatten())
        .map(|answer| {
            let query = answer["query"].as_str().unwrap_or_default().to_string();
            (query, value_of(&answer["value"]))
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
    let mut json = false;
    while let Some(arg) = args.next_if(|arg| arg.starts_with('-')) {
        let (name, glued) = split_glued(&arg);
        match name {
            "--instance" => instance = Some(value("--instance", glued, &mut args)?),
            "--json" if glued.is_none() => json = true,
            "--help" | "-h" if glued.is_none() => {
                let action = CliAction::Help;
                return Ok(CliArgs {
                    instance,
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
        Some("rx") => rx(words[1..].iter().cloned())?,
        Some(_) => CliAction::Line(line(&words)),
    };
    Ok(CliArgs {
        instance,
        json,
        action,
    })
}

/// `rx`'s options: `--since <offset>`, `--follow`, `--hex`
fn rx(args: impl IntoIterator<Item = String>) -> Result<CliAction, ParseError> {
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

fn print_help() {
    println!(
        "\
maxicom cli — the client of a running maxicom, one action per call

Usage:
  maxicom cli [OPTIONS] <COMMAND LINE>
  maxicom cli [OPTIONS] list
  maxicom cli [OPTIONS] events [KIND...]
  maxicom cli [OPTIONS] rx [--since <OFFSET>] [--follow] [--hex]

  COMMAND LINE             Run in maxicom as typed on its command line, waiting
                           for it to end: maxicom cli 'send \"reboot\\r\"; wait-for
                           login: 5000'. One argument is the line as is; several
                           are its words, each one kept whole
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
      --json               The reply as JSON, as the API gives it
  -h, --help               Print help
"
    );
}

#[cfg(test)]
#[path = "tests/cli.rs"]
mod tests;
