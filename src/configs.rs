use std::path::{Path, PathBuf};

pub mod highlights;
pub mod keybindings;
pub mod settings;
pub mod style;
pub mod theme;

// Re-exports
pub use highlights::{HighlightConfig, HighlightError, KeywordHighlight};
pub use keybindings::{
    KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
    KeyBindingTerminalAction, KeyBindingsConfig, KeyPattern, KeyResolver, KeymapFileError,
};
pub use settings::{
    LineDisplayMode, LineLogConfig, Mode, PrefixWrapBehavior, RawSideView, RxNewlineMode,
    SettingsConfig, SettingsError, StatusBarConfig, TerminalDimensionMode, TerminalLogConfig,
    TextEncoding, WrapMode,
};
pub use theme::{ThemeConfig, ThemeError};

const APP_DIR: &str = "maxicom";
const CONFIG_FILE: &str = "config.toml";
const THEME_FILE: &str = "theme.toml";
const KEYMAP_FILE: &str = "keymap.toml";
const HIGHLIGHT_FILE: &str = "highlights.toml";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: SettingsConfig,
    pub theme: ThemeConfig,
    pub keybindings: KeyBindingsConfig,
    pub highlights: HighlightConfig,
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
            .map_err(ConfigsError::Keymap)?;

        let highlights_path = Self::active_highlight_path(&settings);
        let highlights = HighlightConfig::load_or_create_default(&highlights_path)
            .map_err(ConfigsError::Highlight)?;

        Ok(AppConfig {
            settings,
            theme,
            keybindings,
            highlights,
        })
    }

    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
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

    pub fn active_theme_path(settings: &SettingsConfig) -> PathBuf {
        settings
            .theme_path
            .clone()
            .unwrap_or_else(Self::default_theme_path)
    }

    pub fn active_keybindings_path(settings: &SettingsConfig) -> PathBuf {
        settings
            .keybindings_path
            .clone()
            .unwrap_or_else(Self::default_keybindings_path)
    }

    pub fn active_highlight_path(settings: &SettingsConfig) -> PathBuf {
        settings
            .highlights_path
            .clone()
            .unwrap_or_else(Self::default_highlights_path)
    }
}

#[derive(Debug)]
pub enum ConfigsError {
    Settings(SettingsError),
    Theme(ThemeError),
    Keymap(KeymapFileError),
    Highlight(HighlightError),
}

impl std::fmt::Display for ConfigsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings(err) => write!(f, "{err}"),
            Self::Theme(err) => write!(f, "{err}"),
            Self::Keymap(err) => write!(f, "{err}"),
            Self::Highlight(err) => write!(f, "{err}"),
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
        }
    }
}
