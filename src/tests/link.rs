
//! Opening by address: the kind it picks, and the opener off the loop.
use super::*;
use std::net::TcpListener;
use std::time::Instant;

fn config(address: &str) -> LinkConfig {
    LinkConfig {
        address: address.parse::<Address>().unwrap(),
        ..LinkConfig::default()
    }
}

/// Waits for the opener's result, as the loop polls it each frame
fn wait(opener: &mut Opener) -> Result<ByteSource, LinkError> {
    let start = Instant::now();
    loop {
        if let Some(result) = opener.finished() {
            return result;
        }
        assert!(start.elapsed() < CONNECT_TIMEOUT * 2, "no result");
        thread::sleep(Duration::from_millis(5));
    }
}

/// MAX-166: what each kind does: a replay is never written, keeps no session, never
/// reopens and has no line settings; a serial port and a TCP link do the first three, only a
/// serial port has line settings. A TCP link shows "raw TCP" in their place.
/// How: the answers of each kind. The kind each address names is `address`'s test.
#[test]
fn link_kinds() {
    let answers = |kind: LinkKind| {
        [
            kind.writes(),
            kind.keeps_sessions(),
            kind.reopens(),
            kind.has_line_settings(),
        ]
    };
    assert_eq!(answers(LinkKind::Serial), [true; 4]);
    assert_eq!(answers(LinkKind::Tcp), [true, true, true, false]);
    assert_eq!(answers(LinkKind::Replay), [false; 4]);
    assert_eq!(LinkKind::Tcp.line_stand_in(), Some(["raw", "TCP"]));
    assert_eq!(LinkKind::Serial.line_stand_in(), None);
    assert_eq!(LinkKind::Replay.line_stand_in(), None);
}

/// MAX-12: `restart` drops the open running for the new one: its result is the new link's.
/// How: a TCP open started, then restarted on a local listener.
#[test]
fn opener_restart_drops_the_open_running() {
    let first = TcpListener::bind("127.0.0.1:0").unwrap();
    let second = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut opener = Opener::default();
    opener.start(&config(&format!("tcp://{}", first.local_addr().unwrap())));
    opener.restart(&config(&format!("tcp://{}", second.local_addr().unwrap())));
    assert!(matches!(wait(&mut opener), Ok(ByteSource::Tcp(_))));
    assert!(second.accept().is_ok());
    assert!(opener.finished().is_none());
}

/// A TCP open runs off the loop and gives a TCP link; a second start while it runs is
/// ignored, and the result comes once.
/// How: a local listener, two starts, then polls until the result.
#[test]
fn opener_connects_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(&format!("tcp://{}", listener.local_addr().unwrap()));
    let mut opener = Opener::default();
    opener.start(&config);
    opener.start(&config);
    assert!(matches!(wait(&mut opener), Ok(ByteSource::Tcp(_))));
    assert!(opener.finished().is_none());
    assert!(listener.accept().is_ok());
}

/// A refused connect is a `ConnectFailed` naming the address, and the opener can start
/// again after it.
/// How: a port freed by dropping its listener, opened twice.
#[test]
fn opener_reports_a_refused_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    let mut opener = Opener::default();
    for _ in 0..2 {
        opener.start(&config(&format!("tcp://{addr}")));
        match wait(&mut opener) {
            Err(LinkError::ConnectFailed { addr: failed, .. }) => assert_eq!(failed, addr),
            other => panic!("{:?}", other.err()),
        }
    }
}

/// A serial port opens in the loop: its result is there right after the start.
/// How: a port that doesn't exist.
#[test]
fn opener_opens_serial_at_once() {
    let mut opener = Opener::default();
    opener.start(&config("/nonexistent/ttyMAXICOM"));
    assert!(matches!(
        opener.finished(),
        Some(Err(LinkError::OpenFailed { .. }))
    ));
}
