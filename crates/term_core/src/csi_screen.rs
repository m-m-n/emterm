/// CSI screen handlers: ED (Erase in Display), EL (Erase in Line), ECH (Erase Characters).
use crate::terminal_core::TerminalCore;

pub(crate) const SCROLLBACK_SENTINEL: u8 = 0xFF;

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
        let row = self.cursor.row;
        let start = self.cursor.col;
        let end = start.saturating_add(count).min(self.cols);
        if end <= start {
            self.clear_line_range(row, start, end);
            return;
        }

        // Wide-pair partner cleanup (D2/D3): capture pre-erase state before
        // clear_line_range blanks the range to BCE. `clear_line_range` is
        // shared with ED/EL (out of scope for this cleanup), so the
        // partner-blanking stays local to this handler and is never folded
        // into clear_line_range itself.
        let start_is_spacer = self.get_cell_width(start, row) == 0;
        let last_is_base = self.get_cell_width(end - 1, row) == 2;

        self.clear_line_range(row, start, end);

        if start_is_spacer && start > 0 {
            // The range's first erased cell was a spacer; its base at
            // start-1 (untouched by the erase) is now orphaned.
            self.blank_wide_pair_split(start - 1, row);
        }
        if last_is_base {
            // The range's last erased cell was a base; the spacer right
            // after the range (untouched by the erase) is now orphaned.
            self.blank_wide_pair_split(end, row);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 3: CSI Screen handler tests ──────────────────

    #[test]
    fn test_handle_erase_in_display_below() {
        let mut core = TerminalCore::new(10, 5, 0);
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
        let mut core = TerminalCore::new(10, 5, 0);
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
        let mut core = TerminalCore::new(10, 5, 0);
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
        let mut core = TerminalCore::new(10, 5, 0);
        let result = core.handle_erase_in_display(3);
        assert_eq!(result, 0xFF); // SCROLLBACK_SENTINEL
    }

    #[test]
    fn test_handle_erase_in_display_invalid_mode() {
        let mut core = TerminalCore::new(10, 5, 0);
        let result = core.handle_erase_in_display(99);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_handle_erase_in_line_to_end() {
        let mut core = TerminalCore::new(10, 3, 0);
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
        let mut core = TerminalCore::new(10, 3, 0);
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
        let mut core = TerminalCore::new(10, 3, 0);
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
        let mut core = TerminalCore::new(10, 3, 0);
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
        let mut core = TerminalCore::new(10, 3, 0);
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
        let mut core = TerminalCore::new(10, 3, 0);
        core.clear_dirty();
        core.set_cursor(0, 0);
        core.handle_erase_characters(5);
        assert!(core.is_row_dirty(0));
    }

    // ── Wide-pair partner cleanup (task0002 D2/D3) ───────────

    use crate::cell::PackedColor;

    // AC-1: ECH whose erase range starts on a spacer blanks the orphaned
    // base at col-1, preserving its ORIGINAL attributes (distinct from the
    // BCE color used for the actually-erased cell).
    #[test]
    fn test_erase_characters_spacer_start_blanks_left_base() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.set_cursor_fg(1, 9, 0, 0); // indexed 9
        core.set_cursor_bg(1, 3, 0, 0); // indexed 3
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3 (colored)
        core.reset_cursor_attrs();
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@6, spacer@7
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(3, 0); // spacer of the first pair
        core.clear_dirty();
        core.handle_erase_characters(1);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // col2 was the base; its spacer at col3 is erased (part of the
        // range), so col2 is now orphaned and blanked, preserving its
        // ORIGINAL attributes (not the BCE color used for the erase).
        assert_eq!(core.get_cell_char(2, 0), " ");
        assert_eq!(core.get_cell_width(2, 0), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(2, 0)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(2, 0)),
            PackedColor::indexed(3)
        );
        // col3 is the actually-erased cell: BCE, using the cursor's current
        // bg (indexed 5) — distinct from col2's preserved bg (indexed 3).
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_width(3, 0), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(3, 0)),
            PackedColor::indexed(5)
        );
        assert_eq!(core.get_cell_char(4, 0), "C");
        assert_eq!(core.get_cell_char(5, 0), "D");
        // Unrelated pair is untouched.
        assert_eq!(core.get_cell_char(6, 0), "国");
        assert_eq!(core.get_cell_width(6, 0), 2);
        assert_eq!(core.get_cell_width(7, 0), 0);
        assert_eq!(core.get_cell_char(8, 0), "E");
        assert_eq!(core.get_cell_char(9, 0), "F");
        assert!(core.is_row_dirty(0));
    }

    // AC-2: ECH whose erase range ends on a base blanks the orphaned spacer
    // right after the range.
    #[test]
    fn test_erase_characters_base_at_range_end_blanks_right_spacer() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@6, spacer@7
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);

        core.set_cursor(2, 0); // base of the first pair
        core.clear_dirty();
        core.handle_erase_characters(1);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        assert_eq!(core.get_cell_char(2, 0), " "); // erased (BCE)
        assert_eq!(core.get_cell_width(2, 0), 1);
        // col3 was the spacer following the erased base; it is now orphaned
        // and blanked.
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_width(3, 0), 1);
        assert_eq!(core.get_cell_char(4, 0), "C");
        assert_eq!(core.get_cell_char(5, 0), "D");
        // Unrelated pair is untouched.
        assert_eq!(core.get_cell_char(6, 0), "国");
        assert_eq!(core.get_cell_width(6, 0), 2);
        assert_eq!(core.get_cell_width(7, 0), 0);
        assert_eq!(core.get_cell_char(8, 0), "E");
        assert_eq!(core.get_cell_char(9, 0), "F");
        assert!(core.is_row_dirty(0));
    }
}
