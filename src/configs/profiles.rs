//! Profiles (MAX-240): a file of settings laid over the global config while a link uses it,
//! `profiles/<name>.toml`. It holds a description, line settings in `[link]`, keys and named
//! commands in the sections of `keymap.toml`, and highlights, triggers and plots under
//! `[highlights]`, `[triggers]` and `[plots]` in the formats of their files; all optional. A
//! board's or a project's knowledge lives in profiles, never in the code. A stack of profiles
//! is laid in order over the global config, a later one winning, and rebuilt from the global
//! config on each change, never patched

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
use crate::link::serial::{FlowControl, Parity};

/// A profile's `[link]`: the line settings it gives a link, under the link's own
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct LinkLayer {
    pub baud: Option<u32>,
    pub bits: Option<u8>,
    pub parity: Option<Parity>,
    pub stop_bits: Option<u8>,
    pub flow: Option<FlowControl>,
}

/// A profile file as written. Its keymap sections are empty unless written, unlike the
/// global keymap's, which take their defaults
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProfileFile {
    pub description: String,
    pub link: LinkLayer,
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
        let path = profiles.join(format!("{name}.toml"));
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

/// `config` with `profiles` laid over it in order, a later one winning (MAX-240):
/// - keys replace the same key of their section, named commands the same name;
/// - highlights come after the config's, drawn over them, one of the same name replacing it;
/// - triggers and plots replace the same name, a plot keeping its place, others added;
/// - the line settings replace `[link]`'s, under a saved link's own.
///
/// The keymap and the line settings of each step are checked as the global ones: an error
/// names the profile's file
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
        let (line, link) = (&mut config.settings.link, &file.link);
        line.baud = link.baud.unwrap_or(line.baud);
        line.bits = link.bits.unwrap_or(line.bits);
        line.parity = link.parity.unwrap_or(line.parity);
        line.stop_bits = link.stop_bits.unwrap_or(line.stop_bits);
        line.flow = link.flow.unwrap_or(line.flow);
        line.validate()
            .map_err(|err| error(format!("[link]: {err}")))?;
        config.warnings.extend(profile.warnings.iter().cloned());
    }
    Ok(config)
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
