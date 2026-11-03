
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
