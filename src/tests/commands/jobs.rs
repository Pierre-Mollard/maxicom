use std::time::Duration;

use super::*;
use crate::commands::parse;

/// The names of the commands a chain runs, for short asserts
fn chain(line: &str) -> Vec<Invocation> {
    parse(line).unwrap()
}

/// The next command due at `now`: its name and its job's origin
fn next(jobs: &mut Jobs, now: Instant) -> Option<(&'static str, Origin)> {
    let (caller, invocation) = jobs.next(now)?;
    Some((invocation.spec.name, caller.origin))
}

fn trigger(name: &str) -> Origin {
    Origin::Trigger(name.to_string())
}

/// MAX-165 (A1 step 3): one owner's chains run one after the other, the user's typed and
/// bound chains as one owner; a job that waits lets another owner's job go on, never a later
/// one of its own owner; a profile's hooks go first.
/// How: a user chain waiting on a `sleep`, a bound one behind it, a trigger's beside it.
#[test]
fn a_waiting_job_lets_other_owners_go_on() {
    let now = Instant::now();
    let mut jobs = Jobs::default();
    let typed = jobs
        .push(Origin::CommandLine, chain("sleep 100; send a"))
        .unwrap();
    jobs.push(Origin::KeyBinding, chain("send b"));
    jobs.push(trigger("boot"), chain("send c"));

    assert_eq!(next(&mut jobs, now), Some(("sleep", Origin::CommandLine)));
    jobs.wait(typed, Wait::Until(now + Duration::from_millis(100)));
    // the bound chain waits behind the typed one, the trigger's goes on
    assert_eq!(next(&mut jobs, now), Some(("send", trigger("boot"))));
    assert_eq!(next(&mut jobs, now), None);

    let later = now + Duration::from_millis(100);
    assert_eq!(next(&mut jobs, later), Some(("send", Origin::CommandLine)));
    assert_eq!(next(&mut jobs, later), Some(("send", Origin::KeyBinding)));
    assert_eq!(next(&mut jobs, later), None);

    jobs.push(Origin::KeyBinding, chain("send d"));
    jobs.push_front(Origin::Profile("on_load"), chain("baud 9600"));
    assert_eq!(
        next(&mut jobs, later),
        Some(("baud", Origin::Profile("on_load")))
    );
    assert!(jobs.push(Origin::CommandLine, Vec::new()).is_none());
}

/// MAX-165 (A1 step 3): an `open`'s wait and a mash's end only at their result; `waiting`
/// finds who a result is for, and an `open` past its time is listed for the app to fail.
/// How: a job on each, resumed by hand as the link and the mash do.
#[test]
fn waits_end_on_their_result() {
    let now = Instant::now();
    let mut jobs = Jobs::default();
    let opening = jobs.push(Origin::CommandLine, chain("open /dev/ttyUSB0; send a"));
    let mashing = jobs.push(trigger("uboot"), chain("mash x y; send b"));
    let (opening, mashing) = (opening.unwrap(), mashing.unwrap());
    // each wait set right after its command, as the loop does
    assert_eq!(next(&mut jobs, now), Some(("open", Origin::CommandLine)));
    let until = now + Duration::from_secs(1);
    jobs.wait(opening, Wait::LinkOpen { until });
    assert_eq!(next(&mut jobs, now), Some(("mash", trigger("uboot"))));
    jobs.wait(mashing, Wait::Mash);
    assert_eq!(next(&mut jobs, now), None);

    let mash_caller = jobs.waiting(|wait| *wait == Wait::Mash).unwrap();
    assert_eq!(mash_caller.job, Some(mashing));
    assert_eq!(mash_caller.origin, trigger("uboot"));
    jobs.resume(mashing);
    assert_eq!(next(&mut jobs, now), Some(("send", trigger("uboot"))));

    // past the open's time it doesn't go on: it is listed, unanswered
    assert!(jobs.unopened(now).is_empty());
    assert_eq!(next(&mut jobs, until), None);
    assert_eq!(jobs.unopened(until), [opening]);
}

/// MAX-165 (A1 step 3): a failed job drops only its own commands, said by their count.
/// How: two owners' chains, the first failed.
#[test]
fn a_failure_drops_only_its_job() {
    let now = Instant::now();
    let mut jobs = Jobs::default();
    let failing = jobs.push(Origin::CommandLine, chain("send a; send b; send c"));
    jobs.push(trigger("boot"), chain("send d"));
    next(&mut jobs, now);
    assert_eq!(jobs.fail(failing.unwrap()), 2);
    assert_eq!(next(&mut jobs, now), Some(("send", trigger("boot"))));
    assert_eq!(jobs.fail(failing.unwrap()), 0);
}

/// MAX-165 (A1 step 3): the user's `cancel` stops every job; a trigger's only its own, and
/// says whether one of them waited for the mash.
/// How: jobs of the user, of two triggers, one waiting on a mash.
#[test]
fn cancel_stops_its_owners_jobs() {
    let now = Instant::now();
    let mut jobs = Jobs::default();
    let boot = jobs
        .push(trigger("boot"), chain("mash x y; send c"))
        .unwrap();
    assert_eq!(next(&mut jobs, now), Some(("mash", trigger("boot"))));
    jobs.wait(boot, Wait::Mash);
    jobs.push(Origin::CommandLine, chain("send a; send b"));
    jobs.push(trigger("crash"), chain("send d"));

    let cancelled = jobs.cancel(&trigger("boot"));
    assert_eq!(
        cancelled,
        Cancelled {
            commands: 1,
            mash: true
        }
    );
    assert_eq!(next(&mut jobs, now), Some(("send", Origin::CommandLine)));
    // the user's, from a key, takes every job: its own "send b" and the other trigger's
    let cancelled = jobs.cancel(&Origin::KeyBinding);
    assert_eq!(
        cancelled,
        Cancelled {
            commands: 2,
            mash: false
        }
    );
    assert_eq!(next(&mut jobs, now), None);
}
