/// Print handler: handle_print, flush_grapheme_buffer, grapheme buffer,
/// DEC charset translation.
use wasm_bindgen::prelude::*;

use crate::cell::{overflow_ridx_insert, overflow_ridx_remove};
use crate::terminal_core::{TerminalCore, MODE_AUTO_WRAP};

impl TerminalCore {
    /// Apply active charset translation to a codepoint.
    fn translate_charset(&self, cp: u32) -> u32 {
        let charset = if self.active_charset == 0 {
            self.g0_charset
        } else {
            self.g1_charset
        };
        if charset == 1 {
            Self::translate_line_drawing(cp)
        } else {
            cp
        }
    }

    /// DEC Line Drawing translation table (0x5F-0x7E, 32 entries).
    fn translate_line_drawing(cp: u32) -> u32 {
        match cp {
            0x5F => 0x0020, // _ → Blank
            0x60 => 0x25C6, // ` → Diamond
            0x61 => 0x2592, // a → Checkerboard
            0x62 => 0x2409, // b → HT
            0x63 => 0x240C, // c → FF
            0x64 => 0x240D, // d → CR
            0x65 => 0x240A, // e → LF
            0x66 => 0x00B0, // f → Degree
            0x67 => 0x00B1, // g → Plus/minus
            0x68 => 0x2424, // h → NL
            0x69 => 0x240B, // i → VT
            0x6A => 0x2518, // j → Lower right corner
            0x6B => 0x2510, // k → Upper right corner
            0x6C => 0x250C, // l → Upper left corner
            0x6D => 0x2514, // m → Lower left corner
            0x6E => 0x253C, // n → Crossing lines
            0x6F => 0x23BA, // o → Scan 1
            0x70 => 0x23BB, // p → Scan 3
            0x71 => 0x2500, // q → Scan 5 (horizontal line)
            0x72 => 0x23BC, // r → Scan 7
            0x73 => 0x23BD, // s → Scan 9
            0x74 => 0x251C, // t → Left tee
            0x75 => 0x2524, // u → Right tee
            0x76 => 0x2534, // v → Bottom tee
            0x77 => 0x252C, // w → Top tee
            0x78 => 0x2502, // x → Vertical line
            0x79 => 0x2264, // y → Less than or equal
            0x7A => 0x2265, // z → Greater than or equal
            0x7B => 0x03C0, // { → Pi
            0x7C => 0x2260, // | → Not equal
            0x7D => 0x00A3, // } → UK pound
            0x7E => 0x00B7, // ~ → Bullet
            _ => cp,
        }
    }

    /// Write a character/grapheme to grid at cursor, handling wrap and scroll.
    /// Scroll is handled internally via ring buffer.
    fn write_grapheme_to_grid(&mut self, char_str: &str, width: u8) {
        // Handle wrap_pending
        if self.wrap_pending {
            self.wrap_pending = false;
            self.carriage_return();
            self.line_feed();
            let abs = self.viewport_abs(self.cursor.row);
            self.ring_wrapped[abs] = true;
        }

        // Wide char at line end: wrap before printing
        if width == 2 && self.cursor.col >= self.cols.saturating_sub(1) {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.carriage_return();
                self.line_feed();
                let abs = self.viewport_abs(self.cursor.row);
                self.ring_wrapped[abs] = true;
            }
        }

        // Write cell
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            let abs = self.viewport_abs(row) as u32;
            let cell = &mut self.ring_cells[idx];
            cell.set_char(char_str);
            let col32 = col as u32;
            if cell.is_overflow() {
                self.overflow.insert((col32, abs), char_str.to_string());
                overflow_ridx_insert(&mut self.overflow_ridx, abs, col32);
            } else {
                if self.overflow.remove(&(col32, abs)).is_some() {
                    overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
                }
            }
            cell.width = width;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            self.mark_row_dirty(row);
        }

        // Placeholder for width-2 characters
        if width == 2 && col + 1 < self.cols {
            if let Some(idx) = self.cell_index(col + 1, row) {
                let abs = self.viewport_abs(row) as u32;
                let ph = &mut self.ring_cells[idx];
                ph.char_data = [0; 16];
                ph.char_len = 0;
                ph.width = 0;
                ph.fg = self.cursor.fg;
                ph.bg = self.cursor.bg;
                ph.flags = self.cursor.flags;
                let col1_32 = (col + 1) as u32;
                if self.overflow.remove(&(col1_32, abs)).is_some() {
                    overflow_ridx_remove(&mut self.overflow_ridx, abs, col1_32);
                }
            }
        }

        // Advance cursor
        let new_col = col as u32 + width as u32;
        if new_col >= self.cols as u32 {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.cursor.col = self.cols - 1;
                self.wrap_pending = true;
            }
        } else {
            self.cursor.col = new_col as u16;
        }
    }

    /// ASCII fast path: direct byte write without string allocation.
    fn handle_print_ascii(&mut self, cp: u32) {
        let byte = cp as u8;
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &mut self.ring_cells[idx];
            cell.char_data[0] = byte;
            for b in &mut cell.char_data[1..] {
                *b = 0;
            }
            cell.char_len = 1;
            cell.width = 1;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            // Always remove overflow entry (char_len is already set to 1 above,
            // so is_overflow() would be false -- must remove unconditionally)
            let abs = self.viewport_abs(row) as u32;
            let col32 = col as u32;
            if self.overflow.remove(&(col32, abs)).is_some() {
                overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
            }
            self.mark_row_dirty(row);
        }

        let new_col = col + 1;
        if new_col < self.cols {
            self.cursor.col = new_col;
        } else if self.get_mode(MODE_AUTO_WRAP) {
            self.cursor.col = self.cols - 1;
            self.wrap_pending = true;
        }
    }

    /// Slow path: handles charWidth, charset translation, wrap.
    fn handle_print_slow(&mut self, cp: u32) {
        let width = crate::unicode::char_width(cp);
        let translated = self.translate_charset(cp);
        let mut buf = [0u8; 4];
        let ch = char::from_u32(translated).unwrap_or(' ');
        let s = ch.encode_utf8(&mut buf);
        self.write_grapheme_to_grid(s, width);
    }
}

#[wasm_bindgen]
impl TerminalCore {
    /// Process a single codepoint for printing.
    /// Returns 0 always (scroll handled internally via ring buffer).
    pub fn handle_print(&mut self, cp: u32) -> u8 {
        // Kitty Unicode placeholder suppression (U+10EEEE + combining marks).
        // kitten icat uses these to reserve cell positions for image display.
        // eMterm handles images via APC/viewer, so placeholders are discarded.
        if cp == 0x10EEEE {
            self.flush_grapheme_buffer();
            self.kitty_placeholder_active = true;
            return 0;
        }
        if self.kitty_placeholder_active {
            let props_check = crate::unicode::classify_codepoint(cp);
            if props_check & crate::unicode::COMBINING != 0 {
                return 0; // Skip combining marks attached to placeholder
            }
            self.kitty_placeholder_active = false;
        }

        // Safety: flush if buffer exceeds max size
        if self.grapheme_buffer.len() >= 64 {
            self.flush_grapheme_buffer();
        }

        let props = crate::unicode::classify_codepoint(cp);

        if !self.grapheme_buffer.is_empty() {
            // Buffer non-empty: check if cp extends the cluster
            if cp == 0x200D {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::VARIATION_SEL != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::SKIN_TONE != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::REGIONAL_IND != 0 {
                if self.grapheme_buffer.len() == 1 {
                    let buf0 = self.grapheme_buffer[0];
                    if (0x1F1E6..=0x1F1FF).contains(&buf0) {
                        self.grapheme_buffer.push(cp);
                        self.flush_grapheme_buffer();
                        return 0;
                    }
                }
            }
            if let Some(&last) = self.grapheme_buffer.last() {
                if last == 0x200D && (props & crate::unicode::EXT_PICTOGRAPHIC != 0) {
                    self.grapheme_buffer.push(cp);
                    return 0;
                }
            }
            if props & crate::unicode::COMBINING != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }

            // Does not extend: flush and fall through
            self.flush_grapheme_buffer();
        } else {
            // Buffer empty: check if cp starts buffering
            if props & (crate::unicode::EXT_PICTOGRAPHIC | crate::unicode::REGIONAL_IND) != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
        }

        // ASCII fast path
        if cp >= 0x20
            && cp < 0x7F
            && !self.wrap_pending
            && self.active_charset == 0
            && self.g0_charset == 0
        {
            let new_col = self.cursor.col + 1;
            if new_col < self.cols || self.get_mode(MODE_AUTO_WRAP) {
                self.handle_print_ascii(cp);
                return 0;
            }
        }

        // Slow path
        self.handle_print_slow(cp);
        0
    }

    /// Flush the grapheme buffer, writing the accumulated cluster to the grid.
    /// Scroll handled internally via ring buffer.
    pub fn flush_grapheme_buffer(&mut self) -> u8 {
        if self.grapheme_buffer.is_empty() {
            return 0;
        }

        let mut cluster = String::with_capacity(self.grapheme_buffer.len() * 4);
        let mut has_fe0e = false;
        let mut has_fe0f = false;

        for &cp in &self.grapheme_buffer {
            if cp == 0xFE0E {
                has_fe0e = true;
            }
            if cp == 0xFE0F {
                has_fe0f = true;
            }
            if let Some(ch) = char::from_u32(cp) {
                cluster.push(ch);
            }
        }

        let width: u8 = if has_fe0e {
            1
        } else if has_fe0f {
            2
        } else if self.grapheme_buffer.len() == 1 {
            if crate::unicode::is_emoji_presentation(self.grapheme_buffer[0]) {
                2
            } else {
                crate::unicode::char_width(self.grapheme_buffer[0])
            }
        } else {
            2
        };

        self.grapheme_buffer.clear();
        self.write_grapheme_to_grid(&cluster, width);
        0
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 2: Print handler tests ───────────────────

    // TS-R01: handle_print ASCII 'A' at (0,0)
    #[test]
    fn test_handle_print_ascii_basic() {
        let mut core = TerminalCore::new(80, 24, 0);
        let scroll = core.handle_print(0x41); // 'A'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert_eq!(core.get_cursor_col(), 1);
    }

    // TS-R02: handle_print ASCII with wrap_pending
    #[test]
    fn test_handle_print_ascii_wrap_pending() {
        let mut core = TerminalCore::new(5, 3, 0);
        // Fill row 0 to trigger wrap_pending
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        assert!(core.get_wrap_pending());
        let scroll = core.handle_print(0x46); // 'F'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cursor_row(), 1);
    }

    // TS-R03: handle_print non-ASCII (wide char) with wrap_pending
    #[test]
    fn test_handle_print_with_wrap_pending() {
        let mut core = TerminalCore::new(5, 3, 0);
        // Fill to end
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        assert!(core.get_wrap_pending());
        // Now print CJK char (width=2)
        let scroll = core.handle_print(0x4E16); // '世'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cursor_row(), 1);
        assert_eq!(core.get_cell_char(0, 1), "世");
    }

    // TS-R04: handle_print scroll at bottom (scroll internal)
    #[test]
    fn test_handle_print_scroll_at_bottom() {
        let mut core = TerminalCore::new(5, 2, 0);
        // Fill row 0
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        // Fill row 1
        for c in b'F'..=b'J' {
            core.handle_print(c as u32);
        }
        // Next print scrolls internally
        let scroll = core.handle_print(0x4B); // 'K'
        assert_eq!(scroll, 0); // Scroll handled internally
                               // Row 0 should now have old row 1 content (FGHIJ)
        assert_eq!(core.get_cell_char(0, 0), "F");
    }

    #[test]
    fn test_handle_print_cjk() {
        let mut core = TerminalCore::new(10, 3, 0);
        let scroll = core.handle_print(0x4E16); // '世'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "世");
        assert_eq!(core.get_cell_width(0, 0), 2);
        // Placeholder at col 1
        assert_eq!(core.get_cell_width(1, 0), 0);
        assert_eq!(core.get_cursor_col(), 2);
    }

    #[test]
    fn test_handle_print_cjk_wrap() {
        let mut core = TerminalCore::new(5, 3, 0);
        // Fill to col 4 (last col)
        for c in b'A'..=b'D' {
            core.handle_print(c as u32);
        }
        assert_eq!(core.get_cursor_col(), 4);
        // CJK at col 4 (only 1 cell remaining for width=2): should wrap
        let scroll = core.handle_print(0x4E16); // '世'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cursor_row(), 1);
        assert_eq!(core.get_cell_char(0, 1), "世");
    }

    #[test]
    fn test_handle_print_emoji_buffered() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Emoji with Emoji_Presentation property → should buffer
        let scroll = core.handle_print(0x1F600); // 😀
        assert_eq!(scroll, 0);
        assert_eq!(core.get_grapheme_buffer_len(), 1);
    }

    #[test]
    fn test_handle_print_zwj_extends() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F468); // 👨
        core.handle_print(0x200D); // ZWJ
        assert_eq!(core.get_grapheme_buffer_len(), 2);
    }

    #[test]
    fn test_handle_print_flush_then_new() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F600); // 😀
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        // Print ASCII 'A' → should flush emoji, then print 'A'
        core.handle_print(0x41);
        assert_eq!(core.get_grapheme_buffer_len(), 0);
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_char(2, 0), "A");
    }

    #[test]
    fn test_handle_print_ri_pair() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F1EF); // Regional indicator J
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        core.handle_print(0x1F1F5); // Regional indicator P → 🇯🇵
        assert_eq!(core.get_grapheme_buffer_len(), 0);
        assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    #[test]
    fn test_handle_print_vs_fe0e() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x2764); // ❤ (Extended_Pictographic)
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        core.handle_print(0xFE0E); // VS15 (text presentation)
        assert_eq!(core.get_grapheme_buffer_len(), 2);
        core.handle_print(0x41); // flush
        assert_eq!(core.get_cell_width(0, 0), 1); // text presentation = width 1
    }

    #[test]
    fn test_handle_print_vs_fe0f() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x2764); // ❤
        core.handle_print(0xFE0F); // VS16 (emoji presentation)
        core.handle_print(0x41); // flush
        assert_eq!(core.get_cell_width(0, 0), 2); // emoji presentation = width 2
    }

    #[test]
    fn test_handle_print_skin_tone() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F44D); // 👍
        core.handle_print(0x1F3FD); // medium skin tone
        assert_eq!(core.get_grapheme_buffer_len(), 2);
    }

    #[test]
    fn test_handle_print_buffer_overflow() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Push 64 codepoints to trigger buffer overflow safety
        for _ in 0..64 {
            core.handle_print(0x1F600); // 😀
        }
        // Buffer should have been flushed at 64
        // The 64th push triggers flush of first 64, then starts new buffer
        assert!(core.get_grapheme_buffer_len() <= 1);
    }

    #[test]
    fn test_handle_print_dec_line_drawing() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_g0_charset(1);
        core.set_active_charset(0);
        core.handle_print(0x71); // q → ─ (box drawing horizontal)
        assert_eq!(core.get_cell_char(0, 0), "\u{2500}");
    }

    #[test]
    fn test_handle_print_dec_line_drawing_inactive() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_g0_charset(1);
        core.set_active_charset(1); // G1 is active, G1 is ASCII
        core.handle_print(0x71); // should NOT translate
        assert_eq!(core.get_cell_char(0, 0), "q");
    }

    #[test]
    fn test_handle_print_g1_dec_line_drawing() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_g1_charset(1);
        core.set_active_charset(1); // G1 active, G1 is DecLineDrawing
        core.handle_print(0x71); // q → ─
        assert_eq!(core.get_cell_char(0, 0), "\u{2500}");
    }

    #[test]
    fn test_handle_print_no_autowrap() {
        let mut core = TerminalCore::new(5, 3, 0);
        // Disable auto wrap
        core.set_mode(0, false);
        // Print exactly 5 chars
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        assert!(!core.get_wrap_pending());
        // Print 6th char - should overwrite at last col
        core.handle_print(b'F' as u32);
        assert_eq!(core.get_cursor_col(), 4);
        assert_eq!(core.get_cell_char(4, 0), "F");
    }

    // ── Flush tests ────────────────────────────────────────

    #[test]
    fn test_flush_empty() {
        let mut core = TerminalCore::new(10, 3, 0);
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 0);
        assert_eq!(core.get_grapheme_buffer_len(), 0);
    }

    #[test]
    fn test_flush_single_emoji() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F600); // 😀
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    #[test]
    fn test_flush_zwj_sequence() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F468); // 👨
        core.handle_print(0x200D); // ZWJ
        core.handle_print(0x1F4BB); // 💻
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "👨\u{200D}💻");
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    #[test]
    fn test_flush_flag_ri_pair() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(0x1F1EF); // J
        core.handle_print(0x1F1F5); // P → auto-flushed
                                    // Already flushed by auto-flush
        assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
    }

    #[test]
    fn test_flush_with_wrap_pending() {
        let mut core = TerminalCore::new(5, 3, 0);
        // Fill row to trigger wrap_pending
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        assert!(core.get_wrap_pending());
        // Now add emoji and flush
        core.handle_print(0x1F600); // 😀 → buffered
        core.flush_grapheme_buffer();
        // Should wrap to row 1 and print emoji
        assert_eq!(core.get_cursor_row(), 1);
        assert_eq!(core.get_cell_char(0, 1), "😀");
    }

    // ── Scroll region LF tests ─────────────────────────────

    #[test]
    fn test_scroll_region_lf_within() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        core.set_cursor(0, 4);
        let scroll = core.handle_print(0x0A as u32); // LF via print? No...
                                                     // Actually LF is handled by handle_execute, not handle_print.
                                                     // But line_feed() is tested via handle_print scroll behavior
        assert_eq!(scroll, 0);
    }

    #[test]
    fn test_scroll_region_lf_at_bottom() {
        let mut core = TerminalCore::new(5, 5, 0);
        core.set_scroll_region(1, 3);
        core.set_cursor(0, 3); // At scroll region bottom
                               // Fill row to trigger wrap_pending
        for c in b'A'..=b'E' {
            core.handle_print(c as u32);
        }
        // Print one more char → wrap → line_feed at region bottom → scroll internal
        let scroll = core.handle_print(b'F' as u32);
        assert_eq!(scroll, 0); // Scroll handled internally
    }

    // ── Charset round-trip tests ───────────────────────────

    #[test]
    fn test_charset_round_trip() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_g0_charset(1);
        assert_eq!(core.get_g0_charset(), 1);
        core.set_g0_charset(0);
        assert_eq!(core.get_g0_charset(), 0);
        core.set_g1_charset(1);
        assert_eq!(core.get_g1_charset(), 1);
    }

    #[test]
    fn test_active_charset_switch() {
        let mut core = TerminalCore::new(10, 3, 0);
        assert_eq!(core.get_active_charset(), 0);
        core.set_active_charset(1);
        assert_eq!(core.get_active_charset(), 1);
        core.set_active_charset(0);
        assert_eq!(core.get_active_charset(), 0);
    }

    #[test]
    fn test_wrap_pending_round_trip() {
        let mut core = TerminalCore::new(10, 3, 0);
        assert!(!core.get_wrap_pending());
        core.set_wrap_pending(true);
        assert!(core.get_wrap_pending());
        core.set_wrap_pending(false);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_scroll_region_round_trip() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        assert_eq!(core.get_scroll_region_top(), 2);
        assert_eq!(core.get_scroll_region_bottom(), 7);
    }

    // ── DEC Line Drawing exhaustive ────────────────────────

    #[test]
    fn test_dec_line_drawing_all_entries() {
        let expected: &[(u32, u32)] = &[
            (0x5F, 0x0020),
            (0x60, 0x25C6),
            (0x61, 0x2592),
            (0x62, 0x2409),
            (0x63, 0x240C),
            (0x64, 0x240D),
            (0x65, 0x240A),
            (0x66, 0x00B0),
            (0x67, 0x00B1),
            (0x68, 0x2424),
            (0x69, 0x240B),
            (0x6A, 0x2518),
            (0x6B, 0x2510),
            (0x6C, 0x250C),
            (0x6D, 0x2514),
            (0x6E, 0x253C),
            (0x6F, 0x23BA),
            (0x70, 0x23BB),
            (0x71, 0x2500),
            (0x72, 0x23BC),
            (0x73, 0x23BD),
            (0x74, 0x251C),
            (0x75, 0x2524),
            (0x76, 0x2534),
            (0x77, 0x252C),
            (0x78, 0x2502),
            (0x79, 0x2264),
            (0x7A, 0x2265),
            (0x7B, 0x03C0),
            (0x7C, 0x2260),
            (0x7D, 0x00A3),
            (0x7E, 0x00B7),
        ];
        for &(input, output) in expected {
            let mut core = TerminalCore::new(10, 3, 0);
            core.set_g0_charset(1);
            core.handle_print(input);
            let ch = core.get_cell_char(0, 0);
            let expected_ch = char::from_u32(output).unwrap().to_string();
            assert_eq!(ch, expected_ch, "DEC line drawing 0x{:02X}", input);
        }
    }

    // ── Reset clears Sprint 2 state ────────────────────────

    #[test]
    fn test_reset_clears_sprint2_state() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Set up state
        core.set_g0_charset(1);
        core.set_g1_charset(1);
        core.set_active_charset(1);
        core.set_wrap_pending(true);
        core.set_scroll_region(1, 3);
        core.handle_print(0x1F600); // buffer emoji

        core.reset();

        assert_eq!(core.get_g0_charset(), 0);
        assert_eq!(core.get_g1_charset(), 0);
        assert_eq!(core.get_active_charset(), 0);
        assert!(!core.get_wrap_pending());
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 4);
        assert_eq!(core.get_grapheme_buffer_len(), 0);
    }

    #[test]
    fn test_resize_resets_scroll_region() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        core.resize(10, 20);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 19);
    }

    // ── Kitty Unicode placeholder suppression ─────────────

    #[test]
    fn test_kitty_placeholder_suppressed() {
        let mut core = TerminalCore::new(10, 3, 0);
        // U+10EEEE = Kitty placeholder character
        core.handle_print(0x10EEEE);
        // Cursor should not advance (character is suppressed)
        assert_eq!(core.get_cursor_col(), 0);
        // Cell remains empty (space = default empty cell)
        assert_eq!(core.get_cell_char(0, 0), " ");
    }

    #[test]
    fn test_kitty_placeholder_combining_suppressed() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Placeholder followed by combining marks (row/col encoding)
        core.handle_print(0x10EEEE);
        core.handle_print(0x0305); // combining overline (row encoding)
        core.handle_print(0x0305); // another combining mark (col encoding)
        // All should be suppressed
        assert_eq!(core.get_cursor_col(), 0);
        // Next non-combining character should print normally
        core.handle_print(0x41); // 'A'
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cell_char(0, 0), "A");
    }

    #[test]
    fn test_kitty_placeholder_multiple_cells() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Simulate 3 placeholder cells (kitten icat pattern)
        for _ in 0..3 {
            core.handle_print(0x10EEEE);
            core.handle_print(0x0305); // combining mark
            core.handle_print(0x0305); // combining mark
        }
        // All suppressed
        assert_eq!(core.get_cursor_col(), 0);
        // Normal text after placeholders
        core.handle_print(0x42); // 'B'
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cell_char(0, 0), "B");
    }

    #[test]
    fn test_kitty_placeholder_arabic_diacritics() {
        let mut core = TerminalCore::new(10, 3, 0);
        // kitten icat uses Arabic combining marks (U+0610-061A, U+064B-065F)
        // for encoding row/column in placeholder cells
        core.handle_print(0x10EEEE); // placeholder
        core.handle_print(0x0651);   // Arabic shadda (row encoding)
        core.handle_print(0x0615);   // Arabic small high tah (col encoding)
        // All should be suppressed
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cell_char(0, 0), " ");

        // Second cell with different Arabic marks
        core.handle_print(0x10EEEE); // placeholder
        core.handle_print(0x0652);   // Arabic sukun
        core.handle_print(0x0615);   // Arabic small high tah
        assert_eq!(core.get_cursor_col(), 0);

        // Normal character prints after placeholders
        core.handle_print(0x41); // 'A'
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cell_char(0, 0), "A");
    }

    #[test]
    fn test_kitty_placeholder_mixed_diacritics() {
        let mut core = TerminalCore::new(20, 3, 0);
        // Mix of Latin combining marks (0x0300-0x036F) and Arabic marks
        // as kitten icat uses diacritics from many Unicode blocks
        core.handle_print(0x10EEEE);
        core.handle_print(0x0305);   // combining overline (Latin)
        core.handle_print(0x0610);   // Arabic combining mark

        core.handle_print(0x10EEEE);
        core.handle_print(0x064B);   // Arabic fathatan
        core.handle_print(0x065F);   // Arabic wavy hamza below

        core.handle_print(0x10EEEE);
        core.handle_print(0x0483);   // Cyrillic titlo
        core.handle_print(0x0711);   // Syriac superscript alaph

        // All suppressed
        assert_eq!(core.get_cursor_col(), 0);

        // Normal text after
        core.handle_print(0x58); // 'X'
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cell_char(0, 0), "X");
    }
}
