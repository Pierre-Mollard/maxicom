//! A device reached over TCP (MAX-156): a TCP-UART bridge in the device's firmware (ESP32),
//! ser2net or socat sharing a port, a serial device server in raw mode, QEMU's
//! `-serial tcp::…`. Raw bytes, or telnet around them (MAX-161): the telnet layer is taken off
//! the reads and put on the writes here, so what comes out and goes in is the device's bytes.
//! No line settings, the bridge owns the baud, but with RFC 2217 (esptool's and pyserial's
//! servers, ser2net's `telnet(rfc2217)`): the line and the control lines are set as on a serial
//! port. Reads and writes behave like `SerialDevice`'s, so the loop treats both alike
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

use socket2::{SockRef, TcpKeepalive};

use super::{
    Address, LinkError, Protocol,
    error::is_gone,
    lock::{LinkLock, LockState, lock_state},
    probe::maxicom_label,
    serial::{LineControl, LineSettings},
    telnet::{ComPort, Telnet},
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
    telnet: Option<Telnet>,
}

impl TcpDevice {
    /// Connects to `addr` (`host:port`), trying each address the host resolves to for at most
    /// `timeout`. The name lookup itself has no timeout. Telnet sends its requests at once;
    /// RFC 2217 sets `line` once the server agrees, as a serial open does
    pub fn connect(
        addr: &str,
        protocol: Protocol,
        line: &LineSettings,
        timeout: Duration,
    ) -> io::Result<Self> {
        let mut last_error = None;
        for socket in addr.to_socket_addrs()? {
            match Self::connect_to(socket, timeout) {
                Ok(mut stream) => {
                    let address = Address::Tcp(addr.to_string(), protocol);
                    let shared_with = match lock_state(&address) {
                        LockState::Held(info)
                            if info.as_ref().is_none_or(|i| i.pid != std::process::id()) =>
                        {
                            Some(maxicom_label(info.as_ref()))
                        }
                        _ => None,
                    };
                    let mut telnet = match protocol {
                        Protocol::Raw => None,
                        Protocol::Telnet => Some(Telnet::start()),
                        Protocol::Rfc2217 => Some(rfc2217(line)),
                    };
                    if let Some(telnet) = &mut telnet {
                        stream.write_all(&telnet.take_replies())?;
                    }
                    return Ok(Self {
                        stream,
                        _lock: LinkLock::take(&address),
                        shared_with,
                        telnet,
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

    /// The device's bytes: with telnet, its commands taken off and answered
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, LinkError> {
        let read = self.read_raw(buffer)?;
        let Some(telnet) = &mut self.telnet else {
            return Ok(read);
        };
        let len = telnet.decode(&mut buffer[..read]);
        self.flush()?;
        Ok(len)
    }

    fn read_raw(&mut self, buffer: &mut [u8]) -> Result<usize, LinkError> {
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

    /// Sends the device `buffer`, escaped for telnet
    pub fn write(&mut self, buffer: &[u8]) -> Result<(), LinkError> {
        match &self.telnet {
            Some(telnet) => send(&mut self.stream, &telnet.encode(buffer)),
            None => send(&mut self.stream, buffer),
        }
    }

    /// New line settings for the RFC 2217 server's port: DTR and RTS levels wait for the next
    /// open, as on a serial port
    pub fn reconfigure(&mut self, line: &LineSettings) -> Result<(), LinkError> {
        let telnet = self.com_port().ok_or_else(|| {
            LinkError::ConfigFailed(io::Error::other("the server refuses RFC 2217"))
        })?;
        for command in ComPort::line(line) {
            telnet.set(command);
        }
        self.flush()
    }

    /// Sets DTR or RTS, or starts a break, on the RFC 2217 server's port: ending it is the
    /// caller's, `end_break`
    pub fn control(&mut self, control: LineControl) -> Result<(), LinkError> {
        let telnet = self.com_port().ok_or(LinkError::NoControlLines)?;
        telnet.set(control.into());
        self.flush()
    }

    pub fn end_break(&mut self) -> Result<(), LinkError> {
        let telnet = self.com_port().ok_or(LinkError::NoControlLines)?;
        telnet.set(ComPort::Break(false));
        self.flush()
    }

    /// What went wrong with RFC 2217 since the last call
    pub fn take_warnings(&mut self) -> Vec<String> {
        (self.telnet.as_mut()).map_or_else(Vec::new, Telnet::take_warnings)
    }

    /// The telnet layer when it speaks RFC 2217, the server not having refused it
    fn com_port(&mut self) -> Option<&mut Telnet> {
        self.telnet.as_mut().filter(|telnet| telnet.has_com_port())
    }

    /// Sends what the telnet layer has for the server
    fn flush(&mut self) -> Result<(), LinkError> {
        let Some(telnet) = &mut self.telnet else {
            return Ok(());
        };
        let replies = telnet.take_replies();
        if replies.is_empty() {
            return Ok(());
        }
        send(&mut self.stream, &replies)
    }
}

/// RFC 2217 with `line` set as a serial open sets it: the line settings, then DTR and RTS as
/// asked, one at a time. Held until the server agrees
fn rfc2217(line: &LineSettings) -> Telnet {
    let mut telnet = Telnet::rfc2217();
    for command in ComPort::line(line) {
        telnet.set(command);
    }
    for control in line.level_controls() {
        telnet.set(control.into());
    }
    telnet
}

/// A reset or broken connection is a lost link, as on a read (MAX-199)
fn send(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), LinkError> {
    stream.write_all(bytes).map_err(|err| match err {
        err if is_gone(&err) => LinkError::InputClosed,
        err => LinkError::WriteFailed {
            bytes_attempted: bytes.len(),
            source: err,
        },
    })
}

#[cfg(test)]
#[path = "../tests/link/tcp.rs"]
mod tests;
