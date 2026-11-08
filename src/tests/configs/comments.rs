//! The comments added to the generated config files.

use super::*;
use crate::configs::{
    HighlightConfig, KeyBindingsConfig, SettingsConfig, ThemeConfig, TriggersConfig,
};

/// Each key's path in a tree, its sections included
fn paths(prefix: &str, table: &toml::Table, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = match prefix {
            "" => key.clone(),
            _ => format!("{prefix}.{key}"),
        };
        if let toml::Value::Table(table) = value {
            paths(&path, table, out);
        }
        out.push(path);
    }
}

/// MAX-209: the top of the file and the sections take their comment above, a key at the end of
/// its line, an array on one line, two blank lines between top-level sections, and a key without
/// a comment stays as it was.
/// How: a small file, its exact output.
#[test]
fn comments_on_the_top_sections_and_keys() {
    let text = "a = 1\nb = 2\nlist = [\n    1,\n    2,\n]\n\n[one]\nc = 3\nd = 4\n\n[one.sub]\ne = 5\n\n[two]\nf = 6\n";
    let comments = [
        ("", "the file"),
        ("b", "about b"),
        ("list", "about list"),
        ("one", "first line\n\nthird line"),
        ("one.c", "about c"),
    ];
    let expected = "\
# the file

a = 1
b = 2 # about b
list = [1, 2] # about list


# first line
#
# third line
[one]
c = 3 # about c
d = 4

[one.sub]
e = 5


[two]
f = 6
";
    assert_eq!(with_comments(text, &comments), expected);
}

/// MAX-209: a line inside a multi-line value is never taken for a key or a section, whichever
/// way the value is written. A comment on a multi-line string goes above it, as the end of its
/// line is in the string.
/// How: lines that read as `[fake]` and `fake = 1` in both string forms and an array, with
/// comments on those names, and on a string and the key after them.
#[test]
fn multi_line_values_are_left_alone() {
    let text = "\
literal = '''
[fake]
fake = 1'''
basic = \"\"\"
[fake]
fake = 2\"\"\"
list = [
    \"fake = 3\",
    \"[fake]\",
]
one_line = '''still one line'''
after = 4
";
    let comments = [
        ("fake", "WRONG"),
        ("[fake]", "WRONG"),
        ("basic", "about basic"),
        ("after", "about after"),
    ];
    let out = with_comments(text, &comments);
    assert!(!out.contains("WRONG"), "{out}");
    assert!(
        out.contains("# about basic\nbasic = \"\"\"\n[fake]\n"),
        "{out}"
    );
    assert!(out.contains("list = [\"fake = 3\", \"[fake]\"]\n"), "{out}");
    assert!(out.ends_with("after = 4 # about after\n"), "{out}");
}

/// Fails when a comment on every key of `T`'s default file misses its key or changes a value
fn check_every_key_commented<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = toml::to_string_pretty(value).unwrap();
    let mut keys = Vec::new();
    paths("", &toml::from_str(&text).unwrap(), &mut keys);
    let texts: Vec<String> = keys.iter().map(|key| format!("comment of {key}")).collect();
    let comments: Vec<(&str, &str)> = (keys.iter().map(String::as_str))
        .zip(texts.iter().map(String::as_str))
        .collect();

    let out = with_comments(&text, &comments);
    for comment in &texts {
        let line = format!("# {comment}\n");
        assert_eq!(out.matches(&line).count(), 1, "{comment}:\n{out}");
    }
    assert_eq!(&toml::from_str::<T>(&out).unwrap(), value, "{out}");
}

/// MAX-209: on the real files, a comment on any key lands once, and the comments change no
/// value, multi-line status bar templates and strings full of quotes included.
/// How: settings and theme defaults with a comment on every key and section, then settings
/// whose strings look like TOML, serialized, commented and read back.
#[test]
fn comments_keep_the_values() {
    check_every_key_commented(&SettingsConfig::default());
    check_every_key_commented(&ThemeConfig::default());

    let mut settings = SettingsConfig::default();
    settings.capture.tx_mark = String::from("'''");
    settings.line_mode.prompt_prefix = String::from("\"\"\"");
    settings.line_mode.prompt_vi_prefix = String::from("x = '''");
    settings.terminal_mode.answerback = String::from("[fake]\nfake = 1\n'''\n\"\"\"\n");
    settings.ui.status_bar.left = String::from("[a]\nb = '''\n");
    check_every_key_commented(&settings);
}

/// MAX-209: comments on sections keep the rule-based and key-based files as they were.
/// How: the keymap, highlights and triggers defaults with a comment on each top-level name.
#[test]
fn comments_keep_the_other_files() {
    fn check<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let text = toml::to_string_pretty(value).unwrap();
        let tree: toml::Table = toml::from_str(&text).unwrap();
        let mut comments = vec![("", "the file")];
        comments.extend(tree.keys().map(|key| (key.as_str(), "a section")));
        let out = with_comments(&text, &comments);
        assert_eq!(&toml::from_str::<T>(&out).unwrap(), value, "{out}");
    }
    check(&KeyBindingsConfig::default());
    check(&HighlightConfig::default());
    check(&TriggersConfig::default());
}

/// A value in a tree by its dotted path
fn value_at<'a>(tree: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let (first, rest) = path.split_once('.').unwrap_or((path, ""));
    let value = tree.get(first)?;
    match (rest, value) {
        ("", _) => Some(value),
        (rest, toml::Value::Table(table)) => value_at(table, rest),
        _ => None,
    }
}

fn config_tree() -> toml::Table {
    toml::from_str(&toml::to_string_pretty(&SettingsConfig::default()).unwrap()).unwrap()
}

fn tree_of<T: serde::Serialize>(value: &T) -> toml::Table {
    toml::from_str(&toml::to_string_pretty(value).unwrap()).unwrap()
}

/// MAX-209: in every file, a comment on a key that doesn't exist fails, so a renamed or removed
/// setting takes its comment along, and a key's comment holds on its line. Only the keys with
/// choices have one.
/// How: every entry's path in the default's tree, and the keys' texts.
#[test]
fn comments_name_real_keys() {
    for (tree, list) in [
        (config_tree(), CONFIG_COMMENTS),
        (tree_of(&ThemeConfig::default()), THEME_COMMENTS),
        (tree_of(&KeyBindingsConfig::default()), KEYMAP_COMMENTS),
        (tree_of(&HighlightConfig::default()), HIGHLIGHTS_COMMENTS),
        (tree_of(&TriggersConfig::default()), TRIGGERS_COMMENTS),
    ] {
        for (path, text) in list {
            if path.is_empty() {
                continue;
            }
            let value = value_at(&tree, path);
            assert!(value.is_some(), "a comment on {path}, which doesn't exist");
            let is_section = matches!(value, Some(toml::Value::Table(_)));
            assert!(is_section || !text.contains('\n'), "{path}: one line");
        }
    }
}

/// Serde's aliases, accepted but not listed in the comments
const ENUM_ALIASES: [&str; 5] = ["oct", "bin", "bits", "byte", "bytes"];

/// What serde says a key accepts, none when it isn't an enum
fn serde_values(tree: &toml::Table, path: &str) -> Option<Vec<String>> {
    let (section, key) = path.rsplit_once('.').unwrap_or(("", path));
    let bogus = match value_at(tree, path)? {
        toml::Value::String(_) => "\"not-a-value\"",
        toml::Value::Array(_) => "[\"not-a-value\"]",
        _ => return None,
    };
    let text = format!("[{section}]\n{key} = {bogus}\n");
    let text = text.strip_prefix("[]\n").unwrap_or(&text);
    let error = toml::from_str::<SettingsConfig>(text).err()?.to_string();
    let (_, expected) = error
        .split_once("unknown variant")?
        .1
        .split_once("expected")?;
    let values = expected.split('`').skip(1).step_by(2);
    let values = values.filter(|value| !ENUM_ALIASES.contains(value));
    Some(values.map(str::to_owned).collect())
}

/// MAX-209: each `values:` line is what serde accepts, and every enum has one, so a new, renamed
/// or removed variant fails until the comment follows.
/// How: a bogus value on every key, the error's list against the comment, as sets.
#[test]
fn config_values_match_serde() {
    let tree = config_tree();
    let mut keys = Vec::new();
    paths("", &tree, &mut keys);
    for key in keys {
        let comment = CONFIG_COMMENTS.iter().find(|(path, _)| *path == key);
        let listed = comment.and_then(|(_, text)| {
            let line = text
                .lines()
                .find_map(|line| line.strip_prefix("values: "))?;
            let mut values: Vec<&str> = line.split(", ").collect();
            values.sort_unstable();
            Some(values)
        });
        let mut accepted = serde_values(&tree, &key);
        if let Some(accepted) = &mut accepted {
            accepted.sort_unstable();
        }
        assert_eq!(
            listed,
            accepted
                .as_ref()
                .map(|values| { values.iter().map(String::as_str).collect::<Vec<_>>() }),
            "{key}"
        );
    }
}

/// Fails when `list` changes a value of `T`'s default file
fn check_list_keeps_defaults<T>(list: &[(&str, &str)])
where
    T: Default + serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = toml::to_string_pretty(&T::default()).unwrap();
    let out = with_comments(&text, list);
    assert_eq!(toml::from_str::<T>(&out).unwrap(), T::default(), "{out}");
}

/// MAX-209: the real lists change no value of the files they're written in, and each file says
/// what it is at the top.
/// How: each default through its list, read back; each list's header entry.
#[test]
fn real_lists_keep_the_defaults() {
    check_list_keeps_defaults::<SettingsConfig>(CONFIG_COMMENTS);
    check_list_keeps_defaults::<ThemeConfig>(THEME_COMMENTS);
    check_list_keeps_defaults::<KeyBindingsConfig>(KEYMAP_COMMENTS);
    check_list_keeps_defaults::<HighlightConfig>(HIGHLIGHTS_COMMENTS);
    check_list_keeps_defaults::<TriggersConfig>(TRIGGERS_COMMENTS);

    for list in [
        CONFIG_COMMENTS,
        THEME_COMMENTS,
        KEYMAP_COMMENTS,
        HIGHLIGHTS_COMMENTS,
        TRIGGERS_COMMENTS,
    ] {
        assert!(list.iter().any(|(path, _)| path.is_empty()));
    }
}

/// MAX-209: the style written in `theme.toml`'s header is one the theme reads.
/// How: the header's example as an entry, then the same fields as a table.
#[test]
fn theme_header_style_loads() {
    let inline: ThemeConfig =
        toml::from_str("[ui]\nselection = { fg = \"Black\", bg = \"#FFB86C\", bold = true }\n")
            .unwrap();
    let table: ThemeConfig =
        toml::from_str("[ui.selection]\nfg = \"Black\"\nbg = \"#FFB86C\"\nbold = true\n").unwrap();
    assert_eq!(inline, table);
    assert_ne!(inline, ThemeConfig::default());
}
