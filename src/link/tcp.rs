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

use super::{
    Address, LinkError,
    error::is_gone,
    lock::{LinkLock, LockState, lock_state},
    probe::maxicom_label,
};

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
    // maxicom's lock on the address while connected, when it could be taken (MAX-186)
    _lock: Option<LinkLock>,
    // the maxicom that held the address when this one connected, which some servers allow
    shared_with: Option<String>,
}

impl TcpDevice {
    /// Connects to `addr` (`host:port`), trying each address the host resolves to for at most
    /// `timeout`. The name lookup itself has no timeout
    pub fn connect(addr: &str, timeout: Duration) -> io::Result<Self> {
        let mut last_error = None;
        for socket in addr.to_socket_addrs()? {
            match Self::connect_to(socket, timeout) {
                Ok(stream) => {
                    let address = Address::Tcp(addr.to_string());
                    let shared_with = match lock_state(&address) {
                        LockState::Held(info)
                            if info.as_ref().is_none_or(|i| i.pid != std::process::id()) =>
                        {
                            Some(maxicom_label(info.as_ref()))
                        }
                        _ => None,
                    };
                    return Ok(Self {
                        stream,
                        _lock: LinkLock::take(&address),
                        shared_with,
                    });
                }
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

    fn connect_to(addr: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
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
        Ok(stream)
    }

    /// Another maxicom on the same address when this one connected (MAX-186)
    pub fn shared_with(&self) -> Option<&str> {
        self.shared_with.as_deref()
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, LinkError> {
        match self.stream.read(buffer) {
            // the peer closed the connection
            Ok(0) if !buffer.is_empty() => Err(LinkError::InputClosed),
            Ok(bytes_read) => Ok(bytes_read),
            // the read timeout: WouldBlock on Unix, TimedOut on Windows
            Err(e) => match e.kind() {
                ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted => Ok(0),
                // a reset connection, the peer gone as on a close (MAX-199)
                _ if is_gone(&e) => Err(LinkError::InputClosed),
                _ => Err(LinkError::ReadFailed(e)),
            },
        }
    }

    /// A reset or broken connection is a lost link, as on a read (MAX-199)
    pub fn write(&mut self, buffer: &[u8]) -> Result<(), LinkError> {
        self.stream.write_all(buffer).map_err(|err| match err {
            err if is_gone(&err) => LinkError::InputClosed,
            err => LinkError::WriteFailed {
                bytes_attempted: buffer.len(),
                source: err,
            },
        })
    }
}

#[cfg(test)]
#[path = "../tests/link/tcp.rs"]
mod tests;
