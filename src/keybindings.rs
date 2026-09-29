use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindingsConfig {
    pub prefix: String,

    pub direct: BTreeMap<String, String>,
    pub prefix_actions: BTreeMap<String, String>,
    pub copy_mode: BTreeMap<String, String>,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        let mut direct = BTreeMap::new();
        direct.insert("ctrl-q".to_string(), "quit".to_string());
        direct.insert("pageup".to_string(), "scroll_page_up".to_string());
        direct.insert("pagedown".to_string(), "scroll_page_down".to_string());

        let mut prefix_actions = BTreeMap::new();
        prefix_actions.insert("z".to_string(), "help".to_string());
        prefix_actions.insert("q".to_string(), "quit".to_string());
        prefix_actions.insert("x".to_string(), "quit".to_string());
        prefix_actions.insert("s".to_string(), "settings".to_string());
        prefix_actions.insert("l".to_string(), "toggle_logging".to_string());
        prefix_actions.insert("[".to_string(), "enter_copy_mode".to_string());
        prefix_actions.insert("/".to_string(), "enter_search".to_string());
        prefix_actions.insert("r".to_string(), "auto_resize_remote".to_string());
        prefix_actions.insert("b".to_string(), "send_break".to_string());

        let mut copy_mode = BTreeMap::new();
        copy_mode.insert("q".to_string(), "exit".to_string());
        copy_mode.insert("esc".to_string(), "exit".to_string());
        copy_mode.insert("v".to_string(), "toggle_selection".to_string());
        copy_mode.insert("y".to_string(), "yank".to_string());
        copy_mode.insert("h".to_string(), "move_left".to_string());
        copy_mode.insert("left".to_string(), "move_left".to_string());
        copy_mode.insert("j".to_string(), "move_down".to_string());
        copy_mode.insert("down".to_string(), "move_down".to_string());
        copy_mode.insert("k".to_string(), "move_up".to_string());
        copy_mode.insert("up".to_string(), "move_up".to_string());
        copy_mode.insert("l".to_string(), "move_right".to_string());
        copy_mode.insert("right".to_string(), "move_right".to_string());
        copy_mode.insert("0".to_string(), "start_of_line".to_string());
        copy_mode.insert("$".to_string(), "end_of_line".to_string());

        Self {
            prefix: "ctrl-a".to_string(),
            direct,
            prefix_actions,
            copy_mode,
        }
    }
}

impl KeyBindingsConfig {
    pub fn load_or_create_default<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path)?;
            let keymap: KeyBindingsConfig = toml::from_str(&content)?;
            return Ok(keymap);
        }

        let default_keymap = KeyBindingsConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_string = toml::to_string_pretty(&default_keymap)?;
        fs::write(path, toml_string)?;

        Ok(default_keymap)
    }
}
