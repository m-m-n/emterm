/// CSI scroll handlers: SU (Scroll Up), SD (Scroll Down), DECSTBM.
use crate::terminal_core::TerminalCore;

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
        self.cursor.row = if self.get_mode(crate::terminal_core::MODE_ORIGIN) {
            self.scroll_region_top
        } else {
            0
        };
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

    #[test]
    fn test_decstbm_origin_mode_homes_to_region_top() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_mode(crate::terminal_core::MODE_ORIGIN, true);
        core.handle_decstbm(5, 20);
        assert_eq!(core.get_scroll_region_top(), 4);
        assert_eq!(core.get_scroll_region_bottom(), 19);
        // Origin mode: cursor homes to scroll region top
        assert_eq!(core.get_cursor_row(), 4);
        assert_eq!(core.get_cursor_col(), 0);
    }

    // ── scroll-region-scrollback task0001: region-scroll transcription ──
    //
    // All tests below drive the core exclusively through
    // `process_pty_data_fully` (never the internal scroll helpers), per
    // the task plan's Test Notes: this is what makes them regression
    // evidence for real TUI byte sequences rather than internal-API pins.

    /// AC-1 / AC-2: a region whose top margin is the topmost screen row and
    /// whose bottom margin is above the last row transcribes the outgoing
    /// lines to scrollback in chronological order as it scrolls, while
    /// every row below the bottom margin keeps its pre-scroll content and
    /// position, and the bottom-margin row itself ends up blank.
    #[test]
    fn test_ac1_ac2_region_topmost_transcribes_and_preserves_tail() {
        let mut core = TerminalCore::new(10, 6, 10);
        // Region: rows 0..=3 (top margin is the topmost row); rows 4,5 are
        // the tail below the bottom margin.
        core.process_pty_data_fully(b"\x1b[1;4r");
        // Fill region rows with distinct markers and the tail rows with
        // content that must survive untouched.
        core.process_pty_data_fully(b"\x1b[1;1HR0");
        core.process_pty_data_fully(b"\x1b[2;1HR1");
        core.process_pty_data_fully(b"\x1b[3;1HR2");
        core.process_pty_data_fully(b"\x1b[4;1HR3");
        core.process_pty_data_fully(b"\x1b[5;1HTAIL4");
        core.process_pty_data_fully(b"\x1b[6;1HTAIL5");
        let tail4_before = core.get_line_text(4);
        let tail5_before = core.get_line_text(5);
        assert!(tail4_before.starts_with("TAIL4"));
        assert!(tail5_before.starts_with("TAIL5"));

        // Position at the region's bottom row and feed 3 line feeds, so the
        // region scrolls 3 times (N=3).
        core.process_pty_data_fully(b"\x1b[4;1H");
        core.process_pty_data_fully(b"\n\n\n");

        // AC-1: exactly 3 new scrollback lines, chronological (oldest
        // former-top-row first).
        assert_eq!(core.get_scrollback_length(), 3);
        assert_eq!(core.get_scrollback_text(0), "R0");
        assert_eq!(core.get_scrollback_text(1), "R1");
        assert_eq!(core.get_scrollback_text(2), "R2");

        // AC-2: tail rows unchanged at the same screen position.
        assert_eq!(core.get_line_text(4), tail4_before);
        assert_eq!(core.get_line_text(5), tail5_before);
        // AC-2: the bottom-margin row is blank.
        assert!(core.is_line_empty(3));
    }

    /// AC-3: a scroll-up control sequence (CSI Ps S) inside a region whose
    /// top margin is the topmost row transcribes, and a count larger than
    /// the region height transcribes exactly the region height (not the
    /// requested count).
    #[test]
    fn test_ac3_scroll_up_control_sequence_clamped_to_region_height() {
        let mut core = TerminalCore::new(10, 5, 10);
        // Region: rows 0..=2 (height 3); rows 3,4 are the tail.
        core.process_pty_data_fully(b"\x1b[1;3r");
        core.process_pty_data_fully(b"\x1b[1;1HA0");
        core.process_pty_data_fully(b"\x1b[2;1HA1");
        core.process_pty_data_fully(b"\x1b[3;1HA2");
        core.process_pty_data_fully(b"\x1b[4;1HT3");
        core.process_pty_data_fully(b"\x1b[5;1HT4");

        // Scroll-up count (10) far exceeds the region height (3).
        core.process_pty_data_fully(b"\x1b[10S");

        assert_eq!(core.get_scrollback_length(), 3);
        assert_eq!(core.get_scrollback_text(0), "A0");
        assert_eq!(core.get_scrollback_text(1), "A1");
        assert_eq!(core.get_scrollback_text(2), "A2");
        // Tail untouched.
        assert!(core.get_line_text(3).starts_with("T3"));
        assert!(core.get_line_text(4).starts_with("T4"));
    }

    /// AC-4 (negative case): a region whose top margin is NOT the topmost
    /// row pins today's in-place shift — no scrollback growth. This test
    /// necessarily passes before the transcribing path exists (there is
    /// nothing new to make red): it is a regression pin against the
    /// condition being too wide, per the task plan's Test Notes.
    #[test]
    fn test_ac4_region_top_not_topmost_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 10);
        // Region: rows 1..=4 (top margin is NOT the topmost row).
        core.process_pty_data_fully(b"\x1b[2;5r");
        core.process_pty_data_fully(b"\x1b[2;1HB0");
        core.process_pty_data_fully(b"\x1b[3;1HB1");
        core.process_pty_data_fully(b"\x1b[4;1HB2");
        core.process_pty_data_fully(b"\x1b[5;1HB3");
        core.process_pty_data_fully(b"\n\n\n");
        assert_eq!(core.get_scrollback_length(), 0);
        // In-place shift still happened: row 1 now shows old row 2.
        assert!(core.get_line_text(1).starts_with("B3") || core.is_line_empty(1));
    }

    /// AC-4 (negative case): the alternate screen being active suppresses
    /// transcription even though the region's top margin is the topmost
    /// row. Same pin-test caveat as above.
    #[test]
    fn test_ac4_alternate_screen_active_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[?1049h");
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[4;1H");
        core.process_pty_data_fully(b"\n\n\n");
        assert_eq!(core.get_scrollback_length(), 0);
    }

    /// AC-4 (negative case): line insert (CSI L) and line delete (CSI M)
    /// never reach the scroll-up routine at all (FR7) and must add no
    /// scrollback lines regardless of the region's top margin.
    #[test]
    fn test_ac4_line_insert_delete_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[1;1HC0");
        core.process_pty_data_fully(b"\x1b[2;1HC1");
        core.process_pty_data_fully(b"\x1b[3;1HC2");
        core.process_pty_data_fully(b"\x1b[4;1HC3");
        core.process_pty_data_fully(b"\x1b[1;1H\x1b[1L"); // Insert Line at row 0
        assert_eq!(core.get_scrollback_length(), 0);
        core.process_pty_data_fully(b"\x1b[1;1H\x1b[1M"); // Delete Line at row 0
        assert_eq!(core.get_scrollback_length(), 0);
    }

    /// AC-4 (negative case): the scroll-down control sequence (CSI Ps T)
    /// and reverse index (ESC M) at the region top never transcribe —
    /// FR7's "downward-scroll helper and its callers" are untouched.
    #[test]
    fn test_ac4_scroll_down_and_reverse_index_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[1;1H"); // cursor at region top
        core.process_pty_data_fully(b"\x1bM"); // ESC M: reverse index at region top
        assert_eq!(core.get_scrollback_length(), 0);
        core.process_pty_data_fully(b"\x1b[1T"); // CSI 1 T: scroll down 1
        assert_eq!(core.get_scrollback_length(), 0);
    }

    /// AC-4 (negative case): the transcription gate disabled suppresses
    /// transcription even though the other four conditions hold.
    #[test]
    fn test_ac4_gate_disabled_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.set_scroll_region_scrollback_enabled(false);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[4;1H");
        core.process_pty_data_fully(b"\n\n\n");
        assert_eq!(core.get_scrollback_length(), 0);
    }

    /// AC-4 (negative case): zero scrollback capacity suppresses
    /// transcription even though the region's top margin is the topmost
    /// row.
    #[test]
    fn test_ac4_zero_scrollback_capacity_no_transcription() {
        let mut core = TerminalCore::new(10, 6, 0);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[4;1H");
        core.process_pty_data_fully(b"\n\n\n");
        assert_eq!(core.get_scrollback_length(), 0);
    }

    /// AC-4 (positive half): after leaving the alternate screen, the same
    /// triggering byte sequence transcribes again — this is the part of
    /// AC-4 that genuinely requires the transcribing path to exist.
    #[test]
    fn test_ac4_after_leaving_alt_screen_transcription_resumes() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[?1049h");
        core.process_pty_data_fully(b"\x1b[4;1H\n");
        assert_eq!(core.get_scrollback_length(), 0);
        core.process_pty_data_fully(b"\x1b[?1049l");
        core.process_pty_data_fully(b"\x1b[4;1H\n");
        assert_eq!(core.get_scrollback_length(), 1);
    }

    /// AC-5 (unchanged half): the full-screen scroll path still emits the
    /// single-line scroll event for count=1.
    #[test]
    fn test_ac5_full_screen_scroll_unchanged_scroll_event() {
        let mut core = TerminalCore::new(10, 5, 10);
        core.clear_dirty();
        core.process_pty_data_fully(b"\x1b[5;1H\n");
        assert_eq!(core.get_scrollback_length(), 1);
        assert_eq!(core.get_scroll_event_direction(), 1);
        assert_eq!(core.get_scroll_event_count(), 1);
    }

    /// AC-5 (new half): the transcribing region path marks only the
    /// region's rows dirty and emits no full-screen scroll event.
    #[test]
    fn test_ac5_region_transcribing_marks_only_region_dirty_no_scroll_event() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.clear_dirty();
        core.process_pty_data_fully(b"\x1b[4;1H\n");
        // Confirm the transcribing path was actually taken (otherwise the
        // dirty/no-scroll-event assertions below would hold trivially on
        // the pre-existing non-transcribing region path too).
        assert_eq!(core.get_scrollback_length(), 1);
        assert!(core.scroll_event.is_none());
        assert!(core.is_row_dirty(0));
        assert!(core.is_row_dirty(1));
        assert!(core.is_row_dirty(2));
        assert!(core.is_row_dirty(3));
        assert!(!core.is_row_dirty(4));
        assert!(!core.is_row_dirty(5));
    }

    /// AC-6: transcribed lines are ordinary scrollback lines for every
    /// existing consumer — the length accessor at and below capacity, and
    /// the eviction counter incrementing exactly once per evicted line once
    /// capacity is exceeded.
    #[test]
    fn test_ac6_transcription_counts_as_ordinary_scrollback_and_evicts() {
        let mut core = TerminalCore::new(10, 5, 2); // capacity = 2
        core.process_pty_data_fully(b"\x1b[1;4r"); // region rows 0..=3
        core.process_pty_data_fully(b"\x1b[1;1HA0");
        core.process_pty_data_fully(b"\x1b[2;1HA1");
        core.process_pty_data_fully(b"\x1b[3;1HA2");
        core.process_pty_data_fully(b"\x1b[4;1HA3");
        core.process_pty_data_fully(b"\x1b[4;1H");

        // First 2 scrolls: exactly at capacity, no eviction yet.
        core.process_pty_data_fully(b"\n\n");
        assert_eq!(core.get_scrollback_length(), 2);
        assert_eq!(core.get_scrollback_evicted_total(), 0);
        assert_eq!(core.get_scrollback_text(0), "A0");
        assert_eq!(core.get_scrollback_text(1), "A1");

        // 2 more scrolls: each evicts the oldest surviving line once.
        core.process_pty_data_fully(b"\n\n");
        assert_eq!(core.get_scrollback_length(), 2);
        assert_eq!(core.get_scrollback_evicted_total(), 2);
        assert_eq!(core.get_scrollback_text(0), "A2");
        assert_eq!(core.get_scrollback_text(1), "A3");
    }

    /// AC-7: a newly constructed core has the gate enabled by default.
    #[test]
    fn test_ac7_new_core_gate_enabled_by_default() {
        let core = TerminalCore::new(10, 5, 10);
        assert!(core.scroll_region_scrollback_enabled);
    }

    /// AC-7: the setter itself has no immediate side effects — no
    /// scrollback mutation, no dirty marking, no scroll event.
    #[test]
    fn test_ac7_setter_has_no_immediate_side_effects() {
        let mut core = TerminalCore::new(10, 5, 10);
        core.clear_dirty();
        let before = core.get_scrollback_length();

        core.set_scroll_region_scrollback_enabled(false);
        assert_eq!(core.get_scrollback_length(), before);
        assert!(core.get_dirty_rows().is_empty());
        assert!(core.scroll_event.is_none());

        core.set_scroll_region_scrollback_enabled(true);
        assert_eq!(core.get_scrollback_length(), before);
        assert!(core.get_dirty_rows().is_empty());
        assert!(core.scroll_event.is_none());
    }

    /// AC-7: the setter changes only the behaviour of subsequent scrolls.
    #[test]
    fn test_ac7_setter_affects_only_subsequent_scrolls() {
        let mut core = TerminalCore::new(10, 5, 10);
        core.set_scroll_region_scrollback_enabled(false);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[4;1H\n");
        assert_eq!(core.get_scrollback_length(), 0);

        core.set_scroll_region_scrollback_enabled(true);
        core.process_pty_data_fully(b"\x1b[4;1H\n");
        assert_eq!(core.get_scrollback_length(), 1);
    }

    /// Edge case: a region whose bottom margin is the last row is the
    /// full-screen branch, not the region branch — it must keep emitting
    /// the full-screen scroll event rather than taking the new
    /// region-transcribing path (which never emits one, per AC-5).
    #[test]
    fn test_edge_region_bottom_is_last_row_uses_full_screen_path() {
        let mut core = TerminalCore::new(10, 5, 10);
        core.process_pty_data_fully(b"\x1b[1;5r"); // top=0, bottom=rows-1
        core.clear_dirty();
        core.process_pty_data_fully(b"\x1b[5;1H\n");
        assert_eq!(core.get_scroll_event_direction(), 1);
        assert_eq!(core.get_scroll_event_count(), 1);
    }

    /// Edge case: transcribed rows are ordinary scrollback rows across a
    /// resize while a (now inactive) region was in effect. The tail rows
    /// carry non-blank content so the reflow's trailing-blank trim cannot
    /// absorb the transcribed lines back into the (now smaller) viewport —
    /// otherwise a resize down to a viewport that exactly fits the total
    /// content would trivially pass regardless of whether transcription
    /// wired into reflow correctly.
    #[test]
    fn test_edge_resize_after_region_transcription_preserves_scrollback_rows() {
        let mut core = TerminalCore::new(10, 6, 10);
        core.process_pty_data_fully(b"\x1b[1;4r");
        core.process_pty_data_fully(b"\x1b[1;1HA0");
        core.process_pty_data_fully(b"\x1b[2;1HA1");
        core.process_pty_data_fully(b"\x1b[5;1HTAIL4");
        core.process_pty_data_fully(b"\x1b[6;1HTAIL5");
        core.process_pty_data_fully(b"\x1b[4;1H");
        core.process_pty_data_fully(b"\n\n");
        assert_eq!(core.get_scrollback_length(), 2);
        assert_eq!(core.get_scrollback_text(0), "A0");
        assert_eq!(core.get_scrollback_text(1), "A1");

        core.resize(20, 4);

        assert_eq!(core.get_scrollback_text(0), "A0");
        assert_eq!(core.get_scrollback_text(1), "A1");
    }
}
