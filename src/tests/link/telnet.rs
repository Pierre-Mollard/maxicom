//! The telnet layer alone: commands stripped from RX across reads, answers to the
//! negotiation, TX escaped, RFC 2217 commands and their answers.
use std::time::Duration;

use super::*;
use crate::link::serial::Framing;

const ECHO: u8 = 1; // the server echoes what it gets (RFC 857)
const NAWS: u8 = 31; // Negotiate About Window Size (RFC 1073)
const NOP: u8 = 241; // No Operation
const GA: u8 = 249; // Go Ahead: your turn, in half duplex

/// What `telnet` keeps of `bytes`, read at once
fn decode(telnet: &mut Telnet, bytes: &[u8]) -> Vec<u8> {
    let mut buf = bytes.to_vec();
    let len = telnet.decode(&mut buf);
    buf.truncate(len);
    buf
}

/// A layer whose server agreed to everything asked at the start, requests taken
fn agreed() -> Telnet {
    let mut telnet = Telnet::start();
    telnet.take_replies();
    let answers = [
        IAC,
        WILL,
        BINARY,
        IAC,
        DO,
        BINARY,
        IAC,
        WILL,
        SUPPRESS_GO_AHEAD,
        IAC,
        DO,
        SUPPRESS_GO_AHEAD,
    ];
    assert_eq!(decode(&mut telnet, &answers), b"");
    telnet
}

/// The start asks BINARY and SUPPRESS-GO-AHEAD both ways, once.
#[test]
fn start_asks_binary_and_suppress_go_ahead_both_ways() {
    let mut telnet = Telnet::start();
    assert_eq!(
        telnet.take_replies(),
        [
            IAC,
            WILL,
            BINARY,
            IAC,
            DO,
            BINARY,
            IAC,
            WILL,
            SUPPRESS_GO_AHEAD,
            IAC,
            DO,
            SUPPRESS_GO_AHEAD
        ]
    );
    assert!(telnet.take_replies().is_empty());
}

/// The server's answers to maxicom's requests aren't answered, nor are its requests for what
/// is agreed already: no loop. Both sides are then in BINARY.
#[test]
fn answers_and_repeats_are_not_answered() {
    let mut telnet = agreed();
    assert!(telnet.take_replies().is_empty());
    assert!(telnet.him_binary() && telnet.us_binary());

    decode(
        &mut telnet,
        &[IAC, WILL, BINARY, IAC, DO, SUPPRESS_GO_AHEAD],
    );
    assert!(telnet.take_replies().is_empty());
}

/// Any other option is refused, and its refusal from the server isn't answered: it is off
/// on both sides already.
#[test]
fn other_options_are_refused() {
    let mut telnet = agreed();
    decode(&mut telnet, &[IAC, WILL, ECHO, IAC, DO, NAWS]);
    assert_eq!(telnet.take_replies(), [IAC, DONT, ECHO, IAC, WONT, NAWS]);

    decode(&mut telnet, &[IAC, WONT, ECHO, IAC, DONT, NAWS]);
    assert!(telnet.take_replies().is_empty());
}

/// A server that refuses BINARY isn't answered; it can still offer it later, which is then
/// agreed.
#[test]
fn a_refused_request_is_not_answered() {
    let mut telnet = Telnet::start();
    telnet.take_replies();
    decode(&mut telnet, &[IAC, WONT, BINARY, IAC, DONT, BINARY]);
    assert!(telnet.take_replies().is_empty());
    assert!(!telnet.him_binary() && !telnet.us_binary());

    decode(&mut telnet, &[IAC, WILL, BINARY, IAC, DO, BINARY]);
    assert_eq!(telnet.take_replies(), [IAC, DO, BINARY, IAC, WILL, BINARY]);
    assert!(telnet.him_binary() && telnet.us_binary());
}

/// An agreed option the server turns off is acknowledged once.
#[test]
fn an_agreed_option_turned_off_is_acknowledged() {
    let mut telnet = agreed();
    decode(&mut telnet, &[IAC, WONT, BINARY, IAC, DONT, BINARY]);
    assert_eq!(
        telnet.take_replies(),
        [IAC, DONT, BINARY, IAC, WONT, BINARY]
    );
    assert!(!telnet.him_binary() && !telnet.us_binary());

    decode(&mut telnet, &[IAC, WONT, BINARY]);
    assert!(telnet.take_replies().is_empty());
}

/// The device's bytes come out without the commands: `IAC IAC` is a data `0xFF`,
/// one-byte commands and subnegotiations (an `IAC IAC` inside too) are dropped.
#[test]
fn commands_are_stripped() {
    let mut telnet = agreed();
    let stream = [
        &b"boot"[..],
        &[IAC, IAC, IAC, NOP, IAC, GA],
        b" ok",
        &[IAC, SB, 44, 101, IAC, IAC, 0, IAC, SE],
        b"\r\n",
        &[IAC, WILL, ECHO],
        b"$ ",
    ]
    .concat();
    assert_eq!(decode(&mut telnet, &stream), b"boot\xff ok\r\n$ ");
    assert_eq!(telnet.take_replies(), [IAC, DONT, ECHO]);
}

/// A malformed subnegotiation (`IAC` then anything but `IAC` or `SE`) ends there, and a long
/// one is dropped whole: the data after either comes out.
#[test]
fn odd_subnegotiations_end() {
    let mut telnet = agreed();
    assert_eq!(decode(&mut telnet, &[IAC, SB, 44, IAC, NOP, b'a']), b"a");

    let long = [&[IAC, SB, 44][..], &[7; 200], &[IAC, SE], b"b"].concat();
    assert_eq!(decode(&mut telnet, &long), b"b");
    assert_eq!(telnet.sub.len(), SUB_MAX);
}

/// A read can end anywhere in a command: the same stream split at every byte, and read one
/// byte at a time, gives the same bytes and replies as read at once.
/// How: before BINARY is agreed, so the `CR NUL` state is split too.
#[test]
fn commands_split_between_reads() {
    let stream = [
        &b"a\r"[..],
        &[
            NUL, IAC, IAC, IAC, WILL, ECHO, IAC, SB, 44, 1, IAC, IAC, IAC, SE,
        ],
        b"b\r\n",
        &[IAC, DO, NAWS, IAC, NOP],
        b"c",
    ]
    .concat();
    let whole = {
        let mut telnet = Telnet::start();
        (decode(&mut telnet, &stream), telnet.take_replies())
    };
    assert_eq!(whole.0, b"a\r\xffb\r\nc");

    for split in 0..=stream.len() {
        let mut telnet = Telnet::start();
        let mut data = decode(&mut telnet, &stream[..split]);
        data.extend(decode(&mut telnet, &stream[split..]));
        assert_eq!((data, telnet.take_replies()), whole, "split at {split}");
    }
    let mut telnet = Telnet::start();
    let data: Vec<u8> = (stream.iter())
        .flat_map(|&byte| decode(&mut telnet, &[byte]))
        .collect();
    assert_eq!((data, telnet.take_replies()), whole);
}

/// Outside BINARY a `CR NUL` reads as CR, `CR LF` stays; in BINARY a NUL after CR is data.
#[test]
fn cr_nul_outside_binary_only() {
    let mut telnet = Telnet::start();
    assert_eq!(decode(&mut telnet, b"a\r\0b\r\nc\r\r\0"), b"a\rb\r\nc\r\r");

    let mut telnet = agreed();
    assert_eq!(decode(&mut telnet, b"a\r\0b"), b"a\r\0b");
}

/// TX doubles a data `0xFF`; outside BINARY a CR not followed by LF goes as `CR NUL`.
/// Nothing to change borrows the bytes.
#[test]
fn tx_is_escaped() {
    let telnet = Telnet::start();
    assert_eq!(telnet.encode(b"\xffa\xff"), &b"\xff\xffa\xff\xff"[..]);
    assert_eq!(telnet.encode(b"ls\r"), &b"ls\r\0"[..]);
    assert_eq!(telnet.encode(b"ls\r\n\r"), &b"ls\r\n\r\0"[..]);
    assert!(matches!(telnet.encode(b"ls\r\n"), Cow::Borrowed(_)));

    let telnet = agreed();
    assert!(matches!(telnet.encode(b"ls\r"), Cow::Borrowed(_)));
    assert_eq!(telnet.encode(b"\r\xff"), &b"\r\xff\xff"[..]);
}

/// An RFC 2217 layer whose server agreed to everything, requests and masks taken
fn agreed_rfc2217() -> Telnet {
    let mut telnet = Telnet::rfc2217();
    telnet.take_replies();
    decode(&mut telnet, &[IAC, DO, COM_PORT_OPTION]);
    telnet.take_replies();
    telnet
}

/// The server's answer to a command: `IAC SB 44 code+100 value IAC SE`
fn answer(code: u8, value: &[u8]) -> Vec<u8> {
    [
        &[IAC, SB, COM_PORT_OPTION, code + ACK][..],
        value,
        &[IAC, SE],
    ]
    .concat()
}

/// RFC 2217 offers COM-PORT-OPTION after the telnet requests; the commands wait for the
/// server's `DO 44`, behind the masks at 0, then go straight. The bytes are pyserial's
/// client's at open (`rfc2217://`, 115200 8N1).
#[test]
fn rfc2217_holds_commands_until_the_server_agrees() {
    let mut telnet = Telnet::rfc2217();
    let requests = telnet.take_replies();
    assert_eq!(requests[..12], Telnet::start().take_replies());
    assert_eq!(requests[12..], [IAC, WILL, COM_PORT_OPTION]);

    for command in ComPort::line(&LineSettings::default()) {
        telnet.set(command);
    }
    assert!(telnet.take_replies().is_empty());

    decode(&mut telnet, &[IAC, DO, COM_PORT_OPTION]);
    #[rustfmt::skip]
    let sent = [
        IAC, SB, 44, 10, 0, IAC, SE,
        IAC, SB, 44, 11, 0, IAC, SE,
        IAC, SB, 44, 1, 0, 1, 194, 0, IAC, SE,
        IAC, SB, 44, 2, 8, IAC, SE,
        IAC, SB, 44, 3, 1, IAC, SE,
        IAC, SB, 44, 4, 1, IAC, SE,
        IAC, SB, 44, 5, 1, IAC, SE,
    ];
    assert_eq!(telnet.take_replies(), sent);

    telnet.set(ComPort::Dtr(true));
    assert_eq!(telnet.take_replies(), [IAC, SB, 44, 5, 8, IAC, SE]);
}

/// Each command's bytes, as pyserial's client sends them: a `0xFF` in the baud rate doubled.
#[test]
fn rfc2217_command_bytes() {
    let line = LineSettings {
        baud: 65535,
        framing: Framing {
            char_size: CharSize::Bits7,
            parity: Parity::Even,
            stop_bits: StopBits::Two,
        },
        flow_control: FlowControl::XonXoff,
        ..LineSettings::default()
    };
    let cases = [
        (ComPort::line(&line)[0], &[1, 0, 0, IAC, IAC, IAC, IAC][..]),
        (ComPort::line(&line)[1], &[2, 7]),
        (ComPort::line(&line)[2], &[3, 3]),
        (ComPort::line(&line)[3], &[4, 2]),
        (ComPort::line(&line)[4], &[5, 2]),
        (ComPort::Parity(Parity::Odd), &[3, 2]),
        (ComPort::Flow(FlowControl::RtsCts), &[5, 3]),
        (ComPort::from(LineControl::Break(Duration::ZERO)), &[5, 5]),
        (ComPort::Break(false), &[5, 6]),
        (ComPort::from(LineControl::Dtr(false)), &[5, 9]),
        (ComPort::from(LineControl::Rts(true)), &[5, 11]),
        (ComPort::Rts(false), &[5, 12]),
    ];
    let mut telnet = agreed_rfc2217();
    for (command, bytes) in cases {
        telnet.set(command);
        let sub = [&[IAC, SB, COM_PORT_OPTION][..], bytes, &[IAC, SE]].concat();
        assert_eq!(telnet.take_replies(), sub, "{command}");
    }
}

/// An answer with the value asked says nothing, one with another value is a warning. The
/// answers to SET-CONTROL come in order for DTR, RTS and flow alike; extra bytes after the
/// value (sredird's baud rate) and answers to nothing asked are fine.
#[test]
fn rfc2217_answers_are_checked() {
    let mut telnet = agreed_rfc2217();
    telnet.set(ComPort::Baud(115_200));
    telnet.set(ComPort::Dtr(true));
    telnet.set(ComPort::Rts(true));
    telnet.take_replies();
    let answers = [
        answer(SET_CONTROL, &[DTR_ON]),
        answer(SET_CONTROL, &[RTS_OFF]),
        answer(SET_BAUDRATE, &[0, 1, 194, 0, 0, 0, 0, 0]),
        answer(SET_BAUDRATE, &[0, 0, 37, 128]),
        answer(SET_MODEMSTATE_MASK, &[0]),
        answer(7, &[0x10]), // NOTIFY-MODEMSTATE: CTS
    ]
    .concat();
    assert_eq!(decode(&mut telnet, &answers), b"");
    assert_eq!(
        telnet.take_warnings(),
        ["the server set RTS off, asked RTS on"]
    );
    assert!(telnet.take_replies().is_empty());

    telnet.set(ComPort::Baud(115_200));
    telnet.set(ComPort::Parity(Parity::Even));
    let answers = [
        answer(SET_BAUDRATE, &[0, 0, 37, 128]),
        answer(SET_PARITY, &[4]),
    ]
    .concat();
    decode(&mut telnet, &answers);
    assert_eq!(
        telnet.take_warnings(),
        [
            "the server set 9600 baud, asked 115200 baud",
            "the server set [04], asked even parity"
        ]
    );
}

/// A server that refuses COM-PORT-OPTION drops the held commands and every later one, with
/// one warning; nothing goes back to it.
#[test]
fn rfc2217_refused() {
    let mut telnet = Telnet::rfc2217();
    telnet.take_replies();
    telnet.set(ComPort::Baud(9600));
    decode(&mut telnet, &[IAC, DONT, COM_PORT_OPTION]);
    assert!(telnet.take_replies().is_empty());
    assert_eq!(
        telnet.take_warnings(),
        ["the server refuses RFC 2217: no line settings"]
    );

    telnet.set(ComPort::Baud(9600));
    decode(&mut telnet, &[IAC, DONT, COM_PORT_OPTION]);
    assert!(telnet.take_replies().is_empty());
    assert!(telnet.take_warnings().is_empty());
}

/// A plain telnet link has no RFC 2217: commands are ignored.
#[test]
fn plain_telnet_ignores_commands() {
    let mut telnet = agreed();
    telnet.set(ComPort::Dtr(true));
    assert!(telnet.take_replies().is_empty());
}

/// A subnegotiation cut at `SUB_MAX` or ended by a malformed `IAC x` isn't read: the
/// command stays asked, and its whole answer next is checked.
#[test]
fn rfc2217_cut_answers_are_ignored() {
    let mut telnet = agreed_rfc2217();
    telnet.set(ComPort::Baud(115_200));
    let long = answer(SET_BAUDRATE, &[0; SUB_MAX]);
    let malformed = [IAC, SB, COM_PORT_OPTION, SET_BAUDRATE + ACK, 0, IAC, NOP];
    decode(&mut telnet, &[long, malformed.to_vec()].concat());
    assert!(telnet.take_warnings().is_empty());

    decode(&mut telnet, &answer(SET_BAUDRATE, &[0, 0, 37, 128]));
    assert_eq!(
        telnet.take_warnings(),
        ["the server set 9600 baud, asked 115200 baud"]
    );
}

/// What ser2net does on a pty: DTR and RTS never answered, a break off answered "on". Neither
/// is a warning: an answer is matched within its setting, the one matching drops the older
/// unanswered ones, and breaks aren't checked.
#[test]
fn rfc2217_answers_ser2net_on_a_pty() {
    let mut telnet = agreed_rfc2217();
    telnet.set(ComPort::Dtr(false));
    telnet.set(ComPort::Rts(true));
    telnet.set(ComPort::Flow(FlowControl::RtsCts));
    telnet.set(ComPort::Break(true));
    telnet.set(ComPort::Break(false));
    telnet.set(ComPort::Dtr(true));
    let answers = [
        answer(SET_CONTROL, &[RTS_CTS]),
        answer(SET_CONTROL, &[BREAK_ON]),
        answer(SET_CONTROL, &[BREAK_ON]),
        answer(SET_CONTROL, &[DTR_ON]),
    ]
    .concat();
    decode(&mut telnet, &answers);
    assert!(telnet.take_warnings().is_empty());
    assert_eq!(telnet.asked, [ComPort::Rts(true)]);
}
