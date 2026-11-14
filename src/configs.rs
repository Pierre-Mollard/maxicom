use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::de::DeserializeOwned;

use crate::paths::expand_home;

pub mod comments;
pub mod highlights;
pub mod keybindings;
pub mod links;
pub mod plots;
// TODO: (MAX-240) remove the allow at step 3, where a link's profiles are layered
#[allow(dead_code)]
pub mod profiles;
pub mod settings;
pub mod style;
pub mod theme;
pub mod triggers;
pub mod update;

// Re-exports
pub use highlights::{HighlightConfig, HighlightError, KeywordHighlight};
pub use keybindings::{
    KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
    KeyBindingTerminalAction, KeyBindingsConfig, KeyPattern, KeyResolver, KeymapFileError,
};
pub use links::LinksConfig;
pub use plots::{PlotsConfig, PlotsFileError};
pub use settings::{
    CommandLineLayout, DualBufferMode, LineDisplayMode, LinkDefaults, Mode, PrefixWrapBehavior,
    QueryGroup, RawSideView, SettingsConfig, SettingsError, StatusBarConfig, StatusBarLayout,
    TerminalDimensionMode, TerminalModeConfig, TextEncoding, WrapMode,
};
pub use theme::{ThemeConfig, ThemeError};
pub use triggers::{TriggersConfig, TriggersFileError};

const APP_DIR: &str = "maxicom";
const SESSION_DIR: &str = "sessions";
const CONFIG_FILE: &str = "config.toml";
const THEME_FILE: &str = "theme.toml";
const KEYMAP_FILE: &str = "keymap.toml";
const HIGHLIGHT_FILE: &str = "highlights.toml";
const TRIGGERS_FILE: &str = "triggers.toml";
const LINKS_FILE: &str = "links.toml";
const PLOTS_FILE: &str = "plots.toml";

/// The settings file given with `--config`, set once at startup (MAX-206)
static CONFIG_FILE_GIVEN: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: SettingsConfig,
    pub theme: ThemeConfig,
    pub keybindings: KeyBindingsConfig,
    pub highlights: HighlightConfig,
    pub triggers: TriggersConfig,
    pub plots: PlotsConfig,
    /// A `links.toml` that doesn't load doesn't stop the app: its error, said in the app
    pub links: Result<LinksConfig, String>,
    /// What `unknown_keys_warning` found in the settings and the theme, said in the app
    pub warnings: Vec<String>,
}

/// The keys of `text` that `T` doesn't read: a typo or a renamed key takes its default without a
/// word otherwise, as the settings use `#[serde(default)]` (MAX-209). None for a file that
/// doesn't load, the loader says why. A style's own fields aren't checked
fn unknown_keys<T: DeserializeOwned>(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let Ok(deserializer) = toml::Deserializer::parse(text) else {
        return keys;
    };
    let loaded: Result<T, _> =
        serde_ignored::deserialize(deserializer, |path| keys.push(path.to_string()));
    if loaded.is_err() {
        keys.clear();
    }
    // sorted, the TOML parser keeping the file's order (MAX-216)
    keys.sort();
    keys
}

/// A warning naming the keys of the file that `T` doesn't read, none when it reads them all
fn unknown_keys_warning<T: DeserializeOwned>(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let keys = unknown_keys::<T>(&text);
    let name = path.file_name()?.to_string_lossy();
    (!keys.is_empty()).then(|| format!("Unknown keys in {name}: {}", keys.join(", ")))
}

impl AppConfig {
    /// Loads the main settings file, then loads the active theme and keymap paths referenced by it.
    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, ConfigsError> {
        let path = path.as_ref();

        let settings =
            SettingsConfig::load_or_create_default(path).map_err(ConfigsError::Settings)?;

        let theme_path = Self::active_theme_path(&settings);
        let theme =
            ThemeConfig::load_or_create_default(&theme_path).map_err(ConfigsError::Theme)?;

        let keybindings_path = Self::active_keybindings_path(&settings);
        let keybindings = KeyBindingsConfig::load_or_create_default(&keybindings_path)
            .map_err(|err| ConfigsError::Keymap(Box::new(err)))?;

        let highlights_path = Self::active_highlight_path(&settings);
        let highlights = HighlightConfig::load_or_create_default(&highlights_path)
            .map_err(ConfigsError::Highlight)?;

        let triggers_path = Self::active_triggers_path(&settings);
        let triggers = TriggersConfig::load_or_create_default(&triggers_path)
            .map_err(ConfigsError::Triggers)?;

        let plots_path = Self::active_plots_path(&settings);
        let plots =
            PlotsConfig::load_or_create_default(&plots_path).map_err(ConfigsError::Plots)?;

        let links =
            LinksConfig::load(&Self::active_links_path(&settings)).map_err(|err| err.to_string());

        let warnings = [
            unknown_keys_warning::<SettingsConfig>(path),
            unknown_keys_warning::<ThemeConfig>(&theme_path),
            unknown_keys_warning::<PlotsConfig>(&plots_path),
            unknown_keys_warning::<KeyBindingsConfig>(&keybindings_path),
        ];

        Ok(AppConfig {
            settings,
            theme,
            keybindings,
            highlights,
            triggers,
            plots,
            links,
            warnings: warnings.into_iter().flatten().collect(),
        })
    }

    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
    }

    /// Reads the settings from `path` instead of the default file, for the whole run: only
    /// the first call counts (MAX-206)
    pub fn use_config_file(path: PathBuf) {
        let _ = CONFIG_FILE_GIVEN.set(path);
    }

    /// The settings file in use: `--config`'s, else the default one
    pub fn config_path() -> PathBuf {
        (CONFIG_FILE_GIVEN.get().cloned()).unwrap_or_else(Self::default_config_path)
    }

    pub fn default_sessions_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|p| p.join(APP_DIR).join(SESSION_DIR))
    }

    pub fn default_theme_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(THEME_FILE))
            .unwrap_or_else(|| PathBuf::from(THEME_FILE))
    }

    pub fn default_keybindings_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(KEYMAP_FILE))
            .unwrap_or_else(|| PathBuf::from(KEYMAP_FILE))
    }

    pub fn default_highlights_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(HIGHLIGHT_FILE))
            .unwrap_or_else(|| PathBuf::from(HIGHLIGHT_FILE))
    }

    pub fn default_triggers_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(TRIGGERS_FILE))
            .unwrap_or_else(|| PathBuf::from(TRIGGERS_FILE))
    }

    pub fn default_links_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(LINKS_FILE))
            .unwrap_or_else(|| PathBuf::from(LINKS_FILE))
    }

    pub fn default_plots_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(PLOTS_FILE))
            .unwrap_or_else(|| PathBuf::from(PLOTS_FILE))
    }

    /// The folder of the config files
    pub fn config_dir() -> PathBuf {
        let path = Self::default_config_path();
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The config folder, then each config file put out of it (`--config`, a `*_path`
    /// setting), with what they are: what the help and the settings show and copy (MAX-163)
    pub fn config_paths(settings: &SettingsConfig) -> Vec<(&'static str, PathBuf)> {
        let dir = Self::config_dir();
        let files = [
            ("config file", Self::config_path()),
            ("theme file", Self::active_theme_path(settings)),
            ("keymap file", Self::active_keybindings_path(settings)),
            ("highlights file", Self::active_highlight_path(settings)),
            ("triggers file", Self::active_triggers_path(settings)),
            ("links file", Self::active_links_path(settings)),
            ("plots file", Self::active_plots_path(settings)),
        ];
        let moved = (files.into_iter()).filter(|(_, path)| path.parent() != Some(dir.as_path()));
        std::iter::once(("config folder", dir.clone()))
            .chain(moved)
            .collect()
    }

    /// A `*_path` setting, `~` the home folder as in a shell (MAX-163), else the default path
    pub fn active_theme_path(settings: &SettingsConfig) -> PathBuf {
        (settings.theme_path.as_deref()).map_or_else(Self::default_theme_path, expand_home)
    }

    pub fn active_keybindings_path(settings: &SettingsConfig) -> PathBuf {
        (settings.keybindings_path.as_deref())
            .map_or_else(Self::default_keybindings_path, expand_home)
    }

    pub fn active_highlight_path(settings: &SettingsConfig) -> PathBuf {
        (settings.highlights_path.as_deref())
            .map_or_else(Self::default_highlights_path, expand_home)
    }

    pub fn active_triggers_path(settings: &SettingsConfig) -> PathBuf {
        (settings.triggers_path.as_deref()).map_or_else(Self::default_triggers_path, expand_home)
    }

    pub fn active_links_path(settings: &SettingsConfig) -> PathBuf {
        (settings.links_path.as_deref()).map_or_else(Self::default_links_path, expand_home)
    }

    pub fn active_plots_path(settings: &SettingsConfig) -> PathBuf {
        (settings.plots_path.as_deref()).map_or_else(Self::default_plots_path, expand_home)
    }
}

#[derive(Debug)]
pub enum ConfigsError {
    Settings(SettingsError),
    Theme(ThemeError),
    Keymap(Box<KeymapFileError>),
    Highlight(HighlightError),
    Triggers(TriggersFileError),
    Plots(PlotsFileError),
}

impl std::fmt::Display for ConfigsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings(err) => write!(f, "{err}"),
            Self::Theme(err) => write!(f, "{err}"),
            Self::Keymap(err) => write!(f, "{err}"),
            Self::Highlight(err) => write!(f, "{err}"),
            Self::Triggers(err) => write!(f, "{err}"),
            Self::Plots(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ConfigsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Settings(err) => Some(err),
            Self::Theme(err) => Some(err),
            Self::Keymap(err) => Some(err),
            Self::Highlight(err) => Some(err),
            Self::Triggers(err) => Some(err),
            Self::Plots(err) => Some(err),
        }
    }
}

#[cfg(test)]
#[path = "tests/configs.rs"]
mod tests;
