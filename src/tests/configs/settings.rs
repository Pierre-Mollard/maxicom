//! Settings derived at load time.

use super::*;

/// MAX-181: `[link]` reads minicom-like numbers and names, an empty or missing `device`
/// is none, and out-of-range bits, stop bits or baud fail the check.
/// How: a full section, an empty one, then three bad values through `SettingsConfig`.
#[test]
fn link_defaults_from_toml() {
    let parse = |toml: &str| toml::from_str::<LinkDefaults>(toml).unwrap();
    let link = parse(
        "device = \"COM7\"\nbaud = 9600\nbits = 7\nparity = \"even\"\n\
             stop_bits = 2\nflow = \"rtscts\"",
    );
    assert_eq!(link.device(), Some("COM7"));
    let line = link.line();
    assert_eq!(
        (line.baud, line.framing.to_string(), line.flow_control),
        (9600, "7E2".to_string(), FlowControl::RtsCts)
    );
    let empty = parse("");
    assert_eq!(empty, LinkDefaults::default());
    assert_eq!(empty.device(), None);
    assert_eq!(empty.line(), LineSettings::default());

    for (toml, name) in [
        ("bits = 9", "link.bits"),
        ("stop_bits = 0", "link.stop_bits"),
        ("baud = 0", "link.baud"),
    ] {
        let settings = SettingsConfig {
            link: parse(toml),
            ..SettingsConfig::default()
        };
        let err = settings.validate().unwrap_err().to_string();
        assert!(err.starts_with(name), "{toml}: {err}");
    }
}

/// MAX-151: the answer settings are on by default, groups by name; `colors` is no group
#[test]
fn answer_settings_parse() {
    let parse = |toml: &str| toml::from_str::<TerminalModeConfig>(toml);
    let config = parse("").unwrap();
    assert!(config.answer_queries);
    assert_eq!(config.answer_groups, QueryGroup::ALL);
    assert_eq!(config.answerback, "");

    let config = parse("answer_groups = [\"size\", \"modes\"]\nanswerback = \"x\"").unwrap();
    assert_eq!(config.answer_groups, [QueryGroup::Size, QueryGroup::Modes]);
    assert_eq!(config.answerback, "x");
    assert!(parse("answer_groups = [\"colors\"]").is_err());
}

/// MAX-148: `restore_max_age` defaults to 24 hours; both read from the config, the age as
/// a human duration, `0` included. MAX-163: `sessions.restore` defaults to `always`.
#[test]
fn restore_settings_parse() {
    let sessions = SessionsConfig::default();
    assert_eq!(sessions.restore, RestoreMode::Always);
    assert_eq!(sessions.restore_max_age, Duration::from_secs(24 * 3600));
    let parse = |toml: &str| toml::from_str::<SessionsConfig>(toml).unwrap();
    let sessions = parse("restore = \"ask\"\nrestore_max_age = \"2h 30m\"");
    assert_eq!(sessions.restore, RestoreMode::Ask);
    assert_eq!(sessions.restore_max_age, Duration::from_secs(9000));
    assert!(parse("restore_max_age = \"0s\"").restore_max_age.is_zero());
    assert_eq!(parse("restore = \"never\"").restore, RestoreMode::Never);
}

/// MAX-163: `max_size` reads a number with a unit by 1024 (`B` and `iB` optional, any
/// case), a decimal, or bare bytes; `0` is no limit; it writes back in its biggest whole
/// unit; a bad unit or number fails the config.
#[test]
fn session_max_size_parses() {
    let parse = |toml: &str| toml::from_str::<SessionsConfig>(toml).map(|s| s.max_size.0);
    assert_eq!(parse("max_size = \"200M\""), Ok(200 << 20));
    assert_eq!(parse("max_size = \"200MB\""), Ok(200 << 20));
    assert_eq!(parse("max_size = \"512KiB\""), Ok(512 << 10));
    assert_eq!(parse("max_size = \"1.5gb\""), Ok(3 << 29));
    assert_eq!(parse("max_size = \"100\""), Ok(100));
    assert_eq!(parse("max_size = 100"), Ok(100));
    assert!(parse("max_size = \"200X\"").is_err());
    assert!(parse("max_size = \"big\"").is_err());

    let defaults = SessionsConfig::default();
    assert_eq!(defaults.limits().max_size, Some(200 << 20));
    assert_eq!(defaults.keep, 200);
    let unlimited = toml::from_str::<SessionsConfig>("max_size = \"0\"").unwrap();
    assert_eq!(unlimited.limits().max_size, None);

    for size in [200 << 20, 3 << 29, 1000, 0] {
        let text = ByteSize(size).to_string();
        assert_eq!(text.parse(), Ok(ByteSize(size)), "{text}");
    }
    assert_eq!(ByteSize(200 << 20).to_string(), "200M");
    assert_eq!(ByteSize(3 << 29).to_string(), "1536M");
}

/// MAX-163: the default status bar templates are written to the config file one block per
/// line, in `'''` literal strings where TOML keeps each `\`, and read back the same.
/// How: the default `[ui.status_bar]` written as the default config file is, then parsed.
#[test]
fn status_bar_templates_written_one_block_per_line() {
    let config = StatusBarConfig::default();
    let written = toml::to_string_pretty(&config).unwrap();
    assert!(
        written.contains("left = '''\n[ {mode} ](prefix?"),
        "{written}"
    );
    assert!(written.contains("\\\n    [|](muted prio=3)"), "{written}");
    assert!(written.contains("[{framing}](prio=4)'''"), "{written}");
    assert_eq!(toml::from_str::<StatusBarConfig>(&written).unwrap(), config);
}

/// The cached timestamp width matches a formatted timestamp, and feeds the line gutter.
/// How: default format "[%H:%M:%S%.3f]" is 14 chars, plus the 2-char line prefix.
#[test]
fn timestamp_width_is_cached_from_format() {
    let mut settings = SettingsConfig::default();
    assert_eq!(settings.timestamp_width(), 14);
    settings.show_timestamps = true;
    assert_eq!(
        settings.line_gutter_width(),
        14 + settings.line_prefixes().line.chars().count()
    );
}
