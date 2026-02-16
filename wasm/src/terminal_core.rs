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
    fn new() -> Self {
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
    // Sprint 2: Print handler state
    pub(crate) grapheme_buffer: Vec<u32>,
    pub(crate) wrap_pending: bool,
    pub(crate) g0_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) g1_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    pub(crate) active_charset: u8, // 0=G0, 1=G1
    pub(crate) scroll_region_top: u16,
    pub(crate) scroll_region_bottom: u16,
    // Sprint 4: Device response buffer
    pub(crate) response_buffer: [u8; 64],
    pub(crate) response_len: u8,
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
            // Sprint 2
            grapheme_buffer: Vec::with_capacity(8),
            wrap_pending: false,
            g0_charset: 0,
            g1_charset: 0,
            active_charset: 0,
            scroll_region_top: 0,
            scroll_region_bottom: rows.saturating_sub(1),
            // Sprint 4
            response_buffer: [0u8; 64],
            response_len: 0,
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

    // ── Cell access (internal) ───────────────────────────

    pub(crate) fn cell_index(&self, col: u16, row: u16) -> Option<usize> {
        self.viewport_cell_offset(col, row)
    }

    // ── Cell write ───────────────────────────────────────

    pub fn set_cell(
        &mut self,
        col: u16,
        row: u16,
        char_str: &str,
        width: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        if let Some(idx) = self.cell_index(col, row) {
            let abs = self.viewport_abs(row) as u16;
            let cell = &mut self.ring_cells[idx];
            cell.set_char(char_str);
            if cell.is_overflow() {
                self.overflow.insert((col, abs), char_str.to_string());
            } else {
                self.overflow.remove(&(col, abs));
            }
            cell.width = width;
            cell.fg = PackedColor {
                tag: fg_tag,
                r: fg_r,
                g: fg_g,
                b: fg_b,
            };
            cell.bg = PackedColor {
                tag: bg_tag,
                r: bg_r,
                g: bg_g,
                b: bg_b,
            };
            cell.flags = flags;
            self.mark_row_dirty(row);
        }
    }

    pub fn set_cell_ascii(
        &mut self,
        col: u16,
        row: u16,
        byte: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &mut self.ring_cells[idx];
            cell.char_data[0] = byte;
            for b in &mut cell.char_data[1..] {
                *b = 0;
            }
            cell.char_len = 1;
            cell.width = 1;
            cell.fg = PackedColor {
                tag: fg_tag,
                r: fg_r,
                g: fg_g,
                b: fg_b,
            };
            cell.bg = PackedColor {
                tag: bg_tag,
                r: bg_r,
                g: bg_g,
                b: bg_b,
            };
            cell.flags = flags;
            let abs = self.viewport_abs(row) as u16;
            self.overflow.remove(&(col, abs));
            self.mark_row_dirty(row);
        }
    }

    // ── Cell read ────────────────────────────────────────

    pub fn get_cell_char(&self, col: u16, row: u16) -> String {
        if let Some(idx) = self.cell_index(col, row) {
            let abs = self.viewport_abs(row) as u16;
            let cell = &self.ring_cells[idx];
            if cell.is_overflow() {
                self.overflow.get(&(col, abs)).cloned().unwrap_or_default()
            } else {
                cell.get_char_inline().unwrap_or(" ").to_string()
            }
        } else {
            " ".to_string()
        }
    }

    pub fn get_cell_width(&self, col: u16, row: u16) -> u8 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].width)
            .unwrap_or(1)
    }

    pub fn get_cell_fg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].fg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_bg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].bg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_flags(&self, col: u16, row: u16) -> u16 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].flags)
            .unwrap_or(0)
    }

    // ── Batch cell read ──────────────────────────────────

    pub fn get_row_packed(&self, row: u16) -> Vec<u8> {
        if row >= self.rows {
            return Vec::new();
        }
        let abs = self.viewport_abs(row);
        self.pack_row_abs(abs)
    }

    // ── Line operations ──────────────────────────────────

    pub fn clear_line(&mut self, row: u16) {
        if row >= self.rows {
            return;
        }
        let abs = self.viewport_abs(row);
        let base = abs * self.cols as usize;
        for i in base..base + self.cols as usize {
            self.ring_cells[i] = Cell::EMPTY;
        }
        self.ring_wrapped[abs] = false;
        overflow_clear_row(&mut self.overflow, abs as u16);
        self.mark_row_dirty(row);
    }

    pub fn clear_line_range(&mut self, row: u16, start_col: u16, end_col: u16) {
        if row >= self.rows {
            return;
        }
        let start = start_col.min(self.cols) as usize;
        let end = end_col.min(self.cols) as usize;
        let abs = self.viewport_abs(row);
        let base = abs * self.cols as usize;
        for i in base + start..base + end {
            self.ring_cells[i] = Cell::EMPTY;
        }
        overflow_clear_range(&mut self.overflow, abs as u16, start_col, end_col);
        self.mark_row_dirty(row);
    }

    pub fn get_line_text(&self, row: u16) -> String {
        if row >= self.rows {
            return String::new();
        }
        let abs = self.viewport_abs(row);
        self.line_text_abs(abs)
    }

    pub fn is_line_empty(&self, row: u16) -> bool {
        if row >= self.rows {
            return true;
        }
        let base = self.viewport_row_base(row);
        for col in 0..self.cols as usize {
            let cell = &self.ring_cells[base + col];
            if cell.width > 0 {
                if cell.is_overflow() {
                    return false;
                }
                if let Some(s) = cell.get_char_inline() {
                    if s != " " {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn get_line_wrapped(&self, row: u16) -> bool {
        if row < self.rows {
            self.ring_wrapped[self.viewport_abs(row)]
        } else {
            false
        }
    }

    pub fn set_line_wrapped(&mut self, row: u16, wrapped: bool) {
        if row < self.rows {
            let abs = self.viewport_abs(row);
            self.ring_wrapped[abs] = wrapped;
        }
    }

    // ── Row operations (for scroll) ──────────────────────

    pub fn shift_rows_up(&mut self, start_row: u16, end_row: u16, count: u16) {
        if count == 0 || start_row >= self.rows || end_row >= self.rows || start_row > end_row {
            return;
        }
        let count = count.min(end_row - start_row + 1);
        let cols = self.cols as usize;

        // Clear overflow for rows that will be overwritten (deleted range)
        for r in start_row..start_row + count {
            let abs = self.viewport_abs(r) as u16;
            overflow_clear_row(&mut self.overflow, abs);
        }

        // Move row data
        for dst_row in start_row..=end_row.saturating_sub(count) {
            let src_row = dst_row + count;
            if src_row <= end_row {
                let dst_abs = self.viewport_abs(dst_row);
                let src_abs = self.viewport_abs(src_row);
                let dst_base = dst_abs * cols;
                let src_base = src_abs * cols;
                for i in 0..cols {
                    self.ring_cells[dst_base + i] = self.ring_cells[src_base + i];
                }
                self.ring_wrapped[dst_abs] = self.ring_wrapped[src_abs];
                // Move overflow entries from source to destination
                let src_abs_u16 = src_abs as u16;
                let dst_abs_u16 = dst_abs as u16;
                let entries: Vec<(u16, String)> = self
                    .overflow
                    .iter()
                    .filter(|&(&(_, r), _)| r == src_abs_u16)
                    .map(|(&(c, _), v)| (c, v.clone()))
                    .collect();
                self.overflow.retain(|&(_, r), _| r != src_abs_u16);
                for (c, v) in entries {
                    self.overflow.insert((c, dst_abs_u16), v);
                }
            }
        }
        // Clear vacated rows at bottom
        for row in (end_row + 1 - count)..=end_row {
            let abs = self.viewport_abs(row);
            let base = abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = Cell::EMPTY;
            }
            self.ring_wrapped[abs] = false;
            overflow_clear_row(&mut self.overflow, abs as u16);
        }

        // Mark all affected rows dirty
        for row in start_row..=end_row {
            self.mark_row_dirty(row);
        }
    }

    pub fn shift_rows_down(&mut self, start_row: u16, end_row: u16, count: u16) {
        if count == 0 || start_row >= self.rows || end_row >= self.rows || start_row > end_row {
            return;
        }
        let count = count.min(end_row - start_row + 1);
        let cols = self.cols as usize;

        // Clear overflow for rows that will be overwritten (bottom range)
        for r in (end_row + 1 - count)..=end_row {
            let abs = self.viewport_abs(r) as u16;
            overflow_clear_row(&mut self.overflow, abs);
        }

        // Move row data (iterate in reverse)
        for dst_row in (start_row + count..=end_row).rev() {
            let src_row = dst_row - count;
            let dst_abs = self.viewport_abs(dst_row);
            let src_abs = self.viewport_abs(src_row);
            let dst_base = dst_abs * cols;
            let src_base = src_abs * cols;
            for i in 0..cols {
                self.ring_cells[dst_base + i] = self.ring_cells[src_base + i];
            }
            self.ring_wrapped[dst_abs] = self.ring_wrapped[src_abs];
            // Move overflow entries from source to destination
            let src_abs_u16 = src_abs as u16;
            let dst_abs_u16 = dst_abs as u16;
            let entries: Vec<(u16, String)> = self
                .overflow
                .iter()
                .filter(|&(&(_, r), _)| r == src_abs_u16)
                .map(|(&(c, _), v)| (c, v.clone()))
                .collect();
            self.overflow.retain(|&(_, r), _| r != src_abs_u16);
            for (c, v) in entries {
                self.overflow.insert((c, dst_abs_u16), v);
            }
        }
        // Clear vacated rows at top
        for row in start_row..start_row + count {
            let abs = self.viewport_abs(row);
            let base = abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = Cell::EMPTY;
            }
            self.ring_wrapped[abs] = false;
            overflow_clear_row(&mut self.overflow, abs as u16);
        }

        // Mark all affected rows dirty
        for row in start_row..=end_row {
            self.mark_row_dirty(row);
        }
    }

    pub fn copy_row(&mut self, src_row: u16, dst_row: u16) {
        if src_row >= self.rows || dst_row >= self.rows || src_row == dst_row {
            return;
        }
        let cols = self.cols as usize;
        let src_abs = self.viewport_abs(src_row);
        let dst_abs = self.viewport_abs(dst_row);
        let src_base = src_abs * cols;
        let dst_base = dst_abs * cols;
        for i in 0..cols {
            self.ring_cells[dst_base + i] = self.ring_cells[src_base + i];
        }
        self.ring_wrapped[dst_abs] = self.ring_wrapped[src_abs];

        // Copy overflow entries
        let dst_abs_u16 = dst_abs as u16;
        let src_abs_u16 = src_abs as u16;
        overflow_clear_row(&mut self.overflow, dst_abs_u16);
        let src_entries: Vec<(u16, String)> = self
            .overflow
            .iter()
            .filter(|&(&(_, r), _)| r == src_abs_u16)
            .map(|(&(c, _), v)| (c, v.clone()))
            .collect();
        for (c, v) in src_entries {
            self.overflow.insert((c, dst_abs_u16), v);
        }

        self.mark_row_dirty(dst_row);
    }

    pub fn fill_row_default(&mut self, row: u16) {
        self.clear_line(row);
    }

    // ── Resize ───────────────────────────────────────────

    /// Legacy resize (delegates to resize_reflow with scrollback_lines=0).
    /// Kept for backward compatibility with existing tests.
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        // Use reflow with current scrollback capacity
        let scrollback_lines = self.ring_capacity.saturating_sub(self.rows as usize) as u32;
        self.resize_reflow(new_cols, new_rows, scrollback_lines);
    }

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
        self.mark_all_dirty();
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

    // ── Cursor ───────────────────────────────────────────

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
}
