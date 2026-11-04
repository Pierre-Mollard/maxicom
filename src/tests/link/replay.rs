//! Reading a replay: pause, delay, progress, restart.
use super::*;

fn replay_of(name: &str, bytes: &[u8]) -> (Replay, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "maxicom_test_{}_replay_{name}.raw",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    (Replay::open(&path, Duration::ZERO).unwrap(), path)
}

/// A paused replay reads nothing until resumed; the progress counts what was read, to the
/// end; a restart reads it all again from the top.
/// How: a 10-byte file read 4 bytes at a time, paused once, then restarted.
#[test]
fn pause_progress_and_restart() {
    let (mut replay, path) = replay_of("controls", b"0123456789");
    let mut buf = [0; 4];
    assert_eq!(replay.read(&mut buf).unwrap(), 4);
    assert_eq!((replay.progress(), replay.percent()), ((4, 10), 40));
    replay.pause();
    assert_eq!(replay.read(&mut buf).unwrap(), 0);
    replay.resume();
    while replay.read(&mut buf).unwrap() > 0 {}
    assert!(replay.is_finished());
    assert_eq!((replay.progress(), replay.percent()), ((10, 10), 100));

    replay.restart().unwrap();
    assert!(!replay.is_finished());
    assert_eq!(replay.progress(), (0, 10));
    assert_eq!(replay.read(&mut buf).unwrap(), 4);
    assert_eq!(&buf, b"0123");
    let _ = std::fs::remove_file(&path);
}

/// The delay waits between two buffers, and can change while it plays.
/// How: a long delay, a read refused, then no delay.
#[test]
fn delay_changes_while_playing() {
    let (mut replay, path) = replay_of("delay", b"ab");
    replay.set_delay(Duration::from_secs(60));
    let mut buf = [0; 1];
    assert_eq!(replay.read(&mut buf).unwrap(), 0);
    replay.set_delay(Duration::ZERO);
    assert_eq!(replay.read(&mut buf).unwrap(), 1);
    let _ = std::fs::remove_file(&path);
}
