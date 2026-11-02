use std::{env, time::Duration};

use crate::link::{
    LinkConfig, LinkKind,
    serial::{CharSize, FlowControl, Parity, StopBits},
};

/// `link_config` is the link to open, a replay for `--replay` (MAX-168); `replay_delay` is
/// `--delay`, for a replay only. `restore` is `--restore` / `--no-restore`, the last one given,
/// over `sessions.restore`
#[derive(Debug)]
pub enum ParseOutcome {
    Run {
        link_config: LinkConfig,
        replay_delay: Duration,
        restore: Option<bool>,
    },
    Help,
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

pub fn parse_args() -> Result<ParseOutcome, ParseError> {
    parse_args_from(env::args().skip(1)) // skip program name
}

fn parse_args_from(args: impl IntoIterator<Item = String>) -> Result<ParseOutcome, ParseError> {
    let mut args = args.into_iter();
    let mut link_config = LinkConfig::default();
    let mut replay_delay = Duration::ZERO;
    let mut restore: Option<bool> = None;
    // the line options given, refused on a link whose bridge sets the line (MAX-156)
    let mut line_options: Vec<&'static str> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--replay" | "-r" => {
                let path = args
                    .next()
                    .ok_or(ParseError::MissingValue("Missing file path for --replay"))?;
                link_config.address = format!("file://{path}")
                    .parse()
                    .map_err(ParseError::BadFormat)?;
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
            "--port" | "-p" => {
                let port = args.next().ok_or(ParseError::MissingValue("--port"))?;
                link_config.address = port.parse().map_err(ParseError::BadFormat)?;
            }
            "--tcp" => {
                let addr = args.next().ok_or(ParseError::MissingValue("--tcp"))?;
                link_config.address = format!("tcp://{addr}")
                    .parse()
                    .map_err(ParseError::BadFormat)?;
            }
            "--baud" | "-b" => {
                line_options.push("--baud");
                let val = args.next().ok_or(ParseError::MissingValue("--baud"))?;
                link_config.line.baud = val
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                line_options.push("--bits");
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                link_config.line.framing.char_size =
                    val.parse::<CharSize>().map_err(ParseError::BadFormat)?;
            }
            "--stop-bits" => {
                line_options.push("--stop-bits");
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                link_config.line.framing.stop_bits =
                    val.parse::<StopBits>().map_err(ParseError::BadFormat)?;
            }
            "--parity" => {
                line_options.push("--parity");
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                link_config.line.framing.parity =
                    val.parse::<Parity>().map_err(ParseError::BadFormat)?;
            }
            "--flow" => {
                line_options.push("--flow");
                let val = args.next().ok_or(ParseError::MissingValue("--flow"))?;
                link_config.line.flow_control =
                    val.parse::<FlowControl>().map_err(ParseError::BadFormat)?;
            }
            "--restore" => restore = Some(true),
            "--no-restore" => restore = Some(false),
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }
            "--cache-dir" => return Ok(ParseOutcome::CacheDir),
            // the device without `--port`: `maxicom /dev/ttyUSB0`, `maxicom tcp://host:port`
            device if !device.starts_with('-') => {
                link_config.address = device.parse().map_err(ParseError::BadFormat)?;
            }

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    if link_config.address.kind() == LinkKind::Tcp
        && let Some(option) = line_options.first()
    {
        return Err(ParseError::BadFormat(format!(
            "{option} doesn't apply to a TCP link: its bridge sets the line"
        )));
    }
    Ok(ParseOutcome::Run {
        link_config,
        replay_delay,
        restore,
    })
}

pub fn print_help() {
    println!(
        "\
maxicom — serial terminal

Usage:
  maxicom [OPTIONS] [DEVICE]

  DEVICE                   A serial port (/dev/ttyUSB0, COM3), a TCP link
                           (tcp://192.168.1.50:4000) or a file to replay
                           (file://boot.raw), same as --port

Options:
  -p, --port <DEVICE>      Device to open [default: /dev/ttyUSB0]
      --tcp <HOST:PORT>    A device over TCP, same as tcp://HOST:PORT

  Line settings, for a serial port (a TCP link's bridge sets them):
  -b, --baud <RATE>        Baud rate [default: 115200]
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

  -h, --help               Print help
"
    );
}

#[cfg(test)]
mod tests {
    //! Command line options.

    use std::path::PathBuf;

    use super::*;
    use crate::link::Address;

    fn restore_of(args: &[&str]) -> Option<bool> {
        match parse_args_from(args.iter().map(|a| a.to_string())).unwrap() {
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
            restore_of(&["--restore", "-p", "x", "--no-restore"]),
            Some(false)
        );
    }

    /// MAX-154: `--cache-dir` asks for the session folder, wherever it is given.
    #[test]
    fn cache_dir_flag() {
        for args in [&["--cache-dir"][..], &["-p", "x", "--cache-dir"]] {
            let outcome = parse_args_from(args.iter().map(|a| a.to_string())).unwrap();
            assert!(matches!(outcome, ParseOutcome::CacheDir), "{args:?}");
        }
    }

    fn address_of(args: &[&str]) -> Result<String, String> {
        match parse_args_from(args.iter().map(|a| a.to_string())) {
            Ok(ParseOutcome::Run { link_config, .. }) => Ok(link_config.address.to_string()),
            Ok(_) => panic!("not a run"),
            Err(err) => Err(err.to_string()),
        }
    }

    /// MAX-156: the device can be given without `--port`, a TCP one with `--tcp` too, and the
    /// last one given wins.
    #[test]
    fn device_forms() {
        assert_eq!(address_of(&[]).unwrap(), "/dev/ttyUSB0");
        assert_eq!(address_of(&["/dev/ttyACM0"]).unwrap(), "/dev/ttyACM0");
        assert_eq!(address_of(&["COM3", "-b", "9600"]).unwrap(), "COM3");
        let tcp = "tcp://192.168.1.50:4000";
        assert_eq!(address_of(&[tcp]).unwrap(), tcp);
        assert_eq!(address_of(&["--tcp", "192.168.1.50:4000"]).unwrap(), tcp);
        assert_eq!(address_of(&["-p", "/dev/ttyUSB1", tcp]).unwrap(), tcp);
        assert_eq!(
            address_of(&[tcp, "-p", "/dev/ttyUSB1"]).unwrap(),
            "/dev/ttyUSB1"
        );
    }

    /// MAX-168: `--replay` is a replay address, as `file://` is, with its delay; the last
    /// device given wins over it, and a replay with no path is refused.
    /// How: the replay forms, then a device after `--replay`.
    #[test]
    fn replay_forms() {
        let run = |args: &[&str]| match parse_args_from(args.iter().map(|a| a.to_string())) {
            Ok(ParseOutcome::Run {
                link_config,
                replay_delay,
                ..
            }) => (link_config.address, replay_delay),
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
