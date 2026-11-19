//! Wall-clock times (MAX-266): kept as a `Timestamp`, shown in the system's time zone through
//! strftime formats

use jiff::{Timestamp, ZonedDateTime, civil, fmt::strtime, tz::TimeZone};

/// `time` in the system's time zone
pub fn local(time: Timestamp) -> ZonedDateTime {
    time.to_zoned_datetime(TimeZone::system())
}

/// `time` written with the strftime `format`, in the system's time zone. A format jiff can't
/// write gives an empty text: the settings check theirs, a session's meta may hold anything
pub fn format(time: Timestamp, format: &str) -> String {
    strtime::format(format, &local(time)).unwrap_or_default()
}

/// Whether jiff can write the strftime `format`. It only finds out while writing, so a sample
/// time is written
pub fn check(format: &str) -> Result<(), String> {
    strtime::format(format, &local(Timestamp::now()))
        .map(drop)
        .map_err(|err| err.to_string())
}

/// The time a date and time on the system's wall clock names: the earlier one when the clock
/// went back over it, the later one when it jumped over it
pub fn from_local(time: civil::DateTime) -> Option<Timestamp> {
    TimeZone::system()
        .to_ambiguous_timestamp(time)
        .earlier()
        .ok()
}

#[cfg(test)]
#[path = "tests/clock.rs"]
mod tests;
