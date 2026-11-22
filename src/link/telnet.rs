//! Telnet (RFC 854) on a TCP link (MAX-161): the bytes are the device's, but `0xFF` (IAC)
//! starts a command. A command is `IAC verb option` for the negotiation (`WILL`, `WONT`, `DO`,
//! `DONT`), `IAC SB option … IAC SE` for an option's parameters, or `IAC x` alone (`NOP`,
//! `GA`…). A data `0xFF` travels as `IAC IAC`, and outside BINARY a CR travels as `CR NUL` or
//! `CR LF`. Pure: no I/O, the TCP device feeds it what it reads and sends what it gives.
//!
//! RFC 2217 is one more option on top, COM-PORT-OPTION: the client sets the server's serial
//! port with `IAC SB 44 <command> <value> IAC SE`, and the server answers each with
//! `<command + 100>` and the value it set
use std::{
    borrow::Cow,
    collections::VecDeque,
    fmt::{self, Display},
    mem,
};

use super::serial::{CharSize, FlowControl, LineControl, LineSettings, Parity, StopBits};

// the commands (RFC 854)
const IAC: u8 = 255; // Interpret As Command: a command follows, or a data 0xFF if IAC again
const DONT: u8 = 254; // asks the other end to stop doing an option, or refuses its WILL
const DO: u8 = 253; // asks the other end to do an option, or agrees to its WILL
const WONT: u8 = 252; // won't do an option (or stops), refusing a DO
const WILL: u8 = 251; // will do an option, offering it or agreeing to a DO
const SB: u8 = 250; // Subnegotiation Begin: an option's parameters follow
const SE: u8 = 240; // Subnegotiation End, after an IAC
// the Network Virtual Terminal's line end: CR LF, or CR NUL for a CR alone
const CR: u8 = b'\r'; // Carriage Return
const LF: u8 = b'\n'; // Line Feed
const NUL: u8 = 0;

// the options
const BINARY: u8 = 0; // Binary Transmission (RFC 856): 8-bit bytes as they are, no CR NUL
const SUPPRESS_GO_AHEAD: u8 = 3; // RFC 858: no GA after each turn, full duplex
const COM_PORT_OPTION: u8 = 44; // RFC 2217: the server's serial port set by the client

// the COM-PORT-OPTION commands, client to server (RFC 2217)
const SET_BAUDRATE: u8 = 1; // 4 bytes, big-endian
const SET_DATASIZE: u8 = 2; // 5 to 8
const SET_PARITY: u8 = 3; // 1 none, 2 odd, 3 even (4 mark, 5 space)
const SET_STOPSIZE: u8 = 4; // 1, 2 (3 is 1.5)
const SET_CONTROL: u8 = 5; // flow control, BREAK, DTR or RTS: one value each
const SET_LINESTATE_MASK: u8 = 10; // which line state changes the server notifies
const SET_MODEMSTATE_MASK: u8 = 11; // which modem line changes (CTS, DSR, CD, RI) it notifies
const ACK: u8 = 100; // the server answers a command with its code + 100
// the SET-CONTROL values
const NO_FLOW: u8 = 1;
const XON_XOFF: u8 = 2; // software flow control
const RTS_CTS: u8 = 3; // hardware flow control
const BREAK_ON: u8 = 5;
const BREAK_OFF: u8 = 6;
const DTR_ON: u8 = 8;
const DTR_OFF: u8 = 9;
const RTS_ON: u8 = 11;
const RTS_OFF: u8 = 12;

/// The longest subnegotiation kept: what follows is dropped until its `IAC SE`, and the cut
/// one is ignored
const SUB_MAX: usize = 64;
/// The most commands waiting for their answer: a server that never answers drops the oldest
const ASKED_MAX: usize = 16;

/// Where an option stands on one side, as RFC 1143 counts it without WantNo: maxicom never
/// turns an option off. WantYes is a request sent and not answered yet, so its answer is not
/// answered again, which is what keeps two telnet ends from looping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    No,
    WantYes,
    Yes,
}

/// An option maxicom takes, and where it stands on both sides: `us` for what maxicom does
/// (`WILL` / `DO` from the server), `him` for what the server does
#[derive(Debug, Clone, Copy)]
struct Negotiated {
    code: u8,
    us: Side,
    him: Side,
}

/// Where the RX parser is, kept across reads: a command can be split between two
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rx {
    Data,
    /// After a data CR, outside BINARY: a NUL next is dropped
    Cr,
    Iac,
    /// After `IAC` and a negotiation verb, the option next
    Verb(u8),
    Sub,
    SubIac,
}

/// The telnet layer of a link: options asked at the start, answers to the server's, RX
/// commands stripped and TX escaped
pub struct Telnet {
    options: Vec<Negotiated>,
    rx: Rx,
    sub: Vec<u8>,
    /// The subnegotiation went past `SUB_MAX`
    sub_cut: bool,
    replies: Vec<u8>,
    /// COM-PORT-OPTION commands held until the server's `DO 44`
    held: Vec<u8>,
    /// The commands sent, oldest first, until the server answers them
    asked: VecDeque<ComPort>,
    warnings: Vec<String>,
}

impl Telnet {
    /// Asks BINARY and SUPPRESS-GO-AHEAD both ways: bytes as they are, and no half-duplex
    /// go-ahead. The requests wait in `take_replies`
    pub fn start() -> Self {
        let mut telnet = Self {
            options: Vec::new(),
            rx: Rx::Data,
            sub: Vec::new(),
            sub_cut: false,
            replies: Vec::new(),
            held: Vec::new(),
            asked: VecDeque::new(),
            warnings: Vec::new(),
        };
        for code in [BINARY, SUPPRESS_GO_AHEAD] {
            telnet.options.push(Negotiated {
                code,
                us: Side::WantYes,
                him: Side::WantYes,
            });
            telnet
                .replies
                .extend_from_slice(&[IAC, WILL, code, IAC, DO, code]);
        }
        telnet
    }

    /// `start` plus RFC 2217: offers COM-PORT-OPTION (`WILL 44`), and holds the masks at 0 so
    /// the server notifies no line or modem state change: they would only be dropped
    pub fn rfc2217() -> Self {
        let mut telnet = Self::start();
        telnet.options.push(Negotiated {
            code: COM_PORT_OPTION,
            us: Side::WantYes,
            him: Side::No,
        });
        (telnet.replies).extend_from_slice(&[IAC, WILL, COM_PORT_OPTION]);
        for mask in [SET_LINESTATE_MASK, SET_MODEMSTATE_MASK] {
            push_sub(&mut telnet.held, mask, &[0]);
        }
        telnet
    }

    /// Sends a COM-PORT-OPTION command, or holds it until the server agrees to the option.
    /// Nothing waits for its answer: one that differs shows in `take_warnings`, but a break's
    /// isn't checked (ser2net answers a break off with "on"). Ignored on a link without
    /// RFC 2217, or once the server refused it
    pub fn set(&mut self, command: ComPort) {
        let out = match self.side(COM_PORT_OPTION, true) {
            Side::Yes => &mut self.replies,
            Side::WantYes => &mut self.held,
            Side::No => return,
        };
        let (code, value) = command.encode();
        push_sub(out, code, &value);
        if matches!(command, ComPort::Break(_)) {
            return;
        }
        if self.asked.len() == ASKED_MAX {
            self.asked.pop_front();
        }
        self.asked.push_back(command);
    }

    /// Whether the link speaks RFC 2217, the server agreeing or not answered yet
    pub fn has_com_port(&self) -> bool {
        self.side(COM_PORT_OPTION, true) != Side::No
    }

    /// What went wrong with RFC 2217: the server refused it, or set another value than asked
    pub fn take_warnings(&mut self) -> Vec<String> {
        mem::take(&mut self.warnings)
    }

    /// What to send the server: the requests at the start, the answers to its own. Never
    /// shown, recorded or counted as TX
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Strips the commands from `buf`, in place: the device's bytes move to the front, and
    /// their count is returned. Answers to the server's requests wait in `take_replies`
    pub fn decode(&mut self, buf: &mut [u8]) -> usize {
        let mut len = 0;
        for i in 0..buf.len() {
            let byte = buf[i];
            let data = match self.rx {
                Rx::Data | Rx::Cr if byte == IAC => {
                    self.rx = Rx::Iac;
                    None
                }
                Rx::Cr if byte == NUL => {
                    self.rx = Rx::Data;
                    None
                }
                Rx::Data | Rx::Cr => {
                    self.rx = if byte == CR && !self.him_binary() {
                        Rx::Cr
                    } else {
                        Rx::Data
                    };
                    Some(byte)
                }
                Rx::Iac => {
                    self.rx = match byte {
                        WILL | WONT | DO | DONT => Rx::Verb(byte),
                        SB => {
                            self.sub.clear();
                            self.sub_cut = false;
                            Rx::Sub
                        }
                        _ => Rx::Data,
                    };
                    // IAC IAC is a data 0xFF; NOP, GA and the rest mean nothing here
                    (byte == IAC).then_some(IAC)
                }
                Rx::Verb(verb) => {
                    self.rx = Rx::Data;
                    self.negotiate(verb, byte);
                    None
                }
                Rx::Sub if byte == IAC => {
                    self.rx = Rx::SubIac;
                    None
                }
                Rx::Sub => {
                    self.push_sub(byte);
                    None
                }
                Rx::SubIac if byte == IAC => {
                    self.rx = Rx::Sub;
                    self.push_sub(IAC);
                    None
                }
                Rx::SubIac if byte == SE => {
                    self.rx = Rx::Data;
                    self.subnegotiated();
                    None
                }
                // anything else after IAC is malformed: it ends there, ignored
                Rx::SubIac => {
                    self.rx = Rx::Data;
                    None
                }
            };
            if let Some(byte) = data {
                buf[len] = byte;
                len += 1;
            }
        }
        len
    }

    /// `bytes` as they go to the server: a data `0xFF` doubled, and outside BINARY a CR not
    /// followed by LF sent as `CR NUL`. Borrowed when nothing changes
    pub fn encode<'a>(&self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        let binary = self.us_binary();
        let escape = |i: usize| match bytes[i] {
            IAC => Some(IAC),
            CR if !binary && bytes.get(i + 1) != Some(&LF) => Some(NUL),
            _ => None,
        };
        if (0..bytes.len()).all(|i| escape(i).is_none()) {
            return Cow::Borrowed(bytes);
        }
        let mut encoded = Vec::with_capacity(bytes.len() + 8);
        for (i, &byte) in bytes.iter().enumerate() {
            encoded.push(byte);
            encoded.extend(escape(i));
        }
        Cow::Owned(encoded)
    }

    /// Answers `IAC verb code`: an option maxicom takes is agreed once, an answer to its own
    /// request isn't answered, any other option is refused
    fn negotiate(&mut self, verb: u8, code: u8) {
        let Some(option) = self.options.iter_mut().find(|o| o.code == code) else {
            match verb {
                WILL => self.replies.extend_from_slice(&[IAC, DONT, code]),
                DO => self.replies.extend_from_slice(&[IAC, WONT, code]),
                // already off on both sides
                _ => {}
            }
            return;
        };
        let (side, yes, no) = match verb {
            WILL | WONT => (&mut option.him, DO, DONT),
            _ => (&mut option.us, WILL, WONT),
        };
        let agreed = matches!(verb, WILL | DO);
        let was = *side;
        let reply = match (was, agreed) {
            (Side::No, true) => Some(yes),
            (Side::Yes, false) => Some(no),
            // the answer to maxicom's request, or nothing new
            _ => None,
        };
        *side = if agreed { Side::Yes } else { Side::No };
        if let Some(reply) = reply {
            self.replies.extend_from_slice(&[IAC, reply, code]);
        }
        if code == COM_PORT_OPTION && matches!(verb, DO | DONT) {
            self.com_port_answered(was, agreed);
        }
    }

    /// The server agreed to RFC 2217: the held commands go. Or it refused or turned it off:
    /// they are dropped, and so is every command after
    fn com_port_answered(&mut self, was: Side, agreed: bool) {
        if agreed {
            self.replies.append(&mut self.held);
        } else if was != Side::No {
            self.held.clear();
            self.asked.clear();
            (self.warnings).push("the server refuses RFC 2217: no line settings".to_owned());
        }
    }

    fn push_sub(&mut self, byte: u8) {
        if self.sub.len() < SUB_MAX {
            self.sub.push(byte);
        } else {
            self.sub_cut = true;
        }
    }

    /// A whole subnegotiation came: the answer to a COM-PORT-OPTION command, checked against
    /// the commands of its setting (flow, DTR and RTS share SET-CONTROL), the rest ignored
    /// (notifications, other options). A server may never answer some (ser2net on a pty: DTR
    /// and RTS), so the answer matching one also drops the older ones of the setting; one
    /// matching none is a warning against the oldest
    fn subnegotiated(&mut self) {
        let [COM_PORT_OPTION, answer, ref value @ ..] = self.sub[..] else {
            return;
        };
        if self.sub_cut {
            return;
        }
        let Some(code) = answer.checked_sub(ACK) else {
            return;
        };
        let got = ComPort::decode(code, value);
        let setting = |c: &ComPort| {
            c.encode().0 == code
                && got.is_none_or(|got| mem::discriminant(&got) == mem::discriminant(c))
        };
        // the value can come with extra bytes (sredird's baud rate): its start is compared
        let matched = |c: &ComPort| setting(c) && value.starts_with(&c.encode().1);
        if let Some(i) = self.asked.iter().position(matched) {
            let mut index = 0;
            self.asked.retain(|c| {
                index += 1;
                index > i + 1 || !setting(c)
            });
        } else if let Some(i) = self.asked.iter().position(setting) {
            let asked = self.asked.remove(i).expect("position is in the queue");
            let got = got.map_or_else(|| format!("{value:02x?}"), |got| got.to_string());
            (self.warnings).push(format!("the server set {got}, asked {asked}"));
        }
    }

    fn side(&self, code: u8, us: bool) -> Side {
        (self.options.iter().find(|o| o.code == code))
            .map_or(Side::No, |o| if us { o.us } else { o.him })
    }

    /// Whether the server sends its bytes as they are, without `CR NUL`
    fn him_binary(&self) -> bool {
        self.side(BINARY, false) == Side::Yes
    }

    /// Whether maxicom sends its bytes as they are, without `CR NUL`
    fn us_binary(&self) -> bool {
        self.side(BINARY, true) == Side::Yes
    }
}

/// `IAC SB 44 code value IAC SE` at the end of `out`, a `0xFF` in the value doubled
fn push_sub(out: &mut Vec<u8>, code: u8, value: &[u8]) {
    out.extend_from_slice(&[IAC, SB, COM_PORT_OPTION, code]);
    for &byte in value {
        out.push(byte);
        if byte == IAC {
            out.push(IAC);
        }
    }
    out.extend_from_slice(&[IAC, SE]);
}

/// A COM-PORT-OPTION command (RFC 2217): one setting of the server's serial port
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComPort {
    Baud(u32),
    CharSize(CharSize),
    Parity(Parity),
    StopBits(StopBits),
    Flow(FlowControl),
    Break(bool),
    Dtr(bool),
    Rts(bool),
}

impl ComPort {
    /// The line settings of `line`, in pyserial's order; DTR and RTS are the caller's
    pub fn line(line: &LineSettings) -> [Self; 5] {
        [
            Self::Baud(line.baud),
            Self::CharSize(line.framing.char_size),
            Self::Parity(line.framing.parity),
            Self::StopBits(line.framing.stop_bits),
            Self::Flow(line.flow_control),
        ]
    }

    /// The command code and its value
    fn encode(self) -> (u8, Vec<u8>) {
        let on_off = |on, yes, no| (SET_CONTROL, vec![if on { yes } else { no }]);
        match self {
            Self::Baud(baud) => (SET_BAUDRATE, baud.to_be_bytes().to_vec()),
            Self::CharSize(size) => (
                SET_DATASIZE,
                vec![match size {
                    CharSize::Bits5 => 5,
                    CharSize::Bits6 => 6,
                    CharSize::Bits7 => 7,
                    CharSize::Bits8 => 8,
                }],
            ),
            Self::Parity(parity) => (
                SET_PARITY,
                vec![match parity {
                    Parity::None => 1,
                    Parity::Odd => 2,
                    Parity::Even => 3,
                }],
            ),
            Self::StopBits(stop) => (
                SET_STOPSIZE,
                vec![match stop {
                    StopBits::One => 1,
                    StopBits::Two => 2,
                }],
            ),
            Self::Flow(flow) => (
                SET_CONTROL,
                vec![match flow {
                    FlowControl::None => NO_FLOW,
                    FlowControl::XonXoff => XON_XOFF,
                    FlowControl::RtsCts => RTS_CTS,
                }],
            ),
            Self::Break(on) => on_off(on, BREAK_ON, BREAK_OFF),
            Self::Dtr(on) => on_off(on, DTR_ON, DTR_OFF),
            Self::Rts(on) => on_off(on, RTS_ON, RTS_OFF),
        }
    }

    /// The setting a server's answer carries, if maxicom has a name for it (not mark parity,
    /// 1.5 stop bits…)
    fn decode(code: u8, value: &[u8]) -> Option<Self> {
        if code == SET_BAUDRATE {
            let baud = value.first_chunk::<4>()?;
            return Some(Self::Baud(u32::from_be_bytes(*baud)));
        }
        let command = match (code, *value.first()?) {
            (SET_DATASIZE, 5) => Self::CharSize(CharSize::Bits5),
            (SET_DATASIZE, 6) => Self::CharSize(CharSize::Bits6),
            (SET_DATASIZE, 7) => Self::CharSize(CharSize::Bits7),
            (SET_DATASIZE, 8) => Self::CharSize(CharSize::Bits8),
            (SET_PARITY, 1) => Self::Parity(Parity::None),
            (SET_PARITY, 2) => Self::Parity(Parity::Odd),
            (SET_PARITY, 3) => Self::Parity(Parity::Even),
            (SET_STOPSIZE, 1) => Self::StopBits(StopBits::One),
            (SET_STOPSIZE, 2) => Self::StopBits(StopBits::Two),
            (SET_CONTROL, NO_FLOW) => Self::Flow(FlowControl::None),
            (SET_CONTROL, XON_XOFF) => Self::Flow(FlowControl::XonXoff),
            (SET_CONTROL, RTS_CTS) => Self::Flow(FlowControl::RtsCts),
            (SET_CONTROL, BREAK_ON) => Self::Break(true),
            (SET_CONTROL, BREAK_OFF) => Self::Break(false),
            (SET_CONTROL, DTR_ON) => Self::Dtr(true),
            (SET_CONTROL, DTR_OFF) => Self::Dtr(false),
            (SET_CONTROL, RTS_ON) => Self::Rts(true),
            (SET_CONTROL, RTS_OFF) => Self::Rts(false),
            _ => return None,
        };
        Some(command)
    }
}

/// A break starts with `Break(true)`; ending it is the caller's, `Break(false)`
impl From<LineControl> for ComPort {
    fn from(control: LineControl) -> Self {
        match control {
            LineControl::Dtr(on) => Self::Dtr(on),
            LineControl::Rts(on) => Self::Rts(on),
            LineControl::Break(_) => Self::Break(true),
        }
    }
}

/// `115200 baud`, `7 data bits`, `even parity`, `DTR on`…
impl Display for ComPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on_off = |on| if on { "on" } else { "off" };
        match self {
            Self::Baud(baud) => write!(f, "{baud} baud"),
            Self::CharSize(_) => write!(f, "{} data bits", self.encode().1[0]),
            Self::Parity(parity) => write!(f, "{} parity", format!("{parity:?}").to_lowercase()),
            Self::StopBits(StopBits::One) => write!(f, "1 stop bit"),
            Self::StopBits(StopBits::Two) => write!(f, "2 stop bits"),
            Self::Flow(FlowControl::None) => write!(f, "no flow control"),
            Self::Flow(FlowControl::XonXoff) => write!(f, "XON/XOFF flow control"),
            Self::Flow(FlowControl::RtsCts) => write!(f, "RTS/CTS flow control"),
            Self::Break(on) => write!(f, "break {}", on_off(*on)),
            Self::Dtr(on) => write!(f, "DTR {}", on_off(*on)),
            Self::Rts(on) => write!(f, "RTS {}", on_off(*on)),
        }
    }
}

#[cfg(test)]
#[path = "../tests/link/telnet.rs"]
mod tests;
