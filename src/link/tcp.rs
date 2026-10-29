//! A device reached over TCP (MAX-156): a TCP-UART bridge in the device's firmware (ESP32),
//! ser2net or socat sharing a port, a serial device server in raw mode, QEMU's
//! `-serial tcp::…`. Raw bytes only: no telnet (MAX-161), and no line settings, the bridge
//! owns the baud. Reads and writes behave like `SerialDevice`'s, so the loop treats both alike
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

use socket2::{SockRef, TcpKeepalive};

use super::serial::SerialError;

/// A read waits at most this, like the serial port's: the loop idles in the input poll
const READ_TIMEOUT: Duration = Duration::from_millis(1);
/// A peer that stops taking bytes fails the write instead of freezing the loop
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// A silent link is probed after `KEEPALIVE_TIME`, then every `KEEPALIVE_INTERVAL`, and
/// reported dead after `KEEPALIVE_RETRIES` unanswered probes: about 11 s
const KEEPALIVE_TIME: Duration = Duration::from_secs(5);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
const KEEPALIVE_RETRIES: u32 = 3;

pub struct TcpDevice {
    stream: TcpStream,
}

impl TcpDevice {
    /// Connects to `addr` (`host:port`), trying each address the host resolves to for at most
    /// `timeout`. The name lookup itself has no timeout
    pub fn connect(addr: &str, timeout: Duration) -> io::Result<Self> {
        let mut last_error = None;
        for addr in addr.to_socket_addrs()? {
            match Self::connect_to(addr, timeout) {
                Ok(device) => return Ok(device),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("no address found for '{addr}'"),
            )
        }))
    }

    /// Whether `addr` has the `host:port` shape, without resolving it: a malformed one would
    /// never connect, so it fails at start instead of being retried
    pub fn check_address(addr: &str) -> io::Result<()> {
        match addr.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => Ok(()),
            _ => Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("'{addr}' is not host:port"),
            )),
        }
    }

    fn connect_to(addr: SocketAddr, timeout: Duration) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        // typed keys leave at once instead of waiting to fill a packet (Nagle)
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        // a powered-off device or a dropped Wi-Fi never closes the connection: without probes
        // it would look open forever
        let keepalive = TcpKeepalive::new()
            .with_time(KEEPALIVE_TIME)
            .with_interval(KEEPALIVE_INTERVAL)
            .with_retries(KEEPALIVE_RETRIES);
        SockRef::from(&stream).set_tcp_keepalive(&keepalive)?;
        Ok(Self { stream })
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SerialError> {
        match self.stream.read(buffer) {
            // the peer closed the connection
            Ok(0) if !buffer.is_empty() => Err(SerialError::InputClosed),
            Ok(bytes_read) => Ok(bytes_read),
            // the read timeout: WouldBlock on Unix, TimedOut on Windows
            Err(e) => match e.kind() {
                ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted => Ok(0),
                _ => Err(SerialError::ReadFailed(e)),
            },
        }
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<(), SerialError> {
        self.stream
            .write_all(buffer)
            .map_err(|err| SerialError::WriteFailed {
                bytes_attempted: buffer.len(),
                source: err,
            })
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            matches!(result, Err(SerialError::InputClosed)),
            "{result:?}"
        );
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

    /// The address shape is checked without a name lookup.
    /// How: good and bad addresses.
    #[test]
    fn address_shape() {
        for good in [
            "192.168.1.50:4000",
            "localhost:23",
            "esp32.local:4000",
            "[::1]:4000",
        ] {
            assert!(TcpDevice::check_address(good).is_ok(), "{good}");
        }
        for bad in [
            "192.168.1.50",
            ":4000",
            "host:",
            "host:notaport",
            "host:70000",
            "",
        ] {
            assert!(TcpDevice::check_address(bad).is_err(), "{bad}");
        }
    }
}
