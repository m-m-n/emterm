/// CSI screen handlers: ED (Erase in Display), EL (Erase in Line), ECH (Erase Characters).
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

pub(crate) const SCROLLBACK_SENTINEL: u8 = 0xFF;

#[wasm_bindgen]
impl TerminalCore {
    /// CSI J - Erase in Display.
    /// mode: 0=Below, 1=Above, 2=All, 3=Scrollback.
    /// Returns 0 on success, SCROLLBACK_SENTINEL (0xFF) for Scrollback.
    pub fn handle_erase_in_display(&mut self, mode: u8) -> u8 {
        match mode {
            0 => {
                // Below: clear from cursor to end of screen
                self.clear_line_range(self.cursor.row, self.cursor.col, self.cols);
                for r in (self.cursor.row + 1)..self.rows {
                    self.clear_line(r);
                }
                0
            }
            1 => {
                // Above: clear from start to cursor (inclusive)
                for r in 0..self.cursor.row {
                    self.clear_line(r);
                }
                self.clear_line_range(self.cursor.row, 0, self.cursor.col + 1);
                0
            }
            2 => {
                // All: clear entire screen
                for r in 0..self.rows {
                    self.clear_line(r);
                }
                0
            }
            3 => {
                // Scrollback: return sentinel for TS handling
                SCROLLBACK_SENTINEL
            }
            _ => 0, // Invalid mode: no-op
        }
    }

    /// CSI K - Erase in Line.
    /// mode: 0=ToEnd, 1=ToStart, 2=All.
    pub fn handle_erase_in_line(&mut self, mode: u8) {
        match mode {
            0 => {
                // ToEnd: clear from cursor to end of line
                self.clear_line_range(self.cursor.row, self.cursor.col, self.cols);
            }
            1 => {
                // ToStart: clear from start to cursor (inclusive)
                self.clear_line_range(self.cursor.row, 0, self.cursor.col + 1);
            }
            2 => {
                // All: clear entire line
                self.clear_line(self.cursor.row);
            }
            _ => {} // Invalid mode: no-op
        }
    }

    /// CSI X - Erase Characters.
    /// count: number of characters to erase (default 1).
    pub fn handle_erase_characters(&mut self, count: u16) {
        let end = self.cursor.col.saturating_add(count).min(self.cols);
        self.clear_line_range(self.cursor.row, self.cursor.col, end);
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 3: CSI Screen handler tests ──────────────────

    #[test]
    fn test_handle_erase_in_display_below() {
        let mut core = TerminalCore::new(10, 5);
        // Fill entire screen
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        // Set cursor to (3, 2) and erase below
        core.set_cursor(3, 2);
        core.clear_dirty();
        let result = core.handle_erase_in_display(0);
        assert_eq!(result, 0);
        // Cols 0-2 on row 2 should still be 'A'
        assert_eq!(core.get_cell_char(0, 2), "A");
        assert_eq!(core.get_cell_char(2, 2), "A");
        // Cols 3-9 on row 2 should be empty
        assert_eq!(core.get_cell_char(3, 2), " ");
        assert_eq!(core.get_cell_char(9, 2), " ");
        // Rows 3-4 should be cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
        // Row 0-1 should still be 'A'
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(0, 1), "A");
    }

    #[test]
    fn test_handle_erase_in_display_above() {
        let mut core = TerminalCore::new(10, 5);
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'B', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_cursor(3, 2);
        let result = core.handle_erase_in_display(1);
        assert_eq!(result, 0);
        // Rows 0-1 should be cleared
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(0, 1), " ");
        // Cols 0-3 on row 2 should be cleared (inclusive)
        assert_eq!(core.get_cell_char(0, 2), " ");
        assert_eq!(core.get_cell_char(3, 2), " ");
        // Cols 4-9 on row 2 should still be 'B'
        assert_eq!(core.get_cell_char(4, 2), "B");
        // Rows 3-4 untouched
        assert_eq!(core.get_cell_char(0, 3), "B");
    }

    #[test]
    fn test_handle_erase_in_display_all() {
        let mut core = TerminalCore::new(10, 5);
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'C', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_cursor(3, 2);
        let result = core.handle_erase_in_display(2);
        assert_eq!(result, 0);
        // All cells should be cleared
        for r in 0..5 {
            for c in 0..10 {
                assert_eq!(core.get_cell_char(c, r), " ");
            }
        }
    }

    #[test]
    fn test_handle_erase_in_display_scrollback_returns_sentinel() {
        let mut core = TerminalCore::new(10, 5);
        let result = core.handle_erase_in_display(3);
        assert_eq!(result, 0xFF); // SCROLLBACK_SENTINEL
    }

    #[test]
    fn test_handle_erase_in_display_invalid_mode() {
        let mut core = TerminalCore::new(10, 5);
        let result = core.handle_erase_in_display(99);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_handle_erase_in_line_to_end() {
        let mut core = TerminalCore::new(10, 3);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'D', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(5, 0);
        core.clear_dirty();
        core.handle_erase_in_line(0);
        // Cols 0-4 should still be 'D'
        for c in 0..5 {
            assert_eq!(core.get_cell_char(c, 0), "D");
        }
        // Cols 5-9 should be cleared
        for c in 5..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
        assert!(core.is_row_dirty(0));
    }

    #[test]
    fn test_handle_erase_in_line_to_start() {
        let mut core = TerminalCore::new(10, 3);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'E', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(5, 0);
        core.handle_erase_in_line(1);
        // Cols 0-5 should be cleared (inclusive)
        for c in 0..=5 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
        // Cols 6-9 should still be 'E'
        for c in 6..10 {
            assert_eq!(core.get_cell_char(c, 0), "E");
        }
    }

    #[test]
    fn test_handle_erase_in_line_all() {
        let mut core = TerminalCore::new(10, 3);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'F', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(5, 0);
        core.handle_erase_in_line(2);
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
    }

    #[test]
    fn test_handle_erase_characters_normal() {
        let mut core = TerminalCore::new(10, 3);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'G', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 0);
        core.clear_dirty();
        core.handle_erase_characters(4);
        // Cols 0-2 should still be 'G'
        for c in 0..3 {
            assert_eq!(core.get_cell_char(c, 0), "G");
        }
        // Cols 3-6 should be cleared
        for c in 3..7 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
        // Cols 7-9 should still be 'G'
        for c in 7..10 {
            assert_eq!(core.get_cell_char(c, 0), "G");
        }
        assert!(core.is_row_dirty(0));
    }

    #[test]
    fn test_handle_erase_characters_overflow_clamped() {
        let mut core = TerminalCore::new(10, 3);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'H', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(7, 0);
        core.handle_erase_characters(100); // More than remaining
        // Cols 0-6 still 'H'
        for c in 0..7 {
            assert_eq!(core.get_cell_char(c, 0), "H");
        }
        // Cols 7-9 cleared
        for c in 7..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
    }

    #[test]
    fn test_handle_erase_characters_dirty() {
        let mut core = TerminalCore::new(10, 3);
        core.clear_dirty();
        core.set_cursor(0, 0);
        core.handle_erase_characters(5);
        assert!(core.is_row_dirty(0));
    }
}
