//! `wait-for` (MAX-219): its texts found across reads, the first of several
use std::time::Duration;

use super::*;

fn watch(texts: &[&str]) -> TextWatch {
    let texts = texts.iter().map(|text| text.as_bytes().to_vec()).collect();
    TextWatch::new(texts, Instant::now()).unwrap()
}

/// MAX-219: a text cut between two reads still comes in; the bytes before the watch started
/// don't count, as it only sees what it is fed.
/// How: `login:` fed as `log` then `in:`.
#[test]
fn a_text_split_over_two_reads() {
    let mut watch = watch(&["login:"]);
    assert_eq!(watch.feed(b"board login"), None);
    assert_eq!(watch.feed(b"x"), None);
    assert_eq!(watch.feed(b"... log"), None);
    assert_eq!(watch.feed(b"in: "), Some(&b"login:"[..]));
}

/// MAX-219: of several texts, the one ending first in the read comes in, the first given on a
/// tie; an empty text is refused, as it would match anything.
/// How: `OK` and `ERROR` in one read, both orders, then `ERR` and `ERROR` ending together.
#[test]
fn the_text_that_ends_first() {
    assert_eq!(
        watch(&["OK", "ERROR"]).feed(b"ERROR\r\nOK"),
        Some(&b"ERROR"[..])
    );
    assert_eq!(
        watch(&["OK", "ERROR"]).feed(b"OK\r\nERROR"),
        Some(&b"OK"[..])
    );
    assert_eq!(watch(&["ROR", "ERROR"]).feed(b"ERROR"), Some(&b"ROR"[..]));
    assert_eq!(watch(&["OK", "ERROR"]).texts_shown(), "'OK' or 'ERROR'");
    let texts = vec![b"OK".to_vec(), Vec::new()];
    assert!(TextWatch::new(texts, Instant::now()).is_none());
}

/// MAX-219: `wait-for` holds the rest of its chain until one of its texts comes in, counted
/// from when it runs, the note saying which came; it fails its chain at its time, when the
/// link closes, and is gone with a `cancel`. A closed link refuses it.
/// How: `wait-for ...; send x` from the user, the texts fed by hand, the clock passed by hand,
/// the link closed under it.
#[test]
fn wait_for_holds_its_chain() {
    use crate::{
        app::{UpdateEffect, tests::app},
        inputs::{Action, AppAction},
    };
    let mut app = app();
    let toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let queue = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.update(Action::App(AppAction::QueueCommands(invocations)));
    };
    let run = |app: &mut App, now: Instant| {
        let mut effects = Vec::new();
        while let Some((caller, action)) = app.next_command_action(now) {
            effects.push(app.update_for(&caller, action));
        }
        effects
    };

    app.feed_rx(b"OK before\r\n");
    queue(&mut app, "wait-for OK ERROR; send x");
    assert!(matches!(
        &run(&mut app, Instant::now())[..],
        [UpdateEffect::None]
    ));
    assert_eq!(toast(&app), "Waiting for 'OK' or 'ERROR'");
    let waits = app.jobs.running().next().and_then(|job| job.wait());
    assert!(matches!(waits, Some(Wait::Text { .. })), "{waits:?}");
    app.feed_rx(b"flash ER");
    assert!(run(&mut app, Instant::now()).is_empty());
    app.feed_rx(b"ROR\r\n");
    assert_eq!(toast(&app), "wait-for: 'ERROR' came in");
    let effects = run(&mut app, Instant::now());
    assert!(matches!(&effects[..], [UpdateEffect::Transmit(x)] if x == b"x"));

    // its time, from when it was built
    queue(&mut app, "wait-for-ms 500 OK; send x");
    run(&mut app, Instant::now());
    let late = Instant::now() + Duration::from_millis(500);
    assert!(run(&mut app, late).is_empty());
    assert_eq!(
        toast(&app),
        "wait-for: 'OK' didn't come in (1 more not run)"
    );
    app.feed_rx(b"OK");
    assert_eq!(
        toast(&app),
        "wait-for: 'OK' didn't come in (1 more not run)"
    );

    queue(&mut app, "wait-for OK; send x");
    run(&mut app, Instant::now());
    queue(&mut app, "cancel");
    run(&mut app, Instant::now());
    assert!(app.text_watches.is_empty());
    app.feed_rx(b"OK");
    assert!(!toast(&app).contains("came in"), "{}", toast(&app));

    queue(&mut app, "wait-for OK; send x");
    run(&mut app, Instant::now());
    app.link.start(None);
    app.poll_link(&mut [0; 64]).unwrap();
    assert_eq!(
        toast(&app),
        "wait-for: the link closed before 'OK' came in (1 more not run)"
    );
    queue(&mut app, "wait-for OK");
    run(&mut app, Instant::now());
    assert!(toast(&app).starts_with("wait-for: the port isn't open"));
}

/// MAX-219: a client's `wait-for` gives the text that came as its job's value, for its `run`'s
/// reply; the user's gives none (it would open a popup).
/// How: a client's `wait-for-ms 1000 OK ERROR`, `OK` fed, the job's outcome read.
#[test]
fn a_clients_wait_for_answers_the_text() {
    use crate::{
        app::tests::app,
        commands::{Origin, report::Answer, value::Value},
    };
    let mut app = app();
    let client = Origin::Client {
        conn: 1,
        name: None,
    };
    let chain = crate::commands::parse("wait-for-ms 1000 OK ERROR").unwrap();
    let job = app.run_for_client(client, chain).0.unwrap();
    let drain = |app: &mut App| {
        while let Some((caller, action)) = app.next_command_action(Instant::now()) {
            app.update_for(&caller, action);
        }
    };
    drain(&mut app);
    assert!(app.finished_runs().is_empty());
    app.feed_rx(b"\r\nOK\r\n");
    // the job ends at the loop's next turn
    drain(&mut app);
    let runs = app.finished_runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let (ended, outcome) = &runs[0];
    assert_eq!(*ended, job);
    assert!(outcome.failure.is_none(), "{outcome:?}");
    assert_eq!(
        outcome.notes,
        ["Waiting for 'OK' or 'ERROR'", "wait-for: 'OK' came in"]
    );
    let answer = Answer {
        query: "wait-for",
        value: Value::Text("OK".to_string()),
    };
    assert_eq!(outcome.answers, [answer]);
}
