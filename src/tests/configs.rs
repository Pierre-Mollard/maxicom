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
