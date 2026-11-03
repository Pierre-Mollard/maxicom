
//! The link's own rules, without the app: a TCP listener stands for the device.
use std::{net::TcpListener, thread};

use super::*;

fn tcp(listener: &TcpListener) -> Address {
    Address::Tcp(listener.local_addr().unwrap().to_string())
}

fn link_on(address: Address) -> LinkState {
    LinkState::new(LinkConfig {
        address,
        ..LinkConfig::default()
    })
}

/// Polls until the open running gives its result
fn result(link: &mut LinkState) -> (OpenFor, Result<(), LinkError>) {
    let start = Instant::now();
    loop {
        if let Some(result) = link.poll_open() {
            return result;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "no result");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A detached link isn't reopened, past the reopen delay; an attach opens it and clears
/// the detach.
/// How: an open TCP link detached, polled, then attached.
#[test]
fn detached_link_waits_for_an_attach() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut link = link_on(tcp(&listener));
    link.begin_open(OpenFor::Start);
    assert!(matches!(result(&mut link), (OpenFor::Start, Ok(()))));
    link.detach();
    assert!(!link.is_open() && link.detached);
    let until = Instant::now() + REOPEN_EVERY * 2;
    while Instant::now() < until {
        assert!(link.poll_open().is_none());
        thread::sleep(Duration::from_millis(20));
    }
    link.begin_open(OpenFor::Attach);
    assert!(!link.detached);
    assert!(matches!(result(&mut link), (OpenFor::Attach, Ok(()))));
    assert!(link.is_open());
}

/// A failed `open` puts the address and line that were open back, and says which they
/// were (MAX-185: the line too).
/// How: an open TCP link switched, at another baud rate, to a port freed by dropping its
/// listener.
#[test]
fn failed_switch_goes_back() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut link = link_on(tcp(&listener));
    let before = link.config().clone();
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = LinkConfig {
        address: tcp(&closed),
        ..before.clone()
    };
    config.line.baud = 9600;
    drop(closed);
    link.switch_to(config.clone());
    assert_eq!(*link.config(), config);
    match result(&mut link) {
        (OpenFor::Open { previous }, Err(_)) => assert_eq!(previous, before),
        _ => panic!("no failure"),
    }
    assert_eq!(*link.config(), before);
}
