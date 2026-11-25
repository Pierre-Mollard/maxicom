//! The local API (MAX-74): other programs connect to this maxicom through a local socket (a Unix
//! socket file, a named pipe on Windows) and talk JSON-RPC 2.0, one message per line (`rpc`).
//!
//! Threads only move lines: one accepts the connections, each connection has a reader and a
//! writer. The requests reach the loop through a channel it drains on every pass, and the loop
//! answers them with the app in hand: no lock on the app. The methods are A2.1's (MAX-284):
//! `hello` says who this maxicom is, `query` runs a getter, `subscribe` sends the events
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use base64::prelude::*;
use interprocess::local_socket::{
    ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*,
};
use jiff::Timestamp;
use serde_json::{Value as Json, json};

use crate::{
    app::App,
    commands::{CommandError, Target, value::Value},
    event::{Event, KINDS, Logged},
};

pub mod rpc;
#[cfg(windows)]
pub mod windows;

use rpc::{Request, RpcError};

/// The API's version in `hello`: a client checks it before anything else
pub const API_VERSION: u32 = 1;

/// How often the accept thread looks whether it should stop
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// The socket files' start and end, `maxicom-<pid>.sock`
const PREFIX: &str = "maxicom-";
#[cfg(unix)]
const SUFFIX: &str = ".sock";

/// This maxicom, as `hello` gives it
#[derive(Debug, Clone, Copy)]
pub struct Instance {
    pub pid: u32,
    pub since: Timestamp,
}

impl Instance {
    /// This process, started now
    pub fn now() -> Self {
        Self {
            pid: std::process::id(),
            since: Timestamp::now(),
        }
    }

    /// Its socket's name: `maxicom-<pid>`
    pub fn name(&self) -> String {
        format!("{PREFIX}{}", self.pid)
    }
}

/// The folder of the socket files on Unix: the runtime folder, else the cache folder (macOS
/// has no runtime folder). Unused on Windows, whose pipes have no file
pub fn socket_dir() -> Option<PathBuf> {
    (dirs::runtime_dir().or_else(dirs::cache_dir)).map(|dir| dir.join("maxicom"))
}

/// A maxicom found running by its socket (MAX-74), for `maxicom list` (A2.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    /// Its socket file, or its pipe
    pub location: String,
    /// The link it holds, by the lock files: none when it holds no lock (a replay, the
    /// directory) or the lock folder can't be read
    pub link: Option<String>,
}

/// The maxicoms running, by pid: their sockets in `dir`, this user's folder, a socket file
/// being alive when it can be connected to (Unix); every pipe named so (Windows: other users'
/// too, which refuse the connect). Each one's link from the locks held in `lock_dir`. `dir`
/// and `lock_dir` are `socket_dir()` and `lock_dir()` but in tests
// TODO: (MAX-285) used by `maxicom list`; the attribute goes with its first caller
#[cfg_attr(not(test), expect(dead_code, reason = "A2.2's maxicom list (MAX-285)"))]
pub fn running(dir: Option<&Path>, lock_dir: Option<&Path>) -> Vec<Running> {
    let locks = lock_dir
        .map(crate::link::lock::held_locks)
        .unwrap_or_default();
    let mut running: Vec<Running> = (listening(dir).into_iter())
        .filter_map(|(name, location)| {
            let pid = name.strip_prefix(PREFIX)?.parse().ok()?;
            let link = (locks.iter())
                .find(|lock| lock.pid == pid)
                .map(|lock| lock.address.clone());
            Some(Running {
                pid,
                location,
                link,
            })
        })
        .collect();
    running.sort_by_key(|found| found.pid);
    running
}

/// The names and locations of the maxicom sockets listened on: the files of `dir` a connect
/// gets through to (Unix)
#[cfg(unix)]
fn listening(dir: Option<&Path>) -> Vec<(String, String)> {
    let Some(entries) = dir.and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return Vec::new();
    };
    (entries.flatten())
        .filter_map(|entry| {
            let file_name = entry.file_name().into_string().ok()?;
            let name = file_name.strip_suffix(SUFFIX)?;
            let path = entry.path();
            (name.starts_with(PREFIX) && is_alive(&path))
                .then(|| (name.to_string(), path.display().to_string()))
        })
        .collect()
}

/// The names and locations of the maxicom pipes: the pipe folder lists only the pipes that
/// exist, so no connect, which would take an instance of the pipe (Windows)
#[cfg(windows)]
fn listening(_dir: Option<&Path>) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(r"\\.\pipe\") else {
        return Vec::new();
    };
    (entries.flatten())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.starts_with(PREFIX)
                .then(|| (name.clone(), format!(r"\\.\pipe\{name}")))
        })
        .collect()
}

/// Whether a maxicom listens on a socket file: a dead one's file refuses the connect
#[cfg(unix)]
fn is_alive(path: &Path) -> bool {
    use interprocess::local_socket::GenericFilePath;
    (path.to_path_buf().to_fs_name::<GenericFilePath>())
        .and_then(Stream::connect)
        .is_ok()
}

/// The answer to a request, for the loop: none for a notification, which isn't run either
/// (the methods so far only read)
pub fn answer(app: &mut App, instance: &Instance, request: &Request) -> Option<String> {
    let id = request.id.as_ref()?;
    let result = match request.method.as_str() {
        "hello" => hello(app, instance, request.params.as_ref()),
        "query" => query(app, request.params.as_ref()),
        method => Err(RpcError::method_not_found(method)),
    };
    Some(rpc::reply(id, result))
}

/// `hello {client?}`: who this maxicom is. `client` is the name the client gives itself
// TODO: (MAX-286) the client's name kept with its connection, for the TUI to show (A3)
fn hello(app: &App, instance: &Instance, params: Option<&Json>) -> Result<Json, RpcError> {
    let _client = text_param(params, "client", false)?;
    Ok(json!({
        "api": API_VERSION,
        "version": env!("CARGO_PKG_VERSION"),
        "pid": instance.pid,
        "link": app.link_label(),
        "since": crate::commands::query::utc(instance.since),
    }))
}

/// `query {command}`: `command` is a command as typed in the command-line popup, parsed
/// against the registry and the user's `[commands]` (MAX-239); it must come out as one
/// command whose target is a query (MAX-283's getters) or a popup, whose value it gives. Run
/// at once, not as a job: a query never waits and changes nothing. Its value as JSON
fn query(app: &mut App, params: Option<&Json>) -> Result<Json, RpcError> {
    let command = text_param(params, "command", true)?.unwrap_or_default();
    let invocations = crate::commands::parse_with(command, &app.key_resolver.commands)
        .map_err(|error| RpcError::command(&error))?;
    let [invocation] = invocations.as_slice() else {
        return Err(RpcError::not_a_query(format!(
            "one query, not {}",
            invocations.len()
        )));
    };
    let name = invocation.spec.name;
    let (Target::Query(run) | Target::Popup { query: run, .. }) = invocation.spec.target else {
        return Err(RpcError::not_a_query(format!("{name} isn't a query")));
    };
    let value = run(app, &invocation.args).map_err(|reason| {
        RpcError::command(&CommandError::Failed {
            command: name,
            reason,
        })
    })?;
    Ok(json(&value))
}

/// A text param by its name, from params given by name: an error when it isn't a text, or
/// when it is `required` and missing
fn text_param<'a>(
    params: Option<&'a Json>,
    name: &str,
    required: bool,
) -> Result<Option<&'a str>, RpcError> {
    let object = match params {
        None => None,
        Some(Json::Object(object)) => Some(object),
        Some(_) => return Err(RpcError::invalid_params("params are given by name")),
    };
    match object.and_then(|object| object.get(name)) {
        Some(Json::String(text)) => Ok(Some(text)),
        Some(_) => Err(RpcError::invalid_params(format!("\"{name}\" is a string"))),
        None if required => Err(RpcError::invalid_params(format!("\"{name}\" is missing"))),
        None => Ok(None),
    }
}

/// A query's value as JSON: a map an object, its names in order; bytes in base64
pub fn json(value: &Value) -> Json {
    match value {
        Value::Text(text) => json!(text),
        Value::Int(n) => json!(n),
        Value::Bool(yes) => json!(yes),
        Value::List(items) => items.iter().map(json).collect(),
        Value::Bytes(bytes) => json!(BASE64_STANDARD.encode(bytes)),
        Value::Map(entries) => (entries.iter())
            .map(|(name, value)| (name.clone(), json(value)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    }
}

/// What a connection tells the loop, by its number
enum Message {
    /// A connection, and where its replies go
    Opened(u64, Sender<Outgoing>),
    Request(u64, Request),
    Closed(u64),
}

/// What a connection's writer does next
enum Outgoing {
    /// Writes a line
    Line(String),
    /// Stops, the lines before written: the connection closes when its reader is done too.
    /// Dropping a stream doesn't shut it down while another thread holds it
    Close,
}

/// The server while it runs: dropping it stops it and removes its socket file. Connections
/// still open then end when their client sends or closes
pub struct Server {
    /// The socket file on Unix, removed on drop
    path: Option<PathBuf>,
    location: String,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    messages: Receiver<Message>,
    /// Each connection's way back
    replies: HashMap<u64, Sender<Outgoing>>,
    /// What each subscribed connection is told (MAX-284)
    subscriptions: HashMap<u64, Subscription>,
    /// What the subscribers were told of: set on the first pass, the events before it being
    /// in the log for `since`
    published: Option<Published>,
}

/// What the subscribers were last told of (MAX-284): the last event's number, the end of the
/// bytes received, the count of bytes sent
#[derive(Debug, Clone, Copy)]
struct Published {
    event: u64,
    rx: u64,
    tx: usize,
}

/// What a connection subscribed to (MAX-284): event kinds, `rx` for the bytes received, `tx`
/// for the bytes sent
#[derive(Debug, Clone, PartialEq, Eq)]
struct Subscription {
    kinds: Vec<&'static str>,
    rx: bool,
    tx: bool,
}

impl Subscription {
    /// From `subscribe`'s params: `events`, kinds of `event::KINDS`, `rx` and `tx`, every one
    /// when left out
    fn from_params(params: Option<&Json>) -> Result<Self, RpcError> {
        let object = match params {
            None => None,
            Some(Json::Object(object)) => Some(object),
            Some(_) => return Err(RpcError::invalid_params("params are given by name")),
        };
        let Some(names) = object.and_then(|object| object.get("events")) else {
            return Ok(Self {
                kinds: KINDS.to_vec(),
                rx: true,
                tx: true,
            });
        };
        let Json::Array(names) = names else {
            return Err(RpcError::invalid_params("\"events\" is a list of names"));
        };
        let mut subscription = Self::none();
        for name in names {
            match name.as_str() {
                Some("rx") => subscription.rx = true,
                Some("tx") => subscription.tx = true,
                Some(name) => match KINDS.iter().find(|kind| **kind == name) {
                    Some(kind) => subscription.kinds.push(kind),
                    None => return Err(RpcError::invalid_params(format!("no event '{name}'"))),
                },
                None => return Err(RpcError::invalid_params("an event is named by a string")),
            }
        }
        Ok(subscription)
    }

    fn none() -> Self {
        Self {
            kinds: Vec::new(),
            rx: false,
            tx: false,
        }
    }

    fn wants(&self, event: &Event) -> bool {
        self.kinds.contains(&event.kind())
    }

    /// What `other` names added: the kinds kept in the order of `KINDS`
    fn add(&mut self, other: &Self) {
        self.kinds = (KINDS.iter().copied())
            .filter(|kind| self.kinds.contains(kind) || other.kinds.contains(kind))
            .collect();
        self.rx |= other.rx;
        self.tx |= other.tx;
    }

    /// What `other` names taken off
    fn remove(&mut self, other: &Self) {
        self.kinds.retain(|kind| !other.kinds.contains(kind));
        self.rx &= !other.rx;
        self.tx &= !other.tx;
    }

    fn is_empty(&self) -> bool {
        self.kinds.is_empty() && !self.rx && !self.tx
    }

    /// The names it holds, as `events` takes them: the kinds, then `rx` and `tx`
    fn names(&self) -> Vec<&'static str> {
        let rx = self.rx.then_some("rx");
        let tx = self.tx.then_some("tx");
        self.kinds.iter().copied().chain(rx).chain(tx).collect()
    }
}

/// A logged event as a subscriber gets it, the params of an `event` notification: its number,
/// its time, its kind and its fields
pub fn event_json(logged: &Logged) -> Json {
    let mut object = serde_json::Map::new();
    object.insert("seq".to_string(), json!(logged.seq));
    object.insert(
        "time".to_string(),
        json!(crate::commands::query::utc(logged.time)),
    );
    if let Ok(Json::Object(fields)) = serde_json::to_value(&logged.event) {
        object.extend(fields);
    }
    Json::Object(object)
}

impl Server {
    /// Listens as `name`: the socket file `<dir>/<name>.sock` on Unix, after the files of
    /// maxicoms gone are removed; the pipe `\\.\pipe\<name>` on Windows
    pub fn start(dir: Option<&Path>, name: &str) -> io::Result<Self> {
        let (listener_name, path, location) = socket_name(dir, name)?;
        let options = ListenerOptions::new()
            .name(listener_name)
            .nonblocking(ListenerNonblockingMode::Accept);
        // the owner only: another user can't connect. On Unix the socket folder is the owner's
        // only already (`socket_name`); the file's mode too where `interprocess` can set it
        // (not macOS)
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt;
            options.security_descriptor(windows::owner_only()?)
        };
        let listener = options.create_sync()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (to_loop, messages) = mpsc::channel();
        let accept = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("api-accept".to_string())
                .spawn(move || accept_loop(&listener, &stop, &to_loop))?
        };
        Ok(Self {
            path,
            location,
            stop,
            accept: Some(accept),
            messages,
            replies: HashMap::new(),
            subscriptions: HashMap::new(),
            published: None,
        })
    }

    /// Where it listens: the socket file, or the pipe
    pub fn location(&self) -> &str {
        &self.location
    }

    /// The requests that came in since the last call, by connection; the connections opened
    /// and closed meanwhile are taken into account
    pub fn requests(&mut self) -> Vec<(u64, Request)> {
        let mut requests = Vec::new();
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Opened(conn, replies) => {
                    self.replies.insert(conn, replies);
                }
                Message::Request(conn, request) => requests.push((conn, request)),
                Message::Closed(conn) => {
                    self.replies.remove(&conn);
                    self.subscriptions.remove(&conn);
                }
            }
        }
        requests
    }

    /// Sends a reply line to a connection; nothing when it closed meanwhile
    pub fn reply(&mut self, conn: u64, line: String) {
        if let Some(replies) = self.replies.get(&conn)
            && replies.send(Outgoing::Line(line)).is_err()
        {
            self.replies.remove(&conn);
            self.subscriptions.remove(&conn);
        }
    }

    /// The loop's pass (MAX-284): the requests answered with the app in hand, then the
    /// subscribers told of the events emitted and the bytes received since the last pass.
    /// Returns how many requests came in
    pub fn serve(&mut self, app: &mut App, instance: &Instance) -> usize {
        let now = Published {
            event: app.events.last(),
            rx: app.terminal_mode.dump().stream().end(),
            tx: app.stats.tx(),
        };
        let told = *self.published.get_or_insert(now);
        let published = told.event;
        let requests = self.requests();
        let count = requests.len();
        for (conn, request) in requests {
            let reply = match request.method.as_str() {
                "subscribe" => self.subscribe(conn, app, published, &request),
                "unsubscribe" => self.unsubscribe(conn, &request),
                _ => answer(app, instance, &request),
            };
            if let Some(reply) = reply {
                self.reply(conn, reply);
            }
        }
        let events: Vec<&Logged> = app.events.after(published).collect();
        for logged in &events {
            self.tell(
                |subscription| subscription.wants(&logged.event),
                || rpc::notification("event", event_json(logged)),
            );
        }
        if now.rx != told.rx {
            self.tell(
                |subscription| subscription.rx,
                || rpc::notification("rx", json!({ "newest": now.rx })),
            );
        }
        if now.tx != told.tx {
            self.tell(
                |subscription| subscription.tx,
                || rpc::notification("tx", json!({ "sent": now.tx })),
            );
        }
        self.published = Some(Published {
            event: app.events.last(),
            ..now
        });
        count
    }

    /// `subscribe {events?, since?}` (MAX-284): this connection told of `events` from now on,
    /// added to what it subscribed to before. The reply gives the last event's number, the
    /// oldest kept, the end of the bytes received and every name subscribed to now; with
    /// `since`, the events kept after that number of the kinds just added follow it, up to
    /// `published` (the next pass sends the rest): the kinds it had were sent already
    fn subscribe(
        &mut self,
        conn: u64,
        app: &App,
        published: u64,
        request: &Request,
    ) -> Option<String> {
        let id = request.id.as_ref()?;
        let params = request.params.as_ref();
        let since = match params.and_then(|params| params.get("since")) {
            None => None,
            Some(since) => match since.as_u64() {
                Some(since) => Some(since),
                None => {
                    let error = RpcError::invalid_params("\"since\" is an event's number");
                    return Some(rpc::reply(id, Err(error)));
                }
            },
        };
        let asked = match Subscription::from_params(params) {
            Ok(asked) => asked,
            Err(error) => return Some(rpc::reply(id, Err(error))),
        };
        let had = self.subscriptions.get(&conn).cloned();
        let mut subscription = had.clone().unwrap_or_else(Subscription::none);
        subscription.add(&asked);
        let result = json!({
            "last": app.events.last(),
            "oldest": app.events.oldest(),
            "rx": app.terminal_mode.dump().stream().end(),
            "tx": app.stats.tx(),
            "events": subscription.names(),
        });
        self.reply(conn, rpc::reply(id, Ok(result)));
        if let Some(since) = since {
            let missed = (app.events.after(since))
                .take_while(|logged| logged.seq <= published)
                .filter(|logged| asked.wants(&logged.event))
                .filter(|logged| !had.as_ref().is_some_and(|had| had.wants(&logged.event)));
            for logged in missed {
                self.reply(conn, rpc::notification("event", event_json(logged)));
            }
        }
        self.subscriptions.insert(conn, subscription);
        None
    }

    /// `unsubscribe {events?}` (MAX-284): `events` taken off what this connection subscribed
    /// to, the others kept; left out, every one. The reply gives the names still subscribed to
    fn unsubscribe(&mut self, conn: u64, request: &Request) -> Option<String> {
        let result = Subscription::from_params(request.params.as_ref()).map(|asked| {
            let Some(subscription) = self.subscriptions.get_mut(&conn) else {
                return json!({ "events": [] });
            };
            subscription.remove(&asked);
            let left = subscription.names();
            if subscription.is_empty() {
                self.subscriptions.remove(&conn);
            }
            json!({ "events": left })
        });
        (request.id.as_ref()).map(|id| rpc::reply(id, result))
    }

    /// Sends a notification to the subscribers `wanted` picks, made once
    fn tell(&mut self, wanted: impl Fn(&Subscription) -> bool, line: impl Fn() -> String) {
        let conns: Vec<u64> = (self.subscriptions.iter())
            .filter(|(_, subscription)| wanted(subscription))
            .map(|(conn, _)| *conn)
            .collect();
        if conns.is_empty() {
            return;
        }
        let line = line();
        for conn in conns {
            self.reply(conn, line.clone());
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // each writer stops: a connection closes once its client sends or leaves
        for replies in self.replies.values() {
            let _ = replies.send(Outgoing::Close);
        }
        // the listener goes with its thread, its socket file with it
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The listener's name, its socket file if any, and where it is said to listen
fn socket_name(
    dir: Option<&Path>,
    name: &str,
) -> io::Result<(Name<'static>, Option<PathBuf>, String)> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        use std::os::unix::fs::PermissionsExt;
        let dir = dir.ok_or_else(|| io::Error::other("no folder for the socket"))?;
        std::fs::create_dir_all(dir)?;
        // the owner's only: another user can't reach the socket in it, on every Unix (some
        // ignore a socket file's own mode on connect)
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        remove_stale_sockets(dir);
        let path = dir.join(format!("{name}{SUFFIX}"));
        let listener_name = path.clone().to_fs_name::<GenericFilePath>()?;
        let location = path.display().to_string();
        Ok((listener_name, Some(path), location))
    }
    #[cfg(not(unix))]
    {
        use interprocess::local_socket::GenericNamespaced;
        let _ = dir;
        let listener_name = name.to_string().to_ns_name::<GenericNamespaced>()?;
        Ok((listener_name, None, format!(r"\\.\pipe\{name}")))
    }
}

/// Removes the socket files no maxicom listens on any more: a maxicom killed leaves its file,
/// and a connect to it is refused
#[cfg(unix)]
fn remove_stale_sockets(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(PREFIX) && file_name.ends_with(SUFFIX) && !is_alive(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Accepts the connections until told to stop, each served by its own threads
fn accept_loop(
    listener: &interprocess::local_socket::Listener,
    stop: &AtomicBool,
    to_loop: &Sender<Message>,
) {
    let next = AtomicU64::new(1);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            // blocking reads: macOS (BSD) gives an accepted stream the listener's non-blocking
            // mode, and a read would end the connection at once
            Ok(stream) if stream.set_nonblocking(false).is_err() => drop(stream),
            Ok(stream) => {
                let conn = next.fetch_add(1, Ordering::Relaxed);
                let to_loop = to_loop.clone();
                let spawned = thread::Builder::new()
                    .name(format!("api-conn-{conn}"))
                    .spawn(move || serve(conn, stream, &to_loop));
                // no thread for it: the stream dropped closes it
                drop(spawned);
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(_) => thread::sleep(ACCEPT_POLL),
        }
    }
}

/// One connection: its lines read into requests for the loop, its replies written by a thread
/// of their own, so a reply never waits for the client to send
fn serve(conn: u64, stream: Stream, to_loop: &Sender<Message>) {
    // `&Stream` reads and writes: the writer shares it
    let stream = Arc::new(stream);
    let (replies, to_write) = mpsc::channel::<Outgoing>();
    let sending = Arc::clone(&stream);
    let writer = thread::Builder::new()
        .name(format!("api-write-{conn}"))
        .spawn(move || {
            let mut send = &*sending;
            for outgoing in to_write {
                let Outgoing::Line(line) = outgoing else {
                    break;
                };
                let written = (send.write_all(line.as_bytes()))
                    .and_then(|()| send.write_all(b"\n"))
                    .and_then(|()| send.flush());
                if written.is_err() {
                    break;
                }
            }
        });
    if writer.is_err()
        || to_loop
            .send(Message::Opened(conn, replies.clone()))
            .is_err()
    {
        return;
    }
    let mut reader = BufReader::new(&*stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        let mut limited = (&mut reader).take(rpc::MAX_LINE as u64);
        match limited.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.last() != Some(&b'\n') && line.len() >= rpc::MAX_LINE {
            let error = RpcError::parse(format!("a line is {} bytes at most", rpc::MAX_LINE));
            let _ = replies.send(Outgoing::Line(rpc::reply(&Json::Null, Err(error))));
            break;
        }
        let text = line.trim_ascii();
        if text.is_empty() {
            continue;
        }
        match rpc::parse(text) {
            Ok(request) => {
                if to_loop.send(Message::Request(conn, request)).is_err() {
                    break;
                }
            }
            Err(refusal) => {
                let refused = rpc::reply(&refusal.id, Err(refusal.error));
                let _ = replies.send(Outgoing::Line(refused));
                if refusal.close {
                    break;
                }
            }
        }
    }
    let _ = replies.send(Outgoing::Close);
    let _ = to_loop.send(Message::Closed(conn));
}

#[cfg(test)]
#[path = "tests/api.rs"]
mod tests;
