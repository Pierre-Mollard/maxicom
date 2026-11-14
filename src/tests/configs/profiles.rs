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
/// nor plots, no line setting unless written; a name with no file isn't found, the file
/// named.
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
    assert_eq!(file.link, LinkLayer::default());

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

    write(&folder, "board", "[link]\nbaud = 9600\n");
    let laid = layered(&config, &[load(&folder, "board").unwrap()]).unwrap();
    assert_eq!(laid.settings.link.baud, 9600);
    assert_eq!(config.settings.link.baud, 115_200);
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
/// same name in place, the others added; its line settings replace `[link]`'s, the others
/// stay.
/// How: the default files, a profile with `INFO` and `READY`, `u-boot` and `login`,
/// `serial-plotter` and `temp`, a baud rate and flow control.
#[test]
fn profile_over_the_global_files() {
    let folder = Folder::new("profile-other", "");
    write(
        &folder,
        "board",
        concat!(
            "[link]\nbaud = 9600\nflow = \"rtscts\"\n",
            "[highlights]\nINFO = \"Red\"\nREADY = \"Green\"\n",
            "[triggers.u-boot]\nmatch = \"autoboot\"\nrun = \"send x\"\n",
            "[triggers.login]\nmatch = \"login: \"\nrun = 'send root\\r'\n",
            "[plots.serial-plotter]\nseparator = \"=\"\n",
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
    let place = |names: &[String]| names.iter().position(|n| n == "serial-plotter");
    assert_eq!(place(&plots), place(&config.plots.names()));
    assert_eq!(plots.last().unwrap(), "temp");
    assert_eq!(laid.plots.get("serial-plotter").unwrap().separator, "=");

    let link = &laid.settings.link;
    assert_eq!((link.baud, link.flow), (9600, FlowControl::RtsCts));
    assert_eq!((link.bits, link.stop_bits), (8, 1));
}

/// MAX-240 step 2: in a stack, a later profile wins over an earlier one.
/// How: two profiles binding `F6` and setting the baud rate, in both orders.
#[test]
fn later_profile_wins() {
    let folder = Folder::new("profile-stack", "");
    for (name, baud) in [("a", 9600), ("b", 57600)] {
        let text = format!(
            "[link]\nbaud = {baud}\n[prefixed]\nF6 = \"{name}-boot\"\n[commands]\n{name}-boot = {{ run = \"break\" }}\n"
        );
        write(&folder, name, &text);
    }
    let config = folder.load().unwrap();
    let (a, b) = (load(&folder, "a").unwrap(), load(&folder, "b").unwrap());
    let laid = layered(&config, &[a.clone(), b.clone()]).unwrap();
    assert_eq!(laid.keybindings.prefix_actions["F6"], "b-boot");
    assert_eq!(laid.settings.link.baud, 57600);
    let laid = layered(&config, &[b, a]).unwrap();
    assert_eq!(laid.keybindings.prefix_actions["F6"], "a-boot");
    assert_eq!(laid.settings.link.baud, 9600);
}

/// MAX-240 step 2: a profile that doesn't read, or whose keys or line settings don't hold
/// once laid, is an error naming the profile and its file, saying what's wrong, as a global
/// file's would; an unknown key is a warning naming the profile, kept in the laid config.
/// How: bad syntax, a trigger that doesn't compile, a key on an unknown name, a line setting
/// out of range, then unknown keys.
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
    let text = error("[link]\nbits = 9\n");
    assert!(
        text.starts_with(&format!("{head}[link]: link.bits")),
        "{text}"
    );

    write(&folder, "bad", "colour = 1\n[link]\nbauds = 9600\n");
    let profile = load(&folder, "bad").unwrap();
    let warning = "Unknown keys in profile 'bad': colour, link.bauds";
    assert_eq!(profile.warnings, [warning]);
    let laid = layered(&config, &[profile]).unwrap();
    assert_eq!(laid.warnings, [warning]);
}
