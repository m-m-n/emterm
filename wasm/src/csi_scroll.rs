/// CSI scroll handlers: SU (Scroll Up), SD (Scroll Down), DECSTBM.
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    /// CSI S - Scroll Up. Always WASM-internal (returns 0).
    /// Full screen: pushes top lines to scrollback via ring buffer.
    /// Scroll region: shifts rows within region only.
    pub fn handle_scroll_up(&mut self, count: u16) -> u8 {
        self.scroll_up_internal(count);
        0
    }

    /// CSI T - Scroll Down. Always WASM-internal.
    pub fn handle_scroll_down(&mut self, count: u16) {
        self.scroll_down_internal(count);
    }

    /// CSI r - DECSTBM (Set Scrolling Region).
    /// top/bottom are 1-indexed (0 = default).
    pub fn handle_decstbm(&mut self, top: u16, bottom: u16) {
        let t = if top == 0 {
            0
        } else {
            (top - 1).min(self.rows.saturating_sub(1))
        };
        let b = if bottom == 0 {
            self.rows.saturating_sub(1)
        } else {
            (bottom - 1).min(self.rows.saturating_sub(1))
        };
        self.set_scroll_region(t, b);
        self.cursor.col = 0;
        self.cursor.row = 0;
        self.wrap_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: Scroll Tests ──────────────────────────────

    #[test]
    fn test_scroll_up_scroll_region() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        // Fill rows in region
        for r in 2..=7 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'0' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        let result = core.handle_scroll_up(1);
        assert_eq!(result, 0); // WASM handled internally
                               // Row 2 should now have old row 3 content
        assert_eq!(core.get_cell_char(0, 2), "3");
        // Last row in region should be blank
        assert_eq!(core.get_cell_char(0, 7), " ");
    }

    #[test]
    fn test_scroll_up_full_screen() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Fill rows with content
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'A' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        let result = core.handle_scroll_up(3);
        assert_eq!(result, 0); // Always 0 (WASM-internal)
                               // No scrollback since scrollback_lines=0 (at capacity)
                               // Viewport rows shifted up by 3
        assert_eq!(core.get_cell_char(0, 0), "D"); // old row 3
        assert_eq!(core.get_cell_char(0, 1), "E"); // old row 4
        assert_eq!(core.get_cell_char(0, 2), " "); // cleared
    }

    #[test]
    fn test_scroll_up_clamped() {
        let mut core = TerminalCore::new(10, 5, 0);
        let result = core.handle_scroll_up(1000);
        assert_eq!(result, 0); // Always 0 (WASM-internal)
    }

    #[test]
    fn test_scroll_down_basic() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        for r in 2..=7 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'0' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.handle_scroll_down(1);
        // Row 2 should be blank (new line scrolled in)
        assert_eq!(core.get_cell_char(0, 2), " ");
        // Row 3 should have old row 2 content
        assert_eq!(core.get_cell_char(0, 3), "2");
    }

    #[test]
    fn test_scroll_down_full_screen() {
        let mut core = TerminalCore::new(10, 5, 0);
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'A' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.handle_scroll_down(2);
        // Rows 0-1 should be blank
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(0, 1), " ");
        // Row 2 should have old row 0
        assert_eq!(core.get_cell_char(0, 2), "A");
    }

    #[test]
    fn test_decstbm_basic() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 10);
        core.handle_decstbm(5, 20);
        // Scroll region set (1-indexed → 0-indexed)
        assert_eq!(core.get_scroll_region_top(), 4);
        assert_eq!(core.get_scroll_region_bottom(), 19);
        // Cursor homed
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_decstbm_defaults() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.handle_decstbm(0, 0); // Both default → full screen
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 23);
    }

    #[test]
    fn test_decstbm_invalid() {
        let mut core = TerminalCore::new(80, 24, 0);
        // top > bottom (via 1-indexed)
        core.handle_decstbm(20, 5);
        // set_scroll_region should reject invalid (top >= bottom)
        // Cursor still homed
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
    }
}
