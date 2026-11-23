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
    while let Some((_, action)) = app.next_command_action(Instant::now()) {
        effects.push(app.update(action));
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
    let next_origin = |app: &mut App| app.next_command_action(Instant::now()).unwrap().0;

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
    while let Some((origin, action)) = app.next_command_action(Instant::now()) {
        let _ = app.update_from(&origin, action);
    }
    app.feed_rx(b"[   12.345] Kernel panic - not syncing");
    assert_eq!(next_origin(&mut app), Origin::Trigger("panic".to_string()));

    app.on_load_due.push("send c".to_string());
    app.run_profile_hooks();
    assert_eq!(next_origin(&mut app), Origin::Profile("on_load"));
}

/// MAX-12: `sleep` pauses the rest of the chain, which goes on once its time is up; the
/// pause the loop sets for a break does the same. A failure drops a paused chain, pause
/// and all.
/// How: the actions taken one by one, at chosen times.
#[test]
fn command_line_sleep() {
    let mut app = app();
    let queue_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
    };
    let sent = |app: &mut App, next: Option<(Origin, Action)>| match app.update(next.unwrap().1) {
        UpdateEffect::Transmit(bytes) => bytes,
        _ => panic!("expected a transmit"),
    };

    queue_line(&mut app, "send a; sleep 100; send b");
    let now = Instant::now();
    let action = app.next_command_action(now);
    assert_eq!(sent(&mut app, action), b"a");
    let sleep = app.next_command_action(now).unwrap().1;
    app.update(sleep);
    assert!(app.next_command_action(Instant::now()).is_none());
    let later = Instant::now() + Duration::from_millis(150);
    let action = app.next_command_action(later);
    assert_eq!(sent(&mut app, action), b"b");
    assert!(app.next_command_action(later).is_none());

    // the loop pauses the chain for a break
    queue_line(&mut app, "send c");
    app.pause_commands(later + Duration::from_millis(100));
    assert!(app.next_command_action(later).is_none());
    app.fail_commands("break: failed".to_string());
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
    let (origin, dtr) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&origin, dtr);
    assert_eq!(
        last_toast(&app),
        "DTR on: the link has no control lines (1 more not run)"
    );
    assert!(app.next_command_action(Instant::now()).is_none());

    queue(&mut app, "sleep 1000; send y");
    let (origin, sleep) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&origin, sleep);
    let dtr = Action::App(AppAction::LineControl(LineControl::Dtr(true)));
    app.dispatch_from(&Origin::Input, dtr);
    assert_eq!(last_toast(&app), "DTR on: the link has no control lines");
    let later = Instant::now() + Duration::from_secs(2);
    let (_, send) = app.next_command_action(later).unwrap();
    assert!(matches!(app.update(send), UpdateEffect::Transmit(y) if y == b"y"));
}
