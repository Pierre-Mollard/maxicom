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

/// MAX-287 (step 4): a client's command runs as the user's `[api]` level for its access
/// says: allowed when `on`, refused when `off` (its chain stopped, the toast saying which
/// level), asked when `ask` (step 5); `set answer-queries` needs
/// `device`, the other settings `control`; a trigger's command isn't held by the levels.
/// How: chains pushed for a client under the default levels (read, control on, device off),
/// then each level changed; then the same `send` from a trigger.
#[test]
fn clients_run_within_their_levels() {
    use crate::configs::{Level, ReadLevel};
    let mut app = app();
    let client = Origin::Client {
        conn: 1,
        name: Some("script".to_string()),
    };
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let run = |app: &mut App, origin: &Origin, line: &str| {
        app.jobs
            .push(origin.clone(), crate::commands::parse(line).unwrap());
        let mut sent = Vec::new();
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            if let UpdateEffect::Transmit(bytes) = app.update_for(&caller, action) {
                sent.extend(bytes);
            }
        }
        sent
    };

    let echo = app.settings.local_echo;
    assert!(run(&mut app, &client, "toggle-local-echo; send x; send y").is_empty());
    assert_ne!(app.settings.local_echo, echo, "control is on");
    assert_eq!(
        last_toast(&app),
        "send: device access is off in [api] (1 more not run)"
    );
    run(&mut app, &client, "set answer-queries on");
    assert_eq!(last_toast(&app), "set: device access is off in [api]");

    app.settings.api.device = Level::On;
    assert_eq!(run(&mut app, &client, "send x"), b"x");
    app.settings.api.device = Level::Ask;
    assert!(run(&mut app, &client, "send x").is_empty());
    assert_eq!(app.asked().unwrap().command, "send \"x\"");
    app.update(Action::Overlay(DialogAction::Close));

    app.settings.api.control = Level::Off;
    run(&mut app, &client, "toggle-local-echo");
    assert_ne!(app.settings.local_echo, echo, "toggled again");
    assert_eq!(
        last_toast(&app),
        "toggle-local-echo: control access is off in [api]"
    );
    app.settings.api.read = ReadLevel::Off;
    run(&mut app, &client, "status");
    assert_eq!(last_toast(&app), "status: reading is off in [api]");

    app.settings.api.device = Level::Off;
    let trigger = Origin::Trigger("boot".to_string());
    assert_eq!(run(&mut app, &trigger, "send x"), b"x");
}

/// MAX-287 (step 4): each API connection owns its jobs: its chains run in order, beside the
/// other clients', and its `cancel` stops only its own, as a trigger's does.
/// How: two clients each waiting in a `sleep`, the first one's `cancel`, then the second's
/// chain going on.
#[test]
fn a_clients_cancel_stops_only_its_jobs() {
    let mut app = app();
    app.settings.api.device = crate::configs::Level::On;
    let client = |conn| Origin::Client { conn, name: None };
    let line = |text: &str| crate::commands::parse(text).unwrap();
    app.jobs.push(client(1), line("sleep 50; send a"));
    app.jobs.push(client(2), line("sleep 50; send b"));
    let now = Instant::now();
    while let Some((caller, action)) = app.next_command_action(now) {
        app.update_for(&caller, action);
    }
    assert_eq!(app.jobs.running().count(), 2, "both wait");
    let report = app.cancel(&client(1));
    assert_eq!(report.notes[0].text, "Dropped 1 waiting command");
    let later = Instant::now() + Duration::from_millis(100);
    let (caller, action) = app.next_command_action(later).unwrap();
    assert_eq!(caller.origin, client(2));
    assert!(matches!(app.update_for(&caller, action), UpdateEffect::Transmit(b) if b == b"b"));
    assert!(app.next_command_action(later).is_none());
}

/// A client's chain, the effects of its commands run until every job waits, at `now`
fn run_client(app: &mut App, conn: u64, line: &str, now: Instant) -> Vec<u8> {
    let client = Origin::Client {
        conn,
        name: Some("script".to_string()),
    };
    app.jobs.push(client, crate::commands::parse(line).unwrap());
    drain_at(app, now)
}

/// The jobs' commands run until every job waits, at `now`: the bytes they sent
fn drain_at(app: &mut App, now: Instant) -> Vec<u8> {
    let mut sent = Vec::new();
    while let Some((caller, action)) = app.next_command_action(now) {
        if let UpdateEffect::Transmit(bytes) = app.update_for(&caller, action) {
            sent.extend(bytes);
        }
    }
    sent
}

fn key(c: char) -> Action {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    Action::Overlay(DialogAction::HandleKey(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::NONE,
    )))
}

/// MAX-287 (step 5): with `ask`, a client's command waits for the user: its job waits
/// (`status` says `user`), a popup shows the command as typed; confirm runs it, once: the next
/// one asks again; `n` refuses it and stops its chain, the toast saying so.
/// How: a client's `send a; send b; send c` under `device = "ask"`, confirmed, then refused.
#[test]
fn asked_commands_wait_for_the_user() {
    let mut app = app();
    app.settings.api.device = crate::configs::Level::Ask;
    let now = Instant::now();
    assert!(run_client(&mut app, 1, "send a; send b; send c", now).is_empty());
    assert!(matches!(app.overlay(), Some(Overlay::ClientAsk(_))));
    assert_eq!(app.asked().unwrap().command, "send \"a\"");
    let job = app.jobs.running().next().unwrap();
    assert!(matches!(job.wait(), Some(Wait::Asked { .. })));

    app.update(Action::Overlay(DialogAction::Confirm));
    assert!(app.overlay().is_none());
    assert_eq!(drain_at(&mut app, now), b"a");
    assert_eq!(app.asked().unwrap().command, "send \"b\"");
    assert!(matches!(app.overlay(), Some(Overlay::ClientAsk(_))));

    app.update(key('n'));
    assert!(drain_at(&mut app, now).is_empty());
    assert!(app.overlay().is_none());
    assert!(app.asked().is_none());
    assert_eq!(app.jobs.running().count(), 0);
    let toasts = app.notifications.active_toasts();
    assert_eq!(
        toasts.last().unwrap().message,
        "send \"b\": refused by the user (1 more not run)"
    );
}

/// MAX-287 (step 5): `a` (always) runs the command and the client's next ones of the same
/// access without asking; another client is still asked, and so is a `control` command of
/// the first one.
/// How: client 1's `send a; send b` answered always, then client 2's `send c`, then client 1's
/// `toggle-local-echo` under `control = "ask"`.
#[test]
fn always_lets_the_client_go_on() {
    use crate::configs::Level;
    let mut app = app();
    app.settings.api.device = Level::Ask;
    app.settings.api.control = Level::Ask;
    let now = Instant::now();
    run_client(&mut app, 1, "send a; send b", now);
    app.update(key('a'));
    assert_eq!(drain_at(&mut app, now), b"ab");
    assert!(app.asked().is_none());

    assert!(run_client(&mut app, 2, "send c", now).is_empty());
    assert_eq!(app.asked().unwrap().command, "send \"c\"");
    app.update(key('n'));
    drain_at(&mut app, now);
    run_client(&mut app, 1, "toggle-local-echo", now);
    assert_eq!(app.asked().unwrap().command, "toggle-local-echo");
}

/// MAX-287 (step 5): an ask with no answer by `ask_timeout` is refused, its popup closed and
/// the next one shown; asks queue, one popup at a time; a popup the user is in isn't taken
/// over: the ask shows once it closes; the user's `cancel` drops the asks with their jobs.
/// How: two clients asked while the quit question is open, it closed; the first one left
/// past its time; the second one's job cancelled by the user.
#[test]
fn asks_queue_time_out_and_go_with_their_jobs() {
    let mut app = app();
    app.settings.api.device = crate::configs::Level::Ask;
    app.settings.api.ask_timeout = Duration::from_secs(30);
    app.overlay = Some(Overlay::ConfirmQuit);
    let now = Instant::now();
    run_client(&mut app, 1, "send a", now);
    run_client(&mut app, 2, "send b", now + Duration::from_secs(10));
    assert!(matches!(app.overlay(), Some(Overlay::ConfirmQuit)));
    app.overlay = None;
    drain_at(&mut app, now + Duration::from_secs(10));
    assert_eq!(app.asked().unwrap().command, "send \"a\"");

    drain_at(&mut app, now + Duration::from_secs(30));
    let toasts = app.notifications.active_toasts();
    assert_eq!(
        toasts.last().unwrap().message,
        "send \"a\": no answer from the user"
    );
    assert_eq!(app.asked().unwrap().command, "send \"b\"");
    assert!(matches!(app.overlay(), Some(Overlay::ClientAsk(_))));

    let _ = app.cancel(&Origin::Input);
    drain_at(&mut app, now + Duration::from_secs(31));
    assert!(app.asked().is_none());
    assert!(app.overlay().is_none());
}

/// MAX-287 (step 6): `disconnect-clients` cancels every client's jobs, its ask and popup
/// with them, and forgets what the user let the clients do; the user's and the triggers'
/// jobs go on; the loop gets the effect that closes the connections.
/// How: client 1 asked (`device = "ask"`), client 2 in a `sleep`, a trigger and the user in a
/// `sleep`, an always granted; then `disconnect-clients` from the command line.
#[test]
fn disconnect_clients_drops_only_the_clients() {
    let mut app = app();
    app.settings.api.device = crate::configs::Level::Ask;
    let now = Instant::now();
    run_client(&mut app, 1, "send a", now);
    run_client(&mut app, 2, "sleep 1000; toggle-local-echo", now);
    let line = |text: &str| crate::commands::parse(text).unwrap();
    app.jobs.push(
        Origin::Trigger("boot".to_string()),
        line("sleep 1000; send t"),
    );
    app.jobs
        .push(Origin::CommandLine, line("sleep 1000; send u"));
    drain_at(&mut app, now);
    app.allowed_always.push((2, Access::Control));
    assert_eq!(app.jobs.running().count(), 4);
    assert!(matches!(app.overlay(), Some(Overlay::ClientAsk(_))));

    let effect = app.update(Action::App(AppAction::DisconnectClients));
    assert!(matches!(effect, UpdateEffect::DisconnectClients));
    let origins: Vec<&Origin> = app.jobs.running().map(|job| job.origin()).collect();
    assert_eq!(
        origins,
        [&Origin::Trigger("boot".to_string()), &Origin::CommandLine]
    );
    assert!(app.asked().is_none());
    assert!(app.overlay().is_none());
    assert!(app.allowed_always.is_empty());
}

/// MAX-284 (step 3a): a client's chain run with `run_for_client` reports back, not to toasts:
/// when it ends, its notes and its queries' answers; when it stops, why, as a kind (a command
/// failed, refused by the levels, by the user, cancelled) and how many commands didn't run.
/// How: `toggle-local-echo; status`; `send x; send y` (device off); `notify info ''` (nothing to say);
/// `send a` asked and refused; `sleep 1000; send b` cancelled by the user; no toast meanwhile.
/// MAX-285: then a `notify`, whose toast shows.
#[test]
fn client_runs_report_back() {
    use crate::app::jobs::FailureKind;
    let mut app = app();
    let client = Origin::Client {
        conn: 1,
        name: None,
    };
    let toasts = |app: &App| app.notifications.active_toasts().len();
    let before = toasts(&app);
    let run = |app: &mut App, line: &str| {
        let chain = crate::commands::parse(line).unwrap();
        let job = app.run_for_client(client.clone(), chain).0.unwrap();
        drain_at(app, Instant::now());
        job
    };
    let finished = |app: &mut App, job| {
        let runs = app.finished_runs();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0].0, job);
        runs[0].1.clone()
    };

    let job = run(&mut app, "toggle-local-echo; status");
    let done = finished(&mut app, job);
    assert!(done.failure.is_none(), "{done:?}");
    assert_eq!(done.answers.len(), 1);
    assert_eq!(done.answers[0].query, "status");
    assert!(app.overlay().is_none(), "a client gets no popup");

    let job = run(&mut app, "send x; send y");
    let failure = finished(&mut app, job).failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Refused);
    assert_eq!(failure.message, "send: device access is off in [api]");
    assert_eq!(failure.not_run, 1);

    let job = run(&mut app, "notify info ''");
    let failure = finished(&mut app, job).failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Command);
    assert_eq!(failure.message, "notify: nothing to say");

    app.settings.api.device = crate::configs::Level::Ask;
    let job = run(&mut app, "send a");
    app.update(key('n'));
    drain_at(&mut app, Instant::now());
    let failure = finished(&mut app, job).failure.unwrap();
    assert_eq!(failure.kind, FailureKind::ByUser);
    assert_eq!(failure.message, "send \"a\": refused by the user");

    app.settings.api.device = crate::configs::Level::On;
    let job = run(&mut app, "sleep 1000; send b");
    assert!(app.finished_runs().is_empty(), "it waits");
    let _ = app.cancel(&Origin::Input);
    let failure = finished(&mut app, job).failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Cancelled);
    assert_eq!(toasts(&app), before, "a client's job makes no toast");

    // MAX-285: but `notify` is for the user, whoever asks: its toast shows, the client gets
    // no note back
    let job = run(&mut app, "notify info 'flash done'");
    let done = finished(&mut app, job);
    assert!(done.failure.is_none() && done.notes.is_empty(), "{done:?}");
    let shown: Vec<&str> = (app.notifications.active_toasts().iter())
        .map(|toast| toast.message.as_str())
        .collect();
    assert_eq!(shown.last(), Some(&"flash done"));
}

/// MAX-284 (step 3c): a client's `cancel` stops one of its jobs by number, or all of them, and
/// never another client's; the jobs cancelled end `Cancelled`; a line starting with `cancel`
/// cancels the client's jobs first, its notes the new job's; a closed connection's jobs are
/// cancelled, its asks with them, and its always forgotten, another client's kept.
/// How: client 1's two sleeping runs and client 2's one; a cancel by number (its own, then the
/// other's), then of all; `cancel; status`; client 1 asked with always granted to both, gone.
#[test]
fn clients_cancel_their_own_jobs() {
    use crate::app::jobs::FailureKind;
    let mut app = app();
    let client = |conn| Origin::Client { conn, name: None };
    let line = |text: &str| crate::commands::parse(text).unwrap();
    let run = |app: &mut App, conn, text: &str| app.run_for_client(client(conn), line(text)).0;
    let one = run(&mut app, 1, "sleep 1000; toggle-local-echo").unwrap();
    let two = run(&mut app, 1, "sleep 1000; toggle-local-echo").unwrap();
    let other = run(&mut app, 2, "sleep 1000; toggle-local-echo").unwrap();
    drain_at(&mut app, Instant::now());

    assert!(app.cancel_for_client(&client(1), Some(other)).is_err());
    let cancelled = app.cancel_for_client(&client(1), Some(one)).unwrap();
    assert_eq!(cancelled.notes[0].text, format!("Cancelled job {one}"));
    let ended = app.finished_runs();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0, one);
    let kind = ended[0].1.failure.as_ref().unwrap().kind;
    assert_eq!(kind, FailureKind::Cancelled);
    let _ = app.cancel_for_client(&client(1), None).unwrap();
    let ended: Vec<JobId> = app
        .finished_runs()
        .into_iter()
        .map(|(job, _)| job)
        .collect();
    assert_eq!(ended, [two]);
    assert_eq!(app.jobs.running().count(), 1, "client 2's job runs");

    let first = run(&mut app, 2, "sleep 1000; toggle-local-echo").unwrap();
    let next = run(&mut app, 2, "cancel; status").unwrap();
    drain_at(&mut app, Instant::now());
    let runs = app.finished_runs();
    let done = |job| {
        runs.iter()
            .find(|(of, _)| *of == job)
            .map(|(_, outcome)| outcome)
    };
    assert!(done(first).is_some_and(|outcome| outcome.failure.is_some()));
    assert!(
        done(other).is_some(),
        "the cancel took client 2's older job too"
    );
    let next = done(next).unwrap();
    // `first` not started yet: both its commands, and the one `other` has left
    assert_eq!(next.notes, ["Dropped 3 waiting commands"]);
    assert_eq!(next.answers[0].query, "status");

    app.settings.api.device = crate::configs::Level::Ask;
    app.allowed_always.push((2, Access::Device));
    app.allowed_always.push((1, Access::Control));
    run(&mut app, 1, "send a");
    drain_at(&mut app, Instant::now());
    assert!(matches!(app.overlay(), Some(Overlay::ClientAsk(_))));
    app.client_gone(1);
    assert!(app.asked().is_none());
    assert!(app.overlay().is_none());
    assert_eq!(app.allowed_always, [(2, Access::Device)]);
    assert_eq!(app.jobs.running().count(), 0);
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
