use super::state::State;
use super::{Parser, MAX_APC_LEN};
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn apc_string(&mut self, byte: u8) {
        match byte {
            0x1B => {
                self.state = State::ApcEscape;
            }
            _ => {
                if self.apc_buffer.len() < MAX_APC_LEN {
                    self.apc_buffer.push(byte);
                }
            }
        }
    }

    pub(super) fn apc_escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            b'\\' => {
                self.dispatch_apc(emit);
                self.state = State::Ground;
            }
            _ => {
                self.dispatch_apc(emit);
                self.state = State::Escape;
                self.escape(byte, emit);
            }
        }
    }

    pub(super) fn dispatch_apc<F>(&mut self, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let payload = std::mem::take(&mut self.apc_buffer);
        emit(ParsedAction::ApcDispatch(payload));
    }
}
