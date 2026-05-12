use super::Parser;
use super::state::State;
use crate::parser_types::ParsedAction;

impl Parser {
    pub(super) fn ground<F>(&mut self, byte: u8, emit: &mut F)
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

    pub(super) fn handle_utf8_byte<F>(&mut self, byte: u8, emit: &mut F)
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
}
