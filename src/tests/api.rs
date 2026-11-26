//! The API socket itself (MAX-74): a server in a temporary folder on Unix, a pipe of its own
//! name on Windows; the clients are `interprocess` streams, as another program's would be
use super::*;

/// `hello`'s answer, with no socket: the API's version, this maxicom's pid, its link, its start
/// time; an unknown method is refused; a notification gets no reply.
/// How: the test app, a request of each kind.
#[test]
fn hello_says_who_answers() {
    let mut app = crate::app::tests::app();
    let instance = Instance::now();
    let request = |id: Option<Json>, method: &str| Request {
        id,
        method: method.to_string(),
        params: None,
    };
    let hello = answer(&mut app, &instance, &request(Some(json!(1)), "hello")).unwrap();
    let hello: Json = serde_json::from_str(&hello).unwrap();
    let result = &hello["result"];
    assert_eq!(result["api"], json!(API_VERSION));
    assert_eq!(result["pid"], json!(std::process::id()));
    assert_eq!(result["link"], json!(app.link_label()));
    assert_eq!(
        result["since"],
        json!(crate::commands::query::utc(instance.since))
    );
    let unknown = answer(&mut app, &instance, &request(Some(json!(2)), "nope")).unwrap();
    assert!(
        unknown.contains(&rpc::METHOD_NOT_FOUND.to_string()),
        "{unknown}"
    );
    assert_eq!(answer(&mut app, &instance, &request(None, "hello")), None);
    // what clients may do, as `[api]` sets it (MAX-287): the defaults
    assert_eq!(
        result["access"],
        json!({ "read": "on", "control": "on", "device": "off" })
    );
}

/// The reply to `method` with `params`, as JSON
fn ask(app: &mut App, method: &str, params: Json) -> Json {
    let request = Request {
        id: Some(json!(1)),
        method: method.to_string(),
        params: Some(params),
    };
    let reply = answer(app, &Instance::now(), &request).unwrap();
    serde_json::from_str(&reply).unwrap()
}

/// MAX-284 (step 1): `hello` takes the client's name, a string; params given by position or
/// a name that isn't a string are refused as invalid params.
/// How: `hello` with a name, a number for it, an array of params.
#[test]
fn hello_takes_a_client_name() {
    let mut app = crate::app::tests::app();
    let named = ask(&mut app, "hello", json!({"client": "script"}));
    assert_eq!(named["result"]["api"], json!(API_VERSION));
    let number = ask(&mut app, "hello", json!({"client": 1}));
    assert_eq!(
        number["error"]["code"],
        json!(rpc::INVALID_PARAMS),
        "{number}"
    );
    let array = ask(&mut app, "hello", json!(["script"]));
    assert_eq!(
        array["error"]["code"],
        json!(rpc::INVALID_PARAMS),
        "{array}"
    );
}

/// MAX-284 (step 1): `query` runs a getter's command and gives its value as JSON, the same as
/// the TUI's: `status` an object in its order, a popup command (`marks`) its value; a named
/// command of one query runs too.
/// How: the test app; `status` against the query function itself, `marks`, `commands
/// status`, a `[commands]` entry naming `status`.
#[test]
fn query_gives_the_value_as_json() {
    let mut app = crate::app::tests::app();
    let status = ask(&mut app, "query", json!({"command": "status"}));
    let expected = crate::commands::query::status(&mut app, &[]).unwrap();
    assert_eq!(status["result"], json(&expected), "{status}");
    let names: Vec<&String> = status["result"].as_object().unwrap().keys().collect();
    let Value::Map(entries) = &expected else {
        panic!("status is a map")
    };
    assert_eq!(
        names,
        entries.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    let marks = ask(&mut app, "query", json!({"command": "marks"}));
    assert!(
        marks["result"].is_object() || marks["result"].is_array(),
        "{marks}"
    );
    let commands = ask(&mut app, "query", json!({"command": "commands status"}));
    assert_eq!(commands["result"]["type"], json!("query"), "{commands}");
    app.key_resolver.commands = vec![crate::commands::NamedCommand {
        name: "state".to_string(),
        run: "status".to_string(),
        help: String::new(),
        invocations: crate::commands::parse("status").unwrap(),
    }];
    let named = ask(&mut app, "query", json!({"command": "state"}));
    assert_eq!(named["result"], status["result"]);
}

/// MAX-284 (step 1): `query` refuses what isn't one query, each with its code: a command that
/// doesn't parse (unknown command, missing argument), an action or two queries (not a
/// query), a getter that fails, no `command` (invalid params).
/// How: one request of each.
#[test]
fn query_refusals() {
    let mut app = crate::app::tests::app();
    let code = |app: &mut App, params: Json| ask(app, "query", params)["error"]["code"].clone();
    assert_eq!(
        code(&mut app, json!({"command": "nope"})),
        json!(rpc::UNKNOWN_COMMAND)
    );
    assert_eq!(
        code(&mut app, json!({"command": "send"})),
        json!(rpc::MISSING_ARGUMENT)
    );
    assert_eq!(
        code(&mut app, json!({"command": "quit"})),
        json!(rpc::NOT_A_QUERY)
    );
    assert_eq!(
        code(&mut app, json!({"command": "status; status"})),
        json!(rpc::NOT_A_QUERY)
    );
    assert_eq!(
        code(&mut app, json!({"command": "commands nope"})),
        json!(rpc::COMMAND_FAILED)
    );
    assert_eq!(code(&mut app, json!({})), json!(rpc::INVALID_PARAMS));
}

/// MAX-284 (step 1): every kind of value turns into its JSON: texts, numbers, yes or no,
/// lists, maps in their order (not sorted), nested; bytes in base64 (step 2a).
/// How: a value of every kind.
#[test]
fn values_as_json() {
    let value = Value::Map(vec![
        ("z".to_string(), Value::text("a\nb")),
        ("a".to_string(), Value::Int(-3)),
        ("m".to_string(), Value::Bool(true)),
        (
            "list".to_string(),
            Value::List(vec![Value::Int(1), Value::Map(vec![])]),
        ),
        ("bytes".to_string(), Value::Bytes(b"\x1b[1m\xff".to_vec())),
    ]);
    assert_eq!(
        serde_json::to_string(&json(&value)).unwrap(),
        r#"{"z":"a\nb","a":-3,"m":true,"list":[1,{}],"bytes":"G1sxbf8="}"#
    );
}

/// MAX-284 (step 2b): `subscribe`'s `events` names kinds of `KINDS` and streams of `STREAMS`
/// (MAX-290); left out, every one; an unknown name, a name that isn't a string, a list that
/// isn't one, params by position are refused.
/// How: each form of params.
#[test]
fn subscription_params() {
    let all = Subscription::from_params(None).unwrap();
    assert_eq!(
        (all.kinds.len(), all.streams),
        (KINDS.len(), STREAMS.to_vec())
    );
    let some = Subscription::from_params(Some(&json!({"events": ["link-lost", "rx"]}))).unwrap();
    assert_eq!(
        some,
        Subscription {
            kinds: vec!["link-lost"],
            streams: vec!["rx"],
        }
    );
    assert!(some.wants(&Event::LinkLost { link: "Serial" }));
    assert!(!some.wants(&Event::ReplayFinished));
    for params in [
        json!({"events": ["nope"]}),
        json!({"events": [1]}),
        json!({"events": "rx"}),
        json!(["rx"]),
    ] {
        let error = Subscription::from_params(Some(&params)).unwrap_err();
        assert_eq!(error.code, rpc::INVALID_PARAMS, "{params}");
    }
}

/// MAX-284 (step 2b, review change): a subscription grows by the names added, kinds kept in
/// the order of `KINDS`, none twice; it loses only the names taken off; its names are the
/// kinds then `rx`.
/// How: `link-lost` + `rx`, then `link-idle` and `link-lost` again added, then `link-lost`
/// and `rx` taken off.
#[test]
fn subscription_add_and_remove() {
    let named =
        |events: Json| Subscription::from_params(Some(&json!({ "events": events }))).unwrap();
    let mut subscription = named(json!(["link-lost", "rx"]));
    subscription.add(&named(json!(["link-lost", "link-idle"])));
    let mut kinds = vec!["link-lost", "link-idle"];
    kinds.sort_by_key(|kind| KINDS.iter().position(|known| known == kind));
    assert_eq!(subscription.kinds, kinds);
    assert_eq!(subscription.names(), [kinds.as_slice(), &["rx"]].concat());
    subscription.remove(&named(json!(["link-lost", "rx"])));
    assert_eq!(subscription.names(), ["link-idle"]);
    assert!(!subscription.is_empty());
    subscription.remove(&Subscription::from_params(None).unwrap());
    assert!(subscription.is_empty());
}

/// MAX-284 (step 2b): an event as a subscriber gets it: its number, its time (UTC, to the
/// millisecond), its kind, its fields.
/// How: a logged `link-opened`.
#[test]
fn event_as_json() {
    let mut log = crate::event::EventLog::default();
    log.push(Event::LinkOpened {
        label: "/dev/ttyUSB0".to_string(),
        how: crate::event::Opened::Connected,
    });
    let logged = log.after(0).next().unwrap();
    assert_eq!(
        event_json(logged),
        json!({
            "seq": 1,
            "time": crate::commands::query::utc(logged.time),
            "kind": "link-opened",
            "label": "/dev/ttyUSB0",
            "how": "connected",
        })
    );
}

mod socket {
    use std::{
        io::{BufRead, BufReader, Write},
        time::Instant,
    };

    use super::*;

    /// A test's server: its own folder on Unix (removed after), its own pipe name on Windows
    struct TestServer {
        server: Server,
        dir: Option<PathBuf>,
    }

    impl TestServer {
        /// Unix: a short socket name in a folder of the test's own, the path kept under
        /// macOS' 104 bytes (its temporary folder is long). Windows: a pipe name of its own
        fn start(test: &str) -> Self {
            let unique = format!("{}-{test}", std::process::id());
            let dir = cfg!(unix).then(|| {
                let dir = std::env::temp_dir().join(format!("mx-{unique}"));
                let _ = std::fs::remove_dir_all(&dir);
                dir
            });
            let name = if cfg!(unix) {
                "maxicom-t".to_string()
            } else {
                format!("maxicom-test-{unique}")
            };
            let server = Server::start(dir.as_deref(), &name).unwrap();
            Self { server, dir }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(dir) = &self.dir {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    impl std::ops::Deref for TestServer {
        type Target = Server;
        fn deref(&self) -> &Server {
            &self.server
        }
    }

    impl std::ops::DerefMut for TestServer {
        fn deref_mut(&mut self) -> &mut Server {
            &mut self.server
        }
    }

    /// A client, as another program's: an `interprocess` stream. Reads time out where the
    /// system allows it (not on Windows pipes: a test hanging there fails on CI's timeout)
    struct Client {
        reader: BufReader<Stream>,
    }

    impl Client {
        fn connect(server: &Server) -> Self {
            use interprocess::local_socket::GenericFilePath;
            let name = server.location().to_fs_name::<GenericFilePath>().unwrap();
            let stream = Stream::connect(name).unwrap();
            let _ = stream.set_recv_timeout(Some(Duration::from_secs(5)));
            Self {
                reader: BufReader::new(stream),
            }
        }

        /// Sends bytes as they are: a write error is the server having closed, which the
        /// reads that follow show
        fn send_bytes(&self, bytes: &[u8]) {
            let _ = self.reader.get_ref().write_all(bytes);
        }

        fn send(&self, line: &str) {
            self.send_bytes(format!("{line}\n").as_bytes());
        }

        fn reply(&mut self) -> Json {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        }

        /// The server closed: the end, or a reset (Unix) or a broken pipe (Windows) when it
        /// closed with bytes left unread (the line too long)
        fn closed(&mut self) -> bool {
            let mut rest = String::new();
            match self.reader.read_line(&mut rest) {
                Ok(n) => n == 0,
                Err(err) => matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ),
            }
        }
    }

    /// Runs the loop's part until `count` requests were answered
    fn answer_requests(server: &mut Server, count: usize) {
        let mut app = crate::app::tests::app();
        let instance = Instance::now();
        let start = Instant::now();
        let mut answered = 0;
        while answered < count {
            assert!(start.elapsed() < Duration::from_secs(5), "no request");
            for (conn, request) in server.requests() {
                if let Some(reply) = answer(&mut app, &instance, &request) {
                    server.reply(conn, reply);
                }
                answered += 1;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// MAX-287 (step 6): `disconnect_all` closes every client's connection while its reader
    /// waits for a line: each client sees it closed at once, without sending; the server goes
    /// on listening, a new client answered.
    /// How: two clients answered once, dropped, then a third one's `hello`.
    #[test]
    fn disconnect_all_closes_the_clients() {
        let mut server = TestServer::start("drop");
        let mut one = Client::connect(&server);
        let mut two = Client::connect(&server);
        one.send(r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#);
        two.send(r#"{"jsonrpc":"2.0","id":2,"method":"hello"}"#);
        answer_requests(&mut server, 2);
        one.reply();
        two.reply();
        assert_eq!(server.disconnect_all(), 2);
        assert!(one.closed(), "the first client still connected");
        assert!(two.closed(), "the second client still connected");
        assert_eq!(server.disconnect_all(), 0);

        let mut three = Client::connect(&server);
        three.send(r#"{"jsonrpc":"2.0","id":3,"method":"hello"}"#);
        answer_requests(&mut server, 1);
        assert_eq!(three.reply()["id"], json!(3));
    }

    /// MAX-74 (steps 1-2): two clients connect at once, each gets its `hello` answered on its
    /// own connection, while its reader waits for its next line.
    /// How: a test server, two clients.
    #[test]
    fn two_clients_say_hello() {
        let mut server = TestServer::start("hello");
        let mut one = Client::connect(&server);
        let mut two = Client::connect(&server);
        one.send(r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#);
        two.send(r#"{"jsonrpc":"2.0","id":"b","method":"hello"}"#);
        answer_requests(&mut server, 2);
        let (first, second) = (one.reply(), two.reply());
        assert_eq!((&first["id"], &second["id"]), (&json!(1), &json!("b")));
        assert_eq!(first["result"]["api"], json!(API_VERSION));
    }

    /// MAX-74 (step 1): the socket's folder is the owner's only, and the socket file too where
    /// its mode can be set (not macOS); the file goes when the server stops.
    /// How: a test server's folder and file modes, then the file after the server is dropped.
    #[cfg(unix)]
    #[test]
    fn socket_file_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let mut server = TestServer::start("file");
        let path = PathBuf::from(server.location());
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
        assert_eq!(mode(&path), 0o600);
        let dir = server.dir.take();
        drop(server);
        assert!(!path.exists(), "the socket file stays");
        let _ = dir.map(std::fs::remove_dir_all);
    }

    /// MAX-287 (step 1): the socket folder is the runtime folder's `maxicom`, else
    /// `maxicom-<uid>` in the temporary folder, both local to this computer; never the cache
    /// folder, which a home on NFS shares with other computers.
    /// How: the folder with and without a runtime folder.
    #[cfg(unix)]
    #[test]
    fn socket_folder_is_this_computers() {
        let temp = Path::new("/tmp");
        let runtime = Some(PathBuf::from("/run/user/1000"));
        assert_eq!(
            socket_dir_in(runtime, temp, 1000),
            Path::new("/run/user/1000/maxicom")
        );
        assert_eq!(
            socket_dir_in(None, temp, 1000),
            Path::new("/tmp/maxicom-1000")
        );
    }

    /// MAX-287 (step 1): a socket folder already there is used only when it is a folder of
    /// this user's that other users can't write to (the temporary folder is everyone's), then
    /// made the owner's only; a link or a folder others can write to is refused, with no
    /// socket made. Another user's folder is refused too (not tested: it needs a second user).
    /// How: a server started in a link to a folder, in a folder open to all, in a 0755 folder.
    #[cfg(unix)]
    #[test]
    fn socket_folder_checked() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("mx-{}-own", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let set_mode = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        let target = base.join("target");
        std::fs::create_dir(&target).unwrap();
        set_mode(&target, 0o700);
        let link = base.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let refused = Server::start(Some(&link), "maxicom-t").err().unwrap();
        assert!(refused.to_string().ends_with("isn't a folder"), "{refused}");

        let open = base.join("open");
        std::fs::create_dir(&open).unwrap();
        set_mode(&open, 0o777);
        let refused = Server::start(Some(&open), "maxicom-t").err().unwrap();
        assert!(
            refused.to_string().starts_with("other users can write"),
            "{refused}"
        );
        assert!(!open.join("maxicom-t.sock").exists());

        let readable = base.join("readable");
        std::fs::create_dir(&readable).unwrap();
        set_mode(&readable, 0o755);
        let server = Server::start(Some(&readable), "maxicom-t").unwrap();
        assert_eq!(mode(&readable), 0o700);
        drop(server);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// MAX-74 (step 2): the pipe goes when the server stops: a client can't connect any more.
    /// How: a test server's pipe name, connected to after the server is dropped.
    #[cfg(windows)]
    #[test]
    fn pipe_gone_after_stop() {
        use interprocess::local_socket::GenericFilePath;
        let server = TestServer::start("gone-pipe");
        let location = server.location().to_string();
        drop(server);
        let name = location.to_fs_name::<GenericFilePath>().unwrap();
        assert!(Stream::connect(name).is_err(), "the pipe stays");
    }

    /// MAX-74 (step 1): a client that leaves before its reply changes nothing for the
    /// others: its reply goes nowhere, the next client is answered.
    /// How: a `hello` sent then the client dropped, answered; then another client's `hello`.
    #[test]
    fn client_gone_before_its_reply() {
        let mut server = TestServer::start("gone");
        let gone = Client::connect(&server);
        gone.send(r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#);
        drop(gone);
        answer_requests(&mut server, 1);
        let mut next = Client::connect(&server);
        next.send(r#"{"jsonrpc":"2.0","id":2,"method":"hello"}"#);
        answer_requests(&mut server, 1);
        assert_eq!(next.reply()["id"], json!(2));
    }

    /// MAX-74 (step 1): a line that isn't JSON gets a parse error and the connection closes; a
    /// line too long too; an invalid request and an unknown method keep it open.
    /// How: one client per case.
    #[test]
    fn bad_lines() {
        let mut server = TestServer::start("bad");

        let mut client = Client::connect(&server);
        client.send("not json");
        assert_eq!(client.reply()["error"]["code"], json!(rpc::PARSE_ERROR));
        assert!(client.closed());

        let mut client = Client::connect(&server);
        client.send_bytes(&vec![b'x'; rpc::MAX_LINE + 10]);
        assert_eq!(client.reply()["error"]["code"], json!(rpc::PARSE_ERROR));
        assert!(client.closed());

        let mut client = Client::connect(&server);
        client.send(r#"{"jsonrpc":"2.0","id":1}"#);
        assert_eq!(client.reply()["error"]["code"], json!(rpc::INVALID_REQUEST));
        client.send(r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#);
        answer_requests(&mut server, 1);
        let unknown = client.reply();
        assert_eq!(
            (&unknown["id"], &unknown["error"]["code"]),
            (&json!(2), &json!(rpc::METHOD_NOT_FOUND))
        );
    }

    /// MAX-74 (step 3): the running maxicoms are found by their sockets, each with its pid
    /// and the link it holds by the lock files; a dead one's socket file, another socket name
    /// and a lock left by a crash aren't.
    /// How: a server named as this process' instance holding a TCP link's lock in a temporary
    /// lock folder, a lock file with an `.info` nobody holds; on Unix a stale socket file and
    /// a live test server of another name in the same folder.
    #[test]
    fn running_found_with_their_link() {
        use crate::link::lock::LinkLock;
        let me = Instance::now();
        let server = TestServer::start("running");
        let live = Server::start(server.dir.as_deref(), &me.name()).unwrap();
        let locks = server.dir.clone().unwrap_or_else(|| {
            let dir = std::env::temp_dir().join(format!("maxicom-test-{}-locks", me.pid));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        });
        let locks = locks.join("locks");
        std::fs::create_dir_all(&locks).unwrap();
        let address = "tcp://10.0.0.3:4000".parse().unwrap();
        let _lock = LinkLock::take_in(&locks, &address).ok().unwrap();
        std::fs::write(locks.join("gone.lock"), "").unwrap();
        std::fs::write(locks.join("gone.info"), "pid=1\naddress=tcp://gone:1\n").unwrap();
        #[cfg(unix)]
        {
            let dir = server.dir.as_deref().unwrap();
            drop(std::os::unix::net::UnixListener::bind(dir.join("maxicom-1.sock")).unwrap());
        }

        let found = running(server.dir.as_deref(), Some(&locks));
        let mine: Vec<_> = found.iter().filter(|found| found.pid == me.pid).collect();
        assert_eq!(mine.len(), 1, "{found:?}");
        assert_eq!(mine[0].location, live.location());
        assert_eq!(mine[0].link.as_deref(), Some("tcp://10.0.0.3:4000"));
        assert!(found.iter().all(|found| found.pid != 1), "{found:?}");
        #[cfg(unix)]
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(crate::link::lock::held_locks(&locks).len(), 1);
        drop(live);
        assert!(
            running(server.dir.as_deref(), Some(&locks))
                .iter()
                .all(|found| found.pid != me.pid)
        );
        drop(server);
        let _ = std::fs::remove_dir_all(locks);
    }

    /// MAX-74 (step 1): at start, the socket files no maxicom listens on are removed, the
    /// live ones and the other files kept.
    /// How: a socket file left by a listener dropped (std leaves its file), a live server's
    /// file, an unrelated file; then a second server starting in the same folder.
    #[cfg(unix)]
    #[test]
    fn stale_sockets_removed() {
        let dir = std::env::temp_dir().join(format!("mx-{}-stale", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stale = dir.join("maxicom-1.sock");
        drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
        assert!(stale.exists());
        let other = dir.join("notes.txt");
        std::fs::write(&other, "x").unwrap();
        let live = Server::start(Some(&dir), "maxicom-live").unwrap();
        assert!(!stale.exists(), "the stale socket stays");
        let second = Server::start(Some(&dir), "maxicom-second").unwrap();
        assert!(Path::new(live.location()).exists(), "a live socket removed");
        assert!(other.exists());
        drop((live, second));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Serves once, then until `count` requests came in, the app's state as the test set it
    fn serve_requests(server: &mut Server, app: &mut App, count: usize) {
        let instance = Instance::now();
        let start = Instant::now();
        let mut served = server.serve(app, &instance);
        while served < count {
            assert!(start.elapsed() < Duration::from_secs(5), "no request");
            thread::sleep(Duration::from_millis(5));
            served += server.serve(app, &instance);
        }
    }

    /// MAX-284 (step 2b): each subscriber is told of the events it subscribed to, as `event`
    /// notifications in their order; another kind isn't sent to it; after `unsubscribe` nothing
    /// is. The reply gives the last event's number, the oldest kept and the bytes' end.
    /// How: a client subscribed to everything, one to `link-lost` only; two events emitted;
    /// then the first unsubscribes and one more is emitted.
    #[test]
    fn subscribers_told_of_their_events() {
        let mut server = TestServer::start("events");
        let mut app = crate::app::tests::app();
        let mut all = Client::connect(&server);
        let mut lost = Client::connect(&server);
        all.send(r#"{"jsonrpc":"2.0","id":1,"method":"subscribe"}"#);
        lost.send(
            r#"{"jsonrpc":"2.0","id":2,"method":"subscribe","params":{"events":["link-lost"]}}"#,
        );
        serve_requests(&mut server, &mut app, 2);
        let last = app.events.last();
        assert_eq!(all.reply()["result"]["last"], json!(last));
        assert_eq!(lost.reply()["result"]["oldest"], json!(app.events.oldest()));

        app.emit(Event::ReplayFinished);
        app.emit(Event::LinkLost { link: "Serial" });
        serve_requests(&mut server, &mut app, 0);
        let first = all.reply();
        assert_eq!(first["method"], json!("event"));
        assert_eq!(first["params"]["kind"], json!("replay-finished"));
        assert_eq!(first["params"]["seq"], json!(last + 1));
        assert_eq!(all.reply()["params"]["kind"], json!("link-lost"));
        let only = lost.reply();
        assert_eq!(only["params"]["kind"], json!("link-lost"));
        assert_eq!(only["params"]["link"], json!("Serial"));

        all.send(r#"{"jsonrpc":"2.0","id":3,"method":"unsubscribe"}"#);
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(all.reply()["result"], json!({ "events": [] }));
        app.emit(Event::LinkLost { link: "Serial" });
        serve_requests(&mut server, &mut app, 0);
        assert_eq!(lost.reply()["params"]["seq"], json!(last + 3));
        // nothing more for the one gone: its next line is the answer to a new request
        all.send(r#"{"jsonrpc":"2.0","id":4,"method":"hello"}"#);
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(all.reply()["id"], json!(4));
    }

    /// MAX-287 (step 3): with `read = "off"`, `query`, `subscribe` and `unsubscribe` are
    /// refused (`REFUSED`) and `hello` still answers, giving the levels; turned off by a
    /// reload, a subscriber is told nothing more, and on again it subscribes again.
    /// How: a subscriber, `read` turned off, an event emitted, the three methods and `hello`;
    /// then `read` on again and a query.
    #[test]
    fn reading_off_refuses_reads() {
        use crate::configs::ReadLevel;
        let mut server = TestServer::start("read");
        let mut app = crate::app::tests::app();
        let mut client = Client::connect(&server);
        client.send(r#"{"jsonrpc":"2.0","id":1,"method":"subscribe"}"#);
        serve_requests(&mut server, &mut app, 1);
        assert!(client.reply()["result"].is_object());

        app.settings_mut().api.read = ReadLevel::Off;
        app.emit(Event::LinkLost { link: "Serial" });
        client.send(r#"{"jsonrpc":"2.0","id":2,"method":"query","params":{"command":"status"}}"#);
        client.send(r#"{"jsonrpc":"2.0","id":3,"method":"subscribe"}"#);
        client.send(r#"{"jsonrpc":"2.0","id":4,"method":"unsubscribe"}"#);
        client.send(r#"{"jsonrpc":"2.0","id":5,"method":"hello"}"#);
        serve_requests(&mut server, &mut app, 4);
        // no `event` before the refusals: the subscription went with the reload
        for id in 2..=4 {
            let reply = client.reply();
            assert_eq!(reply["id"], json!(id));
            assert_eq!(reply["error"]["code"], json!(rpc::REFUSED), "{reply}");
        }
        let hello = client.reply();
        assert_eq!(hello["id"], json!(5));
        assert_eq!(hello["result"]["access"]["read"], json!("off"));

        app.settings_mut().api.read = ReadLevel::On;
        client.send(r#"{"jsonrpc":"2.0","id":6,"method":"query","params":{"command":"status"}}"#);
        serve_requests(&mut server, &mut app, 1);
        assert!(client.reply()["result"].is_object());
    }

    /// Serves until one request came in and no `run` waits for its job, running the jobs as
    /// the loop does; then `client`'s next line
    fn serve_until_reply(server: &mut Server, app: &mut App, client: &mut Client) -> Json {
        let instance = Instance::now();
        let start = Instant::now();
        let mut served = 0;
        while served == 0 || !server.runs.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "no reply");
            served += server.serve(app, &instance);
            while let Some((caller, action)) = app.next_command_action(Instant::now()) {
                app.dispatch_from(&caller, action);
            }
            thread::sleep(Duration::from_millis(5));
        }
        server.serve(app, &instance);
        client.reply()
    }

    /// MAX-284 (step 3a): `run {command}` runs a command line as a job of the connection,
    /// under the name its `hello` gave: the reply comes when the job ends, with its notes and
    /// its queries' values; a stopped job is an error of its kind (`REFUSED` for the levels,
    /// the message saying how many didn't run); a line that doesn't parse is refused at once.
    /// How: `hello` named, `run` of `status; sleep 1` (its job, still running, listed with the
    /// name), `send x; send y` with device off,
    /// `nope`, a missing `command`.
    #[test]
    fn run_replies_when_its_job_ends() {
        let mut server = TestServer::start("run");
        let mut app = crate::app::tests::app();
        let mut client = Client::connect(&server);
        client.send(r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"py"}}"#);
        serve_requests(&mut server, &mut app, 1);
        client.reply();

        client.send(
            r#"{"jsonrpc":"2.0","id":2,"method":"run","params":{"command":"status; sleep 1"}}"#,
        );
        let done = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(done["id"], json!(2));
        let values = &done["result"]["values"];
        assert_eq!(values[0]["query"], json!("status"));
        let jobs = &values[0]["value"]["jobs"];
        assert_eq!(jobs[0]["origin"], json!("client"), "{jobs}");
        assert_eq!(jobs[0]["name"], json!("py"), "{jobs}");

        client.send(
            r#"{"jsonrpc":"2.0","id":3,"method":"run","params":{"command":"send x; send y"}}"#,
        );
        let refused = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(refused["error"]["code"], json!(rpc::REFUSED), "{refused}");
        assert_eq!(
            refused["error"]["message"],
            json!("send: device access is off in [api] (1 more not run)")
        );

        client.send(r#"{"jsonrpc":"2.0","id":4,"method":"run","params":{"command":"nope"}}"#);
        let unknown = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(unknown["error"]["code"], json!(rpc::UNKNOWN_COMMAND));
        client.send(r#"{"jsonrpc":"2.0","id":5,"method":"run"}"#);
        let missing = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(missing["error"]["code"], json!(rpc::INVALID_PARAMS));
    }

    /// MAX-284 (step 3b): `send {bytes}` runs base64 bytes as a `send` of the connection,
    /// held by the `device` level like any: refused with it off; bytes that aren't base64, or
    /// none given, are refused at once as invalid params.
    /// How: `send` of "aGk=" (device off by default), of "%%", without params.
    #[test]
    fn send_is_a_send_of_the_client() {
        let mut server = TestServer::start("send");
        let mut app = crate::app::tests::app();
        let mut client = Client::connect(&server);
        client.send(r#"{"jsonrpc":"2.0","id":1,"method":"send","params":{"bytes":"aGk="}}"#);
        let refused = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(refused["error"]["code"], json!(rpc::REFUSED), "{refused}");
        assert_eq!(
            refused["error"]["message"],
            json!("send: device access is off in [api]")
        );
        client.send(r#"{"jsonrpc":"2.0","id":2,"method":"send","params":{"bytes":"%%"}}"#);
        let bad = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(bad["error"]["code"], json!(rpc::INVALID_PARAMS), "{bad}");
        client.send(r#"{"jsonrpc":"2.0","id":3,"method":"send"}"#);
        let missing = serve_until_reply(&mut server, &mut app, &mut client);
        assert_eq!(missing["error"]["code"], json!(rpc::INVALID_PARAMS));
    }

    /// MAX-284 (step 3c): `cancel` stops the connection's jobs: the cancel is answered with
    /// what it did, the waiting `run` with `CANCELLED`; a job number not its own is refused; a
    /// connection that closes takes its jobs with it.
    /// How: a `run` in a long `sleep`, `cancel {job: 999}`, then `cancel`; another client's
    /// `run` in a `sleep`, then that client gone.
    #[test]
    fn cancel_and_disconnect_end_the_jobs() {
        let mut server = TestServer::start("cancel");
        let mut app = crate::app::tests::app();
        let instance = Instance::now();
        let mut client = Client::connect(&server);
        client.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"run","params":{"command":"sleep 60000; status"}}"#,
        );
        client.send(r#"{"jsonrpc":"2.0","id":2,"method":"cancel","params":{"job":999}}"#);
        serve_requests(&mut server, &mut app, 2);
        let refused = client.reply();
        assert_eq!(refused["id"], json!(2));
        assert_eq!(refused["error"]["code"], json!(rpc::INVALID_PARAMS));
        assert_eq!(app.jobs().running().count(), 1);

        client.send(r#"{"jsonrpc":"2.0","id":3,"method":"cancel"}"#);
        serve_requests(&mut server, &mut app, 1);
        let cancel = client.reply();
        assert_eq!(cancel["id"], json!(3));
        assert_eq!(
            cancel["result"]["notes"],
            json!(["Dropped 2 waiting commands"])
        );
        server.serve(&mut app, &instance);
        let run = client.reply();
        assert_eq!(run["id"], json!(1));
        assert_eq!(run["error"]["code"], json!(rpc::CANCELLED), "{run}");

        let other = Client::connect(&server);
        other.send(r#"{"jsonrpc":"2.0","id":4,"method":"run","params":{"command":"sleep 60000"}}"#);
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(app.jobs().running().count(), 1);
        drop(other);
        let start = Instant::now();
        while app.jobs().running().count() > 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "the job stays");
            server.serve(&mut app, &instance);
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// MAX-284 (step 2b, review change): `subscribe {events}` adds to what the connection
    /// has and `unsubscribe {events}` takes only those off, each reply giving the names held
    /// after; with `since`, only the kinds just added are sent again; a bare `unsubscribe`
    /// drops the rest; a name unknown to `unsubscribe` is refused, the subscription kept.
    /// How: `link-lost`, then `link-idle` + `rx` added since an event of each kind, then
    /// `link-lost` taken off, events of both kinds emitted, then a bad name, an empty `events`
    /// (adds nothing) and a bare `unsubscribe`.
    #[test]
    fn subscriptions_change_by_kind() {
        let mut server = TestServer::start("kinds");
        let mut app = crate::app::tests::app();
        let mut client = Client::connect(&server);
        let request = |id: u32, method: &str, params: Json| {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
        };
        client.send(&request(1, "subscribe", json!({"events": ["link-lost"]})));
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(client.reply()["result"]["events"], json!(["link-lost"]));
        let since = app.events.last();
        app.emit(Event::LinkLost { link: "Serial" });
        app.emit(Event::LinkIdle { secs: 1 });
        serve_requests(&mut server, &mut app, 0);
        assert_eq!(client.reply()["params"]["kind"], json!("link-lost"));

        let added = json!({"events": ["link-idle", "rx"], "since": since});
        client.send(&request(2, "subscribe", added));
        serve_requests(&mut server, &mut app, 1);
        let names = client.reply()["result"]["events"].clone();
        let mut both = vec!["link-lost", "link-idle"];
        both.sort_by_key(|kind| KINDS.iter().position(|known| known == kind));
        assert_eq!(names, json!([both.as_slice(), &["rx"]].concat()));
        // the missed one of the kind added; not the link-lost sent already
        assert_eq!(client.reply()["params"]["kind"], json!("link-idle"));

        client.send(&request(3, "unsubscribe", json!({"events": ["link-lost"]})));
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(
            client.reply()["result"]["events"],
            json!(["link-idle", "rx"])
        );
        app.emit(Event::LinkLost { link: "Serial" });
        app.emit(Event::LinkIdle { secs: 2 });
        serve_requests(&mut server, &mut app, 0);
        assert_eq!(client.reply()["params"]["secs"], json!(2));

        client.send(&request(4, "unsubscribe", json!({"events": ["nope"]})));
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(client.reply()["error"]["code"], json!(rpc::INVALID_PARAMS));
        client.send(&request(5, "subscribe", json!({"events": []})));
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(
            client.reply()["result"]["events"],
            json!(["link-idle", "rx"])
        );
        client.send(r#"{"jsonrpc":"2.0","id":6,"method":"unsubscribe"}"#);
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(client.reply()["result"]["events"], json!([]));
        app.emit(Event::LinkIdle { secs: 3 });
        serve_requests(&mut server, &mut app, 0);
        client.send(&request(7, "hello", json!({})));
        serve_requests(&mut server, &mut app, 1);
        assert_eq!(client.reply()["id"], json!(7));
    }

    /// MAX-284 (step 2b): with `since`, the events kept after that number follow the reply,
    /// then the new ones, none twice; `rx` tells the end of the bytes received when more came,
    /// `tx` (step 2c) the count of bytes sent.
    /// How: three events emitted and served before the client comes, `subscribe` since the
    /// first with `rx` and `tx`, one more event, bytes received and sent.
    #[test]
    fn since_gives_the_events_missed() {
        let mut server = TestServer::start("since");
        let mut app = crate::app::tests::app();
        let base = app.events.last();
        for secs in 1..=3 {
            app.emit(Event::LinkIdle { secs });
        }
        serve_requests(&mut server, &mut app, 0);
        let mut client = Client::connect(&server);
        let since = base + 1;
        client.send(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"subscribe","params":{{"events":["link-idle","rx","tx"],"since":{since}}}}}"#
        ));
        serve_requests(&mut server, &mut app, 1);
        let result = client.reply()["result"].clone();
        assert_eq!(
            (&result["last"], &result["tx"]),
            (&json!(base + 3), &json!(0))
        );
        let secs = |reply: Json| reply["params"]["secs"].clone();
        assert_eq!(secs(client.reply()), json!(2));
        assert_eq!(secs(client.reply()), json!(3));
        app.emit(Event::LinkIdle { secs: 4 });
        app.feed_rx(b"hello");
        app.stats.update_tx(3);
        serve_requests(&mut server, &mut app, 0);
        assert_eq!(secs(client.reply()), json!(4));
        let rx = client.reply();
        assert_eq!(rx["method"], json!("rx"));
        assert_eq!(rx["params"]["newest"], json!(5));
        let tx = client.reply();
        assert_eq!(tx["method"], json!("tx"));
        assert_eq!(tx["params"]["sent"], json!(3));
    }

    /// MAX-290: `history` and `rows` are streams to follow: the reply gives their newest
    /// numbers, and lines received then rows scrolled up are told with the new ones.
    /// How: `subscribe` to both, then 30 lines received (both modes fed by `always_both`).
    #[test]
    fn lines_and_rows_are_notified() {
        let mut server = TestServer::start("streams");
        let mut app = crate::app::tests::app();
        serve_requests(&mut server, &mut app, 0);
        let mut client = Client::connect(&server);
        client.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"subscribe","params":{"events":["history","rows"]}}"#,
        );
        serve_requests(&mut server, &mut app, 1);
        let result = client.reply()["result"].clone();
        assert_eq!(
            (&result["history"], &result["rows"], &result["events"]),
            (&json!(0), &json!(0), &json!(["history", "rows"]))
        );
        let lines: String = (0..30).map(|n| format!("l{n}\r\n")).collect();
        app.feed_rx(lines.as_bytes());
        serve_requests(&mut server, &mut app, 0);
        let history = client.reply();
        assert_eq!(history["method"], json!("history"));
        assert_eq!(history["params"]["newest"], json!(30));
        let rows = client.reply();
        assert_eq!(rows["method"], json!("rows"));
        let scrolled = app.terminal_mode.rows_kept().end;
        assert!(scrolled > 0);
        assert_eq!(rows["params"]["newest"], json!(scrolled));
    }
}
