use std::{env, path::PathBuf, time::Duration};

use crate::{
    configs::{LinkDefaults, TextEncoding, links::SavedLink},
    link::{
        Address,
        serial::{CharSize, FlowControl, LineLevel, LineSettings, Parity, StopBits},
    },
};

/// `address` is the device to open, a replay for `--replay` (MAX-168), the settings' when
/// none is given and none when they have none (MAX-181); `line` its line settings, the options
/// over the settings' or a saved link's (MAX-185). `replay_delay` is
/// `--delay`, for a replay only. `restore` is `--restore` / `--no-restore`, the last one given,
/// over `sessions.restore`. `capture_file`, `display_hex` and `encoding` are minicom's `-C`,
/// `-H` and `-R`, over the settings (MAX-181). `plot` is `--plot`, over the settings' (MAX-222).
/// `profiles` the stack the link opens with (MAX-240): a saved link's own, then the
/// `--profile` ones in order, a later one winning
#[derive(Debug)]
pub enum ParseOutcome {
    Run {
        address: Option<Address>,
        line: LineSettings,
        replay_delay: Duration,
        restore: Option<bool>,
        capture_file: Option<PathBuf>,
        display_hex: bool,
        encoding: Option<TextEncoding>,
        plot: Option<String>,
        profiles: Vec<String>,
    },
    Help,
    Version,
    CacheDir,
    WriteConfig,
}

#[derive(Debug)]
pub enum ParseError {
    MissingValue(&'static str),
    BadFormat(String),
    UnknownArgument(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingValue(option) => {
                write!(formatter, "Missing value for option '{option}'.")
            }

            ParseError::UnknownArgument(argument) => {
                write!(formatter, "Unknown argument '{argument}'.")
            }

            ParseError::BadFormat(msg) => {
                write!(formatter, "{msg}")
            }
        }
    }
}

/// The options over `defaults`, the `[link]` settings (MAX-181); `links` this machine's saved
/// links, which a device name given or `[link]`'s may be (MAX-185)
pub fn parse_args(
    defaults: &LinkDefaults,
    links: &[SavedLink],
) -> Result<ParseOutcome, ParseError> {
    parse_args_from(env::args().skip(1), defaults, links) // skip program name
}

/// The first words kept for maxicom's own tools (MAX-285): `cli`, the client of a running
/// maxicom, then `api` (MAX-292) and `mcp` (MAX-149). Never a saved link's name: a device file
/// of that name opens as `./cli`
pub const RESERVED: &[&str] = &["cli", "api", "mcp"];

/// The words after `word` when it is the first argument: `cli` (MAX-285), `api` (MAX-292).
/// Read before the settings load, as these clients need none of them
pub fn after(word: &str) -> Option<Vec<String>> {
    after_from(word, env::args().skip(1))
}

fn after_from(word: &str, args: impl IntoIterator<Item = String>) -> Option<Vec<String>> {
    let mut args = args.into_iter();
    if args.next()? != word {
        return None;
    }
    Some(args.collect())
}

/// The `--config` file, read before the settings load since the other options need them
/// (MAX-206)
pub fn config_file() -> Option<PathBuf> {
    config_file_from(env::args().skip(1))
}

/// Whether `--reset-config` is given (MAX-231): read before the settings load, as a file
/// that doesn't load mustn't stop it
pub fn reset_config() -> bool {
    reset_config_from(env::args().skip(1))
}

fn reset_config_from(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == "--reset-config")
}

/// The value of the last `--config`, as the last of any option wins
fn config_file_from(args: impl IntoIterator<Item = String>) -> Option<PathBuf> {
    let args = split_values(args);
    (args.windows(2))
        .rfind(|pair| pair[0] == "--config")
        .map(|pair| PathBuf::from(&pair[1]))
}

/// The link given: an address (`--device`, `--tcp`, `--replay`), or a word that is a saved
/// link's name first, else a device (MAX-185)
enum Given {
    Address(Address),
    Word(String),
}

/// `word` as a saved link's name, its address and line over `[link]`'s, with its profiles,
/// else as a device with `[link]`'s line and none; `what` before a device that doesn't parse
fn by_name(
    word: &str,
    what: &str,
    defaults: &LinkDefaults,
    links: &[SavedLink],
) -> Result<(Address, LineSettings, Vec<String>), ParseError> {
    let bad = |err| ParseError::BadFormat(format!("{what}{err}"));
    match links.iter().find(|link| link.name == word) {
        Some(link) => {
            let address = link.address.parse().map_err(bad)?;
            Ok((address, link.over(defaults).line(), link.profiles.clone()))
        }
        None => Ok((word.parse().map_err(bad)?, defaults.line(), Vec::new())),
    }
}

/// The options that take a value, short then long
const VALUE_OPTIONS: &[(&str, &str)] = &[
    ("-D", "--device"),
    ("-b", "--baudrate"),
    ("-C", "--capturefile"),
    ("-R", "--remotecharset"),
    ("-r", "--replay"),
    ("", "--delay"),
    ("", "--tcp"),
    ("", "--bits"),
    ("", "--stop-bits"),
    ("", "--parity"),
    ("", "--flow"),
    ("", "--dtr"),
    ("", "--rts"),
    ("", "--config"),
    ("", "--plot"),
    ("", "--profile"),
];

/// The value glued to its option split off, as getopt takes it for minicom (MAX-181):
/// `--baudrate=9600` and `-b9600` are `--baudrate 9600` and `-b 9600`. Only for the options
/// that take a value, so `--restore=x` stays an unknown argument
fn split_values(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut split = Vec::new();
    for arg in args {
        let glued = if arg.starts_with("--") {
            arg.split_once('=')
                .filter(|(name, _)| VALUE_OPTIONS.iter().any(|(_, long)| long == name))
        } else {
            VALUE_OPTIONS
                .iter()
                .filter(|(short, _)| !short.is_empty())
                .find_map(|(short, _)| arg.strip_prefix(short).map(|value| (*short, value)))
                .filter(|(_, value)| !value.is_empty())
        };
        match glued {
            Some((name, value)) => split.extend([name.to_string(), value.to_string()]),
            None => split.push(arg),
        }
    }
    split
}

fn parse_args_from(
    args: impl IntoIterator<Item = String>,
    defaults: &LinkDefaults,
    links: &[SavedLink],
) -> Result<ParseOutcome, ParseError> {
    let mut args = split_values(args).into_iter();
    let mut given: Option<Given> = None;
    // the line options, over the line of the link given once it's known
    let mut baud = None;
    let mut char_size = None;
    let mut stop_bits = None;
    let mut parity = None;
    let mut flow = None;
    let mut dtr = None;
    let mut rts = None;
    let mut replay_delay = Duration::ZERO;
    let mut restore: Option<bool> = None;
    let mut capture_file: Option<PathBuf> = None;
    let mut display_hex = false;
    let mut encoding: Option<TextEncoding> = None;
    let mut plot: Option<String> = None;
    let mut profiles: Vec<String> = Vec::new();
    // the line options given, refused on a link whose bridge sets the line (MAX-156)
    let mut line_options: Vec<&'static str> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--replay" | "-r" => {
                let path = args
                    .next()
                    .ok_or(ParseError::MissingValue("Missing file path for --replay"))?;
                given = Some(Given::Address(
                    format!("file://{path}")
                        .parse()
                        .map_err(ParseError::BadFormat)?,
                ));
            }
            "--delay" => {
                let val = args.next().ok_or(ParseError::MissingValue(
                    "Missing millisecond delay for --delay",
                ))?;
                replay_delay = val
                    .parse::<u64>()
                    .map(Duration::from_millis)
                    .map_err(|_| ParseError::BadFormat(format!("Invalid delay '{val}'")))?;
            }
            "--device" | "-D" => {
                // a device, never a saved name: the way to a path that is also one
                let device = args.next().ok_or(ParseError::MissingValue("--device"))?;
                given = Some(Given::Address(
                    device.parse().map_err(ParseError::BadFormat)?,
                ));
            }
            "--tcp" => {
                let addr = args.next().ok_or(ParseError::MissingValue("--tcp"))?;
                given = Some(Given::Address(
                    format!("tcp://{addr}")
                        .parse()
                        .map_err(ParseError::BadFormat)?,
                ));
            }
            "--baudrate" | "-b" => {
                line_options.push("--baudrate");
                let val = args.next().ok_or(ParseError::MissingValue("--baudrate"))?;
                baud =
                    Some(val.parse::<u32>().map_err(|_| {
                        ParseError::BadFormat(format!("Invalid baud rate '{val}'"))
                    })?);
            }
            "--bits" => {
                line_options.push("--bits");
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                char_size = Some(val.parse::<CharSize>().map_err(ParseError::BadFormat)?);
            }
            "--stop-bits" => {
                line_options.push("--stop-bits");
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                stop_bits = Some(val.parse::<StopBits>().map_err(ParseError::BadFormat)?);
            }
            "--parity" => {
                line_options.push("--parity");
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                parity = Some(val.parse::<Parity>().map_err(ParseError::BadFormat)?);
            }
            "--flow" => {
                line_options.push("--flow");
                let val = args.next().ok_or(ParseError::MissingValue("--flow"))?;
                flow = Some(val.parse::<FlowControl>().map_err(ParseError::BadFormat)?);
            }
            "--dtr" => {
                line_options.push("--dtr");
                let val = args.next().ok_or(ParseError::MissingValue("--dtr"))?;
                dtr = Some(val.parse::<LineLevel>().map_err(ParseError::BadFormat)?);
            }
            "--rts" => {
                line_options.push("--rts");
                let val = args.next().ok_or(ParseError::MissingValue("--rts"))?;
                rts = Some(val.parse::<LineLevel>().map_err(ParseError::BadFormat)?);
            }
            "--capturefile" | "-C" => {
                let path = args
                    .next()
                    .ok_or(ParseError::MissingValue("--capturefile"))?;
                capture_file = Some(PathBuf::from(path));
            }
            "--displayhex" | "-H" => display_hex = true,
            "--remotecharset" | "-R" => {
                let val = args
                    .next()
                    .ok_or(ParseError::MissingValue("--remotecharset"))?;
                encoding = Some(val.parse().map_err(ParseError::BadFormat)?);
            }
            "--plot" => {
                plot = Some(args.next().ok_or(ParseError::MissingValue("--plot"))?);
            }
            "--profile" => {
                profiles.push(args.next().ok_or(ParseError::MissingValue("--profile"))?);
            }
            // already read by `config_file`, before the settings loaded (MAX-206)
            "--config" => {
                args.next().ok_or(ParseError::MissingValue("--config"))?;
            }
            "--restore" => restore = Some(true),
            "--no-restore" => restore = Some(false),
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }
            "--version" | "-v" => return Ok(ParseOutcome::Version),
            "--cache-dir" => return Ok(ParseOutcome::CacheDir),
            "--write-config" => return Ok(ParseOutcome::WriteConfig),
            // already handled by `reset_config`, before the settings loaded (MAX-231)
            "--reset-config" => {}
            // not a link anywhere else on the line (MAX-285)
            word if RESERVED.contains(&word) => {
                return Err(ParseError::BadFormat(format!(
                    "'{word}' is reserved for maxicom's own commands: a device of that name \
                     opens as ./{word}"
                )));
            }
            // a saved link's name, or the device without `--device`: `maxicom board`,
            // `maxicom /dev/ttyUSB0`, `maxicom tcp://host:port`
            word if !word.starts_with('-') => given = Some(Given::Word(word.to_string())),

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    // the link given, else the settings' one, if any; a name a saved link's first
    let (address, mut line, own) = match (given, defaults.device()) {
        (Some(Given::Address(address)), _) => (Some(address), defaults.line(), Vec::new()),
        (Some(Given::Word(word)), _) => {
            let (address, line, own) = by_name(&word, "", defaults, links)?;
            (Some(address), line, own)
        }
        (None, Some(device)) => {
            let (address, line, own) = by_name(device, "link.device: ", defaults, links)?;
            (Some(address), line, own)
        }
        (None, None) => (None, defaults.line(), Vec::new()),
    };
    let profiles = own.into_iter().chain(profiles).collect();
    line.baud = baud.unwrap_or(line.baud);
    line.framing.char_size = char_size.unwrap_or(line.framing.char_size);
    line.framing.stop_bits = stop_bits.unwrap_or(line.framing.stop_bits);
    line.framing.parity = parity.unwrap_or(line.framing.parity);
    line.flow_control = flow.unwrap_or(line.flow_control);
    line.dtr = dtr.unwrap_or(line.dtr);
    line.rts = rts.unwrap_or(line.rts);
    if address
        .as_ref()
        .is_some_and(|address| address.kind().is_tcp() && !address.kind().has_line_settings())
        && let Some(option) = line_options.first()
    {
        return Err(ParseError::BadFormat(format!(
            "{option} doesn't apply to a TCP link: its bridge sets the line"
        )));
    }
    Ok(ParseOutcome::Run {
        address,
        line,
        replay_delay,
        restore,
        capture_file,
        display_hex,
        encoding,
        plot,
        profiles,
    })
}

pub fn print_version() {
    println!(
        "\
maxicom {}
Copyright (c) 2026 Pierre Mollard.

Licensed under either of
  * Apache License, Version 2.0 (LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0)
  * MIT license (LICENSE-MIT or https://opensource.org/licenses/MIT)
at your option.
",
        env!("CARGO_PKG_VERSION")
    );
}

pub fn print_help() {
    println!(
        "\
maxicom — serial terminal

Usage:
  maxicom [OPTIONS] [LINK | DEVICE]
  maxicom cli ...          Drive a running maxicom from the shell: see
                           maxicom cli --help
  maxicom api ...          A session with a running maxicom over stdio, for
                           programs through ssh: see maxicom api --help

  LINK                     A saved link's name (links.toml), opened with its line
                           settings, the options over them
  DEVICE                   A serial port (/dev/ttyUSB0, COM3), a TCP link
                           (tcp://192.168.1.50:4000, telnet://host:port) or a
                           file to replay (file://boot.raw), same as --device

Options:
  -D, --device <DEVICE>    Device to open, never a saved name [default: [link]
                           device in config.toml, else the directory]
      --tcp <HOST:PORT>    A device over TCP, same as tcp://HOST:PORT

  Line settings, for a serial port (a TCP link's bridge sets them), the defaults
  from [link] in config.toml:
  -b, --baudrate <RATE>    Baud rate [default: 115200]
      --bits <5|6|7|8>     Character data bits [default: 8]
      --stop-bits <1|2>    Stop bits [default: 1]
      --parity <n|e|o>     Parity: none, even, odd [default: none]
      --flow <none|hw|sw>  Flow control: none, rtscts (hw), xonxoff (sw) [default: none]
      --dtr <keep|on|off>  DTR set on open, keep: as the OS puts it [default: keep]
      --rts <keep|on|off>  RTS set on open, keep: as the OS puts it [default: keep]

  -r, --replay <PATH>      A recording in place of a device (its folder, .raw or
                           .meta), same as file://PATH
      --delay <ms>         Time to wait between chunks of a replay, in milliseconds

      --restore            Restore the last session on this device into the history
      --no-restore         Start with an empty history, whatever sessions.restore says
      --cache-dir          Print the session backups folder and exit
      --write-config       Update the config files to this version and exit: new keys
                           and comments added, your values kept, the old files saved
                           as .bak
      --reset-config       Put the config files back to the defaults and exit, after a
                           confirmation: the old files saved as .bak, links.toml kept

  -C, --capturefile <PATH> Record the received text to PATH from the start
  -H, --displayhex         Show the raw side panel in hex
  -R, --remotecharset <ENC>
                           Read the received bytes as utf8, ascii, cp437 or
                           latin1 [default: the settings' encoding]
      --plot <NAME>        Show the plot NAME of plots.toml in the raw side panel's
                           place [default: plot in config.toml]
      --profile <NAME>     Lay the profile NAME (profiles/NAME.toml next to config.toml)
                           over the config, after a saved link's own; repeatable, the
                           later one winning

  An option's value can be glued to it: --baudrate=9600, -b9600.

      --config <FILE>      Read the settings from FILE instead of config.toml,
                           created with the defaults if it doesn't exist

  -h, --help               Print help
  -v, --version            Print the version
"
    );
}

#[cfg(test)]
#[path = "tests/args.rs"]
mod tests;
