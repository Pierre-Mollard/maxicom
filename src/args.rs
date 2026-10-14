use std::env;

use crate::serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits};

#[derive(Debug)]
pub enum ParseOutcome {
    Config(SerialConfig),
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
    let mut config = SerialConfig::default();
    let mut args = env::args().skip(1); // skip program name

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                config.port = args.next().ok_or(ParseError::MissingValue("--port"))?;
            }
            "--baud" | "-b" => {
                let val = args.next().ok_or(ParseError::MissingValue("--baud"))?;
                config.baudrate = val
                    .parse::<u32>()
                    .map_err(|_| ParseError::BadFormat(format!("Invalid baud rate '{val}'")))?;
            }
            "--bits" => {
                let val = args.next().ok_or(ParseError::MissingValue("--bits"))?;
                config.char_size = val.parse::<CharSize>().map_err(ParseError::BadFormat)?;
            }
            "--stop-bits" => {
                let val = args.next().ok_or(ParseError::MissingValue("--stop-bits"))?;
                config.stop_bits = val.parse::<StopBits>().map_err(ParseError::BadFormat)?;
            }
            "--parity" => {
                let val = args.next().ok_or(ParseError::MissingValue("--parity"))?;
                config.parity = val.parse::<Parity>().map_err(ParseError::BadFormat)?;
            }
            "--flow" => {
                let val = args.next().ok_or(ParseError::MissingValue("--flow"))?;
                config.flow_control = val.parse::<FlowControl>().map_err(ParseError::BadFormat)?;
            }
            "--help" | "-h" => {
                return Ok(ParseOutcome::Help);
            }

            unknown => {
                return Err(ParseError::UnknownArgument(unknown.to_string()));
            }
        }
    }

    Ok(ParseOutcome::Config(config))
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
  -h, --help               Print help
"
    );
}
