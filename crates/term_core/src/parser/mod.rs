use crate::parser_params::ParamParser;
use crate::parser_types::ParsedAction;

mod apc;
mod csi;
mod dcs;
mod escape;
mod ground;
mod osc;
mod state;

use state::State;

/// Maximum size for OSC string data (16MB, matching MAX_DCS_LEN).
const MAX_OSC_LEN: usize = 16 * 1024 * 1024;

/// Maximum size for APC string data (Kitty Graphics).
const MAX_APC_LEN: usize = 4 * 1024 * 1024;

/// Maximum size for DCS string data (SIXEL).
const MAX_DCS_LEN: usize = 16 * 1024 * 1024;

/// ANSI escape sequence parser.
///
/// Processes bytes and emits `ParsedAction` values for each recognized
/// sequence or character. Maintains state between calls to handle
/// sequences that span multiple input buffers.
#[derive(Debug)]
pub(crate) struct Parser {
    state: State,
    params: ParamParser,
    osc_buffer: Vec<u8>,
    osc_param: u16,
    osc_param_done: bool,
    utf8_buffer: Vec<u8>,
    apc_buffer: Vec<u8>,
    dcs_buffer: Vec<u8>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: ParamParser::new(),
            osc_buffer: Vec::with_capacity(256),
            osc_param: 0,
            osc_param_done: false,
            utf8_buffer: Vec::with_capacity(4),
            apc_buffer: Vec::with_capacity(4096),
            dcs_buffer: Vec::with_capacity(4096),
        }
    }

    /// Check if parser is in Ground state with no pending UTF-8 bytes.
    pub fn is_ground_clean(&self) -> bool {
        self.state == State::Ground && self.utf8_buffer.is_empty()
    }

    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.params.reset();
        self.osc_buffer.clear();
        self.osc_param = 0;
        self.osc_param_done = false;
        self.utf8_buffer.clear();
        self.apc_buffer.clear();
        self.dcs_buffer.clear();
    }

    #[cfg(test)]
    pub fn parse<F>(&mut self, input: &[u8], mut emit: F)
    where
        F: FnMut(ParsedAction),
    {
        for &byte in input {
            match self.state {
                State::Ground => self.ground(byte, &mut emit),
                State::Escape => self.escape(byte, &mut emit),
                State::EscapeCharset(designator) => {
                    self.escape_charset(designator, byte, &mut emit)
                }
                State::CsiEntry => self.csi_entry(byte, &mut emit),
                State::CsiParam => self.csi_param(byte, &mut emit),
                State::OscString => self.osc_string(byte, &mut emit),
                State::OscEscape => self.osc_escape(byte, &mut emit),
                State::ApcString => self.apc_string(byte),
                State::ApcEscape => self.apc_escape(byte, &mut emit),
                State::DcsString => self.dcs_string(byte),
                State::DcsEscape => self.dcs_escape(byte, &mut emit),
            }
        }
    }

    /// Parse input with interruptible callback.
    ///
    /// The `emit` callback returns `bool`: `true` to continue, `false` to stop
    /// after the current byte. Returns the number of bytes consumed.
    /// After a CSI dispatch the parser is always in Ground state, so the
    /// remaining bytes can be safely fed to a different parser instance.
    pub fn parse_interruptible<F>(&mut self, input: &[u8], mut emit: F) -> usize
    where
        F: FnMut(ParsedAction) -> bool,
    {
        for (i, &byte) in input.iter().enumerate() {
            let mut should_stop = false;
            {
                let mut wrapper = |action: ParsedAction| {
                    if !emit(action) {
                        should_stop = true;
                    }
                };
                match self.state {
                    State::Ground => self.ground(byte, &mut wrapper),
                    State::Escape => self.escape(byte, &mut wrapper),
                    State::EscapeCharset(designator) => {
                        self.escape_charset(designator, byte, &mut wrapper)
                    }
                    State::CsiEntry => self.csi_entry(byte, &mut wrapper),
                    State::CsiParam => self.csi_param(byte, &mut wrapper),
                    State::OscString => self.osc_string(byte, &mut wrapper),
                    State::OscEscape => self.osc_escape(byte, &mut wrapper),
                    State::ApcString => self.apc_string(byte),
                    State::ApcEscape => self.apc_escape(byte, &mut wrapper),
                    State::DcsString => self.dcs_string(byte),
                    State::DcsEscape => self.dcs_escape(byte, &mut wrapper),
                }
            }
            if should_stop {
                return i + 1;
            }
        }
        input.len()
    }
}

#[cfg(test)]
mod tests;
