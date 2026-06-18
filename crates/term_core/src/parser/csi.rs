use super::Parser;
use super::state::State;
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn csi_entry<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // Parameter bytes (digits)
            b'0'..=b'9' => {
                self.params.add_digit(byte);
                self.state = State::CsiParam;
            }
            // Intermediate bytes (DEC private modes, etc.)
            b'?' | b'>' | b'<' | b'=' | b' ' => {
                self.params.add_intermediate(byte);
                self.state = State::CsiParam;
            }
            // Parameter separator with no preceding digit.
            // `:` (ISO 8613-6 sub-parameter separator) is consumed like `;` so
            // colon-form SGR (e.g. `38:5:n`, `38:2:r:g:b`) does not cancel the
            // CSI and leak its tail as literal text. Sub-parameters collapse to
            // plain parameters (see colon-sub-param tests). Full ISO 8613-6
            // sub-parameter semantics (distinguishing `:` from `;`, e.g. `4:3`
            // underline styles) are not yet modeled.
            b';' | b':' => {
                self.params.finish_param();
                self.state = State::CsiParam;
            }
            // Final bytes (0x40-0x7E)
            0x40..=0x7E => {
                self.dispatch_csi(byte, emit);
            }
            // C0 controls in CSI - execute immediately
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(ParsedAction::Execute(byte));
            }
            // ESC in CSI - abort and start new escape
            0x1B => {
                self.params.reset();
                self.state = State::Escape;
            }
            // Invalid intermediate - cancel CSI
            _ => {
                self.params.reset();
                self.state = State::Ground;
            }
        }
    }

    pub(super) fn csi_param<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // Parameter bytes (digits)
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            // Parameter separator. `:` (ISO 8613-6 sub-parameter separator) is
            // consumed like `;` so colon-form SGR does not cancel the CSI and
            // leak its tail as text; sub-parameters collapse to plain params.
            b';' | b':' => {
                self.params.finish_param();
            }
            // Valid intermediate bytes (0x20-0x2F, after params)
            0x20..=0x2F => {
                self.params.add_intermediate(byte);
            }
            // Final bytes (0x40-0x7E)
            0x40..=0x7E => {
                self.dispatch_csi(byte, emit);
            }
            // C0 controls in CSI - execute immediately
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(ParsedAction::Execute(byte));
            }
            // ESC in CSI - abort and start new escape
            0x1B => {
                self.params.reset();
                self.state = State::Escape;
            }
            // Invalid bytes in param state - cancel CSI
            _ => {
                self.params.reset();
                self.state = State::Ground;
            }
        }
    }

    pub(super) fn dispatch_csi<F>(&mut self, final_byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        use crate::parser_types::{MAX_CSI_INTERMEDIATES, MAX_CSI_PARAMS};

        let param_vec = self.params.finish();
        let intermediates_slice = self.params.intermediates();

        let mut params = [0u16; MAX_CSI_PARAMS];
        let param_count = param_vec.len().min(MAX_CSI_PARAMS) as u8;
        for (i, &p) in param_vec.iter().take(MAX_CSI_PARAMS).enumerate() {
            params[i] = p;
        }

        let mut intermediates = [0u8; MAX_CSI_INTERMEDIATES];
        let intermediate_count = intermediates_slice.len().min(MAX_CSI_INTERMEDIATES) as u8;
        for (i, &b) in intermediates_slice
            .iter()
            .take(MAX_CSI_INTERMEDIATES)
            .enumerate()
        {
            intermediates[i] = b;
        }

        emit(ParsedAction::CsiDispatch {
            params,
            param_count,
            intermediates,
            intermediate_count,
            final_byte,
        });
        self.params.reset();
        self.state = State::Ground;
    }
}
