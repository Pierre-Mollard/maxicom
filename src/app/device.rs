//! The app driving its link (MAX-168): what the link does, said as notices and popups, the
//! command line chain resumed or failed, the session backup switched. `LinkState` keeps the link's
//! own rules. The loop calls `start_link` once, `poll_link` each turn and `dispatch` for each
//! action
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::{App, UpdateEffect};
use crate::{
    capture::{SessionDevice, SessionMeta},
    inputs::Action,
    link::{
        Address, ByteSource, CONNECT_TIMEOUT, LinkError, LinkKind, OpenFor, Reconfigured,
        replay::{Replay, ReplayControl},
        serial::{LineControl, LineSettings},
    },
    overlays::{ErrorDialog, Overlay},
};

/// Max time spent reading in one turn before handling input and drawing
const RX_DRAIN_BUDGET: Duration = Duration::from_millis(5);

/// What a turn of the link did, for the loop's pacing: bytes came in, something changed
#[derive(Debug, Default)]
pub struct LinkTurn {
    pub rx_received: bool,
    pub redraw: bool,
}

impl App {
    /// The link at start, with the device opened before the UI if any. A TCP link connects in
    /// the loop, so the UI is up and says so; a missing device shows its popup, after the
    /// restore question when it asks (MAX-148)
    pub fn start_link(&mut self, device: Option<ByteSource>) {
        self.link.start(device);
        if self.link.is_open() {
            return;
        }
        if self.link_kind() == LinkKind::Tcp {
            self.begin_open(OpenFor::Start);
        } else if matches!(self.overlay(), Some(Overlay::RestoreSession(_))) {
            self.disconnected_after_restore = true;
        } else {
            self.show_disconnected();
        }
    }

    /// One turn of the link: what is waiting is read (bounded), the end of a replay said, the
    /// replies to the remote's queries sent, the break ended, the file and the mash sent on
    /// time; then an idle link detached, or a closed one reopened and its result shown. A read
    /// error nothing handles ends the loop
    pub fn poll_link(&mut self, buffer: &mut [u8]) -> Result<LinkTurn, LinkError> {
        let mut turn = LinkTurn::default();
        if self.disconnected_after_restore && self.overlay().is_none() {
            self.disconnected_after_restore = false;
            if !self.link.is_open() {
                self.show_disconnected();
                turn.redraw = true;
            }
        }

        if self.link.is_open() {
            self.read_link(buffer, &mut turn)?;
            turn.redraw |= turn.rx_received;

            // the end of a replayed file, said once (MAX-166)
            if self.link.is_replay_finished() && !self.is_replay_ended() {
                self.end_replay();
                turn.redraw = true;
            }

            // the replies to the remote's queries, never in a replay (MAX-151)
            let answers = self.take_query_answers();
            if !answers.is_empty() && self.link_kind().writes() && self.link.is_open() {
                self.transmit(&answers);
            }
        }

        // the break ends on time, the input poll waking the loop often enough (MAX-12)
        if let Some(Err(e)) = self.link.end_break_if_due(Instant::now()) {
            self.notify_error(format!("break: {e}"));
        }
        self.send_due(&mut turn);

        if self.link.is_open() {
            let timeout = self.settings.inactivity_detach_timeout;
            if self.link.detach_if_idle(timeout) {
                self.show_detached();
                self.notify_warn(format!(
                    "Device idle for {}s: port detached",
                    timeout.as_secs()
                ));
                turn.redraw = true;
            }
        } else if let Some((why, result)) = self.link.poll_open() {
            match result {
                Ok(()) => self.opened(why),
                Err(e) => self.open_failed(why, e),
            }
            turn.redraw = true;
        }
        Ok(turn)
    }

    /// The action, then the link effect it gives applied to the link: what is left (quit,
    /// clipboard, reload) is the loop's
    pub fn dispatch(&mut self, action: Action) -> UpdateEffect {
        let effect = self.update(action);
        self.apply_link_effect(effect)
    }

    fn apply_link_effect(&mut self, effect: UpdateEffect) -> UpdateEffect {
        match effect {
            UpdateEffect::Open(address) => self.open_link(address),
            UpdateEffect::Replay {
                path,
                delay,
                session,
            } => self.replay_file(path, delay, session),
            UpdateEffect::StopReplay => self.stop_replay_link(),
            UpdateEffect::ToggleAttach => self.toggle_attach(),
            UpdateEffect::Transmit(bytes) => {
                if self.transmit(&bytes) {
                    self.link.note_activity();
                }
            }
            UpdateEffect::Reconfigure(line) => self.reconfigure_link(line),
            UpdateEffect::LineControl(control) => self.line_control(control),
            other => return other,
        }
        UpdateEffect::None
    }

    /// Writes `bytes` to the link, counted and recorded (not in a replay), or opens the error
    /// popup. True when written; false when the link is closed
    pub fn transmit(&mut self, bytes: &[u8]) -> bool {
        match self.link.write(bytes) {
            None => false,
            Some(Ok(())) => {
                if self.link_kind().writes() {
                    self.stats.update_tx(bytes.len());
                    self.record_tx(bytes);
                }
                true
            }
            Some(Err(e)) => {
                let summary = format!(
                    "Could not send the bytes to the {}",
                    self.link_kind().noun()
                );
                self.open_overlay(Overlay::Error(
                    ErrorDialog::new("Write Failed", summary).with_details(e),
                ));
                false
            }
        }
    }

    /// Everything already waiting, instead of one buffer per frame, for `RX_DRAIN_BUDGET` at
    /// most. A lost link shows its popup
    fn read_link(&mut self, buffer: &mut [u8], turn: &mut LinkTurn) -> Result<(), LinkError> {
        let drain_start = Instant::now();
        while let Some(result) = self.link.read(buffer) {
            match result {
                Ok(0) => break,
                Ok(bytes_read) => {
                    turn.rx_received = true;
                    self.stats.update_rx(bytes_read);
                    self.feed_rx(&buffer[..bytes_read]);
                    if drain_start.elapsed() >= RX_DRAIN_BUDGET {
                        break;
                    }
                }
                Err(LinkError::InputClosed) => {
                    self.show_disconnected();
                    self.notify_warn(format!("{} lost", self.link_kind().name()));
                    turn.redraw = true;
                    break;
                }
                Err(LinkError::ReadFailed(e)) => {
                    let summary = format!("Could not read from the {}", self.link_kind().noun());
                    self.open_overlay(Overlay::Error(
                        ErrorDialog::new("Read Failed", summary).with_details(e),
                    ));
                    turn.redraw = true;
                    break;
                }
                Err(e) => {
                    self.notify_error(format!("{} read error", self.link_kind().name()));
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// The file `send-file` sends, a chunk per turn paced to the baud rate (MAX-12), and the
    /// text `mash` sends (MAX-6), on time: the input poll wakes the loop often enough. Both
    /// stop when the link closes or a write fails
    fn send_due(&mut self, turn: &mut LinkTurn) {
        if self.transfer().is_some() {
            if self.link.is_open() {
                if let Some(chunk) = self.next_transfer_chunk(Instant::now()) {
                    if self.transmit(&chunk) {
                        self.link.note_activity();
                    } else {
                        self.stop_transfer("the write failed");
                    }
                    turn.redraw = true;
                }
            } else {
                self.stop_transfer("the link closed");
                turn.redraw = true;
            }
        }

        if self.mash().is_some() {
            if self.link.is_open() {
                if let Some(bytes) = self.next_mash_bytes(Instant::now()) {
                    if !self.transmit(&bytes) {
                        self.stop_mash("the write failed");
                    }
                    turn.redraw = true;
                }
            } else {
                self.stop_mash("the link closed");
                turn.redraw = true;
            }
        }
    }

    fn opened(&mut self, why: OpenFor) {
        let address = &self.link.config().address;
        match why {
            OpenFor::Start => self.notify_success(format!("Connected to {address}")),
            OpenFor::Attach => {
                self.notify_success(format!("Reconnected to {address}"));
                self.close_overlay();
            }
            OpenFor::Retry => {
                self.notify_success(format!("Device Reconnected: {address}"));
                if let Some(Overlay::DeviceDisconnected) = self.overlay() {
                    self.close_overlay();
                }
            }
            OpenFor::Open { previous } => {
                let switched = previous != *address;
                self.notify_success(format!("Opened {address}"));
                if switched {
                    self.restart_session();
                }
                self.resume_commands();
            }
        }
    }

    fn open_failed(&mut self, why: OpenFor, e: LinkError) {
        match why {
            // what a missing port does at start, after the restore question
            OpenFor::Start => {
                self.notify_warn(e.to_string());
                if matches!(self.overlay(), Some(Overlay::RestoreSession(_))) {
                    self.disconnected_after_restore = true;
                } else {
                    self.show_disconnected();
                }
            }
            OpenFor::Attach => self.notify_error(format!("Failed to reattach: {e}")),
            OpenFor::Retry => {}
            // the retries reopen the port that was open, which the link is back on
            OpenFor::Open { .. } => {
                let back = &self.link.config().address;
                self.fail_commands(format!("open: {e}, back to {back}"));
            }
        }
    }

    /// Starts opening the link for `why`; a TCP link says it connects, which takes a while
    fn begin_open(&mut self, why: OpenFor) {
        self.link.begin_open(why);
        if self.link_kind() == LinkKind::Tcp {
            self.notify_info(format!("Connecting to {}", self.link.config().address));
        }
    }

    /// The current link closes and the new one opens as a reconnect does, without waiting: the rest
    /// of the command line waits for its result (MAX-12). A replay that set the live link aside
    /// ends first, so the switch starts from the live link, its session backup and its encoding
    /// (MAX-171)
    fn open_link(&mut self, address: Address) {
        if self.replaced_link.is_some() {
            self.stop_replay();
        }
        self.link.switch_to(address);
        self.pause_commands(Instant::now() + CONNECT_TIMEOUT * 2);
        self.notify_info(format!("Opening {}", self.link.config().address));
        self.close_link_popups();
    }

    /// The file in place of the link, which closes: no wait, a file opens at once (MAX-166).
    /// For `replay` and `open file://` while running, and for `--replay` at start (MAX-168).
    /// Without a `session`, the file's own meta file gives the device, when it has one
    /// (MAX-163)
    pub fn open_replay(
        &mut self,
        path: PathBuf,
        delay: Duration,
        session: Option<&SessionDevice>,
    ) -> io::Result<()> {
        let replay = Replay::open(&path, delay)?;
        self.link.set_replay(replay);
        let meta = session
            .is_none()
            .then(|| SessionMeta::read(&SessionMeta::path_of(&path)))
            .flatten();
        let session = session.or(meta.as_ref().map(|meta| &meta.device));
        self.start_replay(path.clone(), session);
        self.notify_info(format!("Replaying {}", path.display()));
        self.close_link_popups();
        Ok(())
    }

    /// A replay asked while running: a file that doesn't open stops the command line chain
    fn replay_file(&mut self, path: PathBuf, delay: Duration, session: Option<SessionDevice>) {
        if let Err(e) = self.open_replay(path.clone(), delay, session.as_ref()) {
            self.fail_commands(format!("replay: {}: {e}", path.display()));
        }
    }

    /// The link the replay replaced opens again as at start, its popup if it doesn't; a replay
    /// from the command line just stops reading
    fn stop_replay_link(&mut self) {
        self.link.close();
        if self.stop_replay() {
            self.begin_open(OpenFor::Start);
        } else {
            self.notify_info("Replay stopped");
        }
    }

    /// Detaches the link, with its popup, or attaches it again; a replay has nothing to
    /// attach. The attach's result comes with the reopen's, next turn for a port
    fn toggle_attach(&mut self) {
        if !self.link_kind().reopens() {
            return;
        }
        if self.link.is_open() {
            self.link.detach();
            self.show_detached();
            self.notify_warn(format!("{} detached", self.link_kind().name()));
        } else {
            self.begin_open(OpenFor::Attach);
        }
    }

    /// New line settings: a port that refuses them stops the command line chain, the dialog
    /// says why (MAX-12)
    fn reconfigure_link(&mut self, line: LineSettings) {
        match self.link.reconfigure(line) {
            Reconfigured::Applied | Reconfigured::Ignored => {}
            Reconfigured::Kept => {
                self.notify_info("The port is detached: it opens with these settings");
            }
            Reconfigured::Refused(e) => {
                self.drop_pending_commands();
                self.open_overlay(Overlay::Error(
                    ErrorDialog::new("Serial Conf Rejected", "Failed to reconfigure serial")
                        .with_details(e),
                ));
            }
        }
    }

    /// A control line set; a break pauses the command line chain until its end, which the next
    /// turn comes to first
    fn line_control(&mut self, control: LineControl) {
        let now = Instant::now();
        match self.link.control(control, now) {
            None => self.fail_commands(format!("{control}: the port is detached")),
            Some(Ok(())) => {
                if let LineControl::Break(duration) = control {
                    self.pause_commands(now + duration);
                }
            }
            Some(Err(e)) => self.fail_commands(format!("{control}: {e}")),
        }
    }

    /// A replay control on the replay reading, said with a notice (MAX-166)
    pub fn control_replay(&mut self, control: ReplayControl) {
        let Some(replay) = self.link.replay_mut() else {
            return;
        };
        let notice = match control {
            ReplayControl::Pause => {
                replay.pause();
                "Replay paused".to_string()
            }
            ReplayControl::Resume => {
                replay.resume();
                "Replay resumed".to_string()
            }
            ReplayControl::Delay(delay) => {
                replay.set_delay(delay);
                format!("Replay delay {} ms", delay.as_millis())
            }
            ReplayControl::Restart => match replay.restart() {
                Ok(()) => {
                    self.restart_replay_screen();
                    "Replay restarted".to_string()
                }
                Err(e) => return self.notify_error(format!("replay-restart: {e}")),
            },
        };
        self.notify_info(notice);
    }

    fn show_disconnected(&mut self) {
        self.open_overlay(Overlay::DeviceDisconnected);
    }

    fn show_detached(&mut self) {
        self.open_overlay(Overlay::DeviceDetached);
    }

    /// The detached and disconnected popups close when another link takes this one's place
    fn close_link_popups(&mut self) {
        if let Some(Overlay::DeviceDetached | Overlay::DeviceDisconnected) = self.overlay() {
            self.close_overlay();
        }
    }
}

#[cfg(test)]
mod tests {
    //! The app driving a real link: a TCP listener stands for the device, a file for a replay.
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    use super::*;
    use crate::{
        capture::SessionText,
        configs::{KeyBindingsConfig, KeyResolver, SettingsConfig, TextEncoding},
        inputs::AppAction,
        link::{LinkConfig, state::REOPEN_EVERY},
    };

    fn app_on(address: &str) -> App {
        App::new(
            SettingsConfig::default(),
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            LinkConfig {
                address: address.parse::<Address>().unwrap(),
                ..LinkConfig::default()
            },
        )
    }

    fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("tcp://{}", listener.local_addr().unwrap());
        (listener, address)
    }

    fn toasts(app: &App) -> Vec<String> {
        let toasts = app.notifications.active_toasts();
        toasts.iter().map(|toast| toast.message.clone()).collect()
    }

    /// Polls the link as the loop does, until `done` or a few seconds
    fn poll_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let mut buffer = [0; 256];
        let start = Instant::now();
        while !done(app) {
            app.poll_link(&mut buffer).unwrap();
            assert!(start.elapsed() < Duration::from_secs(5), "{what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// A TCP link opened as at start: connecting, then connected, with the peer's stream
    fn connected(listener: &TcpListener, address: &str) -> (App, TcpStream) {
        let mut app = app_on(address);
        app.start_link(None);
        assert!(toasts(&app).contains(&format!("Connecting to {address}")));
        let (peer, _) = listener.accept().unwrap();
        poll_until(&mut app, "connected", |app| app.link.is_open());
        assert!(toasts(&app).contains(&format!("Connected to {address}")));
        (app, peer)
    }

    /// MAX-168: a TCP link connects in the loop at start; what it sends is read, counted and
    /// fed; written bytes reach it, counted; when the peer closes, the link closes with the
    /// disconnected popup and a notice.
    /// How: a local listener, a line from it, a write, then the peer dropped.
    #[test]
    fn tcp_link_reads_writes_and_is_lost() {
        let (listener, address) = listener();
        let (mut app, mut peer) = connected(&listener, &address);
        peer.write_all(b"hi\r\n").unwrap();
        poll_until(&mut app, "read", |app| app.stats.rx() == 4);
        assert!(app.terminal_mode.vt100_screen().contents().contains("hi"));

        app.dispatch(Action::TransmitRaw(b"x".to_vec()));
        let mut byte = [0; 1];
        peer.read_exact(&mut byte).unwrap();
        assert_eq!((&byte, app.stats.tx()), (b"x", 1));

        drop(peer);
        poll_until(&mut app, "lost", |app| !app.link.is_open());
        assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));
        assert!(toasts(&app).contains(&"TCP connection lost".to_string()));
    }

    /// MAX-168: `open` switches the link and resumes the command line on success; a failed
    /// open goes back to the link that was open and says why.
    /// How: a link on one listener opened onto a second, then onto a closed port.
    #[test]
    fn open_switches_and_goes_back_on_failure() {
        let (first, first_address) = listener();
        let (second, second_address) = listener();
        let (mut app, _peer) = connected(&first, &first_address);
        app.dispatch(Action::App(AppAction::Open(
            second_address.parse::<Address>().unwrap(),
        )));
        assert!(!app.link.is_open());
        assert!(toasts(&app).contains(&format!("Opening {second_address}")));
        let _second_peer = second.accept().unwrap();
        poll_until(&mut app, "opened", |app| app.link.is_open());
        assert!(toasts(&app).contains(&format!("Opened {second_address}")));

        let (closed, closed_address) = listener();
        drop(closed);
        app.dispatch(Action::App(AppAction::Open(
            closed_address.parse::<Address>().unwrap(),
        )));
        poll_until(&mut app, "failed", |app| {
            app.link_config().address == second_address.parse::<Address>().unwrap()
        });
        let failed = toasts(&app);
        assert!(
            failed.iter().any(|toast| toast.starts_with("open: ")
                && toast.ends_with(&format!("back to {second_address}"))),
            "{failed:?}"
        );
    }

    /// MAX-168: the detach key closes the link with its popup, and the link isn't reopened
    /// while detached, even when another popup takes the detached one's place; the key again
    /// reattaches it and closes the popup.
    /// How: a TCP link detached, the quit question opened over it, polled past the reopen
    /// delay, then attached.
    #[test]
    fn detach_then_attach() {
        let (listener, address) = listener();
        let (mut app, _peer) = connected(&listener, &address);
        app.dispatch(Action::App(AppAction::ToggleAttach));
        assert!(!app.link.is_open());
        assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
        app.open_overlay(Overlay::ConfirmQuit);
        let mut buffer = [0; 256];
        let until = Instant::now() + REOPEN_EVERY * 2;
        while Instant::now() < until {
            app.poll_link(&mut buffer).unwrap();
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!app.link.is_open());

        app.close_overlay();
        app.dispatch(Action::App(AppAction::ToggleAttach));
        let _peer = listener.accept().unwrap();
        poll_until(&mut app, "attached", |app| app.link.is_open());
        assert!(toasts(&app).contains(&format!("Reconnected to {address}")));
        assert!(app.overlay().is_none());
    }

    /// MAX-168: new line settings on a detached port are kept for its next open, with a
    /// notice; on a link that isn't a port they change nothing.
    /// How: a serial port that isn't there, a 9600 baud config; then a TCP link.
    #[test]
    fn reconfigure_keeps_settings_for_a_detached_port() {
        let mut app = app_on("/nonexistent/ttyMAXICOM");
        let line = LineSettings {
            baud: 9600,
            ..app.link_config().line
        };
        app.apply_link_effect(UpdateEffect::Reconfigure(line));
        assert_eq!(app.link_config().line.baud, 9600);
        let kept = "The port is detached: it opens with these settings";
        assert!(toasts(&app).contains(&kept.to_string()));

        let (listener, address) = listener();
        let (mut app, _peer) = connected(&listener, &address);
        let line = LineSettings {
            baud: 9600,
            ..app.link_config().line
        };
        app.apply_link_effect(UpdateEffect::Reconfigure(line));
        assert_eq!(app.link_config().line.baud, 115_200);
    }

    /// MAX-168: a link quiet for `inactivity_detach_timeout` is detached, with its popup.
    /// How: a TCP link with a short timeout, nothing sent.
    #[test]
    fn idle_link_is_detached() {
        let (listener, address) = listener();
        let (mut app, _peer) = connected(&listener, &address);
        app.settings_mut().inactivity_detach_timeout = Duration::from_millis(50);
        poll_until(&mut app, "detached", |app| !app.link.is_open());
        assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
        assert!(toasts(&app).contains(&"Device idle for 0s: port detached".to_string()));
    }

    /// MAX-168: a replay takes the link's place and is read, its end said once; stopping it
    /// reopens the live link as at start.
    /// How: a TCP link, a replay of a small file, the stop.
    #[test]
    fn replay_reads_ends_and_gives_the_link_back() {
        let (listener, address) = listener();
        let (mut app, _peer) = connected(&listener, &address);
        let path = std::env::temp_dir().join(format!(
            "maxicom_test_{}_device_replay.raw",
            std::process::id()
        ));
        std::fs::write(&path, b"replayed\r\n").unwrap();
        app.dispatch(Action::App(AppAction::Replay {
            path: path.clone(),
            delay: Duration::ZERO,
        }));
        assert_eq!(app.link_kind(), LinkKind::Replay);
        poll_until(&mut app, "finished", App::is_replay_ended);
        let finished = |app: &App| {
            let toasts = toasts(app);
            toasts.iter().filter(|t| *t == "Replay finished").count()
        };
        assert_eq!(finished(&app), 1);
        app.poll_link(&mut [0; 256]).unwrap();
        assert_eq!(finished(&app), 1);
        assert!(
            app.terminal_mode
                .vt100_screen()
                .contents()
                .contains("replayed")
        );

        app.dispatch(Action::App(AppAction::StopReplay));
        assert_eq!(
            app.link_config().address,
            address.parse::<Address>().unwrap()
        );
        let _peer = listener.accept().unwrap();
        poll_until(&mut app, "back", |app| app.link.is_open());
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-171: `open` during a replay that set a live link aside ends the replay first: the live
    /// link comes back with its encoding and its session backup, then the new link opens with a
    /// session backup of its own, and the attach key no longer goes back to the old link. Opening
    /// the live link again keeps its session backup.
    /// How: a TCP link with a session backup, "one" received; a cp437 session file replayed; `open`
    /// onto a second listener, "two" received there, then the attach key; the session files read
    /// back. Then the same with `open` onto the live link, "again" received.
    #[test]
    fn open_during_a_replay_ends_it_first() {
        let root = std::env::temp_dir().join(format!(
            "maxicom_test_{}_open_during_replay",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let sessions = root.join("sessions");
        let replayed = root.join("files").join("replayed.raw");
        std::fs::create_dir_all(replayed.parent().unwrap()).unwrap();
        std::fs::write(&replayed, b"old\r\n").unwrap();
        let (first, first_address) = listener();
        let device = SessionDevice::new(
            &LinkConfig {
                address: first_address.parse::<Address>().unwrap(),
                ..LinkConfig::default()
            },
            TextEncoding::Cp437,
        );
        SessionMeta::new(chrono::Local::now(), device, SessionText::default())
            .write(&SessionMeta::path_of(&replayed))
            .unwrap();
        // the session files' raw bytes, the oldest first
        let raws = || -> Vec<Vec<u8>> {
            crate::capture::tests::session_paths(&sessions)
                .iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == "raw"))
                .map(|path| std::fs::read(path).unwrap())
                .collect()
        };
        let live_with_replay = || {
            let (mut app, mut peer) = connected(&first, &first_address);
            app.start_session(Some(sessions.clone()));
            peer.write_all(b"one\r\n").unwrap();
            poll_until(&mut app, "one", |app| app.stats.rx() == 5);
            let live_encoding = app.settings().encoding;
            app.dispatch(Action::App(AppAction::Replay {
                path: replayed.clone(),
                delay: Duration::ZERO,
            }));
            assert_eq!(app.settings().encoding, TextEncoding::Cp437);
            (app, live_encoding)
        };
        let open = |app: &mut App, address: &str, listener: &TcpListener, text: &[u8]| {
            app.dispatch(Action::App(AppAction::Open(
                address.parse::<Address>().unwrap(),
            )));
            let (mut peer, _) = listener.accept().unwrap();
            poll_until(app, "opened", |app| app.link.is_open());
            peer.write_all(text).unwrap();
            let text = String::from_utf8_lossy(text).trim().to_string();
            poll_until(app, "received", |app| {
                app.terminal_mode.vt100_screen().contents().contains(&text)
            });
        };

        let (second, second_address) = listener();
        let (mut app, live_encoding) = live_with_replay();
        open(&mut app, &second_address, &second, b"two\r\n");
        assert!(app.replaced_link.is_none());
        assert_eq!(app.settings().encoding, live_encoding);
        app.dispatch(Action::App(AppAction::ToggleAttach));
        assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
        let second_address = second_address.parse::<Address>().unwrap();
        assert_eq!(app.link_config().address, second_address);
        drop(app);
        assert_eq!(raws(), [b"one\r\n".to_vec(), b"two\r\n".to_vec()]);

        std::fs::remove_dir_all(&sessions).unwrap();
        let (mut app, _) = live_with_replay();
        open(&mut app, &first_address, &first, b"again\r\n");
        drop(app);
        assert_eq!(raws(), [b"one\r\nagain\r\n".to_vec()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// MAX-166: the replay controls from the command line: a paused replay reads nothing
    /// until resumed, its delay changes, a restart plays it again from the top on a clean
    /// screen, not ended; each refuses with its reason when it would change nothing, and all
    /// refuse without a replay.
    /// How: a 3-line file replayed with a long delay, then each command.
    #[test]
    fn replay_controls() {
        let run = |app: &mut App, line: &str| {
            let invocations = crate::commands::parse(line).unwrap();
            app.dispatch(Action::App(AppAction::QueueCommands(invocations)));
            while let Some(action) = app.next_command_action(Instant::now()) {
                app.dispatch(action);
            }
            toasts(app).last().cloned().unwrap_or_default()
        };
        let mut app = app_on("/nonexistent/ttyMAXICOM");
        assert_eq!(
            run(&mut app, "replay-pause"),
            "replay-pause: no replay runs"
        );

        let path = std::env::temp_dir().join(format!(
            "maxicom_test_{}_device_controls.raw",
            std::process::id()
        ));
        std::fs::write(&path, b"one\r\ntwo\r\nthree\r\n").unwrap();
        run(&mut app, &format!("replay {} 10000", path.display()));
        assert_eq!(app.replay().unwrap().progress(), (0, 17));
        assert_eq!(
            run(&mut app, "replay-resume"),
            "replay-resume: the replay isn't paused"
        );
        assert_eq!(run(&mut app, "replay-pause"), "Replay paused");
        assert_eq!(
            run(&mut app, "replay-pause"),
            "replay-pause: the replay is paused already"
        );
        assert_eq!(run(&mut app, "replay-speed 0"), "Replay delay 0 ms");
        let mut buffer = [0; 256];
        app.poll_link(&mut buffer).unwrap();
        assert_eq!(app.replay().unwrap().progress().0, 0);
        assert_eq!(run(&mut app, "replay-resume"), "Replay resumed");
        poll_until(&mut app, "finished", App::is_replay_ended);
        assert_eq!(app.replay().unwrap().percent(), 100);
        assert!(
            app.terminal_mode
                .vt100_screen()
                .contents()
                .contains("three")
        );
        assert_eq!(
            run(&mut app, "replay-pause"),
            "replay-pause: the replay is finished (replay-restart plays it again)"
        );

        assert_eq!(run(&mut app, "replay-restart"), "Replay restarted");
        assert!(!app.is_replay_ended());
        assert!(
            !app.terminal_mode
                .vt100_screen()
                .contents()
                .contains("three")
        );
        assert_eq!(app.stats.rx(), 0);
        poll_until(&mut app, "again", App::is_replay_ended);
        assert!(
            app.terminal_mode
                .vt100_screen()
                .contents()
                .contains("three")
        );
        assert_eq!(app.stats.rx(), 17);
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-168: a control line needs an open link with control lines: a closed link and a
    /// TCP link refuse it, with the reason.
    /// How: DTR on a link not open yet, then on an open TCP link.
    #[test]
    fn line_control_needs_a_port() {
        let (listener, address) = listener();
        let mut app = app_on(&address);
        app.dispatch(Action::App(AppAction::LineControl(LineControl::Dtr(true))));
        assert!(
            toasts(&app)
                .iter()
                .any(|t| t.ends_with("the port is detached"))
        );
        drop(app);
        let (mut app, _peer) = connected(&listener, &address);
        app.dispatch(Action::App(AppAction::LineControl(LineControl::Dtr(true))));
        let refused = toasts(&app);
        assert!(refused.iter().any(|t| t.starts_with("DTR")), "{refused:?}");
    }
}
