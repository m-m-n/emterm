/// Cursor operations for TerminalCore.
///
/// Extracted from terminal_core.rs: get/set cursor position, style, colors,
/// flags, and save/restore cursor state.
use wasm_bindgen::prelude::*;

use crate::cell::*;
use crate::terminal_core::{CursorState, MODE_ORIGIN, TerminalCore};

#[wasm_bindgen]
impl TerminalCore {
    // ── Cursor ───────────────────────────────────────────

    pub fn get_cursor_col(&self) -> u16 {
        self.cursor.col
    }

    pub fn get_cursor_row(&self) -> u16 {
        self.cursor.row
    }

    pub fn set_cursor(&mut self, col: u16, row: u16) {
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.cursor.row = row.min(self.rows.saturating_sub(1));
    }

    pub fn set_cursor_col(&mut self, col: u16) {
        self.cursor.col = col.min(self.cols.saturating_sub(1));
    }

    pub fn set_cursor_row(&mut self, row: u16) {
        self.cursor.row = row.min(self.rows.saturating_sub(1));
    }

    pub fn get_cursor_visible(&self) -> bool {
        self.cursor.visible
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
    }

    pub fn get_cursor_style(&self) -> u8 {
        self.cursor.style
    }

    pub fn set_cursor_style(&mut self, style: u8) {
        self.cursor.style = if style <= 2 { style } else { 0 };
    }

    pub fn get_cursor_blink(&self) -> bool {
        self.cursor.blink
    }

    pub fn set_cursor_blink(&mut self, blink: bool) {
        self.cursor.blink = blink;
    }

    pub fn get_cursor_fg(&self) -> u32 {
        self.cursor.fg.to_u32()
    }

    pub fn set_cursor_fg(&mut self, tag: u8, r: u8, g: u8, b: u8) {
        self.cursor.fg = PackedColor { tag, r, g, b };
    }

    pub fn get_cursor_bg(&self) -> u32 {
        self.cursor.bg.to_u32()
    }

    pub fn set_cursor_bg(&mut self, tag: u8, r: u8, g: u8, b: u8) {
        self.cursor.bg = PackedColor { tag, r, g, b };
    }

    pub fn get_cursor_flags(&self) -> u16 {
        self.cursor.flags
    }

    pub fn set_cursor_flags(&mut self, flags: u16) {
        self.cursor.flags = flags;
    }

    pub fn reset_cursor_attrs(&mut self) {
        self.cursor.fg = PackedColor::DEFAULT;
        self.cursor.bg = PackedColor::DEFAULT;
        self.cursor.flags = 0;
    }

    pub fn save_cursor(&mut self) {
        let mut saved = self.cursor.clone();
        // Save charset and mode state into CursorState
        saved.g0_charset = self.g0_charset;
        saved.g1_charset = self.g1_charset;
        saved.origin_mode = self.get_mode(MODE_ORIGIN);
        saved.wrap_pending = self.wrap_pending;
        self.saved_cursor = Some(saved);
    }

    pub fn restore_cursor(&mut self) {
        if let Some(saved) = self.saved_cursor.take() {
            self.cursor = saved;
            // Clamp to current bounds
            self.cursor.col = self.cursor.col.min(self.cols.saturating_sub(1));
            self.cursor.row = self.cursor.row.min(self.rows.saturating_sub(1));
            // Restore charset and mode state
            self.g0_charset = self.cursor.g0_charset;
            self.g1_charset = self.cursor.g1_charset;
            self.set_mode(MODE_ORIGIN, self.cursor.origin_mode);
            self.wrap_pending = self.cursor.wrap_pending;
        } else {
            // Reset to defaults if no saved state
            self.cursor = CursorState::new();
            self.g0_charset = 0;
            self.g1_charset = 0;
            self.set_mode(MODE_ORIGIN, false);
            self.wrap_pending = false;
        }
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::cell::*;
    use crate::terminal_core::TerminalCore;

    #[test]
    fn test_cursor_initial() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_cursor_set_clamp() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(100, 50);
        assert_eq!(core.get_cursor_col(), 79);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_cursor_save_restore() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(10, 5);
        core.set_cursor_fg(2, 255, 0, 0);
        core.set_cursor_flags(STYLE_BOLD);
        core.save_cursor();

        core.set_cursor(0, 0);
        core.set_cursor_fg(0, 0, 0, 0);
        core.set_cursor_flags(0);

        core.restore_cursor();
        assert_eq!(core.get_cursor_col(), 10);
        assert_eq!(core.get_cursor_row(), 5);
        assert_eq!(
            PackedColor::from_u32(core.get_cursor_fg()),
            PackedColor::rgb(255, 0, 0)
        );
        assert_eq!(core.get_cursor_flags(), STYLE_BOLD);
    }
}
