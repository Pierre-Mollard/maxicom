use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    app::triggers::{StreamMatcher, Trigger},
    commands::{self, CommandError, tokenizer},
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

        fs::write(path, toml_string).map_err(|err| TriggersFileError::Io {
            path: path.to_path_buf(),
            source: err,
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
mod tests {
    use super::*;

    fn compile(toml: &str) -> Result<Vec<Trigger>, String> {
        let config: TriggersConfig = toml::from_str(toml).map_err(|e| e.to_string())?;
        config.compile().map_err(|e| e.to_string())
    }

    /// The default file round-trips and compiles: both examples, disarmed, the login one
    /// rearming.
    #[test]
    fn default_examples() {
        let text = toml::to_string_pretty(&TriggersConfig::default()).unwrap();
        let config: TriggersConfig = toml::from_str(&text).unwrap();
        assert_eq!(config, TriggersConfig::default());
        let triggers = config.compile().unwrap();
        let names: Vec<_> = triggers.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["login", "u-boot"]);
        assert!(triggers.iter().all(|t| !t.is_armed()));
        assert!(triggers[0].rearm && !triggers[1].rearm);
        assert_eq!(triggers[0].invocations.len(), 1);
    }

    /// `rearm` and `armed` are optional; `match` takes the escapes, `run` a command line.
    #[test]
    fn rule_fields() {
        let triggers = compile(
            r#"
            [menu]
            match = '\x1b[7mBoot menu'
            run = "send-hex 1b; sleep 50; send 1"
            armed = true
            "#,
        )
        .unwrap();
        let menu = &triggers[0];
        assert!(menu.is_armed() && !menu.rearm);
        assert_eq!(menu.invocations.len(), 3);
        let mut menu = menu.clone();
        assert!(menu.fires(b"\x1b[7mBoot menu\x1b[0m"));
    }

    /// A bad rule fails the load, naming it; so does a field that isn't one.
    #[test]
    fn bad_rules() {
        let rule = |text: &str, run: &str| format!("[r]\nmatch = '{text}'\nrun = '{run}'\n");
        assert_eq!(
            compile(&rule("", "send x")).unwrap_err(),
            "[r]: match is empty"
        );
        assert_eq!(
            compile(&rule("\\q", "send x")).unwrap_err(),
            "[r]: match: bad escape '\\q'"
        );
        assert_eq!(compile(&rule("x", " ; ")).unwrap_err(), "[r]: run is empty");
        assert_eq!(
            compile(&rule("x", "rst on")).unwrap_err(),
            "[r]: run: unknown command 'rst'"
        );
        let typo = compile("[r]\nmatch = 'x'\nrun = 'send x'\nrearn = true\n").unwrap_err();
        assert!(typo.contains("unknown field `rearn`"), "{typo}");
    }
}
