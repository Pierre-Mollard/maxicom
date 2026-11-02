//! The link as the app holds it (MAX-168): its config, the device while it is open, the open
//! running and why, and the timers for it. Its own rules are here: a read that gets bytes is
//! activity, a lost link closes, a failed `open` goes back to the address that was open, a
//! detached link isn't reopened until an attach. `app::device` drives it and says what happens
use std::{
    mem,
    time::{Duration, Instant},
};

use super::{
    Address, ByteSource, LinkConfig, LinkError, LinkKind, Opener,
    replay::Replay,
    serial::{LineControl, LineSettings},
};

/// Between two tries to reopen a closed link
pub const REOPEN_EVERY: Duration = Duration::from_millis(500);

pub struct LinkState {
    config: LinkConfig,
    device: Option<ByteSource>,
    opener: Opener,
    open_for: OpenFor,
    /// When the break a `break` command started ends (MAX-12)
    break_until: Option<Instant>,
    last_rx_activity: Instant,
    last_reconnect_attempt: Instant,
    /// Detached by hand or when idle: not reopened until an attach, or another link
    detached: bool,
}

/// Who started the open that runs, which decides what its result shows: a retry stays quiet
/// until it connects, the user's attach reports a failure, the start shows the popup. `open`
/// (MAX-12) went back to the `previous` port when it failed
pub enum OpenFor {
    Start,
    Attach,
    Retry,
    Open { previous: Address },
}

/// What new line settings did: set on the open port, kept for a detached one, refused by the
/// port, or nothing for a link that has none open
pub enum Reconfigured {
    Applied,
    Kept,
    Refused(LinkError),
    Ignored,
}

impl LinkState {
    pub fn new(config: LinkConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            device: None,
            opener: Opener::default(),
            open_for: OpenFor::Retry,
            break_until: None,
            last_rx_activity: now,
            last_reconnect_attempt: now,
            detached: false,
        }
    }

    pub fn config(&self) -> &LinkConfig {
        &self.config
    }

    pub fn kind(&self) -> LinkKind {
        self.config.address.kind()
    }

    pub fn is_open(&self) -> bool {
        self.device.is_some()
    }

    /// Closed until an attach: by hand, idle, or a reattach that failed
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    /// The whole config, a replay's live link put back (MAX-166)
    pub fn set_config(&mut self, config: LinkConfig) {
        self.config = config;
    }

    /// Another address for the link, its line settings kept: a replay's file (MAX-166)
    pub fn set_address(&mut self, address: Address) {
        self.config.address = address;
    }

    /// The link at start, with the device opened before the UI if any; its timers from now
    pub fn start(&mut self, device: Option<ByteSource>) {
        let now = Instant::now();
        self.device = device;
        self.last_reconnect_attempt = now;
        self.last_rx_activity = now;
    }

    /// One read of what is waiting, none when closed. Bytes are activity; a lost link closes
    pub fn read(&mut self, buffer: &mut [u8]) -> Option<Result<usize, LinkError>> {
        let result = self.device.as_mut()?.read(buffer);
        match result {
            Ok(n) if n > 0 => self.last_rx_activity = Instant::now(),
            Err(LinkError::InputClosed) => self.device = None,
            _ => {}
        }
        Some(result)
    }

    /// Writes to the device, none when closed
    pub fn write(&mut self, bytes: &[u8]) -> Option<Result<(), LinkError>> {
        Some(self.device.as_mut()?.write(bytes))
    }

    /// What is sent counts as activity, as what is received, for the idle detach
    pub fn note_activity(&mut self) {
        self.last_rx_activity = Instant::now();
    }

    /// Sets a control line, none when closed; a break ends at `now` + its duration
    pub fn control(&mut self, control: LineControl, now: Instant) -> Option<Result<(), LinkError>> {
        let result = self.device.as_mut()?.control(control);
        if let (Ok(()), LineControl::Break(duration)) = (&result, control) {
            self.break_until = Some(now + duration);
        }
        Some(result)
    }

    /// Ends the break once its time is up: its result then, none before
    pub fn end_break_if_due(&mut self, now: Instant) -> Option<Result<(), LinkError>> {
        if self.break_until.is_none_or(|until| now < until) {
            return None;
        }
        self.break_until = None;
        Some(self.device.as_mut()?.end_break())
    }

    /// Whether the replay read its whole file (MAX-166)
    pub fn is_replay_finished(&self) -> bool {
        self.replay().is_some_and(Replay::is_finished)
    }

    /// The replay playing, for its progress and its controls (MAX-166)
    pub fn replay(&self) -> Option<&Replay> {
        match &self.device {
            Some(ByteSource::Replay(replay)) => Some(replay),
            _ => None,
        }
    }

    pub fn replay_mut(&mut self) -> Option<&mut Replay> {
        match &mut self.device {
            Some(ByteSource::Replay(replay)) => Some(replay),
            _ => None,
        }
    }

    /// New line settings: set on the open port, or kept for a detached one, which opens with
    /// them (MAX-12)
    pub fn reconfigure(&mut self, line: LineSettings) -> Reconfigured {
        let reopens = self.kind().reopens();
        match self.device.as_mut() {
            Some(ByteSource::Serial(dev)) => match dev.reconfigure(&line) {
                Ok(()) => {
                    self.config.line = line;
                    Reconfigured::Applied
                }
                Err(e) => Reconfigured::Refused(e),
            },
            None if reopens => {
                self.config.line = line;
                Reconfigured::Kept
            }
            _ => Reconfigured::Ignored,
        }
    }

    /// Starts opening the link for `why`: the open running is dropped, but an attach takes its
    /// result. Not detached any more
    pub fn begin_open(&mut self, why: OpenFor) {
        let attach = matches!(why, OpenFor::Attach);
        self.open_for = why;
        self.detached = false;
        if attach {
            self.opener.start(&self.config);
        } else {
            self.opener.restart(&self.config);
        }
    }

    /// The link closes and `address` opens in its place, as a reconnect does; a failure goes
    /// back to the address that was open (MAX-12)
    pub fn switch_to(&mut self, address: Address) {
        let previous = mem::replace(&mut self.config.address, address);
        self.device = None;
        self.break_until = None;
        self.begin_open(OpenFor::Open { previous });
    }

    /// A replay's file in place of the device, which closes (MAX-166)
    pub fn set_replay(&mut self, replay: Replay) {
        self.device = Some(ByteSource::Replay(replay));
        self.break_until = None;
        self.detached = false;
    }

    pub fn close(&mut self) {
        self.device = None;
    }

    /// Closes the link until an attach: by hand, or idle
    pub fn detach(&mut self) {
        self.device = None;
        self.detached = true;
    }

    /// Detaches an open link quiet for `timeout` (none when zero); true when it did
    pub fn detach_if_idle(&mut self, timeout: Duration) -> bool {
        let idle = self.is_open()
            && self.kind().reopens()
            && !timeout.is_zero()
            && self.last_rx_activity.elapsed() >= timeout;
        if idle {
            self.detach();
        }
        idle
    }

    /// A closed link opens again every `REOPEN_EVERY` unless detached, then the result of the
    /// open running, with why it was started: an open device is in place, a failed `open` is
    /// back on its previous address. None while it runs, or when the link is open
    pub fn poll_open(&mut self) -> Option<(OpenFor, Result<(), LinkError>)> {
        if self.is_open() {
            return None;
        }
        if self.kind().reopens()
            && !self.detached
            && self.last_reconnect_attempt.elapsed() >= REOPEN_EVERY
        {
            self.last_reconnect_attempt = Instant::now();
            self.opener.start(&self.config);
        }
        let result = self.opener.finished()?;
        let why = mem::replace(&mut self.open_for, OpenFor::Retry);
        Some(match result {
            Ok(device) => {
                self.device = Some(device);
                self.last_rx_activity = Instant::now();
                (why, Ok(()))
            }
            Err(e) => {
                if let OpenFor::Open { previous } = &why {
                    self.config.address = previous.clone();
                }
                (why, Err(e))
            }
        })
    }
}

#[cfg(test)]
mod tests {
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

    /// A failed `open` puts the address that was open back, and says which it was.
    /// How: an open TCP link switched to a port freed by dropping its listener.
    #[test]
    fn failed_switch_goes_back() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut link = link_on(tcp(&listener));
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_address = tcp(&closed);
        drop(closed);
        link.switch_to(closed_address.clone());
        assert_eq!(link.config().address, closed_address);
        match result(&mut link) {
            (OpenFor::Open { previous }, Err(_)) => assert_eq!(previous, tcp(&listener)),
            _ => panic!("no failure"),
        }
        assert_eq!(link.config().address, tcp(&listener));
    }
}
