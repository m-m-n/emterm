/// CSI edit handlers: IL (Insert Lines), DL (Delete Lines),
/// ICH (Insert Characters), DCH (Delete Characters).
use wasm_bindgen::prelude::*;

use crate::cell::*;
use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    /// CSI L - Insert Lines at cursor row within scroll region.
    pub fn handle_insert_lines(&mut self, count: u16) {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        if self.cursor.row < top || self.cursor.row > bottom {
            return; // No-op if cursor outside scroll region
        }
        let count = count.min(bottom - self.cursor.row + 1);
        // Shift rows down from cursor.row within region
        self.shift_rows_down(self.cursor.row, bottom, count);
    }

    /// CSI M - Delete Lines at cursor row within scroll region.
    pub fn handle_delete_lines(&mut self, count: u16) {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        if self.cursor.row < top || self.cursor.row > bottom {
            return; // No-op if cursor outside scroll region
        }
        let count = count.min(bottom - self.cursor.row + 1);
        // Shift rows up from cursor.row within region
        self.shift_rows_up(self.cursor.row, bottom, count);
    }

    /// CSI @ - Insert Characters at cursor position.
    pub fn handle_insert_characters(&mut self, count: u16) {
        let col = self.cursor.col;
        let row = self.cursor.row;
        if col >= self.cols {
            return;
        }
        let remaining = self.cols - col;
        let count = count.min(remaining);
        if count == 0 {
            return;
        }
        let base = self.viewport_row_base(row);

        // Shift cells right (iterate in reverse)
        for c in (col + count..self.cols).rev() {
            self.ring_cells[base + c as usize] = self.ring_cells[base + (c - count) as usize];
        }
        // Clear inserted cells with BCE
        let bce = self.bce_cell();
        for c in col..col + count {
            self.ring_cells[base + c as usize] = bce;
        }
        // Handle overflow entries for this row
        let abs = self.viewport_abs(row) as u32;
        overflow_clear_range(&mut self.overflow, abs, col as u32, self.cols as u32);
        overflow_ridx_clear_range(&mut self.overflow_ridx, abs, col as u32, self.cols as u32);
        self.mark_row_dirty(row);
    }

    /// CSI P - Delete Characters at cursor position.
    pub fn handle_delete_characters(&mut self, count: u16) {
        let col = self.cursor.col;
        let row = self.cursor.row;
        if col >= self.cols {
            return;
        }
        let remaining = self.cols - col;
        let count = count.min(remaining);
        if count == 0 {
            return;
        }
        let base = self.viewport_row_base(row);

        // Shift cells left
        for c in col..self.cols - count {
            self.ring_cells[base + c as usize] = self.ring_cells[base + (c + count) as usize];
        }
        // Clear trailing cells with BCE
        let bce = self.bce_cell();
        for c in self.cols - count..self.cols {
            self.ring_cells[base + c as usize] = bce;
        }
        // Handle overflow entries for this row
        let abs = self.viewport_abs(row) as u32;
        overflow_clear_range(&mut self.overflow, abs, col as u32, self.cols as u32);
        overflow_ridx_clear_range(&mut self.overflow_ridx, abs, col as u32, self.cols as u32);
        self.mark_row_dirty(row);
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: Edit Tests ────────────────────────────────

    #[test]
    fn test_insert_lines_basic() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Fill rows with identifying chars
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'0' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_scroll_region(0, 4);
        core.set_cursor(0, 1);
        core.handle_insert_lines(2);
        // Row 0 should still be '0'
        assert_eq!(core.get_cell_char(0, 0), "0");
        // Rows 1-2 should be blank (inserted)
        assert_eq!(core.get_cell_char(0, 1), " ");
        assert_eq!(core.get_cell_char(0, 2), " ");
        // Row 3 should be old row 1 ('1')
        assert_eq!(core.get_cell_char(0, 3), "1");
        // Row 4 should be old row 2 ('2')
        assert_eq!(core.get_cell_char(0, 4), "2");
    }

    #[test]
    fn test_insert_lines_outside_region() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_scroll_region(1, 3);
        core.set_cursor(0, 0); // Outside scroll region
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'X', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.handle_insert_lines(1);
        // Should be no-op
        assert_eq!(core.get_cell_char(0, 0), "X");
    }

    #[test]
    fn test_insert_lines_count_clamped() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_scroll_region(0, 4);
        core.set_cursor(0, 3);
        core.handle_insert_lines(100); // Much more than available
        // Should not panic
        assert_eq!(core.get_cell_char(0, 3), " ");
    }

    #[test]
    fn test_delete_lines_basic() {
        let mut core = TerminalCore::new(10, 5, 0);
        for r in 0..5 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'0' + r as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_scroll_region(0, 4);
        core.set_cursor(0, 1);
        core.handle_delete_lines(2);
        // Row 0 should still be '0'
        assert_eq!(core.get_cell_char(0, 0), "0");
        // Row 1 should now be old row 3 ('3')
        assert_eq!(core.get_cell_char(0, 1), "3");
        // Row 2 should be old row 4 ('4')
        assert_eq!(core.get_cell_char(0, 2), "4");
        // Rows 3-4 should be blank (shifted in)
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
    }

    #[test]
    fn test_delete_lines_outside_region() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_scroll_region(1, 3);
        core.set_cursor(0, 4); // Below scroll region
        core.handle_delete_lines(1);
        // Should be no-op (cursor outside region)
    }

    #[test]
    fn test_insert_characters_basic() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Fill row 0: "ABCDEFGHIJ"
        for (c, ch) in (b'A'..=b'J').enumerate() {
            core.set_cell_ascii(c as u16, 0, ch, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 0);
        core.clear_dirty();
        core.handle_insert_characters(2);
        // Expected: "ABC  DEFGH"
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        assert_eq!(core.get_cell_char(2, 0), "C");
        assert_eq!(core.get_cell_char(3, 0), " "); // inserted
        assert_eq!(core.get_cell_char(4, 0), " "); // inserted
        assert_eq!(core.get_cell_char(5, 0), "D");
        assert_eq!(core.get_cell_char(6, 0), "E");
        assert_eq!(core.get_cell_char(7, 0), "F");
        assert_eq!(core.get_cell_char(8, 0), "G");
        assert_eq!(core.get_cell_char(9, 0), "H");
        // I, J fell off the right edge
        assert!(core.is_row_dirty(0));
    }

    #[test]
    fn test_insert_characters_clamped() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor(8, 0);
        core.handle_insert_characters(100);
        // Should not panic, effectively clears last 2 cells
        assert_eq!(core.get_cell_char(8, 0), " ");
        assert_eq!(core.get_cell_char(9, 0), " ");
    }

    #[test]
    fn test_delete_characters_basic() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Fill row 0: "ABCDEFGHIJ"
        for (c, ch) in (b'A'..=b'J').enumerate() {
            core.set_cell_ascii(c as u16, 0, ch, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 0);
        core.clear_dirty();
        core.handle_delete_characters(2);
        // Expected: "ABCFGHIJ  "
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        assert_eq!(core.get_cell_char(2, 0), "C");
        assert_eq!(core.get_cell_char(3, 0), "F"); // was at 5
        assert_eq!(core.get_cell_char(4, 0), "G");
        assert_eq!(core.get_cell_char(5, 0), "H");
        assert_eq!(core.get_cell_char(6, 0), "I");
        assert_eq!(core.get_cell_char(7, 0), "J");
        assert_eq!(core.get_cell_char(8, 0), " "); // cleared
        assert_eq!(core.get_cell_char(9, 0), " "); // cleared
        assert!(core.is_row_dirty(0));
    }

    #[test]
    fn test_delete_characters_clamped() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor(8, 0);
        core.handle_delete_characters(100);
        // Should not panic
        assert_eq!(core.get_cell_char(8, 0), " ");
    }

    #[test]
    fn test_edit_dirty_marking() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_scroll_region(0, 4);
        core.clear_dirty();

        // Insert lines
        core.set_cursor(0, 1);
        core.handle_insert_lines(1);
        // Rows in region should be dirty
        assert!(core.is_row_dirty(1));

        core.clear_dirty();

        // Delete lines
        core.set_cursor(0, 1);
        core.handle_delete_lines(1);
        assert!(core.is_row_dirty(1));

        core.clear_dirty();

        // Insert characters
        core.set_cursor(0, 0);
        core.handle_insert_characters(1);
        assert!(core.is_row_dirty(0));

        core.clear_dirty();

        // Delete characters
        core.set_cursor(0, 0);
        core.handle_delete_characters(1);
        assert!(core.is_row_dirty(0));
    }

    // ── BCE tests for ICH/DCH ───────────────────────────────

    use crate::cell::PackedColor;

    #[test]
    fn test_bce_insert_characters() {
        let mut core = TerminalCore::new(10, 3, 0);
        for (c, ch) in (b'A'..=b'J').enumerate() {
            core.set_cell_ascii(c as u16, 0, ch, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 0);
        core.set_cursor_bg(1, 2, 0, 0); // indexed green
        core.handle_insert_characters(2);
        // Inserted cells at col 3, 4 should have green bg
        for col in 3..5 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
        // Non-inserted cells should retain original bg (DEFAULT)
        let bg0 = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg0, PackedColor::DEFAULT);
    }

    #[test]
    fn test_bce_delete_characters() {
        let mut core = TerminalCore::new(10, 3, 0);
        for (c, ch) in (b'A'..=b'J').enumerate() {
            core.set_cell_ascii(c as u16, 0, ch, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 0);
        core.set_cursor_bg(1, 2, 0, 0); // indexed green
        core.handle_delete_characters(2);
        // Trailing cells at col 8, 9 should have green bg
        for col in 8..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
        // Shifted cells should retain original bg
        let bg3 = PackedColor::from_u32(core.get_cell_bg(3, 0));
        assert_eq!(bg3, PackedColor::DEFAULT);
    }
}
