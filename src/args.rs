use std::{env, path::PathBuf};

use crate::serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits};

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
                serial_config.port = args.next().ok_or(ParseError::MissingValue("--port"))?;
            }
            "--baud" | "-b" => {
                let val = args.next().ok_or(ParseError::MissingValue("--baud"))?;
                serial_config.baudrate = val
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                serial_config.char_size = val.parse::<CharSize>().map_err(ParseError::BadFormat)?;
            }
            "--stop-bits" => {
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                serial_config.stop_bits = val.parse::<StopBits>().map_err(ParseError::BadFormat)?;
            }
            "--parity" => {
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                serial_config.parity = val.parse::<Parity>().map_err(ParseError::BadFormat)?;
            }
            "--flow" => {
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
        RunMode::Live(serial_config)
    };
    Ok(ParseOutcome::Run { mode, restore })
}

pub fn print_help() {
    println!(
        "\
maxicom — serial terminal

Usage:
  maxicom [OPTIONS]

Options:
  -p, --port <DEVICE>      Serial device path [default: /dev/ttyUSB0]
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
}
