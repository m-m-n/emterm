/// CSI edit handlers: IL (Insert Lines), DL (Delete Lines),
/// ICH (Insert Characters), DCH (Delete Characters).
use crate::cell::*;
use crate::terminal_core::TerminalCore;

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
        let abs = self.viewport_abs(row);
        let cols_usize = self.cols as usize;
        let base = abs * cols_usize;
        if base + cols_usize > self.ring_cells.len() {
            return;
        }

        // Wide-pair partner cleanup (D2/D3): capture pre-shift state before
        // mutating. If the cursor sits on a spacer, the base at col-1 loses
        // its partner regardless of what the shift moves into `col`.
        let col_was_spacer = self.get_cell_width(col, row) == 0;

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

        // Wide-pair partner cleanup (D2/D3), continued: fix orphans created
        // by the shift itself.
        if col_was_spacer {
            if col > 0 {
                self.blank_wide_pair_split(col - 1, row);
            }
            // The spacer originally at `col` was relocated to `col + count`
            // by the shift, now orphaned (its base stayed behind at col-1,
            // separated by the freshly-inserted BCE run).
            if col + count < self.cols {
                self.blank_wide_pair_split(col + count, row);
            }
        }
        // A base cell shifted into the last column never has a spacer to
        // its right (there is no column beyond it) unless it legitimately
        // is the pre-existing auto-wrap-off quirk from the print path — but
        // ICH must not manufacture a new instance of that quirk, so any
        // base landing there via this operation is blanked.
        if self.get_cell_width(self.cols - 1, row) == 2 {
            self.blank_wide_pair_split(self.cols - 1, row);
        }
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
        let abs = self.viewport_abs(row);
        let cols_usize = self.cols as usize;
        let base = abs * cols_usize;
        if base + cols_usize > self.ring_cells.len() {
            return;
        }

        // Wide-pair partner cleanup (D2/D3): capture pre-shift state before
        // mutating.
        let col_was_spacer = self.get_cell_width(col, row) == 0;
        let boundary = self.cols - count;
        let boundary_shift_nonempty = boundary > col;

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

        // Wide-pair partner cleanup (D2/D3), continued: fix orphans created
        // by the shift itself.
        if col_was_spacer && col > 0 {
            // The spacer that used to sit at `col` (pairing with the base at
            // col-1) got overwritten by the shift; col-1's base is orphaned.
            self.blank_wide_pair_split(col - 1, row);
        }
        if self.get_cell_width(col, row) == 0 {
            // A spacer whose base sat at col+count-1 (inside the deleted
            // range) was relocated to `col` by the shift, now orphaned.
            self.blank_wide_pair_split(col, row);
        }
        if boundary_shift_nonempty && self.get_cell_width(boundary - 1, row) == 2 {
            // The last cell the shift touches (`boundary - 1`) received
            // whatever sat at the row's last column pre-shift. If that is a
            // base, it has lost its spacer (real or BCE-filled) at the
            // shift/BCE-fill boundary and is no longer at the true last
            // column, so the auto-wrap-off quirk exception does not apply.
            self.blank_wide_pair_split(boundary - 1, row);
        }
    }

    /// Blanks a wide-pair half (spacer width=0 or base width=2) left
    /// orphaned at an ICH/DCH edit boundary, restoring the width-1 space
    /// invariant. Preserves the target cell's fg/bg/flags/hyperlink
    /// (IMPLEMENTATION.md D2) — only character content and width change.
    /// No-op for out-of-bounds columns or cells that are not currently a
    /// spacer/base half (IMPLEMENTATION.md D3 reserved name).
    pub(crate) fn blank_wide_pair_split(&mut self, col: u16, row: u16) {
        let Some(idx) = self.cell_index(col, row) else {
            return;
        };
        let width = self.ring_cells[idx].width;
        if width != 0 && width != 2 {
            return;
        }
        self.ring_cells[idx].char_data = [0; 16];
        self.ring_cells[idx].char_data[0] = b' ';
        self.ring_cells[idx].char_len = 1;
        self.ring_cells[idx].width = 1;
        let abs = self.viewport_abs(row) as u32;
        let col32 = col as u32;
        if self.overflow.remove(&(col32, abs)).is_some() {
            overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
        }
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

    // ── Wide-pair partner cleanup (task0002 D2/D3) ───────────

    // AC-3: DCH with the cursor on a spacer blanks the orphaned base at
    // col-1 and leaves no orphan spacer/base after the shift.
    #[test]
    fn test_delete_characters_cursor_on_spacer_blanks_left_base() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@6, spacer@7
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);

        core.set_cursor(3, 0); // spacer of the first pair
        core.clear_dirty();
        core.handle_delete_characters(1);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // col2 was the base of the first pair; its spacer at col3 got
        // overwritten by the shift, so it is now orphaned and blanked.
        assert_eq!(core.get_cell_char(2, 0), " ");
        assert_eq!(core.get_cell_width(2, 0), 1);
        assert_eq!(core.get_cell_char(3, 0), "C");
        assert_eq!(core.get_cell_char(4, 0), "D");
        // Unrelated pair, shifted left by 1, stays intact.
        assert_eq!(core.get_cell_char(5, 0), "国");
        assert_eq!(core.get_cell_width(5, 0), 2);
        assert_eq!(core.get_cell_width(6, 0), 0);
        assert_eq!(core.get_cell_char(7, 0), "E");
        assert_eq!(core.get_cell_char(8, 0), "F");
        assert_eq!(core.get_cell_char(9, 0), " "); // BCE-filled by the delete
        assert!(core.is_row_dirty(0));
    }

    // AC-3 (continued): a base cell shifted into the position just before
    // the shift/BCE-fill boundary, having lost its spacer at that boundary,
    // is blanked rather than left as a newly-relocated no-spacer quirk.
    #[test]
    fn test_delete_characters_shift_boundary_base_loses_spacer_blanks_base() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        for ch in b'B'..=b'I' {
            core.handle_print(ch as u32);
        }
        // Directly place a base-without-spacer at the true last column,
        // mirroring the pre-existing auto-wrap-off print quirk
        // (IMPLEMENTATION.md Grid Invariant exception) as a precondition.
        core.set_cell(9, 0, "世", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        core.set_cursor(1, 0);
        core.clear_dirty();
        core.handle_delete_characters(1);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "C");
        assert_eq!(core.get_cell_char(2, 0), "D");
        assert_eq!(core.get_cell_char(3, 0), "E");
        assert_eq!(core.get_cell_char(4, 0), "F");
        assert_eq!(core.get_cell_char(5, 0), "G");
        assert_eq!(core.get_cell_char(6, 0), "H");
        assert_eq!(core.get_cell_char(7, 0), "I");
        // The base relocated to col8 is no longer the last column (col9 is,
        // now BCE-filled), so it is blanked instead of kept as a quirk.
        assert_eq!(core.get_cell_char(8, 0), " ");
        assert_eq!(core.get_cell_width(8, 0), 1);
        assert_eq!(core.get_cell_char(9, 0), " ");
        assert_eq!(core.get_cell_width(9, 0), 1);
        assert!(core.is_row_dirty(0));
    }

    // AC-4: DCH whose shift-source boundary (col+n) points into the middle
    // of a pair leaves no orphan spacer after the shift.
    #[test]
    fn test_delete_characters_shift_boundary_spacer_leaves_no_orphan() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(b'C' as u32);
        core.handle_print(0x4E16); // '世' -> base@3, spacer@4 (straddles the delete boundary)
        core.handle_print(b'D' as u32);
        core.handle_print(b'E' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@7, spacer@8
        core.handle_print(b'F' as u32);

        core.set_cursor(2, 0);
        core.clear_dirty();
        core.handle_delete_characters(2);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // col2 receives the old spacer at col4 (base at col3 was deleted);
        // that spacer is now orphaned and must be blanked, not left as a
        // dangling width-0 cell.
        assert_eq!(core.get_cell_char(2, 0), " ");
        assert_eq!(core.get_cell_width(2, 0), 1);
        assert_eq!(core.get_cell_char(3, 0), "D");
        assert_eq!(core.get_cell_char(4, 0), "E");
        // Unrelated pair, shifted left by 2, stays intact.
        assert_eq!(core.get_cell_char(5, 0), "国");
        assert_eq!(core.get_cell_width(5, 0), 2);
        assert_eq!(core.get_cell_width(6, 0), 0);
        assert_eq!(core.get_cell_char(7, 0), "F");
        assert_eq!(core.get_cell_char(8, 0), " "); // BCE-filled
        assert_eq!(core.get_cell_char(9, 0), " "); // BCE-filled
        assert!(core.is_row_dirty(0));
    }

    // AC-5: ICH with the cursor on a spacer blanks the orphaned base at
    // col-1, and the spacer relocated rightward by the shift is also
    // blanked rather than left as an orphan.
    #[test]
    fn test_insert_characters_cursor_on_spacer_blanks_left_base_and_shifted_spacer() {
        let mut core = TerminalCore::new(12, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@8, spacer@9
        core.handle_print(b'G' as u32);
        core.handle_print(b'H' as u32);

        core.set_cursor(3, 0); // spacer of the first pair
        core.clear_dirty();
        core.handle_insert_characters(2);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // col2 was the base of the first pair; its spacer at col3 is
        // overwritten by the inserted BCE run, so it is now orphaned.
        assert_eq!(core.get_cell_char(2, 0), " ");
        assert_eq!(core.get_cell_width(2, 0), 1);
        assert_eq!(core.get_cell_char(3, 0), " "); // inserted (BCE)
        assert_eq!(core.get_cell_char(4, 0), " "); // inserted (BCE)
        // col5 receives the spacer originally at col3, relocated by the
        // shift; its base stayed behind at col2 (now blanked), so this
        // relocated spacer is also orphaned and must be blanked.
        assert_eq!(core.get_cell_char(5, 0), " ");
        assert_eq!(core.get_cell_width(5, 0), 1);
        assert_eq!(core.get_cell_char(6, 0), "C");
        assert_eq!(core.get_cell_char(7, 0), "D");
        assert_eq!(core.get_cell_char(8, 0), "E");
        assert_eq!(core.get_cell_char(9, 0), "F");
        // Unrelated pair, shifted right by 2, stays intact.
        assert_eq!(core.get_cell_char(10, 0), "国");
        assert_eq!(core.get_cell_width(10, 0), 2);
        assert_eq!(core.get_cell_width(11, 0), 0);
        assert!(core.is_row_dirty(0));
    }

    // AC-6: ICH whose shift partially pushes a pair off the right edge
    // (spacer dropped, base relocated to the last column) blanks the
    // stranded base instead of leaving a newly-created no-spacer quirk.
    #[test]
    fn test_insert_characters_pushes_spacer_off_edge_blanks_base_at_last_col() {
        let mut core = TerminalCore::new(12, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@2, spacer@3
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);
        core.handle_print(b'G' as u32);
        core.handle_print(0x4E16); // '世' -> base@9, spacer@10 (target pair)
        core.handle_print(b'H' as u32);

        core.set_cursor(0, 0);
        core.clear_dirty();
        core.handle_insert_characters(2);

        assert_eq!(core.get_cell_char(0, 0), " "); // inserted (BCE)
        assert_eq!(core.get_cell_char(1, 0), " "); // inserted (BCE)
        assert_eq!(core.get_cell_char(2, 0), "A");
        assert_eq!(core.get_cell_char(3, 0), "B");
        // Unrelated pair, shifted right by 2, stays intact.
        assert_eq!(core.get_cell_char(4, 0), "国");
        assert_eq!(core.get_cell_width(4, 0), 2);
        assert_eq!(core.get_cell_width(5, 0), 0);
        assert_eq!(core.get_cell_char(6, 0), "C");
        assert_eq!(core.get_cell_char(7, 0), "D");
        assert_eq!(core.get_cell_char(8, 0), "E");
        assert_eq!(core.get_cell_char(9, 0), "F");
        assert_eq!(core.get_cell_char(10, 0), "G");
        // The target pair's spacer falls off the right edge; its base
        // relocates to the last column alone and is blanked instead of
        // being left as a newly-created no-spacer-at-last-column quirk.
        assert_eq!(core.get_cell_char(11, 0), " ");
        assert_eq!(core.get_cell_width(11, 0), 1);
        assert!(core.is_row_dirty(0));
    }
}
