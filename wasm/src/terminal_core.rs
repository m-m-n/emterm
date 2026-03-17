/// TerminalCore: viewport grid and terminal state in WASM linear memory.
///
/// Owns the viewport grid (rows × cols cells), cursor state, terminal modes,
/// tab stops, and dirty row tracking. Exported via wasm_bindgen for JS access.
use wasm_bindgen::prelude::*;

use crate::cell::*;

// ── Mode bit positions (matches SPEC.md) ─────────────────

pub const MODE_AUTO_WRAP: u8 = 0;
pub const MODE_ORIGIN: u8 = 1;
pub const MODE_CURSOR_VISIBLE: u8 = 2;
pub const MODE_CURSOR_BLINK: u8 = 3;
pub const MODE_REVERSE_SCREEN: u8 = 4;
pub const MODE_BRACKETED_PASTE: u8 = 5;
pub const MODE_FOCUS_TRACKING: u8 = 6;
pub const MODE_COLUMN_132: u8 = 7;
// Bits 8-9: cursor keys (2 bits)
// Bits 10-11: mouse tracking (2 bits)
// Bits 12-13: mouse encoding (2 bits)

// ── CursorState ──────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct CursorState {
    pub(crate) col: u16,
    pub(crate) row: u16,
    pub(crate) fg: PackedColor,
    pub(crate) bg: PackedColor,
    pub(crate) flags: u16,
    pub(crate) visible: bool,
    pub(crate) style: u8, // 0=block, 1=underline, 2=bar
    pub(crate) blink: bool,
    // SaveCursor/RestoreCursor extended fields
    pub(crate) g0_charset: u8,
    pub(crate) g1_charset: u8,
    pub(crate) origin_mode: bool,
    pub(crate) wrap_pending: bool,
}

impl CursorState {
    pub(crate) fn new() -> Self {
        Self {
            col: 0,
            row: 0,
            fg: PackedColor::DEFAULT,
            bg: PackedColor::DEFAULT,
            flags: 0,
            visible: true,
            style: 0,
            blink: true,
            g0_charset: 0,
            g1_charset: 0,
            origin_mode: false,
            wrap_pending: false,
        }
    }
}

// ── TerminalCore ─────────────────────────────────────────

#[wasm_bindgen]
pub struct TerminalCore {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    // Ring buffer: unified viewport + scrollback storage
    pub(crate) ring_cells: Vec<Cell>,
    pub(crate) ring_wrapped: Vec<bool>,
    pub(crate) ring_head: usize,     // Index of oldest line in ring
    pub(crate) ring_size: usize,     // Current number of lines (>= rows)
    pub(crate) ring_capacity: usize, // Maximum line count (scrollback_lines + rows)
    pub(crate) dirty: Vec<u64>,
    pub(crate) cursor: CursorState,
    pub(crate) saved_cursor: Option<CursorState>,
    pub(crate) modes: u32,
    pub(crate) tab_stops: Vec<bool>,
    pub(crate) overflow: OverflowTable,
    pub(crate) overflow_ridx: OverflowRowIndex,
    // Sprint 2: Print handler state
    pub(crate) grapheme_buffer: Vec<u32>,
    pub(crate) wrap_pending: bool,
    pub(crate) g0_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) g1_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) active_charset: u8, // 0=G0, 1=G1
    /// Suppress Kitty Unicode placeholder characters (U+10EEEE + combining marks).
    /// Set when U+10EEEE is received; cleared on next non-combining codepoint.
    pub(crate) kitty_placeholder_active: bool,
    pub(crate) scroll_region_top: u16,
    pub(crate) scroll_region_bottom: u16,
    // Sprint 4: Device response buffer
    pub(crate) response_buffer: [u8; 64],
    pub(crate) response_len: u8,
    // Cell size in pixels (for CSI 14t/16t responses)
    pub(crate) cell_width_px: u16,
    pub(crate) cell_height_px: u16,
    // Scroll event for differential rendering
    pub(crate) scroll_event: Option<crate::ring_buffer::ScrollEvent>,
    // Sprint 6: Parser and mode action queue
    pub(crate) parser: crate::parser::Parser,
    pub(crate) mode_actions: Vec<u8>,
    // Sprint 6: Callbacks (read on wasm32 only via fire_*_callback / set_*_callback)
    #[allow(dead_code)]
    pub(crate) osc_callback: Option<crate::callbacks::Callback>,
    #[allow(dead_code)]
    pub(crate) apc_callback: Option<crate::callbacks::Callback>,
    #[allow(dead_code)]
    pub(crate) dcs_callback: Option<crate::callbacks::Callback>,
    #[allow(dead_code)]
    pub(crate) bell_callback: Option<crate::callbacks::Callback>,
    #[allow(dead_code)]
    pub(crate) device_response_callback: Option<crate::callbacks::Callback>,
    // Hyperlink table: maps hyperlink_id -> (params, uri)
    pub(crate) hyperlink_table: Vec<Option<(String, String)>>,
    pub(crate) hyperlink_next_id: u16,
    pub(crate) active_hyperlink_id: u16,
    /// Set when cursor transitions hidden→visible (DECTCEM set while previously hidden).
    /// Used by process_pty_data to interrupt parsing so the JS side can render
    /// the intermediate state (e.g., vim's search wrap message).
    pub(crate) cursor_just_shown: bool,
    /// When true, cursor hidden→visible transitions interrupt parsing.
    /// Disable if it causes flicker in applications that frequently toggle cursor.
    pub(crate) cursor_show_interrupt: bool,
}

#[wasm_bindgen]
impl TerminalCore {
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u16, rows: u16, scrollback_lines: u32) -> Self {
        debug_assert!(cols > 0 && rows > 0, "cols and rows must be > 0");
        let ring_capacity = scrollback_lines as usize + rows as usize;
        let total = ring_capacity * cols as usize;
        let dirty_words = (rows as usize + 63) / 64;

        // Default modes: autoWrap=true, cursorVisible=true, cursorBlink=true
        let default_modes =
            (1u32 << MODE_AUTO_WRAP) | (1u32 << MODE_CURSOR_VISIBLE) | (1u32 << MODE_CURSOR_BLINK);

        let mut tab_stops = vec![false; cols as usize];
        for i in (0..cols as usize).step_by(8) {
            tab_stops[i] = true;
        }

        let mut core = Self {
            cols,
            rows,
            ring_cells: vec![Cell::EMPTY; total],
            ring_wrapped: vec![false; ring_capacity],
            ring_head: 0,
            ring_size: rows as usize,
            ring_capacity,
            dirty: vec![u64::MAX; dirty_words], // all dirty initially
            cursor: CursorState::new(),
            saved_cursor: None,
            modes: default_modes,
            tab_stops,
            overflow: OverflowTable::new(),
            overflow_ridx: OverflowRowIndex::new(),
            // Sprint 2
            grapheme_buffer: Vec::with_capacity(8),
            wrap_pending: false,
            g0_charset: 0,
            g1_charset: 0,
            active_charset: 0,
            kitty_placeholder_active: false,
            scroll_region_top: 0,
            scroll_region_bottom: rows.saturating_sub(1),
            // Sprint 4
            response_buffer: [0u8; 64],
            response_len: 0,
            cell_width_px: 8,
            cell_height_px: 16,
            // Scroll event
            scroll_event: None,
            // Sprint 6
            parser: crate::parser::Parser::new(),
            mode_actions: Vec::new(),
            // Sprint 6: Callbacks
            osc_callback: None,
            apc_callback: None,
            dcs_callback: None,
            bell_callback: None,
            device_response_callback: None,
            // Hyperlink
            hyperlink_table: vec![None], // index 0 = no hyperlink
            hyperlink_next_id: 1,
            active_hyperlink_id: 0,
            cursor_just_shown: false,
            cursor_show_interrupt: false,
        };
        core.mark_all_dirty();
        core
    }

    // ── Grid dimensions ──────────────────────────────────

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Set cell size in pixels (for CSI 14t/16t XTWINOPS responses).
    /// Called from TypeScript after measuring character dimensions.
    pub fn set_cell_size_px(&mut self, width: u16, height: u16) {
        self.cell_width_px = width;
        self.cell_height_px = height;
    }

    /// Get cell width in pixels.
    pub fn get_cell_width_px(&self) -> u16 {
        self.cell_width_px
    }

    /// Get cell height in pixels.
    pub fn get_cell_height_px(&self) -> u16 {
        self.cell_height_px
    }

    // ── Resize ───────────────────────────────────────────

    /// Legacy resize (delegates to resize_reflow with scrollback_lines=0).
    /// Kept for backward compatibility with existing tests.
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        // Use reflow with current scrollback capacity
        let scrollback_lines = self.ring_capacity.saturating_sub(self.rows as usize) as u32;
        self.resize_reflow(new_cols, new_rows, scrollback_lines);
    }

    // ── Scroll Event ─────────────────────────────────────

    /// Returns the scroll event direction: 1=Up, 0=none.
    pub fn get_scroll_event_direction(&self) -> u8 {
        match &self.scroll_event {
            Some(e) => match e.direction {
                crate::ring_buffer::ScrollDirection::Up => 1,
            },
            None => 0,
        }
    }

    /// Returns the scroll event count (0 if no event).
    pub fn get_scroll_event_count(&self) -> u16 {
        self.scroll_event.as_ref().map_or(0, |e| e.count)
    }

    /// Clears the pending scroll event.
    pub fn clear_scroll_event(&mut self) {
        self.scroll_event = None;
    }

    // ── Reset ────────────────────────────────────────────

    pub fn reset(&mut self) {
        let total = self.ring_capacity * self.cols as usize;
        self.ring_cells = vec![Cell::EMPTY; total];
        self.ring_wrapped = vec![false; self.ring_capacity];
        self.ring_head = 0;
        self.ring_size = self.rows as usize;
        self.cursor = CursorState::new();
        self.saved_cursor = None;
        self.modes =
            (1u32 << MODE_AUTO_WRAP) | (1u32 << MODE_CURSOR_VISIBLE) | (1u32 << MODE_CURSOR_BLINK);
        self.tab_stops = vec![false; self.cols as usize];
        for i in (0..self.cols as usize).step_by(8) {
            self.tab_stops[i] = true;
        }
        self.overflow.clear();
        self.overflow_ridx.clear();
        self.scroll_event = None;
        // Sprint 2
        self.grapheme_buffer.clear();
        self.wrap_pending = false;
        self.g0_charset = 0;
        self.g1_charset = 0;
        self.active_charset = 0;
        self.scroll_region_top = 0;
        self.scroll_region_bottom = self.rows.saturating_sub(1);
        // Sprint 4
        self.response_buffer = [0u8; 64];
        self.response_len = 0;
        // Sprint 6
        self.parser.reset();
        self.mode_actions.clear();
        self.cursor_just_shown = false;
        // Note: callbacks are NOT cleared on reset (terminal reset != dispose)
        self.mark_all_dirty();
    }

    /// Take and clear the mode action queue.
    pub fn take_mode_actions(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.mode_actions)
    }

    /// Enable/disable cursor hidden→visible interrupt.
    pub fn set_cursor_show_interrupt(&mut self, enable: bool) {
        self.cursor_show_interrupt = enable;
    }

    // ── Sprint 2: Charset ───────────────────────────────

    pub fn get_g0_charset(&self) -> u8 {
        self.g0_charset
    }

    pub fn set_g0_charset(&mut self, val: u8) {
        self.g0_charset = if val <= 1 { val } else { 0 };
    }

    pub fn get_g1_charset(&self) -> u8 {
        self.g1_charset
    }

    pub fn set_g1_charset(&mut self, val: u8) {
        self.g1_charset = if val <= 1 { val } else { 0 };
    }

    pub fn get_active_charset(&self) -> u8 {
        self.active_charset
    }

    pub fn set_active_charset(&mut self, val: u8) {
        self.active_charset = if val <= 1 { val } else { 0 };
    }

    // ── Sprint 2: Scroll region ─────────────────────────

    pub fn get_scroll_region_top(&self) -> u16 {
        self.scroll_region_top
    }

    pub fn get_scroll_region_bottom(&self) -> u16 {
        self.scroll_region_bottom
    }

    pub fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let t = top.min(self.rows.saturating_sub(1));
        let b = bottom.min(self.rows.saturating_sub(1));
        if t < b {
            self.scroll_region_top = t;
            self.scroll_region_bottom = b;
        } else {
            // Invalid region: reset to full screen
            self.scroll_region_top = 0;
            self.scroll_region_bottom = self.rows.saturating_sub(1);
        }
    }

    // ── Sprint 2: Wrap pending ──────────────────────────

    pub fn get_wrap_pending(&self) -> bool {
        self.wrap_pending
    }

    pub fn set_wrap_pending(&mut self, val: bool) {
        self.wrap_pending = val;
    }

    // ── Sprint 2: Grapheme buffer ───────────────────────

    pub fn get_grapheme_buffer_len(&self) -> u32 {
        self.grapheme_buffer.len() as u32
    }

    pub fn clear_grapheme_buffer(&mut self) {
        self.grapheme_buffer.clear();
    }

    // ── Sprint 2: Internal print helpers ────────────────

    pub(crate) fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    /// Advance cursor row. Scrolls internally if at scroll_region_bottom.
    pub(crate) fn line_feed(&mut self) {
        if self.cursor.row >= self.scroll_region_bottom {
            self.scroll_up_internal(1);
        } else {
            self.cursor.row += 1;
        }
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Grid construction ────────────────────────────────

    #[test]
    fn test_grid_new_80x24() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        // All cells should be empty spaces
        for row in 0..24 {
            assert!(core.is_line_empty(row));
        }
    }

    // ── Cell set/get round-trip ──────────────────────────

    #[test]
    fn test_set_get_cell_ascii() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    #[test]
    fn test_set_get_cell_cjk() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(5, 3, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(5, 3), "漢");
        assert_eq!(core.get_cell_width(5, 3), 2);
    }

    #[test]
    fn test_set_get_cell_ascii_fast() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_ascii(10, 5, b'Z', 2, 100, 200, 50, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(10, 5), "Z");
        assert_eq!(core.get_cell_width(10, 5), 1);
        let fg = core.get_cell_fg(10, 5);
        assert_eq!(fg >> 24, 2); // tag = RGB
        assert_eq!((fg >> 16) & 0xFF, 100); // r
    }

    #[test]
    fn test_set_get_cell_with_attrs() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Set with RGB fg, indexed bg, bold+italic
        core.set_cell(
            0,
            0,
            "X",
            1,
            2,
            255,
            128,
            64,
            1,
            42,
            0,
            0,
            STYLE_BOLD | STYLE_ITALIC,
        );
        assert_eq!(core.get_cell_char(0, 0), "X");
        let fg = core.get_cell_fg(0, 0);
        assert_eq!(PackedColor::from_u32(fg), PackedColor::rgb(255, 128, 64));
        let bg = core.get_cell_bg(0, 0);
        assert_eq!(PackedColor::from_u32(bg), PackedColor::indexed(42));
        assert_eq!(core.get_cell_flags(0, 0), STYLE_BOLD | STYLE_ITALIC);
    }

    // ── Out-of-bounds ────────────────────────────────────

    #[test]
    fn test_oob_write_noop() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Should not panic
        core.set_cell(80, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 24, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }

    #[test]
    fn test_oob_read_default() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.get_cell_char(80, 0), " ");
        assert_eq!(core.get_cell_width(0, 24), 1);
        assert_eq!(core.get_cell_fg(100, 100), 0);
    }

    // ── Line operations ──────────────────────────────────

    #[test]
    fn test_clear_line() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.clear_line(0);
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(1, 0), " ");
        assert!(core.is_line_empty(0));
    }

    #[test]
    fn test_clear_line_range() {
        let mut core = TerminalCore::new(80, 24, 0);
        for col in 0..10 {
            core.set_cell(col, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_line_range(0, 3, 7);
        assert_eq!(core.get_cell_char(2, 0), "X");
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_char(6, 0), " ");
        assert_eq!(core.get_cell_char(7, 0), "X");
    }

    #[test]
    fn test_get_line_text() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.set_cell(0, 0, "H", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Width-0 placeholder (e.g., second cell of wide char)
        let text = core.get_line_text(0);
        assert!(text.starts_with("Hi"));
    }

    #[test]
    fn test_get_line_text_skips_width0() {
        let mut core = TerminalCore::new(10, 1, 0);
        core.set_cell(0, 0, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Set width=0 placeholder at col 1
        core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let text = core.get_line_text(0);
        // Should have "漢" followed by spaces, not the empty placeholder
        assert!(text.starts_with("漢"));
        assert!(!text.contains('\0'));
    }

    #[test]
    fn test_is_line_empty() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert!(core.is_line_empty(0));
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.is_line_empty(0));
    }

    // ── Row operations ───────────────────────────────────

    #[test]
    fn test_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Set identifiable content on each row
        for row in 0..5 {
            core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.shift_rows_up(0, 4, 2);
        // Row 0 should now have what was row 2
        assert_eq!(core.get_cell_char(0, 0), "2");
        assert_eq!(core.get_cell_char(0, 1), "3");
        assert_eq!(core.get_cell_char(0, 2), "4");
        // Bottom rows should be cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
    }

    #[test]
    fn test_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.shift_rows_down(0, 4, 2);
        // Top rows should be cleared
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(0, 1), " ");
        // Original rows shifted down
        assert_eq!(core.get_cell_char(0, 2), "0");
        assert_eq!(core.get_cell_char(0, 3), "1");
        assert_eq!(core.get_cell_char(0, 4), "2");
    }

    #[test]
    fn test_copy_row() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "X", 1, 2, 255, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
        core.set_line_wrapped(0, true);
        core.copy_row(0, 3);
        assert_eq!(core.get_cell_char(0, 3), "X");
        assert_eq!(core.get_cell_flags(0, 3), STYLE_BOLD);
        assert!(core.get_line_wrapped(3));
    }

    #[test]
    fn test_fill_row_default() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 2, "Z", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.fill_row_default(2);
        assert!(core.is_line_empty(2));
    }

    // ── Resize ───────────────────────────────────────────

    #[test]
    fn test_resize_grow_cols() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(20, 5);
        assert_eq!(core.cols(), 20);
        assert_eq!(core.get_cell_char(5, 0), "A");
    }

    #[test]
    fn test_resize_shrink_cols() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(8, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(5, 5);
        assert_eq!(core.cols(), 5);
        // Col 8 should be gone, reading it via get_cell_char returns default
        assert_eq!(core.get_cell_char(8, 0), " ");
    }

    #[test]
    fn test_resize_grow_shrink_rows() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        // Grow
        core.resize(10, 10);
        assert_eq!(core.rows(), 10);
        assert_eq!(core.get_cell_char(0, 0), "A");

        // Shrink
        core.resize(10, 3);
        assert_eq!(core.rows(), 3);
        assert_eq!(core.get_cell_char(0, 0), "A");
    }

    // ── Reset ────────────────────────────────────────────

    #[test]
    fn test_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell(5, 5, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
        core.set_cursor(40, 12);
        core.set_mode(MODE_BRACKETED_PASTE, true);
        core.reset();

        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        assert!(!core.get_mode(MODE_BRACKETED_PASTE));
        assert!(core.is_line_empty(5));
    }

    // ── Batch row packed ─────────────────────────────────

    #[test]
    fn test_get_row_packed_basic() {
        let mut core = TerminalCore::new(3, 1, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let packed = core.get_row_packed(0);
        assert!(!packed.is_empty());
        // First byte should be char_len=1, then 'A'
        assert_eq!(packed[0], 1); // char_len
        assert_eq!(packed[1], b'A'); // char data
    }

    // ── Overflow side table with shift ───────────────────

    #[test]
    fn test_overflow_remapped_on_shift_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        assert!(long.as_bytes().len() > 16);
        core.set_cell(0, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 3), long);

        core.shift_rows_up(0, 4, 2);
        // Row 3 shifted to row 1
        assert_eq!(core.get_cell_char(0, 1), long);
    }

    // ── Phase 4: Reverse index tests ────────────────────

    #[test]
    fn test_ridx_maintained_on_set_cell_overflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));
        assert!(core.overflow_ridx[&abs].contains(&3));
    }

    #[test]
    fn test_ridx_removed_on_overwrite_with_ascii() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));

        // Overwrite with ASCII
        core.set_cell_ascii(3, 2, b'X', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    #[test]
    fn test_ridx_maintained_after_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let old_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&old_abs));

        core.shift_rows_up(0, 4, 2);
        // Row 3 -> row 1
        let new_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&new_abs));
        assert!(core.overflow_ridx[&new_abs].contains(&5));
        // Old abs should be gone
        assert!(!core.overflow_ridx.contains_key(&old_abs));
    }

    #[test]
    fn test_ridx_maintained_after_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let old_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&old_abs));

        core.shift_rows_down(0, 4, 2);
        // Row 1 -> row 3
        let new_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&new_abs));
        assert!(core.overflow_ridx[&new_abs].contains(&5));
    }

    #[test]
    fn test_ridx_cleared_on_clear_line() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let abs = core.viewport_abs(2) as u32;
        assert!(core.overflow_ridx.contains_key(&abs));

        core.clear_line(2);
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    #[test]
    fn test_ridx_copy_row() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        core.copy_row(1, 3);
        let dst_abs = core.viewport_abs(3) as u32;
        assert!(core.overflow_ridx.contains_key(&dst_abs));
        assert!(core.overflow_ridx[&dst_abs].contains(&5));
        // Source should still have it
        let src_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow_ridx.contains_key(&src_abs));
    }

    #[test]
    fn test_ridx_cleared_on_reset() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.is_empty());

        core.reset();
        assert!(core.overflow_ridx.is_empty());
    }

    // ── process_pty_data interruptible tests ─────────────

    #[test]
    fn test_process_pty_data_normal_consumes_all() {
        let mut core = TerminalCore::new(80, 24, 0);
        let data = b"Hello";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, data.len());
        assert!(core.mode_actions.is_empty());
    }

    #[test]
    fn test_process_pty_data_stops_on_buffer_switch() {
        let mut core = TerminalCore::new(80, 24, 0);
        // CSI ?1049h (8 bytes) followed by "AB"
        let data = b"\x1B[?1049hAB";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, 8);
        assert!(core.has_pending_buffer_switch());
        // The mode action should be MODE_ACTION_SAVE_AND_SWITCH_TO_ALT (2)
        let actions = core.take_mode_actions();
        assert!(actions.contains(&2));
    }

    #[test]
    fn test_has_pending_buffer_switch_empty() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(!core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_skips_ts_fallback() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Simulate TS_FALLBACK entry: [0xFF, lo, hi]
        core.mode_actions.push(0xFF);
        core.mode_actions.push(0x01);
        core.mode_actions.push(0x00);
        assert!(!core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_detects_switch_to_alt() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.mode_actions.push(1); // SWITCH_TO_ALT
        assert!(core.has_pending_buffer_switch());
    }

    #[test]
    fn test_has_pending_buffer_switch_detects_switch_to_main() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.mode_actions.push(3); // SWITCH_TO_MAIN
        assert!(core.has_pending_buffer_switch());
    }

    // ── SGR combined RGB through full parse pipeline ──────

    #[test]
    fn test_process_pty_data_sgr_combined_rgb_fg_bg() {
        // Full pipeline test: raw bytes → parser → CSI dispatch → SGR handler.
        // ESC[38;2;200;200;200;48;2;43;48;59m = 10 SGR params
        // Then print 'X' to commit cursor attrs to a cell.
        let mut core = TerminalCore::new(80, 24, 0);
        let data = b"\x1b[38;2;200;200;200;48;2;43;48;59mX";
        let consumed = core.process_pty_data(data);
        assert_eq!(consumed, data.len());
        // Cell at (0,0) should have the correct colors
        let fg = PackedColor::from_u32(core.get_cell_fg(0, 0));
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(fg, PackedColor::rgb(200, 200, 200));
        assert_eq!(
            bg,
            PackedColor::rgb(43, 48, 59),
            "bg should be rgb(43,48,59), not indexed(3)"
        );
    }

    // ── Grapheme buffer flush on non-Print dispatch ──────

    #[test]
    fn test_grapheme_buffer_flushed_before_csi_cursor_move() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Print emoji (gets buffered as Extended_Pictographic)
        // then CSI CUP to move cursor, then print 'A'
        // Emoji should be at position (0,0), not at the CUP destination
        let data = b"\xF0\x9F\x98\x80\x1B[3;5HA"; // 😀 \x1b[3;5H A
        core.process_pty_data(data);
        // 😀 should be at (0, 0)
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        // 'A' should be at (4, 2) [CUP row=3 col=5 → 0-indexed (2, 4)]
        assert_eq!(core.get_cell_char(4, 2), "A");
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_execute_cr() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Move cursor to col 10 first, print emoji then CR
        // Emoji should be at col 10 (flushed before CR), not lost
        let data = b"\x1B[1;11H\xF0\x9F\x98\x80\r"; // CUP(1,11) 😀 CR
        core.process_pty_data(data);
        // 😀 should be at (10, 0) with width 2
        assert_eq!(core.get_cell_char(10, 0), "😀");
        assert_eq!(core.get_cell_width(10, 0), 2);
        // After CR, cursor should be at col 0
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_execute_lf() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Print emoji then LF then 'A'
        let data = b"\xF0\x9F\x98\x80\nA"; // 😀 LF A
        core.process_pty_data(data);
        // 😀 should be at (0, 0), width 2
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        // After LF, cursor moves to row 1 (col stays at 2 from emoji advance)
        // 'A' should be at (2, 1)
        assert_eq!(core.get_cell_char(2, 1), "A");
    }

    #[test]
    fn test_grapheme_buffer_flushed_before_esc_dispatch() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Move to row 1 first so ESC M (Reverse Index) goes to row 0
        // Print emoji at row 1, then ESC M
        let data = b"\x1B[2;1H\xF0\x9F\x98\x80\x1BM"; // CUP(2,1) 😀 ESC_M
        core.process_pty_data(data);
        // 😀 should be at (0, 1) — row 1, col 0
        assert_eq!(core.get_cell_char(0, 1), "😀");
        assert_eq!(core.get_cell_width(0, 1), 2);
        // After ESC M (reverse index), cursor should be at row 0
        assert_eq!(core.get_cursor_row(), 0);
    }

    // ── DEC mode 1048 immediate save/restore ──────────────

    #[test]
    fn test_dec_1048_save_restore_immediate_in_data_stream() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Write "AB" at (0,0), save cursor (CSI ?1048h), move to (10,5),
        // write "CD", restore cursor (CSI ?1048l), write "EF"
        // "EF" should appear at (2,0) (where cursor was saved), not at (12,5)
        let data = b"AB\x1B[?1048h\x1B[6;11HCD\x1B[?1048lEF";
        core.process_pty_data(data);
        // "AB" at (0,0) and (1,0)
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(1, 0), "B");
        // "CD" at (10,5) and (11,5)
        assert_eq!(core.get_cell_char(10, 5), "C");
        assert_eq!(core.get_cell_char(11, 5), "D");
        // "EF" at (2,0) and (3,0) (restored cursor position)
        assert_eq!(core.get_cell_char(2, 0), "E");
        assert_eq!(core.get_cell_char(3, 0), "F");
        // No mode actions should be queued (handled immediately)
        assert!(core.mode_actions.is_empty());
    }

    #[test]
    fn test_dec_1048_and_esc7_share_same_saved_cursor() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Save with ESC 7 at (5,3), move, restore with CSI ?1048l
        // They should share the same saved cursor slot
        core.set_cursor(5, 3);
        let data = b"\x1B7\x1B[10;20HX\x1B[?1048l";
        core.process_pty_data(data);
        // Cursor should be restored to (5,3) from ESC 7 save
        assert_eq!(core.get_cursor_col(), 5);
        assert_eq!(core.get_cursor_row(), 3);
    }

    // ── Cell size propagation tests ──────────────────────

    #[test]
    fn test_cell_size_defaults() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.get_cell_width_px(), 8);
        assert_eq!(core.get_cell_height_px(), 16);
    }

    #[test]
    fn test_cell_size_preserved_after_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        core.reset();
        // Cell size is not reset (app-managed, not terminal state)
        assert_eq!(core.get_cell_width_px(), 10);
        assert_eq!(core.get_cell_height_px(), 20);
    }

    #[test]
    fn test_xtwinops_cell_size_after_buffer_switch_defaults() {
        // Simulates the problem: a new alternate core starts with default 8x16
        // CSI 16t should return the default before cell size is set
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;16;8t");

        // After setting cell size, CSI 16t should return the new values
        core.set_cell_size_px(10, 20);
        core.handle_xtwinops_cell_size();
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;20;10t");
    }

    // ── BCE (Background Color Erase) tests ──────────────

    /// Helper: set cursor bg to green (indexed color 2)
    fn set_cursor_bg_green(core: &mut TerminalCore) {
        core.set_cursor_bg(1, 2, 0, 0); // tag=1 (indexed), index=2 (green)
    }

    #[test]
    fn test_bce_clear_line() {
        let mut core = TerminalCore::new(10, 3, 0);
        set_cursor_bg_green(&mut core);
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
    }

    #[test]
    fn test_bce_clear_line_range() {
        let mut core = TerminalCore::new(10, 3, 0);
        set_cursor_bg_green(&mut core);
        core.clear_line_range(0, 3, 7);
        for col in 3..7 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::indexed(2),
                "col {col} should have green bg"
            );
        }
        // Cols outside range should still be default
        let bg0 = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg0, PackedColor::DEFAULT);
        let bg9 = PackedColor::from_u32(core.get_cell_bg(9, 0));
        assert_eq!(bg9, PackedColor::DEFAULT);
    }

    #[test]
    fn test_bce_default_bg_unchanged() {
        // When cursor.bg is DEFAULT, erased cells should have DEFAULT bg
        let mut core = TerminalCore::new(10, 3, 0);
        // cursor.bg is already DEFAULT
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(bg, PackedColor::DEFAULT);
        }
    }

    #[test]
    fn test_bce_sgr_reset_then_erase() {
        let mut core = TerminalCore::new(10, 3, 0);
        // Set green bg
        set_cursor_bg_green(&mut core);
        // Reset cursor attrs (simulates ESC[0m)
        core.reset_cursor_attrs();
        core.clear_line(0);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(
                bg,
                PackedColor::DEFAULT,
                "After reset, bg should be DEFAULT"
            );
        }
    }

    #[test]
    fn test_bce_256_color() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor_bg(1, 196, 0, 0); // indexed color 196
        core.clear_line(0);
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg, PackedColor::indexed(196));
    }

    #[test]
    fn test_bce_rgb_color() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cursor_bg(2, 100, 200, 50); // RGB
        core.clear_line(0);
        let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
        assert_eq!(bg, PackedColor::rgb(100, 200, 50));
    }

    #[test]
    fn test_bce_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        set_cursor_bg_green(&mut core);
        core.shift_rows_up(0, 4, 2);
        // Vacated bottom rows (3, 4) should have green bg
        for row in 3..5 {
            for col in 0..10 {
                let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
                assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
            }
        }
    }

    #[test]
    fn test_bce_shift_rows_down() {
        let mut core = TerminalCore::new(10, 5, 0);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        set_cursor_bg_green(&mut core);
        core.shift_rows_down(0, 4, 2);
        // Vacated top rows (0, 1) should have green bg
        for row in 0..2 {
            for col in 0..10 {
                let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
                assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
            }
        }
    }
}
