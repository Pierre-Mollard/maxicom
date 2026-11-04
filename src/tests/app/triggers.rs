use super::*;

const UBOOT: &[u8] = b"Hit any key to stop autoboot";

fn matcher() -> StreamMatcher {
    StreamMatcher::new(UBOOT).unwrap()
}

#[test]
fn empty_text() {
    assert!(StreamMatcher::new(b"").is_none());
}

/// Found in the chunk where it ends, cut anywhere: each cut of the line in two, then one
/// byte a chunk.
#[test]
fn cut_across_chunks() {
    let line = b"\r\nU-Boot 2024.01\r\nHit any key to stop autoboot:  3 ";
    for cut in 0..=line.len() {
        let mut matcher = matcher();
        let (first, second) = line.split_at(cut);
        let ends_in_first = cut >= line.len() - 5;
        assert_eq!(matcher.feed(first), ends_in_first, "cut at {cut}");
        assert_eq!(matcher.feed(second), !ends_in_first, "cut at {cut}");
    }

    let mut matcher = matcher();
    let found: Vec<usize> = (line.iter().enumerate())
        .filter(|(_, byte)| matcher.feed(&[**byte]))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(found, [line.len() - 6]);
}

/// U-Boot's countdown: the digit redrawn with `\b` after the text, then the text cut
/// short when a key stops it. The text is found once, the redraws don't find it again.
#[test]
fn countdown_redraws() {
    let mut matcher = matcher();
    assert!(matcher.feed(b"Hit any key to stop autoboot:  3"));
    for redraw in [b"\x08\x08\x08  2", b"\x08\x08\x08  1", b"\x08\x08\x08  0"] {
        assert!(!matcher.feed(redraw));
    }
    // a `\b` inside the text isn't the text
    assert!(!matcher.feed(b"\r\nHit any key to stop auto\x08boot"));
}

/// Once per appearance: the same chunk twice in it is one match; it matches again when it
/// comes again, even right after.
#[test]
fn again_after_new_output() {
    let mut matcher = StreamMatcher::new(b"=> ").unwrap();
    assert!(matcher.feed(b"=> => "));
    assert!(!matcher.feed(b"printenv\r\n"));
    assert!(matcher.feed(b"bootdelay=3\r\n=> "));
    assert!(matcher.feed(b"=> "));
    assert!(!matcher.feed(b""));
}

/// A one-byte text keeps no tail.
#[test]
fn one_byte() {
    let mut matcher = StreamMatcher::new(b"#").unwrap();
    assert!(!matcher.feed(b"root@board:~"));
    assert!(matcher.feed(b"# "));
    assert!(!matcher.feed(b" "));
}

fn trigger(rearm: bool, armed: bool) -> Trigger {
    let invocations = crate::commands::parse("send ' '").unwrap();
    let run = "send ' '".to_string();
    Trigger::new(
        "u-boot".into(),
        String::from_utf8_lossy(UBOOT).into(),
        matcher(),
        run,
        invocations,
        rearm,
        armed,
    )
}

/// A trigger fires while armed; one shot, it disarms then; with `rearm` it fires again.
#[test]
fn one_shot_or_rearmed() {
    let line = b"Hit any key to stop autoboot:  3";
    let mut disarmed = trigger(false, false);
    assert!(!disarmed.fires(line));

    let mut once = trigger(false, true);
    assert!(once.fires(line));
    assert!(!once.is_armed());
    assert!(!once.fires(line));

    let mut rearmed = trigger(true, true);
    assert!(rearmed.fires(line));
    assert!(rearmed.fires(line));
    assert!(rearmed.is_armed());
}

/// Armed again, a trigger starts from the next bytes.
#[test]
fn armed_again() {
    let mut uboot = trigger(false, true);
    assert!(!uboot.fires(b"Hit any key"));
    uboot.arm(false);
    uboot.arm(true);
    assert!(!uboot.fires(b" to stop autoboot"));
    // arming an armed one keeps what it saw
    assert!(!uboot.fires(b"Hit any key"));
    uboot.arm(true);
    assert!(uboot.fires(b" to stop autoboot"));
}

/// After a reset, a text half seen before doesn't complete.
#[test]
fn reset() {
    let mut matcher = matcher();
    assert!(!matcher.feed(b"Hit any key"));
    matcher.reset();
    assert!(!matcher.feed(b" to stop autoboot"));
    assert!(matcher.feed(b"Hit any key to stop autoboot"));
}
