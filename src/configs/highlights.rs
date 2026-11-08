use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};

use crate::configs::{
    comments::{HIGHLIGHTS_COMMENTS, with_comments},
    style::ConfigStyle,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightConfig {
    #[serde(flatten)]
    pub rules: BTreeMap<String, ConfigStyle>,
}

impl Default for HighlightConfig {
    fn default() -> Self {
        let mut rules = BTreeMap::new();

        rules.insert(
            "ERROR".to_string(),
            ConfigStyle::new(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        );

        rules.insert(
            "WARN".to_string(),
            ConfigStyle::new(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        );

        rules.insert("ESP_OK".to_string(), ConfigStyle::from_fg(Color::Green));

        Self { rules }
    }
}

#[derive(Debug, Clone)]
pub struct KeywordHighlight {
    pub pattern: String,
    pub style: Style,
}

impl HighlightConfig {
    pub fn compile_rules(&self) -> Vec<KeywordHighlight> {
        self.rules
            .iter()
            .map(|(pattern, style)| KeywordHighlight {
                pattern: pattern.clone(),
                style: style.to_style(),
            })
            .collect()
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
