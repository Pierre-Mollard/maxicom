//! Profiles (MAX-240): a file of settings laid over the global config while a link uses it,
//! `profiles/<name>.toml`. It holds a description, keys and named commands in the sections of
//! `keymap.toml`, and highlights, triggers and plots under `[highlights]`, `[triggers]` and
//! `[plots]` in the formats of their files; all optional. No line settings: those are the
//! saved link's. A board's or a project's knowledge lives in profiles, never in the code. A
//! stack of profiles is laid in order over the global config, a later one winning, and
//! rebuilt from the global config on each change, never patched

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{
    AppConfig, HighlightConfig, KeyBindingsConfig, KeyPattern, KeyResolver, PlotsConfig,
    TriggersConfig, keybindings::NamedCommandConfig, unknown_keys,
};

/// A profile file as written. Its keymap sections are empty unless written, unlike the
/// global keymap's, which take their defaults. `enabled = false` turns it off: never laid,
/// a stack naming it warned about, its file not checked past its syntax
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProfileFile {
    pub description: String,
    pub enabled: bool,
    pub prefixed: BTreeMap<String, String>,
    pub direct: BTreeMap<String, String>,
    pub line_mode: BTreeMap<String, String>,
    pub terminal_mode: BTreeMap<String, String>,
    pub search_mode: BTreeMap<String, String>,
    pub copy_mode: BTreeMap<String, String>,
    pub dialog: BTreeMap<String, String>,
    pub commands: BTreeMap<String, NamedCommandConfig>,
    pub highlights: Option<HighlightConfig>,
    pub triggers: Option<TriggersConfig>,
    pub plots: Option<PlotsConfig>,
}

/// On, and empty
impl Default for ProfileFile {
    fn default() -> Self {
        Self {
            description: String::new(),
            enabled: true,
            prefixed: BTreeMap::new(),
            direct: BTreeMap::new(),
            line_mode: BTreeMap::new(),
            terminal_mode: BTreeMap::new(),
            search_mode: BTreeMap::new(),
            copy_mode: BTreeMap::new(),
            dialog: BTreeMap::new(),
            commands: BTreeMap::new(),
            highlights: None,
            triggers: None,
            plots: None,
        }
    }
}

/// A profile as read from its file, with the unknown keys found in it
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub path: PathBuf,
    pub file: ProfileFile,
    pub warnings: Vec<String>,
}

/// Why a profile doesn't load, or doesn't lay over the config: the message names its file
#[derive(Debug)]
pub struct ProfileError {
    pub name: String,
    pub path: PathBuf,
    pub reason: String,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (name, path, reason) = (&self.name, self.path.display(), &self.reason);
        write!(f, "profile '{name}' ({path}): {reason}")
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// The profile `name`, `<name>.toml` in the folder `profiles`, checked as the global
    /// files are: its triggers compile, its plots read numbers
    pub fn load(profiles: &Path, name: &str) -> Result<Self, ProfileError> {
        let path = file_of(profiles, name);
        let error = |reason: String| ProfileError {
            name: name.to_string(),
            path: path.clone(),
            reason,
        };
        if !path.exists() {
            return Err(error("no such file".to_string()));
        }
        let text = fs::read_to_string(&path).map_err(|err| error(err.to_string()))?;
        let file: ProfileFile =
            toml::from_str(&text).map_err(|err| error(format!("invalid syntax:\n{err}")))?;
        // off: nothing of it is used, so nothing more to check
        if !file.enabled {
            return Ok(Self {
                name: name.to_string(),
                path,
                file,
                warnings: Vec::new(),
            });
        }
        if let Some(triggers) = &file.triggers {
            triggers.compile().map_err(|err| error(err.to_string()))?;
        }
        if let Some(plots) = &file.plots {
            plots.check().map_err(|err| error(err.to_string()))?;
        }
        let unknown = unknown_keys::<ProfileFile>(&text);
        let warnings = (!unknown.is_empty())
            .then(|| format!("Unknown keys in profile '{name}': {}", unknown.join(", ")))
            .into_iter()
            .collect();
        Ok(Self {
            name: name.to_string(),
            path,
            file,
            warnings,
        })
    }
}

/// The profiles maxicom comes with, as their files: written when the profiles folder is
/// created, by `--write-config` when one is missing and by `--reset-config`. Board names
/// live here only, never in the code
pub const BUILT_IN: [(&str, &str); 2] = [
    ("arduino", include_str!("../../profiles/arduino.toml")),
    ("esp32", include_str!("../../profiles/esp32.toml")),
];

/// The built-in profiles written in the folder `dir` when it doesn't exist yet, as the
/// other config files are on a first start: once there, a profile deleted stays deleted
pub fn write_folder(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        write_missing(dir)?;
    }
    Ok(())
}

/// The built-in profiles missing from the folder `dir` written in it, the folder created,
/// their files returned: `--write-config`'s, for one added since the folder was
pub fn write_missing(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (name, text) in BUILT_IN {
        let path = file_of(dir, name);
        if !path.exists() {
            fs::create_dir_all(dir)?;
            fs::write(&path, text)?;
            written.push(path);
        }
    }
    Ok(written)
}

/// The file of the profile `name` in the folder `dir`
pub fn file_of(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.toml"))
}

/// The profiles of `names` with no file in the folder `dir`, for a link saved with them
pub fn missing<'a>(dir: &Path, names: &'a [String]) -> Vec<&'a str> {
    (names.iter())
        .filter(|name| !file_of(dir, name).exists())
        .map(String::as_str)
        .collect()
}

/// `config` with `profiles` laid over it in order, a later one winning (MAX-240):
/// - keys replace the same key of their section, named commands the same name;
/// - highlights come after the config's, drawn over them, one of the same name replacing it;
/// - triggers and plots replace the same name, a plot keeping its place, others added.
///
/// The keymap of each step is checked as the global one: an error names the profile's file
pub fn layered(config: &AppConfig, profiles: &[Profile]) -> Result<AppConfig, ProfileError> {
    let mut config = config.clone();
    for profile in profiles {
        let error = |reason: String| ProfileError {
            name: profile.name.clone(),
            path: profile.path.clone(),
            reason,
        };
        let file = &profile.file;
        lay_keymap(&mut config.keybindings, file);
        KeyResolver::try_new(&config.keybindings).map_err(|err| error(err.to_string()))?;
        if let Some(highlights) = &file.highlights {
            let entries = &mut config.highlights.entries;
            entries.retain(|(name, _)| highlights.entries.iter().all(|(own, _)| own != name));
            entries.extend(highlights.entries.iter().cloned());
        }
        if let Some(triggers) = &file.triggers {
            let rules = triggers.rules.iter();
            config
                .triggers
                .rules
                .extend(rules.map(|(k, v)| (k.clone(), v.clone())));
        }
        if let Some(plots) = &file.plots {
            for (name, plot) in &plots.plots {
                let own = config.plots.plots.iter_mut().find(|(own, _)| own == name);
                match own {
                    Some((_, place)) => *place = plot.clone(),
                    None => config.plots.plots.push((name.clone(), plot.clone())),
                }
            }
        }
        config.warnings.extend(profile.warnings.iter().cloned());
    }
    Ok(config)
}

/// `config` with the profiles `names` of the folder `dir` laid over it in order: the config
/// a link with these profiles runs with. A profile turned off (`enabled = false`) is left
/// out, a warning saying so
pub fn with_profiles(
    config: &AppConfig,
    dir: &Path,
    names: &[String],
) -> Result<AppConfig, ProfileError> {
    let profiles: Result<Vec<_>, _> = names.iter().map(|name| Profile::load(dir, name)).collect();
    let (on, off): (Vec<_>, Vec<_>) = profiles?.into_iter().partition(|p| p.file.enabled);
    let mut laid = layered(config, &on)?;
    let off = off.iter().map(|profile| {
        let name = &profile.name;
        format!("Profile '{name}' is off (enabled = false in its file): not laid")
    });
    laid.warnings.extend(off);
    Ok(laid)
}

/// `file`'s keys over `keymap`'s, section by section: a key replaces the same key however
/// it's written (`F6`, `f6`), and a named command the one of the same name
fn lay_keymap(keymap: &mut KeyBindingsConfig, file: &ProfileFile) {
    let sections = [
        (&mut keymap.prefix_actions, &file.prefixed),
        (&mut keymap.direct_actions, &file.direct),
        (&mut keymap.line_mode, &file.line_mode),
        (&mut keymap.terminal_mode, &file.terminal_mode),
        (&mut keymap.search_mode, &file.search_mode),
        (&mut keymap.copy_mode, &file.copy_mode),
        (&mut keymap.dialog_actions, &file.dialog),
    ];
    for (section, keys) in sections {
        for (key, value) in keys {
            let pattern = key.parse::<KeyPattern>().ok();
            section.retain(|own, _| {
                own != key && (pattern.is_none() || own.parse::<KeyPattern>().ok() != pattern)
            });
            section.insert(key.clone(), value.clone());
        }
    }
    let commands = file.commands.iter();
    keymap
        .commands
        .extend(commands.map(|(k, v)| (k.clone(), v.clone())));
}

#[cfg(test)]
#[path = "../tests/configs/profiles.rs"]
mod tests;
