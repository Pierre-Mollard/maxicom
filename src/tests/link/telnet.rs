//! The telnet layer alone: commands stripped from RX across reads, answers to the
//! negotiation, TX escaped.
use super::*;

const ECHO: u8 = 1; // the server echoes what it gets (RFC 857)
const NAWS: u8 = 31; // Negotiate About Window Size (RFC 1073)
const SE: u8 = 240; // Subnegotiation End: the decoder ends on any IAC x but IAC IAC
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
