//! Toast lifetime and the redraw signal returned by `tick`.

use super::*;

/// `tick` reports a change only when a toast actually expires.
/// How: one expired and one long-lived toast, ticked twice.
#[test]
fn tick_reports_expired_toasts_once() {
    let mut manager = NotificationManager::new();
    manager.push("gone", ToastLevel::Info, Duration::ZERO);
    manager.push("stays", ToastLevel::Info, Duration::from_secs(60));
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
