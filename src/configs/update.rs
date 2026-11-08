//! `--write-config` (MAX-209): brings the config files up to date in place, keeping their
//! values: each is loaded, serialized with the keys it lacks, and written with its comments.
//! `links.toml` isn't one of them, the app writes it itself.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};

use super::{
    AppConfig,
    comments::{
        CONFIG_COMMENTS, HIGHLIGHTS_COMMENTS, KEYMAP_COMMENTS, THEME_COMMENTS, TRIGGERS_COMMENTS,
        with_comments,
    },
    unknown_keys,
};

/// A file as it is, as it would be, and what to say about the change
struct Update {
    path: PathBuf,
    old: String,
    new: String,
    notes: Vec<String>,
}

/// The update of the file at `path`, holding `value`, the file loaded. Comments must leave the
/// values as they are, or nothing is written
fn update_of<T: Serialize + DeserializeOwned>(
    path: PathBuf,
    value: &T,
    comments: &[(&str, &str)],
) -> Result<Update, String> {
    let failed = |err: &dyn std::fmt::Display| format!("{}: {err}", path.display());
    let old = fs::read_to_string(&path).map_err(|err| failed(&err))?;
    let text = toml::to_string_pretty(value).map_err(|err| failed(&err))?;
    let new = with_comments(&text, comments);
    let same_values = toml::from_str::<toml::Table>(&new).ok() == toml::from_str(&text).ok();
    if !same_values {
        return Err(failed(&"the comments would change its values"));
    }
    let unknown = unknown_keys::<T>(&old);
    let notes = (!unknown.is_empty())
        .then(|| format!("unknown keys dropped: {}", unknown.join(", ")))
        .into_iter()
        .collect();
    Ok(Update {
        path,
        old,
        new,
        notes,
    })
}

/// Brings the files of the settings at `path` up to date, what happened to each in the lines
/// returned. A file that doesn't load stops it before anything is updated, and the old version
/// of an updated file stays next to it, as `.bak`
pub fn update_files(path: &Path) -> Result<Vec<String>, String> {
    let mut config = AppConfig::load_or_create_default(path).map_err(|err| err.to_string())?;
    let added = config.keybindings.add_new_defaults();
    let settings = &config.settings;

    let mut updates = [
        update_of(path.to_path_buf(), settings, CONFIG_COMMENTS)?,
        update_of(
            AppConfig::active_theme_path(settings),
            &config.theme,
            THEME_COMMENTS,
        )?,
        update_of(
            AppConfig::active_keybindings_path(settings),
            &config.keybindings,
            KEYMAP_COMMENTS,
        )?,
        update_of(
            AppConfig::active_highlight_path(settings),
            &config.highlights,
            HIGHLIGHTS_COMMENTS,
        )?,
        update_of(
            AppConfig::active_triggers_path(settings),
            &config.triggers,
            TRIGGERS_COMMENTS,
        )?,
    ];
    if !added.is_empty() {
        updates[2]
            .notes
            .push(format!("new keys added: {}", added.join(", ")));
    }

    let mut report = Vec::new();
    for update in updates {
        let file = update.path.display();
        if update.old == update.new {
            report.push(format!("{file}: up to date"));
            continue;
        }
        let name = (update.path.file_name()).map_or_else(Default::default, |n| n.to_string_lossy());
        let backup = update.path.with_file_name(format!("{name}.bak"));
        let failed = |err: std::io::Error| format!("{file}: {err}");
        fs::copy(&update.path, &backup).map_err(failed)?;
        fs::write(&update.path, &update.new).map_err(failed)?;
        report.push(format!(
            "{file}: updated, the old file is {}",
            backup.display()
        ));
        report.extend(update.notes.iter().map(|note| format!("  {note}")));
    }
    Ok(report)
}

#[cfg(test)]
#[path = "../tests/configs/update.rs"]
mod tests;
