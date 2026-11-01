use std::path::{Path, PathBuf};

use crate::capture::expand_home;

pub mod highlights;
pub mod keybindings;
pub mod settings;
pub mod style;
pub mod theme;
pub mod triggers;

// Re-exports
pub use highlights::{HighlightConfig, HighlightError, KeywordHighlight};
pub use keybindings::{
    KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
    KeyBindingTerminalAction, KeyBindingsConfig, KeyPattern, KeyResolver, KeymapFileError,
};
pub use settings::{
    CommandLineLayout, DualBufferMode, LineDisplayMode, Mode, PrefixWrapBehavior, QueryGroup,
    RawSideView, RxNewlineMode, SettingsConfig, SettingsError, StatusBarConfig, StatusBarLayout,
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

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: SettingsConfig,
    pub theme: ThemeConfig,
    pub keybindings: KeyBindingsConfig,
    pub highlights: HighlightConfig,
    pub triggers: TriggersConfig,
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

        let triggers_path = Self::active_triggers_path(&settings);
        let triggers = TriggersConfig::load_or_create_default(&triggers_path)
            .map_err(ConfigsError::Triggers)?;

        Ok(AppConfig {
            settings,
            theme,
            keybindings,
            highlights,
            triggers,
        })
    }

    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
    }

    pub fn default_journal_dir() -> Option<PathBuf> {
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

    /// The folder of the config files
    pub fn config_dir() -> PathBuf {
        let path = Self::default_config_path();
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The config folder, then each config file a `*_path` setting puts out of it, with what
    /// they are: what the help and the settings show and copy (MAX-163)
    pub fn config_paths(settings: &SettingsConfig) -> Vec<(&'static str, PathBuf)> {
        let dir = Self::config_dir();
        let files = [
            ("theme file", Self::active_theme_path(settings)),
            ("keymap file", Self::active_keybindings_path(settings)),
            ("highlights file", Self::active_highlight_path(settings)),
            ("triggers file", Self::active_triggers_path(settings)),
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
}

#[derive(Debug)]
pub enum ConfigsError {
    Settings(SettingsError),
    Theme(ThemeError),
    Keymap(KeymapFileError),
    Highlight(HighlightError),
    Triggers(TriggersFileError),
}

impl std::fmt::Display for ConfigsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings(err) => write!(f, "{err}"),
            Self::Theme(err) => write!(f, "{err}"),
            Self::Keymap(err) => write!(f, "{err}"),
            Self::Highlight(err) => write!(f, "{err}"),
            Self::Triggers(err) => write!(f, "{err}"),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MAX-163: the config folder comes first, then only the files a `*_path` setting puts
    /// out of it, in the theme, keymap, highlights, triggers order.
    /// How: the default settings, then a theme elsewhere, a keymap set inside the folder and
    /// triggers elsewhere.
    #[test]
    fn config_paths_list_the_folder_and_the_files_out_of_it() {
        let dir = AppConfig::config_dir();
        let settings = SettingsConfig::default();
        assert_eq!(
            AppConfig::config_paths(&settings),
            [("config folder", dir.clone())]
        );

        let mut settings = SettingsConfig::default();
        settings.theme_path = Some(PathBuf::from("/themes/dark.toml"));
        settings.keybindings_path = Some(dir.join("other_keymap.toml"));
        settings.triggers_path = Some(PathBuf::from("/boards/triggers.toml"));
        assert_eq!(
            AppConfig::config_paths(&settings),
            [
                ("config folder", dir),
                ("theme file", PathBuf::from("/themes/dark.toml")),
                ("triggers file", PathBuf::from("/boards/triggers.toml")),
            ]
        );
    }

    /// MAX-163: a `*_path` setting starting with `~` is in the home folder, not in a `~` folder
    /// of the working directory. Only the paths are computed, nothing is written.
    #[test]
    fn config_file_paths_expand_home() {
        let home = dirs::home_dir().unwrap();
        let mut settings = SettingsConfig::default();
        settings.theme_path = Some(PathBuf::from("~/maxicom_test/themes"));
        settings.keybindings_path = Some(PathBuf::from("~/maxicom_test/keybindings"));
        settings.highlights_path = Some(PathBuf::from("~/maxicom_test/highlights"));
        settings.triggers_path = Some(PathBuf::from("~/maxicom_test/triggers"));
        let dir = home.join("maxicom_test");
        assert_eq!(AppConfig::active_theme_path(&settings), dir.join("themes"));
        assert_eq!(
            AppConfig::active_keybindings_path(&settings),
            dir.join("keybindings")
        );
        assert_eq!(
            AppConfig::active_highlight_path(&settings),
            dir.join("highlights")
        );
        assert_eq!(
            AppConfig::active_triggers_path(&settings),
            dir.join("triggers")
        );
        assert_eq!(
            AppConfig::config_paths(&settings)[1],
            ("theme file", dir.join("themes"))
        );
    }
}
