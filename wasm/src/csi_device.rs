/// CSI device response handlers: DSR, DA1, DA2, response buffer.
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    /// CSI Ps n - Device Status Report.
    /// Returns response length (0 if no response).
    pub fn handle_device_status_report(&mut self, ps: u8) -> u8 {
        match ps {
            5 => {
                // OK status
                self.write_response(b"\x1b[0n")
            }
            6 => {
                // Cursor position report (1-indexed)
                self.format_cpr()
            }
            _ => 0, // Unknown: no response
        }
    }

    /// CSI c - Primary Device Attributes.
    /// Returns response length.
    pub fn handle_primary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[?65;1;4;22c")
    }

    /// CSI > c - Secondary Device Attributes.
    /// Returns response length.
    pub fn handle_secondary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[>65;1;0c")
    }

    /// Get pointer to response buffer in linear memory.
    pub fn get_response_ptr(&self) -> *const u8 {
        self.response_buffer.as_ptr()
    }

    /// Get length of last device response.
    pub fn get_response_len(&self) -> u32 {
        self.response_len as u32
    }

    /// Get response buffer contents as a byte vector.
    /// Convenient alternative to ptr/len for TS integration.
    pub fn get_response_bytes(&self) -> Vec<u8> {
        self.response_buffer[..self.response_len as usize].to_vec()
    }
}

impl TerminalCore {
    /// CSI ? Ps $ p - DECRPM (DEC Private Mode Report).
    /// Reports whether a DEC private mode is set, reset, or not recognized.
    /// Response: CSI ? Ps ; Pm $ y
    ///   Pm=0: not recognized, Pm=1: set, Pm=2: reset
    pub fn handle_decrpm(&mut self, mode: u16) -> u8 {
        let pm: u8 = match mode {
            // Known modes tracked in WASM bitfield
            3 => {
                if self.get_mode(crate::terminal_core::MODE_COLUMN_132) {
                    1
                } else {
                    2
                }
            }
            5 => {
                if self.get_mode(crate::terminal_core::MODE_REVERSE_SCREEN) {
                    1
                } else {
                    2
                }
            }
            6 => {
                if self.get_mode(crate::terminal_core::MODE_ORIGIN) {
                    1
                } else {
                    2
                }
            }
            7 => {
                if self.get_mode(crate::terminal_core::MODE_AUTO_WRAP) {
                    1
                } else {
                    2
                }
            }
            12 => {
                if self.get_mode(crate::terminal_core::MODE_CURSOR_BLINK) {
                    1
                } else {
                    2
                }
            }
            25 => {
                if self.get_mode(crate::terminal_core::MODE_CURSOR_VISIBLE) {
                    1
                } else {
                    2
                }
            }
            1004 => {
                if self.get_mode(crate::terminal_core::MODE_FOCUS_TRACKING) {
                    1
                } else {
                    2
                }
            }
            2004 => {
                if self.get_mode(crate::terminal_core::MODE_BRACKETED_PASTE) {
                    1
                } else {
                    2
                }
            }
            2026 => {
                if self.get_mode(crate::terminal_core::MODE_SYNCHRONIZED_OUTPUT) {
                    1
                } else {
                    2
                }
            }
            // Known modes tracked in TS (report as recognized but defer to TS)
            1 | 47 | 1000 | 1002 | 1003 | 1005 | 1006 | 1047 | 1048 | 1049 => 2,
            // Unknown modes
            _ => 0,
        };
        // Format: ESC [ ? <mode> ; <pm> $ y
        let mut buf = [0u8; 20];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        buf[2] = b'?';
        let mut pos = 3;
        pos = Self::write_u16_decimal(&mut buf, pos, mode);
        buf[pos] = b';';
        pos += 1;
        buf[pos] = pm + b'0';
        pos += 1;
        buf[pos] = b'$';
        pos += 1;
        buf[pos] = b'y';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// Write bytes to response buffer. Returns length.
    fn write_response(&mut self, data: &[u8]) -> u8 {
        let len = data.len().min(self.response_buffer.len());
        self.response_buffer[..len].copy_from_slice(&data[..len]);
        self.response_len = len as u8;
        len as u8
    }

    /// Format cursor position report into response buffer.
    fn format_cpr(&mut self) -> u8 {
        let row = self.cursor.row.saturating_add(1);
        let col = self.cursor.col.saturating_add(1);
        // Format: ESC [ row ; col R
        let mut buf = [0u8; 20];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        let mut pos = 2;
        pos = Self::write_u16_decimal(&mut buf, pos, row);
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u16_decimal(&mut buf, pos, col);
        buf[pos] = b'R';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// CSI 14 t - Report text area size in pixels.
    /// Response: ESC [ 4 ; <height> ; <width> t
    pub fn handle_xtwinops_text_area_px(&mut self) -> u8 {
        let height = self.rows as u32 * self.cell_height_px as u32;
        let width = self.cols as u32 * self.cell_width_px as u32;
        self.format_xtwinops(4, height, width)
    }

    /// CSI 16 t - Report character cell size in pixels.
    /// Response: ESC [ 6 ; <height> ; <width> t
    pub fn handle_xtwinops_cell_size(&mut self) -> u8 {
        self.format_xtwinops(6, self.cell_height_px as u32, self.cell_width_px as u32)
    }

    /// CSI 18 t - Report text area size in characters.
    /// Response: ESC [ 8 ; <rows> ; <cols> t
    pub fn handle_xtwinops_text_area_chars(&mut self) -> u8 {
        self.format_xtwinops(8, self.rows as u32, self.cols as u32)
    }

    /// Format XTWINOPS response: ESC [ <ps> ; <p1> ; <p2> t
    fn format_xtwinops(&mut self, ps: u8, p1: u32, p2: u32) -> u8 {
        let mut buf = [0u8; 32];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        let mut pos = 2;
        buf[pos] = ps + b'0';
        pos += 1;
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u32_decimal(&mut buf, pos, p1);
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u32_decimal(&mut buf, pos, p2);
        buf[pos] = b't';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// Write a u16 as decimal digits to buffer, return new position.
    fn write_u16_decimal(buf: &mut [u8], start: usize, val: u16) -> usize {
        if val == 0 {
            buf[start] = b'0';
            return start + 1;
        }
        let mut digits = [0u8; 5];
        let mut n = val;
        let mut count = 0;
        while n > 0 {
            digits[count] = (n % 10) as u8 + b'0';
            n /= 10;
            count += 1;
        }
        let mut pos = start;
        for i in (0..count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
        pos
    }

    /// Write a u32 as decimal digits to buffer, return new position.
    fn write_u32_decimal(buf: &mut [u8], start: usize, val: u32) -> usize {
        if val == 0 {
            buf[start] = b'0';
            return start + 1;
        }
        let mut digits = [0u8; 10];
        let mut n = val;
        let mut count = 0;
        while n > 0 {
            digits[count] = (n % 10) as u8 + b'0';
            n /= 10;
            count += 1;
        }
        let mut pos = start;
        for i in (0..count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
        pos
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: Device Response Tests ─────────────────────

    #[test]
    fn test_dsr_ok_status() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_device_status_report(5);
        assert_eq!(len, 4);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[0n");
    }

    #[test]
    fn test_dsr_cursor_position_home() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 0);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[1;1R");
    }

    #[test]
    fn test_dsr_cursor_position_nonzero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(9, 4); // 0-indexed
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[5;10R"); // 1-indexed
    }

    #[test]
    fn test_dsr_unknown() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_device_status_report(99);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_da1() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_primary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?65;1;4;22c");
    }

    #[test]
    fn test_da2() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_secondary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[>65;1;0c");
    }

    #[test]
    fn test_response_ptr_len() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_primary_device_attributes();
        let ptr = core.get_response_ptr();
        let len = core.get_response_len();
        assert!(!ptr.is_null());
        assert!(len > 0);
    }

    #[test]
    fn test_dsr_large_position() {
        let mut core = TerminalCore::new(500, 500, 0);
        core.set_cursor(499, 499);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[500;500R");
    }

    // ── XTWINOPS Tests ──────────────────────────────────

    #[test]
    fn test_xtwinops_cell_size() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;20;10t");
    }

    #[test]
    fn test_xtwinops_text_area_px() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        let len = core.handle_xtwinops_text_area_px();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        // 24 rows * 20px = 480, 80 cols * 10px = 800
        assert_eq!(&bytes, b"\x1b[4;480;800t");
    }

    #[test]
    fn test_xtwinops_text_area_chars() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_xtwinops_text_area_chars();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[8;24;80t");
    }

    #[test]
    fn test_xtwinops_default_cell_size() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Default: 8x16
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;16;8t");
    }

    // ── DECRPM Tests ──────────────────────────────────────

    #[test]
    fn test_decrpm_mode_2026_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_decrpm(2026);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?2026;2$y"); // Pm=2 (reset)
    }

    #[test]
    fn test_decrpm_mode_2026_set() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(2026, true);
        let len = core.handle_decrpm(2026);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?2026;1$y"); // Pm=1 (set)
    }

    #[test]
    fn test_decrpm_known_mode_autowrap() {
        let mut core = TerminalCore::new(80, 24, 0);
        // autoWrap defaults to true
        let len = core.handle_decrpm(7);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?7;1$y"); // Pm=1 (set)
    }

    #[test]
    fn test_decrpm_unknown_mode() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_decrpm(9999);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?9999;0$y"); // Pm=0 (not recognized)
    }

    #[test]
    fn test_decrpm_ts_tracked_mode() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Mode 1 (DECCKM) is tracked in TS, reported as reset (2)
        let len = core.handle_decrpm(1);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?1;2$y"); // Pm=2 (reset)
    }

    #[test]
    fn test_cell_size_getters() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Default values
        assert_eq!(core.get_cell_width_px(), 8);
        assert_eq!(core.get_cell_height_px(), 16);

        // After setting
        core.set_cell_size_px(10, 20);
        assert_eq!(core.get_cell_width_px(), 10);
        assert_eq!(core.get_cell_height_px(), 20);
    }
}
