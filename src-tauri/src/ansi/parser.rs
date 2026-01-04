//! ANSI escape sequence parser state machine.
//!
//! This module implements a state machine that parses ANSI escape sequences
//! from a byte stream. It handles incomplete sequences across buffer boundaries
//! and emits structured `TerminalAction` values.
//!
//! # State Machine
//!
//! The parser follows a simplified version of the state machine described in
//! the ECMA-48 standard, with states:
//!
//! - Ground: Normal character processing
//! - Escape: After receiving ESC (0x1B)
//! - EscapeIntermediate: ESC followed by intermediate bytes
//! - CsiEntry: After ESC [
//! - CsiParam: Collecting CSI parameters
//! - CsiIntermediate: CSI intermediate bytes
//! - OscString: Collecting OSC data
//!
//! # Example
//!
//! ```
//! use app_lib::ansi::{Parser, TerminalAction};
//!
//! let mut parser = Parser::new();
//! let mut actions = Vec::new();
//!
//! parser.parse(b"Hello\x1b[31mRed\x1b[0m", |action| {
//!     actions.push(action);
//! });
//! ```

use crate::ansi::params::ParamParser;
use crate::ansi::sequence::{CharSet, CsiAction, EraseMode, EscAction, OscAction, TerminalAction};

/// Maximum size for OSC string data.
const MAX_OSC_LEN: usize = 4096;

/// Parser state machine states.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Normal character processing.
    Ground,
    /// After receiving ESC (0x1B).
    Escape,
    /// ESC followed by ( or ) for charset selection.
    EscapeCharset(u8),
    /// After ESC [, entering CSI sequence.
    CsiEntry,
    /// Collecting CSI parameters.
    CsiParam,
    /// After ESC ], collecting OSC string.
    OscString,
    /// After ESC in OSC string, waiting for backslash to complete ST.
    OscEscape,
}

/// ANSI escape sequence parser.
///
/// This parser processes bytes and emits `TerminalAction` values for each
/// recognized sequence or character. It maintains state between calls to
/// handle sequences that span multiple input buffers.
#[derive(Debug)]
pub struct Parser {
    /// Current parser state.
    state: State,
    /// CSI parameter parser.
    params: ParamParser,
    /// OSC string accumulator.
    osc_buffer: Vec<u8>,
    /// OSC parameter (the number before the semicolon).
    osc_param: u16,
    /// Whether we've seen the semicolon in OSC.
    osc_param_done: bool,
    /// UTF-8 multibyte sequence accumulator.
    utf8_buffer: Vec<u8>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Creates a new parser in the ground state.
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: ParamParser::new(),
            osc_buffer: Vec::with_capacity(256),
            osc_param: 0,
            osc_param_done: false,
            utf8_buffer: Vec::new(),
        }
    }

    /// Resets the parser to its initial state.
    ///
    /// This clears any partial sequence data and returns to the ground state.
    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.params.reset();
        self.osc_buffer.clear();
        self.osc_param = 0;
        self.osc_param_done = false;
        self.utf8_buffer.clear();
    }

    /// Parses input bytes and calls the emit function for each action.
    ///
    /// # Arguments
    ///
    /// * `input` - Bytes to parse
    /// * `emit` - Function called for each recognized terminal action
    ///
    /// # Example
    ///
    /// ```
    /// use app_lib::ansi::{Parser, TerminalAction};
    ///
    /// let mut parser = Parser::new();
    /// parser.parse(b"AB\n", |action| {
    ///     println!("{:?}", action);
    /// });
    /// ```
    pub fn parse<F>(&mut self, input: &[u8], mut emit: F)
    where
        F: FnMut(TerminalAction),
    {
        for &byte in input {
            self.advance(byte, &mut emit);
        }
    }

    /// Advances the state machine by one byte.
    fn advance<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match self.state {
            State::Ground => self.ground(byte, emit),
            State::Escape => self.escape(byte, emit),
            State::EscapeCharset(selector) => self.escape_charset(selector, byte, emit),
            State::CsiEntry => self.csi_entry(byte, emit),
            State::CsiParam => self.csi_param(byte, emit),
            State::OscString => self.osc_string(byte, emit),
            State::OscEscape => self.osc_escape(byte, emit),
        }
    }

    /// Ground state: process printable characters and C0 controls.
    fn ground<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // ESC - start escape sequence
            0x1B => {
                self.state = State::Escape;
            }
            // C0 control characters (except ESC)
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(TerminalAction::Execute(byte));
            }
            // DEL - ignore
            0x7F => {}
            // Printable ASCII
            0x20..=0x7E => {
                emit(TerminalAction::Print(byte as char));
            }
            // UTF-8 start and continuation bytes
            0x80..=0xFF => {
                self.handle_utf8_byte(byte, emit);
            }
        }
    }

    /// Handle UTF-8 multibyte sequences.
    ///
    /// Accumulates bytes and emits complete UTF-8 characters.
    /// Invalid sequences emit the Unicode replacement character (U+FFFD).
    /// This function should only be called for bytes in the range 0x80-0xFF.
    fn handle_utf8_byte<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        // Check if this is a start byte (0b11xxxxxx) or continuation byte (0b10xxxxxx)
        match byte {
            // 2-byte sequence start: 110xxxxx
            0xC0..=0xDF => {
                // Flush any incomplete sequence
                if !self.utf8_buffer.is_empty() {
                    emit(TerminalAction::Print('\u{FFFD}'));
                }
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
            }
            // 3-byte sequence start: 1110xxxx
            0xE0..=0xEF => {
                // Flush any incomplete sequence
                if !self.utf8_buffer.is_empty() {
                    emit(TerminalAction::Print('\u{FFFD}'));
                }
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
            }
            // 4-byte sequence start: 11110xxx
            0xF0..=0xF7 => {
                // Flush any incomplete sequence
                if !self.utf8_buffer.is_empty() {
                    emit(TerminalAction::Print('\u{FFFD}'));
                }
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
            }
            // Continuation byte: 10xxxxxx
            0x80..=0xBF => {
                if self.utf8_buffer.is_empty() {
                    // Continuation byte without start - invalid, emit replacement char
                    emit(TerminalAction::Print('\u{FFFD}'));
                } else {
                    self.utf8_buffer.push(byte);

                    // Check if we have a complete sequence
                    let expected_len = match self.utf8_buffer[0] {
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF7 => 4,
                        _ => 1,
                    };

                    if self.utf8_buffer.len() == expected_len {
                        // Try to decode
                        if let Ok(s) = std::str::from_utf8(&self.utf8_buffer) {
                            for ch in s.chars() {
                                emit(TerminalAction::Print(ch));
                            }
                        } else {
                            // Invalid UTF-8 sequence
                            emit(TerminalAction::Print('\u{FFFD}'));
                        }
                        self.utf8_buffer.clear();
                    }
                }
            }
            // Invalid UTF-8 start bytes (0b11111xxx) or unexpected ASCII (should not reach here)
            _ => {
                // Flush any incomplete sequence
                if !self.utf8_buffer.is_empty() {
                    emit(TerminalAction::Print('\u{FFFD}'));
                    self.utf8_buffer.clear();
                }
                // Emit replacement char for invalid byte
                emit(TerminalAction::Print('\u{FFFD}'));
            }
        }
    }

    /// Escape state: after receiving ESC.
    fn escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // CSI introducer
            b'[' => {
                self.params.reset();
                self.state = State::CsiEntry;
            }
            // OSC introducer
            b']' => {
                self.osc_buffer.clear();
                self.osc_param = 0;
                self.osc_param_done = false;
                self.state = State::OscString;
            }
            // ESC 7 - Save cursor
            b'7' => {
                emit(TerminalAction::Esc(EscAction::SaveCursor));
                self.state = State::Ground;
            }
            // ESC 8 - Restore cursor
            b'8' => {
                emit(TerminalAction::Esc(EscAction::RestoreCursor));
                self.state = State::Ground;
            }
            // ESC D - Index
            b'D' => {
                emit(TerminalAction::Esc(EscAction::Index));
                self.state = State::Ground;
            }
            // ESC E - Next Line
            b'E' => {
                emit(TerminalAction::Esc(EscAction::NextLine));
                self.state = State::Ground;
            }
            // ESC H - Horizontal Tab Set
            b'H' => {
                emit(TerminalAction::Esc(EscAction::HorizontalTabSet));
                self.state = State::Ground;
            }
            // ESC M - Reverse Index
            b'M' => {
                emit(TerminalAction::Esc(EscAction::ReverseIndex));
                self.state = State::Ground;
            }
            // ESC c - Reset to Initial State
            b'c' => {
                emit(TerminalAction::Esc(EscAction::ResetToInitialState));
                self.state = State::Ground;
            }
            // ESC ( - G0 charset selector
            b'(' => {
                self.state = State::EscapeCharset(b'(');
            }
            // ESC ) - G1 charset selector
            b')' => {
                self.state = State::EscapeCharset(b')');
            }
            // ESC ESC - emit ESC and stay in escape state
            0x1B => {
                emit(TerminalAction::Execute(0x1B));
            }
            // C0 control in escape state - execute immediately
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(TerminalAction::Execute(byte));
            }
            // Unknown escape sequence
            _ => {
                emit(TerminalAction::Esc(EscAction::Unknown(byte)));
                self.state = State::Ground;
            }
        }
    }

    /// Escape charset state: ESC ( or ESC ) followed by charset designator.
    fn escape_charset<F>(&mut self, selector: u8, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        let charset = CharSet::from(byte);
        let action = if selector == b'(' {
            EscAction::SetG0CharSet(charset)
        } else {
            EscAction::SetG1CharSet(charset)
        };
        emit(TerminalAction::Esc(action));
        self.state = State::Ground;
    }

    /// CSI entry state: just entered CSI, looking for parameters or final byte.
    fn csi_entry<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // Parameter bytes
            b'0'..=b'9' => {
                self.params.add_digit(byte);
                self.state = State::CsiParam;
            }
            // Semicolon with no first param
            b';' => {
                self.params.finish_param();
                self.state = State::CsiParam;
            }
            // Intermediate bytes (like '?' for DEC private)
            b'<' | b'=' | b'>' | b'?' => {
                self.params.add_intermediate(byte);
                self.state = State::CsiParam;
            }
            // Valid intermediate bytes (SP and !")
            b' ' | b'!' => {
                self.params.add_intermediate(byte);
                self.state = State::CsiParam;
            }
            // Final byte with no parameters
            0x40..=0x7E => {
                self.dispatch_csi(byte, emit);
            }
            // C0 control in CSI - execute immediately
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(TerminalAction::Execute(byte));
            }
            // ESC - abort CSI and start new escape
            0x1B => {
                self.params.reset();
                self.state = State::Escape;
            }
            // Invalid intermediate bytes (0x20-0x2F except valid ones) - cancel CSI
            0x20..=0x2F => {
                // Cancel sequence and return to ground
                self.params.reset();
                self.state = State::Ground;
            }
            // Invalid - return to ground
            _ => {
                self.params.reset();
                self.state = State::Ground;
            }
        }
    }

    /// CSI param state: collecting parameters.
    fn csi_param<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // More digits
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            // Parameter separator
            b';' => {
                self.params.finish_param();
            }
            // Colon separator (subparameters, e.g., for SGR)
            b':' => {
                // For now, treat colon like semicolon
                // Proper handling would track subparameters
                self.params.finish_param();
            }
            // Valid intermediate bytes (0x20-0x2F) - only certain ones are valid
            b' ' | b'!' => {
                // These are valid - continue collecting
                self.params.add_intermediate(byte);
            }
            // Invalid intermediate bytes in parameter state - cancel CSI
            0x20..=0x2F => {
                // Invalid intermediate byte - cancel sequence
                self.params.reset();
                self.state = State::Ground;
            }
            // Final byte
            0x40..=0x7E => {
                self.dispatch_csi(byte, emit);
            }
            // C0 control in CSI - execute immediately
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(TerminalAction::Execute(byte));
            }
            // ESC - abort CSI and start new escape
            0x1B => {
                self.params.reset();
                self.state = State::Escape;
            }
            // Invalid - return to ground
            _ => {
                self.params.reset();
                self.state = State::Ground;
            }
        }
    }

    /// Dispatch a complete CSI sequence.
    fn dispatch_csi<F>(&mut self, final_byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        let params = self.params.finish();
        let intermediates: Vec<u8> = self.params.intermediates().to_vec();
        let is_private = self.params.is_dec_private();

        let action = match (is_private, final_byte) {
            // DEC Private modes
            (true, b'h') => CsiAction::SetMode(params),
            (true, b'l') => CsiAction::ResetMode(params),

            // Standard CSI sequences
            (false, b'm') => CsiAction::Sgr(params),

            // Cursor movement
            (false, b'A') => CsiAction::CursorUp(ParamParser::get_first_or_one(&params)),
            (false, b'B') => CsiAction::CursorDown(ParamParser::get_first_or_one(&params)),
            (false, b'C') => CsiAction::CursorForward(ParamParser::get_first_or_one(&params)),
            (false, b'D') => CsiAction::CursorBack(ParamParser::get_first_or_one(&params)),
            (false, b'E') => CsiAction::CursorNextLine(ParamParser::get_first_or_one(&params)),
            (false, b'F') => CsiAction::CursorPreviousLine(ParamParser::get_first_or_one(&params)),
            (false, b'G') => {
                CsiAction::CursorHorizontalAbsolute(ParamParser::get_first_or_one(&params))
            }
            (false, b'H') | (false, b'f') => {
                let row = ParamParser::get_param(&params, 0, 1);
                let col = ParamParser::get_param(&params, 1, 1);
                CsiAction::CursorPosition { row, col }
            }
            (false, b'd') => {
                CsiAction::CursorVerticalAbsolute(ParamParser::get_first_or_one(&params))
            }

            // Erase operations
            (false, b'J') => {
                CsiAction::EraseInDisplay(EraseMode::from(ParamParser::get_first_or_zero(&params)))
            }
            (false, b'K') => {
                CsiAction::EraseInLine(EraseMode::from(ParamParser::get_first_or_zero(&params)))
            }
            (false, b'X') => CsiAction::EraseCharacters(ParamParser::get_first_or_one(&params)),

            // Insert/Delete operations
            (false, b'@') => CsiAction::InsertCharacters(ParamParser::get_first_or_one(&params)),
            (false, b'P') => CsiAction::DeleteCharacters(ParamParser::get_first_or_one(&params)),
            (false, b'L') => CsiAction::InsertLines(ParamParser::get_first_or_one(&params)),
            (false, b'M') => CsiAction::DeleteLines(ParamParser::get_first_or_one(&params)),

            // Scroll operations
            (false, b'S') => CsiAction::ScrollUp(ParamParser::get_first_or_one(&params)),
            (false, b'T') => CsiAction::ScrollDown(ParamParser::get_first_or_one(&params)),
            (false, b'r') => {
                let top = ParamParser::get_param(&params, 0, 1);
                let bottom = ParamParser::get_param(&params, 1, 0); // 0 means use rows
                CsiAction::SetScrollRegion { top, bottom }
            }

            // Device status
            (false, b'n') => CsiAction::DeviceStatusReport(ParamParser::get_first_or_zero(&params)),

            // Device Attributes
            (false, b'c') => {
                // CSI c or CSI 0 c - Primary Device Attributes
                // Check for CSI > c (Secondary) or CSI = c (Tertiary)
                if intermediates.contains(&b'>') {
                    CsiAction::SecondaryDeviceAttributes
                } else if intermediates.contains(&b'=') {
                    CsiAction::TertiaryDeviceAttributes
                } else {
                    CsiAction::PrimaryDeviceAttributes
                }
            }

            // Unknown sequence
            _ => CsiAction::Unknown {
                params,
                intermediates,
                final_byte,
            },
        };

        emit(TerminalAction::Csi(action));
        self.params.reset();
        self.state = State::Ground;
    }

    /// OSC string state: collecting OSC data.
    fn osc_string<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // BEL terminates OSC
            0x07 => {
                self.dispatch_osc(emit);
                self.state = State::Ground;
            }
            // ESC might be start of ST (ESC \)
            0x1B => {
                // Transition to OscEscape to check for backslash
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

    /// OSC escape state: after ESC in OSC, waiting for backslash.
    fn osc_escape<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        match byte {
            // Backslash completes ST (String Terminator)
            b'\\' => {
                self.dispatch_osc(emit);
                self.state = State::Ground;
            }
            // Any other byte after ESC in OSC - treat ESC as abort, process this byte as new escape
            _ => {
                // Dispatch current OSC
                self.dispatch_osc(emit);
                // Process the byte in escape state
                self.state = State::Escape;
                self.escape(byte, emit);
            }
        }
    }

    /// Dispatch a complete OSC sequence.
    /// Clears OSC state but does NOT change parser state (caller handles that).
    fn dispatch_osc<F>(&mut self, emit: &mut F)
    where
        F: FnMut(TerminalAction),
    {
        let data = String::from_utf8_lossy(&self.osc_buffer).to_string();

        let action = match self.osc_param {
            0 => OscAction::SetTitleAndIcon(data),
            1 => OscAction::SetIconName(data),
            2 => OscAction::SetTitle(data),
            4 => {
                // Color palette format: index;color
                // e.g., "0;rgb:00/00/00" or "0;#000000"
                if let Some(semi_pos) = data.find(';') {
                    let (index_str, color) = data.split_at(semi_pos);
                    if let Ok(index) = index_str.parse::<u8>() {
                        OscAction::SetColorPalette {
                            index,
                            color: color[1..].to_string(),
                        }
                    } else {
                        OscAction::Unknown { ps: 4, data }
                    }
                } else {
                    OscAction::Unknown { ps: 4, data }
                }
            }
            7 => OscAction::SetWorkingDirectory(data),
            8 => {
                // Hyperlink format: params;uri
                if let Some(semi_pos) = data.find(';') {
                    let (params, uri) = data.split_at(semi_pos);
                    OscAction::Hyperlink {
                        params: params.to_string(),
                        uri: uri[1..].to_string(), // Skip the semicolon
                    }
                } else {
                    OscAction::Unknown { ps: 8, data }
                }
            }
            10 => OscAction::SetForegroundColor(data),
            11 => OscAction::SetBackgroundColor(data),
            777 => {
                // eMterm extension format: verb;param1;param2;...
                let parts: Vec<&str> = data.split(';').collect();
                if !parts.is_empty() {
                    let verb = parts[0].to_string();
                    let params = parts[1..].iter().map(|s| s.to_string()).collect();
                    OscAction::EmtermExtension { verb, params }
                } else {
                    OscAction::Unknown { ps: 777, data }
                }
            }
            _ => OscAction::Unknown {
                ps: self.osc_param,
                data,
            },
        };

        emit(TerminalAction::Osc(action));
        self.osc_buffer.clear();
        self.osc_param = 0;
        self.osc_param_done = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to collect all actions from parsing input.
    fn parse_all(input: &[u8]) -> Vec<TerminalAction> {
        let mut parser = Parser::new();
        let mut actions = Vec::new();
        parser.parse(input, |action| actions.push(action));
        actions
    }

    // =========================================================================
    // Printable ASCII Tests
    // =========================================================================

    #[test]
    fn test_parse_printable_ascii() {
        let actions = parse_all(b"Hello");
        assert_eq!(actions.len(), 5);
        assert_eq!(actions[0], TerminalAction::Print('H'));
        assert_eq!(actions[1], TerminalAction::Print('e'));
        assert_eq!(actions[2], TerminalAction::Print('l'));
        assert_eq!(actions[3], TerminalAction::Print('l'));
        assert_eq!(actions[4], TerminalAction::Print('o'));
    }

    #[test]
    fn test_parse_space() {
        let actions = parse_all(b" ");
        assert_eq!(actions, vec![TerminalAction::Print(' ')]);
    }

    #[test]
    fn test_parse_all_printable() {
        // Test range 0x20 to 0x7E
        let input = b"!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let actions = parse_all(input);
        assert_eq!(actions.len(), input.len());
        for action in &actions {
            assert!(matches!(action, TerminalAction::Print(_)));
        }
    }

    // =========================================================================
    // C0 Control Character Tests
    // =========================================================================

    #[test]
    fn test_parse_c0_bel() {
        let actions = parse_all(b"\x07");
        assert_eq!(actions, vec![TerminalAction::Execute(0x07)]);
    }

    #[test]
    fn test_parse_c0_bs() {
        let actions = parse_all(b"\x08");
        assert_eq!(actions, vec![TerminalAction::Execute(0x08)]);
    }

    #[test]
    fn test_parse_c0_ht() {
        let actions = parse_all(b"\x09");
        assert_eq!(actions, vec![TerminalAction::Execute(0x09)]);
    }

    #[test]
    fn test_parse_c0_lf() {
        let actions = parse_all(b"\x0A");
        assert_eq!(actions, vec![TerminalAction::Execute(0x0A)]);
    }

    #[test]
    fn test_parse_c0_cr() {
        let actions = parse_all(b"\x0D");
        assert_eq!(actions, vec![TerminalAction::Execute(0x0D)]);
    }

    #[test]
    fn test_parse_mixed_text_and_controls() {
        let actions = parse_all(b"A\r\nB");
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[0], TerminalAction::Print('A'));
        assert_eq!(actions[1], TerminalAction::Execute(0x0D)); // CR
        assert_eq!(actions[2], TerminalAction::Execute(0x0A)); // LF
        assert_eq!(actions[3], TerminalAction::Print('B'));
    }

    // =========================================================================
    // ESC Sequence Tests
    // =========================================================================

    #[test]
    fn test_parse_esc_save_cursor() {
        let actions = parse_all(b"\x1B7");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::SaveCursor)]);
    }

    #[test]
    fn test_parse_esc_restore_cursor() {
        let actions = parse_all(b"\x1B8");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::RestoreCursor)]);
    }

    #[test]
    fn test_parse_esc_index() {
        let actions = parse_all(b"\x1BD");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::Index)]);
    }

    #[test]
    fn test_parse_esc_next_line() {
        let actions = parse_all(b"\x1BE");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::NextLine)]);
    }

    #[test]
    fn test_parse_esc_horizontal_tab_set() {
        let actions = parse_all(b"\x1BH");
        assert_eq!(
            actions,
            vec![TerminalAction::Esc(EscAction::HorizontalTabSet)]
        );
    }

    #[test]
    fn test_parse_esc_reverse_index() {
        let actions = parse_all(b"\x1BM");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::ReverseIndex)]);
    }

    #[test]
    fn test_parse_esc_reset() {
        let actions = parse_all(b"\x1Bc");
        assert_eq!(
            actions,
            vec![TerminalAction::Esc(EscAction::ResetToInitialState)]
        );
    }

    #[test]
    fn test_parse_esc_g0_charset_ascii() {
        let actions = parse_all(b"\x1B(B");
        assert_eq!(
            actions,
            vec![TerminalAction::Esc(EscAction::SetG0CharSet(CharSet::Ascii))]
        );
    }

    #[test]
    fn test_parse_esc_g0_charset_line_drawing() {
        let actions = parse_all(b"\x1B(0");
        assert_eq!(
            actions,
            vec![TerminalAction::Esc(EscAction::SetG0CharSet(
                CharSet::DecLineDrawing
            ))]
        );
    }

    #[test]
    fn test_parse_esc_g1_charset() {
        let actions = parse_all(b"\x1B)A");
        assert_eq!(
            actions,
            vec![TerminalAction::Esc(EscAction::SetG1CharSet(CharSet::Uk))]
        );
    }

    #[test]
    fn test_parse_esc_unknown() {
        let actions = parse_all(b"\x1BX");
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::Unknown(b'X'))]);
    }

    // =========================================================================
    // CSI Sequence Tests
    // =========================================================================

    #[test]
    fn test_parse_csi_sgr_reset() {
        let actions = parse_all(b"\x1B[m");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::Sgr(vec![]))]);
    }

    #[test]
    fn test_parse_csi_sgr_explicit_reset() {
        let actions = parse_all(b"\x1B[0m");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::Sgr(vec![0]))]);
    }

    #[test]
    fn test_parse_csi_sgr_bold() {
        let actions = parse_all(b"\x1B[1m");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::Sgr(vec![1]))]);
    }

    #[test]
    fn test_parse_csi_sgr_red_foreground() {
        let actions = parse_all(b"\x1B[31m");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::Sgr(vec![31]))]);
    }

    #[test]
    fn test_parse_csi_sgr_multiple_params() {
        let actions = parse_all(b"\x1B[1;31m");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::Sgr(vec![1, 31]))]
        );
    }

    #[test]
    fn test_parse_csi_sgr_256_color() {
        let actions = parse_all(b"\x1B[38;5;196m");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::Sgr(vec![38, 5, 196]))]
        );
    }

    #[test]
    fn test_parse_csi_sgr_rgb() {
        let actions = parse_all(b"\x1B[38;2;255;0;128m");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::Sgr(vec![
                38, 2, 255, 0, 128
            ]))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_up() {
        let actions = parse_all(b"\x1B[A");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::CursorUp(1))]);
    }

    #[test]
    fn test_parse_csi_cursor_up_with_count() {
        let actions = parse_all(b"\x1B[5A");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::CursorUp(5))]);
    }

    #[test]
    fn test_parse_csi_cursor_down() {
        let actions = parse_all(b"\x1B[3B");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::CursorDown(3))]);
    }

    #[test]
    fn test_parse_csi_cursor_forward() {
        let actions = parse_all(b"\x1B[10C");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorForward(10))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_back() {
        let actions = parse_all(b"\x1B[2D");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::CursorBack(2))]);
    }

    #[test]
    fn test_parse_csi_cursor_position() {
        let actions = parse_all(b"\x1B[10;20H");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPosition {
                row: 10,
                col: 20
            })]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position_default() {
        let actions = parse_all(b"\x1B[H");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPosition {
                row: 1,
                col: 1
            })]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position_partial() {
        let actions = parse_all(b"\x1B[;10H");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPosition {
                row: 1,
                col: 10
            })]
        );
    }

    #[test]
    fn test_parse_csi_erase_display_below() {
        let actions = parse_all(b"\x1B[J");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::EraseInDisplay(
                EraseMode::Below
            ))]
        );
    }

    #[test]
    fn test_parse_csi_erase_display_all() {
        let actions = parse_all(b"\x1B[2J");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::EraseInDisplay(
                EraseMode::All
            ))]
        );
    }

    #[test]
    fn test_parse_csi_erase_line() {
        let actions = parse_all(b"\x1B[K");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::EraseInLine(
                EraseMode::Below
            ))]
        );
    }

    #[test]
    fn test_parse_csi_dec_private_set_mode() {
        let actions = parse_all(b"\x1B[?25h");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::SetMode(vec![25]))]
        );
    }

    #[test]
    fn test_parse_csi_dec_private_reset_mode() {
        let actions = parse_all(b"\x1B[?25l");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::ResetMode(vec![25]))]
        );
    }

    #[test]
    fn test_parse_csi_device_status_report() {
        let actions = parse_all(b"\x1B[6n");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeviceStatusReport(6))]
        );
    }

    #[test]
    fn test_parse_csi_device_status_report_5() {
        let actions = parse_all(b"\x1B[5n");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeviceStatusReport(5))]
        );
    }

    #[test]
    fn test_parse_csi_primary_device_attributes() {
        let actions = parse_all(b"\x1B[c");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::PrimaryDeviceAttributes)]
        );
    }

    #[test]
    fn test_parse_csi_primary_device_attributes_with_param() {
        let actions = parse_all(b"\x1B[0c");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::PrimaryDeviceAttributes)]
        );
    }

    #[test]
    fn test_parse_csi_secondary_device_attributes() {
        let actions = parse_all(b"\x1B[>c");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::SecondaryDeviceAttributes)]
        );
    }

    #[test]
    fn test_parse_csi_tertiary_device_attributes() {
        let actions = parse_all(b"\x1B[=c");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::TertiaryDeviceAttributes)]
        );
    }

    #[test]
    fn test_parse_csi_unknown() {
        let actions = parse_all(b"\x1B[1;2;3z");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::Unknown {
                params: vec![1, 2, 3],
                intermediates: vec![],
                final_byte: b'z',
            })]
        );
    }

    // =========================================================================
    // OSC Sequence Tests
    // =========================================================================

    #[test]
    fn test_parse_osc_set_title() {
        let actions = parse_all(b"\x1B]2;My Title\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetTitle(
                "My Title".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_set_title_and_icon() {
        let actions = parse_all(b"\x1B]0;Terminal\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetTitleAndIcon(
                "Terminal".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_set_icon_name() {
        let actions = parse_all(b"\x1B]1;Icon\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetIconName(
                "Icon".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_working_directory() {
        let actions = parse_all(b"\x1B]7;file:///home/user\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetWorkingDirectory(
                "file:///home/user".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_hyperlink() {
        let actions = parse_all(b"\x1B]8;id=1;https://example.com\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::Hyperlink {
                params: "id=1".to_string(),
                uri: "https://example.com".to_string(),
            })]
        );
    }

    #[test]
    fn test_parse_osc_unknown() {
        let actions = parse_all(b"\x1B]99;data\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::Unknown {
                ps: 99,
                data: "data".to_string(),
            })]
        );
    }

    #[test]
    fn test_parse_osc_color_palette() {
        let actions = parse_all(b"\x1B]4;0;rgb:00/00/00\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetColorPalette {
                index: 0,
                color: "rgb:00/00/00".to_string(),
            })]
        );
    }

    #[test]
    fn test_parse_osc_foreground_color() {
        let actions = parse_all(b"\x1B]10;#ffffff\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetForegroundColor(
                "#ffffff".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_background_color() {
        let actions = parse_all(b"\x1B]11;#000000\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetBackgroundColor(
                "#000000".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_osc_emterm_extension() {
        let actions = parse_all(b"\x1B]777;markdown;title;body\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "markdown".to_string(),
                params: vec!["title".to_string(), "body".to_string()],
            })]
        );
    }

    // =========================================================================
    // OSC 777 Markdown Extension Tests
    // =========================================================================

    #[test]
    fn test_parse_osc_emterm_markdown_begin() {
        // Test begin verb with full parameters
        let actions =
            parse_all(b"\x1B]777;emterm;markdown;begin;id=550e8400-e29b-41d4-a716-446655440000;format=gfm\x1B\\");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "emterm".to_string(),
                params: vec![
                    "markdown".to_string(),
                    "begin".to_string(),
                    "id=550e8400-e29b-41d4-a716-446655440000".to_string(),
                    "format=gfm".to_string(),
                ],
            })]
        );
    }

    #[test]
    fn test_parse_osc_emterm_markdown_chunk() {
        // Test chunk verb with Base64 data
        let actions =
            parse_all(b"\x1B]777;emterm;markdown;chunk;id=550e8400-e29b-41d4-a716-446655440000;seq=0;data=IyBIZWxsbw==\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "emterm".to_string(),
                params: vec![
                    "markdown".to_string(),
                    "chunk".to_string(),
                    "id=550e8400-e29b-41d4-a716-446655440000".to_string(),
                    "seq=0".to_string(),
                    "data=IyBIZWxsbw==".to_string(),
                ],
            })]
        );
    }

    #[test]
    fn test_parse_osc_emterm_markdown_end() {
        // Test end verb
        let actions = parse_all(
            b"\x1B]777;emterm;markdown;end;id=550e8400-e29b-41d4-a716-446655440000\x1B\\",
        );
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "emterm".to_string(),
                params: vec![
                    "markdown".to_string(),
                    "end".to_string(),
                    "id=550e8400-e29b-41d4-a716-446655440000".to_string(),
                ],
            })]
        );
    }

    #[test]
    fn test_parse_osc_emterm_markdown_begin_minimal() {
        // Test begin verb with minimal parameters (only required id)
        let actions = parse_all(b"\x1B]777;emterm;markdown;begin;id=test-id\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "emterm".to_string(),
                params: vec![
                    "markdown".to_string(),
                    "begin".to_string(),
                    "id=test-id".to_string(),
                ],
            })]
        );
    }

    #[test]
    fn test_parse_osc_777_empty_data() {
        // Empty OSC 777 should emit Unknown
        let actions = parse_all(b"\x1B]777;\x07");
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::EmtermExtension {
                verb: "".to_string(),
                params: vec![],
            })]
        );
    }

    #[test]
    fn test_parse_osc_st_terminator() {
        // Test OSC with ESC \ (ST) terminator
        let actions = parse_all(b"\x1B]2;My Title\x1B\\");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetTitle("My Title".to_string()))
        );
    }

    // =========================================================================
    // Buffer Boundary Tests
    // =========================================================================

    #[test]
    fn test_parse_split_csi_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split "\x1B[31m" across two calls
        parser.parse(b"\x1B[", |action| actions.push(action));
        assert!(actions.is_empty()); // No complete action yet

        parser.parse(b"31m", |action| actions.push(action));
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::Sgr(vec![31]))]);
    }

    #[test]
    fn test_parse_split_esc_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split "\x1B7" across two calls
        parser.parse(b"\x1B", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"7", |action| actions.push(action));
        assert_eq!(actions, vec![TerminalAction::Esc(EscAction::SaveCursor)]);
    }

    #[test]
    fn test_parse_split_osc_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split OSC across multiple calls
        parser.parse(b"\x1B]2;My ", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"Title\x07", |action| actions.push(action));
        assert_eq!(
            actions,
            vec![TerminalAction::Osc(OscAction::SetTitle(
                "My Title".to_string()
            ))]
        );
    }

    #[test]
    fn test_parse_split_byte_by_byte() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Parse "\x1B[1;31m" byte by byte
        for byte in b"\x1B[1;31m" {
            parser.parse(&[*byte], |action| actions.push(action));
        }

        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::Sgr(vec![1, 31]))]
        );
    }

    // =========================================================================
    // Mixed Content Tests
    // =========================================================================

    #[test]
    fn test_parse_text_with_formatting() {
        // Input: "Hello" + ESC[31m + "Red" + ESC[0m + "World"
        // Actions:
        // - Print x5 (Hello)
        // - Csi Sgr [31]
        // - Print x3 (Red)
        // - Csi Sgr [0]
        // - Print x5 (World)
        // Total = 5 + 1 + 3 + 1 + 5 = 15
        let actions = parse_all(b"Hello\x1B[31mRed\x1B[0mWorld");
        assert_eq!(actions.len(), 15);
        assert_eq!(actions[0], TerminalAction::Print('H'));
        assert_eq!(actions[5], TerminalAction::Csi(CsiAction::Sgr(vec![31])));
        assert_eq!(actions[9], TerminalAction::Csi(CsiAction::Sgr(vec![0])));
    }

    #[test]
    fn test_parse_cursor_movement_sequence() {
        let actions = parse_all(b"\x1B[H\x1B[2J");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            TerminalAction::Csi(CsiAction::CursorPosition { row: 1, col: 1 })
        );
        assert_eq!(
            actions[1],
            TerminalAction::Csi(CsiAction::EraseInDisplay(EraseMode::All))
        );
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_parse_del_ignored() {
        let actions = parse_all(b"A\x7FB");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], TerminalAction::Print('A'));
        assert_eq!(actions[1], TerminalAction::Print('B'));
    }

    #[test]
    fn test_parse_empty_input() {
        let actions = parse_all(b"");
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_reset() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Start a sequence
        parser.parse(b"\x1B[31", |action| actions.push(action));
        assert!(actions.is_empty());

        // Reset and parse something new
        parser.reset();
        parser.parse(b"A", |action| actions.push(action));
        assert_eq!(actions, vec![TerminalAction::Print('A')]);
    }

    #[test]
    fn test_parse_c0_in_csi() {
        // C0 controls should be executed even in the middle of CSI
        let actions = parse_all(b"\x1B[1\x07;31m");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], TerminalAction::Execute(0x07)); // BEL
        assert_eq!(actions[1], TerminalAction::Csi(CsiAction::Sgr(vec![1, 31])));
    }

    #[test]
    fn test_parse_esc_in_csi_aborts() {
        // ESC in CSI should abort the CSI and start new escape
        let actions = parse_all(b"\x1B[1\x1B7");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Esc(EscAction::SaveCursor));
    }

    // =========================================================================
    // Phase 4: Cursor and Screen Operations Tests
    // =========================================================================

    #[test]
    fn test_parse_csi_cursor_next_line() {
        let actions = parse_all(b"\x1B[E");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorNextLine(1))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_next_line_with_count() {
        let actions = parse_all(b"\x1B[5E");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorNextLine(5))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_previous_line() {
        let actions = parse_all(b"\x1B[F");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPreviousLine(1))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_previous_line_with_count() {
        let actions = parse_all(b"\x1B[3F");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPreviousLine(3))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_horizontal_absolute() {
        let actions = parse_all(b"\x1B[G");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorHorizontalAbsolute(1))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_horizontal_absolute_with_col() {
        let actions = parse_all(b"\x1B[15G");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorHorizontalAbsolute(15))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_vertical_absolute() {
        let actions = parse_all(b"\x1B[d");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorVerticalAbsolute(1))]
        );
    }

    #[test]
    fn test_parse_csi_cursor_vertical_absolute_with_row() {
        let actions = parse_all(b"\x1B[10d");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorVerticalAbsolute(10))]
        );
    }

    #[test]
    fn test_parse_csi_insert_lines() {
        let actions = parse_all(b"\x1B[L");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::InsertLines(1))]
        );
    }

    #[test]
    fn test_parse_csi_insert_lines_with_count() {
        let actions = parse_all(b"\x1B[5L");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::InsertLines(5))]
        );
    }

    #[test]
    fn test_parse_csi_delete_lines() {
        let actions = parse_all(b"\x1B[M");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeleteLines(1))]
        );
    }

    #[test]
    fn test_parse_csi_delete_lines_with_count() {
        let actions = parse_all(b"\x1B[3M");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeleteLines(3))]
        );
    }

    #[test]
    fn test_parse_csi_insert_characters() {
        let actions = parse_all(b"\x1B[@");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::InsertCharacters(1))]
        );
    }

    #[test]
    fn test_parse_csi_insert_characters_with_count() {
        let actions = parse_all(b"\x1B[10@");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::InsertCharacters(10))]
        );
    }

    #[test]
    fn test_parse_csi_delete_characters() {
        let actions = parse_all(b"\x1B[P");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeleteCharacters(1))]
        );
    }

    #[test]
    fn test_parse_csi_delete_characters_with_count() {
        let actions = parse_all(b"\x1B[4P");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::DeleteCharacters(4))]
        );
    }

    #[test]
    fn test_parse_csi_erase_characters() {
        let actions = parse_all(b"\x1B[X");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::EraseCharacters(1))]
        );
    }

    #[test]
    fn test_parse_csi_erase_characters_with_count() {
        let actions = parse_all(b"\x1B[8X");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::EraseCharacters(8))]
        );
    }

    #[test]
    fn test_parse_csi_scroll_up() {
        let actions = parse_all(b"\x1B[S");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::ScrollUp(1))]);
    }

    #[test]
    fn test_parse_csi_scroll_up_with_count() {
        let actions = parse_all(b"\x1B[5S");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::ScrollUp(5))]);
    }

    #[test]
    fn test_parse_csi_scroll_down() {
        let actions = parse_all(b"\x1B[T");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::ScrollDown(1))]);
    }

    #[test]
    fn test_parse_csi_scroll_down_with_count() {
        let actions = parse_all(b"\x1B[3T");
        assert_eq!(actions, vec![TerminalAction::Csi(CsiAction::ScrollDown(3))]);
    }

    #[test]
    fn test_parse_csi_set_scroll_region() {
        let actions = parse_all(b"\x1B[r");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::SetScrollRegion {
                top: 1,
                bottom: 0
            })]
        );
    }

    #[test]
    fn test_parse_csi_set_scroll_region_with_params() {
        let actions = parse_all(b"\x1B[5;20r");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::SetScrollRegion {
                top: 5,
                bottom: 20
            })]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position_f() {
        // Test the 'f' variant of cursor position
        let actions = parse_all(b"\x1B[10;20f");
        assert_eq!(
            actions,
            vec![TerminalAction::Csi(CsiAction::CursorPosition {
                row: 10,
                col: 20
            })]
        );
    }

    // =========================================================================
    // Issue 2: OSC Termination Handling Tests
    // =========================================================================

    #[test]
    fn test_parse_osc_bel_terminator() {
        // OSC terminated with BEL
        let actions = parse_all(b"\x1B]2;Window Title\x07");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetTitle("Window Title".to_string()))
        );
    }

    #[test]
    fn test_parse_osc_st_terminator_proper() {
        // OSC terminated with ST (ESC \)
        let actions = parse_all(b"\x1B]0;Full Title\x1B\\");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetTitleAndIcon("Full Title".to_string()))
        );
    }

    #[test]
    fn test_parse_osc_st_split_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split OSC with ST terminator across buffers
        parser.parse(b"\x1B]2;Title\x1B", |action| actions.push(action));
        assert!(actions.is_empty()); // Not complete yet

        parser.parse(b"\\", |action| actions.push(action));
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetTitle("Title".to_string()))
        );
    }

    #[test]
    fn test_parse_osc_esc_without_backslash() {
        // OSC with ESC followed by something other than backslash
        // Should dispatch OSC and process the next byte as escape sequence
        let actions = parse_all(b"\x1B]2;Title\x1B7");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetTitle("Title".to_string()))
        );
        assert_eq!(actions[1], TerminalAction::Esc(EscAction::SaveCursor));
    }

    #[test]
    fn test_parse_osc_multiple_with_different_terminators() {
        // Mix BEL and ST terminators
        let actions = parse_all(b"\x1B]1;Icon\x07\x1B]2;Title\x1B\\");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            TerminalAction::Osc(OscAction::SetIconName("Icon".to_string()))
        );
        assert_eq!(
            actions[1],
            TerminalAction::Osc(OscAction::SetTitle("Title".to_string()))
        );
    }

    // =========================================================================
    // Issue 3: CSI Cancellation Handling Tests
    // =========================================================================

    #[test]
    fn test_parse_csi_with_valid_intermediate() {
        // CSI with space intermediate (cursor style)
        let actions = parse_all(b"\x1B[1 q");
        // Should be parsed as unknown but not canceled
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            TerminalAction::Csi(CsiAction::Unknown { .. }) => {}
            _ => panic!("Expected Unknown CSI action"),
        }
    }

    #[test]
    fn test_parse_csi_with_invalid_intermediate_cancels() {
        // CSI with invalid intermediate byte should cancel
        let actions = parse_all(b"\x1B[1#mA");
        // The CSI should be canceled (# consumed), m and A should be printed
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], TerminalAction::Print('m'));
        assert_eq!(actions[1], TerminalAction::Print('A'));
    }

    #[test]
    fn test_parse_csi_multiple_invalid_intermediates() {
        // Multiple invalid intermediate bytes
        let actions = parse_all(b"\x1B[#$%A");
        // CSI should be canceled at first invalid byte (#), then $, %, A printed
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0], TerminalAction::Print('$'));
        assert_eq!(actions[1], TerminalAction::Print('%'));
        assert_eq!(actions[2], TerminalAction::Print('A'));
    }

    #[test]
    fn test_parse_csi_cancel_then_valid_sequence() {
        // Invalid CSI, then valid CSI
        let actions = parse_all(b"\x1B[#m\x1B[1m");
        // First CSI canceled (# consumed), m printed, second CSI should work
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], TerminalAction::Print('m'));
        match &actions[1] {
            TerminalAction::Csi(CsiAction::Sgr(params)) => {
                assert_eq!(params, &vec![1]);
            }
            _ => panic!("Expected Sgr action"),
        }
    }

    #[test]
    fn test_parse_csi_dec_private_not_canceled() {
        // DEC private mode with '?' should not cancel
        let actions = parse_all(b"\x1B[?25h");
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            TerminalAction::Csi(CsiAction::SetMode(_))
        ));
    }

    #[test]
    fn test_parse_csi_esc_aborts_and_starts_new() {
        // ESC in CSI should abort CSI and start new escape
        let actions = parse_all(b"\x1B[31\x1B7");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Esc(EscAction::SaveCursor));
    }

    // =========================================================================
    // Issue 1: UTF-8 Multibyte Processing Tests
    // =========================================================================

    #[test]
    fn test_parse_utf8_2byte_japanese_hiragana() {
        // Japanese Hiragana "あ" (U+3042) = 0xE3 0x81 0x82
        let actions = parse_all("あ".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Print('あ'));
    }

    #[test]
    fn test_parse_utf8_3byte_chinese() {
        // Chinese "中" (U+4E2D) = 0xE4 0xB8 0xAD
        let actions = parse_all("中".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Print('中'));
    }

    #[test]
    fn test_parse_utf8_4byte_emoji() {
        // Emoji "😀" (U+1F600) = 0xF0 0x9F 0x98 0x80
        let actions = parse_all("😀".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Print('😀'));
    }

    #[test]
    fn test_parse_utf8_mixed_ascii_and_multibyte() {
        // "Hello世界" = ASCII + Chinese characters
        let actions = parse_all("Hello世界".as_bytes());
        assert_eq!(actions.len(), 7);
        assert_eq!(actions[0], TerminalAction::Print('H'));
        assert_eq!(actions[1], TerminalAction::Print('e'));
        assert_eq!(actions[2], TerminalAction::Print('l'));
        assert_eq!(actions[3], TerminalAction::Print('l'));
        assert_eq!(actions[4], TerminalAction::Print('o'));
        assert_eq!(actions[5], TerminalAction::Print('世'));
        assert_eq!(actions[6], TerminalAction::Print('界'));
    }

    #[test]
    fn test_parse_utf8_split_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split "中" (0xE4 0xB8 0xAD) across two parse calls
        parser.parse(&[0xE4, 0xB8], |action| actions.push(action));
        assert!(actions.is_empty()); // Not complete yet

        parser.parse(&[0xAD], |action| actions.push(action));
        assert_eq!(actions, vec![TerminalAction::Print('中')]);
    }

    #[test]
    fn test_parse_utf8_invalid_continuation() {
        // Continuation byte without start byte
        let actions = parse_all(&[0x80]);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], TerminalAction::Print('\u{FFFD}'));
    }

    #[test]
    fn test_parse_utf8_invalid_sequence() {
        // Invalid UTF-8 start byte
        let actions = parse_all(&[0xF8, 0x80, 0x80, 0x80]);
        assert!(actions.len() >= 1);
        assert_eq!(actions[0], TerminalAction::Print('\u{FFFD}'));
    }

    #[test]
    fn test_parse_utf8_truncated_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Start of 3-byte sequence but never complete it
        parser.parse(&[0xE4, 0xB8], |action| actions.push(action));
        assert!(actions.is_empty());

        // Send a new ASCII character - should not affect incomplete sequence
        parser.parse(b"A", |action| actions.push(action));
        assert_eq!(actions, vec![TerminalAction::Print('A')]);
    }
}
