//! `--write-config`: the config files brought up to date in place.

use super::*;
use crate::configs::{KeyBindingsConfig, SettingsConfig};

/// A temporary folder with a settings file pointing every other file into it
struct Folder(PathBuf);

impl Folder {
    fn new(test: &str, settings: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("maxicom-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut text = String::new();
        for (key, file) in [
            ("theme_path", "theme.toml"),
            ("keybindings_path", "keymap.toml"),
            ("highlights_path", "highlights.toml"),
            ("triggers_path", "triggers.toml"),
            ("links_path", "links.toml"),
        ] {
            text += &format!("{key} = {:?}\n", dir.join(file).display().to_string());
        }
        fs::write(dir.join("config.toml"), text + settings).unwrap();
        Self(dir)
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.path(file)).unwrap()
    }

    fn update(&self) -> Result<Vec<String>, String> {
        update_files(&self.path("config.toml"))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// MAX-209: an old settings file keeps its values, gets the keys it lacks and the comments, and
/// its old version stays as `.bak`; an unknown key is dropped and named.
/// How: a short file with a changed value and a typo, updated, read back.
#[test]
fn update_keeps_values_and_adds_keys_and_comments() {
    let folder = Folder::new("update-values", "mdoe = \"line\"\nrx_buffer_size = 1024\n");
    let old = folder.read("config.toml");

    let report = folder.update().unwrap().join("\n");
    assert!(report.contains("unknown keys dropped: mdoe"), "{report}");

    let new = folder.read("config.toml");
    assert!(
        new.contains("rx_buffer_size = 1024 # 512 to 65536\n"),
        "{new}"
    );
    assert!(
        new.contains("# values: pretty, clean, raw, hex, octal, binary\n"),
        "{new}"
    );
    assert!(!new.contains("mdoe"), "{new}");
    assert_eq!(folder.read("config.toml.bak"), old);

    let settings: SettingsConfig = toml::from_str(&new).unwrap();
    assert_eq!(settings.rx_buffer_size, 1024);
    assert_eq!(settings.line_mode, SettingsConfig::default().line_mode);
}

/// MAX-209: a second update changes nothing and keeps the `.bak` of the first, the original.
/// How: two updates in a row on an old file.
#[test]
fn update_twice_changes_nothing() {
    let folder = Folder::new("update-twice", "rx_buffer_size = 1024\n");
    let old = folder.read("config.toml");
    folder.update().unwrap();
    let new = folder.read("config.toml");

    let report = folder.update().unwrap();
    assert_eq!(
        report
            .iter()
            .filter(|l| l.ends_with(": up to date"))
            .count(),
        5,
        "{report:?}"
    );
    assert_eq!(folder.read("config.toml"), new);
    assert_eq!(folder.read("config.toml.bak"), old);
}

/// MAX-209: a file that doesn't load stops the update before anything is written.
/// How: valid settings and a broken theme, whose settings stay as they were, with no `.bak`.
#[test]
fn update_stops_on_a_file_that_doesnt_load() {
    let folder = Folder::new("update-invalid", "rx_buffer_size = 1024\n");
    fs::write(folder.path("theme.toml"), "[palette\n").unwrap();
    let old = folder.read("config.toml");

    assert!(folder.update().is_err());
    assert_eq!(folder.read("config.toml"), old);
    assert!(!folder.path("config.toml.bak").exists());
    assert_eq!(folder.read("theme.toml"), "[palette\n");
}

/// MAX-209: a keymap from before a new default gets it, said in the report. A key changed on
/// purpose stays, an action still bound elsewhere in the section isn't added again, and a
/// default whose key and action are both gone comes back.
/// How: a default keymap without the mark keys, `Ctrl-a q` moved to `help`, `Ctrl-a ?` removed
/// with `help` still on `z`, and `quit` removed from the section, updated and read back.
#[test]
fn update_adds_new_default_keys() {
    let folder = Folder::new("update-keymap", "");
    let mut keymap = KeyBindingsConfig::default();
    for key in [",", "'", "(", ")"] {
        keymap.prefix_actions.remove(key);
    }
    keymap.prefix_actions.insert("q".into(), "help".into());
    keymap.prefix_actions.remove("x");
    keymap.prefix_actions.remove("?");
    fs::write(
        folder.path("keymap.toml"),
        toml::to_string_pretty(&keymap).unwrap(),
    )
    .unwrap();

    let report = folder.update().unwrap().join("\n");
    assert!(report.contains("new keys added: "), "{report}");

    let new: KeyBindingsConfig = toml::from_str(&folder.read("keymap.toml")).unwrap();
    assert_eq!(new.prefix_actions[","], "mark");
    assert_eq!(new.prefix_actions["q"], "help");
    assert_eq!(new.prefix_actions["x"], "quit");
    assert!(!new.prefix_actions.contains_key("?"));
}
