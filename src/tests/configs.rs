use super::*;

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
