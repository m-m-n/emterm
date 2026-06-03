use super::state::State;
use super::{MAX_OSC_LEN, Parser};
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn osc_string<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // BEL terminates OSC
            0x07 => {
                self.dispatch_osc(emit);
                self.state = State::Ground;
            }
            // ESC might be start of ST (ESC \)
            0x1B => {
                self.state = State::OscEscape;
            }
            // OSC parameter (number before semicolon)
            b'0'..=b'9' if !self.osc_param_done => {
                self.osc_param = self.osc_param.saturating_mul(10) + (byte - b'0') as u16;
            }
            // Semicolon separates param from data
            b';' if !self.osc_param_done => {
                self.osc_param_done = true;
            }
            // Data bytes
            _ => {
                if self.osc_buffer.len() < MAX_OSC_LEN {
                    self.osc_buffer.push(byte);
                }
            }
        }
    }

    pub(super) fn osc_escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // Backslash completes ST
            b'\\' => {
                self.dispatch_osc(emit);
                self.state = State::Ground;
            }
            // Any other byte after ESC in OSC
            _ => {
                self.dispatch_osc(emit);
                self.state = State::Escape;
                self.escape(byte, emit);
            }
        }
    }

    pub(super) fn dispatch_osc<F>(&mut self, emit: &mut F)
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
        });
        self.osc_param = 0;
        self.osc_param_done = false;
    }
}
