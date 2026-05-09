use super::state::State;
use super::{Parser, MAX_DCS_LEN};
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn dcs_string(&mut self, byte: u8) {
        match byte {
            0x1B => {
                self.state = State::DcsEscape;
            }
            _ => {
                if self.dcs_buffer.len() < MAX_DCS_LEN {
                    self.dcs_buffer.push(byte);
                }
            }
        }
    }

    pub(super) fn dcs_escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            b'\\' => {
                self.dispatch_dcs(emit);
                self.state = State::Ground;
            }
            _ => {
                self.dispatch_dcs(emit);
                self.state = State::Escape;
                self.escape(byte, emit);
            }
        }
    }

    pub(super) fn dispatch_dcs<F>(&mut self, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let payload = std::mem::take(&mut self.dcs_buffer);
        emit(ParsedAction::DcsDispatch(payload));
    }
}
