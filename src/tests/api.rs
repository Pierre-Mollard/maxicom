//! The API socket itself (MAX-74): a server in a temporary folder on Unix, a pipe of its own
//! name on Windows; the clients are `interprocess` streams, as another program's would be
use super::*;

/// `hello`'s answer, with no socket: the API's version, this maxicom's pid, its link, its start
/// time; an unknown method is refused; a notification gets no reply.
/// How: the test app, a request of each kind.
#[test]
fn hello_says_who_answers() {
    let app = crate::app::tests::app();
    let instance = Instance::now();
    let request = |id: Option<Json>, method: &str| Request {
        id,
        method: method.to_string(),
        params: None,
    };
    let hello = answer(&app, &instance, &request(Some(json!(1)), "hello")).unwrap();
    let hello: Json = serde_json::from_str(&hello).unwrap();
    let result = &hello["result"];
    assert_eq!(result["api"], json!(API_VERSION));
    assert_eq!(result["pid"], json!(std::process::id()));
    assert_eq!(result["link"], json!(app.link_label()));
    assert_eq!(
        result["since"],
        json!(crate::commands::query::utc(instance.since))
    );
    let unknown = answer(&app, &instance, &request(Some(json!(2)), "nope")).unwrap();
    assert!(unknown.contains("-32601"), "{unknown}");
    assert_eq!(answer(&app, &instance, &request(None, "hello")), None);
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
        let app = crate::app::tests::app();
        let instance = Instance::now();
        let start = Instant::now();
        let mut answered = 0;
        while answered < count {
            assert!(start.elapsed() < Duration::from_secs(5), "no request");
            for (conn, request) in server.requests() {
                if let Some(reply) = answer(&app, &instance, &request) {
                    server.reply(conn, reply);
                }
                answered += 1;
            }
            thread::sleep(Duration::from_millis(5));
        }
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
        assert_eq!(client.reply()["error"]["code"], json!(-32700));
        assert!(client.closed());

        let mut client = Client::connect(&server);
        client.send_bytes(&vec![b'x'; rpc::MAX_LINE + 10]);
        assert_eq!(client.reply()["error"]["code"], json!(-32700));
        assert!(client.closed());

        let mut client = Client::connect(&server);
        client.send(r#"{"jsonrpc":"2.0","id":1}"#);
        assert_eq!(client.reply()["error"]["code"], json!(-32600));
        client.send(r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#);
        answer_requests(&mut server, 1);
        let unknown = client.reply();
        assert_eq!(
            (&unknown["id"], &unknown["error"]["code"]),
            (&json!(2), &json!(-32601))
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
}
