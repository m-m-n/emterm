use super::state::State;
use super::{MAX_OSC_LEN, Parser};
use crate::parser_types::{OscTerminator, ParsedAction};

impl Parser {
    pub(super) fn osc_string<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // BEL terminates OSC (not part of the received length)
            0x07 => {
                self.dispatch_osc(emit, OscTerminator::Bel);
                self.state = State::Ground;
            }
            // ESC might be start of ST (ESC \); not counted either
            0x1B => {
                self.state = State::OscEscape;
            }
            // OSC parameter (number before semicolon). The value saturates on
            // both the multiply and the add: once above the u16 range it
            // stays at `u16::MAX` for the rest of the digits, which no OSC
            // code (native or host-registered) uses.
            b'0'..=b'9' if !self.osc_param_done => {
                self.count_osc_byte();
                self.osc_param = self
                    .osc_param
                    .saturating_mul(10)
                    .saturating_add((byte - b'0') as u16);
            }
            // Semicolon separates param from data
            b';' if !self.osc_param_done => {
                self.count_osc_byte();
                self.osc_param_done = true;
            }
            // Data bytes
            _ => {
                self.count_osc_byte();
                if self.osc_buffer.len() < MAX_OSC_LEN {
                    self.osc_buffer.push(byte);
                }
            }
        }
    }

    /// Count one received byte of the open OSC string (SC-2). Counts every
    /// byte the string consumes, including the ones dropped past
    /// `MAX_OSC_LEN`; saturates instead of overflowing.
    fn count_osc_byte(&mut self) {
        self.osc_received_len = self.osc_received_len.saturating_add(1);
    }

    pub(super) fn osc_escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // Backslash completes ST
            b'\\' => {
                self.dispatch_osc(emit, OscTerminator::St);
                self.state = State::Ground;
            }
            // Any other byte after ESC in OSC: not a real terminator (SC-1:
            // classified deterministically as `Unterminated`, never guessed
            // as BEL/ST) — the string is cut short and the byte is
            // reprocessed as a fresh escape sequence.
            _ => {
                self.dispatch_osc(emit, OscTerminator::Unterminated);
                self.state = State::Escape;
                self.escape(byte, emit);
            }
        }
    }

    pub(super) fn dispatch_osc<F>(&mut self, emit: &mut F, terminator: OscTerminator)
    where
        F: FnMut(ParsedAction),
    {
        let buf = std::mem::replace(&mut self.osc_buffer, Vec::with_capacity(256));
        let data = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };

        emit(ParsedAction::OscDispatch {
            param: self.osc_param,
            data,
            terminator,
            received_len: self.osc_received_len,
        });
        self.osc_param = 0;
        self.osc_param_done = false;
        self.osc_received_len = 0;
    }
}
