//! The API socket itself (MAX-74): a server in a temporary folder, clients on Unix sockets.
//! Windows' named pipes come with step 2
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

#[cfg(unix)]
mod socket {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::{UnixListener, UnixStream},
        time::Instant,
    };

    use super::*;

    /// A folder of its own for each test's sockets
    fn temp_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maxicom_api_{}_{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn client(server: &Server) -> (UnixStream, BufReader<UnixStream>) {
        let stream = UnixStream::connect(server.location()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        (stream, reader)
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

    fn read_reply(reader: &mut BufReader<UnixStream>) -> Json {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    /// MAX-74 (step 1): two clients connect at once, each gets its `hello` answered on its own
    /// connection; the socket file is the owner's only and goes when the server stops.
    /// How: a server in a temporary folder, two Unix socket clients.
    #[test]
    fn two_clients_say_hello() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("hello");
        let mut server = Server::start(Some(&dir), "maxicom-test").unwrap();
        let path = PathBuf::from(server.location());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let (mut one, mut one_reader) = client(&server);
        let (mut two, mut two_reader) = client(&server);
        writeln!(one, r#"{{"jsonrpc":"2.0","id":1,"method":"hello"}}"#).unwrap();
        writeln!(two, r#"{{"jsonrpc":"2.0","id":"b","method":"hello"}}"#).unwrap();
        answer_requests(&mut server, 2);
        let first = read_reply(&mut one_reader);
        let second = read_reply(&mut two_reader);
        assert_eq!((&first["id"], &second["id"]), (&json!(1), &json!("b")));
        assert_eq!(first["result"]["api"], json!(API_VERSION));
        drop(server);
        assert!(!path.exists(), "the socket file stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-74 (step 1): a client that leaves before its reply changes nothing for the
    /// others: its reply goes nowhere, the next client is answered.
    /// How: a `hello` sent then the client dropped, answered; then another client's `hello`.
    #[test]
    fn client_gone_before_its_reply() {
        let dir = temp_dir("gone");
        let mut server = Server::start(Some(&dir), "maxicom-test").unwrap();
        let (mut gone, _) = client(&server);
        writeln!(gone, r#"{{"jsonrpc":"2.0","id":1,"method":"hello"}}"#).unwrap();
        drop(gone);
        answer_requests(&mut server, 1);
        let (mut next, mut reader) = client(&server);
        writeln!(next, r#"{{"jsonrpc":"2.0","id":2,"method":"hello"}}"#).unwrap();
        answer_requests(&mut server, 1);
        assert_eq!(read_reply(&mut reader)["id"], json!(2));
        drop(server);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-74 (step 1): a line that isn't JSON gets a parse error and the connection closes; a
    /// line too long too; an invalid request and an unknown method keep it open.
    /// How: one client per case; an empty read (or a reset) is the closed connection.
    #[test]
    fn bad_lines() {
        let dir = temp_dir("bad");
        let mut server = Server::start(Some(&dir), "maxicom-test").unwrap();
        // closed: the end, or a reset when the server closed with bytes left unread (the
        // line too long)
        let closed = |reader: &mut BufReader<UnixStream>| {
            let mut rest = String::new();
            match reader.read_line(&mut rest) {
                Ok(n) => n == 0,
                Err(err) => err.kind() == std::io::ErrorKind::ConnectionReset,
            }
        };

        let (mut stream, mut reader) = client(&server);
        writeln!(stream, "not json").unwrap();
        assert_eq!(read_reply(&mut reader)["error"]["code"], json!(-32700));
        assert!(closed(&mut reader));

        let (mut stream, mut reader) = client(&server);
        let long = vec![b'x'; rpc::MAX_LINE + 10];
        let _ = stream.write_all(&long);
        assert_eq!(read_reply(&mut reader)["error"]["code"], json!(-32700));
        assert!(closed(&mut reader));

        let (mut stream, mut reader) = client(&server);
        writeln!(stream, r#"{{"jsonrpc":"2.0","id":1}}"#).unwrap();
        assert_eq!(read_reply(&mut reader)["error"]["code"], json!(-32600));
        writeln!(stream, r#"{{"jsonrpc":"2.0","id":2,"method":"nope"}}"#).unwrap();
        answer_requests(&mut server, 1);
        let unknown = read_reply(&mut reader);
        assert_eq!(
            (&unknown["id"], &unknown["error"]["code"]),
            (&json!(2), &json!(-32601))
        );
        drop(server);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-74 (step 1): at start, the socket files no maxicom listens on are removed, the
    /// live ones and the other files kept.
    /// How: a socket file left by a listener dropped (std leaves its file), a live server's
    /// file, an unrelated file; then a second server starting in the same folder.
    #[test]
    fn stale_sockets_removed() {
        let dir = temp_dir("stale");
        std::fs::create_dir_all(&dir).unwrap();
        let stale = dir.join("maxicom-1.sock");
        drop(UnixListener::bind(&stale).unwrap());
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
