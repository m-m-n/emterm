use super::Parser;
use super::state::State;
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // CSI introducer
            b'[' => {
                self.state = State::CsiEntry;
                self.params.reset();
            }
            // OSC introducer
            b']' => {
                self.state = State::OscString;
                self.osc_buffer.clear();
                self.osc_param = 0;
                self.osc_param_done = false;
            }
            // APC introducer (for Kitty Graphics Protocol)
            b'_' => {
                self.state = State::ApcString;
                self.apc_buffer.clear();
            }
            // DCS introducer (for SIXEL)
            b'P' => {
                self.state = State::DcsString;
                self.dcs_buffer.clear();
            }
            // Charset designation G0
            b'(' => {
                self.state = State::EscapeCharset(b'(');
            }
            // Charset designation G1
            b')' => {
                self.state = State::EscapeCharset(b')');
            }
            // Known ESC final bytes
            b'7' | b'8' | b'D' | b'E' | b'H' | b'M' | b'c' => {
                emit(ParsedAction::EscDispatch {
                    intermediate: None,
                    final_byte: byte,
                });
                self.state = State::Ground;
            }
            // Another ESC - restart escape
            0x1B => {
                // Stay in Escape state
            }
            // Unknown ESC sequence
            _ => {
                emit(ParsedAction::EscDispatch {
                    intermediate: None,
                    final_byte: byte,
                });
                self.state = State::Ground;
            }
        }
    }

    pub(super) fn escape_charset<F>(&mut self, designator: u8, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        emit(ParsedAction::EscDispatch {
            intermediate: Some(designator),
            final_byte: byte,
        });
        self.state = State::Ground;
    }
}
