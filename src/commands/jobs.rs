//! Jobs (MAX-165): the chains running, each with who started it, one command at a time.
//! Jobs don't wait for each other: one that waits (a `sleep`, an `open`, a `mash`) lets the
//! others go on, and one that fails drops only its own commands. The chains of one owner
//! (the user's, a trigger's, the profiles', a client's) still run one after the other, the oldest
//! first. Pure: `App` builds the commands and applies their effects (`app::jobs`)
use std::{collections::VecDeque, time::Instant};

use super::{Invocation, Origin};

/// A job's number, unique while the app runs
pub type JobId = u64;

/// Who an action runs for: who started it, and the job it is a command of; none for the
/// user's own input, which isn't part of a chain
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub origin: Origin,
    pub job: Option<JobId>,
}

impl Caller {
    /// The user's own input: a key, a click, a popup's answer
    pub fn input() -> Self {
        Self {
            origin: Origin::Input,
            job: None,
        }
    }
}

/// What a job waits for before its next command, as the command that made it wait reports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// A time: a `sleep`, a `break`'s end
    Until(Instant),
    /// The link an `open` switched to, opened or failed; `until` at the latest
    LinkOpen { until: Instant },
    /// A `mash`'s text coming in; the mash ends on its own timeout
    Mash,
    /// A file going through (`send-file`, `upload`, `download`) to its end; a stop on an
    /// error fails the job, its own toast saying why
    Transfer,
    /// The user's answer to a client's command the `[api]` levels ask about (MAX-287); refused
    /// at `until`
    Asked { until: Instant },
}

/// What `cancel` took away: how many commands were waiting, and whether a job waited for the
/// mash, which stops then
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cancelled {
    pub commands: usize,
    pub mash: bool,
}

/// A chain running: its commands still to come, and what it waits for
#[derive(Debug)]
pub struct Job {
    id: JobId,
    origin: Origin,
    commands: VecDeque<Invocation>,
    wait: Option<Wait>,
}

impl Job {
    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Its commands not run yet, the one due first
    pub fn commands(&self) -> &VecDeque<Invocation> {
        &self.commands
    }

    pub fn wait(&self) -> Option<Wait> {
        self.wait
    }
}

/// Whose a job is, for its turn and for `cancel`: one owner's chains run one after the other
#[derive(Debug, PartialEq, Eq)]
enum Owner<'a> {
    /// The user: their input, the command line, their key bindings
    User,
    /// A trigger, by its name
    Trigger(&'a str),
    /// The profiles' hooks
    Profile,
    /// An API connection, by its number (MAX-287): each client's chains in order, beside
    /// the others'
    Client(u64),
}

fn owner(origin: &Origin) -> Owner<'_> {
    match origin {
        Origin::Input | Origin::CommandLine | Origin::KeyBinding => Owner::User,
        Origin::Trigger(name) => Owner::Trigger(name),
        Origin::Profile(_) => Owner::Profile,
        Origin::Client { conn, .. } => Owner::Client(*conn),
    }
}

/// The jobs running, the oldest first
#[derive(Debug, Default)]
pub struct Jobs {
    jobs: VecDeque<Job>,
    next_id: JobId,
}

impl Jobs {
    /// A chain started by `origin`, after the jobs running; none for an empty one
    pub fn push(&mut self, origin: Origin, chain: Vec<Invocation>) -> Option<JobId> {
        let job = self.job(origin, chain)?;
        self.jobs.push_back(job);
        Some(self.next_id - 1)
    }

    /// A chain that goes before the jobs running: a profile's hooks, settings before the
    /// board talks (MAX-247)
    pub fn push_front(&mut self, origin: Origin, chain: Vec<Invocation>) -> Option<JobId> {
        let job = self.job(origin, chain)?;
        self.jobs.push_front(job);
        Some(self.next_id - 1)
    }

    fn job(&mut self, origin: Origin, chain: Vec<Invocation>) -> Option<Job> {
        if chain.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        Some(Job {
            id,
            origin,
            commands: chain.into(),
            wait: None,
        })
    }

    /// The next command due at `now`: the oldest job's that doesn't wait, and that no older
    /// job of its owner holds back. A time waited for is over at `now`; a job done is gone
    pub fn next(&mut self, now: Instant) -> Option<(Caller, Invocation)> {
        self.jobs
            .retain(|job| !job.commands.is_empty() || job.wait.is_some());
        let mut owners = Vec::new();
        for job in &mut self.jobs {
            let job_owner = owner(&job.origin);
            if owners.contains(&job_owner) {
                continue;
            }
            owners.push(job_owner);
            match job.wait {
                Some(Wait::Until(until)) if now >= until => job.wait = None,
                Some(_) => continue,
                None => {}
            }
            let Some(invocation) = job.commands.pop_front() else {
                continue;
            };
            let caller = Caller {
                origin: job.origin.clone(),
                job: Some(job.id),
            };
            return Some((caller, invocation));
        }
        None
    }

    /// The jobs running, the oldest first (MAX-283): one with no command left and nothing to
    /// wait for is done, out of the list even before `next` drops it, as the job of the query
    /// reading the list is when the query is its last command
    pub fn running(&self) -> impl Iterator<Item = &Job> {
        (self.jobs.iter()).filter(|job| !job.commands.is_empty() || job.wait.is_some())
    }

    /// `job` waits for `wait` before its next command
    pub fn wait(&mut self, job: JobId, wait: Wait) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job) {
            job.wait = Some(wait);
        }
    }

    /// Who the job waiting for what `waits` tells runs for, if one does
    pub fn waiting(&self, waits: impl Fn(&Wait) -> bool) -> Option<Caller> {
        let mut jobs = self.jobs.iter();
        let job = jobs.find(|job| job.wait.as_ref().is_some_and(&waits))?;
        Some(Caller {
            origin: job.origin.clone(),
            job: Some(job.id),
        })
    }

    /// `invocation` back at the front of `job`, due again: a command held for the user's
    /// answer (MAX-287)
    pub fn put_back(&mut self, job: JobId, invocation: Invocation) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job) {
            job.commands.push_front(invocation);
        }
    }

    /// `job`'s next command dropped, not run
    pub fn skip(&mut self, job: JobId) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job) {
            job.commands.pop_front();
        }
    }

    /// Whether `job` still runs: not done, failed or cancelled
    pub fn has(&self, job: JobId) -> bool {
        self.running().any(|running| running.id == job)
    }

    /// `job` waits no more: its next command is due
    pub fn resume(&mut self, job: JobId) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job) {
            job.wait = None;
        }
    }

    /// The jobs whose `open` had no answer by its time at `now`, for the app to fail
    pub fn unopened(&self, now: Instant) -> Vec<JobId> {
        let late = |job: &&Job| matches!(job.wait, Some(Wait::LinkOpen { until }) if now >= until);
        self.jobs.iter().filter(late).map(|job| job.id).collect()
    }

    /// `job` ends: how many of its commands didn't run
    pub fn fail(&mut self, job: JobId) -> usize {
        match self.jobs.iter().position(|j| j.id == job) {
            Some(at) => self.jobs.remove(at).map_or(0, |job| job.commands.len()),
            None => 0,
        }
    }

    /// Every client's jobs ended (MAX-287), the others kept
    pub fn cancel_clients(&mut self) {
        self.jobs
            .retain(|job| !matches!(owner(&job.origin), Owner::Client(_)));
    }

    /// `cancel` from `origin`: the user's stops every job, a trigger's, a profile's or a
    /// client's only the ones of its owner
    pub fn cancel(&mut self, origin: &Origin) -> Cancelled {
        let by = owner(origin);
        let mut cancelled = Cancelled::default();
        self.jobs.retain(|job| {
            let kept = by != Owner::User && owner(&job.origin) != by;
            if !kept {
                cancelled.commands += job.commands.len();
                cancelled.mash |= job.wait == Some(Wait::Mash);
            }
            kept
        });
        cancelled
    }
}

#[cfg(test)]
#[path = "../tests/commands/jobs.rs"]
mod tests;
