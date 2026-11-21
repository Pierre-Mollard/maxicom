use super::*;
use std::fs;

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
/// How: the keys of a settings file given another name, then none.
#[test]
fn unknown_keys_warning_names_the_file() {
    let path = Path::new("/somewhere/board.toml");
    let (_, keys) = read_toml::<SettingsConfig>("mdoe = \"line\"\nkep = 1\n").unwrap();
    assert_eq!(
        unknown_keys_warning(path, &keys).as_deref(),
        Some("Unknown keys in board.toml: kep, mdoe")
    );
    assert_eq!(unknown_keys_warning(path, &[]), None);
}

/// MAX-248 step 0: the errors of a file that doesn't load, each file kind: a syntax error and
/// a value of the wrong type, with their line and column, as said today. A `links.toml` error
/// doesn't stop the load, it's kept.
/// How: each file broken in a folder alone, the error's text with the folder's path replaced.
#[test]
fn load_errors_keep_their_text() {
    let cases = [
        ("config.toml", "broken = \n"),
        ("config.toml", "rx_buffer_size = \"big\"\n"),
        ("theme.toml", "[palette]\nbroken = \n"),
        ("theme.toml", "[palette]\ntext = 5\n"),
        ("keymap.toml", "[prefixed]\nbroken = \n"),
        ("keymap.toml", "prefix = 5\n"),
        ("highlights.toml", "READY = \"Green\"\nbroken = \n"),
        ("highlights.toml", "READY = 5\n"),
        ("triggers.toml", "[r]\nbroken = \n"),
        ("triggers.toml", "[r]\nmatch = 5\nrun = \"send x\"\n"),
        ("plots.toml", "[temp]\nbroken = \n"),
        ("plots.toml", "[temp]\nseparator = 5\n"),
        ("links.toml", "[[link]]\nbroken = \n"),
        ("links.toml", "[[link]]\nname = 3\naddress = \"COM3\"\n"),
    ];
    let mut said = Vec::new();
    for (i, (file, text)) in cases.iter().enumerate() {
        let folder = Folder::new(&format!("load-errors-{i}"), "");
        if *file == "config.toml" {
            let settings = folder.read("config.toml");
            folder.write(file, &(settings + text));
        } else {
            folder.write(file, text);
        }
        let error = match folder.load() {
            Ok(config) => config.links.err().unwrap(),
            Err(err) => err.to_string(),
        };
        said.push(error.replace(&folder.path("").display().to_string(), "<dir>/"));
    }
    // the settings file starts with the folder's 6 paths; a trigger's bad value points at
    // its table, `[triggers]` being flattened
    let expected = [
        "invalid settings syntax in '<dir>/config.toml':\nTOML parse error at line 7, column 10\n  |\n7 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid settings syntax in '<dir>/config.toml':\nTOML parse error at line 7, column 18\n  |\n7 | rx_buffer_size = \"big\"\n  |                  ^^^^^\ninvalid type: string \"big\", expected usize\n",
        "invalid theme in '<dir>/theme.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid theme in '<dir>/theme.toml':\nTOML parse error at line 2, column 8\n  |\n2 | text = 5\n  |        ^\nFailed to parse Colors: data did not match any variant of untagged enum ColorFormat\n\n",
        "invalid syntax in keymap file '<dir>/keymap.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid syntax in keymap file '<dir>/keymap.toml':\nTOML parse error at line 1, column 10\n  |\n1 | prefix = 5\n  |          ^\ninvalid type: integer `5`, expected a string\n",
        "invalid highlight in '<dir>/highlights.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid highlight in '<dir>/highlights.toml':\nTOML parse error at line 1, column 9\n  |\n1 | READY = 5\n  |         ^\ndata did not match any variant of untagged enum EntryDef\n",
        "invalid syntax in '<dir>/triggers.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid syntax in '<dir>/triggers.toml':\nTOML parse error at line 1, column 1\n  |\n1 | [r]\n  | ^\ninvalid type: integer `5`, expected a string\n",
        "invalid syntax in '<dir>/plots.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid syntax in '<dir>/plots.toml':\nTOML parse error at line 2, column 13\n  |\n2 | separator = 5\n  |             ^\ninvalid type: integer `5`, expected a string\n",
        "invalid syntax in '<dir>/links.toml':\nTOML parse error at line 2, column 10\n  |\n2 | broken = \n  |          ^\nstring values must be quoted, expected literal string\n",
        "invalid syntax in '<dir>/links.toml':\nTOML parse error at line 2, column 8\n  |\n2 | name = 3\n  |        ^\ninvalid type: integer `3`, expected a string\n",
    ];
    assert_eq!(said.len(), expected.len());
    for (said, expected) in said.iter().zip(expected) {
        assert_eq!(said, expected);
    }
}

/// MAX-248 step 0: the keys a file doesn't read are named in one warning per file, settings,
/// theme, plots then keymap, sorted; the values around them still load. Highlights, triggers
/// and links aren't checked.
/// How: one or two unknown keys in each file, next to known ones, loaded.
#[test]
fn unknown_key_warnings_of_each_file() {
    let folder = Folder::new("unknown-each", "mdoe = \"line\"\nrx_buffer_size = 1024\n");
    folder.write(
        "theme.toml",
        "[palette]\ntext = \"Red\"\ntxet = \"Red\"\n[nope]\na = 1\n",
    );
    folder.write("plots.toml", "[temp]\nseparator = \"=\"\ncolour = 1\n");
    folder.write(
        "keymap.toml",
        "prefx = \"ctrl-b\"\n[prefixed]\nq = \"quit\"\n",
    );
    folder.write("highlights.toml", "READY = \"Green\"\n");
    let config = folder.load().unwrap();
    assert_eq!(
        config.warnings,
        [
            "Unknown keys in config.toml: mdoe",
            "Unknown keys in theme.toml: nope, palette.txet",
            "Unknown keys in plots.toml: temp.colour",
            "Unknown keys in keymap.toml: prefx",
        ]
    );
    assert_eq!(config.settings.rx_buffer_size, 1024);
    assert_eq!(config.theme.palette.text, ratatui::style::Color::Red);
    assert_eq!(config.plots.names(), ["temp"]);
    assert_eq!(config.keybindings.prefix_actions["q"], "quit");
}

/// MAX-248: the one read gives the values `toml::from_str` gives, and no unknown key, on each
/// file maxicom writes itself, comments included.
/// How: each config type's default written as maxicom writes it, read both ways.
#[test]
fn read_toml_reads_as_from_str() {
    fn same<T>(value: &T, comments: &[(&str, &str)])
    where
        T: serde::Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let text = toml::to_string_pretty(value).unwrap();
        let text = comments::with_comments(&text, comments);
        let plain: T = toml::from_str(&text).unwrap();
        assert_eq!(read_toml::<T>(&text).unwrap(), (plain, Vec::new()));
    }
    same(&SettingsConfig::default(), comments::CONFIG_COMMENTS);
    same(&ThemeConfig::default(), comments::THEME_COMMENTS);
    same(&KeyBindingsConfig::default(), comments::KEYMAP_COMMENTS);
    same(&HighlightConfig::default(), comments::HIGHLIGHTS_COMMENTS);
    same(&TriggersConfig::default(), comments::TRIGGERS_COMMENTS);
    same(&PlotsConfig::default(), comments::PLOTS_COMMENTS);
}
