use super::*;
use ratatui::style::Modifier;

fn rules(toml: &str) -> Vec<KeywordHighlight> {
    let config: HighlightConfig = toml::from_str(toml).unwrap();
    config.compile_rules()
}

fn style_of<'a>(rules: &'a [KeywordHighlight], pattern: &str) -> &'a Style {
    &rules.iter().find(|r| r.pattern == pattern).unwrap().style
}

/// MAX-216 step 0: the file's forms as released (0.1.x) keep parsing: a word = a color, a
/// table = a style, a quoted key for a word with spaces.
/// How: one entry of each, compiled.
#[test]
fn released_forms_parse() {
    let rules = rules(
        r#"
ESP_OK = "Green"
"Guru Meditation" = "LightRed"

[ERROR]
fg = "White"
bg = "Red"
bold = true
"#,
    );
    assert_eq!(rules.len(), 3);
    assert_eq!(style_of(&rules, "ESP_OK").fg, Some(Color::Green));
    assert_eq!(
        style_of(&rules, "Guru Meditation").fg,
        Some(Color::LightRed)
    );
    let error = style_of(&rules, "ERROR");
    assert_eq!((error.fg, error.bg), (Some(Color::White), Some(Color::Red)));
    assert!(error.add_modifier.contains(Modifier::BOLD));
}

/// MAX-216: the default file round-trips, its three groups in order (errors, warnings, ok),
/// each ignoring case and matching whole words, between the two single words given as examples:
/// `INFO` in a color, `TIMEOUT` in a style, both as written, inside other words too.
/// How: the defaults written as TOML, read back, compiled.
#[test]
fn default_file_round_trips() {
    let text = toml::to_string_pretty(&HighlightConfig::default()).unwrap();
    let config: HighlightConfig = toml::from_str(&text).unwrap();
    assert_eq!(config, HighlightConfig::default());
    let names: Vec<_> = config
        .entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, ["INFO", "errors", "warnings", "ok", "TIMEOUT"]);
    let rules = config.compile_rules();
    let (single, groups): (Vec<_>, Vec<_>) = rules
        .iter()
        .partition(|r| ["INFO", "TIMEOUT"].contains(&r.pattern.as_str()));
    assert_eq!(single.len(), 2);
    assert!(single.iter().all(|r| !r.ignore_case && !r.whole_word));
    assert!(groups.iter().all(|r| r.ignore_case && r.whole_word));
    assert_eq!(*style_of(&rules, "INFO"), Style::default().fg(Color::Cyan));
    let timeout = style_of(&rules, "TIMEOUT");
    assert_eq!(timeout.fg, Some(Color::Magenta));
    assert!(timeout.add_modifier.contains(Modifier::BOLD));
    assert_eq!(style_of(&rules, "FAILED").bg, Some(Color::Red));
    assert_eq!(style_of(&rules, "WARNING").fg, Some(Color::Yellow));
    assert_eq!(style_of(&rules, "OK").fg, Some(Color::Green));
}

/// MAX-216: a group is a table with a `words` list, its matching flags and its style; each
/// word becomes a rule with the group's style and flags. A one-word table takes the flags
/// too, its key the word.
/// How: an `errors` group of three words, a `[WARN]` table with `ignore_case`.
#[test]
fn group_entry_compiles_to_one_rule_per_word() {
    let rules = rules(
        r#"
[errors]
words = ["ERROR", "FAIL", "Failed"]
ignore_case = true
whole_word = true
fg = "White"
bg = "Red"

[WARN]
ignore_case = true
fg = "Yellow"
"#,
    );
    let patterns: Vec<_> = rules.iter().map(|r| r.pattern.as_str()).collect();
    assert_eq!(patterns, ["ERROR", "FAIL", "Failed", "WARN"]);
    for rule in &rules[..3] {
        assert!(rule.ignore_case && rule.whole_word, "{}", rule.pattern);
        assert_eq!(
            (rule.style.fg, rule.style.bg),
            (Some(Color::White), Some(Color::Red))
        );
    }
    assert!(rules[3].ignore_case && !rules[3].whole_word);
    assert_eq!(rules[3].style.fg, Some(Color::Yellow));
}

/// MAX-216: the rules keep the file's order, so a later entry is drawn over an earlier one
/// whatever their names; the flags are off by default.
/// How: `ZED`, a group `alpha`, `MID`, in that order.
#[test]
fn rules_keep_the_file_order() {
    let rules = rules(
        r#"
ZED = "Red"
MID = "Blue"

[alpha]
words = ["a", "b"]
fg = "Green"

[ONE]
fg = "Cyan"
"#,
    );
    let patterns: Vec<_> = rules.iter().map(|r| r.pattern.as_str()).collect();
    assert_eq!(patterns, ["ZED", "MID", "a", "b", "ONE"]);
    assert!(rules.iter().all(|r| !r.ignore_case && !r.whole_word));
}

/// MAX-216: written back (`--write-config`), a file keeps its entries in order and in their
/// form: a word with a color stays `KEY = "color"`, a group a table with `words` first.
/// How: a file read, written, read again.
#[test]
fn groups_round_trip_in_order() {
    let text = r#"
ZED = "Red"

[errors]
words = ["ERROR", "FAIL"]
ignore_case = true
fg = "White"
bg = "Red"
"#;
    let config: HighlightConfig = toml::from_str(text).unwrap();
    let written = toml::to_string_pretty(&config).unwrap();
    let again: HighlightConfig = toml::from_str(&written).unwrap();
    assert_eq!(again, config);
    assert!(written.starts_with("ZED = \"Red\"\n"), "{written}");
    let words = written.find("words = ").unwrap();
    assert!(words < written.find("ignore_case").unwrap(), "{written}");
    assert!(
        written.find("ignore_case").unwrap() < written.find("fg = ").unwrap(),
        "{written}"
    );
}
