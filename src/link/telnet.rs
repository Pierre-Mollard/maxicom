//! Telnet (RFC 854) on a TCP link (MAX-161): the bytes are the device's, but `0xFF` (IAC)
//! starts a command. A command is `IAC verb option` for the negotiation (`WILL`, `WONT`, `DO`,
//! `DONT`), `IAC SB option … IAC SE` for an option's parameters, or `IAC x` alone (`NOP`,
//! `GA`…). A data `0xFF` travels as `IAC IAC`, and outside BINARY a CR travels as `CR NUL` or
//! `CR LF`. Pure: no I/O, the TCP device feeds it what it reads and sends what it gives
use std::borrow::Cow;

// the commands (RFC 854)
const IAC: u8 = 255; // Interpret As Command: a command follows, or a data 0xFF if IAC again
const DONT: u8 = 254; // asks the other end to stop doing an option, or refuses its WILL
const DO: u8 = 253; // asks the other end to do an option, or agrees to its WILL
const WONT: u8 = 252; // won't do an option (or stops), refusing a DO
const WILL: u8 = 251; // will do an option, offering it or agreeing to a DO
const SB: u8 = 250; // Subnegotiation Begin: an option's parameters follow
// the Network Virtual Terminal's line end: CR LF, or CR NUL for a CR alone
const CR: u8 = b'\r'; // Carriage Return
const LF: u8 = b'\n'; // Line Feed
const NUL: u8 = 0;

// the options
const BINARY: u8 = 0; // Binary Transmission (RFC 856): 8-bit bytes as they are, no CR NUL
const SUPPRESS_GO_AHEAD: u8 = 3; // RFC 858: no GA after each turn, full duplex

/// The longest subnegotiation kept: what follows is dropped until its `IAC SE`
const SUB_MAX: usize = 64;

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
    replies: Vec<u8>,
}

impl Telnet {
    /// Asks BINARY and SUPPRESS-GO-AHEAD both ways: bytes as they are, and no half-duplex
    /// go-ahead. The requests wait in `take_replies`
    pub fn start() -> Self {
        let mut telnet = Self {
            options: Vec::new(),
            rx: Rx::Data,
            sub: Vec::new(),
            replies: Vec::new(),
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
                // IAC SE ends it; anything else after IAC is malformed and ends it too
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
        let reply = match (*side, agreed) {
            (Side::No, true) => Some(yes),
            (Side::Yes, false) => Some(no),
            // the answer to maxicom's request, or nothing new
            _ => None,
        };
        *side = if agreed { Side::Yes } else { Side::No };
        if let Some(reply) = reply {
            self.replies.extend_from_slice(&[IAC, reply, code]);
        }
    }

    fn push_sub(&mut self, byte: u8) {
        if self.sub.len() < SUB_MAX {
            self.sub.push(byte);
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

#[cfg(test)]
#[path = "../tests/link/telnet.rs"]
mod tests;
