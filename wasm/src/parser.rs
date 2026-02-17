use crate::parser_params::ParamParser;
use crate::parser_types::ParsedAction;

/// Maximum size for OSC string data.
const MAX_OSC_LEN: usize = 4096;

/// Maximum size for APC string data (Kitty Graphics).
const MAX_APC_LEN: usize = 4 * 1024 * 1024;

/// Maximum size for DCS string data (SIXEL).
const MAX_DCS_LEN: usize = 16 * 1024 * 1024;

/// Parser state machine states.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Ground,
    Escape,
    EscapeCharset(u8),
    CsiEntry,
    CsiParam,
    OscString,
    OscEscape,
    ApcString,
    ApcEscape,
    DcsString,
    DcsEscape,
}

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
            apc_buffer: Vec::new(),
            dcs_buffer: Vec::new(),
        }
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

    fn ground<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        // Handle UTF-8 continuation if we're in a multibyte sequence
        if !self.utf8_buffer.is_empty() {
            self.handle_utf8_byte(byte, emit);
            return;
        }

        match byte {
            // C0 control characters (0x00-0x1F)
            0x1B => {
                self.state = State::Escape;
            }
            0x00..=0x1A | 0x1C..=0x1F => {
                emit(ParsedAction::Execute(byte));
            }
            // DEL - ignored
            0x7F => {}
            // Printable ASCII (0x20-0x7E)
            0x20..=0x7E => {
                emit(ParsedAction::Print(byte as char));
            }
            // UTF-8 continuation byte without start byte
            0x80..=0xBF => {
                emit(ParsedAction::Print('\u{FFFD}'));
            }
            // UTF-8 start bytes
            0xC0..=0xDF => {
                self.utf8_buffer.push(byte);
                // 2-byte sequence
            }
            0xE0..=0xEF => {
                self.utf8_buffer.push(byte);
                // 3-byte sequence
            }
            0xF0..=0xF7 => {
                self.utf8_buffer.push(byte);
                // 4-byte sequence
            }
            // Invalid start bytes
            _ => {
                emit(ParsedAction::Print('\u{FFFD}'));
            }
        }
    }

    fn handle_utf8_byte<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        // Check if this is a valid continuation byte
        if (0x80..=0xBF).contains(&byte) {
            self.utf8_buffer.push(byte);

            let expected_len = match self.utf8_buffer[0] {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => {
                    self.utf8_buffer.clear();
                    emit(ParsedAction::Print('\u{FFFD}'));
                    return;
                }
            };

            if self.utf8_buffer.len() == expected_len {
                // Try to decode
                if let Ok(s) = std::str::from_utf8(&self.utf8_buffer) {
                    if let Some(ch) = s.chars().next() {
                        emit(ParsedAction::Print(ch));
                    }
                } else {
                    emit(ParsedAction::Print('\u{FFFD}'));
                }
                self.utf8_buffer.clear();
            }
        } else {
            // Invalid continuation - emit replacement and process this byte as new
            self.utf8_buffer.clear();
            // Process the current byte as ground state
            self.ground(byte, emit);
        }
    }

    fn escape<F>(&mut self, byte: u8, emit: &mut F)
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

    fn escape_charset<F>(&mut self, designator: u8, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        emit(ParsedAction::EscDispatch {
            intermediate: Some(designator),
            final_byte: byte,
        });
        self.state = State::Ground;
    }

    fn csi_entry<F>(&mut self, byte: u8, emit: &mut F)
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
            b'?' | b'>' | b'=' | b' ' => {
                self.params.add_intermediate(byte);
                self.state = State::CsiParam;
            }
            // Semicolon (parameter separator with no preceding digit)
            b';' => {
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

    fn csi_param<F>(&mut self, byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        match byte {
            // Parameter bytes (digits)
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            // Parameter separator
            b';' => {
                self.params.finish_param();
            }
            // Valid intermediate bytes (after params)
            b' ' => {
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

    fn dispatch_csi<F>(&mut self, final_byte: u8, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let params = self.params.finish();
        let intermediates = self.params.intermediates().to_vec();

        emit(ParsedAction::CsiDispatch {
            params,
            intermediates,
            final_byte,
        });
        self.params.reset();
        self.state = State::Ground;
    }

    fn osc_string<F>(&mut self, byte: u8, emit: &mut F)
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

    fn osc_escape<F>(&mut self, byte: u8, emit: &mut F)
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

    fn dispatch_osc<F>(&mut self, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let data = String::from_utf8_lossy(&self.osc_buffer).to_string();

        emit(ParsedAction::OscDispatch {
            param: self.osc_param,
            data,
        });
        self.osc_buffer.clear();
        self.osc_param = 0;
        self.osc_param_done = false;
    }

    fn apc_string(&mut self, byte: u8) {
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

    fn apc_escape<F>(&mut self, byte: u8, emit: &mut F)
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

    fn dispatch_apc<F>(&mut self, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let payload = std::mem::take(&mut self.apc_buffer);
        emit(ParsedAction::ApcDispatch(payload));
    }

    fn dcs_string(&mut self, byte: u8) {
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

    fn dcs_escape<F>(&mut self, byte: u8, emit: &mut F)
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

    fn dispatch_dcs<F>(&mut self, emit: &mut F)
    where
        F: FnMut(ParsedAction),
    {
        let payload = std::mem::take(&mut self.dcs_buffer);
        emit(ParsedAction::DcsDispatch(payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(input: &[u8]) -> Vec<ParsedAction> {
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
        assert_eq!(actions[0], ParsedAction::Print('H'));
        assert_eq!(actions[1], ParsedAction::Print('e'));
        assert_eq!(actions[2], ParsedAction::Print('l'));
        assert_eq!(actions[3], ParsedAction::Print('l'));
        assert_eq!(actions[4], ParsedAction::Print('o'));
    }

    #[test]
    fn test_parse_space() {
        let actions = parse_all(b" ");
        assert_eq!(actions, vec![ParsedAction::Print(' ')]);
    }

    #[test]
    fn test_parse_all_printable() {
        let input = b"!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let actions = parse_all(input);
        assert_eq!(actions.len(), input.len());
        for action in &actions {
            assert!(matches!(action, ParsedAction::Print(_)));
        }
    }

    // =========================================================================
    // C0 Control Character Tests
    // =========================================================================

    #[test]
    fn test_parse_c0_bel() {
        let actions = parse_all(b"\x07");
        assert_eq!(actions, vec![ParsedAction::Execute(0x07)]);
    }

    #[test]
    fn test_parse_c0_bs() {
        let actions = parse_all(b"\x08");
        assert_eq!(actions, vec![ParsedAction::Execute(0x08)]);
    }

    #[test]
    fn test_parse_c0_ht() {
        let actions = parse_all(b"\x09");
        assert_eq!(actions, vec![ParsedAction::Execute(0x09)]);
    }

    #[test]
    fn test_parse_c0_lf() {
        let actions = parse_all(b"\x0A");
        assert_eq!(actions, vec![ParsedAction::Execute(0x0A)]);
    }

    #[test]
    fn test_parse_c0_cr() {
        let actions = parse_all(b"\x0D");
        assert_eq!(actions, vec![ParsedAction::Execute(0x0D)]);
    }

    #[test]
    fn test_parse_mixed_text_and_controls() {
        let actions = parse_all(b"A\r\nB");
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[0], ParsedAction::Print('A'));
        assert_eq!(actions[1], ParsedAction::Execute(0x0D));
        assert_eq!(actions[2], ParsedAction::Execute(0x0A));
        assert_eq!(actions[3], ParsedAction::Print('B'));
    }

    // =========================================================================
    // ESC Sequence Tests
    // =========================================================================

    #[test]
    fn test_parse_esc_save_cursor() {
        let actions = parse_all(b"\x1B7");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'7',
            }]
        );
    }

    #[test]
    fn test_parse_esc_restore_cursor() {
        let actions = parse_all(b"\x1B8");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'8',
            }]
        );
    }

    #[test]
    fn test_parse_esc_index() {
        let actions = parse_all(b"\x1BD");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'D',
            }]
        );
    }

    #[test]
    fn test_parse_esc_next_line() {
        let actions = parse_all(b"\x1BE");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'E',
            }]
        );
    }

    #[test]
    fn test_parse_esc_horizontal_tab_set() {
        let actions = parse_all(b"\x1BH");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'H',
            }]
        );
    }

    #[test]
    fn test_parse_esc_reverse_index() {
        let actions = parse_all(b"\x1BM");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'M',
            }]
        );
    }

    #[test]
    fn test_parse_esc_reset() {
        let actions = parse_all(b"\x1Bc");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'c',
            }]
        );
    }

    #[test]
    fn test_parse_esc_g0_charset_ascii() {
        let actions = parse_all(b"\x1B(B");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: Some(b'('),
                final_byte: b'B',
            }]
        );
    }

    #[test]
    fn test_parse_esc_g0_charset_line_drawing() {
        let actions = parse_all(b"\x1B(0");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: Some(b'('),
                final_byte: b'0',
            }]
        );
    }

    #[test]
    fn test_parse_esc_g1_charset() {
        let actions = parse_all(b"\x1B)A");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: Some(b')'),
                final_byte: b'A',
            }]
        );
    }

    #[test]
    fn test_parse_esc_unknown() {
        let actions = parse_all(b"\x1BX");
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'X',
            }]
        );
    }

    // =========================================================================
    // CSI Sequence Tests
    // =========================================================================

    #[test]
    fn test_parse_csi_sgr_reset() {
        let actions = parse_all(b"\x1B[m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_explicit_reset() {
        let actions = parse_all(b"\x1B[0m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![0],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_bold() {
        let actions = parse_all(b"\x1B[1m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![1],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_red_foreground() {
        let actions = parse_all(b"\x1B[31m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![31],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_multiple_params() {
        let actions = parse_all(b"\x1B[1;31m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![1, 31],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_256_color() {
        let actions = parse_all(b"\x1B[38;5;196m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![38, 5, 196],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_sgr_rgb() {
        let actions = parse_all(b"\x1B[38;2;255;0;128m");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![38, 2, 255, 0, 128],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_up() {
        let actions = parse_all(b"\x1B[A");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'A',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_up_with_count() {
        let actions = parse_all(b"\x1B[5A");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![5],
                intermediates: vec![],
                final_byte: b'A',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_down() {
        let actions = parse_all(b"\x1B[3B");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![3],
                intermediates: vec![],
                final_byte: b'B',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_forward() {
        let actions = parse_all(b"\x1B[10C");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![10],
                intermediates: vec![],
                final_byte: b'C',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_back() {
        let actions = parse_all(b"\x1B[2D");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![2],
                intermediates: vec![],
                final_byte: b'D',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position() {
        let actions = parse_all(b"\x1B[10;20H");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![10, 20],
                intermediates: vec![],
                final_byte: b'H',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position_default() {
        let actions = parse_all(b"\x1B[H");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'H',
            }]
        );
    }

    #[test]
    fn test_parse_csi_cursor_position_partial() {
        let actions = parse_all(b"\x1B[;10H");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![0, 10],
                intermediates: vec![],
                final_byte: b'H',
            }]
        );
    }

    #[test]
    fn test_parse_csi_erase_display_below() {
        let actions = parse_all(b"\x1B[J");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'J',
            }]
        );
    }

    #[test]
    fn test_parse_csi_erase_display_all() {
        let actions = parse_all(b"\x1B[2J");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![2],
                intermediates: vec![],
                final_byte: b'J',
            }]
        );
    }

    #[test]
    fn test_parse_csi_erase_line() {
        let actions = parse_all(b"\x1B[K");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'K',
            }]
        );
    }

    #[test]
    fn test_parse_csi_dec_private_set_mode() {
        let actions = parse_all(b"\x1B[?25h");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![25],
                intermediates: vec![b'?'],
                final_byte: b'h',
            }]
        );
    }

    #[test]
    fn test_parse_csi_dec_private_reset_mode() {
        let actions = parse_all(b"\x1B[?25l");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![25],
                intermediates: vec![b'?'],
                final_byte: b'l',
            }]
        );
    }

    #[test]
    fn test_parse_csi_device_status_report() {
        let actions = parse_all(b"\x1B[6n");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![6],
                intermediates: vec![],
                final_byte: b'n',
            }]
        );
    }

    #[test]
    fn test_parse_csi_primary_device_attributes() {
        let actions = parse_all(b"\x1B[c");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'c',
            }]
        );
    }

    #[test]
    fn test_parse_csi_secondary_device_attributes() {
        let actions = parse_all(b"\x1B[>c");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![b'>'],
                final_byte: b'c',
            }]
        );
    }

    #[test]
    fn test_parse_csi_tertiary_device_attributes() {
        let actions = parse_all(b"\x1B[=c");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![b'='],
                final_byte: b'c',
            }]
        );
    }

    #[test]
    fn test_parse_csi_unknown() {
        let actions = parse_all(b"\x1B[1;2;3z");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![1, 2, 3],
                intermediates: vec![],
                final_byte: b'z',
            }]
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
            vec![ParsedAction::OscDispatch {
                param: 2,
                data: "My Title".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_set_title_and_icon() {
        let actions = parse_all(b"\x1B]0;Terminal\x07");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 0,
                data: "Terminal".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_working_directory() {
        let actions = parse_all(b"\x1B]7;file:///home/user\x07");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 7,
                data: "file:///home/user".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_hyperlink() {
        let actions = parse_all(b"\x1B]8;id=1;https://example.com\x07");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 8,
                data: "id=1;https://example.com".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_unknown() {
        let actions = parse_all(b"\x1B]99;data\x07");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 99,
                data: "data".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_semantic_prompt_a() {
        let actions = parse_all(b"\x1B]133;A\x1B\\");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 133,
                data: "A".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_semantic_prompt_d_with_exit_code() {
        let actions = parse_all(b"\x1B]133;D;0\x1B\\");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 133,
                data: "D;0".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_emterm_extension() {
        let actions = parse_all(b"\x1B]777;markdown;title;body\x07");
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 777,
                data: "markdown;title;body".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_osc_st_terminator() {
        let actions = parse_all(b"\x1B]2;My Title\x1B\\");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            ParsedAction::OscDispatch {
                param: 2,
                data: "My Title".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_osc_esc_without_backslash() {
        let actions = parse_all(b"\x1B]2;Title\x1B7");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            ParsedAction::OscDispatch {
                param: 2,
                data: "Title".to_string(),
            }
        );
        assert_eq!(
            actions[1],
            ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'7',
            }
        );
    }

    // =========================================================================
    // Buffer Boundary Tests (Streaming Input)
    // =========================================================================

    #[test]
    fn test_parse_split_csi_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1B[", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"31m", |action| actions.push(action));
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![31],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_split_esc_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1B", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"7", |action| actions.push(action));
        assert_eq!(
            actions,
            vec![ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'7',
            }]
        );
    }

    #[test]
    fn test_parse_split_osc_sequence() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1B]2;My ", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"Title\x07", |action| actions.push(action));
        assert_eq!(
            actions,
            vec![ParsedAction::OscDispatch {
                param: 2,
                data: "My Title".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_split_byte_by_byte() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        for byte in b"\x1B[1;31m" {
            parser.parse(&[*byte], |action| actions.push(action));
        }

        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![1, 31],
                intermediates: vec![],
                final_byte: b'm',
            }]
        );
    }

    #[test]
    fn test_parse_split_osc_st_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1B]2;Title\x1B", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"\\", |action| actions.push(action));
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            ParsedAction::OscDispatch {
                param: 2,
                data: "Title".to_string(),
            }
        );
    }

    // =========================================================================
    // UTF-8 Tests
    // =========================================================================

    #[test]
    fn test_parse_utf8_japanese_hiragana() {
        let actions = parse_all("あ".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], ParsedAction::Print('あ'));
    }

    #[test]
    fn test_parse_utf8_chinese() {
        let actions = parse_all("中".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], ParsedAction::Print('中'));
    }

    #[test]
    fn test_parse_utf8_emoji() {
        let actions = parse_all("😀".as_bytes());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], ParsedAction::Print('😀'));
    }

    #[test]
    fn test_parse_utf8_mixed_ascii_and_multibyte() {
        let actions = parse_all("Hello世界".as_bytes());
        assert_eq!(actions.len(), 7);
        assert_eq!(actions[0], ParsedAction::Print('H'));
        assert_eq!(actions[5], ParsedAction::Print('世'));
        assert_eq!(actions[6], ParsedAction::Print('界'));
    }

    #[test]
    fn test_parse_utf8_split_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        // Split "中" (0xE4 0xB8 0xAD) across two parse calls
        parser.parse(&[0xE4, 0xB8], |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(&[0xAD], |action| actions.push(action));
        assert_eq!(actions, vec![ParsedAction::Print('中')]);
    }

    #[test]
    fn test_parse_utf8_invalid_continuation() {
        let actions = parse_all(&[0x80]);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], ParsedAction::Print('\u{FFFD}'));
    }

    #[test]
    fn test_parse_utf8_invalid_sequence() {
        let actions = parse_all(&[0xF8, 0x80, 0x80, 0x80]);
        assert!(actions.len() >= 1);
        assert_eq!(actions[0], ParsedAction::Print('\u{FFFD}'));
    }

    // =========================================================================
    // APC Tests (Raw payload - no Kitty parsing)
    // =========================================================================

    #[test]
    fn test_parse_apc_basic() {
        let actions = parse_all(b"\x1B_Ga=T,f=100;iVBORw0KGgo=\x1B\\");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ParsedAction::ApcDispatch(payload) => {
                let s = String::from_utf8_lossy(payload);
                assert_eq!(s, "Ga=T,f=100;iVBORw0KGgo=");
            }
            _ => panic!("Expected ApcDispatch"),
        }
    }

    #[test]
    fn test_parse_apc_split_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1B_Ga=T,f=100;iVBO", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"Rw0KGgo=\x1B\\", |action| actions.push(action));
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ParsedAction::ApcDispatch(payload) => {
                let s = String::from_utf8_lossy(payload);
                assert_eq!(s, "Ga=T,f=100;iVBORw0KGgo=");
            }
            _ => panic!("Expected ApcDispatch"),
        }
    }

    #[test]
    fn test_parse_apc_followed_by_text() {
        let actions = parse_all(b"\x1B_Ga=q;\x1B\\Hello");
        assert_eq!(actions.len(), 6); // 1 APC + 5 chars
        assert!(matches!(&actions[0], ParsedAction::ApcDispatch(_)));
        assert_eq!(actions[1], ParsedAction::Print('H'));
    }

    // =========================================================================
    // DCS Tests (Raw payload - no SIXEL parsing)
    // =========================================================================

    #[test]
    fn test_parse_dcs_basic() {
        let actions = parse_all(b"\x1BP0;1;0q#0;2;100;0;0~\x1B\\");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ParsedAction::DcsDispatch(payload) => {
                let s = String::from_utf8_lossy(payload);
                assert_eq!(s, "0;1;0q#0;2;100;0;0~");
            }
            _ => panic!("Expected DcsDispatch"),
        }
    }

    #[test]
    fn test_parse_dcs_split_across_buffers() {
        let mut parser = Parser::new();
        let mut actions = Vec::new();

        parser.parse(b"\x1BP0;1;0q#0;2;", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.parse(b"100;0;0~\x1B\\", |action| actions.push(action));
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ParsedAction::DcsDispatch(_)));
    }

    #[test]
    fn test_parse_dcs_followed_by_text() {
        let actions = parse_all(b"\x1BPq~\x1B\\Hello");
        assert_eq!(actions.len(), 6); // 1 DCS + 5 chars
        assert!(matches!(&actions[0], ParsedAction::DcsDispatch(_)));
        assert_eq!(actions[1], ParsedAction::Print('H'));
    }

    #[test]
    fn test_parse_mixed_apc_and_dcs() {
        let actions = parse_all(b"\x1B_Ga=q;\x1B\\\x1BPq~\x1B\\");
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], ParsedAction::ApcDispatch(_)));
        assert!(matches!(&actions[1], ParsedAction::DcsDispatch(_)));
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_parse_del_ignored() {
        let actions = parse_all(b"A\x7FB");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], ParsedAction::Print('A'));
        assert_eq!(actions[1], ParsedAction::Print('B'));
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

        parser.parse(b"\x1B[31", |action| actions.push(action));
        assert!(actions.is_empty());

        parser.reset();
        parser.parse(b"A", |action| actions.push(action));
        assert_eq!(actions, vec![ParsedAction::Print('A')]);
    }

    #[test]
    fn test_parse_c0_in_csi() {
        let actions = parse_all(b"\x1B[1\x07;31m");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], ParsedAction::Execute(0x07));
        assert_eq!(
            actions[1],
            ParsedAction::CsiDispatch {
                params: vec![1, 31],
                intermediates: vec![],
                final_byte: b'm',
            }
        );
    }

    #[test]
    fn test_parse_esc_in_csi_aborts() {
        let actions = parse_all(b"\x1B[1\x1B7");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            ParsedAction::EscDispatch {
                intermediate: None,
                final_byte: b'7',
            }
        );
    }

    #[test]
    fn test_parse_text_with_formatting() {
        let actions = parse_all(b"Hello\x1B[31mRed\x1B[0mWorld");
        assert_eq!(actions.len(), 15);
        assert_eq!(actions[0], ParsedAction::Print('H'));
        assert_eq!(
            actions[5],
            ParsedAction::CsiDispatch {
                params: vec![31],
                intermediates: vec![],
                final_byte: b'm',
            }
        );
        assert_eq!(
            actions[9],
            ParsedAction::CsiDispatch {
                params: vec![0],
                intermediates: vec![],
                final_byte: b'm',
            }
        );
    }

    #[test]
    fn test_parse_cursor_movement_sequence() {
        let actions = parse_all(b"\x1B[H\x1B[2J");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            ParsedAction::CsiDispatch {
                params: vec![],
                intermediates: vec![],
                final_byte: b'H',
            }
        );
        assert_eq!(
            actions[1],
            ParsedAction::CsiDispatch {
                params: vec![2],
                intermediates: vec![],
                final_byte: b'J',
            }
        );
    }

    #[test]
    fn test_parse_csi_with_space_intermediate() {
        let actions = parse_all(b"\x1B[1 q");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            ParsedAction::CsiDispatch {
                params: vec![1],
                intermediates: vec![b' '],
                final_byte: b'q',
            }
        );
    }

    #[test]
    fn test_parse_scroll_region() {
        let actions = parse_all(b"\x1B[5;20r");
        assert_eq!(
            actions,
            vec![ParsedAction::CsiDispatch {
                params: vec![5, 20],
                intermediates: vec![],
                final_byte: b'r',
            }]
        );
    }
}
