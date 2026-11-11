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
    parse_args_from(args.iter().map(|a| a.to_string()), &defaults, &[])
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

/// MAX-209: `--write-config` asks for the update, wherever it is given.
#[test]
fn write_config_flag() {
    for args in [&["--write-config"][..], &["-D", "x", "--write-config"]] {
        let outcome = parse(args).unwrap();
        assert!(matches!(outcome, ParseOutcome::WriteConfig), "{args:?}");
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
        &[],
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

/// MAX-185: a word given, or `[link]`'s device, is a saved link's name first: its address
/// with its line settings, the line options over them, put before or after it; anything
/// else is a device. `-D` is always a device. A saved TCP link refuses the line options.
/// How: a serial link at 9600 7E1 and a TCP one, given by name with and without options,
/// through `-D`, then as `[link]`'s device.
#[test]
fn saved_links_by_name() {
    let links: Vec<SavedLink> = toml::from_str::<crate::configs::LinksConfig>(
            "[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB1'\nbaud = 9600\nbits = 7\nparity = 'even'\n\
             [[link]]\nname = 'router'\naddress = 'tcp://10.0.0.1:4000'\n",
        )
        .unwrap()
        .links;
    let run = |defaults: &LinkDefaults, args: &[&str]| match parse_args_from(
        args.iter().map(|a| a.to_string()),
        defaults,
        &links,
    ) {
        Ok(ParseOutcome::Run { address, line, .. }) => Ok((
            address.map(|address| address.to_string()),
            line.baud,
            line.framing.to_string(),
        )),
        Ok(other) => panic!("not a run: {other:?}"),
        Err(err) => Err(err.to_string()),
    };
    let link = |device: &str, baud, framing: &str| {
        Ok((Some(device.to_string()), baud, framing.to_string()))
    };
    let none = LinkDefaults::default();
    assert_eq!(run(&none, &["esp32"]), link("/dev/ttyUSB1", 9600, "7E1"));
    assert_eq!(
        run(&none, &["-b", "57600", "esp32", "--bits", "8"]),
        link("/dev/ttyUSB1", 57600, "8E1")
    );
    assert_eq!(run(&none, &["-D", "esp32"]), link("esp32", 115_200, "8N1"));
    assert_eq!(run(&none, &["COM3"]), link("COM3", 115_200, "8N1"));
    assert!(run(&none, &["router"]).is_ok());
    assert!(run(&none, &["router", "-b", "9600"]).is_err());
    let by_device = LinkDefaults {
        device: "esp32".to_string(),
        ..LinkDefaults::default()
    };
    assert_eq!(run(&by_device, &[]), link("/dev/ttyUSB1", 9600, "7E1"));
}

/// MAX-206: `--config` gives the settings file, spaced or glued, the last one winning; none
/// gives none. The normal parse skips it with its value, and refuses it without one.
/// How: the pre-read on each form, then the parse with a device after it, then alone.
#[test]
fn config_file_option() {
    let file = |args: &[&str]| config_file_from(args.iter().map(|a| a.to_string()));
    assert_eq!(file(&[]), None);
    assert_eq!(file(&["-b", "9600"]), None);
    assert_eq!(
        file(&["--config", "esp.toml", "-b", "9600"]),
        Some(PathBuf::from("esp.toml"))
    );
    assert_eq!(
        file(&["--config=a.toml", "--config", "b.toml"]),
        Some(PathBuf::from("b.toml"))
    );

    assert_eq!(
        address_of(&["--config", "esp.toml", "COM3"]).unwrap(),
        "COM3"
    );
    assert!(matches!(
        parse(&["--config"]),
        Err(ParseError::MissingValue("--config"))
    ));
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

/// MAX-222: `--plot` names the plot shown at start, glued or not; none by default, and it
/// needs its name.
#[test]
fn plot_option() {
    let plot = |args: &[&str]| match parse(args) {
        Ok(ParseOutcome::Run { plot, .. }) => Ok(plot),
        Ok(other) => panic!("not a run: {other:?}"),
        Err(err) => Err(err.to_string()),
    };
    assert_eq!(plot(&[]), Ok(None));
    assert_eq!(
        plot(&["--plot", "arduino"]),
        Ok(Some("arduino".to_string()))
    );
    assert_eq!(plot(&["--plot=pid"]), Ok(Some("pid".to_string())));
    assert_eq!(
        plot(&["--plot"]),
        Err("Missing value for option '--plot'.".to_string())
    );
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
