use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use humantime_serde::re::humantime;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::configs::comments::{PLOTS_COMMENTS, with_comments};

/// The most series a plot draws: `keys` can't name more
pub const MAX_SERIES: usize = 8;

/// `plots.toml` (MAX-222): a table per plot, named after it, in the file's order
#[derive(Debug, Clone, PartialEq)]
pub struct PlotsConfig {
    pub plots: Vec<(String, PlotDef)>,
}

/// A plot as written: the numbers it reads in a line, `key<separator>value` or bare columns,
/// and how it draws them
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlotDef {
    pub separator: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub columns: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_contains: Option<String>,
    pub window: PlotWindow,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, Color>,
}

impl Default for PlotDef {
    fn default() -> Self {
        Self {
            separator: "=".to_string(),
            keys: Vec::new(),
            columns: false,
            line_contains: None,
            window: PlotWindow::Time(Duration::from_secs(30)),
            min: None,
            max: None,
            colors: BTreeMap::new(),
        }
    }
}

/// The X axis: a duration slides on the time each line arrived, a number keeps the last
/// samples of each series
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotWindow {
    Time(Duration),
    Samples(usize),
}

impl Serialize for PlotWindow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Time(duration) => {
                serializer.serialize_str(&humantime::format_duration(*duration).to_string())
            }
            Self::Samples(count) => serializer.serialize_u64(*count as u64),
        }
    }
}

impl<'de> Deserialize<'de> for PlotWindow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Window;
        impl serde::de::Visitor<'_> for Window {
            type Value = PlotWindow;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a duration (\"30s\") or a number of samples (500)")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
                humantime::parse_duration(text)
                    .map(PlotWindow::Time)
                    .map_err(|err| E::custom(format!("window '{text}': {err}")))
            }

            fn visit_u64<E: serde::de::Error>(self, count: u64) -> Result<Self::Value, E> {
                Ok(PlotWindow::Samples(count as usize))
            }

            fn visit_i64<E: serde::de::Error>(self, count: i64) -> Result<Self::Value, E> {
                let count = u64::try_from(count)
                    .map_err(|_| E::custom(format!("window {count}: not a number of samples")))?;
                self.visit_u64(count)
            }
        }
        deserializer.deserialize_any(Window)
    }
}

/// Written as a map, in order; read in the file's order
impl Serialize for PlotsConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.plots.len()))?;
        for (name, plot) in &self.plots {
            map.serialize_entry(name, plot)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PlotsConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Plots;
        impl<'de> serde::de::Visitor<'de> for Plots {
            type Value = PlotsConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("plots: a table per plot")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut plots = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    plots.push(entry);
                }
                Ok(PlotsConfig { plots })
            }
        }
        deserializer.deserialize_map(Plots)
    }
}

impl Default for PlotsConfig {
    /// Two demos: `key=value` pairs on a time axis, and the Arduino IDE plotter's format
    /// (`name:value` or bare numbers) on the last 500 samples
    fn default() -> Self {
        let plots = vec![
            ("key-value".to_string(), PlotDef::default()),
            (
                "arduino".to_string(),
                PlotDef {
                    separator: ":".to_string(),
                    columns: true,
                    window: PlotWindow::Samples(500),
                    ..PlotDef::default()
                },
            ),
        ];
        Self { plots }
    }
}

impl PlotsConfig {
    pub fn names(&self) -> Vec<String> {
        self.plots.iter().map(|(name, _)| name.clone()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&PlotDef> {
        (self.plots.iter()).find_map(|(plot, def)| (plot == name).then_some(def))
    }

    /// The first plot that can't draw anything, by its name. `off` is `plot off`'s word
    pub fn check(&self) -> Result<(), PlotError> {
        for (name, plot) in &self.plots {
            if name == "off" {
                return Err(PlotError {
                    plot: name.clone(),
                    kind: PlotErrorKind::ReservedName,
                });
            }
            plot.check().map_err(|kind| PlotError {
                plot: name.clone(),
                kind,
            })?;
        }
        Ok(())
    }

    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, PlotsFileError> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path).map_err(|err| PlotsFileError::Io {
                path: path.to_path_buf(),
                source: err,
            })?;

            let config: PlotsConfig =
                toml::from_str(&content).map_err(|err| PlotsFileError::TomlParse {
                    path: path.to_path_buf(),
                    source: err,
                })?;

            config.check().map_err(|err| PlotsFileError::Plot {
                path: path.to_path_buf(),
                source: err,
            })?;

            return Ok(config);
        }

        let default_config = PlotsConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| PlotsFileError::Io {
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let toml_string =
            toml::to_string_pretty(&default_config).map_err(|err| PlotsFileError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::other(err.to_string()),
            })?;

        fs::write(path, with_comments(&toml_string, PLOTS_COMMENTS)).map_err(|err| {
            PlotsFileError::Io {
                path: path.to_path_buf(),
                source: err,
            }
        })?;

        Ok(default_config)
    }
}

impl PlotDef {
    fn check(&self) -> Result<(), PlotErrorKind> {
        if self.separator.is_empty() {
            return Err(PlotErrorKind::EmptySeparator);
        }
        if self.line_contains.as_deref() == Some("") {
            return Err(PlotErrorKind::EmptyLineContains);
        }
        if self.keys.len() > MAX_SERIES {
            return Err(PlotErrorKind::TooManyKeys(self.keys.len()));
        }
        if let Some(key) = self.keys.iter().find(|key| key.trim().is_empty()) {
            return Err(PlotErrorKind::BadKey(key.clone()));
        }
        if matches!(
            self.window,
            PlotWindow::Time(Duration::ZERO) | PlotWindow::Samples(0)
        ) {
            return Err(PlotErrorKind::EmptyWindow);
        }
        match (self.min, self.max) {
            (Some(min), Some(max)) if min >= max => Err(PlotErrorKind::BadRange { min, max }),
            _ => Ok(()),
        }
    }
}

/// A plot that can't draw anything, by its name
#[derive(Debug, PartialEq)]
pub struct PlotError {
    pub plot: String,
    pub kind: PlotErrorKind,
}

#[derive(Debug, PartialEq)]
pub enum PlotErrorKind {
    EmptySeparator,
    EmptyLineContains,
    TooManyKeys(usize),
    BadKey(String),
    EmptyWindow,
    BadRange { min: f64, max: f64 },
    ReservedName,
}

impl std::fmt::Display for PlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plot = &self.plot;
        match &self.kind {
            PlotErrorKind::EmptySeparator => write!(f, "[{plot}]: separator is empty"),
            PlotErrorKind::EmptyLineContains => write!(f, "[{plot}]: line_contains is empty"),
            PlotErrorKind::TooManyKeys(count) => {
                write!(f, "[{plot}]: {count} keys, at most {MAX_SERIES}")
            }
            PlotErrorKind::BadKey(key) => write!(f, "[{plot}]: key '{key}' is blank"),
            PlotErrorKind::EmptyWindow => write!(f, "[{plot}]: window is 0"),
            PlotErrorKind::BadRange { min, max } => {
                write!(f, "[{plot}]: min {min} is not below max {max}")
            }
            PlotErrorKind::ReservedName => {
                write!(f, "[{plot}]: 'off' turns the plot off, name it otherwise")
            }
        }
    }
}

impl std::error::Error for PlotError {}

#[derive(Debug)]
pub enum PlotsFileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Plot {
        path: PathBuf,
        source: PlotError,
    },
}

impl std::fmt::Display for PlotsFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read plots at '{}': {source}", path.display())
            }
            Self::TomlParse { path, source } => {
                write!(f, "invalid syntax in '{}':\n{source}", path.display())
            }
            Self::Plot { path, source } => {
                write!(f, "invalid plot in '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PlotsFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TomlParse { source, .. } => Some(source),
            Self::Plot { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
#[path = "../tests/configs/plots.rs"]
mod tests;
