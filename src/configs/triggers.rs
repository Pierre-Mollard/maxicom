use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    app::triggers::{StreamMatcher, Trigger},
    commands::{self, CommandError, tokenizer},
    configs::comments::{TRIGGERS_COMMENTS, with_comments},
};

/// `triggers.toml` (MAX-6): a table per rule, named after it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggersConfig {
    #[serde(flatten)]
    pub rules: BTreeMap<String, TriggerRule>,
}

/// A rule as written: `match` is the text, with the command line escapes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerRule {
    #[serde(rename = "match")]
    pub text: String,
    pub run: String,
    #[serde(default)]
    pub rearm: bool,
    #[serde(default)]
    pub armed: bool,
}

impl Default for TriggersConfig {
    /// Examples, disarmed: nothing is sent until the user arms them
    fn default() -> Self {
        let rule = |text: &str, run: &str, rearm| TriggerRule {
            text: text.to_string(),
            run: run.to_string(),
            rearm,
            armed: false,
        };
        let rules = BTreeMap::from([
            (
                "u-boot".to_string(),
                rule("Hit any key to stop autoboot", "send ' '", false),
            ),
            ("login".to_string(), rule("login: ", "send root\\r", true)),
        ]);
        Self { rules }
    }
}

impl TriggersConfig {
    pub fn compile(&self) -> Result<Vec<Trigger>, TriggerError> {
        self.rules
            .iter()
            .map(|(name, rule)| {
                let error = |kind| TriggerError {
                    rule: name.clone(),
                    kind,
                };
                let text = tokenizer::unescape(&rule.text)
                    .map_err(|e| error(TriggerErrorKind::BadMatch(e)))?;
                let matcher =
                    StreamMatcher::new(&text).ok_or_else(|| error(TriggerErrorKind::EmptyMatch))?;
                let invocations =
                    commands::parse(&rule.run).map_err(|e| error(TriggerErrorKind::BadRun(e)))?;
                if invocations.is_empty() {
                    return Err(error(TriggerErrorKind::EmptyRun));
                }
                Ok(Trigger::new(
                    name.clone(),
                    rule.text.clone(),
                    matcher,
                    rule.run.trim().to_string(),
                    invocations,
                    rule.rearm,
                    rule.armed,
                ))
            })
            .collect()
    }

    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, TriggersFileError> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path).map_err(|err| TriggersFileError::Io {
                path: path.to_path_buf(),
                source: err,
            })?;

            let config: TriggersConfig =
                toml::from_str(&content).map_err(|err| TriggersFileError::TomlParse {
                    path: path.to_path_buf(),
                    source: err,
                })?;

            config.compile().map_err(|err| TriggersFileError::Rule {
                path: path.to_path_buf(),
                source: err,
            })?;

            return Ok(config);
        }

        let default_config = TriggersConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| TriggersFileError::Io {
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let toml_string =
            toml::to_string_pretty(&default_config).map_err(|err| TriggersFileError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::other(err.to_string()),
            })?;

        fs::write(path, with_comments(&toml_string, TRIGGERS_COMMENTS)).map_err(|err| {
            TriggersFileError::Io {
                path: path.to_path_buf(),
                source: err,
            }
        })?;

        Ok(default_config)
    }
}

/// A rule that doesn't compile, by its name
#[derive(Debug, PartialEq, Eq)]
pub struct TriggerError {
    pub rule: String,
    pub kind: TriggerErrorKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TriggerErrorKind {
    EmptyMatch,
    BadMatch(CommandError),
    EmptyRun,
    BadRun(CommandError),
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rule = &self.rule;
        match &self.kind {
            TriggerErrorKind::EmptyMatch => write!(f, "[{rule}]: match is empty"),
            TriggerErrorKind::BadMatch(err) => write!(f, "[{rule}]: match: {err}"),
            TriggerErrorKind::EmptyRun => write!(f, "[{rule}]: run is empty"),
            TriggerErrorKind::BadRun(err) => write!(f, "[{rule}]: run: {err}"),
        }
    }
}

impl std::error::Error for TriggerError {}

#[derive(Debug)]
pub enum TriggersFileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Rule {
        path: PathBuf,
        source: TriggerError,
    },
}

impl std::fmt::Display for TriggersFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read triggers at '{}': {source}",
                    path.display()
                )
            }
            Self::TomlParse { path, source } => {
                write!(f, "invalid syntax in '{}':\n{source}", path.display())
            }
            Self::Rule { path, source } => {
                write!(f, "invalid trigger in '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for TriggersFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TomlParse { source, .. } => Some(source),
            Self::Rule { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
#[path = "../tests/configs/triggers.rs"]
mod tests;
