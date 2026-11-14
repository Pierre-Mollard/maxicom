use super::*;

/// A temporary folder with a settings file pointing every other file into it, so a test
/// never reads nor writes the real config folder
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(test: &str, settings: &str) -> Self {
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
            ("plots_path", "plots.toml"),
        ] {
            text += &format!("{key} = {:?}\n", dir.join(file).display().to_string());
        }
        fs::write(dir.join("config.toml"), text + settings).unwrap();
        Self(dir)
    }

    pub fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }

    pub fn read(&self, file: &str) -> String {
        fs::read_to_string(self.path(file)).unwrap()
    }

    pub fn write(&self, file: &str, text: &str) {
        fs::write(self.path(file), text).unwrap();
    }

    pub fn load(&self) -> Result<AppConfig, ConfigsError> {
        AppConfig::load_or_create_default(self.path("config.toml"))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// MAX-240 step 1: each config file loads from the path its setting gives, as written.
/// How: one change in each file of a folder, loaded.
#[test]
fn app_config_loads_each_file_from_its_path() {
    let folder = Folder::new("app-config-files", "rx_buffer_size = 1024\n");
    folder.write(
        "keymap.toml",
        concat!(
            "prefix = \"ctrl-b\"\n",
            "[prefixed]\nq = \"quit\"\nh = \"ping\"\n",
            "[commands]\nping = { run = \"send AT\" }\n",
        ),
    );
    folder.write("highlights.toml", "READY = \"Green\"\n");
    folder.write(
        "triggers.toml",
        "[login]\nmatch = \"login: \"\nrun = 'send root\\r'\n",
    );
    folder.write("plots.toml", "[temp]\nseparator = \"=\"\n");
    folder.write(
        "links.toml",
        "[[link]]\nname = \"board\"\naddress = \"/dev/ttyUSB0\"\n",
    );

    let config = folder.load().unwrap();
    assert_eq!(config.settings.rx_buffer_size, 1024);
    assert_eq!(config.keybindings.prefix, "ctrl-b");
    assert_eq!(config.keybindings.prefix_actions.len(), 2);
    assert_eq!(config.keybindings.commands["ping"].run, "send AT");
    let words: Vec<_> = config
        .highlights
        .entries
        .iter()
        .map(|(w, _)| w.as_str())
        .collect();
    assert_eq!(words, ["READY"]);
    let triggers: Vec<_> = config.triggers.rules.keys().map(String::as_str).collect();
    assert_eq!(triggers, ["login"]);
    assert_eq!(config.plots.names(), ["temp"]);
    let links = config.links.unwrap();
    assert_eq!(links.links[0].name, "board");
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
}

/// MAX-239: a keymap section under its old name, or a misspelled key of a named command,
/// is said in a warning naming the keymap file, instead of being skipped without a word.
/// How: `[prefix_actions]` and `[commands]` with `hepl`.
#[test]
fn keymap_unknown_keys_warned() {
    let folder = Folder::new("keymap-unknown", "");
    folder.write(
        "keymap.toml",
        "[prefix_actions]\nq = \"quit\"\n[commands]\nping = { run = \"send AT\", hepl = \"\" }\n",
    );
    let config = folder.load().unwrap();
    assert_eq!(
        config.warnings,
        ["Unknown keys in keymap.toml: commands.ping.hepl, prefix_actions"]
    );
}

/// MAX-240 step 1: a file that isn't there is written with its defaults, which load; the
/// links file isn't written, and no links load.
/// How: a folder with only the settings file.
#[test]
fn app_config_writes_the_missing_files() {
    let folder = Folder::new("app-config-missing", "");
    let config = folder.load().unwrap();
    for file in [
        "theme.toml",
        "keymap.toml",
        "highlights.toml",
        "triggers.toml",
        "plots.toml",
    ] {
        assert!(folder.path(file).exists(), "{file}");
    }
    assert!(!folder.path("links.toml").exists());
    assert_eq!(config.keybindings, KeyBindingsConfig::default());
    assert_eq!(config.plots, PlotsConfig::default());
    assert_eq!(config.links.unwrap().links, []);
}

/// MAX-240 step 1: a file that doesn't load stops the whole config, its error naming it; a
/// `links.toml` that doesn't load doesn't, its error kept to be said in the app.
/// How: a bad trigger, then a bad key, then a bad link.
#[test]
fn app_config_error_names_the_file() {
    let folder = Folder::new("app-config-error", "");
    folder.write(
        "triggers.toml",
        "[login]\nmatch = \"login: \"\nrun = \"rst\"\n",
    );
    let error = folder.load().err().unwrap().to_string();
    assert!(error.contains("triggers.toml"), "{error}");

    folder.write("triggers.toml", "");
    folder.write("keymap.toml", "[prefixed]\nh = \"nope\"\n");
    let error = folder.load().err().unwrap().to_string();
    assert!(error.contains("keymap.toml"), "{error}");
    assert!(error.contains("unknown action 'nope'"), "{error}");

    folder.write("keymap.toml", "");
    folder.write("links.toml", "[[link]]\nname = 3\n");
    assert!(folder.load().unwrap().links.is_err());
}

/// MAX-163: the config folder comes first, then only the files a `*_path` setting puts
/// out of it, in the theme, keymap, highlights, triggers, links order.
/// How: the default settings, then a theme elsewhere, a keymap set inside the folder,
/// triggers and links elsewhere.
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
    settings.links_path = Some(PathBuf::from("/share/links.toml"));
    assert_eq!(
        AppConfig::config_paths(&settings),
        [
            ("config folder", dir),
            ("theme file", PathBuf::from("/themes/dark.toml")),
            ("triggers file", PathBuf::from("/boards/triggers.toml")),
            ("links file", PathBuf::from("/share/links.toml")),
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
    settings.links_path = Some(PathBuf::from("~/maxicom_test/links"));
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
    assert_eq!(AppConfig::active_links_path(&settings), dir.join("links"));
    assert_eq!(
        AppConfig::config_paths(&settings)[1],
        ("theme file", dir.join("themes"))
    );
}

/// MAX-209: a typo, an unknown nested key, an unknown section and the theme keys renamed since
/// 0.1.0 are named, while the optional `*_path` keys and a style's fields aren't.
/// How: settings and a theme holding both kinds, read for their unknown keys.
#[test]
fn unknown_keys_named_and_known_ones_not() {
    let settings = "mdoe = \"line\"\ntheme_path = \"t.toml\"\nmode = \"line\"\n\
        [line_mode]\nhistory_sise = 5\n[nope]\na = 1\n";
    assert_eq!(
        unknown_keys::<SettingsConfig>(settings),
        ["line_mode.history_sise", "mdoe", "nope"]
    );

    let theme = "[ui]\nselection = { fg = \"Black\", bold = true }\n\
        [terminal]\nterm_flash_fg_color = \"Red\"\n\
        [session_browser]\nmark_restorable = \"Red\"\n";
    assert_eq!(
        unknown_keys::<ThemeConfig>(theme),
        [
            "session_browser.mark_restorable",
            "terminal.term_flash_fg_color"
        ]
    );
}

/// MAX-209: the files maxicom writes itself have no unknown key, and a file that doesn't load
/// isn't reported twice.
/// How: the generated settings and theme with their comments, then two invalid files.
#[test]
fn unknown_keys_none_in_generated_files() {
    let text = toml::to_string_pretty(&SettingsConfig::default()).unwrap();
    let text = comments::with_comments(&text, comments::CONFIG_COMMENTS);
    assert!(unknown_keys::<SettingsConfig>(&text).is_empty());
    let text = toml::to_string_pretty(&ThemeConfig::default()).unwrap();
    let text = comments::with_comments(&text, comments::THEME_COMMENTS);
    assert!(unknown_keys::<ThemeConfig>(&text).is_empty());
    let text = toml::to_string_pretty(&KeyBindingsConfig::default()).unwrap();
    let text = comments::with_comments(&text, comments::KEYMAP_COMMENTS);
    assert!(unknown_keys::<KeyBindingsConfig>(&text).is_empty());

    assert!(unknown_keys::<SettingsConfig>("mdoe = ").is_empty());
    assert!(unknown_keys::<SettingsConfig>("mdoe = 1\nmode = \"zzz\"\n").is_empty());
}

/// MAX-209: the warning names the file as it's really called, and says nothing without an
/// unknown key.
/// How: a settings file given another name in a temporary folder, then a clean one.
#[test]
fn unknown_keys_warning_names_the_file() {
    let dir = std::env::temp_dir().join(format!("maxicom-unknown-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.toml");

    fs::write(&path, "mdoe = \"line\"\nkep = 1\n").unwrap();
    assert_eq!(
        unknown_keys_warning::<SettingsConfig>(&path).as_deref(),
        Some("Unknown keys in board.toml: kep, mdoe")
    );
    fs::write(&path, "mode = \"line\"\n").unwrap();
    assert_eq!(unknown_keys_warning::<SettingsConfig>(&path), None);

    fs::remove_dir_all(&dir).unwrap();
}
