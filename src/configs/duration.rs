use jiff::{SignedDuration, Span, SpanRelativeTo, SpanRound, Unit};
use serde::{Deserialize, Deserializer, Serializer};
use std::time::Duration;

/// A config duration in jiff's friendly format ("500ms", "2h 30m", "1day", "1w"), a day
/// being 24 hours. Months and years have no fixed length and are refused
pub fn parse(text: &str) -> Result<Duration, String> {
    let span: Span = text.parse().map_err(|err| format!("{err}"))?;
    let signed = span
        .to_duration(SpanRelativeTo::days_are_24_hours())
        .map_err(|err| format!("{err}"))?;
    Duration::try_from(signed).map_err(|_| String::from("a duration can't be negative"))
}

/// The friendly form, days the largest unit: "1d", "2m 30s", "500ms", "0s"
pub fn format(duration: Duration) -> String {
    let span = SignedDuration::try_from(duration)
        .and_then(Span::try_from)
        .and_then(|span| span.round(SpanRound::new().largest(Unit::Day).days_are_24_hours()));
    match span {
        Ok(span) => format!("{span:#}"),
        Err(_) => format!("{}s", duration.as_secs()),
    }
}

pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format(*duration))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse(&text).map_err(|err| serde::de::Error::custom(format!("'{text}': {err}")))
}

#[cfg(test)]
#[path = "../tests/configs/duration.rs"]
pub mod tests;
