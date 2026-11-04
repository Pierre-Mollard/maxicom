//! The TCP source against a local listener: connect, both directions, the close, refusal.
use super::*;
use std::net::TcpListener;
use std::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(1);

/// A listener on a free local port, and its `host:port`
fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    (listener, addr)
}

/// Reads until `len` bytes came, as the loop would over several frames
fn read_exactly(device: &mut TcpDevice, len: usize) -> Vec<u8> {
    let mut received = Vec::new();
    let mut buffer = [0; 64];
    let start = Instant::now();
    while received.len() < len && start.elapsed() < TIMEOUT {
        let n = device.read(&mut buffer).unwrap();
        received.extend_from_slice(&buffer[..n]);
    }
    received
}

/// Bytes go both ways, and a read with nothing waiting returns 0 instead of blocking.
/// How: connect, read on a silent link, then a write each way.
#[test]
fn bytes_go_both_ways() {
    let (listener, addr) = listener();
    let mut device = TcpDevice::connect(&addr, TIMEOUT).unwrap();
    let (mut peer, _) = listener.accept().unwrap();

    let start = Instant::now();
    assert_eq!(device.read(&mut [0; 16]).unwrap(), 0);
    assert!(start.elapsed() < Duration::from_millis(500));

    peer.write_all(b"boot\r\n").unwrap();
    assert_eq!(read_exactly(&mut device, 6), b"boot\r\n");

    device.write(b"ls\r").unwrap();
    let mut received = [0; 3];
    peer.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"ls\r");
}

/// The peer closing the connection reads as a lost device, like an unplugged USB port.
/// How: the accepted stream dropped, then reads until the error.
#[test]
fn peer_close_is_input_closed() {
    let (listener, addr) = listener();
    let mut device = TcpDevice::connect(&addr, TIMEOUT).unwrap();
    drop(listener.accept().unwrap());

    let start = Instant::now();
    let result = loop {
        match device.read(&mut [0; 16]) {
            Ok(0) if start.elapsed() < TIMEOUT => continue,
            other => break other,
        }
    };
    assert!(matches!(result, Err(LinkError::InputClosed)), "{result:?}");
}

/// Connecting where nothing listens fails at once, and a reconnect works once something
/// listens again.
/// How: a port freed by dropping its listener, then a new listener on it.
#[test]
fn refused_then_reconnects() {
    let (listener, addr) = listener();
    drop(listener);
    assert!(TcpDevice::connect(&addr, TIMEOUT).is_err());

    let listener = TcpListener::bind(&addr).unwrap();
    let mut device = TcpDevice::connect(&addr, TIMEOUT).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    peer.write_all(b"again").unwrap();
    assert_eq!(read_exactly(&mut device, 5), b"again");
}

/// An address that resolves to nothing is an error, not a hang.
/// How: a malformed port.
#[test]
fn bad_address_is_an_error() {
    assert!(TcpDevice::connect("127.0.0.1:notaport", TIMEOUT).is_err());
    assert!(TcpDevice::connect("127.0.0.1", TIMEOUT).is_err());
}
