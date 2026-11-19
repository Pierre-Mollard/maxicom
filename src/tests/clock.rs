//! Wall-clock times through jiff (MAX-266).

use super::*;

/// MAX-266: the strftime formats the defaults, the docs and the code use write what chrono
/// wrote, in the system's time zone.
/// How: one wall-clock time through `from_local`, then each format through `format`.
#[test]
fn formats_write_what_chrono_wrote() {
    let time = from_local(civil::date(2026, 11, 2).at(9, 5, 7, 42_000_000)).unwrap();
    let cases = [
        ("[%H:%M:%S%.3f]", "[09:05:07.042]"),
        ("%H:%M:%S", "09:05:07"),
        ("maxicom_%Y%m%d_%H%M%S", "maxicom_20261102_090507"),
        ("%Y%m%d_%H%M%S%3f", "20261102_090507042"),
        ("%Y-%m-%d %H:%M", "2026-11-02 09:05"),
        ("%G-W%V", "2026-W45"),
        ("%a %b %d", "Mon Nov 02"),
    ];
    for (pattern, expected) in cases {
        assert_eq!(format(time, pattern), expected, "{pattern}");
    }
}

/// MAX-266: a format jiff can't write fails the check and writes nothing, where chrono's
/// `format` panicked.
/// How: unknown specifiers and a lone `%` through `check` and `format`.
#[test]
fn bad_formats_fail_the_check() {
    assert!(check("[%H:%M:%S%.3f]").is_ok());
    for pattern in ["%J", "%", "%H:%M %", "%Ez"] {
        assert!(check(pattern).is_err(), "{pattern}");
        assert_eq!(format(Timestamp::now(), pattern), "", "{pattern}");
    }
}

/// MAX-266: a wall-clock time read back gives the same wall-clock time.
/// How: `from_local`, then `local`'s date and time.
#[test]
fn local_round_trip() {
    let wall = civil::date(2026, 7, 14).at(23, 59, 1, 0);
    assert_eq!(local(from_local(wall).unwrap()).datetime(), wall);
}
