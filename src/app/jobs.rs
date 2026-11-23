//! Running chains (MAX-12): their commands queued with who started them (MAX-165), built
//! one at a time on the state the previous ones left, paused by a `sleep`, a `break` or an
//! `open`, failed with the rest dropped, or cancelled. `commands` parses and builds them, the
//! loop applies their effects
use std::time::Instant;

use super::{App, RECANCEL_FOR, cancelled_text};
use crate::{
    commands::{CommandError, Invocation, Origin, Target},
    configs::KeyBindingAction,
    inputs::Action,
    notification::ToastLevel,
    overlays::Overlay,
};

impl App {
    /// The next action of a chain (MAX-12), with who started it (MAX-165). Each is built when
    /// its turn comes, on the state the previous ones left, and the loop applies its effect
    /// before the next. A command that refuses (the link can't do it) stops the chain, with
    /// the error. None either while the chain is paused (before `now`) or the
    /// restore dialog asks, as the device an `open` switched to may (MAX-187)
    pub fn next_command_action(&mut self, now: Instant) -> Option<(Origin, Action)> {
        if matches!(self.overlay, Some(Overlay::RestoreSession(_))) {
            return None;
        }
        if let Some(resume_at) = self.commands_resume_at {
            if now < resume_at {
                return None;
            }
            self.commands_resume_at = None;
        }
        let (origin, invocation) = self.pending_commands.pop_front()?;
        let built = match invocation.spec.target {
            Target::Action(action) => Ok(action.to_action(self)),
            Target::Build(build) => build(self, &invocation.args),
        };
        built
            .map_err(|reason| {
                let error = CommandError::Failed {
                    command: invocation.spec.name,
                    reason,
                };
                self.fail_commands(error.to_string());
            })
            .ok()
            .map(|action| (origin, action))
    }

    /// A command failed with `message`, here or where the loop applied its effect: the rest of
    /// its chain is dropped, the error says how much
    pub fn fail_commands(&mut self, message: String) {
        match self.drop_pending_commands() {
            0 => self.notify_error(message),
            n => self.notify_error(format!("{message} ({n} more not run)")),
        }
    }

    /// Drops the rest of a chain, quietly: how many commands it had
    pub fn drop_pending_commands(&mut self) -> usize {
        self.commands_resume_at = None;
        std::mem::take(&mut self.pending_commands).len()
    }

    /// A chain to run (MAX-12), started by `origin` (MAX-165): after the commands waiting, but
    /// a chain starting with `cancel` stops them instead of waiting behind them
    pub fn queue_chain(&mut self, origin: Origin, mut invocations: Vec<Invocation>) {
        let cancels = invocations.first().is_some_and(|invocation| {
            matches!(
                invocation.spec.target,
                Target::Action(KeyBindingAction::CancelCommands)
            )
        });
        if cancels {
            invocations.remove(0);
            self.cancel();
        }
        let queued = invocations.into_iter().map(|i| (origin.clone(), i));
        self.pending_commands.extend(queued);
    }

    /// `cancel` (MAX-12): stops the file being sent and drops the commands waiting
    pub fn cancel(&mut self) {
        // the transfer says it in its own box, "Cancelled", where a failure says "Stopped"
        // (MAX-228)
        let transfer_cancelled = self.transfer.is_some();
        if let Some(mut transfer) = self.transfer.take() {
            // a protocol's receiver is told to stop too
            let cans = transfer.cancel();
            if !cans.is_empty() {
                self.write_link(&cans, false);
                self.recancel_until = Some(Instant::now() + RECANCEL_FOR);
            }
            self.notify_transfer(ToastLevel::Warn, cancelled_text(&transfer));
        }
        let mut stopped = Vec::new();
        if self.mash.take().is_some() {
            stopped.push("Stopped mashing".to_string());
        }
        match self.drop_pending_commands() {
            0 => {}
            1 => stopped.push("dropped 1 waiting command".to_string()),
            n => stopped.push(format!("dropped {n} waiting commands")),
        }
        if !stopped.is_empty() {
            let mut message = stopped.join(", ");
            message[..1].make_ascii_uppercase();
            self.notify_warn(message);
        } else if !transfer_cancelled {
            self.notify_info("Nothing to cancel");
        }
    }

    /// The rest of a chain waits until `until` (a `sleep`, the end of a `break`, an `open` at
    /// the latest)
    pub fn pause_commands(&mut self, until: Instant) {
        self.commands_resume_at = Some(until);
    }

    /// The rest of a chain goes on now: the link `open` waited for is open
    pub fn resume_commands(&mut self) {
        self.commands_resume_at = None;
    }
}

#[cfg(test)]
#[path = "../tests/app/jobs.rs"]
mod tests;
