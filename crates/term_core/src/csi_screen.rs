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
                let row = self.cursor.row;
                let start = self.cursor.col;
                let end = self.cols;
                // Wide-pair partner cleanup (D2/D3): see the comment on
                // handle_erase_characters below for the shape of this
                // pattern.
                let start_is_spacer = self.get_cell_width(start, row) == 0;
                let last_is_base = self.get_cell_width(end - 1, row) == 2;

                self.clear_line_range(row, start, end);

                if start_is_spacer && start > 0 {
                    self.blank_wide_pair_split(start - 1, row);
                }
                if last_is_base {
                    self.blank_wide_pair_split(end, row);
                }

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

                let row = self.cursor.row;
                let start = 0;
                let end = self.cursor.col + 1;
                // Wide-pair partner cleanup (D2/D3): see the comment on
                // handle_erase_characters below for the shape of this
                // pattern.
                let start_is_spacer = self.get_cell_width(start, row) == 0;
                let last_is_base = self.get_cell_width(end - 1, row) == 2;

                self.clear_line_range(row, start, end);

                if start_is_spacer && start > 0 {
                    self.blank_wide_pair_split(start - 1, row);
                }
                if last_is_base {
                    self.blank_wide_pair_split(end, row);
                }
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
                let row = self.cursor.row;
                let start = self.cursor.col;
                let end = self.cols;
                // Wide-pair partner cleanup (D2/D3): see the comment on
                // handle_erase_characters below for the shape of this
                // pattern.
                let start_is_spacer = self.get_cell_width(start, row) == 0;
                let last_is_base = self.get_cell_width(end - 1, row) == 2;

                self.clear_line_range(row, start, end);

                if start_is_spacer && start > 0 {
                    self.blank_wide_pair_split(start - 1, row);
                }
                if last_is_base {
                    self.blank_wide_pair_split(end, row);
                }
            }
            1 => {
                // ToStart: clear from start to cursor (inclusive)
                let row = self.cursor.row;
                let start = 0;
                let end = self.cursor.col + 1;
                // Wide-pair partner cleanup (D2/D3): see the comment on
                // handle_erase_characters below for the shape of this
                // pattern.
                let start_is_spacer = self.get_cell_width(start, row) == 0;
                let last_is_base = self.get_cell_width(end - 1, row) == 2;

                self.clear_line_range(row, start, end);

                if start_is_spacer && start > 0 {
                    self.blank_wide_pair_split(start - 1, row);
                }
                if last_is_base {
                    self.blank_wide_pair_split(end, row);
                }
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
        // clear_line_range blanks the range to BCE. The ED/EL cursor-row
        // call sites (handle_erase_in_display, handle_erase_in_line above)
        // perform this same local cleanup at their own call sites; it is
        // never folded into clear_line_range itself because that function
        // is a shared primitive whose full-row callers (clear_line, EL 2,
        // ED 2) must not gain partner behavior.
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

    // ── EL/ED wide-pair partner cleanup (task0001) ───────────

    // AC-1 (TS-1): EL 0 whose erase range starts on a spacer blanks the
    // orphaned base at col-1, preserving its ORIGINAL attributes (distinct
    // from the BCE color used for the erase). An unrelated pair before the
    // range proves no over-reach.
    #[test]
    fn test_erase_in_line_to_end_spacer_at_cursor_blanks_left_base() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@2, spacer@3
        core.set_cursor_fg(1, 9, 0, 0); // indexed 9
        core.set_cursor_bg(1, 3, 0, 0); // indexed 3
        core.handle_print(0x4E16); // '世' -> base@4, spacer@5 (colored)
        core.reset_cursor_attrs();

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(5, 0); // spacer of the target pair
        core.clear_dirty();
        core.handle_erase_in_line(0);

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // Unrelated pair before the range is untouched.
        assert_eq!(core.get_cell_char(2, 0), "国");
        assert_eq!(core.get_cell_width(2, 0), 2);
        assert_eq!(core.get_cell_width(3, 0), 0);
        // col4 was the base; its spacer at col5 is erased (inside the
        // range), so col4 is now orphaned and blanked, preserving its
        // ORIGINAL attributes (not the BCE color used for the erase).
        assert_eq!(core.get_cell_char(4, 0), " ");
        assert_eq!(core.get_cell_width(4, 0), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(4, 0)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(4, 0)),
            PackedColor::indexed(3)
        );
        // col5..9 are the actually-erased range: BCE, using the cursor's
        // current bg (indexed 5) — distinct from col4's preserved bg.
        for c in 5..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
            assert_eq!(
                PackedColor::from_u32(core.get_cell_bg(c, 0)),
                PackedColor::indexed(5)
            );
        }
        assert!(core.is_row_dirty(0));
    }

    // AC-2 (TS-2): EL 1 whose erase range ends on a base blanks the
    // orphaned spacer right after the range. An unrelated pair after the
    // range proves no over-reach.
    #[test]
    fn test_erase_in_line_to_start_base_at_cursor_blanks_right_spacer() {
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
        core.set_cursor(2, 0); // base of the target pair
        core.clear_dirty();
        core.handle_erase_in_line(1);

        // col0..2 are the actually-erased range: BCE, using the cursor's
        // current bg (indexed 5).
        for c in 0..=2 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
            assert_eq!(
                PackedColor::from_u32(core.get_cell_bg(c, 0)),
                PackedColor::indexed(5)
            );
        }
        // col3 was the spacer following the erased base; it is now orphaned
        // and blanked, preserving its ORIGINAL attributes.
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_width(3, 0), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(3, 0)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(3, 0)),
            PackedColor::indexed(3)
        );
        assert_eq!(core.get_cell_char(4, 0), "C");
        assert_eq!(core.get_cell_char(5, 0), "D");
        // Unrelated pair after the range is untouched.
        assert_eq!(core.get_cell_char(6, 0), "国");
        assert_eq!(core.get_cell_width(6, 0), 2);
        assert_eq!(core.get_cell_width(7, 0), 0);
        assert_eq!(core.get_cell_char(8, 0), "E");
        assert_eq!(core.get_cell_char(9, 0), "F");
        assert!(core.is_row_dirty(0));
    }

    // AC-3 (TS-3): ED 0 reproduces the EL 0 cursor-row result (AC-1) and
    // also fully clears the rows below the cursor.
    #[test]
    fn test_erase_in_display_below_spacer_at_cursor_blanks_left_base() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Row 0 (above cursor): must stay untouched by ED 0.
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'Z', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // Row 2 (below cursor): must be fully cleared by ED 0.
        for c in 0..10 {
            core.set_cell_ascii(c, 2, b'Y', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }

        // Row 1 (cursor row): same layout as the EL 0 case (AC-1).
        core.set_cursor(0, 1);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@2, spacer@3
        core.set_cursor_fg(1, 9, 0, 0);
        core.set_cursor_bg(1, 3, 0, 0);
        core.handle_print(0x4E16); // '世' -> base@4, spacer@5 (colored)
        core.reset_cursor_attrs();

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(5, 1); // spacer of the target pair, row 1
        core.clear_dirty();
        let result = core.handle_erase_in_display(0);
        assert_eq!(result, 0);

        // Row 0 untouched (above cursor).
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), "Z");
        }
        // Row 1: identical assertions to the EL 0 case.
        assert_eq!(core.get_cell_char(0, 1), "A");
        assert_eq!(core.get_cell_char(1, 1), "B");
        assert_eq!(core.get_cell_char(2, 1), "国");
        assert_eq!(core.get_cell_width(2, 1), 2);
        assert_eq!(core.get_cell_width(3, 1), 0);
        assert_eq!(core.get_cell_char(4, 1), " ");
        assert_eq!(core.get_cell_width(4, 1), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(4, 1)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(4, 1)),
            PackedColor::indexed(3)
        );
        for c in 5..10 {
            assert_eq!(core.get_cell_char(c, 1), " ");
            assert_eq!(core.get_cell_width(c, 1), 1);
        }
        // Row 2 fully cleared (below cursor).
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 2), " ");
        }
        assert!(core.is_row_dirty(1));
        assert!(core.is_row_dirty(2));
    }

    // AC-3 (TS-3): ED 1 reproduces the EL 1 cursor-row result (AC-2) and
    // also fully clears the rows above the cursor.
    #[test]
    fn test_erase_in_display_above_base_at_cursor_blanks_right_spacer() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Row 0 (above cursor): must be fully cleared by ED 1.
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'Z', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // Row 2 (below cursor): must stay untouched by ED 1.
        for c in 0..10 {
            core.set_cell_ascii(c, 2, b'Y', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }

        // Row 1 (cursor row): same layout as the EL 1 case (AC-2).
        core.set_cursor(0, 1);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.set_cursor_fg(1, 9, 0, 0);
        core.set_cursor_bg(1, 3, 0, 0);
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3 (colored)
        core.reset_cursor_attrs();
        core.handle_print(b'C' as u32);
        core.handle_print(b'D' as u32);
        core.handle_print(0x56FD); // '国' (unrelated pair) -> base@6, spacer@7
        core.handle_print(b'E' as u32);
        core.handle_print(b'F' as u32);

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(2, 1); // base of the target pair, row 1
        core.clear_dirty();
        let result = core.handle_erase_in_display(1);
        assert_eq!(result, 0);

        // Row 0 fully cleared (above cursor).
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
        }
        // Row 1: identical assertions to the EL 1 case.
        for c in 0..=2 {
            assert_eq!(core.get_cell_char(c, 1), " ");
            assert_eq!(core.get_cell_width(c, 1), 1);
        }
        assert_eq!(core.get_cell_char(3, 1), " ");
        assert_eq!(core.get_cell_width(3, 1), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(3, 1)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(3, 1)),
            PackedColor::indexed(3)
        );
        assert_eq!(core.get_cell_char(4, 1), "C");
        assert_eq!(core.get_cell_char(5, 1), "D");
        assert_eq!(core.get_cell_char(6, 1), "国");
        assert_eq!(core.get_cell_width(6, 1), 2);
        assert_eq!(core.get_cell_width(7, 1), 0);
        assert_eq!(core.get_cell_char(8, 1), "E");
        assert_eq!(core.get_cell_char(9, 1), "F");
        // Row 2 untouched (below cursor).
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 2), "Y");
        }
        assert!(core.is_row_dirty(0));
        assert!(core.is_row_dirty(1));
    }

    // AC-4 (TS-4): EL 0 with the cursor on the base (both halves of the
    // pair inside the range) triggers no extra blanking outside the range.
    #[test]
    fn test_erase_in_line_to_end_base_at_cursor_no_extra_blank() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(b'B' as u32);
        core.set_cursor_fg(1, 9, 0, 0);
        core.set_cursor_bg(1, 3, 0, 0);
        core.handle_print(0x4E16); // '世' -> base@2, spacer@3
        core.reset_cursor_attrs();

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(2, 0); // base of the pair (not spacer)
        core.clear_dirty();
        core.handle_erase_in_line(0);

        // col0/col1 untouched: no left-partner blank fired.
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // col2..9 (including the former base/spacer) are all plain BCE
        // fill using the erase's bg — NOT the pair's preserved original
        // bg, proving no extra (incorrect) attribute-preserving blank ran.
        for c in 2..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
            assert_eq!(
                PackedColor::from_u32(core.get_cell_bg(c, 0)),
                PackedColor::indexed(5)
            );
        }
        assert!(core.is_row_dirty(0));
    }

    // AC-4 (TS-4): EL 1 with the cursor on the spacer (base also inside the
    // range) triggers no extra blanking outside the range.
    #[test]
    fn test_erase_in_line_to_start_spacer_at_cursor_no_extra_blank() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.set_cursor_fg(1, 9, 0, 0);
        core.set_cursor_bg(1, 3, 0, 0);
        core.handle_print(0x4E16); // '世' -> base@1, spacer@2
        core.reset_cursor_attrs();
        core.handle_print(b'C' as u32);

        core.set_cursor_bg(1, 5, 0, 0); // indexed 5: BCE color for the erase
        core.set_cursor(2, 0); // spacer of the pair (not base)
        core.clear_dirty();
        core.handle_erase_in_line(1);

        // col0..2 (including the former base/spacer) are all plain BCE
        // fill using the erase's bg — proving no extra blank ran.
        for c in 0..=2 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
            assert_eq!(
                PackedColor::from_u32(core.get_cell_bg(c, 0)),
                PackedColor::indexed(5)
            );
        }
        // col3 untouched: no right-partner blank fired.
        assert_eq!(core.get_cell_char(3, 0), "C");
        assert!(core.is_row_dirty(0));
    }

    // AC-5 (TS-5): EL 0 with the cursor at col 0 (full-row range, no left
    // partner) is a safe no-op for the cleanup step. col 0 is forced into a
    // spacer-shaped cell (width 0) to exercise the `start > 0` guard —
    // without it, `start - 1` at start=0 would underflow.
    #[test]
    fn test_erase_in_line_to_end_cursor_at_col_zero_no_left_partner() {
        let mut core = TerminalCore::new(10, 1, 0);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cell(0, 0, " ", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(0, 0);
        core.clear_dirty();
        core.handle_erase_in_line(0); // must not panic (underflow guard)
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
        }
        assert!(core.is_row_dirty(0));
    }

    // AC-5 (TS-5): EL 1 with cursor.col + 1 == cols (right partner check
    // out of bounds) is a safe no-op for the cleanup step. col (cols-1) is
    // forced into a base-shaped cell (width 2) with no spacer to exercise
    // blank_wide_pair_split's out-of-bounds guard.
    #[test]
    fn test_erase_in_line_to_start_cursor_at_last_col_no_right_partner() {
        let mut core = TerminalCore::new(10, 1, 0);
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'B', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cell(9, 0, "世", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(9, 0); // cursor.col + 1 == cols
        core.clear_dirty();
        core.handle_erase_in_line(1); // must not panic (OOB guard)
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
        }
        assert!(core.is_row_dirty(0));
    }

    // AC-6 (TS-6): EL 2 over a row containing wide pairs produces a fully
    // BCE-cleared row with no behavioral change attributable to partner
    // cleanup.
    #[test]
    fn test_erase_in_line_all_wide_pair_no_partner_cleanup() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.handle_print(b'A' as u32);
        core.handle_print(0x4E16); // base@1, spacer@2
        core.handle_print(b'B' as u32);
        core.handle_print(0x56FD); // base@4, spacer@5
        core.handle_print(b'C' as u32);
        core.set_cursor(3, 0);
        core.clear_dirty();
        core.handle_erase_in_line(2);
        for c in 0..10 {
            assert_eq!(core.get_cell_char(c, 0), " ");
            assert_eq!(core.get_cell_width(c, 0), 1);
        }
        assert!(core.is_row_dirty(0));
    }

    // AC-6 (TS-6): ED 2 over rows containing wide pairs produces fully
    // BCE-cleared rows with no behavioral change attributable to partner
    // cleanup.
    #[test]
    fn test_erase_in_display_all_wide_pair_no_partner_cleanup() {
        let mut core = TerminalCore::new(10, 2, 0);
        core.handle_print(0x4E16); // row0: base@0, spacer@1
        core.set_cursor(0, 1);
        core.handle_print(0x56FD); // row1: base@0, spacer@1
        core.set_cursor(3, 0);
        core.clear_dirty();
        let result = core.handle_erase_in_display(2);
        assert_eq!(result, 0);
        for r in 0..2 {
            for c in 0..10 {
                assert_eq!(core.get_cell_char(c, r), " ");
                assert_eq!(core.get_cell_width(c, r), 1);
            }
        }
    }

    // ── Overflow wide-pair partner cleanup (task0002 D1/D2, TS4) ─────

    // TS4 (AC-2, FR2): ECH with the cursor on the spacer of an
    // overflow-table base (grapheme > 16 inline bytes) blanks the orphaned
    // base at col-1, proving blank_wide_pair_split's char_len == 0xFF
    // branch runs through the ECH call site.
    #[test]
    fn test_erase_characters_cursor_on_spacer_of_overflow_base_blanks_left_base() {
        let mut core = TerminalCore::new(10, 1, 0);
        // D1: ZWJ family emoji 👨‍👩‍👧‍👦 (7 codepoints, 25 UTF-8 bytes,
        // exceeding the 16-byte inline cell capacity) buffers in the
        // grapheme accumulator until a following non-combining char flushes
        // it to the grid (D1 note 1).
        core.handle_print(0x1F468); // 👨
        core.handle_print(0x200D); // ZWJ
        core.handle_print(0x1F469); // 👩
        core.handle_print(0x200D); // ZWJ
        core.handle_print(0x1F467); // 👧
        core.handle_print(0x200D); // ZWJ
        core.handle_print(0x1F466); // 👦
        core.handle_print(b'X' as u32); // flush -> base@col0 (overflow), spacer@col1, 'X'@col2

        // Minimal pre-condition (D1): col0 is an overflow-table base, col1
        // is its spacer.
        assert!(core.ring_cells[core.cell_index(0, 0).unwrap()].is_overflow());
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0);

        core.set_cursor(1, 0); // spacer position
        core.clear_dirty();
        core.handle_erase_characters(1);

        // Post-condition (FR2 + D4): col0 reads back as a plain space, not
        // an empty string.
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert!(core.is_row_dirty(0));
    }
}
