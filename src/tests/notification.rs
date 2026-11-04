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
