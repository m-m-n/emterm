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
        self.write_response(b"\x1b[?64;1;2;6;22c")
    }

    /// CSI > c - Secondary Device Attributes.
    /// Returns response length.
    pub fn handle_secondary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[>41;1;0c")
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
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: Device Response Tests ─────────────────────

    #[test]
    fn test_dsr_ok_status() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_device_status_report(5);
        assert_eq!(len, 4);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[0n");
    }

    #[test]
    fn test_dsr_cursor_position_home() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 0);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[1;1R");
    }

    #[test]
    fn test_dsr_cursor_position_nonzero() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(9, 4); // 0-indexed
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[5;10R"); // 1-indexed
    }

    #[test]
    fn test_dsr_unknown() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_device_status_report(99);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_da1() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_primary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?64;1;2;6;22c");
    }

    #[test]
    fn test_da2() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_secondary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[>41;1;0c");
    }

    #[test]
    fn test_response_ptr_len() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_primary_device_attributes();
        let ptr = core.get_response_ptr();
        let len = core.get_response_len();
        assert!(!ptr.is_null());
        assert!(len > 0);
    }

    #[test]
    fn test_dsr_large_position() {
        let mut core = TerminalCore::new(500, 500);
        core.set_cursor(499, 499);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[500;500R");
    }
}
