use std::{env, path::PathBuf, time::Duration};

use crate::{
    configs::{LinkDefaults, TextEncoding},
    link::{
        Address, LinkKind,
        serial::{CharSize, FlowControl, LineSettings, Parity, StopBits},
    },
};

/// `address` is the device to open, a replay for `--replay` (MAX-168), the settings' when
/// none is given and none when they have none (MAX-181); `line` its line settings, the options
/// over the settings'. `replay_delay` is
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

/// The options over `defaults`, the `[link]` settings (MAX-181)
pub fn parse_args(defaults: &LinkDefaults) -> Result<ParseOutcome, ParseError> {
    parse_args_from(env::args().skip(1), defaults) // skip program name
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
) -> Result<ParseOutcome, ParseError> {
    let mut args = split_values(args).into_iter();
    let mut address: Option<Address> = None;
    let mut line = defaults.line();
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
                address = Some(
                    format!("file://{path}")
                        .parse()
                        .map_err(ParseError::BadFormat)?,
                );
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
                let device = args.next().ok_or(ParseError::MissingValue("--device"))?;
                address = Some(device.parse().map_err(ParseError::BadFormat)?);
            }
            "--tcp" => {
                let addr = args.next().ok_or(ParseError::MissingValue("--tcp"))?;
                address = Some(
                    format!("tcp://{addr}")
                        .parse()
                        .map_err(ParseError::BadFormat)?,
                );
            }
            "--baudrate" | "-b" => {
                line_options.push("--baudrate");
                let val = args.next().ok_or(ParseError::MissingValue("--baudrate"))?;
                line.baud = val
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                line_options.push("--bits");
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                line.framing.char_size = val.parse::<CharSize>().map_err(ParseError::BadFormat)?;
            }
            "--stop-bits" => {
                line_options.push("--stop-bits");
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                line.framing.stop_bits = val.parse::<StopBits>().map_err(ParseError::BadFormat)?;
            }
            "--parity" => {
                line_options.push("--parity");
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                line.framing.parity = val.parse::<Parity>().map_err(ParseError::BadFormat)?;
            }
            "--flow" => {
                line_options.push("--flow");
                let val = args.next().ok_or(ParseError::MissingValue("--flow"))?;
                line.flow_control = val.parse::<FlowControl>().map_err(ParseError::BadFormat)?;
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
            // the device without `--device`: `maxicom /dev/ttyUSB0`, `maxicom tcp://host:port`
            device if !device.starts_with('-') => {
                address = Some(device.parse().map_err(ParseError::BadFormat)?);
            }

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    // the device given, else the settings' one, if any
    let address = match (address, defaults.device()) {
        (Some(address), _) => Some(address),
        (None, Some(device)) => Some(
            device
                .parse()
                .map_err(|err| ParseError::BadFormat(format!("link.device: {err}")))?,
        ),
        (None, None) => None,
    };
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
    println!("maxicom {}", env!("CARGO_PKG_VERSION"));
}

pub fn print_help() {
    println!(
        "\
maxicom — serial terminal

Usage:
  maxicom [OPTIONS] [DEVICE]

  DEVICE                   A serial port (/dev/ttyUSB0, COM3), a TCP link
                           (tcp://192.168.1.50:4000) or a file to replay
                           (file://boot.raw), same as --device

Options:
  -D, --device <DEVICE>    Device to open [default: [link] device in config.toml,
                           else the directory of the serial ports]
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
mod tests {
    //! Command line options.

    use std::path::PathBuf;

    use super::*;
    use crate::link::Address;

    /// The arguments over a `[link]` with a device, so the tests can leave it out
    fn parse(args: &[&str]) -> Result<ParseOutcome, ParseError> {
        let defaults = LinkDefaults {
            device: "/dev/ttyUSB0".to_string(),
            ..LinkDefaults::default()
        };
        parse_args_from(args.iter().map(|a| a.to_string()), &defaults)
    }

    fn restore_of(args: &[&str]) -> Option<bool> {
        match parse(args).unwrap() {
            ParseOutcome::Run { restore, .. } => restore,
            _ => panic!("not a run"),
        }
    }

    /// MAX-148: `--restore` and `--no-restore` override `sessions.restore`, the last one given
    /// wins, and without them the setting decides.
    #[test]
    fn restore_flags() {
        assert_eq!(restore_of(&[]), None);
        assert_eq!(restore_of(&["--restore"]), Some(true));
        assert_eq!(restore_of(&["--no-restore"]), Some(false));
        assert_eq!(
            restore_of(&["--restore", "-D", "x", "--no-restore"]),
            Some(false)
        );
    }

    /// MAX-154: `--cache-dir` asks for the session folder, wherever it is given.
    #[test]
    fn cache_dir_flag() {
        for args in [&["--cache-dir"][..], &["-D", "x", "--cache-dir"]] {
            let outcome = parse(args).unwrap();
            assert!(matches!(outcome, ParseOutcome::CacheDir), "{args:?}");
        }
    }

    fn address_of(args: &[&str]) -> Result<String, String> {
        match parse(args) {
            Ok(ParseOutcome::Run { address, .. }) => Ok(address.expect("a device").to_string()),
            Ok(_) => panic!("not a run"),
            Err(err) => Err(err.to_string()),
        }
    }

    /// MAX-181: the line settings default to 115200 8N1 with no flow control, the options set
    /// them in their short and long forms, minicom's names (`-D, --device`, `-b, --baudrate`)
    /// in place of `-p, --port` and `--baud`, and a missing or bad value, an unknown option
    /// and `-h` stop the run.
    /// How: no option, every line option, the device in both forms, then the refused ones.
    #[test]
    fn line_options() {
        let run = |args: &[&str]| match parse(args) {
            Ok(ParseOutcome::Run { line, .. }) => line,
            other => panic!("not a run: {other:?}"),
        };
        let line = run(&[]);
        assert_eq!(
            (line.baud, line.framing.to_string(), line.flow_control),
            (115_200, "8N1".to_string(), FlowControl::None)
        );

        let line = run(&[
            "-b",
            "9600",
            "--bits",
            "7",
            "--parity",
            "e",
            "--stop-bits",
            "2",
            "--flow",
            "hw",
        ]);
        assert_eq!(
            (line.baud, line.framing.to_string(), line.flow_control),
            (9600, "7E2".to_string(), FlowControl::RtsCts)
        );
        assert_eq!(run(&["--baudrate", "57600"]).baud, 57600);
        assert_eq!(address_of(&["--device", "COM3"]).unwrap(), "COM3");
        assert_eq!(address_of(&["-D", "COM3"]).unwrap(), "COM3");

        for args in [
            &["-b"][..],
            &["-b", "fast"],
            &["--bits", "9"],
            &["--flow", "x"],
        ] {
            assert!(address_of(args).is_err(), "{args:?}");
        }
        for old in ["--port", "-p", "--baud"] {
            let err = address_of(&[old, "1"]).unwrap_err();
            assert_eq!(err, format!("Unknown argument '{old}'."));
        }
        let help = parse(&["-h"]).unwrap();
        assert!(matches!(help, ParseOutcome::Help));
    }

    /// MAX-181: without options the link is `[link]`'s, the options over it one by one; with
    /// no device there, a named one is needed, on any system. `[link]`'s line doesn't count
    /// as options given to a TCP device.
    /// How: a full `[link]` with and without options, then an empty one, then TCP devices.
    #[test]
    fn link_defaults_under_the_options() {
        let parse_over = |defaults: &LinkDefaults, args: &[&str]| match parse_args_from(
            args.iter().map(|a| a.to_string()),
            defaults,
        ) {
            Ok(ParseOutcome::Run { address, line, .. }) => Ok((
                address.map(|address| address.to_string()),
                line.baud,
                line.framing.to_string(),
            )),
            Ok(other) => panic!("not a run: {other:?}"),
            Err(err) => Err(err.to_string()),
        };
        let run = |device: &str, baud, framing: &str| {
            Ok((Some(device.to_string()), baud, framing.to_string()))
        };
        let saved = LinkDefaults {
            device: "COM7".to_string(),
            baud: 9600,
            bits: 7,
            parity: Parity::Even,
            stop_bits: 2,
            flow: FlowControl::RtsCts,
        };
        assert_eq!(parse_over(&saved, &[]), run("COM7", 9600, "7E2"));
        assert_eq!(
            parse_over(&saved, &["-D", "COM3", "-b", "115200", "--parity", "n"]),
            run("COM3", 115_200, "7N2")
        );

        let empty = LinkDefaults::default();
        let no_device = Ok((None, 115_200, "8N1".to_string()));
        assert_eq!(parse_over(&empty, &[]), no_device);
        assert_eq!(parse_over(&empty, &["COM3"]), run("COM3", 115_200, "8N1"));

        let tcp = LinkDefaults {
            device: "tcp://192.168.1.50:4000".to_string(),
            ..saved
        };
        assert!(parse_over(&tcp, &[]).is_ok());
        assert!(parse_over(&tcp, &["-b", "9600"]).is_err());
        let bad = LinkDefaults {
            device: "tcp://nohost".to_string(),
            ..LinkDefaults::default()
        };
        let err = parse_over(&bad, &[]).unwrap_err();
        assert!(err.starts_with("link.device: "), "{err}");
    }

    /// MAX-181: minicom's getopt forms, the value glued to its option (`--baudrate=9600`,
    /// `-b9600`), for the options that take one; a flag with `=` stays unknown.
    /// How: each glued form against its spaced one, then `--restore=x`.
    #[test]
    fn glued_values() {
        let split = |args: &[&str]| split_values(args.iter().map(|a| a.to_string()));
        assert_eq!(split(&["--baudrate=9600"]), ["--baudrate", "9600"]);
        assert_eq!(
            split(&["-b9600", "-D/dev/ttyUSB1"]),
            ["-b", "9600", "-D", "/dev/ttyUSB1"]
        );
        assert_eq!(split(&["--device=tcp://h:1"]), ["--device", "tcp://h:1"]);
        assert_eq!(split(&["-D", "COM3", "-H"]), ["-D", "COM3", "-H"]);
        assert_eq!(address_of(&["-DCOM3", "-b9600"]).unwrap(), "COM3");
        let err = address_of(&["--restore=x"]).unwrap_err();
        assert_eq!(err, "Unknown argument '--restore=x'.");
    }

    /// MAX-181: minicom's `-C` (a text recording from the start), `-H` (the raw panel in hex),
    /// `-R` (the encoding, its iconv names too) and `-v` (the version); none by default.
    /// How: no option, then each one, then a bad encoding.
    #[test]
    fn minicom_options() {
        let run = |args: &[&str]| match parse(args) {
            Ok(ParseOutcome::Run {
                capture_file,
                display_hex,
                encoding,
                ..
            }) => (capture_file, display_hex, encoding),
            other => panic!("not a run: {other:?}"),
        };
        assert_eq!(run(&[]), (None, false, None));
        let file = Some(PathBuf::from("boot.cap"));
        assert_eq!(run(&["-C", "boot.cap"]), (file.clone(), false, None));
        assert_eq!(run(&["--capturefile=boot.cap"]), (file, false, None));
        assert_eq!(run(&["-H"]), (None, true, None));
        assert_eq!(run(&["--displayhex"]), (None, true, None));
        let latin1 = Some(TextEncoding::Latin1);
        assert_eq!(run(&["-R", "latin1"]), (None, false, latin1));
        assert_eq!(
            run(&["--remotecharset", "ISO-8859-1"]),
            (None, false, latin1)
        );
        assert_eq!(run(&["-RUTF-8"]).2, Some(TextEncoding::Utf8));
        assert!(
            address_of(&["-R", "ebcdic"])
                .unwrap_err()
                .contains("utf8, ascii")
        );
        for version in ["-v", "--version"] {
            let outcome = parse(&[version]).unwrap();
            assert!(matches!(outcome, ParseOutcome::Version), "{version}");
        }
    }

    /// MAX-156: the device can be given without `--device`, a TCP one with `--tcp` too, and the
    /// last one given wins.
    #[test]
    fn device_forms() {
        assert_eq!(address_of(&[]).unwrap(), "/dev/ttyUSB0");
        assert_eq!(address_of(&["/dev/ttyACM0"]).unwrap(), "/dev/ttyACM0");
        assert_eq!(address_of(&["COM3", "-b", "9600"]).unwrap(), "COM3");
        let tcp = "tcp://192.168.1.50:4000";
        assert_eq!(address_of(&[tcp]).unwrap(), tcp);
        assert_eq!(address_of(&["--tcp", "192.168.1.50:4000"]).unwrap(), tcp);
        assert_eq!(address_of(&["-D", "/dev/ttyUSB1", tcp]).unwrap(), tcp);
        assert_eq!(
            address_of(&[tcp, "-D", "/dev/ttyUSB1"]).unwrap(),
            "/dev/ttyUSB1"
        );
    }

    /// MAX-168: `--replay` is a replay address, as `file://` is, with its delay; the last
    /// device given wins over it, and a replay with no path is refused.
    /// How: the replay forms, then a device after `--replay`.
    #[test]
    fn replay_forms() {
        let run = |args: &[&str]| match parse(args) {
            Ok(ParseOutcome::Run {
                address,
                replay_delay,
                ..
            }) => (address.expect("a device"), replay_delay),
            _ => panic!("not a run"),
        };
        let replay = Address::Replay(PathBuf::from("/tmp/boot.raw"));
        assert_eq!(
            run(&["--replay", "/tmp/boot.raw"]),
            (replay.clone(), Duration::ZERO)
        );
        assert_eq!(
            run(&["-r", "/tmp/boot.raw", "--delay", "20"]),
            (replay.clone(), Duration::from_millis(20))
        );
        assert_eq!(run(&["file:///tmp/boot.raw"]).0, replay);
        assert_eq!(
            address_of(&["-r", "/tmp/boot.raw", "COM3"]).unwrap(),
            "COM3"
        );
        assert!(address_of(&["--replay", ""]).is_err());
    }

    /// MAX-156: a TCP link refuses a malformed address and the line options, which its bridge
    /// sets; a serial port takes them.
    #[test]
    fn tcp_link_refuses_line_options() {
        let tcp = "tcp://192.168.1.50:4000";
        for option in [&["-b", "9600"][..], &["--parity", "e"], &["--flow", "hw"]] {
            let args = [&[tcp][..], option].concat();
            let err = address_of(&args).unwrap_err();
            assert!(err.contains("doesn't apply to a TCP link"), "{err}");
        }
        assert!(
            address_of(&["tcp://localhost"])
                .unwrap_err()
                .contains("host:port")
        );
        assert!(address_of(&["--tcp", "host:notaport"]).is_err());
        assert!(address_of(&["/dev/ttyUSB0", "-b", "9600", "--parity", "e"]).is_ok());
    }
}
