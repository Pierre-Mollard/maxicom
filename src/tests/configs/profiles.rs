//! Profiles (MAX-240): a file of settings laid over the global config.

use super::*;
use crate::configs::tests::Folder;

/// `text` as the profile `name` of `folder`'s `profiles` folder
fn write(folder: &Folder, name: &str, text: &str) {
    let dir = folder.path("profiles");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.toml")), text).unwrap();
}

fn load(folder: &Folder, name: &str) -> Result<Profile, ProfileError> {
    Profile::load(&folder.path("profiles"), name)
}

/// MAX-240 step 2: a profile file holds only what it writes: no keys, no highlights, triggers
/// nor plots unless written; a name with no file isn't found, the file named.
/// How: a profile with a description and highlights only, then a name with no file.
#[test]
fn profile_holds_what_it_writes() {
    let folder = Folder::new("profile-file", "");
    write(
        &folder,
        "board",
        "description = \"A board\"\n[highlights]\nREADY = \"Green\"\n",
    );
    let profile = load(&folder, "board").unwrap();
    assert_eq!(profile.name, "board");
    let file = &profile.file;
    assert_eq!(file.description, "A board");
    assert_eq!(file.highlights.as_ref().unwrap().entries.len(), 1);
    assert!(file.triggers.is_none() && file.plots.is_none());
    assert!(file.prefixed.is_empty() && file.commands.is_empty());
    assert!(file.enabled, "on unless written");

    let error = load(&folder, "nope").err().unwrap().to_string();
    let path = folder.path("profiles").join("nope.toml");
    assert_eq!(
        error,
        format!("profile 'nope' ({}): no such file", path.display())
    );
}

/// MAX-240 step 2: no profile changes nothing, and laying profiles leaves the global config
/// as it was, for the next rebuild.
/// How: the default config, with no profile, then with one.
#[test]
fn global_config_stays() {
    let folder = Folder::new("profile-global", "");
    let config = folder.load().unwrap();
    let same = layered(&config, &[]).unwrap();
    assert_eq!(format!("{same:?}"), format!("{config:?}"));

    write(&folder, "board", "[highlights]\nREADY = \"Green\"\n");
    let laid = layered(&config, &[load(&folder, "board").unwrap()]).unwrap();
    let global = config.highlights.entries.len();
    assert_eq!(laid.highlights.entries.len(), global + 1);
    assert_eq!(folder.load().unwrap().highlights.entries.len(), global);
    assert_eq!(config.highlights.entries.len(), global);
}

/// MAX-240 step 2: a profile's keys replace the same key of their section however it's
/// written, its named commands the ones of the same name, the other keys stay; the keys
/// resolve.
/// How: `h` on the profile's own `board-reset`, `F6` and copy mode `x` on a new command,
/// `Shift-PageUp` for the default `shift-pageup`.
#[test]
fn profile_keymap_over_the_global_one() {
    let folder = Folder::new("profile-keymap", "");
    write(
        &folder,
        "board",
        concat!(
            "[prefixed]\nF6 = \"boot\"\n",
            "[direct]\n\"Shift-PageUp\" = \"scroll-to-top\"\n",
            "[copy_mode]\nx = \"boot\"\n",
            "[commands]\n",
            "boot = { run = \"dtr off; rts on\" }\n",
            "board-reset = { run = \"rts on; sleep 100; rts off\", help = \"Reset by RTS\" }\n",
        ),
    );
    let config = folder.load().unwrap();
    let laid = layered(&config, &[load(&folder, "board").unwrap()]).unwrap();
    let keymap = &laid.keybindings;
    assert_eq!(keymap.prefix_actions["F6"], "boot");
    assert_eq!(keymap.prefix_actions["h"], "board-reset");
    assert_eq!(keymap.prefix_actions["q"], "quit");
    assert_eq!(keymap.direct_actions["Shift-PageUp"], "scroll-to-top");
    assert!(!keymap.direct_actions.contains_key("shift-pageup"));
    assert_eq!(keymap.commands["board-reset"].help, "Reset by RTS");
    assert!(keymap.commands.contains_key("ctrl-c"));

    let resolver = KeyResolver::try_new(keymap).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    let names = |name: &str| -> Vec<&str> {
        let command = &resolver.prefix_commands[&key(name)];
        command.invocations.iter().map(|i| i.spec.name).collect()
    };
    assert_eq!(names("h"), ["rts", "sleep", "rts"]);
    assert_eq!(names("F6"), ["dtr", "rts"]);
    assert_eq!(resolver.copy_commands[&key("x")].name, "boot");
    assert_eq!(config.keybindings, KeyBindingsConfig::default());
}

/// MAX-240 step 2: a profile's highlights come after the global ones, drawn over them, one
/// of the same name replacing it; its triggers replace the same name; its plots replace the
/// same name in place, the others added.
/// How: the default files, a profile with `INFO` and `READY`, `u-boot` and `login`,
/// `sensors` and `temp`.
#[test]
fn profile_over_the_global_files() {
    let folder = Folder::new("profile-other", "");
    write(
        &folder,
        "board",
        concat!(
            "[highlights]\nINFO = \"Red\"\nREADY = \"Green\"\n",
            "[triggers.u-boot]\nmatch = \"autoboot\"\nrun = \"send x\"\n",
            "[triggers.login]\nmatch = \"login: \"\nrun = 'send root\\r'\n",
            "[plots.sensors]\nseparator = \"=\"\n",
            "[plots.temp]\nseparator = \":\"\n",
        ),
    );
    let config = folder.load().unwrap();
    let laid = layered(&config, &[load(&folder, "board").unwrap()]).unwrap();

    let names: Vec<_> = laid
        .highlights
        .entries
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    let global = config.highlights.entries.len();
    assert_eq!(names.len(), global + 1);
    assert_eq!(names[global - 1..], ["INFO", "READY"]);
    assert_eq!(names.iter().filter(|n| **n == "INFO").count(), 1);

    let triggers = &laid.triggers.rules;
    assert_eq!(triggers["u-boot"].run, "send x");
    assert!(triggers.contains_key("login"));

    let plots = laid.plots.names();
    let place = |names: &[String]| names.iter().position(|n| n == "sensors");
    assert_eq!(place(&plots), place(&config.plots.names()));
    assert_eq!(plots.last().unwrap(), "temp");
    assert_eq!(laid.plots.get("sensors").unwrap().separator, "=");
}

/// MAX-240 step 2: in a stack, a later profile wins over an earlier one.
/// How: two profiles binding `F6`, in both orders.
#[test]
fn later_profile_wins() {
    let folder = Folder::new("profile-stack", "");
    for name in ["a", "b"] {
        let text = format!(
            "[prefixed]\nF6 = \"{name}-boot\"\n[commands]\n{name}-boot = {{ run = \"break\" }}\n"
        );
        write(&folder, name, &text);
    }
    let config = folder.load().unwrap();
    let (a, b) = (load(&folder, "a").unwrap(), load(&folder, "b").unwrap());
    let laid = layered(&config, &[a.clone(), b.clone()]).unwrap();
    assert_eq!(laid.keybindings.prefix_actions["F6"], "b-boot");
    let laid = layered(&config, &[b, a]).unwrap();
    assert_eq!(laid.keybindings.prefix_actions["F6"], "a-boot");
}

/// MAX-240 step 2: a profile that doesn't read, or whose keys don't hold once laid, is an
/// error naming the profile and its file, saying what's wrong, as a global file's would; an
/// unknown key is a warning naming the profile, kept in the laid config. Line settings are
/// the saved link's: a `[link]` in a profile is unknown, `[link]` untouched.
/// How: bad syntax, a trigger that doesn't compile, a key on an unknown name, then unknown
/// keys and a `[link]`.
#[test]
fn profile_errors_name_the_profile_and_file() {
    let folder = Folder::new("profile-errors", "");
    let config = folder.load().unwrap();
    let path = folder.path("profiles").join("bad.toml");
    let head = format!("profile 'bad' ({}): ", path.display());
    let error = |text: &str| {
        write(&folder, "bad", text);
        match load(&folder, "bad") {
            Ok(profile) => layered(&config, &[profile]).err().unwrap().to_string(),
            Err(err) => err.to_string(),
        }
    };

    let text = error("[highlights]\nREADY = \n");
    assert!(
        text.starts_with(&format!("{head}invalid syntax:\n")),
        "{text}"
    );
    let text = error("[triggers.login]\nmatch = \"login: \"\nrun = \"rst\"\n");
    assert!(text.starts_with(&head), "{text}");
    assert!(text.contains("rst"), "{text}");
    let text = error("[prefixed]\nF6 = \"nope\"\n");
    assert_eq!(text, format!("{head}in [prefixed]: unknown action 'nope'"));

    write(&folder, "bad", "colour = 1\n[link]\nbaud = 9600\n");
    let profile = load(&folder, "bad").unwrap();
    let warning = "Unknown keys in profile 'bad': colour, link";
    assert_eq!(profile.warnings, [warning]);
    let laid = layered(&config, &[profile]).unwrap();
    assert_eq!(laid.warnings, [warning]);
    assert_eq!(laid.settings.link, config.settings.link);
}

/// MAX-240 step 3c: the profiles of a stack with no file in the folder, for the warning of
/// a link saved with them.
/// How: one profile written, a stack of it and two more.
#[test]
fn missing_profiles() {
    let folder = Folder::new("profile-missing", "");
    write(&folder, "board", "");
    let dir = folder.path("profiles");
    let names = ["nope".to_string(), "board".to_string(), "gone".to_string()];
    assert_eq!(missing(&dir, &names), ["nope", "gone"]);
    assert!(missing(&dir, &names[1..2]).is_empty());
}

/// MAX-240 step 3: a link opens with its profiles laid in order from the folder, a missing
/// one stopping it with its name.
/// How: a profile with a highlight, then a stack with a missing profile.
#[test]
fn profiles_laid_from_the_folder() {
    let folder = Folder::new("profile-link", "");
    write(&folder, "board", "[highlights]\nREADY = \"Green\"\n");
    let config = folder.load().unwrap();
    let dir = folder.path("profiles");
    let laid = with_profiles(&config, &dir, &["board".to_string()]).unwrap();
    let last = laid
        .highlights
        .entries
        .last()
        .map(|(name, _)| name.as_str());
    assert_eq!(last, Some("READY"));

    let names = ["board".to_string(), "nope".to_string()];
    let error = with_profiles(&config, &dir, &names).err().unwrap();
    assert_eq!(error.name, "nope");
}

/// MAX-240 step 4: the config's load writes the built-in profiles with their folder, as real
/// files to read and edit; once the folder is there, an edited profile stays and a deleted
/// one stays deleted. `write_missing` (`--write-config`'s) writes the deleted one again, only
/// that one.
/// How: a first load, `esp32.toml` edited and `arduino.toml` deleted, a second load, then
/// `write_missing`.
#[test]
fn built_in_profiles_written_with_the_folder() {
    let folder = Folder::new("profile-built-in", "");
    folder.load().unwrap();
    let dir = folder.path("profiles");
    for (name, text) in BUILT_IN {
        assert_eq!(fs::read_to_string(file_of(&dir, name)).unwrap(), text);
    }

    fs::write(file_of(&dir, "esp32"), "description = \"mine\"\n").unwrap();
    fs::remove_file(file_of(&dir, "arduino")).unwrap();
    folder.load().unwrap();
    assert_eq!(load(&folder, "esp32").unwrap().file.description, "mine");
    assert!(!file_of(&dir, "arduino").exists());

    assert_eq!(write_missing(&dir).unwrap(), [file_of(&dir, "arduino")]);
    let arduino = fs::read_to_string(file_of(&dir, "arduino")).unwrap();
    assert_eq!(arduino, BUILT_IN[0].1);
    assert_eq!(load(&folder, "esp32").unwrap().file.description, "mine");
    assert!(write_missing(&dir).unwrap().is_empty());
}

/// MAX-240 step 4: the built-in profiles load without a warning and lay over the default
/// config: `esp32`'s RTS reset in place of the DTR one on the same key, its bootloader on
/// `H`, its log level lines and its armed crash trigger; `arduino`'s serial plotter plot.
/// MAX-9: `esp32`'s `on_open` is commented out, and runs `board-reset` once uncommented.
/// MAX-220: its crash triggers, armed, silent, rearmed, each marking `c`. MAX-247: both
/// switch to line mode on load.
/// How: the default files, each profile laid over them; `esp32`'s text with `on_open`
/// uncommented.
#[test]
fn built_in_profiles_lay_over_the_defaults() {
    let folder = Folder::new("profile-built-in-lay", "");
    let config = folder.load().unwrap();
    let dir = folder.path("profiles");
    for (name, _) in BUILT_IN {
        assert!(load(&folder, name).unwrap().warnings.is_empty(), "{name}");
    }

    let esp32 = with_profiles(&config, &dir, &["esp32".to_string()]).unwrap();
    let keymap = &esp32.keybindings;
    assert_eq!(keymap.prefix_actions["h"], "board-reset");
    assert_eq!(keymap.prefix_actions["H"], "bootloader");
    assert!(
        keymap.commands["board-reset"]
            .run
            .starts_with("dtr off; rts on")
    );
    assert!(keymap.commands.contains_key("usb-bootloader"));
    let resolver = KeyResolver::try_new(keymap).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    assert_eq!(resolver.prefix_commands[&key("H")].name, "bootloader");
    let lines: Vec<_> = (esp32.highlights.entries.iter())
        .map(|(name, _)| name.as_str())
        .filter(|name| name.starts_with("esp-"))
        .collect();
    assert_eq!(lines, ["esp-error", "esp-warning", "esp-info", "esp-debug"]);
    esp32.highlights.compile_rules();
    let crashes: Vec<_> = (esp32.triggers.rules.iter())
        .filter(|(name, _)| name.starts_with("esp-"))
        .map(|(name, rule)| {
            assert!(rule.armed && rule.silent && rule.rearm, "{name}");
            assert_eq!(rule.mark.as_deref(), Some("c"), "{name}");
            name.as_str()
        })
        .collect();
    assert_eq!(
        crashes,
        [
            "esp-abort",
            "esp-brownout",
            "esp-panic",
            "esp-stack-overflow",
            "esp-task-watchdog"
        ]
    );
    esp32.triggers.compile().unwrap();
    assert_eq!(load(&folder, "esp32").unwrap().file.on_open, None);
    let (_, text) = BUILT_IN.iter().find(|(name, _)| *name == "esp32").unwrap();
    let uncommented = text.replace("# on_open = ", "on_open = ");
    let file: ProfileFile = toml::from_str(&uncommented).unwrap();
    let line = file.on_open.unwrap();
    let invocations = crate::commands::parse_with(&line, &resolver.commands).unwrap();
    let names: Vec<_> = invocations.iter().map(|i| i.spec.name).collect();
    assert_eq!(names, ["dtr", "rts", "sleep", "rts"]);

    for (name, _) in BUILT_IN {
        let on_load = load(&folder, name).unwrap().file.on_load.unwrap();
        assert_eq!(on_load, "mode line", "{name}");
        crate::commands::parse(&on_load).unwrap();
    }

    let arduino = with_profiles(&config, &dir, &["arduino".to_string()]).unwrap();
    assert_eq!(arduino.plots.names().last().unwrap(), "serial-plotter");
    assert!(!config.plots.names().contains(&"serial-plotter".to_string()));
}

/// MAX-240: a profile with `enabled = false` is off: a stack naming it lays the others and
/// says it's left out; its file isn't checked past its syntax, so a broken rule in it stops
/// nothing.
/// How: `off` with a highlight and a trigger that doesn't compile, and `board`, as a stack.
#[test]
fn off_profile_left_out() {
    let folder = Folder::new("profile-off", "");
    let off = concat!(
        "enabled = false\n",
        "[highlights]\nOFF = \"Red\"\n",
        "[triggers.bad]\nmatch = \"x\"\nrun = \"rst\"\n",
    );
    write(&folder, "off", off);
    write(&folder, "board", "[highlights]\nREADY = \"Green\"\n");
    let config = folder.load().unwrap();
    assert!(!load(&folder, "off").unwrap().file.enabled);

    let names = ["off".to_string(), "board".to_string()];
    let laid = with_profiles(&config, &folder.path("profiles"), &names).unwrap();
    let highlights: Vec<_> = (laid.highlights.entries.iter())
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(highlights.contains(&"READY") && !highlights.contains(&"OFF"));
    assert!(!laid.triggers.rules.contains_key("bad"));
    let warning = "Profile 'off' is off (enabled = false in its file): not laid";
    assert_eq!(laid.warnings.last().map(String::as_str), Some(warning));
}

/// MAX-240 step 5: the profiles `profile load` completes: the folder's `.toml` files that are
/// on, sorted; one that doesn't read is listed, so loading it says why; an off one isn't.
/// How: on, off, broken and a file that isn't a profile, then a folder that isn't there.
#[test]
fn on_names_lists_the_profiles_that_are_on() {
    let folder = Folder::new("profile-names", "");
    write(&folder, "zeta", "");
    write(&folder, "alpha", "description = \"a\"\n");
    write(&folder, "off", "enabled = false\n");
    write(&folder, "broken", "enabled = = \n");
    fs::write(folder.path("profiles/notes.txt"), "").unwrap();
    assert_eq!(
        on_names(&folder.path("profiles")),
        ["alpha", "broken", "zeta"]
    );
    assert!(on_names(&folder.path("nowhere")).is_empty());
}

/// MAX-248 step 0: a profile that doesn't read says why with the line and column; the
/// profiles popup's list says the reason alone. A trigger's bad value points at its table.
/// How: a syntax error, a value of the wrong type, a trigger's, loaded then listed.
#[test]
fn profile_read_errors_keep_their_text() {
    let folder = Folder::new("profile-read-errors", "");
    let head = "profile 'bad' (<dir>/profiles/bad.toml): invalid syntax:\nTOML parse error at ";
    let cases = [
        (
            "[highlights]\nREADY = \n",
            "line 2, column 9\n  |\n2 | READY = \n  |         ^\n\
             string values must be quoted, expected literal string\n",
            "string values must be quoted, expected literal string",
        ),
        (
            "description = 5\n",
            "line 1, column 15\n  |\n1 | description = 5\n  |               ^\n\
             invalid type: integer `5`, expected a string\n",
            "invalid type: integer `5`, expected a string",
        ),
        (
            "[triggers.r]\nmatch = 5\nrun = \"send x\"\n",
            "line 1, column 2\n  |\n1 | [triggers.r]\n  |  ^^^^^^^^\n\
             invalid type: integer `5`, expected a string\n",
            "invalid type: integer `5`, expected a string",
        ),
    ];
    let dir = folder.path("").display().to_string();
    for (text, error, listed) in cases {
        write(&folder, "bad", text);
        let said = load(&folder, "bad").err().unwrap().to_string();
        assert_eq!(said.replace(&dir, "<dir>/"), format!("{head}{error}"));
        let listed = Some(listed.to_string());
        assert_eq!(list(&folder.path("profiles"))[0].error, listed);
    }
}
