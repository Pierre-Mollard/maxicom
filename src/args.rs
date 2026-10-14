use std::{env, path::PathBuf};

use crate::serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits};

#[derive(Debug, Clone)]
pub enum RunMode {
    Live(SerialConfig),
    Replay { file_path: PathBuf, delay_ms: u64 },
}
#[derive(Debug)]
pub enum ParseOutcome {
    Run(RunMode),
    Help,
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
    let mut args = env::args().skip(1); // skip program name
    let mut serial_config = SerialConfig::default();
    let mut replay_file: Option<PathBuf> = None;
    let mut replay_delay: u64 = 0;

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
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    if let Some(file_path) = replay_file {
        Ok(ParseOutcome::Run(RunMode::Replay {
            file_path,
            delay_ms: replay_delay,
        }))
    } else {
        Ok(ParseOutcome::Run(RunMode::Live(serial_config)))
    }
}

pub fn print_help() {
    println!(
        "\
MaxiCom — serial terminal

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

  -h, --help               Print help
"
    );
}
