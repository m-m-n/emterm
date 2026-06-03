/// ESC handler: handle_esc dispatch and handler implementations.
///
/// Action codes:
///   0 = SaveCursor (DECSC)
///   1 = RestoreCursor (DECRC)
///   2 = Index (IND)
///   3 = NextLine (NEL)
///   4 = ReverseIndex (RI)
///   5 = HorizontalTabSet (HTS)
///   6 = FullReset (RIS)
///   7 = SetG0 charset
///   8 = SetG1 charset
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    /// Dispatch an ESC action. Returns 0 always.
    /// action: code 0-8, data: charset value for SetG0/SetG1.
    pub fn handle_esc(&mut self, action: u8, data: u8) -> u8 {
        match action {
            0 => self.save_cursor(),
            1 => self.restore_cursor(),
            2 => self.esc_index(),
            3 => self.esc_next_line(),
            4 => self.esc_reverse_index(),
            5 => self.esc_horizontal_tab_set(),
            6 => self.esc_full_reset(),
            7 => self.set_g0_charset(data),
            8 => self.set_g1_charset(data),
            _ => {} // Unknown: no-op
        }
        0
    }
}

impl TerminalCore {
    /// ESC D — Index: move cursor down one line, scroll if at bottom of scroll region.
    fn esc_index(&mut self) {
        if self.cursor.row >= self.scroll_region_bottom {
            self.scroll_up_internal(1);
        } else {
            self.cursor.row += 1;
        }
    }

    /// ESC E — Next Line: carriage return + index.
    fn esc_next_line(&mut self) {
        self.cursor.col = 0;
        self.esc_index();
    }

    /// ESC M — Reverse Index: move cursor up one line, scroll down if at top of scroll region.
    fn esc_reverse_index(&mut self) {
        if self.cursor.row <= self.scroll_region_top {
            self.scroll_down_internal(1);
        } else {
            self.cursor.row -= 1;
        }
    }

    /// ESC H — Horizontal Tab Set: set tab stop at cursor column.
    fn esc_horizontal_tab_set(&mut self) {
        let col = self.cursor.col as usize;
        if col < self.tab_stops.len() {
            self.tab_stops[col] = true;
        }
    }

    /// ESC c — Full Reset (RIS): reset entire terminal state.
    fn esc_full_reset(&mut self) {
        self.reset();
    }

    /// Internal ESC dispatch: maps raw (intermediate, final_byte) to handler calls.
    pub(crate) fn handle_esc_internal(&mut self, intermediate: Option<u8>, final_byte: u8) {
        match (intermediate, final_byte) {
            (Some(b'('), byte) => self.set_g0_charset(charset_byte_to_value(byte)),
            (Some(b')'), byte) => self.set_g1_charset(charset_byte_to_value(byte)),
            (None, b'7') => self.save_cursor(),
            (None, b'8') => self.restore_cursor(),
            (None, b'D') => self.esc_index(),
            (None, b'E') => self.esc_next_line(),
            (None, b'M') => self.esc_reverse_index(),
            (None, b'H') => self.esc_horizontal_tab_set(),
            (None, b'c') => self.esc_full_reset(),
            _ => {} // Unknown: ignore
        }
    }
}

fn charset_byte_to_value(byte: u8) -> u8 {
    match byte {
        b'0' => 1, // DecLineDrawing
        _ => 0,    // ASCII (includes 'B', 'A', etc.)
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::{MODE_ORIGIN, TerminalCore};

    // ── SaveCursor / RestoreCursor ──────────────────────

    #[test]
    fn test_save_restore_cursor_position_and_attrs() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 5);
        core.set_cursor_fg(2, 100, 200, 50); // RGB fg
        core.set_g0_charset(1);
        core.set_g1_charset(1);
        core.set_wrap_pending(true);
        core.set_mode(MODE_ORIGIN, true);

        core.handle_esc(0, 0); // SaveCursor

        // Change state
        core.set_cursor(0, 0);
        core.reset_cursor_attrs();
        core.set_g0_charset(0);
        core.set_g1_charset(0);
        core.set_wrap_pending(false);
        core.set_mode(MODE_ORIGIN, false);

        core.handle_esc(1, 0); // RestoreCursor

        assert_eq!(core.get_cursor_col(), 10);
        assert_eq!(core.get_cursor_row(), 5);
        assert_eq!(core.get_g0_charset(), 1);
        assert_eq!(core.get_g1_charset(), 1);
        assert!(core.get_wrap_pending());
        assert!(core.get_mode(MODE_ORIGIN));
    }

    #[test]
    fn test_restore_cursor_no_saved_state_defaults() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 10);
        core.set_g0_charset(1);
        core.set_mode(MODE_ORIGIN, true);
        core.set_wrap_pending(true);

        core.handle_esc(1, 0); // RestoreCursor (no saved state)

        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_g0_charset(), 0);
        assert_eq!(core.get_g1_charset(), 0);
        assert!(!core.get_wrap_pending());
        assert!(!core.get_mode(MODE_ORIGIN));
    }

    // ── Index (IND) ─────────────────────────────────────

    #[test]
    fn test_esc_index_mid_screen() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 10);
        core.handle_esc(2, 0); // Index
        assert_eq!(core.get_cursor_row(), 11);
        assert_eq!(core.get_cursor_col(), 5); // col unchanged
    }

    #[test]
    fn test_esc_index_at_scroll_bottom_full_screen() {
        let mut core = TerminalCore::new(10, 5, 5);
        for r in 0..5 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(3, 4); // At bottom
        core.handle_esc(2, 0); // Index → scroll up
        assert_eq!(core.get_cursor_row(), 4); // Stays at bottom
        assert_eq!(core.get_cursor_col(), 3);
        // Content shifted up
        assert_eq!(core.get_cell_char(0, 0), "1");
        assert_eq!(core.get_cell_char(0, 3), "4");
        assert_eq!(core.get_cell_char(0, 4), " "); // new blank
        assert_eq!(core.get_scrollback_length(), 1);
    }

    #[test]
    fn test_esc_index_at_scroll_region_bottom() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        for r in 2..=7 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(0, 7); // At scroll region bottom
        core.handle_esc(2, 0); // Index → region scroll
        assert_eq!(core.get_cursor_row(), 7);
        assert_eq!(core.get_cell_char(0, 2), "3"); // shifted up
        assert_eq!(core.get_cell_char(0, 7), " "); // new blank
    }

    // ── NextLine (NEL) ──────────────────────────────────

    #[test]
    fn test_esc_next_line_mid_screen() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(30, 10);
        core.handle_esc(3, 0); // NextLine
        assert_eq!(core.get_cursor_col(), 0); // CR
        assert_eq!(core.get_cursor_row(), 11); // LF
    }

    #[test]
    fn test_esc_next_line_at_bottom() {
        let mut core = TerminalCore::new(10, 5, 5);
        core.set_cursor(5, 4);
        core.handle_esc(3, 0); // NextLine at bottom → scroll
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 4);
        assert_eq!(core.get_scrollback_length(), 1);
    }

    // ── ReverseIndex (RI) ───────────────────────────────

    #[test]
    fn test_esc_reverse_index_mid_screen() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 10);
        core.handle_esc(4, 0); // ReverseIndex
        assert_eq!(core.get_cursor_row(), 9);
        assert_eq!(core.get_cursor_col(), 5); // col unchanged
    }

    #[test]
    fn test_esc_reverse_index_at_scroll_top() {
        let mut core = TerminalCore::new(10, 5, 0);
        for r in 0..5 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(0, 0); // At top
        core.handle_esc(4, 0); // ReverseIndex → scroll down
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_cell_char(0, 0), " "); // new blank
        assert_eq!(core.get_cell_char(0, 1), "0"); // shifted down
        assert_eq!(core.get_cell_char(0, 2), "1");
    }

    #[test]
    fn test_esc_reverse_index_at_scroll_region_top() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_scroll_region(2, 7);
        for r in 2..=7 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(0, 2); // At scroll region top
        core.handle_esc(4, 0); // ReverseIndex → region scroll down
        assert_eq!(core.get_cursor_row(), 2);
        assert_eq!(core.get_cell_char(0, 2), " "); // new blank
        assert_eq!(core.get_cell_char(0, 3), "2"); // shifted down
    }

    // ── HTS ─────────────────────────────────────────────

    #[test]
    fn test_esc_hts_sets_tab_stop() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.clear_all_tab_stops();
        core.set_cursor(5, 0);
        core.handle_esc(5, 0); // HTS at col 5
        core.set_cursor(0, 0);
        core.handle_execute(0x09); // HT → should go to col 5
        assert_eq!(core.get_cursor_col(), 5);
    }

    // ── RIS ─────────────────────────────────────────────

    #[test]
    fn test_esc_ris_resets_all() {
        let mut core = TerminalCore::new(10, 5, 5);
        // Set various state
        core.set_cursor(5, 3);
        core.set_g0_charset(1);
        core.set_g1_charset(1);
        core.set_active_charset(1);
        core.set_cell(0, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Generate some scrollback (full screen scroll)
        core.scroll_up_internal(1);
        assert!(core.get_scrollback_length() > 0);
        // Now set scroll region (after generating scrollback)
        core.set_scroll_region(1, 3);

        core.handle_esc(6, 0); // RIS

        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_g0_charset(), 0);
        assert_eq!(core.get_g1_charset(), 0);
        assert_eq!(core.get_active_charset(), 0);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 4);
        // Ring buffer reset
        assert_eq!(core.get_scrollback_length(), 0);
    }

    // ── SetG0 / SetG1 ──────────────────────────────────

    #[test]
    fn test_esc_set_g0() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_esc(7, 1); // SetG0 = DecLineDrawing
        assert_eq!(core.get_g0_charset(), 1);
        core.handle_esc(7, 0); // SetG0 = ASCII
        assert_eq!(core.get_g0_charset(), 0);
    }

    #[test]
    fn test_esc_set_g1() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_esc(8, 1); // SetG1 = DecLineDrawing
        assert_eq!(core.get_g1_charset(), 1);
        core.handle_esc(8, 0); // SetG1 = ASCII
        assert_eq!(core.get_g1_charset(), 0);
    }

    // ── Unknown action ──────────────────────────────────

    #[test]
    fn test_esc_unknown_action_noop() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 5);
        let result = core.handle_esc(99, 0);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 5);
        assert_eq!(core.get_cursor_row(), 5);
    }
}
