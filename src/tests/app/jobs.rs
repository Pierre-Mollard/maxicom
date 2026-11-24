//! Chains run one command at a time, paused, failed and cancelled (MAX-12), each command
//! with who started its chain (MAX-165)
use std::time::Duration;

use super::*;
use crate::{
    app::{
        UpdateEffect,
        capture::tests::asking_app,
        tests::{app, parse_fails},
    },
    inputs::{AppAction, DialogAction},
    link::serial::LineControl,
};

/// MAX-12: the command line runs its chain one action at a time, in order, each effect
/// returned for the loop to apply, and closes; a line that doesn't parse runs nothing,
/// warns and stays open.
/// How: a chain toggling a setting then giving two effects, then a chain with a typo.
#[test]
fn command_line_runs_its_chain_in_order() {
    let mut app = app();
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm))
    };
    let echo = app.settings.local_echo;
    let effect = run_line(&mut app, "toggle-local-echo; reload-config; toggle-attach");
    assert!(matches!(effect, UpdateEffect::None));
    assert!(app.overlay().is_none());
    let mut effects = Vec::new();
    while let Some((caller, action)) = app.next_command_action(Instant::now()) {
        effects.push(app.update_for(&caller, action));
    }
    assert!(matches!(
        effects[..],
        [
            UpdateEffect::None,
            UpdateEffect::ReloadConfig,
            UpdateEffect::ToggleAttach
        ]
    ));
    assert_ne!(app.settings.local_echo, echo);

    run_line(&mut app, "toggle-local-echo; nope");
    assert!(matches!(app.overlay(), Some(Overlay::CommandLine(_))));
    assert!(app.next_command_action(Instant::now()).is_none());
    assert_ne!(app.settings.local_echo, echo);
    let toast = app.notifications.active_toasts().last().unwrap();
    assert_eq!(toast.message, "unknown command 'nope'");
}

/// MAX-12: a command line bound to a key runs as a typed one, but stays out of the command
/// history; it waits behind a waiting line, unless it starts with `cancel`.
#[test]
fn bound_line_runs_as_typed() {
    let mut app = app();
    let line = |text: &str| crate::commands::parse(text).unwrap();
    app.update(Action::App(AppAction::QueueCommands(line(
        "send a; sleep 1000; send b",
    ))));
    let now = Instant::now();
    let send = app.next_command_action(now).unwrap().1;
    assert!(matches!(app.update(send), UpdateEffect::Transmit(a) if a == b"a"));
    let sleep = app.next_command_action(now).unwrap().1;
    app.update(sleep);
    app.update(Action::App(AppAction::QueueCommands(line(
        "cancel; send c",
    ))));
    let send = app.next_command_action(Instant::now()).unwrap().1;
    assert!(matches!(app.update(send), UpdateEffect::Transmit(c) if c == b"c"));
    assert!(app.next_command_action(Instant::now()).is_none());
    assert!(app.command_history().is_empty());
}

/// MAX-165 (A1 step 1): every queued command carries who started its chain: the command line
/// popup, a key binding, a trigger by its name, a profile's `on_load` hook.
/// How: one chain from each, the origin `next_command_action` gives with its action.
#[test]
fn queued_commands_carry_their_origin() {
    let mut app = app();
    let next_origin = |app: &mut App| app.next_command_action(Instant::now()).unwrap().0.origin;

    app.update(KeyBindingAction::CommandLine.to_action(&app));
    app.update(Action::Overlay(DialogAction::Paste("send a".to_string())));
    app.update(Action::Overlay(DialogAction::Confirm));
    assert_eq!(next_origin(&mut app), Origin::CommandLine);

    let chain = crate::commands::parse("send b").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    assert_eq!(next_origin(&mut app), Origin::KeyBinding);

    app.set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
    let chain = crate::commands::parse("trigger on panic").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    while let Some((caller, action)) = app.next_command_action(Instant::now()) {
        app.update_for(&caller, action);
    }
    app.feed_rx(b"[   12.345] Kernel panic - not syncing");
    assert_eq!(next_origin(&mut app), Origin::Trigger("panic".to_string()));

    app.on_load_due.push("send c".to_string());
    app.run_profile_hooks();
    assert_eq!(next_origin(&mut app), Origin::Profile("on_load"));
}

/// MAX-12: `sleep` pauses the rest of the chain, which goes on once its time is up. A failure
/// drops a waiting job, its wait and all (MAX-165).
/// How: the actions taken one by one, at chosen times.
#[test]
fn command_line_sleep() {
    let mut app = app();
    let queue_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
    };
    let sent = |app: &mut App, next: Option<(Caller, Action)>| {
        let (caller, action) = next.unwrap();
        match app.update_for(&caller, action) {
            UpdateEffect::Transmit(bytes) => bytes,
            _ => panic!("expected a transmit"),
        }
    };

    queue_line(&mut app, "send a; sleep 100; send b");
    let now = Instant::now();
    let action = app.next_command_action(now);
    assert_eq!(sent(&mut app, action), b"a");
    let (caller, sleep) = app.next_command_action(now).unwrap();
    app.update_for(&caller, sleep);
    assert!(app.next_command_action(Instant::now()).is_none());
    let later = Instant::now() + Duration::from_millis(150);
    let action = app.next_command_action(later);
    assert_eq!(sent(&mut app, action), b"b");
    assert!(app.next_command_action(later).is_none());

    // a failure while the job waits drops it
    queue_line(&mut app, "sleep 100; send c");
    let (caller, sleep) = app.next_command_action(later).unwrap();
    app.update_for(&caller, sleep);
    assert!(app.next_command_action(Instant::now()).is_none());
    app.fail_job(caller.job.unwrap(), "break: failed".to_string());
    assert!(
        app.next_command_action(later + Duration::from_secs(1))
            .is_none()
    );
    queue_line(&mut app, "send d");
    let action = app.next_command_action(Instant::now());
    assert_eq!(sent(&mut app, action), b"d");
    assert!(parse_fails(&mut app, "sleep"));
    assert!(parse_fails(&mut app, "sleep 60001"));
}

/// MAX-12: a line starting with `cancel` runs it at once, not behind the commands waiting:
/// it drops them, pause and all, and stops the file being sent, then the rest of its line
/// runs.
/// How: a chain paused by `sleep`, then a transfer and a paused chain together.
#[test]
fn command_line_cancel() {
    use crate::transfer::FileTransfer;
    let mut app = app();
    let queue_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
    };
    let last_toast = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };

    queue_line(&mut app, "cancel");
    assert_eq!(last_toast(&app), "Nothing to cancel");

    queue_line(&mut app, "sleep 1000; send a");
    let sleep = app.next_command_action(Instant::now()).unwrap().1;
    app.update(sleep);
    queue_line(&mut app, "cancel; send b");
    assert_eq!(last_toast(&app), "Dropped 1 waiting command");
    let send = app.next_command_action(Instant::now()).unwrap().1;
    assert!(matches!(app.update(send), UpdateEffect::Transmit(b) if b == b"b"));

    let transfer = FileTransfer::raw(
        Box::new(std::io::Cursor::new(vec![b'x'; 10])),
        "fw.bin".to_string(),
        10,
        960.0,
        Duration::ZERO,
        Instant::now(),
    );
    app.update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    queue_line(&mut app, "sleep 1000; send c; send d");
    let sleep = app.next_command_action(Instant::now()).unwrap().1;
    app.update(sleep);
    queue_line(&mut app, "cancel");
    // the transfer in its own box, "Cancelled" (MAX-228), the commands apart
    assert_eq!(last_toast(&app), "Dropped 2 waiting commands");
    let toasts = app.notifications.active_toasts();
    let transfer = toasts.iter().find(|toast| toast.key == Some("transfer"));
    let transfer = transfer.map(|toast| toast.message.as_str());
    assert_eq!(transfer, Some("Cancelled: fw.bin at 0%"));
    assert!(app.transfer().is_none());
    assert!(
        app.next_command_action(Instant::now() + Duration::from_secs(2))
            .is_none()
    );
}

/// MAX-165 (A1 step 0): a command line queued while the restore dialog asks waits for the
/// answer (MAX-187), then runs.
/// How: a bound line queued on an app asking, answered with Enter.
#[test]
fn restore_dialog_holds_a_command_line() {
    let (mut app, dir) = asking_app("holds_line");
    let line = crate::commands::parse("send a").unwrap();
    app.update(Action::App(AppAction::QueueCommands(line)));
    assert!(app.next_command_action(Instant::now()).is_none());
    app.update(Action::Overlay(DialogAction::Confirm));
    assert!(app.overlay().is_none());
    let send = app.next_command_action(Instant::now()).unwrap().1;
    assert!(matches!(app.dispatch(send), UpdateEffect::None));
    assert!(app.next_command_action(Instant::now()).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-165 (A1 step 2): what an action reports is shown for who started it. A chain's
/// failure stops the rest of the chain, said with how many didn't run; the same failure from
/// the user's own input is only said, and a chain waiting goes on.
/// How: `dtr on` on a link with no control lines, in a chain, then as input while a chain
/// sleeps.
#[test]
fn a_failure_stops_only_its_chain() {
    let mut app = app();
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let queue = |app: &mut App, chain: &str| {
        let chain = crate::commands::parse(chain).unwrap();
        app.update(Action::App(AppAction::QueueCommands(chain)));
    };

    queue(&mut app, "dtr on; send x");
    let (caller, dtr) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&caller, dtr);
    assert_eq!(
        last_toast(&app),
        "DTR on: the link has no control lines (1 more not run)"
    );
    assert!(app.next_command_action(Instant::now()).is_none());

    queue(&mut app, "sleep 1000; send y");
    let (caller, sleep) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&caller, sleep);
    let dtr = Action::App(AppAction::LineControl(LineControl::Dtr(true)));
    app.dispatch_from(&Caller::input(), dtr);
    assert_eq!(last_toast(&app), "DTR on: the link has no control lines");
    let later = Instant::now() + Duration::from_secs(2);
    let (_, send) = app.next_command_action(later).unwrap();
    assert!(matches!(app.update(send), UpdateEffect::Transmit(y) if y == b"y"));
}

/// MAX-165 (A1 step 3): a trigger's chain runs while the user's waits on a `sleep`, and its
/// failure drops only its own rest: the user's chain goes on once its time is up.
/// How: `sleep 1000; send u` bound to a key, then a trigger firing `send t; dtr on; send no`
/// on a link with no control lines.
#[test]
fn a_trigger_runs_while_the_user_waits() {
    let config: crate::configs::TriggersConfig = toml::from_str(
        r#"
            [boot]
            match = "boot>"
            run = "send t; dtr on; send no"
            armed = true
            "#,
    )
    .unwrap();
    let mut app = app();
    app.set_triggers(config.compile().unwrap());
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let chain = crate::commands::parse("sleep 1000; send u").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    let (caller, sleep) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&caller, sleep);

    app.feed_rx(b"boot>");
    let (caller, send) = app.next_command_action(Instant::now()).unwrap();
    assert_eq!(caller.origin, Origin::Trigger("boot".to_string()));
    assert!(matches!(app.update_for(&caller, send), UpdateEffect::Transmit(t) if t == b"t"));
    let (caller, dtr) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&caller, dtr);
    assert_eq!(
        last_toast(&app),
        "DTR on: the link has no control lines (1 more not run)"
    );
    assert!(app.next_command_action(Instant::now()).is_none());

    let later = Instant::now() + Duration::from_secs(2);
    let (caller, send) = app.next_command_action(later).unwrap();
    assert_eq!(caller.origin, Origin::KeyBinding);
    assert!(matches!(app.update_for(&caller, send), UpdateEffect::Transmit(u) if u == b"u"));
    assert!(app.next_command_action(later).is_none());
}

/// MAX-165 (A1 step 3): an `open` with no answer by its time fails its chain, rather than
/// the next command finding the link closed.
/// How: the open's wait set as the link sets it, already past its time.
#[test]
fn an_unanswered_open_fails_its_chain() {
    let mut app = app();
    let chain = crate::commands::parse("open /dev/ttyUSB9; send x").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    let now = Instant::now();
    let (caller, open) = app.next_command_action(now).unwrap();
    app.update_for(&caller, open);
    app.jobs
        .wait(caller.job.unwrap(), Wait::LinkOpen { until: now });

    assert!(app.next_command_action(now).is_none());
    let toast = app.notifications.active_toasts().last().unwrap();
    let secs = (crate::link::CONNECT_TIMEOUT * 2).as_secs();
    let failed = format!("open: no answer after {secs}s (1 more not run)");
    assert_eq!(toast.message, failed);
}

/// MAX-165 (A1 step 4): a trigger's chain can't run a command that needs the user: one
/// opening a popup fails its chain, no popup open; `mark` without its letter, which would
/// take the user's next key, fails too, `mark a` runs.
/// How: chains pushed for a trigger, drained as the loop does.
#[test]
fn automation_cant_run_what_asks_the_user() {
    let mut app = app();
    let trigger = Origin::Trigger("boot".to_string());
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let drain = |app: &mut App| {
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            app.update_for(&caller, action);
        }
    };

    let chain = crate::commands::parse("help; send x").unwrap();
    app.jobs.push(trigger.clone(), chain);
    drain(&mut app);
    assert!(app.overlay().is_none());
    assert_eq!(
        last_toast(&app),
        "help: it opens what the user answers: only the user runs it (1 more not run)"
    );

    app.jobs
        .push(trigger.clone(), crate::commands::parse("mark").unwrap());
    drain(&mut app);
    assert_eq!(
        last_toast(&app),
        "mark: it waits for a key: give its argument"
    );
    assert!(app.pending_mark().is_none());

    app.feed_rx(b"line\r\n");
    app.jobs
        .push(trigger, crate::commands::parse("mark a").unwrap());
    drain(&mut app);
    assert_eq!(last_toast(&app), "mark a");
}

/// MAX-165 (A1 step 5): `status` from the user opens its answer in a popup: the link, its
/// state, the mode, DTR (MAX-283) unset by the open; from a trigger it does nothing, and the
/// trigger's chain goes on.
/// How: `status` bound to a key, then a trigger's `status; send x`.
#[test]
fn status_answers_the_user_only() {
    let mut app = app();
    let drain = |app: &mut App| {
        let mut effects = Vec::new();
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update_for(&caller, action));
        }
        effects
    };
    let chain = crate::commands::parse("status").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    drain(&mut app);
    let Some(Overlay::Query(view)) = app.overlay() else {
        panic!("no answer popup");
    };
    assert_eq!(view.query, "status");
    assert!(view.lines.iter().any(|line| line.starts_with("link: ")));
    assert!(view.lines.contains(&"state: open".to_string()), "{view:?}");
    assert!(view.lines.contains(&"rx: 0".to_string()), "{view:?}");
    assert!(view.lines.contains(&"dtr: unset".to_string()), "{view:?}");
    app.close_overlay();

    let trigger = Origin::Trigger("boot".to_string());
    app.jobs
        .push(trigger, crate::commands::parse("status; send x").unwrap());
    let effects = drain(&mut app);
    assert!(app.overlay().is_none());
    assert!(matches!(&effects[..], [UpdateEffect::None, UpdateEffect::Transmit(x)] if x == b"x"));
}

/// MAX-283 (step 1): `status` shows the session backup (its stem, its folder, running or
/// paused by a replay) and the jobs running (their number, origin, trigger name, command due,
/// commands left, wait); the job of the `status` asked, its last command, is done and left out.
/// How: no session and no job, then a session in a temp folder and a trigger's chain waiting
/// on a `sleep`, `status` run as the user's chain; then a replay over the live link.
#[test]
fn status_shows_the_session_and_the_jobs() {
    let mut app = app();
    let status = |app: &mut App| {
        let chain = crate::commands::parse("status").unwrap();
        app.update(Action::App(AppAction::QueueCommands(chain)));
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            app.update_for(&caller, action);
        }
        let Some(Overlay::Query(view)) = app.overlay() else {
            panic!("no answer popup");
        };
        let lines = view.lines.clone();
        app.close_overlay();
        lines
    };
    let lines = status(&mut app);
    assert!(lines.contains(&"session: none".to_string()), "{lines:?}");
    assert!(lines.contains(&"jobs: none".to_string()), "{lines:?}");

    let dir = std::env::temp_dir().join(format!("maxicom_status_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.start_session(Some(dir.clone()));
    let trigger = Origin::Trigger("boot".to_string());
    let job = (app.jobs.push(
        trigger,
        crate::commands::parse("sleep 60000; send x").unwrap(),
    ))
    .unwrap();
    let (caller, action) = app.next_command_action(Instant::now()).unwrap();
    app.update_for(&caller, action);
    let lines = status(&mut app);
    let (raw, false) = app.session_backup().unwrap() else {
        panic!("the session isn't running");
    };
    let folder = raw.parent().unwrap();
    let stem = folder.file_name().unwrap().to_string_lossy();
    let at = lines.iter().position(|line| line == "session:").unwrap();
    assert_eq!(
        lines[at + 1..at + 4],
        [
            format!("  stem: {stem}"),
            format!("  folder: {}", folder.display()),
            "  state: running".to_string(),
        ]
    );
    let at = lines.iter().position(|line| line == "jobs:").unwrap();
    assert_eq!(
        lines[at + 1..],
        [
            format!("  id: {job}"),
            "  origin: trigger".to_string(),
            "  name: boot".to_string(),
            "  next: send".to_string(),
            "  left: 1".to_string(),
            "  wait: time".to_string(),
        ]
    );

    app.start_replay(std::path::PathBuf::from("/tmp/a.raw"), None);
    let lines = status(&mut app);
    assert!(lines.contains(&"  state: paused".to_string()), "{lines:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-283 (steps 5-6): `marks`, `triggers` and `notifications` have two faces: the user's
/// opens the popup, a trigger's gives its value to nobody and its chain goes on, where it was
/// refused before.
/// How: each from the command line, then a trigger's `<name>; send x`.
#[test]
fn popup_queries_open_their_popup_for_the_user_only() {
    let mut app = app();
    let drain = |app: &mut App| {
        let mut effects = Vec::new();
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update_for(&caller, action));
        }
        effects
    };
    for name in ["marks", "triggers", "notifications"] {
        let chain = crate::commands::parse(name).unwrap();
        app.update(Action::App(AppAction::QueueCommands(chain)));
        drain(&mut app);
        let popup = match app.overlay() {
            Some(Overlay::Marks(_)) => "marks",
            Some(Overlay::Triggers(_)) => "triggers",
            Some(Overlay::Notices(_)) => "notifications",
            _ => "none",
        };
        assert_eq!(popup, name);
        app.close_overlay();

        let trigger = Origin::Trigger("boot".to_string());
        let chain = crate::commands::parse(&format!("{name}; send x")).unwrap();
        app.jobs.push(trigger, chain);
        let effects = drain(&mut app);
        assert!(app.overlay().is_none(), "{name}");
        assert!(
            matches!(&effects[..], [UpdateEffect::None, UpdateEffect::Transmit(x)] if x == b"x"),
            "{name}"
        );
    }
}
