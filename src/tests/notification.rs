//! Toast lifetime and the redraw signal returned by `tick`.

use super::*;

/// All the toasts drawn now, as a frame does
fn show_all(manager: &mut NotificationManager) {
    let ids: Vec<u64> = manager.active_toasts().iter().map(|t| t.id).collect();
    manager.shown(ids, Instant::now());
}

/// `tick` reports a change only when a toast actually expires.
/// How: one expired and one long-lived toast, ticked twice.
#[test]
fn tick_reports_expired_toasts_once() {
    let mut manager = NotificationManager::new();
    manager.push("gone", ToastLevel::Info, Duration::ZERO);
    manager.push("stays", ToastLevel::Info, Duration::from_secs(60));
    show_all(&mut manager);
    assert!(manager.tick());
    assert_eq!(manager.active_toasts().len(), 1);
    assert!(!manager.tick());
}

/// No more than 4 toasts are kept, the oldest is dropped first.
/// How: pushes 5 and checks the remaining messages.
#[test]
fn push_keeps_the_four_newest() {
    let mut manager = NotificationManager::new();
    for i in 0..5 {
        manager.push(format!("t{i}"), ToastLevel::Info, Duration::from_secs(60));
    }
    let messages: Vec<&str> = manager
        .active_toasts()
        .iter()
        .map(|t| t.message.as_str())
        .collect();
    assert_eq!(messages, ["t1", "t2", "t3", "t4"]);
}

/// MAX-197: a toast is dismissed by its id, wherever it stands; none takes the top one (the
/// oldest); all of them go at once. An id already gone drops nothing.
/// How: three toasts, the middle one by id, then the top one, then the rest.
#[test]
fn dismiss_by_id_top_or_all() {
    let mut manager = NotificationManager::new();
    for name in ["a", "b", "c"] {
        manager.push(name, ToastLevel::Info, Duration::from_secs(60));
    }
    let messages = |manager: &NotificationManager| -> Vec<String> {
        let toasts = manager.active_toasts();
        toasts.iter().map(|t| t.message.clone()).collect()
    };
    let b = manager.active_toasts()[1].id;
    assert!(manager.dismiss(Some(b)));
    assert_eq!(messages(&manager), ["a", "c"]);
    assert!(!manager.dismiss(Some(b)));

    assert!(manager.dismiss(None));
    assert_eq!(messages(&manager), ["c"]);

    manager.push("d", ToastLevel::Info, Duration::from_secs(60));
    assert!(manager.dismiss_all());
    assert!(manager.active_toasts().is_empty());
    assert!(!manager.dismiss_all());
    assert!(!manager.dismiss(None));
}

/// MAX-228: a keyed toast takes the place of its key's toast still shown, with the same id
/// (a click still finds it), its text, level and time renewed; other toasts stay as they
/// are. Once gone, the key starts a new one at the bottom.
/// How: a plain toast, a keyed one, another plain one, then the key again.
#[test]
fn keyed_toast_replaces_in_place() {
    let mut manager = NotificationManager::new();
    let long = Duration::from_secs(60);
    manager.push("plain", ToastLevel::Info, long);
    manager.push_keyed("transfer", "Sending", ToastLevel::Info, Duration::ZERO);
    manager.push("other", ToastLevel::Warn, long);
    show_all(&mut manager);
    let id = manager.active_toasts()[1].id;

    manager.push_keyed("transfer", "Sent", ToastLevel::Success, long);
    let toasts = manager.active_toasts();
    let messages: Vec<&str> = toasts.iter().map(|t| t.message.as_str()).collect();
    assert_eq!(messages, ["plain", "Sent", "other"]);
    assert_eq!(toasts[1].id, id);
    assert!(matches!(toasts[1].level, ToastLevel::Success));
    // the renewed time: it didn't expire with the first one's zero
    assert!(!manager.tick());

    assert!(manager.dismiss(Some(id)));
    manager.push_keyed("transfer", "again", ToastLevel::Info, long);
    let toasts = manager.active_toasts();
    assert_eq!(toasts.last().map(|t| t.message.as_str()), Some("again"));
    assert_eq!(toasts.len(), 3);
}

/// MAX-228: a full stack drops the oldest of the least important: infos and successes
/// first, a warning when none is left, an error only when only errors are.
/// How: an error and a warning among infos, then a burst of infos; then only warnings and
/// errors; then only errors.
#[test]
fn full_stack_drops_the_least_important() {
    let long = Duration::from_secs(60);
    let messages = |manager: &NotificationManager| -> Vec<String> {
        let toasts = manager.active_toasts();
        toasts.iter().map(|t| t.message.clone()).collect()
    };
    let mut manager = NotificationManager::new();
    manager.push("error", ToastLevel::Error, long);
    manager.push("info 1", ToastLevel::Info, long);
    manager.push("warn", ToastLevel::Warn, long);
    manager.push("ok 1", ToastLevel::Success, long);
    for i in 2..6 {
        manager.push(format!("info {i}"), ToastLevel::Info, long);
    }
    assert_eq!(messages(&manager), ["error", "warn", "info 4", "info 5"]);

    manager.push("warn 2", ToastLevel::Warn, long);
    manager.push("error 2", ToastLevel::Error, long);
    assert_eq!(messages(&manager), ["error", "warn", "warn 2", "error 2"]);
    manager.push("info 6", ToastLevel::Info, long);
    assert_eq!(messages(&manager), ["error", "warn 2", "error 2", "info 6"]);

    let mut errors = NotificationManager::new();
    for i in 0..5 {
        errors.push(format!("e{i}"), ToastLevel::Error, long);
    }
    assert_eq!(messages(&errors), ["e1", "e2", "e3", "e4"]);
}

/// MAX-228: the same message at the same level while shown renews that toast and counts it,
/// in its place; another level is another toast. A keyed box counts its message said again
/// and starts over on a new one.
/// How: a warning twice around another toast, the same text as an error; then a key.
#[test]
fn repeats_count_in_one_toast() {
    let long = Duration::from_secs(60);
    let mut manager = NotificationManager::new();
    manager.push("busy", ToastLevel::Warn, Duration::ZERO);
    manager.push("other", ToastLevel::Info, long);
    show_all(&mut manager);
    manager.push("busy", ToastLevel::Warn, long);
    manager.push("busy", ToastLevel::Error, long);
    let toasts = manager.active_toasts();
    let seen: Vec<(&str, u32)> = toasts
        .iter()
        .map(|t| (t.message.as_str(), t.count))
        .collect();
    assert_eq!(seen, [("busy", 2), ("other", 1), ("busy", 1)]);
    // its time renewed: the zero of the first one is gone
    assert!(!manager.tick());

    manager.push_keyed("link", "lost", ToastLevel::Warn, long);
    manager.push_keyed("link", "lost", ToastLevel::Warn, long);
    let count = |manager: &NotificationManager| manager.active_toasts()[3].count;
    assert_eq!(count(&manager), 2);
    manager.push_keyed("link", "Reconnected", ToastLevel::Success, long);
    assert_eq!(count(&manager), 1);
}

/// MAX-228: a toast's time starts at its first frame: one not drawn yet (a short screen)
/// waits; drawn again later, its time doesn't start over.
/// How: a zero-length toast ticked before and after it's shown, then a long one shown twice.
#[test]
fn time_starts_when_first_drawn() {
    let mut manager = NotificationManager::new();
    manager.push("unseen", ToastLevel::Error, Duration::ZERO);
    assert!(!manager.tick());
    assert_eq!(manager.active_toasts().len(), 1);
    show_all(&mut manager);
    assert!(manager.tick());

    manager.push("read me", ToastLevel::Info, Duration::from_secs(60));
    let first = Instant::now();
    show_all(&mut manager);
    let expires = manager.active_toasts()[0].expires_at.unwrap();
    show_all(&mut manager);
    assert_eq!(manager.active_toasts()[0].expires_at, Some(expires));
    assert!(expires >= first + Duration::from_secs(60));
}
