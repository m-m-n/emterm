/// C0 control handler: handle_execute, tab stop search, line feed execution.
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

pub(crate) const BEL_SENTINEL: u8 = 0xFE;

#[wasm_bindgen]
impl TerminalCore {
    /// Handle a C0 Execute action.
    /// Returns: scroll count (0-N for LF/VT/FF), BEL_SENTINEL (0xFE) for BEL.
    pub fn handle_execute(&mut self, byte: u8) -> u8 {
        match byte {
            0x07 => BEL_SENTINEL, // BEL
            0x08 => {
                // BS: decrement cursor.col, clamped to 0
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.wrap_pending = false;
                0
            }
            0x09 => {
                // HT: move to next tab stop
                self.cursor.col = self.find_next_tab_stop();
                self.wrap_pending = false;
                0
            }
            0x0A | 0x0B | 0x0C => {
                // LF/VT/FF
                self.execute_line_feed()
            }
            0x0D => {
                // CR
                self.cursor.col = 0;
                self.wrap_pending = false;
                0
            }
            0x0E => {
                // SO: switch to G1
                self.active_charset = 1;
                0
            }
            0x0F => {
                // SI: switch to G0
                self.active_charset = 0;
                0
            }
            _ => 0, // Unknown C0: no-op
        }
    }
}

impl TerminalCore {
    /// Internal C0 handler for process_pty_data.
    /// Fires bell callback instead of returning sentinel.
    pub(crate) fn handle_execute_internal(&mut self, byte: u8) {
        if byte == 0x07 {
            self.fire_bell_callback();
            return;
        }
        self.handle_execute(byte);
    }

    /// Find the next tab stop after the current cursor column.
    /// Returns the tab stop column, or cols-1 if no more stops.
    fn find_next_tab_stop(&self) -> u16 {
        self.next_tab_stop(self.cursor.col)
    }

    /// Execute LF: line_feed (scroll handled internally) + clear wrap_pending.
    fn execute_line_feed(&mut self) -> u8 {
        self.line_feed();
        self.wrap_pending = false;
        0
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 3: C0 handler tests ─────────────────────────

    #[test]
    fn test_handle_execute_bel_returns_sentinel() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_execute(0x07), 0xFE);
    }

    #[test]
    fn test_handle_execute_bs_at_col5() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 0);
        let result = core.handle_execute(0x08);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 4);
    }

    #[test]
    fn test_handle_execute_bs_at_col0_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 0);
        let result = core.handle_execute(0x08);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_execute_bs_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_execute(0x08);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_ht_default_tab_stops() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 8);
    }

    #[test]
    fn test_handle_execute_ht_col7_to_col8() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(7, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 8);
    }

    #[test]
    fn test_handle_execute_ht_col8_to_col16() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(8, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 16);
    }

    #[test]
    fn test_handle_execute_ht_past_last_stop() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(78, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_execute_ht_custom_tab_stops() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.clear_all_tab_stops();
        core.set_tab_stop(5);
        core.set_tab_stop(15);
        core.set_cursor(0, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 5);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 15);
        // No more stops → cols-1
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_execute_ht_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_execute(0x09);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_lf_mid_screen() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 6);
    }

    #[test]
    fn test_handle_execute_lf_at_scroll_region_bottom() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(0, 15); // At scroll region bottom
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 0); // Scroll handled internally
        assert_eq!(core.get_cursor_row(), 15); // Stay at bottom
    }

    #[test]
    fn test_handle_execute_lf_at_bottom_no_scroll_region() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 23); // Last row
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 0); // Scroll handled internally
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_execute_vt_same_as_lf() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        let result = core.handle_execute(0x0B); // VT
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 6);
    }

    #[test]
    fn test_handle_execute_ff_same_as_lf() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        let result = core.handle_execute(0x0C); // FF
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 6);
    }

    #[test]
    fn test_handle_execute_cr() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(30, 5);
        let result = core.handle_execute(0x0D);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 5); // Row unchanged
    }

    #[test]
    fn test_handle_execute_cr_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_execute(0x0D);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_so() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_execute(0x0E); // SO
        assert_eq!(core.get_active_charset(), 1);
    }

    #[test]
    fn test_handle_execute_si() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_active_charset(1);
        core.handle_execute(0x0F); // SI
        assert_eq!(core.get_active_charset(), 0);
    }

    #[test]
    fn test_handle_execute_lf_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        core.set_wrap_pending(true);
        core.handle_execute(0x0A);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_unknown_byte_noop() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 5);
        let result = core.handle_execute(0x01); // SOH
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 5);
        assert_eq!(core.get_cursor_row(), 5);
    }

}
