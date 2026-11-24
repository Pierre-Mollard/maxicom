//! The local API (MAX-74): other programs connect to this maxicom through a local socket (a Unix
//! socket file, a named pipe on Windows) and talk JSON-RPC 2.0, one message per line (`rpc`).
//!
//! Threads only move lines: one accepts the connections, each connection has a reader and a
//! writer. The requests reach the loop through a channel it drains on every pass, and the loop
//! answers them with the app in hand: no lock on the app. The methods are A2.1's (MAX-284);
//! `hello` says who this maxicom is
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

use interprocess::local_socket::{
    ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*,
};
use jiff::Timestamp;
use serde_json::{Value as Json, json};

use crate::app::App;

pub mod rpc;

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

/// The answer to a request, for the loop: none for a notification
pub fn answer(app: &App, instance: &Instance, request: &Request) -> Option<String> {
    let result = match request.method.as_str() {
        "hello" => Ok(json!({
            "api": API_VERSION,
            "version": env!("CARGO_PKG_VERSION"),
            "pid": instance.pid,
            "link": app.link_label(),
            "since": crate::commands::query::utc(instance.since),
        })),
        method => Err(RpcError::method_not_found(method)),
    };
    (request.id.as_ref()).map(|id| rpc::reply(id, result))
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
}

impl Server {
    /// Listens as `name`: the socket file `<dir>/<name>.sock` on Unix, after the files of
    /// maxicoms gone are removed; the pipe `\\.\pipe\<name>` on Windows
    pub fn start(dir: Option<&Path>, name: &str) -> io::Result<Self> {
        let (listener_name, path, location) = socket_name(dir, name)?;
        let options = ListenerOptions::new()
            .name(listener_name)
            .nonblocking(ListenerNonblockingMode::Accept);
        // the owner only: another user can't connect
        #[cfg(unix)]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
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
        let dir = dir.ok_or_else(|| io::Error::other("no folder for the socket"))?;
        std::fs::create_dir_all(dir)?;
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
    use interprocess::local_socket::GenericFilePath;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !(file_name.starts_with(PREFIX) && file_name.ends_with(SUFFIX)) {
            continue;
        }
        let alive = (path.clone().to_fs_name::<GenericFilePath>())
            .and_then(Stream::connect)
            .is_ok();
        if !alive {
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
