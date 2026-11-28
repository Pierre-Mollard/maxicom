//! Tab completion of `maxicom` in the shell (MAX-294): each shell's wrapper calls maxicom back
//! on a Tab, which answers from the one table of its options below, the parser's too, and the
//! names of the user's setup
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use serde::de::DeserializeOwned;

use crate::{
    api,
    args::{self, RESERVED},
    commands::{ArgKind, BAUD_RATE, Candidate, ENCODING, FLOW, NamedCommand, Names},
    configs::{
        AppConfig, KeyBindingsConfig, PlotsConfig, TriggersConfig, links::LinksConfig, profiles,
        read_toml, settings::SettingsConfig,
    },
    link::{lock, serial},
};

/// One option of `maxicom`: its short name (empty for none), its long one, the kind of value it
/// takes (none for a flag) and the help line shown next to it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionSpec {
    pub short: &'static str,
    pub long: &'static str,
    pub value: Option<ArgKind>,
    pub help: &'static str,
}

const fn flag(short: &'static str, long: &'static str, help: &'static str) -> OptionSpec {
    OptionSpec {
        short,
        long,
        value: None,
        help,
    }
}

const fn with(
    short: &'static str,
    long: &'static str,
    kind: ArgKind,
    help: &'static str,
) -> OptionSpec {
    OptionSpec {
        short,
        long,
        value: Some(kind),
        help,
    }
}

const NO_SUGGESTION: ArgKind = ArgKind::Int {
    min: 0,
    max: i64::MAX,
    suggestions: &[],
};

const LINE_LEVEL: ArgKind = ArgKind::Enum(&["keep", "on", "off"]);

/// Every option of `maxicom`, in `--help`'s order; the ones taking a value are the parser's
/// `value_options`, so that a value glued to its option splits off
#[rustfmt::skip]
pub const OPTIONS: &[OptionSpec] = &[
    with("-D", "--device", ArgKind::Address, "Device to open, never a saved name"),
    with("", "--tcp", ArgKind::String, "A device over TCP, HOST:PORT"),
    with("-b", "--baudrate", BAUD_RATE, "Baud rate"),
    with("", "--bits", ArgKind::Enum(&["5", "6", "7", "8"]), "Character data bits"),
    with("", "--stop-bits", ArgKind::Enum(&["1", "2"]), "Stop bits"),
    with("", "--parity", ArgKind::Enum(&["none", "even", "odd"]), "Parity"),
    with("", "--flow", FLOW, "Flow control: hw is RTS/CTS, sw XON/XOFF"),
    with("", "--dtr", LINE_LEVEL, "DTR set on open"),
    with("", "--rts", LINE_LEVEL, "RTS set on open"),
    with("-r", "--replay", ArgKind::Path, "A recording in place of a device"),
    with("", "--delay", NO_SUGGESTION, "Milliseconds between chunks of a replay"),
    flag("", "--restore", "Restore the last session on this device"),
    flag("", "--no-restore", "Start with an empty history"),
    flag("", "--cache-dir", "Print the session backups folder and exit"),
    flag("", "--write-config", "Update the config files to this version and exit"),
    flag("", "--reset-config", "Put the config files back to the defaults and exit"),
    with("-C", "--capturefile", ArgKind::Path, "Record the received text to a file"),
    flag("-H", "--displayhex", "Show the raw side panel in hex"),
    with("-R", "--remotecharset", ENCODING, "Read the received bytes as this encoding"),
    with("", "--plot", ArgKind::Plot, "Show this plot of plots.toml"),
    with("", "--profile", ArgKind::Profile, "Lay this profile over the config"),
    with("", "--config", ArgKind::Path, "Read the settings from this file"),
    with("", "--completions", ArgKind::Enum(SHELLS), "Print the shell's completion script"),
    flag("-h", "--help", "Print help"),
    flag("-v", "--version", "Print the version"),
];

/// The shells `--completions` writes a script for
pub const SHELLS: &[&str] = &["bash", "zsh", "fish", "powershell"];

/// The script of `shell`, which calls `maxicom --complete` back on each Tab; none for a shell
/// not in `SHELLS`
pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(include_str!("completion/maxicom.bash")),
        "zsh" => Some(include_str!("completion/maxicom.zsh")),
        "fish" => Some(include_str!("completion/maxicom.fish")),
        "powershell" => Some(include_str!("completion/maxicom.ps1")),
        _ => None,
    }
}

/// `maxicom --completions <shell>`: its script printed, else what the shells are
pub fn print_script(shell: &str) -> ExitCode {
    match script(shell) {
        Some(script) => {
            print!("{script}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "maxicom: no completion for '{shell}': --completions takes {}",
                SHELLS.join(", ")
            );
            ExitCode::FAILURE
        }
    }
}

/// `maxicom cli`'s options, before its action (MAX-285, MAX-292)
#[rustfmt::skip]
pub const CLI_OPTIONS: &[OptionSpec] = &[
    with("", "--instance", ArgKind::String, "The maxicom to reach: pid, link or address"),
    with("", "--socket", ArgKind::Path, "The socket to reach it at"),
    flag("", "--json", "The reply as the wire gives it"),
    flag("-h", "--help", "Print help"),
];

/// `maxicom api`'s options (MAX-292)
#[rustfmt::skip]
pub const API_OPTIONS: &[OptionSpec] = &[
    with("", "--instance", ArgKind::String, "The maxicom to reach: pid, link or address"),
    with("", "--socket", ArgKind::Path, "The socket to reach it at"),
    flag("-h", "--help", "Print help"),
];

/// `maxicom cli rx`'s options
#[rustfmt::skip]
pub const RX_OPTIONS: &[OptionSpec] = &[
    with("", "--since", NO_SUGGESTION, "From this offset, else the oldest byte kept"),
    flag("", "--follow", "Then the bytes as they come"),
    flag("", "--hex", "In hex, else as they came"),
];

/// `maxicom cli`'s actions besides a command line
#[rustfmt::skip]
const CLI_ACTIONS: &[(&str, &str)] = &[
    ("list", "The maxicoms running"),
    ("events", "The events of these kinds, live"),
    ("rx", "The bytes received"),
];

/// What the shell completes the word with: these values, each with its help line, or the
/// paths of the disk, which the shell lists itself, after `prefix` (the `--config=` of a
/// glued value, else empty)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Values(Vec<Candidate>),
    Files { prefix: String },
}

const NOTHING: Reply = Reply::Values(Vec::new());

/// The completion of `current`, the word being typed, after the words `before` it (the
/// program's name left out). For `maxicom`: the value of the option before it, glued
/// (`--parity=e`) or not; the options for a word starting with `-`; else a link to open,
/// the saved links first, then the serial ports. `maxicom cli` and `maxicom api` the same
/// way with their options, then `cli`'s action
pub fn complete(before: &[String], current: &str, setup: &Setup) -> Reply {
    match before.first().map(String::as_str) {
        Some("cli") => cli(&before[1..], current, setup),
        Some("api") => option_word(API_OPTIONS, &before[1..], current, setup).unwrap_or(NOTHING),
        Some(word) if RESERVED.contains(&word) => NOTHING,
        _ => option_word(OPTIONS, before, current, setup).unwrap_or_else(|| {
            let links = setup.links.iter().map(|name| candidate(name, "saved link"));
            let ports = (setup.ports.iter()).map(|name| candidate(name, "serial port"));
            starting(links.chain(ports), current)
        }),
    }
}

/// `maxicom cli`'s words: its options, then its action, `list`, `events` and its kinds,
/// `rx` and its options, else a command line, as the command line completes it
fn cli(before: &[String], current: &str, setup: &Setup) -> Reply {
    // the options' words, with their values
    let mut action = 0;
    while let Some(word) = before.get(action).filter(|word| word.starts_with('-')) {
        let takes = find(CLI_OPTIONS, word).is_some_and(|option| option.value.is_some());
        action += if takes { 2 } else { 1 };
    }
    let Some(verb) = before.get(action) else {
        if let Some(reply) = option_word(CLI_OPTIONS, before, current, setup) {
            return reply;
        }
        let actions = (CLI_ACTIONS.iter()).map(|(name, help)| candidate(name, help));
        let actions = starting(actions, current);
        let Reply::Values(mut found) = actions else {
            return actions;
        };
        found.extend(command_line(&[], current, setup));
        return Reply::Values(found);
    };
    let rest = &before[action + 1..];
    match verb.as_str() {
        "list" => NOTHING,
        "events" => starting(
            (crate::event::KINDS.iter())
                .filter(|kind| !rest.iter().any(|given| given == *kind))
                .map(|kind| candidate(kind, "")),
            current,
        ),
        "rx" => option_word(RX_OPTIONS, rest, current, setup).unwrap_or(NOTHING),
        _ => Reply::Values(command_line(&before[action..], current, setup)),
    }
}

/// The command line `words` then `current` completes with, as the command line does (MAX-12):
/// its candidates for the last word, each one whole for the shell's word `current`
fn command_line(words: &[String], current: &str, setup: &Setup) -> Vec<Candidate> {
    let mut line = words.join(" ");
    if !line.is_empty() {
        line.push(' ');
    }
    // where the shell's word starts in the line: the candidates replace its end
    let offset = line.len();
    line.push_str(current);
    let completion = crate::commands::complete(&line, &setup.names());
    let Some(kept) = completion.start.checked_sub(offset) else {
        return Vec::new();
    };
    (completion.candidates.into_iter())
        .map(|found| Candidate {
            value: format!("{}{}", &current[..kept], found.value),
            help: found.help,
        })
        .collect()
}

/// The completion of `current` when it is an option of `table` or the value of one, after
/// the words `before`: none for another word
fn option_word(
    table: &'static [OptionSpec],
    before: &[String],
    current: &str,
    setup: &Setup,
) -> Option<Reply> {
    if let Some(option) = value_expected(table, before) {
        return Some(value_of(option, "", current, setup));
    }
    if let Some((name, value)) = (current.split_once('=')).filter(|_| current.starts_with("--")) {
        return Some(
            match find(table, name).filter(|option| option.value.is_some()) {
                Some(option) => value_of(option, &format!("{name}="), value, setup),
                None => NOTHING,
            },
        );
    }
    if !current.starts_with('-') {
        return None;
    }
    let spelled = table.iter().flat_map(|option| {
        let short = (!option.short.is_empty()).then_some(option.short);
        (short.into_iter().chain([option.long])).map(|name| candidate(name, option.help))
    });
    Some(starting(spelled, current))
}

/// The candidates whose value starts with `typed`
fn starting(candidates: impl Iterator<Item = Candidate>, typed: &str) -> Reply {
    Reply::Values(
        candidates
            .filter(|found| found.value.starts_with(typed))
            .collect(),
    )
}

/// The option of `table` named so, by its short name or its long one
fn find(table: &'static [OptionSpec], name: &str) -> Option<&'static OptionSpec> {
    (table.iter())
        .find(|option| option.long == name || !option.short.is_empty() && option.short == name)
}

/// The option of `table` whose value the next word is: the last word before it is an option
/// taking one, and not itself the value of the option before it (`-C -b`: `-b` is the file)
fn value_expected(table: &'static [OptionSpec], before: &[String]) -> Option<&'static OptionSpec> {
    let mut expected = None;
    for word in before {
        expected = match expected {
            Some(_) => None,
            None => find(table, word).filter(|option| option.value.is_some()),
        };
    }
    expected
}

/// The values of `option` starting with `typed`, each after `prefix`: the maxicoms running
/// for `--instance`, else by the kind of its value
fn value_of(option: &OptionSpec, prefix: &str, typed: &str, setup: &Setup) -> Reply {
    let glued = |found: Candidate| Candidate {
        value: format!("{prefix}{}", found.value),
        help: found.help,
    };
    if option.long == "--instance" {
        return starting(
            setup.instances.iter().cloned().map(glued),
            &format!("{prefix}{typed}"),
        );
    }
    let listed: Vec<&str> = match option.value {
        Some(ArgKind::Path) => {
            return Reply::Files {
                prefix: prefix.to_string(),
            };
        }
        // `--device` never names a saved link
        Some(ArgKind::Address) => setup.ports.iter().map(String::as_str).collect(),
        Some(ArgKind::Plot) => setup.plots.iter().map(String::as_str).collect(),
        Some(ArgKind::Profile) => setup.profiles.iter().map(String::as_str).collect(),
        Some(kind) => kind.candidates(""),
        None => Vec::new(),
    };
    Reply::Values(
        (listed.into_iter())
            .filter(|value| value.starts_with(typed))
            .map(|value| candidate(&format!("{prefix}{value}"), ""))
            .collect(),
    )
}

fn candidate(value: &str, help: &str) -> Candidate {
    Candidate {
        value: value.to_string(),
        help: help.to_string(),
    }
}

/// `maxicom --complete -- <words>`, hidden: the words after `maxicom` up to the cursor, the
/// one being typed last (maybe empty), as each shell's wrapper passes them. Prints the
/// reply and nothing else, never an error, as the shell would show it in the line: a first
/// line `values`, then `value<TAB>help` per candidate, or the one line `files<TAB>prefix`
pub fn main(words: Vec<String>) -> ExitCode {
    let (current, before) = match words.split_last() {
        Some((current, before)) => (current.as_str(), before),
        None => ("", &[][..]),
    };
    // `--config` given on the line, its settings and profiles
    let config =
        args::config_file_from(before.iter().cloned()).unwrap_or_else(AppConfig::config_path);
    let mut setup = Setup::read(&config, serial::port_names());
    // the maxicoms running, by their sockets only: no `hello`, which may wait
    if before
        .first()
        .is_some_and(|word| word == "cli" || word == "api")
    {
        let running = api::running(api::socket_dir().as_deref(), lock::lock_dir().as_deref());
        setup.instances = (running.into_iter())
            .map(|found| Candidate {
                value: found.pid.to_string(),
                help: found.link.unwrap_or_default(),
            })
            .collect();
    }
    let mut out = io::stdout().lock();
    let _ = out.write_all(printed(&complete(before, current, &setup)).as_bytes());
    ExitCode::SUCCESS
}

/// The reply as `main` prints it
fn printed(reply: &Reply) -> String {
    match reply {
        Reply::Files { prefix } => format!("files\t{prefix}\n"),
        Reply::Values(candidates) => {
            let lines = candidates.iter().map(|found| match found.help.as_str() {
                "" => format!("{}\n", found.value),
                help => format!("{}\t{help}\n", found.value),
            });
            std::iter::once("values\n".to_string())
                .chain(lines)
                .collect()
        }
    }
}

/// The names of the user's setup a completion offers, read and never written: a file that
/// isn't there, or doesn't read, gives the defaults, as the app would write them, and nothing
/// is said. `ports` the serial ports, from the system; `instances` the maxicoms
/// running, by pid with their link
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Setup {
    pub links: Vec<String>,
    pub ports: Vec<String>,
    pub plots: Vec<String>,
    pub profiles: Vec<String>,
    pub triggers: Vec<String>,
    pub named: Vec<NamedCommand>,
    pub instances: Vec<Candidate>,
}

impl Setup {
    /// The setup of the settings file `config`: its links for this machine, its plots, the
    /// profiles that are on next to it, its triggers and the named commands of its keymap
    pub fn read(config: &Path, ports: Vec<String>) -> Self {
        let settings: SettingsConfig = read_file(config).unwrap_or_default();
        let host = crate::configs::links::this_host();
        // `load` takes no file as no link, and never writes
        let links = (LinksConfig::load(&AppConfig::active_links_path(&settings)).ok())
            .map(|config| {
                (config.links.iter())
                    .filter(|link| link.is_for(host.as_deref()))
                    .map(|link| link.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let plots: PlotsConfig = read_or_default(&AppConfig::active_plots_path(&settings));
        let triggers: TriggersConfig = read_or_default(&AppConfig::active_triggers_path(&settings));
        let keymap: KeyBindingsConfig =
            read_or_default(&AppConfig::active_keybindings_path(&settings));
        let named = (keymap.commands.into_iter())
            .map(|(name, command)| NamedCommand {
                name,
                run: command.run,
                help: command.help,
                invocations: Vec::new(),
            })
            .collect();
        Self {
            links,
            ports,
            plots: plots.names(),
            profiles: profiles::on_names(&AppConfig::profiles_dir_of(config)),
            triggers: triggers.rules.into_keys().collect(),
            named,
            instances: Vec::new(),
        }
    }

    pub fn names(&self) -> Names<'_> {
        Names {
            triggers: &self.triggers,
            links: &self.links,
            ports: &self.ports,
            plots: &self.plots,
            profiles: &self.profiles,
            named: &self.named,
        }
    }
}

/// The TOML file at `path` read, else the defaults
fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    read_file(path).unwrap_or_default()
}

/// The TOML file at `path` read, none when it isn't there or doesn't read
fn read_file<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    read_toml(&text).ok().map(|(value, _)| value)
}

#[cfg(test)]
#[path = "tests/completion.rs"]
mod tests;
