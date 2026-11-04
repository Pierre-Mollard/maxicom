//! The session browser's color steps, and its theme section read back.

use super::*;

/// MAX-153: each date step starts at its bound, the last one has no end.
#[test]
fn session_date_steps() {
    let colors = SessionBrowserColors::default();
    let hours = |h: u64| Duration::from_secs(h * 60 * 60);
    let steps = [
        (Duration::ZERO, colors.date_hour),
        (hours(1) - Duration::from_secs(1), colors.date_hour),
        (hours(1), colors.date_day),
        (hours(24), colors.date_week),
        (hours(7 * 24), colors.date_month),
        (hours(30 * 24), colors.date_older),
        (hours(1000 * 24), colors.date_older),
    ];
    for (age, color) in steps {
        assert_eq!(colors.date_color(age), color, "{age:?}");
    }
}

/// MAX-153: each size step starts at its bound, 1 GiB and more is the last one.
#[test]
fn session_size_steps() {
    let colors = SessionBrowserColors::default();
    let (kib, mib) = (1024, 1024 * 1024);
    let steps = [
        (0, colors.size_empty),
        (1, colors.size_small),
        (100 * kib - 1, colors.size_small),
        (100 * kib, colors.size_medium),
        (mib, colors.size_large),
        (100 * mib, colors.size_huge),
        (1024 * mib - 1, colors.size_huge),
        (1024 * mib, colors.size_giant),
    ];
    for (bytes, color) in steps {
        assert_eq!(colors.size_color(bytes), color, "{bytes}");
    }
}

/// MAX-153: a port's color comes from its name only, so it is the same at every launch,
/// and close names get apart.
/// How: the FNV-1a hash of "ttyUSB0" is fixed, so its color is pinned.
#[test]
fn session_port_color_is_stable() {
    let colors = SessionBrowserColors::default();
    assert_eq!(colors.port_color("ttyUSB0"), colors.port_color("ttyUSB0"));
    assert_eq!(colors.port_color("ttyUSB0"), Color::Indexed(211));
    let ports = ["ttyUSB0", "ttyUSB1", "ttyACM0", "COM3"];
    let distinct: std::collections::HashSet<_> =
        ports.iter().map(|port| colors.port_color(port)).collect();
    assert!(distinct.len() > 1);
}

/// MAX-153: a theme giving only some of the section keeps the defaults for the rest, and
/// the defaults write and read back the same.
#[test]
fn session_browser_theme_reads_back() {
    let theme: ThemeConfig =
        toml::from_str("[session_browser]\ndate_hour = \"#00ff00\"\n").unwrap();
    let colors = theme.session_browser;
    assert_eq!(colors.date_hour, Color::Rgb(0, 0xff, 0));
    assert_eq!(
        colors.size_giant,
        SessionBrowserColors::default().size_giant
    );
    let written = toml::to_string_pretty(&ThemeConfig::default()).unwrap();
    let read: ThemeConfig = toml::from_str(&written).unwrap();
    assert_eq!(read, ThemeConfig::default());
}
