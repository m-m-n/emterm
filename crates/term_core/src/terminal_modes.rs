/// Mode, tab stop, and dirty tracking operations for TerminalCore.
///
/// Extracted from terminal_core.rs: get/set terminal modes, tab stop
/// management, and dirty row tracking for differential rendering.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    // ── Modes ────────────────────────────────────────────

    pub fn get_modes(&self) -> u32 {
        self.modes
    }

    pub fn set_modes(&mut self, modes: u32) {
        self.modes = modes;
    }

    pub fn get_mode(&self, bit: u8) -> bool {
        if bit < 32 {
            (self.modes >> bit) & 1 != 0
        } else {
            false
        }
    }

    pub fn set_mode(&mut self, bit: u8, value: bool) {
        if bit < 32 {
            if value {
                self.modes |= 1 << bit;
            } else {
                self.modes &= !(1 << bit);
            }
        }
    }

    // ── Tab stops ────────────────────────────────────────

    pub fn set_tab_stop(&mut self, col: u16) {
        if (col as usize) < self.tab_stops.len() {
            self.tab_stops[col as usize] = true;
        }
    }

    pub fn clear_tab_stop(&mut self, col: u16) {
        if (col as usize) < self.tab_stops.len() {
            self.tab_stops[col as usize] = false;
        }
    }

    pub fn clear_all_tab_stops(&mut self) {
        for stop in &mut self.tab_stops {
            *stop = false;
        }
    }

    pub fn next_tab_stop(&self, from_col: u16) -> u16 {
        for i in (from_col as usize + 1)..self.tab_stops.len() {
            if self.tab_stops[i] {
                return i as u16;
            }
        }
        self.cols.saturating_sub(1)
    }

    // ── Dirty tracking ───────────────────────────────────

    pub fn get_dirty_rows(&self) -> Vec<u16> {
        let mut result = Vec::new();
        for row in 0..self.rows {
            if self.is_row_dirty(row) {
                result.push(row);
            }
        }
        result
    }

    pub fn is_row_dirty(&self, row: u16) -> bool {
        if row >= self.rows {
            return false;
        }
        let word = row as usize / 64;
        let bit = row as usize % 64;
        word < self.dirty.len() && (self.dirty[word] >> bit) & 1 != 0
    }

    pub fn mark_row_dirty(&mut self, row: u16) {
        if row < self.rows {
            let word = row as usize / 64;
            let bit = row as usize % 64;
            if word < self.dirty.len() {
                self.dirty[word] |= 1u64 << bit;
            }
        }
    }

    pub fn mark_all_dirty(&mut self) {
        for word in &mut self.dirty {
            *word = u64::MAX;
        }
    }

    pub fn clear_dirty(&mut self) {
        for word in &mut self.dirty {
            *word = 0;
        }
    }

    /// Shift dirty bits down by 1 position (row N's bit moves to row N-1).
    /// Row 0's dirty bit is discarded (scrolled into scrollback).
    /// Used when full-screen scroll optimization shifts the viewport mapping.
    pub(crate) fn shift_dirty_down_by_one(&mut self) {
        let len = self.dirty.len();
        if len == 0 {
            return;
        }
        for i in 0..len {
            // Shift current word right by 1 (row N → row N-1 within this word)
            self.dirty[i] >>= 1;
            // Bring in the lowest bit from the next word as this word's highest bit
            if i + 1 < len {
                if self.dirty[i + 1] & 1 != 0 {
                    self.dirty[i] |= 1u64 << 63;
                }
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::terminal_core::*;

    // ── Modes ────────────────────────────────────────────

    #[test]
    fn test_modes_default() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        assert!(core.get_mode(MODE_CURSOR_VISIBLE));
        assert!(core.get_mode(MODE_CURSOR_BLINK));
        assert!(!core.get_mode(MODE_ORIGIN));
        assert!(!core.get_mode(MODE_BRACKETED_PASTE));
    }

    #[test]
    fn test_modes_set_get() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_mode(MODE_BRACKETED_PASTE, true);
        assert!(core.get_mode(MODE_BRACKETED_PASTE));
        core.set_mode(MODE_AUTO_WRAP, false);
        assert!(!core.get_mode(MODE_AUTO_WRAP));
    }

    // ── Tab stops ────────────────────────────────────────

    #[test]
    fn test_tab_stops_default() {
        let core = TerminalCore::new(80, 24, 0);
        // Default: every 8 columns
        assert_eq!(core.next_tab_stop(0), 8);
        assert_eq!(core.next_tab_stop(7), 8);
        assert_eq!(core.next_tab_stop(8), 16);
        assert_eq!(core.next_tab_stop(15), 16);
    }

    #[test]
    fn test_tab_stops_set_clear() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.clear_all_tab_stops();
        core.set_tab_stop(10);
        core.set_tab_stop(30);
        assert_eq!(core.next_tab_stop(0), 10);
        assert_eq!(core.next_tab_stop(10), 30);
        assert_eq!(core.next_tab_stop(30), 79); // end of line

        core.clear_tab_stop(10);
        assert_eq!(core.next_tab_stop(0), 30);
    }

    // ── Dirty tracking ───────────────────────────────────

    #[test]
    fn test_dirty_after_set_cell() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.clear_dirty();
        assert!(!core.is_row_dirty(5));
        core.set_cell(0, 5, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(core.is_row_dirty(5));
    }

    #[test]
    fn test_dirty_clear_resets() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Initially all dirty
        assert!(!core.get_dirty_rows().is_empty());
        core.clear_dirty();
        assert!(core.get_dirty_rows().is_empty());
    }

    #[test]
    fn test_dirty_resize_marks_all() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.clear_dirty();
        core.resize(100, 30);
        let dirty = core.get_dirty_rows();
        assert_eq!(dirty.len(), 30);
    }
}
