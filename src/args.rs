use std::{env, path::PathBuf};

use crate::link::{
    Address,
    serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits},
    tcp::TcpDevice,
};

#[derive(Debug, Clone)]
pub enum RunMode {
    Live(SerialConfig),
    Replay { file_path: PathBuf, delay_ms: u64 },
}
/// `restore` is `--restore` / `--no-restore`, the last one given, over `capture.restore`
#[derive(Debug)]
pub enum ParseOutcome {
    Run {
        mode: RunMode,
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
    let mut serial_config = SerialConfig::default();
    let mut replay_file: Option<PathBuf> = None;
    let mut replay_delay: u64 = 0;
    let mut restore: Option<bool> = None;
    // the line options given, refused on a link whose bridge sets the line (MAX-156)
    let mut line_options: Vec<&'static str> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--replay" | "-r" => {
                let path = args
                    .next()
                    .ok_or(ParseError::MissingValue("Missing file path for --replay"))?;
                replay_file = Some(PathBuf::from(path));
            }
            "--delay" => {
                let val = args.next().ok_or(ParseError::MissingValue(
                    "Missing millisecond delay for --delay",
                ))?;
                replay_delay = val
                    .parse::<u64>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid delay '{val}'")))?;
            }
            "--port" | "-p" => {
                let port = args.next().ok_or(ParseError::MissingValue("--port"))?;
                serial_config.address = Address::parse(&port);
            }
            "--tcp" => {
                let addr = args.next().ok_or(ParseError::MissingValue("--tcp"))?;
                serial_config.address = Address::Tcp(addr);
            }
            "--baud" | "-b" => {
                line_options.push("--baud");
                let val = args.next().ok_or(ParseError::MissingValue("--baud"))?;
                serial_config.baudrate = val
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                line_options.push("--bits");
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                serial_config.char_size = val.parse::<CharSize>().map_err(ParseError::BadFormat)?;
            }
            "--stop-bits" => {
                line_options.push("--stop-bits");
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                serial_config.stop_bits = val.parse::<StopBits>().map_err(ParseError::BadFormat)?;
            }
            "--parity" => {
                line_options.push("--parity");
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                serial_config.parity = val.parse::<Parity>().map_err(ParseError::BadFormat)?;
            }
            "--flow" => {
                line_options.push("--flow");
                let val = args.next().ok_or(ParseError::MissingValue("--flow"))?;
                serial_config.flow_control =
                    val.parse::<FlowControl>().map_err(ParseError::BadFormat)?;
            }
            "--restore" => restore = Some(true),
            "--no-restore" => restore = Some(false),
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }
            "--cache-dir" => return Ok(ParseOutcome::CacheDir),
            // the device without `--port`: `maxicom /dev/ttyUSB0`, `maxicom tcp://host:port`
            device if !device.starts_with('-') => serial_config.address = Address::parse(device),

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    let mode = if let Some(file_path) = replay_file {
        RunMode::Replay {
            file_path,
            delay_ms: replay_delay,
        }
    } else {
        if let Address::Tcp(addr) = &serial_config.address {
            // a malformed address would never connect
            TcpDevice::check_address(addr).map_err(|e| ParseError::BadFormat(e.to_string()))?;
            if let Some(option) = line_options.first() {
                return Err(ParseError::BadFormat(format!(
                    "{option} doesn't apply to a TCP link: its bridge sets the line"
                )));
            }
        }
        RunMode::Live(serial_config)
    };
    Ok(ParseOutcome::Run { mode, restore })
}

pub fn print_help() {
    println!(
        "\
maxicom — serial terminal

Usage:
  maxicom [OPTIONS] [DEVICE]

  DEVICE                   A serial port (/dev/ttyUSB0, COM3) or a TCP link
                           (tcp://192.168.1.50:4000), same as --port

Options:
  -p, --port <DEVICE>      Device to open [default: /dev/ttyUSB0]
      --tcp <HOST:PORT>    A device over TCP, same as tcp://HOST:PORT

  Line settings, for a serial port (a TCP link's bridge sets them):
  -b, --baud <RATE>        Baud rate [default: 115200]
      --bits <5|6|7|8>     Character data bits [default: 8]
      --stop-bits <1|2>    Stop bits [default: 1]
      --parity <n|e|o>     Parity: none, even, odd [default: none]
      --flow <none|hw|sw>  Flow control: none, rtscts (hw), xonxoff (sw) [default: none]

  -r, --replay <PATH>      File to replay raw bytes to terminal instead of serial device
      --delay <ms>         Time to wait between chunks in millisecond

      --restore            Restore the last session on this device into the history
      --no-restore         Start with an empty history, whatever capture.restore says
      --cache-dir          Print the session backups folder and exit

  -h, --help               Print help
"
    );
}

#[cfg(test)]
mod tests {
    //! Command line options.

    use super::*;

    fn restore_of(args: &[&str]) -> Option<bool> {
        match parse_args_from(args.iter().map(|a| a.to_string())).unwrap() {
            ParseOutcome::Run { restore, .. } => restore,
            _ => panic!("not a run"),
        }
    }

    /// MAX-148: `--restore` and `--no-restore` override `capture.restore`, the last one given
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

    fn live_port(args: &[&str]) -> Result<String, String> {
        match parse_args_from(args.iter().map(|a| a.to_string())) {
            Ok(ParseOutcome::Run {
                mode: RunMode::Live(config),
                ..
            }) => Ok(config.address.to_string()),
            Ok(_) => panic!("not a live run"),
            Err(err) => Err(err.to_string()),
        }
    }

    /// MAX-156: the device can be given without `--port`, a TCP one with `--tcp` too, and the
    /// last one given wins.
    #[test]
    fn device_forms() {
        assert_eq!(live_port(&[]).unwrap(), "/dev/ttyUSB0");
        assert_eq!(live_port(&["/dev/ttyACM0"]).unwrap(), "/dev/ttyACM0");
        assert_eq!(live_port(&["COM3", "-b", "9600"]).unwrap(), "COM3");
        let tcp = "tcp://192.168.1.50:4000";
        assert_eq!(live_port(&[tcp]).unwrap(), tcp);
        assert_eq!(live_port(&["--tcp", "192.168.1.50:4000"]).unwrap(), tcp);
        assert_eq!(live_port(&["-p", "/dev/ttyUSB1", tcp]).unwrap(), tcp);
        assert_eq!(
            live_port(&[tcp, "-p", "/dev/ttyUSB1"]).unwrap(),
            "/dev/ttyUSB1"
        );
    }

    /// MAX-156: a TCP link refuses a malformed address and the line options, which its bridge
    /// sets; a serial port takes them.
    #[test]
    fn tcp_link_refuses_line_options() {
        let tcp = "tcp://192.168.1.50:4000";
        for option in [&["-b", "9600"][..], &["--parity", "e"], &["--flow", "hw"]] {
            let args = [&[tcp][..], option].concat();
            let err = live_port(&args).unwrap_err();
            assert!(err.contains("doesn't apply to a TCP link"), "{err}");
        }
        assert!(
            live_port(&["tcp://localhost"])
                .unwrap_err()
                .contains("host:port")
        );
        assert!(live_port(&["--tcp", "host:notaport"]).is_err());
        assert!(live_port(&["/dev/ttyUSB0", "-b", "9600", "--parity", "e"]).is_ok());
    }
}
