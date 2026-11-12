//! The app driving its link (MAX-168): what the link does, said as notices and popups, the
//! command line chain resumed or failed, the session backup switched. `LinkState` keeps the link's
//! own rules. The loop calls `start_link` once, `poll_link` each turn and `dispatch` for each
//! action
use std::{
    io,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use ratatui::style::Color;

use super::{App, UpdateEffect};
use crate::{
    capture::{CaptureDevice, RecordingMeta, Session, recording_raw},
    configs::{
        AppConfig, LinksConfig,
        links::{SavedLink, default_name},
    },
    inputs::Action,
    link::{
        Address, ByteSource, CONNECT_TIMEOUT, LinkConfig, LinkError, LinkKind, OpenFor,
        Reconfigured,
        probe::{self, ProbeState},
        replay::{Replay, ReplayControl},
        serial::{self, LineControl, LineSettings},
    },
    notification::ToastLevel,
    overlays::{Directory, DirectoryTab, ErrorDialog, LinkForm, Overlay},
    transfer::{
        FileTransfer,
        modem::{ACK, CANCEL, NAK},
    },
};

/// How long a link's test may take before it's marked unanswered (MAX-181): above the TCP
/// probe's own timeout
const LINK_TEST_TIMEOUT: Duration = if cfg!(windows) {
    Duration::from_secs(3)
} else {
    Duration::from_secs(2)
};

/// The directory's links being tested, one thread each: their results by address, and when
/// the ones left stop being waited for (MAX-181, MAX-186)
pub struct LinkTest {
    results: mpsc::Receiver<(String, ProbeState)>,
    until: Instant,
}

/// Max time spent reading in one turn before handling input and drawing
const RX_DRAIN_BUDGET: Duration = Duration::from_millis(5);

/// What a turn of the link did, for the loop's pacing: bytes came in, something changed
#[derive(Debug, Default)]
pub struct LinkTurn {
    pub rx_received: bool,
    pub redraw: bool,
}

/// The lines line mode holds during a group replay at least, freed with it (MAX-202): about
/// 150 MB when full, as measured in MAX-214
const GROUP_REPLAY_LINES: usize = 1_000_000;

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

    /// Started without a device while the system lists `ports` or a link is saved (MAX-181,
    /// MAX-185): the directory open to pick one, the link waiting detached on the first
    /// (nothing opens on its own: an open can reset a board). The port picked starts as a switch
    /// does, once open: its last session restored, then its backup (MAX-187)
    pub fn start_in_directory(&mut self, ports: Vec<String>, sessions_dir: Option<PathBuf>) {
        self.sessions_dir = sessions_dir;
        self.device_unpicked = true;
        self.link.start(None);
        self.link.detach();
        self.open_directory(ports);
    }

    pub fn is_device_unpicked(&self) -> bool {
        self.device_unpicked
    }

    /// The directory's serial ports and saved links tested, both tabs at once, each address
    /// once and on its own thread, so one that hangs holds none of the others; the link in use
    /// left out (MAX-181, MAX-186). The results go by address, shown on both tabs
    pub fn test_links(&mut self) {
        // the link is maxicom's only while open: detached, lost or not picked, it's tested
        let current = self
            .link
            .is_open()
            .then(|| self.link.config().address.clone());
        let Some(Overlay::Directory(directory)) = &mut self.overlay else {
            return;
        };
        let ports = (directory.ports.iter()).filter_map(|port| port.parse::<Address>().ok());
        let links = (directory.links.iter().flatten()).map(|(_, config)| config.address.clone());
        let mut seen = std::collections::HashSet::new();
        let addresses = (ports.chain(links)).filter(|address| {
            current.as_ref() != Some(address) && seen.insert(address.to_string())
        });
        let (sender, results) = mpsc::channel();
        for address in addresses {
            let key = address.to_string();
            directory.tests.insert(key.clone(), None);
            let sender = sender.clone();
            thread::spawn(move || {
                let state = probe::probe(&address);
                let _ = sender.send((key, state));
            });
        }
        let until = Instant::now() + LINK_TEST_TIMEOUT;
        self.link_test = Some(LinkTest { results, until });
    }

    /// The test's results into the directory, the ones left past the timeout said unanswered;
    /// done once none is left, or the directory closed. True when a mark changed
    pub fn poll_link_test(&mut self) -> bool {
        let Some(test) = &self.link_test else {
            return false;
        };
        let results: Vec<_> = test.results.try_iter().collect();
        let timed_out = Instant::now() >= test.until;
        let Some(Overlay::Directory(directory)) = &mut self.overlay else {
            self.link_test = None;
            return false;
        };
        let changed = !results.is_empty() || timed_out;
        for (address, state) in results {
            directory.tests.insert(address, Some(state));
        }
        let waiting = directory.tests.values_mut().filter(|state| state.is_none());
        if timed_out {
            waiting.for_each(|state| *state = Some(ProbeState::NoAnswer));
            self.link_test = None;
        } else if waiting.count() == 0 {
            self.link_test = None;
        }
        changed
    }

    fn open_directory(&mut self, ports: Vec<String>) {
        let directory = self.directory(ports);
        self.open_overlay(Overlay::Directory(directory));
    }

    /// The link form on a link to save (MAX-185), the directory staying when it can't be
    pub fn name_link(&mut self, address: Option<Address>) {
        match self.link_to_save(address) {
            Ok(link) => {
                let form = LinkForm::new(link, &self.settings().link);
                self.open_overlay(Overlay::LinkForm(Box::new(form)));
            }
            Err(err) => self.notify_warn(err),
        }
    }

    /// The link in use (`None`) with its line settings, or a port with `[link]`'s, as it's
    /// saved, under its address's name (MAX-185). Refused: no link picked yet, a replay, an
    /// address saved already
    pub fn link_to_save(&self, address: Option<Address>) -> Result<SavedLink, String> {
        let config = match address {
            None if self.device_unpicked => {
                return Err("No link in use: pick one first".to_string());
            }
            None => self.link.config().clone(),
            Some(address) => LinkConfig {
                address,
                line: self.settings().link.line(),
            },
        };
        if config.address.kind() == LinkKind::Replay {
            return Err("A replay isn't a link to save".to_string());
        }
        if let Some(name) = self.saved_name(&config.address) {
            return Err(format!("{} is saved already, as '{name}'", config.address));
        }
        let name = default_name(&config.address);
        Ok(SavedLink::from_config(name, &config, self.host.as_deref()))
    }

    /// The saved link of this machine named `name` (MAX-185)
    pub fn saved_link(&self, name: &str) -> Option<&SavedLink> {
        let links = self.links.as_ref().ok()?;
        (links.links.iter()).find(|link| link.is_for(self.host.as_deref()) && link.name == name)
    }

    /// This machine's saved links' names, in the file's order, for the command line
    pub fn saved_names(&self) -> Vec<String> {
        let links = self.links.as_ref().map(|links| links.links.as_slice());
        (links.unwrap_or_default().iter())
            .filter(|link| link.is_for(self.host.as_deref()))
            .map(|link| link.name.clone())
            .collect()
    }

    /// What a saved link opens: its address, its line over `[link]`'s; none for an address
    /// that doesn't parse
    pub fn resolve(&self, link: &SavedLink) -> Option<LinkConfig> {
        let address = link.address.parse().ok()?;
        let line = link.over(&self.settings().link).line();
        Some(LinkConfig { address, line })
    }

    /// `link` written to `links.toml`, or in place of `replaces`, and said; why not else
    pub fn store_link(
        &mut self,
        link: &SavedLink,
        replaces: Option<&SavedLink>,
    ) -> Result<(), String> {
        let path = AppConfig::active_links_path(self.settings());
        self.links = Ok(LinksConfig::put(&path, link.clone(), replaces)?);
        self.notify_success(format!("Saved link '{}'", link.name));
        Ok(())
    }

    /// `link` taken out of `links.toml`, and said; why not else
    pub fn remove_link(&mut self, link: &SavedLink) -> Result<(), String> {
        let path = AppConfig::active_links_path(self.settings());
        self.links = Ok(LinksConfig::remove(&path, link)?);
        self.notify_success(format!("Deleted link '{}'", link.name));
        Ok(())
    }

    /// This machine's saved links
    fn links_here(&self) -> impl Iterator<Item = &SavedLink> {
        let links = self
            .links
            .as_ref()
            .map_or(&[][..], |links| &links.links[..]);
        (links.iter()).filter(|link| link.is_for(self.host.as_deref()))
    }

    /// The name `address` is saved under for this machine, when it is (MAX-185)
    pub fn saved_name(&self, address: &Address) -> Option<&str> {
        self.links_here()
            .find(|link| link.address.parse::<Address>().ok().as_ref() == Some(address))
            .map(|link| link.name.as_str())
    }

    /// This machine's saved colors by address, the first link of an address first (MAX-201)
    pub fn link_colors(&self) -> Vec<(Address, Color)> {
        self.links_here()
            .filter_map(|link| Some((link.address.parse().ok()?, link.color?)))
            .collect()
    }

    /// The color `address` is saved with for this machine, when it is (MAX-201)
    pub fn saved_color(&self, address: &Address) -> Option<Color> {
        self.links_here()
            .filter(|link| link.address.parse::<Address>().ok().as_ref() == Some(address))
            .find_map(|link| link.color)
    }

    /// The name of the link in use, when it's saved: what the status bar shows for it
    pub fn link_name(&self) -> Option<&str> {
        self.saved_name(&self.link.config().address)
    }

    /// The link in use as the notices and the link's popups name it: a saved one by its name,
    /// its address after it so the device stays findable (`esp32 (/dev/ttyUSB0)`)
    pub fn link_label(&self) -> String {
        let address = &self.link.config().address;
        match self.link_name() {
            Some(name) => format!("{name} ({address})"),
            None => address.to_string(),
        }
    }

    /// Why the link was lost, for its popup: the read error, none for a plain close (MAX-199)
    pub fn lost_reason(&self) -> Option<&str> {
        self.lost_reason.as_deref()
    }

    /// The form for a new link, for this machine with `[link]`'s line, or for `link` to edit
    /// (MAX-185)
    pub fn open_link_form(&mut self, link: Option<SavedLink>) {
        let defaults = &self.settings().link;
        let form = match link {
            Some(link) => LinkForm::edit(link, defaults),
            None => {
                let hosts = self.host.clone().into_iter().collect();
                LinkForm::new(
                    SavedLink {
                        hosts,
                        ..SavedLink::default()
                    },
                    defaults,
                )
            }
        };
        self.open_overlay(Overlay::LinkForm(Box::new(form)));
    }

    /// `link` added to `links.toml`, or in place of `replaces`, then the directory on it; the
    /// popup stays when it isn't saved, saying why (MAX-185)
    pub fn save_link(&mut self, link: SavedLink, replaces: Option<SavedLink>) {
        match self.store_link(&link, replaces.as_ref()) {
            Ok(()) => {
                self.open_directory(serial::port_names());
                if let Some(Overlay::Directory(directory)) = &mut self.overlay
                    && let Ok(links) = &directory.links
                    && let Some(idx) = links.iter().position(|(saved, _)| *saved == link)
                {
                    directory.tab = DirectoryTab::Saved;
                    directory.link_selected = idx;
                }
            }
            Err(err) => self.notify_warn(format!("{}: {err}", link.name)),
        }
    }

    /// `link` taken out of `links.toml`, said, then the directory back on the saved links,
    /// the error said there when it wasn't (MAX-185)
    pub fn delete_link(&mut self, link: &SavedLink) {
        if let Err(err) = self.remove_link(link) {
            self.notify_warn(format!("{}: {err}", link.name));
        }
        self.open_directory(serial::port_names());
        if let Some(Overlay::Directory(directory)) = &mut self.overlay {
            directory.tab = DirectoryTab::Saved;
        }
    }

    /// The directory on `ports` and this machine's saved links, each with its line settings
    /// over `[link]`'s (MAX-185)
    pub fn directory(&self, ports: Vec<String>) -> Directory {
        let current = &self.link.config().address;
        let links = self.links.as_ref().map_err(Clone::clone).map(|config| {
            (config.links.iter())
                .filter(|link| link.is_for(self.host.as_deref()))
                .filter_map(|link| Some((link.clone(), self.resolve(link)?)))
                .collect()
        });
        Directory::new(ports, current).with_links(links, current)
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
            // none while a protocol transfer owns the link (MAX-162)
            if !answers.is_empty()
                && self.link_kind().writes()
                && self.link.is_open()
                && !self.link_busy()
            {
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
                let message = format!("Device idle for {}s: port detached", timeout.as_secs());
                self.notify_link(ToastLevel::Warn, message);
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
        let effect = self.apply_link_effect(effect);
        self.restore_link_popup();
        effect
    }

    fn apply_link_effect(&mut self, effect: UpdateEffect) -> UpdateEffect {
        match effect {
            UpdateEffect::Open(address) => {
                let line = self.link.config().line;
                self.open_link(LinkConfig { address, line });
            }
            UpdateEffect::OpenLink(config) => self.open_link(config),
            UpdateEffect::Replay {
                path,
                delay,
                session,
            } => self.replay_file(path, delay, session),
            UpdateEffect::ReplayGroup(sessions) => self.open_group_replay(&sessions),
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

    /// Writes `bytes` to the link, counted and recorded (not in a replay). True when written;
    /// false when the link is closed, lost by this write, or the write failed: said once, not
    /// per key typed into it (MAX-199). Refused while a protocol transfer owns the link, said
    /// once too (MAX-162): a key, `send`, a terminal answer would break a packet
    pub fn transmit(&mut self, bytes: &[u8]) -> bool {
        if self.link_busy() {
            self.say_link_busy();
            return false;
        }
        self.write_link(bytes, true)
    }

    /// A write refused while a protocol transfer owns the link, said once per transfer
    pub fn say_link_busy(&mut self) {
        if !std::mem::replace(&mut self.busy_said, true) {
            let message = "A file transfer is running: cancel stops it";
            self.notify_transfer(ToastLevel::Warn, message);
        }
    }

    /// `transmit` without the busy check, for the transfer's own bytes; `record` false keeps
    /// them out of the text recording (a protocol's packets aren't text)
    pub fn write_link(&mut self, bytes: &[u8], record: bool) -> bool {
        match self.link.write(bytes) {
            None => false,
            Some(Ok(())) => {
                self.write_failing = false;
                if self.link_kind().writes() {
                    self.stats.update_tx(bytes.len());
                    if record {
                        self.record_tx(bytes);
                    }
                }
                true
            }
            Some(Err(LinkError::InputClosed)) => {
                self.lose_link(None);
                false
            }
            Some(Err(e)) => {
                self.write_failed(&e);
                false
            }
        }
    }

    /// A write error that leaves the link open, said once: quiet until a write goes through
    /// or the link opens again (MAX-199)
    fn write_failed(&mut self, e: &LinkError) {
        if !std::mem::replace(&mut self.write_failing, true) {
            let message = format!("{}: {e}", self.link_kind().name());
            self.notify_link(ToastLevel::Error, message);
        }
    }

    /// The link closed under a read or a write: its popup, with the error that closed it when
    /// it isn't a plain close, and a notice (MAX-199)
    fn lose_link(&mut self, reason: Option<String>) {
        self.lost_reason = reason;
        self.show_disconnected();
        let message = format!("{} lost", self.link_kind().name());
        self.notify_link(ToastLevel::Warn, message);
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
                    self.receive(&buffer[..bytes_read]);
                    if drain_start.elapsed() >= RX_DRAIN_BUDGET {
                        break;
                    }
                }
                // closed either way: a read error is lost too, its reason in the popup, once
                // (MAX-199)
                Err(e @ (LinkError::InputClosed | LinkError::ReadFailed(_))) => {
                    self.lose_link(matches!(e, LinkError::ReadFailed(_)).then(|| e.to_string()));
                    turn.redraw = true;
                    break;
                }
                Err(e) => {
                    let message = format!("{} read error", self.link_kind().name());
                    self.notify_link(ToastLevel::Error, message);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Live RX: to the views, or through the protocol transfer running (MAX-162). The device's
    /// output before the receiver starts and after the transfer ends is shown; what is between
    /// (the receiver's C, ACK, NAK) is the protocol's, only kept by the raw recordings
    fn receive(&mut self, chunk: &[u8]) {
        if !self.transfer().is_some_and(FileTransfer::takes_rx) {
            self.recancel(chunk);
            self.feed_rx(chunk);
            return;
        }
        let step = self.transfer_step(chunk, Instant::now());
        let (before, rest) = chunk.split_at(step.protocol.start);
        let (protocol, after) = rest.split_at(step.protocol.len());
        for part in [before, after] {
            if !part.is_empty() {
                self.feed_rx(part);
            }
        }
        if !protocol.is_empty() {
            self.record_protocol_rx(protocol);
        }
        self.write_transfer(&step.out);
    }

    /// A protocol transfer cancelled a moment ago: the other side's ACK or NAK means it missed
    /// the CANs (`rb` drops what came with a block's end), so they go again
    fn recancel(&mut self, chunk: &[u8]) {
        let Some(until) = self.recancel_until else {
            return;
        };
        if Instant::now() >= until {
            self.recancel_until = None;
        } else if chunk.iter().any(|&byte| byte == ACK || byte == NAK) {
            self.write_link(&CANCEL, false);
        }
    }

    /// The transfer's bytes, past the busy check; a failed write ends it
    fn write_transfer(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let record = !self.transfer().is_some_and(FileTransfer::takes_rx);
        if self.write_link(bytes, record) {
            self.link.note_activity();
        } else {
            self.stop_transfer("the write failed");
        }
    }

    /// The file `send-file` sends, a chunk per turn paced to the baud rate (MAX-12), an
    /// `upload`'s timeouts (MAX-162), and the text `mash` sends (MAX-6), on time: the input
    /// poll wakes the loop often enough. Both stop when the link closes or a write fails
    fn send_due(&mut self, turn: &mut LinkTurn) {
        if self.transfer().is_some() {
            if self.link.is_open() {
                let step = self.transfer_step(&[], Instant::now());
                turn.redraw |= !step.out.is_empty() || self.transfer().is_none();
                self.write_transfer(&step.out);
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
        self.link_lost = false;
        self.lost_reason = None;
        self.write_failing = false;
        let label = self.link_label();
        // some servers take several clients: said, not refused (MAX-186); in a box of its own,
        // as the link's box says it opened right after
        if let Some(holder) = self.link.shared_with() {
            self.notify_warn(format!("{label} is open in {holder} too"));
        }
        match why {
            OpenFor::Start => {
                self.notify_link(ToastLevel::Success, format!("Connected to {label}"))
            }
            OpenFor::Attach => {
                self.notify_link(ToastLevel::Success, format!("Reconnected to {label}"));
                self.close_overlay();
            }
            OpenFor::Retry => {
                self.notify_link(ToastLevel::Success, format!("Reconnected to {label}"));
                if let Some(Overlay::DeviceDisconnected) = self.overlay() {
                    self.close_overlay();
                }
            }
            OpenFor::Open { previous } => {
                let switched = previous.address != self.link.config().address;
                self.notify_link(ToastLevel::Success, format!("Opened {label}"));
                if let Some(Overlay::Directory(_)) = self.overlay() {
                    self.close_overlay();
                }
                // the first port picked from the start's directory gets its backup too; taken
                // before the test, or a switched pick left it set (MAX-181)
                let first_pick = std::mem::take(&mut self.device_unpicked);
                if switched || first_pick {
                    self.switch_session();
                }
                self.resume_commands();
            }
        }
    }

    fn open_failed(&mut self, why: OpenFor, e: LinkError) {
        // the directory that asked shows why, on the port's row (MAX-181)
        if let (OpenFor::Open { .. }, Some(Overlay::Directory(directory))) =
            (&why, &mut self.overlay)
            && let Some(port) = directory.opening.take()
        {
            let state = match &e {
                LinkError::OpenFailed { source, .. } => probe::port_state_of(source),
                e => ProbeState::Error(e.to_string()),
            };
            directory.tests.insert(port, Some(state));
        }
        match why {
            // what a missing port does at start, after the restore question
            OpenFor::Start => {
                self.notify_link(ToastLevel::Warn, e.to_string());
                if matches!(self.overlay(), Some(Overlay::RestoreSession(_))) {
                    self.disconnected_after_restore = true;
                } else {
                    self.show_disconnected();
                }
            }
            // still detached, its popup back: no quiet retries of a port held elsewhere
            OpenFor::Attach => {
                self.notify_link(ToastLevel::Error, format!("Failed to reattach: {e}"));
                self.link.detach();
                if self.overlay.is_none() {
                    self.show_detached();
                }
            }
            OpenFor::Retry => {}
            // nothing was open before the first pick: the link waits again (MAX-181)
            OpenFor::Open { .. } if self.device_unpicked => {
                self.link.detach();
                self.fail_commands(format!("open: {e}"));
            }
            // the retries reopen the port that was open, which the link is back on
            OpenFor::Open { .. } => {
                let back = self.link_label();
                self.fail_commands(format!("open: {e}, back to {back}"));
            }
        }
    }

    /// Starts opening the link for `why`; a TCP link says it connects, which takes a while
    fn begin_open(&mut self, why: OpenFor) {
        self.link.begin_open(why);
        if self.link_kind() == LinkKind::Tcp {
            let message = format!("Connecting to {}", self.link_label());
            self.notify_link(ToastLevel::Info, message);
        }
    }

    /// The current link closes and the new one opens as a reconnect does, without waiting: the rest
    /// of the command line waits for its result (MAX-12). A replay that set the live link aside
    /// ends first, so the switch starts from the live link, its session backup and its encoding
    /// (MAX-171)
    fn open_link(&mut self, config: LinkConfig) {
        if self.replaced_link().is_some() {
            self.stop_replay();
        }
        self.link.switch_to(config);
        self.pause_commands(Instant::now() + CONNECT_TIMEOUT * 2);
        let message = format!("Opening {}", self.link_label());
        self.notify_link(ToastLevel::Info, message);
        self.close_link_popups();
    }

    /// The file in place of the link, which closes: no wait, a file opens at once (MAX-166).
    /// For `replay` and `open file://` while running, and for `--replay` at start (MAX-168).
    /// `path` is a recording: its `.raw`, its `.meta` or its folder; anything else is refused
    /// (MAX-175). Without a `session`, the recording's meta file gives the device, when it has
    /// one (MAX-163)
    pub fn open_replay(
        &mut self,
        path: PathBuf,
        delay: Duration,
        session: Option<&CaptureDevice>,
    ) -> io::Result<()> {
        let path = recording_raw(&path)?;
        let replay = Replay::open(&path, delay)?;
        self.link.set_replay(replay);
        let meta = session
            .is_none()
            .then(|| RecordingMeta::read(&RecordingMeta::path_of(&path)))
            .flatten();
        let session = session.or(meta.as_ref().map(|meta| &meta.device));
        self.start_replay(path.clone(), session);
        self.notify_info(format!("Replaying {}", path.display()));
        self.close_link_popups();
        Ok(())
    }

    /// A replay asked while running: a file that doesn't open stops the command line chain
    fn replay_file(&mut self, path: PathBuf, delay: Duration, session: Option<CaptureDevice>) {
        if let Err(e) = self.open_replay(path.clone(), delay, session.as_ref()) {
            self.fail_commands(format!("replay: {}: {e}", path.display()));
        }
    }

    /// A group of sessions in place of the link (MAX-202): loaded at once, so its replay starts
    /// finished, on the newest session's file and device, in terminal mode as a replay does.
    /// Line mode holds it on a history of its own, which goes with the replay
    fn open_group_replay(&mut self, sessions: &[Session]) {
        let Some(newest) = sessions.last() else {
            return;
        };
        let Some(path) = newest.raw_path() else {
            return;
        };
        let replay = match Replay::loaded(path) {
            Ok(replay) => replay,
            Err(e) => return self.notify_warn(format!("Group not replayed: {e}")),
        };
        self.link.set_replay(replay);
        let device = newest.meta.as_ref().map(|meta| &meta.device);
        self.start_replay(path.to_path_buf(), device);
        if let Some(run) = &mut self.replay {
            run.ended = true;
            run.group = Some(sessions.len());
        }
        self.close_link_popups();
        let lines = GROUP_REPLAY_LINES.max(self.settings.line_mode.history_size);
        self.load_group(sessions, lines);
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
        // no device to attach yet: the directory to pick one (MAX-181)
        if self.device_unpicked {
            self.open_directory(crate::link::serial::port_names());
            return;
        }
        if self.link.is_open() {
            self.link.detach();
            self.show_detached();
            let message = format!("{} detached", self.link_kind().name());
            self.notify_link(ToastLevel::Warn, message);
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
        self.link_lost = true;
        self.open_overlay(Overlay::DeviceDisconnected);
    }

    /// While the link waits (no device picked yet, detached, lost), its popup is up again
    /// once nothing else is open: no app without its device behind a closed popup (MAX-181)
    fn restore_link_popup(&mut self) {
        if self.overlay.is_some() || self.link.is_open() {
            return;
        }
        let popup = if self.device_unpicked {
            Overlay::NoDevice
        } else if self.link.is_detached() {
            Overlay::DeviceDetached
        } else if self.link_lost {
            Overlay::DeviceDisconnected
        } else {
            return;
        };
        self.open_overlay(popup);
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
#[path = "../tests/app/device.rs"]
mod tests;
