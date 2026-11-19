//! Config durations, read and written through jiff (MAX-266).

use super::*;

/// MAX-266: every form the docs, the default templates and humantime-era configs use still
/// reads the same; a day is 24 hours.
/// How: each text through `parse`, against the expected `Duration`.
#[test]
fn parse_keeps_the_documented_forms() {
    let cases = [
        ("500ms", Duration::from_millis(500)),
        ("30s", Duration::from_secs(30)),
        ("5m", Duration::from_secs(300)),
        ("5min", Duration::from_secs(300)),
        ("4h", Duration::from_secs(4 * 3600)),
        ("2 hours", Duration::from_secs(2 * 3600)),
        ("2h 30m", Duration::from_secs(9000)),
        ("1h30m", Duration::from_secs(5400)),
        ("1day", Duration::from_secs(24 * 3600)),
        ("1d", Duration::from_secs(24 * 3600)),
        ("1week", Duration::from_secs(7 * 24 * 3600)),
        ("0s", Duration::ZERO),
    ];
    for (text, expected) in cases {
        assert_eq!(parse(text), Ok(expected), "{text}");
    }
}

/// MAX-266: text that isn't a duration, a negative one and the units without a fixed length
/// (months, years) are errors.
/// How: each text through `parse`.
#[test]
fn parse_refuses_bad_durations() {
    for text in ["soon", "", "30", "-5s", "5s ago", "1month", "1y"] {
        assert!(parse(text).is_err(), "{text}");
    }
}

/// MAX-266: written back in jiff's friendly form, days the largest unit, and read back the same.
/// How: each `Duration` through `format`, then through `parse`.
#[test]
fn format_writes_the_friendly_form() {
    let cases = [
        (Duration::ZERO, "0s"),
        (Duration::from_millis(500), "500ms"),
        (Duration::from_millis(1500), "1s 500ms"),
        (Duration::from_secs(30), "30s"),
        (Duration::from_secs(150), "2m 30s"),
        (Duration::from_secs(4 * 3600), "4h"),
        (Duration::from_secs(24 * 3600), "1d"),
        (Duration::from_secs(9 * 24 * 3600), "9d"),
    ];
    for (duration, expected) in cases {
        assert_eq!(format(duration), expected);
        assert_eq!(parse(expected), Ok(duration));
    }
}
