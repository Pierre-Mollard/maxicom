//! The app driving its link (MAX-168): what the link does, said as notices and popups, the
//! chain resumed or failed, the session backup switched. `LinkState` keeps the link's own
//! rules. The loop calls `start_link` once, `poll_link` each turn and `dispatch_from` for each
//! action
use std::{
    io,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use ratatui::style::Color;

use super::{App, UpdateEffect, triggers::Trigger};
use crate::{
    capture::{CaptureDevice, RecordingMeta, Session, recording_raw},
    commands::{
        Origin,
        jobs::{Caller, Wait},
        report::{Note, Report},
    },
    configs::{
        AppConfig, KeyResolver, LinksConfig, PlotsConfig,
        links::{SavedLink, default_name},
        profiles,
    },
    event::{Cause, Event, Opened},
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
        if self.link_kind().is_tcp() {
            if let Some(note) = self.begin_open(OpenFor::Start) {
                self.show_note(note);
            }
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
    pub fn name_link(&mut self, address: Option<Address>) -> Option<Note> {
        match self.link_to_save(address) {
            Ok(link) => {
                let form = LinkForm::new(link, &self.settings().link);
                self.open_overlay(Overlay::LinkForm(Box::new(form)));
                None
            }
            Err(err) => Some(Note::warn(err)),
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

    /// `link` written to `links.toml`, or in place of `replaces`, with what to say; why not
    /// else
    pub fn store_link(
        &mut self,
        link: &SavedLink,
        replaces: Option<&SavedLink>,
    ) -> Result<Vec<Note>, String> {
        let path = AppConfig::active_links_path(self.settings());
        self.links = Ok(LinksConfig::put(&path, link.clone(), replaces)?);
        let saved = format!("Saved link '{}'", link.name);
        let mut notes = vec![Note::new(ToastLevel::Success, saved)];
        // saved all the same: the file can come after, the link's open says if it doesn't
        let missing = profiles::missing(&self.profiles_dir, &link.profiles);
        if !missing.is_empty() {
            let (names, dir) = (missing.join("', '"), self.profiles_dir.display());
            notes.push(Note::warn(format!("No profile '{names}' in {dir}")));
        }
        Ok(notes)
    }

    /// `link` taken out of `links.toml`, with what to say; why not else
    pub fn remove_link(&mut self, link: &SavedLink) -> Result<Note, String> {
        let path = AppConfig::active_links_path(self.settings());
        self.links = Ok(LinksConfig::remove(&path, link)?);
        let deleted = format!("Deleted link '{}'", link.name);
        Ok(Note::new(ToastLevel::Success, deleted))
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
        self.saved_at(address).map(|link| link.name.as_str())
    }

    /// The link saved for `address` on this machine, when there's one
    fn saved_at(&self, address: &Address) -> Option<&SavedLink> {
        self.links_here()
            .find(|link| link.address.parse::<Address>().ok().as_ref() == Some(address))
    }

    /// The profile stack laid over the config (MAX-240)
    pub fn profiles(&self) -> &[String] {
        &self.profiles
    }

    /// The stack laid at start: the link's, then `--profile`'s (MAX-240)
    pub fn set_profiles(&mut self, profiles: Vec<String>) {
        self.on_open = self.on_open_of(&profiles);
        self.on_load_due.extend(self.on_load_of(&profiles));
        self.profiles = profiles;
    }

    /// The `on_load` lines of the profiles of `profiles` not laid yet, in stack order, the
    /// ones that are on (MAX-247): a profile laid again over the others doesn't run theirs
    fn on_load_of(&self, profiles: &[String]) -> Vec<String> {
        (profiles.iter())
            .filter(|name| !self.profiles.contains(name))
            .filter_map(|name| self.profile(name).ok())
            .filter(|profile| profile.file.enabled)
            .filter_map(|profile| profile.file.on_load)
            .collect()
    }

    /// The `on_open` of the stack `profiles`, the later one that has it and is on (MAX-9),
    /// read with the stack: a profile that doesn't read has none
    fn on_open_of(&self, profiles: &[String]) -> Option<String> {
        (profiles.iter().rev())
            .filter_map(|name| self.profile(name).ok())
            .filter(|profile| profile.file.enabled)
            .find_map(|profile| profile.file.on_open)
    }

    /// The profiles' hooks that are due, each a job before the ones running (the rest of an
    /// `open` or a `profile load` chain), a hook that doesn't parse said:
    /// - the `on_load` of the profiles just laid (MAX-247), first: settings before the board
    ///   talks;
    /// - the stack's `on_open` once a link opened (MAX-9): at start, on an attach and on an
    ///   open, not on a reopen after a lost port (a board's native USB port drops on its
    ///   reset, it would loop), nor on a replay.
    pub fn run_profile_hooks(&mut self) {
        let mut hooks: Vec<(&str, String)> = (std::mem::take(&mut self.on_load_due).into_iter())
            .map(|chain| ("on_load", chain))
            .collect();
        if std::mem::take(&mut self.on_open_due)
            && let Some(chain) = self.on_open.clone()
        {
            hooks.push(("on_open", chain));
        }
        let mut parsed = Vec::new();
        for (hook, chain) in hooks {
            match crate::commands::parse_with(&chain, &self.key_resolver.commands) {
                Ok(chain) => parsed.push((Origin::Profile(hook), chain)),
                Err(err) => {
                    let text = format!("{hook}: {err}");
                    self.emit(Event::ConfigWarning { text });
                }
            }
        }
        // one job a hook, `on_load` first: the profiles' jobs run one after the other
        for (origin, chain) in parsed.into_iter().rev() {
            self.jobs.push_front(origin, chain);
        }
    }

    /// The stack of the link just opened, when it isn't the one laid, for the loop to lay it
    /// (MAX-240): a saved link's own, none for another
    pub fn take_profiles_due(&mut self) -> Option<Vec<String>> {
        self.profiles_due.take()
    }

    /// The profile `name` of the profiles folder, as it reads (MAX-240)
    pub fn profile(&self, name: &str) -> Result<profiles::Profile, profiles::ProfileError> {
        profiles::Profile::load(&self.profiles_dir, name)
    }

    /// The folder the profiles are read from (MAX-240)
    pub fn profiles_dir(&self) -> &std::path::Path {
        &self.profiles_dir
    }

    /// The profiles popup's rows: the folder's, then the ones laid that have no file, so
    /// they can be taken off (MAX-240)
    pub fn profile_rows(&self) -> Vec<profiles::Listed> {
        let mut rows = profiles::list(&self.profiles_dir);
        let gone: Vec<_> = (self.profiles.iter())
            .filter(|name| rows.iter().all(|row| row.name != **name))
            .map(|name| profiles::Listed {
                name: name.clone(),
                description: String::new(),
                enabled: true,
                error: Some("no file".to_string()),
            })
            .collect();
        rows.extend(gone);
        rows
    }

    /// The profiles to complete: the folder's that are on, then the ones laid, each once
    pub fn profile_names(&self) -> Vec<String> {
        let mut names = profiles::on_names(&self.profiles_dir);
        let laid = self.profiles.iter().filter(|name| !names.contains(name));
        names.extend(laid.cloned().collect::<Vec<_>>());
        names
    }

    /// Why the profile `name` can't be laid: no file, a file that doesn't read, or off
    /// (MAX-240)
    pub fn check_profile(&self, name: &str) -> Result<(), String> {
        let profile = self.profile(name).map_err(|err| err.to_string())?;
        if !profile.file.enabled {
            return Err(format!("'{name}' is off (enabled = false in its file)"));
        }
        Ok(())
    }

    /// The stack with `name` taken off when it's laid, else on top; refused, with the reason,
    /// for one that can't be laid (MAX-240)
    pub fn toggled_profile(&self, name: &str) -> Result<Vec<String>, String> {
        let mut stack = self.profiles.clone();
        if stack.iter().any(|own| own == name) {
            stack.retain(|own| own != name);
        } else {
            self.check_profile(name)?;
            stack.push(name.to_string());
        }
        Ok(stack)
    }

    /// The stack with `name` one place up (`up`, later, over more of the others) or down
    /// (MAX-240); none at the end it goes to, refused for one that isn't laid
    pub fn moved_profile(&self, name: &str, up: bool) -> Result<Option<Vec<String>>, String> {
        let Some(place) = self.profiles.iter().position(|own| own == name) else {
            return Err(format!("'{name}' isn't laid: lay it first"));
        };
        let other = if up {
            place + 1
        } else {
            match place.checked_sub(1) {
                Some(other) => other,
                None => return Ok(None),
            }
        };
        if other >= self.profiles.len() {
            return Ok(None);
        }
        let mut stack = self.profiles.clone();
        stack.swap(place, other);
        Ok(Some(stack))
    }

    /// The stack laid, said in its notice box (MAX-240)
    pub fn profiles_note(&self) -> Note {
        let message = match self.profiles.as_slice() {
            [] => "No profile laid".to_string(),
            profiles => format!("Profiles: {}", profiles.join(", ")),
        };
        Note::info(message).in_slot("profiles")
    }

    /// `profiles` for the loop to lay, none when they're the ones laid
    pub fn set_profiles_due(&mut self, profiles: Vec<String>) {
        self.profiles_due = (profiles != self.profiles).then_some(profiles);
    }

    /// The stack a replay plays with (MAX-240): its session's from the meta, then `extra`
    /// (`--profile`'s at start). A profile not found is said and left out, the replay running
    /// all the same
    fn replay_profiles(&mut self, session: Vec<String>, extra: &[String]) -> Option<Note> {
        let mut stack = session;
        stack.extend(extra.iter().cloned());
        let missing: Vec<String> = (profiles::missing(&self.profiles_dir, &stack).into_iter())
            .map(str::to_string)
            .collect();
        let note = (!missing.is_empty()).then(|| {
            let (names, dir) = (missing.join("', '"), self.profiles_dir.display());
            Note::warn(format!("Replay without profile '{names}': not in {dir}"))
        });
        stack.retain(|name| !missing.contains(name));
        self.set_profiles_due(stack);
        note
    }

    /// `profiles` laid over the config (MAX-240): what they change of it, the keys, the
    /// triggers and the plots. The settings stay as they are, toggles included, which a
    /// reload would put back from the file
    pub fn apply_profiles(
        &mut self,
        profiles: Vec<String>,
        key_resolver: KeyResolver,
        triggers: Vec<Trigger>,
        plots: PlotsConfig,
    ) {
        self.on_open = self.on_open_of(&profiles);
        self.on_load_due.extend(self.on_load_of(&profiles));
        self.profiles = profiles;
        self.key_resolver = key_resolver;
        self.prefix_active = false;
        self.set_triggers(triggers);
        self.set_plots(plots);
        self.warn_shadowed_keys();
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

    /// The address of the link in use, as it's written (MAX-285)
    pub fn link_address(&self) -> String {
        self.link.config().address.to_string()
    }

    /// The link in use as the notices and the link's popups name it: a saved one by its name,
    /// its address after it so the device stays findable (`board (/dev/ttyUSB0)`)
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
    pub fn save_link(&mut self, link: SavedLink, replaces: Option<SavedLink>) -> Report {
        match self.store_link(&link, replaces.as_ref()) {
            Ok(notes) => {
                self.open_directory(serial::port_names());
                if let Some(Overlay::Directory(directory)) = &mut self.overlay
                    && let Ok(links) = &directory.links
                    && let Some(idx) = links.iter().position(|(saved, _)| *saved == link)
                {
                    directory.tab = DirectoryTab::Saved;
                    directory.link_selected = idx;
                }
                Report::from(notes)
            }
            Err(err) => Report::from(Note::warn(format!("{}: {err}", link.name))),
        }
    }

    /// `link` taken out of `links.toml`, said, then the directory back on the saved links,
    /// the error said there when it wasn't (MAX-185)
    pub fn delete_link(&mut self, link: &SavedLink) -> Report {
        let note = match self.remove_link(link) {
            Ok(note) => note,
            Err(err) => Note::warn(format!("{}: {err}", link.name)),
        };
        self.open_directory(serial::port_names());
        if let Some(Overlay::Directory(directory)) = &mut self.overlay {
            directory.tab = DirectoryTab::Saved;
        }
        Report::from(note)
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
                // never busy here, and a failed write says itself (its event)
                let _ = self.transmit(&answers);
            }
        }

        // the break ends on time, the input poll waking the loop often enough (MAX-12)
        if let Some(Err(e)) = self.link.end_break_if_due(Instant::now()) {
            self.emit(Event::BreakFailed {
                error: e.to_string(),
            });
        }
        self.send_due(&mut turn);

        if self.link.is_open() {
            let timeout = self.settings.inactivity_detach_timeout;
            if self.link.detach_if_idle(timeout) {
                self.show_detached();
                let secs = timeout.as_secs();
                self.emit(Event::LinkIdle { secs });
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

    /// The action for `caller` (MAX-165), then the link effect it gives applied to the link,
    /// what that reports shown: what is left (quit, clipboard, reload) is the loop's
    pub fn dispatch_from(&mut self, caller: &Caller, action: Action) -> UpdateEffect {
        let (effect, mut report) = self.update_from(&caller.origin, action);
        let (effect, link_report) = self.apply_link_effect(&caller.origin, effect);
        report.extend(link_report);
        self.present(caller, report);
        self.restore_link_popup();
        effect
    }

    /// The link effect applied, what reaches the device or the link published with `origin`
    /// as its cause (MAX-284)
    fn apply_link_effect(
        &mut self,
        origin: &Origin,
        effect: UpdateEffect,
    ) -> (UpdateEffect, Report) {
        let report = match effect {
            UpdateEffect::Open(address) => {
                let line = self.link.config().line;
                self.open_link(origin, LinkConfig { address, line })
            }
            UpdateEffect::OpenLink(config) => self.open_link(origin, config),
            UpdateEffect::Replay {
                path,
                delay,
                session,
            } => self.replay_file(origin, path, delay, session),
            UpdateEffect::ReplayGroup(sessions) => self.open_group_replay(origin, sessions),
            UpdateEffect::StopReplay => self.stop_replay_link(origin),
            UpdateEffect::ToggleAttach => self.toggle_attach(origin),
            UpdateEffect::SetAttached(attach) => self.set_attached(origin, attach),
            UpdateEffect::Transmit(bytes) => match self.transmit(&bytes) {
                Ok(()) => {
                    self.link.note_activity();
                    Report::default()
                }
                Err(note) => Report::from(Vec::from_iter(note)),
            },
            UpdateEffect::Reconfigure(line) => self.reconfigure_link(origin, line),
            UpdateEffect::LineControl(control) => self.line_control(origin, control),
            other => return (other, Report::default()),
        };
        (UpdateEffect::None, report)
    }

    /// Writes `bytes` to the link, counted and recorded (not in a replay). Not written when the
    /// link is closed, lost by this write, or the write failed: said once by its event, not per
    /// key typed into it (MAX-199). Refused while a protocol transfer owns the link, with a note
    /// the first time (MAX-162): a key, `send`, a terminal answer would break a packet
    pub fn transmit(&mut self, bytes: &[u8]) -> Result<(), Option<Note>> {
        if self.link_busy() {
            return Err(self.say_link_busy());
        }
        match self.write_link(bytes, true) {
            true => Ok(()),
            false => Err(None),
        }
    }

    /// A write refused while a protocol transfer owns the link, said once per transfer
    pub fn say_link_busy(&mut self) -> Option<Note> {
        let said = std::mem::replace(&mut self.busy_said, true);
        let message = "A file transfer is running: cancel stops it";
        (!said).then(|| Note::warn(message).in_slot("transfer"))
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
            let link = self.link_kind().name();
            self.emit(Event::WriteFailed {
                link,
                error: e.to_string(),
            });
        }
    }

    /// The link closed under a read or a write: its popup, with the error that closed it when
    /// it isn't a plain close, and a notice (MAX-199)
    fn lose_link(&mut self, reason: Option<String>) {
        self.lost_reason = reason;
        self.show_disconnected();
        let link = self.link_kind().name();
        self.emit(Event::LinkLost { link });
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
                    let link = self.link_kind().name();
                    self.emit(Event::ReadFailed { link });
                    return Err(e);
                }
            }
        }
        // what the RFC 2217 server answered wrong (MAX-161)
        for text in self.link.take_warnings() {
            self.emit(Event::LinkWarning { text });
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
                    if let Err(note) = self.transmit(&bytes) {
                        if let Some(note) = note {
                            self.show_note(note);
                        }
                        self.stop_mash("the write failed");
                    }
                    turn.redraw = true;
                }
            } else {
                self.stop_mash("the link closed");
                turn.redraw = true;
            }
        }
        if !self.link.is_open() {
            self.fail_text_watches("the link closed");
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
            let (label, holder) = (label.clone(), holder.to_string());
            self.emit(Event::LinkShared { label, holder });
        }
        if !matches!(why, OpenFor::Retry) {
            self.on_open_due = true;
        }
        let how = match why {
            OpenFor::Start => Opened::Connected,
            OpenFor::Attach | OpenFor::Retry => Opened::Reconnected,
            OpenFor::Open { .. } => Opened::Switched,
        };
        self.emit(Event::LinkOpened { label, how });
        // the levels its line asks, set by the open itself (MAX-9)
        if self.link_kind().has_line_settings() {
            let (dtr, rts) = self.link.levels();
            for (line, level) in [("dtr", dtr), ("rts", rts)] {
                if let Some(on) = level.level() {
                    let cause = Cause::link();
                    self.emit(Event::LineLevel { line, on, cause });
                }
            }
        }
        match why {
            OpenFor::Start => {}
            OpenFor::Attach => {
                self.close_overlay();
                let opening = |wait: &Wait| matches!(wait, Wait::LinkOpen { .. });
                self.complete_waiting(opening, Report::default());
            }
            OpenFor::Retry => {
                if let Some(Overlay::DeviceDisconnected) = self.overlay() {
                    self.close_overlay();
                }
            }
            OpenFor::Open { previous } => {
                let switched = previous.address != self.link.config().address;
                let profiles = self.opening_profiles.take().unwrap_or_default();
                self.set_profiles_due(profiles);
                if let Some(Overlay::Directory(_)) = self.overlay() {
                    self.close_overlay();
                }
                // the first port picked from the start's directory gets its backup too; taken
                // before the test, or a switched pick left it set (MAX-181)
                let first_pick = std::mem::take(&mut self.device_unpicked);
                if switched || first_pick {
                    self.switch_session();
                }
                let opening = |wait: &Wait| matches!(wait, Wait::LinkOpen { .. });
                self.complete_waiting(opening, Report::default());
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
                self.emit(Event::LinkNotOpened {
                    error: e.to_string(),
                });
                if matches!(self.overlay(), Some(Overlay::RestoreSession(_))) {
                    self.disconnected_after_restore = true;
                } else {
                    self.show_disconnected();
                }
            }
            // still detached, its popup back: no quiet retries of a port held elsewhere
            OpenFor::Attach => {
                self.emit(Event::ReattachFailed {
                    error: e.to_string(),
                });
                self.link.detach();
                if self.overlay.is_none() {
                    self.show_detached();
                }
                // its toast said it, and the detached popup is back: `attach`'s job only stops
                let failed = Report::shown(format!("attach: {e}"));
                self.complete_waiting(|wait| matches!(wait, Wait::LinkOpen { .. }), failed);
            }
            OpenFor::Retry => {}
            // nothing was open before the first pick: the link waits again (MAX-181)
            OpenFor::Open { .. } if self.device_unpicked => {
                let error = e.to_string();
                self.emit(Event::OpenFailed { error, back: None });
                self.link.detach();
                let failed = Report::failed(format!("open: {e}"));
                self.complete_waiting(|wait| matches!(wait, Wait::LinkOpen { .. }), failed);
            }
            // the retries reopen the port that was open, which the link is back on
            OpenFor::Open { .. } => {
                self.opening_profiles = None;
                let back = self.link_label();
                let (error, label) = (e.to_string(), back.clone());
                self.emit(Event::OpenFailed {
                    error,
                    back: Some(label),
                });
                let failed = Report::failed(format!("open: {e}, back to {back}"));
                self.complete_waiting(|wait| matches!(wait, Wait::LinkOpen { .. }), failed);
            }
        }
    }

    /// Starts opening the link for `why`; a TCP link says it connects, which takes a while
    fn begin_open(&mut self, why: OpenFor) -> Option<Note> {
        self.link.begin_open(why);
        let connecting = || format!("Connecting to {}", self.link_label());
        (self.link_kind().is_tcp()).then(|| link_note(ToastLevel::Info, connecting()))
    }

    /// The current link closes and the new one opens as a reconnect does, without waiting: the rest
    /// of the chain waits for its result (MAX-12). A replay that set the live link aside
    /// ends first, so the switch starts from the live link, its session backup and its encoding
    /// (MAX-171)
    fn open_link(&mut self, origin: &Origin, config: LinkConfig) -> Report {
        if self.replaced_link().is_some() {
            self.stop_replay();
        }
        // the stack goes with the link once it opened, a failed open going back to both: a
        // saved link's own, then `--profile`'s on the first pick from the start's directory
        let own = self
            .saved_at(&config.address)
            .map(|link| link.profiles.clone());
        let mut profiles = own.unwrap_or_default();
        if self.device_unpicked {
            profiles.extend(self.profiles.iter().cloned());
        }
        self.opening_profiles = Some(profiles);
        self.link.switch_to(config);
        let label = self.link_label();
        let cause = Cause::from(origin);
        self.emit(Event::LinkOpening { label, cause });
        let message = format!("Opening {}", self.link_label());
        self.close_link_popups();
        // the rest of the chain waits for the open's result, a reconnect's time at most
        let until = Instant::now() + CONNECT_TIMEOUT * 2;
        Report {
            wait: Some(Wait::LinkOpen { until }),
            ..Report::from(link_note(ToastLevel::Info, message))
        }
    }

    /// The file in place of the link, which closes: no wait, a file opens at once (MAX-166).
    /// For `replay` and `open file://` while running, and for `--replay` at start (MAX-168).
    /// `path` is a recording: its `.raw`, its `.meta` or its folder; anything else is refused
    /// (MAX-175). Without a `session`, the recording's meta file gives the device, when it has
    /// one (MAX-163). It plays with its meta's profile stack, `extra` over it (MAX-240). The
    /// notes say what it played with, for the caller to show
    pub fn open_replay(
        &mut self,
        path: PathBuf,
        delay: Duration,
        session: Option<&CaptureDevice>,
        extra: &[String],
    ) -> io::Result<Vec<Note>> {
        let path = recording_raw(&path)?;
        let replay = Replay::open(&path, delay)?;
        self.link.set_replay(replay);
        let meta = RecordingMeta::read(&RecordingMeta::path_of(&path));
        let session = session.or(meta.as_ref().map(|meta| &meta.device));
        self.start_replay(path.clone(), session);
        let profiles = meta.map(|meta| meta.profiles).unwrap_or_default();
        let mut notes: Vec<Note> = self.replay_profiles(profiles, extra).into_iter().collect();
        notes.push(Note::info(format!("Replaying {}", path.display())));
        self.close_link_popups();
        Ok(notes)
    }

    /// A replay asked while running: a file that doesn't open stops the chain
    fn replay_file(
        &mut self,
        origin: &Origin,
        path: PathBuf,
        delay: Duration,
        session: Option<CaptureDevice>,
    ) -> Report {
        match self.open_replay(path.clone(), delay, session.as_ref(), &[]) {
            Ok(notes) => {
                self.emit(Event::ReplayStarted {
                    path: path.display().to_string(),
                    sessions: 1,
                    cause: Cause::from(origin),
                });
                Report::from(notes)
            }
            Err(e) => Report::failed(format!("replay: {}: {e}", path.display())),
        }
    }

    /// A group of sessions in place of the link (MAX-202): loaded, so its replay starts
    /// finished, on the newest session's file, device and profiles, in terminal mode as a
    /// replay does. Line mode holds it on a history of its own, which goes with the replay.
    /// The loading popup shows while it loads, a slice a loop turn (MAX-272)
    fn open_group_replay(&mut self, origin: &Origin, sessions: Vec<Session>) -> Report {
        let Some(newest) = sessions.last() else {
            return Report::default();
        };
        let Some(path) = newest.raw_path() else {
            return Report::default();
        };
        let replay = match Replay::loaded(path) {
            Ok(replay) => replay,
            Err(e) => return Report::from(Note::warn(format!("Group not replayed: {e}"))),
        };
        self.link.set_replay(replay);
        self.emit(Event::ReplayStarted {
            path: path.display().to_string(),
            sessions: sessions.len(),
            cause: Cause::from(origin),
        });
        let device = newest.meta.as_ref().map(|meta| &meta.device);
        self.start_replay(path.to_path_buf(), device);
        let profiles = (newest.meta.as_ref()).map(|meta| meta.profiles.clone());
        let note = self.replay_profiles(profiles.unwrap_or_default(), &[]);
        let count = sessions.len();
        self.close_link_popups();
        let lines = GROUP_REPLAY_LINES.max(self.settings.line_mode.history_size);
        let load = self.start_group_load(sessions, lines);
        if let Some(run) = &mut self.replay {
            run.ended = true;
            run.group = Some(count);
            run.loading = Some(Box::new(load));
        }
        self.open_overlay(Overlay::Loading);
        Report::from(Vec::from_iter(note))
    }

    /// The link the replay replaced opens again as at start, its popup if it doesn't; a replay
    /// from the command line just stops reading
    fn stop_replay_link(&mut self, origin: &Origin) -> Report {
        let mut notes = Vec::new();
        // a group still loading stops where it is (MAX-272)
        if let Some(run) = &mut self.replay
            && run.loading.take().is_some()
        {
            notes.push(Note::info("Group load stopped"));
        }
        self.link.close();
        self.emit(Event::ReplayStopped {
            cause: Cause::from(origin),
        });
        if self.stop_replay() {
            notes.extend(self.begin_open(OpenFor::Start));
            let (label, cause) = (self.link_label(), Cause::from(origin));
            self.emit(Event::LinkOpening { label, cause });
        } else {
            notes.push(Note::info("Replay stopped"));
        }
        Report::from(notes)
    }

    /// Detaches the link, with its popup, or attaches it again; a replay has nothing to
    /// attach. The attach's result comes with the reopen's, next turn for a port: the rest of
    /// the chain waits for it (MAX-219)
    fn toggle_attach(&mut self, origin: &Origin) -> Report {
        if !self.link_kind().reopens() {
            return Report::default();
        }
        // no device to attach yet: the directory to pick one (MAX-181)
        if self.device_unpicked {
            self.open_directory(crate::link::serial::port_names());
            return Report::default();
        }
        if self.link.is_open() {
            self.detach_link(origin)
        } else {
            self.attach_link(origin)
        }
    }

    /// `attach` / `detach` (MAX-219): nothing when the link is so already. A replay has no
    /// link to set, nor has the start's directory before a pick
    fn set_attached(&mut self, origin: &Origin, attach: bool) -> Report {
        let verb = if attach { "attach" } else { "detach" };
        if !self.link_kind().reopens() {
            return Report::failed(format!("{verb}: a replay has no link"));
        }
        if self.device_unpicked {
            return Report::failed(format!("{verb}: no device picked yet"));
        }
        match attach {
            true if self.link.is_open() => Report::default(),
            false if self.link.is_detached() => Report::default(),
            true => self.attach_link(origin),
            false => self.detach_link(origin),
        }
    }

    /// The link opens again, the rest of the chain waiting for its result as `open`'s does
    fn attach_link(&mut self, origin: &Origin) -> Report {
        let opening = self.begin_open(OpenFor::Attach);
        let (label, cause) = (self.link_label(), Cause::from(origin));
        self.emit(Event::LinkOpening { label, cause });
        let until = Instant::now() + CONNECT_TIMEOUT * 2;
        Report {
            wait: Some(Wait::LinkOpen { until }),
            ..Report::from(Vec::from_iter(opening))
        }
    }

    /// The link closes until an attach, with its popup: a reconnect running stops too
    fn detach_link(&mut self, origin: &Origin) -> Report {
        self.link.detach();
        self.show_detached();
        let cause = Cause::from(origin);
        self.emit(Event::LinkDetached { cause });
        let message = format!("{} detached", self.link_kind().name());
        Report::from(link_note(ToastLevel::Warn, message))
    }

    /// New line settings: a port that refuses them stops the chain, the dialog
    /// says why (MAX-12)
    /// New line settings; DTR and RTS changed on an open port wait for its next open (MAX-9)
    /// Set or kept for the next open, the settings are published (MAX-284)
    fn reconfigure_link(&mut self, origin: &Origin, line: LineSettings) -> Report {
        let old = self.link_config().line;
        let levels_changed = (old.dtr, old.rts) != (line.dtr, line.rts);
        let reconfigured = self.link.reconfigure(line);
        if let Reconfigured::Applied | Reconfigured::Kept = reconfigured {
            self.emit(Event::LineSettings {
                baud: line.baud,
                framing: line.framing.to_string(),
                flow: line.flow_control,
                cause: Cause::from(origin),
            });
        }
        match reconfigured {
            Reconfigured::Applied if levels_changed => {
                Report::from(Note::info("DTR and RTS levels are set at the next open"))
            }
            Reconfigured::Applied | Reconfigured::Ignored => Report::default(),
            Reconfigured::Kept => Report::from(Note::info(
                "The port is detached: it opens with these settings",
            )),
            Reconfigured::Refused(e) => {
                let message = format!("Failed to reconfigure serial: {e}");
                self.open_overlay(Overlay::Error(
                    ErrorDialog::new("Serial Conf Rejected", "Failed to reconfigure serial")
                        .with_details(e),
                ));
                Report::shown(message)
            }
        }
    }

    /// A control line set; the rest of the chain waits for a break's end, which the next turn
    /// comes to first
    fn line_control(&mut self, origin: &Origin, control: LineControl) -> Report {
        let now = Instant::now();
        match self.link.control(control, now) {
            None => return Report::failed(format!("{control}: the port is detached")),
            Some(Err(e)) => return Report::failed(format!("{control}: {e}")),
            Some(Ok(())) => {}
        }
        let cause = Cause::from(origin);
        match control {
            LineControl::Dtr(on) => {
                self.emit(Event::LineLevel {
                    line: "dtr",
                    on,
                    cause,
                });
                Report::default()
            }
            LineControl::Rts(on) => {
                self.emit(Event::LineLevel {
                    line: "rts",
                    on,
                    cause,
                });
                Report::default()
            }
            LineControl::Break(duration) => {
                let ms = duration.as_millis() as u64;
                self.emit(Event::BreakSent { ms, cause });
                Report::waiting(Wait::Until(now + duration))
            }
        }
    }

    /// A replay control on the replay reading, said with a notice (MAX-166)
    pub fn control_replay(&mut self, origin: &Origin, control: ReplayControl) -> Report {
        let Some(replay) = self.link.replay_mut() else {
            return Report::default();
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
                Err(e) => return Report::failed(format!("replay-restart: {e}")),
            },
        };
        let (control, delay_ms) = match control {
            ReplayControl::Pause => ("pause", None),
            ReplayControl::Resume => ("resume", None),
            ReplayControl::Delay(delay) => ("delay", Some(delay.as_millis() as u64)),
            ReplayControl::Restart => ("restart", None),
        };
        let cause = Cause::from(origin);
        self.emit(Event::ReplayControl {
            control,
            delay_ms,
            cause,
        });
        Report::from(Note::info(notice))
    }

    fn show_disconnected(&mut self) {
        self.link_lost = true;
        self.open_overlay(Overlay::DeviceDisconnected);
    }

    /// While the link waits (no device picked yet, detached, lost), its popup is up again
    /// once nothing else is open: no app without its device behind a closed popup (MAX-181)
    pub fn restore_link_popup(&mut self) {
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

/// A note in the link's box, as the link's events go: connecting then connected, lost then
/// reconnected, detached then attached (MAX-228)
fn link_note(level: ToastLevel, text: impl Into<String>) -> Note {
    Note::new(level, text).in_slot("link")
}

#[cfg(test)]
#[path = "../tests/app/device.rs"]
mod tests;
