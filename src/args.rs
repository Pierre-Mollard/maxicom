use std::{env, path::PathBuf, time::Duration};

use crate::{
    configs::{LinkDefaults, TextEncoding, links::SavedLink},
    link::{
        Address, LinkKind,
        serial::{CharSize, FlowControl, LineSettings, Parity, StopBits},
    },
};

/// `address` is the device to open, a replay for `--replay` (MAX-168), the settings' when
/// none is given and none when they have none (MAX-181); `line` its line settings, the options
/// over the settings' or a saved link's (MAX-185). `replay_delay` is
/// `--delay`, for a replay only. `restore` is `--restore` / `--no-restore`, the last one given,
/// over `sessions.restore`. `capture_file`, `display_hex` and `encoding` are minicom's `-C`,
/// `-H` and `-R`, over the settings (MAX-181)
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
    },
    Help,
    Version,
    CacheDir,
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

/// The link given: an address (`--device`, `--tcp`, `--replay`), or a word that is a saved
/// link's name first, else a device (MAX-185)
enum Given {
    Address(Address),
    Word(String),
}

/// `word` as a saved link's name, its address and line over `[link]`'s, else as a device with
/// `[link]`'s line; `what` before a device that doesn't parse
fn by_name(
    word: &str,
    what: &str,
    defaults: &LinkDefaults,
    links: &[SavedLink],
) -> Result<(Address, LineSettings), ParseError> {
    let bad = |err| ParseError::BadFormat(format!("{what}{err}"));
    match links.iter().find(|link| link.name == word) {
        Some(link) => {
            let address = link.address.parse().map_err(bad)?;
            Ok((address, link.over(defaults).line()))
        }
        None => Ok((word.parse().map_err(bad)?, defaults.line())),
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
    let mut replay_delay = Duration::ZERO;
    let mut restore: Option<bool> = None;
    let mut capture_file: Option<PathBuf> = None;
    let mut display_hex = false;
    let mut encoding: Option<TextEncoding> = None;
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
            "--restore" => restore = Some(true),
            "--no-restore" => restore = Some(false),
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }
            "--version" | "-v" => return Ok(ParseOutcome::Version),
            "--cache-dir" => return Ok(ParseOutcome::CacheDir),
            // a saved link's name, or the device without `--device`: `maxicom esp32`,
            // `maxicom /dev/ttyUSB0`, `maxicom tcp://host:port`
            word if !word.starts_with('-') => given = Some(Given::Word(word.to_string())),

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    // the link given, else the settings' one, if any; a name a saved link's first
    let (address, mut line) = match (given, defaults.device()) {
        (Some(Given::Address(address)), _) => (Some(address), defaults.line()),
        (Some(Given::Word(word)), _) => {
            let (address, line) = by_name(&word, "", defaults, links)?;
            (Some(address), line)
        }
        (None, Some(device)) => {
            let (address, line) = by_name(device, "link.device: ", defaults, links)?;
            (Some(address), line)
        }
        (None, None) => (None, defaults.line()),
    };
    line.baud = baud.unwrap_or(line.baud);
    line.framing.char_size = char_size.unwrap_or(line.framing.char_size);
    line.framing.stop_bits = stop_bits.unwrap_or(line.framing.stop_bits);
    line.framing.parity = parity.unwrap_or(line.framing.parity);
    line.flow_control = flow.unwrap_or(line.flow_control);
    if address
        .as_ref()
        .is_some_and(|address| address.kind() == LinkKind::Tcp)
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

  LINK                     A saved link's name (links.toml), opened with its line
                           settings, the options over them
  DEVICE                   A serial port (/dev/ttyUSB0, COM3), a TCP link
                           (tcp://192.168.1.50:4000) or a file to replay
                           (file://boot.raw), same as --device

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

  -r, --replay <PATH>      A recording in place of a device (its folder, .raw or
                           .meta), same as file://PATH
      --delay <ms>         Time to wait between chunks of a replay, in milliseconds

      --restore            Restore the last session on this device into the history
      --no-restore         Start with an empty history, whatever sessions.restore says
      --cache-dir          Print the session backups folder and exit

  -C, --capturefile <PATH> Record the received text to PATH from the start
  -H, --displayhex         Show the raw side panel in hex
  -R, --remotecharset <ENC>
                           Read the received bytes as utf8, ascii, cp437 or
                           latin1 [default: the settings' encoding]

  An option's value can be glued to it: --baudrate=9600, -b9600.

  -h, --help               Print help
  -v, --version            Print the version
"
    );
}

#[cfg(test)]
#[path = "tests/args.rs"]
mod tests;
