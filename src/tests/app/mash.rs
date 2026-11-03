
use super::*;

/// A space every 50 ms, at once first, never two for a late turn, until 200 ms.
#[test]
fn paced_until_the_timeout() {
    let start = Instant::now();
    let ms = |ms| start + Duration::from_millis(ms);
    let every = Duration::from_millis(50);
    let timeout = Duration::from_millis(200);
    let mut mash = Mash::new(b" ".to_vec(), b"=> ", every, timeout, start).unwrap();
    assert_eq!(mash.step(start), MashStep::Send(b" "));
    assert_eq!(mash.step(ms(49)), MashStep::Wait);
    assert_eq!(mash.step(ms(50)), MashStep::Send(b" "));
    // a late turn: one send, the next 50 ms after it
    assert_eq!(mash.step(ms(170)), MashStep::Send(b" "));
    assert_eq!(mash.step(ms(199)), MashStep::Wait);
    assert_eq!(mash.step(ms(200)), MashStep::TimedOut);
    assert_eq!(mash.deadline(), ms(200));
}

/// The text waited for is matched across chunks, and shown printable.
#[test]
fn until_seen() {
    let now = Instant::now();
    let second = Duration::from_secs(1);
    assert!(Mash::new(b" ".to_vec(), b"", second, second, now).is_none());
    let mut mash = Mash::new(b"\x03".to_vec(), b"=> ", second, second, now).unwrap();
    assert!(!mash.until_seen(b"autoboot in 1\r\n="));
    assert!(mash.until_seen(b"> "));
    let mash = Mash::new(b" ".to_vec(), b"\r\n#", second, second, now).unwrap();
    assert_eq!(mash.until_text(), "\\r\\n#");
}
