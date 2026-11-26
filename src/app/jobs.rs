//! Running chains as jobs (MAX-12, MAX-165): their commands built one at a time on the state
//! the previous ones left, a job waiting for what its command reported (a `sleep`, a
//! `break`, an `open`, a `mash`), stopped on its failure, or cancelled. `commands::jobs` keeps
//! the jobs, `commands` parses and builds their commands, the loop applies their effects
use std::time::Instant;

use super::{App, RECANCEL_FOR, cancelled_text};
use crate::link::CONNECT_TIMEOUT;
use crate::{
    api::READING_OFF,
    commands::{
        Access, CommandError, Invocation, Origin, Target,
        jobs::{Caller, JobId, Wait},
        report::Note,
        report::{Answer, Failure, Report},
    },
    configs::{KeyBindingAction, Level, ReadLevel},
    inputs::{Action, AppAction},
    overlays::{AskAnswer, Overlay, QueryView},
};

/// A client's command held for the user's answer (MAX-287)
#[derive(Debug, Clone)]
pub struct Ask {
    pub job: JobId,
    /// The client asking: its name for the popup, its connection for an always
    pub client: Origin,
    /// The command as typed
    pub command: String,
    pub access: Access,
    /// Refused when no answer came by then
    pub until: Instant,
}

/// What the `[api]` levels say of a client's command (MAX-287)
enum Gate {
    /// It runs
    Run,
    /// It is refused, saying why
    Refuse(String),
    /// The user is asked, for this access
    Ask(Access),
}

impl App {
    /// The next action of a job (MAX-12), with who it runs for (MAX-165): the oldest job's
    /// that doesn't wait. Each is built when its turn comes, on the state the previous ones
    /// left, and the loop applies its effect before the next. A command that refuses (the link
    /// can't do it) stops its job, with the error. None while every job waits, or while the
    /// restore dialog asks, as the device an `open` switched to may (MAX-187)
    pub fn next_command_action(&mut self, now: Instant) -> Option<(Caller, Action)> {
        if matches!(self.overlay, Some(Overlay::RestoreSession(_))) {
            return None;
        }
        // an `open` with no answer by its time fails, rather than the next command finding
        // the link closed
        for job in self.jobs.unopened(now) {
            let secs = (CONNECT_TIMEOUT * 2).as_secs();
            self.fail_job(job, format!("open: no answer after {secs}s"));
        }
        self.sync_asks(now);
        let (caller, invocation) = self.jobs.next(now)?;
        // a popup or a key to wait for would take the user's keys: theirs to run (MAX-165);
        // a client runs what the user's `[api]` levels let it (MAX-287)
        let asks = (!caller.origin.is_user()).then(|| invocation.asks_user());
        let refused = match asks.flatten() {
            Some(reason) => Some(reason.to_string()),
            None => match self.client_gate(&caller, &invocation) {
                Gate::Run => None,
                Gate::Refuse(reason) => Some(reason),
                // held, its job waiting: the other jobs go on
                Gate::Ask(access) => {
                    self.hold_for_user(&caller, invocation, access, now);
                    return self.next_command_action(now);
                }
            },
        };
        let built = match (refused, invocation.spec.target) {
            (Some(reason), _) => Err(reason),
            (None, Target::Action(action)) => Ok(action.to_action(self)),
            (None, Target::Popup { action, .. }) if caller.origin.is_user() => {
                Ok(action.to_action(self))
            }
            (None, Target::Build(build)) => build(self, &invocation.args),
            // run at once: a query changes nothing, its answer goes to who asked
            (None, Target::Query(query) | Target::Popup { query, .. }) => {
                query(self, &invocation.args).map(|value| {
                    let query = invocation.spec.name;
                    Action::App(AppAction::Answer(Box::new(Answer { query, value })))
                })
            }
        };
        match built {
            Ok(action) => Some((caller, action)),
            Err(reason) => {
                let error = CommandError::Failed {
                    command: invocation.spec.name,
                    reason,
                };
                if let Some(job) = caller.job {
                    self.fail_job(job, error.to_string());
                }
                None
            }
        }
    }

    /// What the user's `[api]` levels say of a client's command (MAX-287), as its access
    /// says: run, refused, or asked; any other origin runs. An `ask` the user answered for this
    /// job, or for this client always, runs
    fn client_gate(&mut self, caller: &Caller, invocation: &Invocation) -> Gate {
        let Origin::Client { conn, .. } = caller.origin else {
            return Gate::Run;
        };
        let api = self.settings.api;
        let access = invocation.access();
        let (level, what) = match access {
            Access::Read if api.read == ReadLevel::Off => {
                return Gate::Refuse(READING_OFF.to_string());
            }
            Access::Read => return Gate::Run,
            Access::Control => (api.control, "control"),
            Access::Device => (api.device, "device"),
        };
        match level {
            Level::On => Gate::Run,
            Level::Off => Gate::Refuse(format!("{what} access is off in [api]")),
            Level::Ask if self.allowed_always.contains(&(conn, access)) => Gate::Run,
            Level::Ask => {
                let once = (self.allowed_once.iter()).position(|job| Some(*job) == caller.job);
                match once {
                    Some(at) => {
                        self.allowed_once.remove(at);
                        Gate::Run
                    }
                    None => Gate::Ask(access),
                }
            }
        }
    }

    /// A client's command held for the user's answer (MAX-287): back at the front of its job,
    /// which waits for the answer, refused after `[api] ask_timeout`
    fn hold_for_user(
        &mut self,
        caller: &Caller,
        invocation: Invocation,
        access: Access,
        now: Instant,
    ) {
        let Some(job) = caller.job else {
            return;
        };
        let until = now + self.settings.api.ask_timeout;
        let command = invocation.text();
        self.jobs.put_back(job, invocation);
        self.jobs.wait(job, Wait::Asked { until });
        self.asks.push_back(Ask {
            job,
            client: caller.origin.clone(),
            command,
            access,
            until,
        });
        self.sync_asks(now);
    }

    /// An ask refused, by the user or for want of an answer: its held command dropped, then
    /// its job failed with `why`
    fn refuse_ask(&mut self, ask: Ask, why: &str) {
        self.jobs.skip(ask.job);
        self.fail_job(ask.job, format!("{}: {why}", ask.command));
    }

    /// The asks kept in step with their jobs (MAX-287): one with no answer by its time is
    /// refused, one whose job is gone (cancelled, failed) dropped; the first one shown when no
    /// other popup is open, so it never takes the keys of one the user is in
    fn sync_asks(&mut self, now: Instant) {
        while let Some(at) = self.asks.iter().position(|ask| now >= ask.until) {
            if let Some(ask) = self.asks.remove(at) {
                self.refuse_ask(ask, "no answer from the user");
            }
        }
        let jobs = &self.jobs;
        self.asks.retain(|ask| jobs.has(ask.job));
        let first = self.asks.front().map(|ask| ask.job);
        if let Some(Overlay::ClientAsk(shown)) = self.overlay
            && Some(shown) != first
        {
            self.overlay = None;
        }
        if self.overlay.is_none()
            && let Some(job) = first
        {
            self.open_overlay(Overlay::ClientAsk(job));
        }
    }

    /// Every client's jobs cancelled and what the user let them do forgotten, their
    /// connections closing (MAX-287): their asks go with their jobs
    pub fn drop_clients(&mut self) {
        self.jobs.cancel_clients();
        self.allowed_once.clear();
        self.allowed_always.clear();
        self.sync_asks(Instant::now());
    }

    /// The client's command asked first, the one its popup shows (MAX-287)
    pub fn asked(&self) -> Option<&Ask> {
        self.asks.front()
    }

    /// The user's answer to the command asked (MAX-287): sent runs it, always runs it and the
    /// next ones of its access from that client without asking, refused stops its job
    pub fn answer_ask(&mut self, answer: AskAnswer) {
        if matches!(self.overlay, Some(Overlay::ClientAsk(_))) {
            self.overlay = None;
        }
        let Some(ask) = self.asks.pop_front() else {
            return;
        };
        match answer {
            AskAnswer::Send => self.allowed_once.push(ask.job),
            AskAnswer::Always => {
                if let Origin::Client { conn, .. } = ask.client {
                    self.allowed_always.push((conn, ask.access));
                }
            }
            AskAnswer::Refuse => {
                self.refuse_ask(ask, "refused by the user");
                return;
            }
        }
        self.jobs.resume(ask.job);
    }

    /// What an action for `caller` reported (MAX-165), shown: its notes as toasts, then its
    /// failure, or what its job waits for. A job's failure stops the rest of it, quietly when a
    /// popup shows it; the user's own input has no job, its failure is only said
    pub fn present(&mut self, caller: &Caller, report: Report) {
        for note in report.notes {
            self.show_note(note);
        }
        // a query's answer opens its popup for the user; a trigger's or a profile's is nobody's
        if let Some(answer) = report.answer
            && caller.origin.is_user()
        {
            self.open_overlay(Overlay::Query(QueryView::new(answer)));
        }
        match (report.failure, caller.job) {
            (Some(Failure::Said(message)), Some(job)) => self.fail_job(job, message),
            (Some(Failure::Shown(_)), Some(job)) => {
                self.jobs.fail(job);
            }
            (Some(Failure::Said(message)), None) => self.notify_error(message),
            (Some(Failure::Shown(_)), None) | (None, None) => {}
            (None, Some(job)) => {
                if let Some(wait) = report.wait {
                    self.jobs.wait(job, wait);
                }
            }
        }
    }

    /// A command of `job` failed with `message`: the rest of it is dropped, the error says how
    /// much
    pub fn fail_job(&mut self, job: JobId, message: String) {
        match self.jobs.fail(job) {
            0 => self.notify_error(message),
            n => self.notify_error(format!("{message} ({n} more not run)")),
        }
    }

    /// A delayed result (MAX-165): what a job waited for came (the link opened, the mash's text
    /// came in, the file went through), its `report` presented for that job, which goes on or
    /// stops on its failure. With no job waiting, it is the user's: the directory's `open`
    /// isn't a command
    pub fn complete_waiting(&mut self, waits: impl Fn(&Wait) -> bool, report: Report) {
        match self.jobs.waiting(waits) {
            Some(caller) => {
                if let Some(job) = caller.job {
                    self.jobs.resume(job);
                }
                self.present(&caller, report);
            }
            None => self.present(&Caller::input(), report),
        }
    }

    /// A chain to run (MAX-12), started by `origin` (MAX-165), as a job after the ones running;
    /// one starting with `cancel` cancels first instead of waiting behind them, which it
    /// reports
    pub fn queue_chain(&mut self, origin: Origin, mut invocations: Vec<Invocation>) -> Report {
        let cancels = invocations.first().is_some_and(|invocation| {
            matches!(
                invocation.spec.target,
                Target::Action(KeyBindingAction::CancelCommands)
            )
        });
        let mut report = Report::default();
        if cancels {
            invocations.remove(0);
            report = self.cancel(&origin);
        }
        self.jobs.push(origin, invocations);
        report
    }

    /// `cancel` from `origin` (MAX-12, MAX-165), which it reports. The user's stops the file
    /// being sent, the mash and every job; a trigger's or a profile's only the jobs of its
    /// owner, and the mash one of them waited for
    pub fn cancel(&mut self, origin: &Origin) -> Report {
        let user = origin.is_user();
        let mut notes = Vec::new();
        // the transfer says it in its own box, "Cancelled", where a failure says "Stopped"
        // (MAX-228)
        let transfer = if user { self.transfer.take() } else { None };
        let transfer_cancelled = transfer.is_some();
        if let Some(mut transfer) = transfer {
            // a protocol's receiver is told to stop too
            let cans = transfer.cancel();
            if !cans.is_empty() {
                self.write_link(&cans, false);
                self.recancel_until = Some(Instant::now() + RECANCEL_FOR);
            }
            notes.push(Note::warn(cancelled_text(&transfer)).in_slot("transfer"));
        }
        let cancelled = self.jobs.cancel(origin);
        let mut stopped = Vec::new();
        if (user || cancelled.mash) && self.mash.take().is_some() {
            stopped.push("Stopped mashing".to_string());
        }
        match cancelled.commands {
            0 => {}
            1 => stopped.push("dropped 1 waiting command".to_string()),
            n => stopped.push(format!("dropped {n} waiting commands")),
        }
        if !stopped.is_empty() {
            let mut message = stopped.join(", ");
            message[..1].make_ascii_uppercase();
            notes.push(Note::warn(message));
        } else if !transfer_cancelled {
            notes.push(Note::info("Nothing to cancel"));
        }
        Report::from(notes)
    }
}

#[cfg(test)]
#[path = "../tests/app/jobs.rs"]
mod tests;
