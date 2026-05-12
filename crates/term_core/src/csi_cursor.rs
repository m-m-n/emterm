/// CSI cursor handlers: CUU, CUD, CUF, CUB, CNL, CPL, CHA, CUP, VPA.
use crate::terminal_core::{MODE_ORIGIN, TerminalCore};

impl TerminalCore {
    /// Convert 1-indexed ANSI col parameter to 0-indexed, clamped.
    pub(crate) fn to_zero_indexed_col(&self, col: u16) -> u16 {
        if col == 0 {
            0
        } else {
            (col - 1).min(self.cols.saturating_sub(1))
        }
    }

    /// Convert 1-indexed ANSI row parameter to 0-indexed, clamped.
    /// When origin mode (DECOM) is enabled, row is relative to scroll region.
    pub(crate) fn to_zero_indexed_row(&self, row: u16) -> u16 {
        if self.get_mode(MODE_ORIGIN) {
            let base = self.scroll_region_top;
            let max = self.scroll_region_bottom;
            if row == 0 {
                base
            } else {
                base.saturating_add(row - 1).min(max)
            }
        } else if row == 0 {
            0
        } else {
            (row - 1).min(self.rows.saturating_sub(1))
        }
    }
}

impl TerminalCore {
    /// CSI A - Cursor Up by count rows.
    /// Stops at scroll region top when cursor is within the scroll region.
    pub fn handle_cursor_up(&mut self, count: u16) {
        let min_row = if self.cursor.row >= self.scroll_region_top
            && self.cursor.row <= self.scroll_region_bottom
        {
            self.scroll_region_top
        } else {
            0
        };
        self.cursor.row = self.cursor.row.saturating_sub(count).max(min_row);
        self.wrap_pending = false;
    }

    /// CSI B - Cursor Down by count rows.
    /// Stops at scroll region bottom when cursor is within the scroll region.
    pub fn handle_cursor_down(&mut self, count: u16) {
        let max_row = if self.cursor.row >= self.scroll_region_top
            && self.cursor.row <= self.scroll_region_bottom
        {
            self.scroll_region_bottom
        } else {
            self.rows.saturating_sub(1)
        };
        self.cursor.row = self.cursor.row.saturating_add(count).min(max_row);
        self.wrap_pending = false;
    }

    /// CSI C - Cursor Forward by count cols.
    pub fn handle_cursor_forward(&mut self, count: u16) {
        self.cursor.col = self
            .cursor
            .col
            .saturating_add(count)
            .min(self.cols.saturating_sub(1));
        self.wrap_pending = false;
    }

    /// CSI D - Cursor Back by count cols.
    pub fn handle_cursor_back(&mut self, count: u16) {
        self.cursor.col = self.cursor.col.saturating_sub(count);
        self.wrap_pending = false;
    }

    /// CSI E - Cursor Next Line (down + col=0).
    /// Stops at scroll region bottom when cursor is within the scroll region.
    pub fn handle_cursor_next_line(&mut self, count: u16) {
        let max_row = if self.cursor.row >= self.scroll_region_top
            && self.cursor.row <= self.scroll_region_bottom
        {
            self.scroll_region_bottom
        } else {
            self.rows.saturating_sub(1)
        };
        self.cursor.row = self.cursor.row.saturating_add(count).min(max_row);
        self.cursor.col = 0;
        self.wrap_pending = false;
    }

    /// CSI F - Cursor Previous Line (up + col=0).
    /// Stops at scroll region top when cursor is within the scroll region.
    pub fn handle_cursor_previous_line(&mut self, count: u16) {
        let min_row = if self.cursor.row >= self.scroll_region_top
            && self.cursor.row <= self.scroll_region_bottom
        {
            self.scroll_region_top
        } else {
            0
        };
        self.cursor.row = self.cursor.row.saturating_sub(count).max(min_row);
        self.cursor.col = 0;
        self.wrap_pending = false;
    }

    /// CSI G - Cursor Horizontal Absolute (1-indexed input).
    pub fn handle_cursor_horizontal_absolute(&mut self, col: u16) {
        self.cursor.col = self.to_zero_indexed_col(col);
        self.wrap_pending = false;
    }

    /// CSI H - Cursor Position (1-indexed inputs).
    pub fn handle_cursor_position(&mut self, row: u16, col: u16) {
        self.cursor.row = self.to_zero_indexed_row(row);
        self.cursor.col = self.to_zero_indexed_col(col);
        self.wrap_pending = false;
    }

    /// CSI d - Cursor Vertical Absolute (1-indexed input).
    pub fn handle_cursor_vertical_absolute(&mut self, row: u16) {
        self.cursor.row = self.to_zero_indexed_row(row);
        self.wrap_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::{MODE_ORIGIN, TerminalCore};

    // ── Sprint 3: CSI Cursor handler tests ──────────────────

    #[test]
    fn test_handle_cursor_up_normal() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        core.handle_cursor_up(3);
        assert_eq!(core.get_cursor_row(), 2);
    }

    #[test]
    fn test_handle_cursor_up_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 2);
        core.handle_cursor_up(10);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_handle_cursor_up_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        core.set_wrap_pending(true);
        core.handle_cursor_up(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_down_normal() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 5);
        core.handle_cursor_down(3);
        assert_eq!(core.get_cursor_row(), 8);
    }

    #[test]
    fn test_handle_cursor_down_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 20);
        core.handle_cursor_down(100);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_cursor_down_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_down(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_forward_normal() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 0);
        core.handle_cursor_forward(3);
        assert_eq!(core.get_cursor_col(), 8);
    }

    #[test]
    fn test_handle_cursor_forward_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(70, 0);
        core.handle_cursor_forward(100);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_forward_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_forward(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_back_normal() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 0);
        core.handle_cursor_back(3);
        assert_eq!(core.get_cursor_col(), 2);
    }

    #[test]
    fn test_handle_cursor_back_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(2, 0);
        core.handle_cursor_back(10);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_back_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_back(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_next_line() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 5);
        core.handle_cursor_next_line(3);
        assert_eq!(core.get_cursor_row(), 8);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_next_line_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 20);
        core.handle_cursor_next_line(100);
        assert_eq!(core.get_cursor_row(), 23);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_previous_line() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 5);
        core.handle_cursor_previous_line(3);
        assert_eq!(core.get_cursor_row(), 2);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_previous_line_clamped() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 2);
        core.handle_cursor_previous_line(10);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_horizontal_absolute(5);
        assert_eq!(core.get_cursor_col(), 4); // 1-indexed → 0-indexed
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_zero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_horizontal_absolute(0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_overflow() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_horizontal_absolute(1000);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_horizontal_absolute(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_position() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_position(5, 10);
        assert_eq!(core.get_cursor_row(), 4);
        assert_eq!(core.get_cursor_col(), 9);
    }

    #[test]
    fn test_handle_cursor_position_zero_zero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 10);
        core.handle_cursor_position(0, 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_position_overflow() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_position(1000, 1000);
        assert_eq!(core.get_cursor_row(), 23);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_position_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_position(1, 1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_vertical_absolute() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_vertical_absolute(5);
        assert_eq!(core.get_cursor_row(), 4);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_zero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_vertical_absolute(0);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_overflow() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_cursor_vertical_absolute(1000);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_wrap_pending(true);
        core.handle_cursor_vertical_absolute(1);
        assert!(!core.get_wrap_pending());
    }

    // ── Origin mode (DECOM) tests ──────────────────────────

    #[test]
    fn test_origin_mode_cup_relative_to_scroll_region() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_mode(MODE_ORIGIN, true);
        // CUP(1,1) with origin mode → scroll_region_top (row 5)
        core.handle_cursor_position(1, 1);
        assert_eq!(core.get_cursor_row(), 5);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_origin_mode_cup_clamped_to_scroll_region() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_mode(MODE_ORIGIN, true);
        // CUP(100,1) with origin mode → clamped to scroll_region_bottom (row 15)
        core.handle_cursor_position(100, 1);
        assert_eq!(core.get_cursor_row(), 15);
    }

    #[test]
    fn test_origin_mode_cup_zero_goes_to_region_top() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_mode(MODE_ORIGIN, true);
        core.set_cursor(10, 10);
        core.handle_cursor_position(0, 0);
        assert_eq!(core.get_cursor_row(), 5);
    }

    #[test]
    fn test_origin_mode_vpa_relative_to_scroll_region() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_mode(MODE_ORIGIN, true);
        core.handle_cursor_vertical_absolute(3);
        // Row 3 relative to region top 5 → absolute row 7
        assert_eq!(core.get_cursor_row(), 7);
    }

    #[test]
    fn test_no_origin_mode_cup_absolute() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        // Origin mode OFF (default)
        core.handle_cursor_position(1, 1);
        assert_eq!(core.get_cursor_row(), 0);
    }

    // ── Scroll region clamping tests ───────────────────────

    #[test]
    fn test_cursor_up_stops_at_scroll_region_top() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(0, 7); // Inside scroll region
        core.handle_cursor_up(100);
        assert_eq!(core.get_cursor_row(), 5); // Stops at region top
    }

    #[test]
    fn test_cursor_up_outside_region_stops_at_zero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(0, 3); // Outside scroll region (above)
        core.handle_cursor_up(100);
        assert_eq!(core.get_cursor_row(), 0); // Stops at row 0
    }

    #[test]
    fn test_cursor_down_stops_at_scroll_region_bottom() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(0, 7); // Inside scroll region
        core.handle_cursor_down(100);
        assert_eq!(core.get_cursor_row(), 15); // Stops at region bottom
    }

    #[test]
    fn test_cursor_down_outside_region_stops_at_last_row() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(0, 20); // Outside scroll region (below)
        core.handle_cursor_down(100);
        assert_eq!(core.get_cursor_row(), 23); // Stops at last row
    }

    #[test]
    fn test_cursor_next_line_stops_at_scroll_region_bottom() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(10, 7);
        core.handle_cursor_next_line(100);
        assert_eq!(core.get_cursor_row(), 15);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_cursor_previous_line_stops_at_scroll_region_top() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_scroll_region(5, 15);
        core.set_cursor(10, 7);
        core.handle_cursor_previous_line(100);
        assert_eq!(core.get_cursor_row(), 5);
        assert_eq!(core.get_cursor_col(), 0);
    }
}
