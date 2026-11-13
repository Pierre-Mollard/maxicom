//! One history entry's line: the time column before the prefix and the content.

use super::*;
use chrono::{Local, TimeZone};

fn settings() -> SettingsConfig {
    let mut settings = SettingsConfig::default();
    settings.show_timestamps = true;
    settings.timestamp_format = "[%H:%M:%S]".to_string();
    settings.refresh_timestamp_width();
    settings
}

fn spans(entry: &LogEntry, settings: &SettingsConfig) -> Vec<(String, Style)> {
    let theme = ThemeConfig::default();
    let prefix = settings.line_prefixes().line.to_string();
    history_entry_to_line(entry, settings, &[], &[], None, &theme, &prefix)
        .spans
        .into_iter()
        .map(|span| (span.content.into_owned(), span.style))
        .collect()
}

/// A timed entry starts with its time in the timestamp style, then the prefix and the text;
/// with timestamps hidden, the prefix comes first.
#[test]
fn entry_line_starts_with_its_time() {
    let theme = ThemeConfig::default();
    let time = Local.with_ymd_and_hms(2026, 1, 1, 12, 34, 56).unwrap();
    let entry = LogEntry::new("hello".to_string(), time);
    let mut settings = settings();
    let line = spans(&entry, &settings);
    assert_eq!(
        line[0],
        ("[12:34:56]".to_string(), *theme.line_mode.timestamp)
    );
    assert_eq!(line[1].0, settings.line_prefixes().line);
    assert_eq!(line[2].0, "hello");

    settings.show_timestamps = false;
    assert_eq!(spans(&entry, &settings)[0].0, settings.line_prefixes().line);
}

/// MAX-148: an entry without a time keeps the time column blank, as wide as a time, so its
/// prefix and text line up with the timed ones.
#[test]
fn entry_without_time_has_a_blank_column() {
    let time = Local.with_ymd_and_hms(2026, 1, 1, 12, 34, 56).unwrap();
    let mut entry = LogEntry::new("hello".to_string(), time);
    let settings = settings();
    let timed = spans(&entry, &settings);
    entry.timestamp = None;
    let blank = spans(&entry, &settings);
    assert_eq!(blank[0].0, " ".repeat(settings.timestamp_width()));
    assert_eq!(blank[0].0.width(), timed[0].0.width());
    assert_eq!(blank[1..], timed[1..]);
}
