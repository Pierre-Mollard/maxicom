use std::{
    fs,
    path::{Path, PathBuf},
};

use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};

use crate::configs::{
    comments::{HIGHLIGHTS_COMMENTS, with_comments},
    style::{ConfigStyle, StyleTable},
};

/// The highlights, in the file's order: a later one is drawn over an earlier one (MAX-216)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightConfig {
    pub entries: Vec<(String, HighlightEntry)>,
}

/// One entry of the file: a group of `words` with one style (MAX-216), or a single word, its
/// key, as released: `WORD = "Green"` or a `[WORD]` style table; or the prefixes of the lines
/// it styles whole, `line_start` (MAX-229)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EntryDef", into = "EntryDef")]
pub struct HighlightEntry {
    pub words: Option<Vec<String>>,
    pub line_start: Option<Vec<String>>,
    pub ignore_case: bool,
    pub whole_word: bool,
    pub style: ConfigStyle,
}

impl HighlightEntry {
    /// One word in a color, as `WORD = "Green"` writes it
    pub fn color(fg: Color) -> Self {
        Self::styled(Style::default().fg(fg))
    }

    pub fn styled(style: Style) -> Self {
        Self {
            words: None,
            line_start: None,
            ignore_case: false,
            whole_word: false,
            style: style.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum EntryDef {
    Color(Color),
    Table(EntryTable),
}

#[derive(Serialize, Deserialize)]
struct EntryTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    words: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_start: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    ignore_case: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    whole_word: bool,
    #[serde(flatten)]
    style: StyleTable,
}

impl TryFrom<EntryDef> for HighlightEntry {
    type Error = &'static str;

    fn try_from(def: EntryDef) -> Result<Self, Self::Error> {
        let table = match def {
            EntryDef::Color(fg) => return Ok(Self::color(fg)),
            EntryDef::Table(table) => table,
        };
        if table.line_start.is_some() {
            if table.words.is_some() {
                return Err("an entry has `words` or `line_start`, not both");
            }
            if table.whole_word {
                return Err("`whole_word` doesn't go with `line_start`");
            }
        }
        Ok(Self {
            words: table.words,
            line_start: table.line_start,
            ignore_case: table.ignore_case,
            whole_word: table.whole_word,
            style: table.style.into(),
        })
    }
}

impl From<HighlightEntry> for EntryDef {
    fn from(entry: HighlightEntry) -> Self {
        let single = entry.words.is_none()
            && entry.line_start.is_none()
            && !entry.ignore_case
            && !entry.whole_word;
        match entry.style.fg {
            Some(fg) if single && entry.style.is_fg_only() => Self::Color(fg),
            _ => Self::Table(EntryTable {
                words: entry.words,
                line_start: entry.line_start,
                ignore_case: entry.ignore_case,
                whole_word: entry.whole_word,
                style: entry.style.into(),
            }),
        }
    }
}

/// Written as a map, in order; read in the file's order
impl Serialize for HighlightConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, entry) in &self.entries {
            map.serialize_entry(key, entry)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for HighlightConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> serde::de::Visitor<'de> for Entries {
            type Value = HighlightConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("highlights: a word or a group, each with its style")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(HighlightConfig { entries })
            }
        }
        deserializer.deserialize_map(Entries)
    }
}

impl Default for HighlightConfig {
    /// Three groups, case ignored and whole words only: errors, warnings, successes; and a word
    /// on its own in each released form, as examples: `INFO` in a color, `TIMEOUT` in a style
    /// (MAX-216)
    fn default() -> Self {
        let group = |words: &[&str], style: Style| HighlightEntry {
            words: Some(words.iter().map(|word| word.to_string()).collect()),
            line_start: None,
            ignore_case: true,
            whole_word: true,
            style: style.into(),
        };
        let bold = ratatui::style::Modifier::BOLD;
        let entries = vec![
            ("INFO".to_string(), HighlightEntry::color(Color::Cyan)),
            (
                "errors".to_string(),
                group(
                    &[
                        "ERROR", "ERR", "FAIL", "FAILED", "FAILURE", "FATAL", "PANIC",
                    ],
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Red)
                        .add_modifier(bold),
                ),
            ),
            (
                "warnings".to_string(),
                group(
                    &["WARN", "WARNING"],
                    Style::default().fg(Color::Yellow).add_modifier(bold),
                ),
            ),
            (
                "ok".to_string(),
                group(
                    &["OK", "SUCCESS", "PASS", "PASSED"],
                    Style::default().fg(Color::Green),
                ),
            ),
            (
                "TIMEOUT".to_string(),
                HighlightEntry::styled(Style::default().fg(Color::Magenta).add_modifier(bold)),
            ),
        ];
        Self { entries }
    }
}

/// A word to color, from an entry: its group's style and matching flags (MAX-216); or, with
/// `line_start`, the prefix of the lines to color whole (MAX-229)
#[derive(Debug, Clone)]
pub struct KeywordHighlight {
    pub pattern: String,
    pub style: Style,
    pub ignore_case: bool,
    pub whole_word: bool,
    pub line_start: bool,
}

impl HighlightConfig {
    /// One rule per word or line prefix, in the file's order, the line rules first so words are
    /// drawn over the lines; an entry without `words` or `line_start` is its key
    pub fn compile_rules(&self) -> Vec<KeywordHighlight> {
        let rules = |line_rules: bool| {
            self.entries
                .iter()
                .filter(move |(_, entry)| entry.line_start.is_some() == line_rules)
                .flat_map(|(key, entry)| {
                    let patterns = (entry.line_start.clone())
                        .or_else(|| entry.words.clone())
                        .unwrap_or_else(|| vec![key.clone()]);
                    patterns.into_iter().map(|pattern| KeywordHighlight {
                        pattern,
                        style: entry.style.to_style(),
                        ignore_case: entry.ignore_case,
                        whole_word: entry.whole_word,
                        line_start: entry.line_start.is_some(),
                    })
                })
        };
        rules(true).chain(rules(false)).collect()
    }

    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, HighlightError> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path).map_err(|err| HighlightError::Io {
                path: path.to_path_buf(),
                source: err,
            })?;

            let config: HighlightConfig =
                toml::from_str(&content).map_err(|err| HighlightError::Parse {
                    path: path.to_path_buf(),
                    source: err,
                })?;

            return Ok(config);
        }

        let default_config = HighlightConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| HighlightError::Io {
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let toml_string =
            toml::to_string_pretty(&default_config).map_err(|err| HighlightError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::other(err.to_string()),
            })?;

        fs::write(path, with_comments(&toml_string, HIGHLIGHTS_COMMENTS)).map_err(|err| {
            HighlightError::Io {
                path: path.to_path_buf(),
                source: err,
            }
        })?;

        Ok(default_config)
    }
}

#[derive(Debug)]
pub enum HighlightError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read highlights at '{}': {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(f, "invalid highlight in '{}':\n{source}", path.display())
            }
        }
    }
}

impl std::error::Error for HighlightError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
#[path = "../tests/configs/highlights.rs"]
mod tests;
