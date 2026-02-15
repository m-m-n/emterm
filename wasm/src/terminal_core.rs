/// TerminalCore: viewport grid and terminal state in WASM linear memory.
///
/// Owns the viewport grid (rows × cols cells), cursor state, terminal modes,
/// tab stops, and dirty row tracking. Exported via wasm_bindgen for JS access.
use wasm_bindgen::prelude::*;

use crate::cell::*;

// ── Sentinel constants ───────────────────────────────────

const BEL_SENTINEL: u8 = 0xFE;
const SCROLLBACK_SENTINEL: u8 = 0xFF;

// ── Sprint 4: Mode action code constants ─────────────────
const MODE_ACTION_NONE: u8 = 0;
const MODE_ACTION_SWITCH_TO_ALT: u8 = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT: u8 = 2;
const MODE_ACTION_SWITCH_TO_MAIN: u8 = 3;
const MODE_ACTION_SAVE_CURSOR: u8 = 4;
const MODE_ACTION_RESTORE_CURSOR: u8 = 5;
const MODE_ACTION_TS_FALLBACK: u8 = 0xFF;

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
pub struct CursorState {
    pub col: u16,
    pub row: u16,
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: u16,
    pub visible: bool,
    pub style: u8, // 0=block, 1=underline, 2=bar
    pub blink: bool,
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
        }
    }
}

// ── TerminalCore ─────────────────────────────────────────

#[wasm_bindgen]
pub struct TerminalCore {
    cols: u16,
    rows: u16,
    grid: Vec<Cell>,
    wrapped: Vec<bool>,
    dirty: Vec<u64>,
    cursor: CursorState,
    saved_cursor: Option<CursorState>,
    modes: u32,
    tab_stops: Vec<bool>,
    overflow: OverflowTable,
    // Sprint 2: Print handler state
    grapheme_buffer: Vec<u32>,
    wrap_pending: bool,
    g0_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    g1_charset: u8,     // 0=Ascii, 1=DecLineDrawing
    active_charset: u8, // 0=G0, 1=G1
    scroll_region_top: u16,
    scroll_region_bottom: u16,
    // Sprint 4: Device response buffer
    response_buffer: [u8; 64],
    response_len: u8,
}

#[wasm_bindgen]
impl TerminalCore {
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u16, rows: u16) -> Self {
        debug_assert!(cols > 0 && rows > 0, "cols and rows must be > 0");
        let total = cols as usize * rows as usize;
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
            grid: vec![Cell::EMPTY; total],
            wrapped: vec![false; rows as usize],
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

    fn cell_index(&self, col: u16, row: u16) -> Option<usize> {
        if col < self.cols && row < self.rows {
            Some(row as usize * self.cols as usize + col as usize)
        } else {
            None
        }
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
            let cell = &mut self.grid[idx];
            cell.set_char(char_str);
            if cell.is_overflow() {
                self.overflow.insert((col, row), char_str.to_string());
            } else {
                self.overflow.remove(&(col, row));
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
            let cell = &mut self.grid[idx];
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
            self.overflow.remove(&(col, row));
            self.mark_row_dirty(row);
        }
    }

    // ── Cell read ────────────────────────────────────────

    pub fn get_cell_char(&self, col: u16, row: u16) -> String {
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &self.grid[idx];
            if cell.is_overflow() {
                self.overflow.get(&(col, row)).cloned().unwrap_or_default()
            } else {
                cell.get_char_inline().unwrap_or(" ").to_string()
            }
        } else {
            " ".to_string()
        }
    }

    pub fn get_cell_width(&self, col: u16, row: u16) -> u8 {
        self.cell_index(col, row)
            .map(|i| self.grid[i].width)
            .unwrap_or(1)
    }

    pub fn get_cell_fg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.grid[i].fg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_bg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.grid[i].bg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_flags(&self, col: u16, row: u16) -> u16 {
        self.cell_index(col, row)
            .map(|i| self.grid[i].flags)
            .unwrap_or(0)
    }

    // ── Batch cell read ──────────────────────────────────

    pub fn get_row_packed(&self, row: u16) -> Vec<u8> {
        if row >= self.rows {
            return Vec::new();
        }
        // Estimate capacity: ~12 bytes per cell for ASCII (common case)
        let mut buf = Vec::with_capacity(self.cols as usize * 12);
        let base = row as usize * self.cols as usize;

        for col in 0..self.cols {
            let cell = &self.grid[base + col as usize];
            if cell.is_overflow() {
                let s = self
                    .overflow
                    .get(&(col, row))
                    .map(|s| s.as_bytes())
                    .unwrap_or(b" ");
                let len = s.len();
                buf.push(0xFF); // overflow marker
                buf.push((len >> 8) as u8);
                buf.push(len as u8);
                buf.extend_from_slice(s);
            } else {
                let len = cell.char_len;
                buf.push(len);
                buf.extend_from_slice(&cell.char_data[..len as usize]);
            }
            buf.push(cell.width);
            // fg: 4 bytes
            buf.push(cell.fg.tag);
            buf.push(cell.fg.r);
            buf.push(cell.fg.g);
            buf.push(cell.fg.b);
            // bg: 4 bytes
            buf.push(cell.bg.tag);
            buf.push(cell.bg.r);
            buf.push(cell.bg.g);
            buf.push(cell.bg.b);
            // flags: 2 bytes (little-endian)
            buf.push(cell.flags as u8);
            buf.push((cell.flags >> 8) as u8);
        }
        buf
    }

    // ── Line operations ──────────────────────────────────

    pub fn clear_line(&mut self, row: u16) {
        if row >= self.rows {
            return;
        }
        let base = row as usize * self.cols as usize;
        for i in base..base + self.cols as usize {
            self.grid[i] = Cell::EMPTY;
        }
        self.wrapped[row as usize] = false;
        overflow_clear_row(&mut self.overflow, row);
        self.mark_row_dirty(row);
    }

    pub fn clear_line_range(&mut self, row: u16, start_col: u16, end_col: u16) {
        if row >= self.rows {
            return;
        }
        let start = start_col.min(self.cols) as usize;
        let end = end_col.min(self.cols) as usize;
        let base = row as usize * self.cols as usize;
        for i in base + start..base + end {
            self.grid[i] = Cell::EMPTY;
        }
        overflow_clear_range(&mut self.overflow, row, start_col, end_col);
        self.mark_row_dirty(row);
    }

    pub fn get_line_text(&self, row: u16) -> String {
        if row >= self.rows {
            return String::new();
        }
        let mut text = String::new();
        let base = row as usize * self.cols as usize;
        for col in 0..self.cols {
            let cell = &self.grid[base + col as usize];
            if cell.width > 0 {
                if cell.is_overflow() {
                    if let Some(s) = self.overflow.get(&(col, row)) {
                        text.push_str(s);
                    }
                } else if let Some(s) = cell.get_char_inline() {
                    text.push_str(s);
                }
            }
        }
        text
    }

    pub fn is_line_empty(&self, row: u16) -> bool {
        if row >= self.rows {
            return true;
        }
        let base = row as usize * self.cols as usize;
        for col in 0..self.cols as usize {
            let cell = &self.grid[base + col];
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
            self.wrapped[row as usize]
        } else {
            false
        }
    }

    pub fn set_line_wrapped(&mut self, row: u16, wrapped: bool) {
        if row < self.rows {
            self.wrapped[row as usize] = wrapped;
        }
    }

    // ── Row operations (for scroll) ──────────────────────

    pub fn shift_rows_up(&mut self, start_row: u16, end_row: u16, count: u16) {
        if count == 0 || start_row >= self.rows || end_row >= self.rows || start_row > end_row {
            return;
        }
        let count = count.min(end_row - start_row + 1);
        let cols = self.cols as usize;

        // Move row data
        for dst_row in start_row..=end_row.saturating_sub(count) {
            let src_row = dst_row + count;
            if src_row <= end_row {
                let dst_base = dst_row as usize * cols;
                let src_base = src_row as usize * cols;
                for i in 0..cols {
                    self.grid[dst_base + i] = self.grid[src_base + i];
                }
                self.wrapped[dst_row as usize] = self.wrapped[src_row as usize];
            }
        }
        // Clear vacated rows at bottom
        for row in (end_row + 1 - count)..=end_row {
            let base = row as usize * cols;
            for i in base..base + cols {
                self.grid[i] = Cell::EMPTY;
            }
            self.wrapped[row as usize] = false;
        }

        // Remap overflow side table
        overflow_shift_up(&mut self.overflow, start_row, end_row, count);

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

        // Move row data (iterate in reverse)
        for dst_row in (start_row + count..=end_row).rev() {
            let src_row = dst_row - count;
            let dst_base = dst_row as usize * cols;
            let src_base = src_row as usize * cols;
            for i in 0..cols {
                self.grid[dst_base + i] = self.grid[src_base + i];
            }
            self.wrapped[dst_row as usize] = self.wrapped[src_row as usize];
        }
        // Clear vacated rows at top
        for row in start_row..start_row + count {
            let base = row as usize * cols;
            for i in base..base + cols {
                self.grid[i] = Cell::EMPTY;
            }
            self.wrapped[row as usize] = false;
        }

        overflow_shift_down(&mut self.overflow, start_row, end_row, count);

        for row in start_row..=end_row {
            self.mark_row_dirty(row);
        }
    }

    pub fn copy_row(&mut self, src_row: u16, dst_row: u16) {
        if src_row >= self.rows || dst_row >= self.rows || src_row == dst_row {
            return;
        }
        let cols = self.cols as usize;
        let src_base = src_row as usize * cols;
        let dst_base = dst_row as usize * cols;
        for i in 0..cols {
            self.grid[dst_base + i] = self.grid[src_base + i];
        }
        self.wrapped[dst_row as usize] = self.wrapped[src_row as usize];

        // Copy overflow entries
        overflow_clear_row(&mut self.overflow, dst_row);
        let src_entries: Vec<(u16, String)> = self
            .overflow
            .iter()
            .filter(|&(&(_, r), _)| r == src_row)
            .map(|(&(c, _), v)| (c, v.clone()))
            .collect();
        for (c, v) in src_entries {
            self.overflow.insert((c, dst_row), v);
        }

        self.mark_row_dirty(dst_row);
    }

    pub fn fill_row_default(&mut self, row: u16) {
        self.clear_line(row);
    }

    // ── Resize ───────────────────────────────────────────

    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        debug_assert!(new_cols > 0 && new_rows > 0);
        let old_cols = self.cols;
        let old_rows = self.rows;

        let mut new_grid = vec![Cell::EMPTY; new_cols as usize * new_rows as usize];
        let copy_rows = old_rows.min(new_rows);
        let copy_cols = old_cols.min(new_cols);

        for row in 0..copy_rows as usize {
            let old_base = row * old_cols as usize;
            let new_base = row * new_cols as usize;
            for col in 0..copy_cols as usize {
                new_grid[new_base + col] = self.grid[old_base + col];
            }
        }

        self.grid = new_grid;

        // Resize wrapped
        self.wrapped.resize(new_rows as usize, false);

        // Resize tab stops
        self.tab_stops.resize(new_cols as usize, false);
        if new_cols > old_cols {
            for i in (old_cols as usize..new_cols as usize).step_by(8) {
                if !self.tab_stops[i] {
                    self.tab_stops[i] = true;
                }
            }
        }

        self.cols = new_cols;
        self.rows = new_rows;

        // Clamp cursor
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));

        // Resize dirty bitset and mark all dirty
        let dirty_words = (new_rows as usize + 63) / 64;
        self.dirty = vec![0; dirty_words];
        self.mark_all_dirty();

        // Clean up overflow
        overflow_resize(&mut self.overflow, new_cols, new_rows);

        // Sprint 2: reset print state on resize
        self.scroll_region_top = 0;
        self.scroll_region_bottom = new_rows.saturating_sub(1);
        self.wrap_pending = false;
        self.grapheme_buffer.clear();
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
        self.saved_cursor = Some(self.cursor.clone());
    }

    pub fn restore_cursor(&mut self) {
        if let Some(saved) = self.saved_cursor.take() {
            self.cursor = saved;
            // Clamp to current bounds
            self.cursor.col = self.cursor.col.min(self.cols.saturating_sub(1));
            self.cursor.row = self.cursor.row.min(self.rows.saturating_sub(1));
        } else {
            // Reset to defaults if no saved state
            self.cursor = CursorState::new();
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
        let total = self.cols as usize * self.rows as usize;
        self.grid = vec![Cell::EMPTY; total];
        self.wrapped = vec![false; self.rows as usize];
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

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    /// Advance cursor row. Returns true if scroll is needed
    /// (cursor at scroll_region_bottom).
    fn line_feed(&mut self) -> bool {
        if self.cursor.row >= self.scroll_region_bottom {
            true
        } else {
            self.cursor.row += 1;
            false
        }
    }

    /// Apply active charset translation to a codepoint.
    fn translate_charset(&self, cp: u32) -> u32 {
        let charset = if self.active_charset == 0 {
            self.g0_charset
        } else {
            self.g1_charset
        };
        if charset == 1 {
            Self::translate_line_drawing(cp)
        } else {
            cp
        }
    }

    /// DEC Line Drawing translation table (0x5F-0x7E, 32 entries).
    fn translate_line_drawing(cp: u32) -> u32 {
        match cp {
            0x5F => 0x0020, // _ → Blank
            0x60 => 0x25C6, // ` → Diamond
            0x61 => 0x2592, // a → Checkerboard
            0x62 => 0x2409, // b → HT
            0x63 => 0x240C, // c → FF
            0x64 => 0x240D, // d → CR
            0x65 => 0x240A, // e → LF
            0x66 => 0x00B0, // f → Degree
            0x67 => 0x00B1, // g → Plus/minus
            0x68 => 0x2424, // h → NL
            0x69 => 0x240B, // i → VT
            0x6A => 0x2518, // j → Lower right corner
            0x6B => 0x2510, // k → Upper right corner
            0x6C => 0x250C, // l → Upper left corner
            0x6D => 0x2514, // m → Lower left corner
            0x6E => 0x253C, // n → Crossing lines
            0x6F => 0x23BA, // o → Scan 1
            0x70 => 0x23BB, // p → Scan 3
            0x71 => 0x2500, // q → Scan 5 (horizontal line)
            0x72 => 0x23BC, // r → Scan 7
            0x73 => 0x23BD, // s → Scan 9
            0x74 => 0x251C, // t → Left tee
            0x75 => 0x2524, // u → Right tee
            0x76 => 0x2534, // v → Bottom tee
            0x77 => 0x252C, // w → Top tee
            0x78 => 0x2502, // x → Vertical line
            0x79 => 0x2264, // y → Less than or equal
            0x7A => 0x2265, // z → Greater than or equal
            0x7B => 0x03C0, // { → Pi
            0x7C => 0x2260, // | → Not equal
            0x7D => 0x00A3, // } → UK pound
            0x7E => 0x00B7, // ~ → Bullet
            _ => cp,
        }
    }

    /// Write a character/grapheme to grid at cursor, handling wrap and scroll.
    /// Returns scroll count.
    fn write_grapheme_to_grid(&mut self, char_str: &str, width: u8) -> u8 {
        let mut scroll_count: u8 = 0;

        // Handle wrap_pending
        if self.wrap_pending {
            self.wrap_pending = false;
            self.carriage_return();
            if self.line_feed() {
                scroll_count += 1;
            }
            self.wrapped[self.cursor.row as usize] = true;
        }

        // Wide char at line end: wrap before printing
        if width == 2 && self.cursor.col >= self.cols.saturating_sub(1) {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.carriage_return();
                if self.line_feed() {
                    scroll_count += 1;
                }
                self.wrapped[self.cursor.row as usize] = true;
            }
        }

        // Write cell
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &mut self.grid[idx];
            cell.set_char(char_str);
            if cell.is_overflow() {
                self.overflow.insert((col, row), char_str.to_string());
            } else {
                self.overflow.remove(&(col, row));
            }
            cell.width = width;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            self.mark_row_dirty(row);
        }

        // Placeholder for width-2 characters
        if width == 2 && col + 1 < self.cols {
            if let Some(idx) = self.cell_index(col + 1, row) {
                let ph = &mut self.grid[idx];
                ph.char_data = [0; 16];
                ph.char_len = 0;
                ph.width = 0;
                ph.fg = self.cursor.fg;
                ph.bg = self.cursor.bg;
                ph.flags = self.cursor.flags;
                self.overflow.remove(&(col + 1, row));
            }
        }

        // Advance cursor
        let new_col = col as u32 + width as u32;
        if new_col >= self.cols as u32 {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.cursor.col = self.cols - 1;
                self.wrap_pending = true;
            }
        } else {
            self.cursor.col = new_col as u16;
        }

        scroll_count
    }

    /// ASCII fast path: direct byte write without string allocation.
    fn handle_print_ascii(&mut self, cp: u32) -> u8 {
        let byte = cp as u8;
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &mut self.grid[idx];
            cell.char_data[0] = byte;
            for b in &mut cell.char_data[1..] {
                *b = 0;
            }
            cell.char_len = 1;
            cell.width = 1;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            // Always remove overflow entry (char_len is already set to 1 above,
            // so is_overflow() would be false -- must remove unconditionally)
            self.overflow.remove(&(col, row));
            self.mark_row_dirty(row);
        }

        let new_col = col + 1;
        if new_col < self.cols {
            self.cursor.col = new_col;
        } else if self.get_mode(MODE_AUTO_WRAP) {
            self.cursor.col = self.cols - 1;
            self.wrap_pending = true;
        }

        0
    }

    /// Slow path: handles charWidth, charset translation, wrap.
    fn handle_print_slow(&mut self, cp: u32) -> u8 {
        let width = crate::unicode::char_width(cp);
        let translated = self.translate_charset(cp);
        let mut buf = [0u8; 4];
        let ch = char::from_u32(translated).unwrap_or(' ');
        let s = ch.encode_utf8(&mut buf);
        self.write_grapheme_to_grid(s, width)
    }

    // ── Sprint 2: Public print API ──────────────────────

    /// Process a single codepoint for printing.
    /// Returns the number of scroll-up operations the caller should perform.
    pub fn handle_print(&mut self, cp: u32) -> u8 {
        let mut scroll_count: u8 = 0;

        // Safety: flush if buffer exceeds max size
        if self.grapheme_buffer.len() >= 64 {
            scroll_count += self.flush_grapheme_buffer();
        }

        let props = crate::unicode::classify_codepoint(cp);

        if !self.grapheme_buffer.is_empty() {
            // Buffer non-empty: check if cp extends the cluster
            if cp == 0x200D {
                self.grapheme_buffer.push(cp);
                return scroll_count;
            }
            if props & crate::unicode::VARIATION_SEL != 0 {
                self.grapheme_buffer.push(cp);
                return scroll_count;
            }
            if props & crate::unicode::SKIN_TONE != 0 {
                self.grapheme_buffer.push(cp);
                return scroll_count;
            }
            if props & crate::unicode::REGIONAL_IND != 0 {
                if self.grapheme_buffer.len() == 1 {
                    let buf0 = self.grapheme_buffer[0];
                    if (0x1F1E6..=0x1F1FF).contains(&buf0) {
                        self.grapheme_buffer.push(cp);
                        scroll_count += self.flush_grapheme_buffer();
                        return scroll_count;
                    }
                }
            }
            if let Some(&last) = self.grapheme_buffer.last() {
                if last == 0x200D && (props & crate::unicode::EXT_PICTOGRAPHIC != 0) {
                    self.grapheme_buffer.push(cp);
                    return scroll_count;
                }
            }
            if props & crate::unicode::COMBINING != 0 {
                self.grapheme_buffer.push(cp);
                return scroll_count;
            }

            // Does not extend: flush and fall through
            scroll_count += self.flush_grapheme_buffer();
        } else {
            // Buffer empty: check if cp starts buffering
            if props & (crate::unicode::EXT_PICTOGRAPHIC | crate::unicode::REGIONAL_IND) != 0 {
                self.grapheme_buffer.push(cp);
                return scroll_count;
            }
        }

        // ASCII fast path
        if cp >= 0x20
            && cp < 0x7F
            && !self.wrap_pending
            && self.active_charset == 0
            && self.g0_charset == 0
        {
            let new_col = self.cursor.col + 1;
            if new_col < self.cols || self.get_mode(MODE_AUTO_WRAP) {
                return scroll_count + self.handle_print_ascii(cp);
            }
        }

        // Slow path
        scroll_count + self.handle_print_slow(cp)
    }

    // ── Sprint 3: C0 Control Handler ─────────────────────

    /// Handle a C0 Execute action.
    /// Returns: scroll count (0-N for LF/VT/FF), BEL_SENTINEL (0xFE) for BEL.
    pub fn handle_execute(&mut self, byte: u8) -> u8 {
        match byte {
            0x07 => BEL_SENTINEL, // BEL
            0x08 => {
                // BS: decrement cursor.col, clamped to 0
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.wrap_pending = false;
                0
            }
            0x09 => {
                // HT: move to next tab stop
                self.cursor.col = self.find_next_tab_stop();
                self.wrap_pending = false;
                0
            }
            0x0A | 0x0B | 0x0C => {
                // LF/VT/FF
                self.execute_line_feed()
            }
            0x0D => {
                // CR
                self.cursor.col = 0;
                self.wrap_pending = false;
                0
            }
            0x0E => {
                // SO: switch to G1
                self.active_charset = 1;
                0
            }
            0x0F => {
                // SI: switch to G0
                self.active_charset = 0;
                0
            }
            _ => 0, // Unknown C0: no-op
        }
    }

    /// Find the next tab stop after the current cursor column.
    /// Returns the tab stop column, or cols-1 if no more stops.
    fn find_next_tab_stop(&self) -> u16 {
        self.next_tab_stop(self.cursor.col)
    }

    /// Execute LF: line_feed + clear wrap_pending.
    /// Returns 1 if scroll needed, 0 otherwise.
    fn execute_line_feed(&mut self) -> u8 {
        let scroll = if self.line_feed() { 1 } else { 0 };
        self.wrap_pending = false;
        scroll
    }

    // ── Sprint 3: CSI Cursor Handlers ───────────────────────

    /// Convert 1-indexed ANSI col parameter to 0-indexed, clamped.
    fn to_zero_indexed_col(&self, col: u16) -> u16 {
        if col == 0 {
            0
        } else {
            (col - 1).min(self.cols.saturating_sub(1))
        }
    }

    /// Convert 1-indexed ANSI row parameter to 0-indexed, clamped.
    fn to_zero_indexed_row(&self, row: u16) -> u16 {
        if row == 0 {
            0
        } else {
            (row - 1).min(self.rows.saturating_sub(1))
        }
    }

    /// CSI A - Cursor Up by count rows.
    pub fn handle_cursor_up(&mut self, count: u16) {
        self.cursor.row = self.cursor.row.saturating_sub(count);
        self.wrap_pending = false;
    }

    /// CSI B - Cursor Down by count rows.
    pub fn handle_cursor_down(&mut self, count: u16) {
        self.cursor.row = self
            .cursor
            .row
            .saturating_add(count)
            .min(self.rows.saturating_sub(1));
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
    pub fn handle_cursor_next_line(&mut self, count: u16) {
        self.cursor.row = self
            .cursor
            .row
            .saturating_add(count)
            .min(self.rows.saturating_sub(1));
        self.cursor.col = 0;
        self.wrap_pending = false;
    }

    /// CSI F - Cursor Previous Line (up + col=0).
    pub fn handle_cursor_previous_line(&mut self, count: u16) {
        self.cursor.row = self.cursor.row.saturating_sub(count);
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

    // ── Sprint 3: CSI Screen Handlers ───────────────────────

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
        let end = self.cursor.col.saturating_add(count).min(self.cols);
        self.clear_line_range(self.cursor.row, self.cursor.col, end);
    }

    /// Flush the grapheme buffer, writing the accumulated cluster to the grid.
    /// Returns the number of scroll-up operations the caller should perform.
    pub fn flush_grapheme_buffer(&mut self) -> u8 {
        if self.grapheme_buffer.is_empty() {
            return 0;
        }

        let mut cluster = String::with_capacity(self.grapheme_buffer.len() * 4);
        let mut has_fe0e = false;
        let mut has_fe0f = false;

        for &cp in &self.grapheme_buffer {
            if cp == 0xFE0E {
                has_fe0e = true;
            }
            if cp == 0xFE0F {
                has_fe0f = true;
            }
            if let Some(ch) = char::from_u32(cp) {
                cluster.push(ch);
            }
        }

        let width: u8 = if has_fe0e {
            1
        } else if has_fe0f {
            2
        } else if self.grapheme_buffer.len() == 1 {
            if crate::unicode::is_emoji_presentation(self.grapheme_buffer[0]) {
                2
            } else {
                crate::unicode::char_width(self.grapheme_buffer[0])
            }
        } else {
            2
        };

        self.grapheme_buffer.clear();
        self.write_grapheme_to_grid(&cluster, width)
    }

    // ── Sprint 4: SGR Handler ───────────────────────────────

    /// Handle SGR (Select Graphic Rendition) parameters.
    /// Parses the raw parameter array and applies attributes to cursor.
    pub fn handle_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            // Empty params = Reset
            self.cursor.fg = PackedColor::DEFAULT;
            self.cursor.bg = PackedColor::DEFAULT;
            self.cursor.flags = 0;
            return;
        }

        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => {
                    self.cursor.fg = PackedColor::DEFAULT;
                    self.cursor.bg = PackedColor::DEFAULT;
                    self.cursor.flags = 0;
                }
                1 => self.cursor.flags |= STYLE_BOLD,
                2 => self.cursor.flags |= STYLE_DIM,
                3 => self.cursor.flags |= STYLE_ITALIC,
                4 => self.cursor.flags |= STYLE_UNDERLINE,
                5 => self.cursor.flags |= STYLE_BLINK,
                7 => self.cursor.flags |= STYLE_REVERSE,
                8 => self.cursor.flags |= STYLE_HIDDEN,
                9 => self.cursor.flags |= STYLE_STRIKETHROUGH,
                22 => self.cursor.flags &= !(STYLE_BOLD | STYLE_DIM),
                23 => self.cursor.flags &= !STYLE_ITALIC,
                24 => self.cursor.flags &= !STYLE_UNDERLINE,
                25 => self.cursor.flags &= !STYLE_BLINK,
                27 => self.cursor.flags &= !STYLE_REVERSE,
                28 => self.cursor.flags &= !STYLE_HIDDEN,
                29 => self.cursor.flags &= !STYLE_STRIKETHROUGH,
                30..=37 => self.cursor.fg = PackedColor::indexed((p - 30) as u8),
                38 => {
                    // Extended foreground color
                    i += 1;
                    if i >= params.len() {
                        break;
                    }
                    match params[i] {
                        5 => {
                            // 38;5;n - Indexed color
                            i += 1;
                            if i < params.len() {
                                self.cursor.fg = PackedColor::indexed(params[i] as u8);
                            }
                        }
                        2 => {
                            // 38;2;r;g;b - RGB color
                            if i + 3 < params.len() {
                                self.cursor.fg = PackedColor::rgb(
                                    params[i + 1] as u8,
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                );
                                i += 3;
                            }
                        }
                        _ => {}
                    }
                }
                39 => self.cursor.fg = PackedColor::DEFAULT,
                40..=47 => self.cursor.bg = PackedColor::indexed((p - 40) as u8),
                48 => {
                    // Extended background color
                    i += 1;
                    if i >= params.len() {
                        break;
                    }
                    match params[i] {
                        5 => {
                            // 48;5;n - Indexed color
                            i += 1;
                            if i < params.len() {
                                self.cursor.bg = PackedColor::indexed(params[i] as u8);
                            }
                        }
                        2 => {
                            // 48;2;r;g;b - RGB color
                            if i + 3 < params.len() {
                                self.cursor.bg = PackedColor::rgb(
                                    params[i + 1] as u8,
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                );
                                i += 3;
                            }
                        }
                        _ => {}
                    }
                }
                49 => self.cursor.bg = PackedColor::DEFAULT,
                90..=97 => self.cursor.fg = PackedColor::indexed((p - 90 + 8) as u8),
                100..=107 => self.cursor.bg = PackedColor::indexed((p - 100 + 8) as u8),
                _ => {} // Unknown: ignore
            }
            i += 1;
        }
    }

    // ── Sprint 4: Edit Handlers ─────────────────────────────

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
        let base = row as usize * self.cols as usize;

        // Shift cells right (iterate in reverse)
        for c in (col + count..self.cols).rev() {
            self.grid[base + c as usize] = self.grid[base + (c - count) as usize];
        }
        // Clear inserted cells
        for c in col..col + count {
            self.grid[base + c as usize] = Cell::EMPTY;
        }
        // Handle overflow entries for this row
        overflow_clear_range(&mut self.overflow, row, col, self.cols);
        self.mark_row_dirty(row);
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
        let base = row as usize * self.cols as usize;

        // Shift cells left
        for c in col..self.cols - count {
            self.grid[base + c as usize] = self.grid[base + (c + count) as usize];
        }
        // Clear trailing cells
        for c in self.cols - count..self.cols {
            self.grid[base + c as usize] = Cell::EMPTY;
        }
        // Handle overflow entries for this row
        overflow_clear_range(&mut self.overflow, row, col, self.cols);
        self.mark_row_dirty(row);
    }

    // ── Sprint 4: Scroll Handlers ───────────────────────────

    /// CSI S - Scroll Up.
    /// Returns 0 if handled by WASM (scroll region internal).
    /// Returns count if TS should handle scrollback (full screen scroll).
    pub fn handle_scroll_up(&mut self, count: u16) -> u8 {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        let is_full_screen = top == 0 && bottom == self.rows.saturating_sub(1);

        if is_full_screen {
            // Full screen: TS handles scrollback
            return count.min(255) as u8;
        }

        // Scroll region: WASM handles internally
        let count = count.min(bottom - top + 1);
        self.shift_rows_up(top, bottom, count);
        0
    }

    /// CSI T - Scroll Down. Always WASM-internal.
    pub fn handle_scroll_down(&mut self, count: u16) {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        let count = count.min(bottom - top + 1);
        self.shift_rows_down(top, bottom, count);
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
        self.cursor.row = 0;
        self.wrap_pending = false;
    }

    // ── Sprint 4: Mode Handler ──────────────────────────────

    /// CSI ? Pm h/l - Set/Reset DEC Private Mode.
    /// Returns action code for TS-side execution.
    pub fn handle_set_mode(&mut self, mode: u16, enable: bool) -> u8 {
        match mode {
            // Boolean modes: set directly in WASM bitfield
            3 => {
                self.set_mode(MODE_COLUMN_132, enable);
                MODE_ACTION_NONE
            }
            5 => {
                self.set_mode(MODE_REVERSE_SCREEN, enable);
                MODE_ACTION_NONE
            }
            6 => {
                self.set_mode(MODE_ORIGIN, enable);
                MODE_ACTION_NONE
            }
            7 => {
                self.set_mode(MODE_AUTO_WRAP, enable);
                MODE_ACTION_NONE
            }
            12 => {
                self.set_mode(MODE_CURSOR_BLINK, enable);
                MODE_ACTION_NONE
            }
            25 => {
                self.set_mode(MODE_CURSOR_VISIBLE, enable);
                MODE_ACTION_NONE
            }

            // Buffer switch modes: return action code
            47 | 1047 => {
                if enable {
                    MODE_ACTION_SWITCH_TO_ALT
                } else {
                    MODE_ACTION_SWITCH_TO_MAIN
                }
            }
            1048 => {
                if enable {
                    MODE_ACTION_SAVE_CURSOR
                } else {
                    MODE_ACTION_RESTORE_CURSOR
                }
            }
            1049 => {
                if enable {
                    MODE_ACTION_SAVE_AND_SWITCH_TO_ALT
                } else {
                    MODE_ACTION_SWITCH_TO_MAIN
                }
            }

            // Boolean modes handled via TS fallback for multi-valued side effects
            1004 => {
                self.set_mode(MODE_FOCUS_TRACKING, enable);
                MODE_ACTION_NONE
            }
            2004 => {
                self.set_mode(MODE_BRACKETED_PASTE, enable);
                MODE_ACTION_NONE
            }

            // Multi-valued modes: TS fallback
            1 | 1000 | 1002 | 1003 | 1005 | 1006 => MODE_ACTION_TS_FALLBACK,

            // Unknown mode: no-op
            _ => MODE_ACTION_NONE,
        }
    }

    // ── Sprint 4: Device Response Handlers ──────────────────

    /// CSI Ps n - Device Status Report.
    /// Returns response length (0 if no response).
    pub fn handle_device_status_report(&mut self, ps: u8) -> u8 {
        match ps {
            5 => {
                // OK status
                self.write_response(b"\x1b[0n")
            }
            6 => {
                // Cursor position report (1-indexed)
                self.format_cpr()
            }
            _ => 0, // Unknown: no response
        }
    }

    /// CSI c - Primary Device Attributes.
    /// Returns response length.
    pub fn handle_primary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[?64;1;2;6;22c")
    }

    /// CSI > c - Secondary Device Attributes.
    /// Returns response length.
    pub fn handle_secondary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[>41;1;0c")
    }

    /// Get pointer to response buffer in linear memory.
    pub fn get_response_ptr(&self) -> *const u8 {
        self.response_buffer.as_ptr()
    }

    /// Get length of last device response.
    pub fn get_response_len(&self) -> u32 {
        self.response_len as u32
    }

    /// Get response buffer contents as a byte vector.
    /// Convenient alternative to ptr/len for TS integration.
    pub fn get_response_bytes(&self) -> Vec<u8> {
        self.response_buffer[..self.response_len as usize].to_vec()
    }

    /// Write bytes to response buffer. Returns length.
    fn write_response(&mut self, data: &[u8]) -> u8 {
        let len = data.len().min(self.response_buffer.len());
        self.response_buffer[..len].copy_from_slice(&data[..len]);
        self.response_len = len as u8;
        len as u8
    }

    /// Format cursor position report into response buffer.
    fn format_cpr(&mut self) -> u8 {
        let row = self.cursor.row.saturating_add(1);
        let col = self.cursor.col.saturating_add(1);
        // Format: ESC [ row ; col R
        let mut buf = [0u8; 20];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        let mut pos = 2;
        pos = Self::write_u16_decimal(&mut buf, pos, row);
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u16_decimal(&mut buf, pos, col);
        buf[pos] = b'R';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// Write a u16 as decimal digits to buffer, return new position.
    fn write_u16_decimal(buf: &mut [u8], start: usize, val: u16) -> usize {
        if val == 0 {
            buf[start] = b'0';
            return start + 1;
        }
        let mut digits = [0u8; 5];
        let mut n = val;
        let mut count = 0;
        while n > 0 {
            digits[count] = (n % 10) as u8 + b'0';
            n /= 10;
            count += 1;
        }
        let mut pos = start;
        for i in (0..count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
        pos
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Grid construction ────────────────────────────────

    #[test]
    fn test_grid_new_80x24() {
        let core = TerminalCore::new(80, 24);
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
        let mut core = TerminalCore::new(80, 24);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    #[test]
    fn test_set_get_cell_cjk() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cell(5, 3, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(5, 3), "漢");
        assert_eq!(core.get_cell_width(5, 3), 2);
    }

    #[test]
    fn test_set_get_cell_ascii_fast() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cell_ascii(10, 5, b'Z', 2, 100, 200, 50, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(10, 5), "Z");
        assert_eq!(core.get_cell_width(10, 5), 1);
        let fg = core.get_cell_fg(10, 5);
        assert_eq!(fg >> 24, 2); // tag = RGB
        assert_eq!((fg >> 16) & 0xFF, 100); // r
    }

    #[test]
    fn test_set_get_cell_with_attrs() {
        let mut core = TerminalCore::new(80, 24);
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
        let mut core = TerminalCore::new(80, 24);
        // Should not panic
        core.set_cell(80, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 24, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }

    #[test]
    fn test_oob_read_default() {
        let core = TerminalCore::new(80, 24);
        assert_eq!(core.get_cell_char(80, 0), " ");
        assert_eq!(core.get_cell_width(0, 24), 1);
        assert_eq!(core.get_cell_fg(100, 100), 0);
    }

    // ── Cursor ───────────────────────────────────────────

    #[test]
    fn test_cursor_initial() {
        let core = TerminalCore::new(80, 24);
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_cursor_set_clamp() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(100, 50);
        assert_eq!(core.get_cursor_col(), 79);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_cursor_save_restore() {
        let mut core = TerminalCore::new(80, 24);
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
        let core = TerminalCore::new(80, 24);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        assert!(core.get_mode(MODE_CURSOR_VISIBLE));
        assert!(core.get_mode(MODE_CURSOR_BLINK));
        assert!(!core.get_mode(MODE_ORIGIN));
        assert!(!core.get_mode(MODE_BRACKETED_PASTE));
    }

    #[test]
    fn test_modes_set_get() {
        let mut core = TerminalCore::new(80, 24);
        core.set_mode(MODE_BRACKETED_PASTE, true);
        assert!(core.get_mode(MODE_BRACKETED_PASTE));
        core.set_mode(MODE_AUTO_WRAP, false);
        assert!(!core.get_mode(MODE_AUTO_WRAP));
    }

    // ── Tab stops ────────────────────────────────────────

    #[test]
    fn test_tab_stops_default() {
        let core = TerminalCore::new(80, 24);
        // Default: every 8 columns
        assert_eq!(core.next_tab_stop(0), 8);
        assert_eq!(core.next_tab_stop(7), 8);
        assert_eq!(core.next_tab_stop(8), 16);
        assert_eq!(core.next_tab_stop(15), 16);
    }

    #[test]
    fn test_tab_stops_set_clear() {
        let mut core = TerminalCore::new(80, 24);
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
        let mut core = TerminalCore::new(80, 24);
        core.clear_dirty();
        assert!(!core.is_row_dirty(5));
        core.set_cell(0, 5, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(core.is_row_dirty(5));
    }

    #[test]
    fn test_dirty_clear_resets() {
        let mut core = TerminalCore::new(80, 24);
        // Initially all dirty
        assert!(!core.get_dirty_rows().is_empty());
        core.clear_dirty();
        assert!(core.get_dirty_rows().is_empty());
    }

    #[test]
    fn test_dirty_resize_marks_all() {
        let mut core = TerminalCore::new(80, 24);
        core.clear_dirty();
        core.resize(100, 30);
        let dirty = core.get_dirty_rows();
        assert_eq!(dirty.len(), 30);
    }

    // ── Line operations ──────────────────────────────────

    #[test]
    fn test_clear_line() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.clear_line(0);
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(1, 0), " ");
        assert!(core.is_line_empty(0));
    }

    #[test]
    fn test_clear_line_range() {
        let mut core = TerminalCore::new(80, 24);
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
        let mut core = TerminalCore::new(10, 1);
        core.set_cell(0, 0, "H", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Width-0 placeholder (e.g., second cell of wide char)
        let text = core.get_line_text(0);
        assert!(text.starts_with("Hi"));
    }

    #[test]
    fn test_get_line_text_skips_width0() {
        let mut core = TerminalCore::new(10, 1);
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
        let mut core = TerminalCore::new(80, 24);
        assert!(core.is_line_empty(0));
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.is_line_empty(0));
    }

    // ── Row operations ───────────────────────────────────

    #[test]
    fn test_shift_rows_up() {
        let mut core = TerminalCore::new(10, 5);
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
        let mut core = TerminalCore::new(10, 5);
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
        let mut core = TerminalCore::new(10, 5);
        core.set_cell(0, 0, "X", 1, 2, 255, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
        core.set_line_wrapped(0, true);
        core.copy_row(0, 3);
        assert_eq!(core.get_cell_char(0, 3), "X");
        assert_eq!(core.get_cell_flags(0, 3), STYLE_BOLD);
        assert!(core.get_line_wrapped(3));
    }

    #[test]
    fn test_fill_row_default() {
        let mut core = TerminalCore::new(10, 5);
        core.set_cell(0, 2, "Z", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.fill_row_default(2);
        assert!(core.is_line_empty(2));
    }

    // ── Resize ───────────────────────────────────────────

    #[test]
    fn test_resize_grow_cols() {
        let mut core = TerminalCore::new(10, 5);
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(20, 5);
        assert_eq!(core.cols(), 20);
        assert_eq!(core.get_cell_char(5, 0), "A");
    }

    #[test]
    fn test_resize_shrink_cols() {
        let mut core = TerminalCore::new(10, 5);
        core.set_cell(8, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.resize(5, 5);
        assert_eq!(core.cols(), 5);
        // Col 8 should be gone, reading it via get_cell_char returns default
        assert_eq!(core.get_cell_char(8, 0), " ");
    }

    #[test]
    fn test_resize_grow_shrink_rows() {
        let mut core = TerminalCore::new(10, 5);
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
        let mut core = TerminalCore::new(80, 24);
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
        let mut core = TerminalCore::new(3, 1);
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
        let mut core = TerminalCore::new(10, 5);
        let long = "👨‍👩‍👧‍👦";
        assert!(long.as_bytes().len() > 16);
        core.set_cell(0, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 3), long);

        core.shift_rows_up(0, 4, 2);
        // Row 3 shifted to row 1
        assert_eq!(core.get_cell_char(0, 1), long);
    }

    // ── Sprint 2: Print handler tests ───────────────────

    // TS-R01: handle_print ASCII 'A' at (0,0)
    #[test]
    fn test_handle_print_ascii_basic() {
        let mut core = TerminalCore::new(80, 24);
        let scroll = core.handle_print(0x41); // 'A'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cursor_row(), 0);
    }

    // TS-R02: handle_print ASCII at (cols-1,0) with autoWrap
    #[test]
    fn test_handle_print_ascii_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 0);
        let scroll = core.handle_print(0x41); // 'A'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(79, 0), "A");
        assert!(core.get_wrap_pending());
        assert_eq!(core.get_cursor_col(), 79);
    }

    // TS-R03: handle_print ASCII with wrap_pending
    #[test]
    fn test_handle_print_with_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 0);
        core.handle_print(0x41); // 'A' - sets wrap_pending
        assert!(core.get_wrap_pending());
        let scroll = core.handle_print(0x42); // 'B' - triggers wrap
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 1), "B");
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cursor_row(), 1);
        assert!(!core.get_wrap_pending());
        assert!(core.get_line_wrapped(1));
    }

    // TS-R04: handle_print ASCII at bottom with wrap_pending → scroll
    #[test]
    fn test_handle_print_scroll_at_bottom() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 23);
        core.handle_print(0x41); // 'A' - sets wrap_pending
        let scroll = core.handle_print(0x42); // 'B' - triggers wrap+scroll
        assert_eq!(scroll, 1);
        assert_eq!(core.get_cursor_col(), 1);
        assert_eq!(core.get_cursor_row(), 23);
    }

    // TS-R05: handle_print CJK at (0,0)
    #[test]
    fn test_handle_print_cjk() {
        let mut core = TerminalCore::new(80, 24);
        let scroll = core.handle_print(0x6F22); // '漢'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "漢");
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0); // placeholder
        assert_eq!(core.get_cursor_col(), 2);
    }

    // TS-R06: handle_print CJK at (cols-1,0) with autoWrap
    #[test]
    fn test_handle_print_cjk_wrap() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 0);
        let scroll = core.handle_print(0x6F22); // '漢' width=2 wraps
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 1), "漢");
        assert_eq!(core.get_cell_width(0, 1), 2);
        assert_eq!(core.get_cursor_col(), 2);
        assert_eq!(core.get_cursor_row(), 1);
    }

    // TS-R07: handle_print Emoji → buffered
    #[test]
    fn test_handle_print_emoji_buffered() {
        let mut core = TerminalCore::new(80, 24);
        let scroll = core.handle_print(0x1F600); // 😀
        assert_eq!(scroll, 0);
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        assert_eq!(core.get_cell_char(0, 0), " "); // not written yet
    }

    // TS-R08: handle_print ZWJ after emoji → extends buffer
    #[test]
    fn test_handle_print_zwj_extends() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F468); // 👨
        core.handle_print(0x200D); // ZWJ
        assert_eq!(core.get_grapheme_buffer_len(), 2);
    }

    // TS-R09: handle_print non-extending after buffered emoji → flush + new
    #[test]
    fn test_handle_print_flush_then_new() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F600); // 😀 → buffer
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        core.handle_print(0x41); // 'A' → flush 😀 then print A
        assert_eq!(core.get_grapheme_buffer_len(), 0);
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_char(2, 0), "A");
        assert_eq!(core.get_cursor_col(), 3);
    }

    // TS-R10: Regional Indicator pair → auto-flush
    #[test]
    fn test_handle_print_ri_pair() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F1EF); // Regional Indicator J
        assert_eq!(core.get_grapheme_buffer_len(), 1);
        core.handle_print(0x1F1F5); // Regional Indicator P → auto-flush
        assert_eq!(core.get_grapheme_buffer_len(), 0);
        assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    // TS-R11: Variation Selector FE0E → width 1
    #[test]
    fn test_handle_print_vs_fe0e() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x2660); // ♠
        core.handle_print(0xFE0E); // VS15 text
        assert_eq!(core.get_grapheme_buffer_len(), 2);
        core.handle_print(0x41); // flush
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    // TS-R12: Variation Selector FE0F → width 2
    #[test]
    fn test_handle_print_vs_fe0f() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x2660); // ♠
        core.handle_print(0xFE0F); // VS16 emoji
        assert_eq!(core.get_grapheme_buffer_len(), 2);
        core.handle_print(0x41); // flush
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    // TS-R13: Skin tone modifier → extends buffer
    #[test]
    fn test_handle_print_skin_tone() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F44B); // 👋
        core.handle_print(0x1F3FD); // Medium skin tone
        assert_eq!(core.get_grapheme_buffer_len(), 2);
    }

    // TS-R14: Buffer overflow → auto-flush at 64
    #[test]
    fn test_handle_print_buffer_overflow() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F468); // 👨
        for _ in 0..31 {
            core.handle_print(0x200D);
            core.handle_print(0x1F468);
        }
        assert_eq!(core.get_grapheme_buffer_len(), 63);
        core.handle_print(0x200D); // now 64
                                   // Next should trigger auto-flush (>= 64)
        core.handle_print(0x1F468);
        assert!(core.get_grapheme_buffer_len() <= 1);
    }

    // TS-R15: DEC Line Drawing 'q' → '─' when active
    #[test]
    fn test_handle_print_dec_line_drawing() {
        let mut core = TerminalCore::new(80, 24);
        core.set_g0_charset(1); // DecLineDrawing
        let scroll = core.handle_print(0x71); // 'q'
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "─"); // U+2500
    }

    // TS-R16: DEC Line Drawing inactive → no translation
    #[test]
    fn test_handle_print_dec_line_drawing_inactive() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x71); // 'q'
        assert_eq!(core.get_cell_char(0, 0), "q");
    }

    // TS-R17: G1 charset with DecLineDrawing
    #[test]
    fn test_handle_print_g1_dec_line_drawing() {
        let mut core = TerminalCore::new(80, 24);
        core.set_g1_charset(1);
        core.set_active_charset(1);
        core.handle_print(0x78); // 'x'
        assert_eq!(core.get_cell_char(0, 0), "│"); // U+2502
    }

    // TS-R18: autoWrap OFF at line end
    #[test]
    fn test_handle_print_no_autowrap() {
        let mut core = TerminalCore::new(80, 24);
        core.set_mode(MODE_AUTO_WRAP, false);
        core.set_cursor(79, 0);
        core.handle_print(0x41); // 'A'
        assert_eq!(core.get_cell_char(79, 0), "A");
        assert_eq!(core.get_cursor_col(), 79);
        assert!(!core.get_wrap_pending());
        core.handle_print(0x42); // 'B' overwrites
        assert_eq!(core.get_cell_char(79, 0), "B");
        assert_eq!(core.get_cursor_col(), 79);
    }

    // TS-R19: flush_grapheme_buffer empty
    #[test]
    fn test_flush_empty() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.flush_grapheme_buffer(), 0);
    }

    // TS-R20: flush_grapheme_buffer single emoji
    #[test]
    fn test_flush_single_emoji() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F600); // 😀
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_char(0, 0), "😀");
        assert_eq!(core.get_cell_width(0, 0), 2);
    }

    // TS-R21: flush_grapheme_buffer ZWJ sequence
    #[test]
    fn test_flush_zwj_sequence() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F468); // 👨
        core.handle_print(0x200D); // ZWJ
        core.handle_print(0x1F469); // 👩
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cell_width(0, 0), 2);
        let ch = core.get_cell_char(0, 0);
        assert!(ch.contains('\u{200D}'));
    }

    // TS-R22: flush_grapheme_buffer flag (RI pair)
    #[test]
    fn test_flush_flag_ri_pair() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_print(0x1F1EF); // J
        core.handle_print(0x1F1F5); // P → auto-flush
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
    }

    // TS-R23: flush_grapheme_buffer with wrap_pending
    #[test]
    fn test_flush_with_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 23);
        core.handle_print(0x41); // 'A' → wrap_pending
        assert!(core.get_wrap_pending());
        core.handle_print(0x1F600); // 😀 → buffered
        let scroll = core.flush_grapheme_buffer();
        assert_eq!(scroll, 1);
        assert_eq!(core.get_cursor_row(), 23);
    }

    // TS-R24: scroll_region: LF within region
    #[test]
    fn test_scroll_region_lf_within() {
        let mut core = TerminalCore::new(80, 24);
        core.set_scroll_region(5, 20);
        core.set_cursor(79, 10);
        core.handle_print(0x41); // wrap_pending
        let scroll = core.handle_print(0x42); // LF within region
        assert_eq!(scroll, 0);
        assert_eq!(core.get_cursor_row(), 11);
    }

    // TS-R25: scroll_region: LF at region bottom
    #[test]
    fn test_scroll_region_lf_at_bottom() {
        let mut core = TerminalCore::new(80, 24);
        core.set_scroll_region(5, 20);
        core.set_cursor(79, 20);
        core.handle_print(0x41); // wrap_pending
        let scroll = core.handle_print(0x42); // LF at bottom → scroll
        assert_eq!(scroll, 1);
        assert_eq!(core.get_cursor_row(), 20);
    }

    // TS-R26: Charset getter/setter round-trip
    #[test]
    fn test_charset_round_trip() {
        let mut core = TerminalCore::new(80, 24);
        core.set_g0_charset(1);
        assert_eq!(core.get_g0_charset(), 1);
        core.set_g1_charset(1);
        assert_eq!(core.get_g1_charset(), 1);
        core.set_g0_charset(0);
        assert_eq!(core.get_g0_charset(), 0);
    }

    // TS-R27: Active charset switch
    #[test]
    fn test_active_charset_switch() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.get_active_charset(), 0);
        core.set_active_charset(1);
        assert_eq!(core.get_active_charset(), 1);
        core.set_active_charset(0);
        assert_eq!(core.get_active_charset(), 0);
    }

    // TS-R28: wrap_pending getter/setter
    #[test]
    fn test_wrap_pending_round_trip() {
        let mut core = TerminalCore::new(80, 24);
        assert!(!core.get_wrap_pending());
        core.set_wrap_pending(true);
        assert!(core.get_wrap_pending());
        core.set_wrap_pending(false);
        assert!(!core.get_wrap_pending());
    }

    // TS-R29: scroll_region getter/setter
    #[test]
    fn test_scroll_region_round_trip() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 23);
        core.set_scroll_region(5, 20);
        assert_eq!(core.get_scroll_region_top(), 5);
        assert_eq!(core.get_scroll_region_bottom(), 20);
    }

    // DEC Line Drawing: all 32 entries
    #[test]
    fn test_dec_line_drawing_all_entries() {
        let expected: [(u32, u32); 32] = [
            (0x5F, 0x0020),
            (0x60, 0x25C6),
            (0x61, 0x2592),
            (0x62, 0x2409),
            (0x63, 0x240C),
            (0x64, 0x240D),
            (0x65, 0x240A),
            (0x66, 0x00B0),
            (0x67, 0x00B1),
            (0x68, 0x2424),
            (0x69, 0x240B),
            (0x6A, 0x2518),
            (0x6B, 0x2510),
            (0x6C, 0x250C),
            (0x6D, 0x2514),
            (0x6E, 0x253C),
            (0x6F, 0x23BA),
            (0x70, 0x23BB),
            (0x71, 0x2500),
            (0x72, 0x23BC),
            (0x73, 0x23BD),
            (0x74, 0x251C),
            (0x75, 0x2524),
            (0x76, 0x2534),
            (0x77, 0x252C),
            (0x78, 0x2502),
            (0x79, 0x2264),
            (0x7A, 0x2265),
            (0x7B, 0x03C0),
            (0x7C, 0x2260),
            (0x7D, 0x00A3),
            (0x7E, 0x00B7),
        ];
        for (input, output) in expected {
            assert_eq!(
                TerminalCore::translate_line_drawing(input),
                output,
                "0x{:02X} → 0x{:04X}",
                input,
                output
            );
        }
    }

    // Reset clears Sprint 2 state
    #[test]
    fn test_reset_clears_sprint2_state() {
        let mut core = TerminalCore::new(80, 24);
        core.set_g0_charset(1);
        core.set_g1_charset(1);
        core.set_active_charset(1);
        core.set_wrap_pending(true);
        core.set_scroll_region(5, 20);
        core.handle_print(0x1F600);
        assert_eq!(core.get_grapheme_buffer_len(), 1);

        core.reset();

        assert_eq!(core.get_g0_charset(), 0);
        assert_eq!(core.get_g1_charset(), 0);
        assert_eq!(core.get_active_charset(), 0);
        assert!(!core.get_wrap_pending());
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 23);
        assert_eq!(core.get_grapheme_buffer_len(), 0);
    }

    // Resize resets scroll region
    #[test]
    fn test_resize_resets_scroll_region() {
        let mut core = TerminalCore::new(80, 24);
        core.set_scroll_region(5, 20);
        core.set_wrap_pending(true);

        core.resize(100, 30);

        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 29);
        assert!(!core.get_wrap_pending());
    }

    // ── Sprint 3: C0 handler tests ─────────────────────────

    #[test]
    fn test_handle_execute_bel_returns_sentinel() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_execute(0x07), 0xFE);
    }

    #[test]
    fn test_handle_execute_bs_at_col5() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(5, 0);
        let result = core.handle_execute(0x08);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 4);
    }

    #[test]
    fn test_handle_execute_bs_at_col0_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 0);
        let result = core.handle_execute(0x08);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_execute_bs_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(5, 0);
        core.set_wrap_pending(true);
        core.handle_execute(0x08);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_ht_default_tab_stops() {
        let mut core = TerminalCore::new(80, 24);
        // col=0 → next tab stop at 8
        core.set_cursor(0, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 8);
    }

    #[test]
    fn test_handle_execute_ht_col7_to_col8() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(7, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 8);
    }

    #[test]
    fn test_handle_execute_ht_col8_to_col16() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(8, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 16);
    }

    #[test]
    fn test_handle_execute_ht_past_last_stop() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(78, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 79); // cols-1
    }

    #[test]
    fn test_handle_execute_ht_custom_tab_stops() {
        let mut core = TerminalCore::new(80, 24);
        core.clear_all_tab_stops();
        core.set_tab_stop(5);
        core.set_tab_stop(20);
        core.set_cursor(0, 0);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 5);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 20);
        core.handle_execute(0x09);
        assert_eq!(core.get_cursor_col(), 79); // no more stops
    }

    #[test]
    fn test_handle_execute_ht_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_execute(0x09);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_lf_mid_screen() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 0);
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 1);
    }

    #[test]
    fn test_handle_execute_lf_at_scroll_region_bottom() {
        let mut core = TerminalCore::new(80, 24);
        core.set_scroll_region(5, 20);
        core.set_cursor(0, 20);
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 1);
        assert_eq!(core.get_cursor_row(), 20);
    }

    #[test]
    fn test_handle_execute_lf_at_bottom_no_scroll_region() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 23);
        let result = core.handle_execute(0x0A);
        assert_eq!(result, 1);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_execute_vt_same_as_lf() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        let result = core.handle_execute(0x0B);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 6);
    }

    #[test]
    fn test_handle_execute_ff_same_as_lf() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        let result = core.handle_execute(0x0C);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_row(), 6);
    }

    #[test]
    fn test_handle_execute_cr() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(40, 10);
        let result = core.handle_execute(0x0D);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 10);
    }

    #[test]
    fn test_handle_execute_cr_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_execute(0x0D);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_so() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.get_active_charset(), 0);
        core.handle_execute(0x0E);
        assert_eq!(core.get_active_charset(), 1);
    }

    #[test]
    fn test_handle_execute_si() {
        let mut core = TerminalCore::new(80, 24);
        core.set_active_charset(1);
        core.handle_execute(0x0F);
        assert_eq!(core.get_active_charset(), 0);
    }

    #[test]
    fn test_handle_execute_lf_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.set_wrap_pending(true);
        core.handle_execute(0x0A);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_execute_unknown_byte_noop() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(5, 5);
        let result = core.handle_execute(0x01);
        assert_eq!(result, 0);
        assert_eq!(core.get_cursor_col(), 5);
        assert_eq!(core.get_cursor_row(), 5);
    }

    // ── Sprint 3: CSI Cursor handler tests ──────────────────

    #[test]
    fn test_handle_cursor_up_normal() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.handle_cursor_up(3);
        assert_eq!(core.get_cursor_row(), 2);
    }

    #[test]
    fn test_handle_cursor_up_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.handle_cursor_up(10);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_handle_cursor_up_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.set_wrap_pending(true);
        core.handle_cursor_up(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_down_normal() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.handle_cursor_down(3);
        assert_eq!(core.get_cursor_row(), 8);
    }

    #[test]
    fn test_handle_cursor_down_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 5);
        core.handle_cursor_down(100);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_cursor_down_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_down(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_forward_normal() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(10, 0);
        core.handle_cursor_forward(5);
        assert_eq!(core.get_cursor_col(), 15);
    }

    #[test]
    fn test_handle_cursor_forward_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(10, 0);
        core.handle_cursor_forward(100);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_forward_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_forward(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_back_normal() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(10, 0);
        core.handle_cursor_back(5);
        assert_eq!(core.get_cursor_col(), 5);
    }

    #[test]
    fn test_handle_cursor_back_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(10, 0);
        core.handle_cursor_back(100);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_back_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_back(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_next_line() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(15, 3);
        core.handle_cursor_next_line(2);
        assert_eq!(core.get_cursor_row(), 5);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_next_line_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(15, 20);
        core.handle_cursor_next_line(100);
        assert_eq!(core.get_cursor_row(), 23);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_previous_line() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(15, 5);
        core.handle_cursor_previous_line(2);
        assert_eq!(core.get_cursor_row(), 3);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_previous_line_clamped() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(15, 2);
        core.handle_cursor_previous_line(100);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_horizontal_absolute(5); // 1-indexed → col=4
        assert_eq!(core.get_cursor_col(), 4);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_zero() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_horizontal_absolute(0); // 0 → col=0
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_overflow() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_horizontal_absolute(1000);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_horizontal_absolute_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_horizontal_absolute(1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_position() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_position(3, 5); // 1-indexed → row=2, col=4
        assert_eq!(core.get_cursor_row(), 2);
        assert_eq!(core.get_cursor_col(), 4);
    }

    #[test]
    fn test_handle_cursor_position_zero_zero() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_position(0, 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert_eq!(core.get_cursor_col(), 0);
    }

    #[test]
    fn test_handle_cursor_position_overflow() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_position(1000, 1000);
        assert_eq!(core.get_cursor_row(), 23);
        assert_eq!(core.get_cursor_col(), 79);
    }

    #[test]
    fn test_handle_cursor_position_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_position(1, 1);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_handle_cursor_vertical_absolute() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_vertical_absolute(5); // 1-indexed → row=4
        assert_eq!(core.get_cursor_row(), 4);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_zero() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_vertical_absolute(0);
        assert_eq!(core.get_cursor_row(), 0);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_overflow() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_cursor_vertical_absolute(1000);
        assert_eq!(core.get_cursor_row(), 23);
    }

    #[test]
    fn test_handle_cursor_vertical_absolute_clears_wrap_pending() {
        let mut core = TerminalCore::new(80, 24);
        core.set_wrap_pending(true);
        core.handle_cursor_vertical_absolute(1);
        assert!(!core.get_wrap_pending());
    }

    // ── Sprint 3: CSI Screen handler tests ──────────────────

    #[test]
    fn test_handle_erase_in_display_below() {
        let mut core = TerminalCore::new(10, 5);
        // Fill entire screen
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell(col, row, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.clear_dirty();
        core.set_cursor(5, 2);
        let result = core.handle_erase_in_display(0); // Below
        assert_eq!(result, 0);
        // Row 2, cols 0-4 should still have "X"
        assert_eq!(core.get_cell_char(4, 2), "X");
        // Row 2, cols 5-9 should be cleared
        assert_eq!(core.get_cell_char(5, 2), " ");
        assert_eq!(core.get_cell_char(9, 2), " ");
        // Rows below should be cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
        // Rows above should be untouched
        assert_eq!(core.get_cell_char(0, 0), "X");
        assert_eq!(core.get_cell_char(0, 1), "X");
        // Dirty rows marked
        assert!(core.is_row_dirty(2));
        assert!(core.is_row_dirty(3));
        assert!(core.is_row_dirty(4));
    }

    #[test]
    fn test_handle_erase_in_display_above() {
        let mut core = TerminalCore::new(10, 5);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell(col, row, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.clear_dirty();
        core.set_cursor(5, 2);
        let result = core.handle_erase_in_display(1); // Above
        assert_eq!(result, 0);
        // Rows above cursor should be cleared
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(9, 1), " ");
        // Row 2, cols 0-5 should be cleared (inclusive of cursor col)
        assert_eq!(core.get_cell_char(0, 2), " ");
        assert_eq!(core.get_cell_char(5, 2), " ");
        // Row 2, cols 6-9 should still have "X"
        assert_eq!(core.get_cell_char(6, 2), "X");
        // Rows below should be untouched
        assert_eq!(core.get_cell_char(0, 3), "X");
        assert_eq!(core.get_cell_char(0, 4), "X");
    }

    #[test]
    fn test_handle_erase_in_display_all() {
        let mut core = TerminalCore::new(10, 5);
        for row in 0..5 {
            for col in 0..10 {
                core.set_cell(col, row, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.clear_dirty();
        let result = core.handle_erase_in_display(2); // All
        assert_eq!(result, 0);
        for row in 0..5 {
            for col in 0..10 {
                assert_eq!(core.get_cell_char(col, row), " ");
            }
            assert!(core.is_row_dirty(row));
        }
    }

    #[test]
    fn test_handle_erase_in_display_scrollback_returns_sentinel() {
        let mut core = TerminalCore::new(10, 5);
        let result = core.handle_erase_in_display(3); // Scrollback
        assert_eq!(result, 0xFF);
    }

    #[test]
    fn test_handle_erase_in_display_invalid_mode() {
        let mut core = TerminalCore::new(10, 5);
        let result = core.handle_erase_in_display(4);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_handle_erase_in_line_to_end() {
        let mut core = TerminalCore::new(10, 5);
        for col in 0..10 {
            core.set_cell(col, 2, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_dirty();
        core.set_cursor(5, 2);
        core.handle_erase_in_line(0); // ToEnd
                                      // Cols 0-4 should still have "X"
        assert_eq!(core.get_cell_char(4, 2), "X");
        // Cols 5-9 should be cleared
        assert_eq!(core.get_cell_char(5, 2), " ");
        assert_eq!(core.get_cell_char(9, 2), " ");
        assert!(core.is_row_dirty(2));
    }

    #[test]
    fn test_handle_erase_in_line_to_start() {
        let mut core = TerminalCore::new(10, 5);
        for col in 0..10 {
            core.set_cell(col, 2, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_dirty();
        core.set_cursor(5, 2);
        core.handle_erase_in_line(1); // ToStart (inclusive)
                                      // Cols 0-5 should be cleared
        assert_eq!(core.get_cell_char(0, 2), " ");
        assert_eq!(core.get_cell_char(5, 2), " ");
        // Cols 6-9 should still have "X"
        assert_eq!(core.get_cell_char(6, 2), "X");
        assert!(core.is_row_dirty(2));
    }

    #[test]
    fn test_handle_erase_in_line_all() {
        let mut core = TerminalCore::new(10, 5);
        for col in 0..10 {
            core.set_cell(col, 2, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_dirty();
        core.set_cursor(3, 2);
        core.handle_erase_in_line(2); // All
        for col in 0..10 {
            assert_eq!(core.get_cell_char(col, 2), " ");
        }
        assert!(core.is_row_dirty(2));
    }

    #[test]
    fn test_handle_erase_characters_normal() {
        let mut core = TerminalCore::new(10, 5);
        for col in 0..10 {
            core.set_cell(col, 2, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.clear_dirty();
        core.set_cursor(5, 2);
        core.handle_erase_characters(3);
        // Cols 5,6,7 should be cleared
        assert_eq!(core.get_cell_char(5, 2), " ");
        assert_eq!(core.get_cell_char(6, 2), " ");
        assert_eq!(core.get_cell_char(7, 2), " ");
        // Col 8 should still have "X"
        assert_eq!(core.get_cell_char(8, 2), "X");
        assert!(core.is_row_dirty(2));
    }

    #[test]
    fn test_handle_erase_characters_overflow_clamped() {
        let mut core = TerminalCore::new(10, 5);
        for col in 0..10 {
            core.set_cell(col, 2, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(8, 2);
        core.handle_erase_characters(100); // overflow
                                           // Cols 8-9 cleared, no panic
        assert_eq!(core.get_cell_char(8, 2), " ");
        assert_eq!(core.get_cell_char(9, 2), " ");
        // Col 7 untouched
        assert_eq!(core.get_cell_char(7, 2), "X");
    }

    #[test]
    fn test_handle_erase_characters_dirty() {
        let mut core = TerminalCore::new(10, 5);
        core.clear_dirty();
        core.set_cursor(0, 0);
        core.handle_erase_characters(5);
        assert!(core.is_row_dirty(0));
    }

    // ── Sprint 4: SGR Tests ─────────────────────────────────

    #[test]
    fn test_sgr_empty_resets() {
        let mut core = TerminalCore::new(80, 24);
        core.cursor.fg = PackedColor::indexed(1);
        core.cursor.flags = STYLE_BOLD;
        core.handle_sgr(&[]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.bg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    #[test]
    fn test_sgr_reset_param0() {
        let mut core = TerminalCore::new(80, 24);
        core.cursor.fg = PackedColor::indexed(1);
        core.cursor.flags = STYLE_BOLD | STYLE_ITALIC;
        core.handle_sgr(&[0]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    #[test]
    fn test_sgr_style_flags() {
        let cases: &[(u16, u16)] = &[
            (1, STYLE_BOLD),
            (2, STYLE_DIM),
            (3, STYLE_ITALIC),
            (4, STYLE_UNDERLINE),
            (5, STYLE_BLINK),
            (7, STYLE_REVERSE),
            (8, STYLE_HIDDEN),
            (9, STYLE_STRIKETHROUGH),
        ];
        for &(param, flag) in cases {
            let mut core = TerminalCore::new(80, 24);
            core.handle_sgr(&[param]);
            assert_ne!(
                core.cursor.flags & flag,
                0,
                "SGR {} should set flag 0x{:04x}",
                param,
                flag
            );
        }
    }

    #[test]
    fn test_sgr_style_resets() {
        let cases: &[(u16, u16)] = &[
            (22, STYLE_BOLD | STYLE_DIM),
            (23, STYLE_ITALIC),
            (24, STYLE_UNDERLINE),
            (25, STYLE_BLINK),
            (27, STYLE_REVERSE),
            (28, STYLE_HIDDEN),
            (29, STYLE_STRIKETHROUGH),
        ];
        for &(param, flag) in cases {
            let mut core = TerminalCore::new(80, 24);
            core.cursor.flags = 0xFFFF;
            core.handle_sgr(&[param]);
            assert_eq!(
                core.cursor.flags & flag,
                0,
                "SGR {} should clear flag 0x{:04x}",
                param,
                flag
            );
        }
    }

    #[test]
    fn test_sgr_standard_foreground() {
        for p in 30..=37 {
            let mut core = TerminalCore::new(80, 24);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.fg, PackedColor::indexed((p - 30) as u8));
        }
    }

    #[test]
    fn test_sgr_standard_background() {
        for p in 40..=47 {
            let mut core = TerminalCore::new(80, 24);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.bg, PackedColor::indexed((p - 40) as u8));
        }
    }

    #[test]
    fn test_sgr_bright_foreground() {
        for p in 90..=97 {
            let mut core = TerminalCore::new(80, 24);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.fg, PackedColor::indexed((p - 90 + 8) as u8));
        }
    }

    #[test]
    fn test_sgr_bright_background() {
        for p in 100..=107 {
            let mut core = TerminalCore::new(80, 24);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.bg, PackedColor::indexed((p - 100 + 8) as u8));
        }
    }

    #[test]
    fn test_sgr_indexed_fg() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[38, 5, 196]);
        assert_eq!(core.cursor.fg, PackedColor::indexed(196));
    }

    #[test]
    fn test_sgr_indexed_bg() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[48, 5, 21]);
        assert_eq!(core.cursor.bg, PackedColor::indexed(21));
    }

    #[test]
    fn test_sgr_rgb_fg() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[38, 2, 255, 128, 0]);
        assert_eq!(core.cursor.fg, PackedColor::rgb(255, 128, 0));
    }

    #[test]
    fn test_sgr_rgb_bg() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[48, 2, 0, 128, 255]);
        assert_eq!(core.cursor.bg, PackedColor::rgb(0, 128, 255));
    }

    #[test]
    fn test_sgr_default_fg_bg() {
        let mut core = TerminalCore::new(80, 24);
        core.cursor.fg = PackedColor::indexed(5);
        core.cursor.bg = PackedColor::indexed(3);
        core.handle_sgr(&[39]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        core.handle_sgr(&[49]);
        assert_eq!(core.cursor.bg, PackedColor::DEFAULT);
    }

    #[test]
    fn test_sgr_multiple_params() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[1, 31, 42]);
        assert_ne!(core.cursor.flags & STYLE_BOLD, 0);
        assert_eq!(core.cursor.fg, PackedColor::indexed(1)); // red
        assert_eq!(core.cursor.bg, PackedColor::indexed(2)); // green
    }

    #[test]
    fn test_sgr_truncated_extended() {
        let mut core = TerminalCore::new(80, 24);
        // 38;5 without index - should not panic
        core.handle_sgr(&[38, 5]);
        // 38 without subtype - should not panic
        core.handle_sgr(&[38]);
    }

    #[test]
    fn test_sgr_unknown_param() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_sgr(&[99]);
        // Should not crash, attrs unchanged
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    // ── Sprint 4: Edit Tests ────────────────────────────────

    #[test]
    fn test_insert_lines_basic() {
        let mut core = TerminalCore::new(10, 5);
        // Fill rows with identifiable content
        for row in 0..5 {
            core.set_cell(0, row, &format!("{}", row), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(0, 1);
        core.handle_insert_lines(2);
        // Row 0 unchanged
        assert_eq!(core.get_cell_char(0, 0), "0");
        // Rows 1-2 should be blank (inserted)
        assert_eq!(core.get_cell_char(0, 1), " ");
        assert_eq!(core.get_cell_char(0, 2), " ");
        // Old row 1 moved to row 3
        assert_eq!(core.get_cell_char(0, 3), "1");
    }

    #[test]
    fn test_insert_lines_outside_region() {
        let mut core = TerminalCore::new(10, 10);
        core.set_scroll_region(2, 7);
        core.set_cursor(0, 1); // Outside region
        core.set_cell(0, 1, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.handle_insert_lines(1);
        // No-op: content unchanged
        assert_eq!(core.get_cell_char(0, 1), "X");
    }

    #[test]
    fn test_insert_lines_count_clamped() {
        let mut core = TerminalCore::new(10, 5);
        core.set_cursor(0, 3);
        core.set_cell(0, 3, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.handle_insert_lines(100); // Exceeds available rows
                                       // Should still work, no panic, row 3 cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
    }

    #[test]
    fn test_delete_lines_basic() {
        let mut core = TerminalCore::new(10, 5);
        for row in 0..5 {
            core.set_cell(0, row, &format!("{}", row), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(0, 1);
        core.handle_delete_lines(2);
        // Row 0 unchanged
        assert_eq!(core.get_cell_char(0, 0), "0");
        // Old rows 3,4 moved to 1,2
        assert_eq!(core.get_cell_char(0, 1), "3");
        assert_eq!(core.get_cell_char(0, 2), "4");
        // Rows 3-4 cleared
        assert_eq!(core.get_cell_char(0, 3), " ");
        assert_eq!(core.get_cell_char(0, 4), " ");
    }

    #[test]
    fn test_delete_lines_outside_region() {
        let mut core = TerminalCore::new(10, 10);
        core.set_scroll_region(2, 7);
        core.set_cursor(0, 1);
        core.set_cell(0, 1, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.handle_delete_lines(1);
        assert_eq!(core.get_cell_char(0, 1), "X");
    }

    #[test]
    fn test_insert_characters_basic() {
        let mut core = TerminalCore::new(10, 1);
        for col in 0..10 {
            core.set_cell(
                col,
                0,
                &format!("{}", col % 10),
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            );
        }
        core.set_cursor(3, 0);
        core.handle_insert_characters(2);
        // Cols 0-2 unchanged
        assert_eq!(core.get_cell_char(0, 0), "0");
        assert_eq!(core.get_cell_char(2, 0), "2");
        // Cols 3-4 should be blank (inserted)
        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_char(4, 0), " ");
        // Old col 3 moved to col 5
        assert_eq!(core.get_cell_char(5, 0), "3");
    }

    #[test]
    fn test_insert_characters_clamped() {
        let mut core = TerminalCore::new(10, 1);
        core.set_cursor(8, 0);
        core.set_cell(8, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.handle_insert_characters(100); // Exceeds remaining cols
        assert_eq!(core.get_cell_char(8, 0), " ");
        assert_eq!(core.get_cell_char(9, 0), " ");
    }

    #[test]
    fn test_delete_characters_basic() {
        let mut core = TerminalCore::new(10, 1);
        for col in 0..10 {
            core.set_cell(
                col,
                0,
                &format!("{}", col % 10),
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            );
        }
        core.set_cursor(3, 0);
        core.handle_delete_characters(2);
        // Cols 0-2 unchanged
        assert_eq!(core.get_cell_char(0, 0), "0");
        assert_eq!(core.get_cell_char(2, 0), "2");
        // Old col 5 moved to col 3
        assert_eq!(core.get_cell_char(3, 0), "5");
        // Trailing cols cleared
        assert_eq!(core.get_cell_char(8, 0), " ");
        assert_eq!(core.get_cell_char(9, 0), " ");
    }

    #[test]
    fn test_delete_characters_clamped() {
        let mut core = TerminalCore::new(10, 1);
        core.set_cursor(8, 0);
        core.set_cell(8, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.handle_delete_characters(100);
        assert_eq!(core.get_cell_char(8, 0), " ");
    }

    #[test]
    fn test_edit_dirty_marking() {
        let mut core = TerminalCore::new(10, 5);
        core.clear_dirty();
        core.set_cursor(0, 2);
        core.handle_insert_lines(1);
        assert!(core.is_row_dirty(2));

        core.clear_dirty();
        core.set_cursor(0, 2);
        core.handle_delete_lines(1);
        assert!(core.is_row_dirty(2));

        core.clear_dirty();
        core.set_cursor(3, 0);
        core.handle_insert_characters(1);
        assert!(core.is_row_dirty(0));

        core.clear_dirty();
        core.set_cursor(3, 0);
        core.handle_delete_characters(1);
        assert!(core.is_row_dirty(0));
    }

    // ── Sprint 4: Scroll Tests ──────────────────────────────

    #[test]
    fn test_scroll_up_scroll_region() {
        let mut core = TerminalCore::new(10, 10);
        core.set_scroll_region(2, 7);
        for row in 2..=7 {
            core.set_cell(0, row, &format!("{}", row), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        let result = core.handle_scroll_up(2);
        assert_eq!(result, 0); // WASM handled
                               // Rows 4-7 moved to 2-5
        assert_eq!(core.get_cell_char(0, 2), "4");
        assert_eq!(core.get_cell_char(0, 5), "7");
        // Bottom rows cleared
        assert_eq!(core.get_cell_char(0, 6), " ");
        assert_eq!(core.get_cell_char(0, 7), " ");
    }

    #[test]
    fn test_scroll_up_full_screen() {
        let mut core = TerminalCore::new(10, 10);
        // Full screen (default region 0..9)
        let result = core.handle_scroll_up(3);
        assert_eq!(result, 3); // TS handles scrollback
    }

    #[test]
    fn test_scroll_up_clamped() {
        let mut core = TerminalCore::new(10, 10);
        core.set_scroll_region(2, 5);
        let result = core.handle_scroll_up(100);
        assert_eq!(result, 0); // Still WASM (scroll region)
    }

    #[test]
    fn test_scroll_down_basic() {
        let mut core = TerminalCore::new(10, 10);
        core.set_scroll_region(2, 7);
        for row in 2..=7 {
            core.set_cell(0, row, &format!("{}", row), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.handle_scroll_down(2);
        // Top rows cleared
        assert_eq!(core.get_cell_char(0, 2), " ");
        assert_eq!(core.get_cell_char(0, 3), " ");
        // Rows 2-5 moved to 4-7
        assert_eq!(core.get_cell_char(0, 4), "2");
        assert_eq!(core.get_cell_char(0, 7), "5");
    }

    #[test]
    fn test_scroll_down_full_screen() {
        let mut core = TerminalCore::new(10, 5);
        for row in 0..5 {
            core.set_cell(0, row, &format!("{}", row), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.handle_scroll_down(2);
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_char(0, 1), " ");
        assert_eq!(core.get_cell_char(0, 2), "0");
        assert_eq!(core.get_cell_char(0, 4), "2");
    }

    #[test]
    fn test_decstbm_basic() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(10, 15);
        core.wrap_pending = true;
        core.handle_decstbm(5, 20);
        assert_eq!(core.get_scroll_region_top(), 4); // 1-indexed to 0-indexed
        assert_eq!(core.get_scroll_region_bottom(), 19);
        assert_eq!(core.get_cursor_col(), 0);
        assert_eq!(core.get_cursor_row(), 0);
        assert!(!core.get_wrap_pending());
    }

    #[test]
    fn test_decstbm_defaults() {
        let mut core = TerminalCore::new(80, 24);
        core.set_scroll_region(5, 15);
        core.handle_decstbm(0, 0);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 23);
    }

    #[test]
    fn test_decstbm_invalid() {
        let mut core = TerminalCore::new(80, 24);
        // top >= bottom should reset to full screen
        core.handle_decstbm(20, 5);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 23);
    }

    // ── Sprint 4: Mode Tests ────────────────────────────────

    #[test]
    fn test_mode_boolean_autowrap() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(7, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        let code = core.handle_set_mode(7, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_AUTO_WRAP));
    }

    #[test]
    fn test_mode_boolean_cursor_visible() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(25, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_CURSOR_VISIBLE));
    }

    #[test]
    fn test_mode_boolean_origin() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(6, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_ORIGIN));
    }

    #[test]
    fn test_mode_buffer_switch_47() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(47, true), 1); // switchToAlt
        assert_eq!(core.handle_set_mode(47, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_buffer_switch_1049() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(1049, true), 2); // saveAndSwitchToAlt
        assert_eq!(core.handle_set_mode(1049, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_save_restore_cursor_1048() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(1048, true), 4); // saveCursor
        assert_eq!(core.handle_set_mode(1048, false), 5); // restoreCursor
    }

    #[test]
    fn test_mode_ts_fallback() {
        let mut core = TerminalCore::new(80, 24);
        for mode in [1, 1000, 1002, 1003, 1005, 1006] {
            assert_eq!(
                core.handle_set_mode(mode, true),
                0xFF,
                "Mode {} should fallback",
                mode
            );
        }
        // 1004 and 2004 are boolean modes handled in WASM
        assert_eq!(core.handle_set_mode(1004, true), 0);
        assert!(core.get_mode(MODE_FOCUS_TRACKING));
        assert_eq!(core.handle_set_mode(2004, true), 0);
        assert!(core.get_mode(MODE_BRACKETED_PASTE));
    }

    #[test]
    fn test_mode_unknown() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(9999, true), 0);
    }

    // ── Sprint 4: Device Response Tests ─────────────────────

    #[test]
    fn test_dsr_ok_status() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_device_status_report(5);
        assert_eq!(len, 4);
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[0n");
    }

    #[test]
    fn test_dsr_cursor_position_home() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(0, 0);
        let len = core.handle_device_status_report(6);
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[1;1R");
    }

    #[test]
    fn test_dsr_cursor_position_nonzero() {
        let mut core = TerminalCore::new(80, 24);
        core.set_cursor(79, 23);
        let len = core.handle_device_status_report(6);
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[24;80R");
    }

    #[test]
    fn test_dsr_unknown() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_device_status_report(0);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_da1() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_primary_device_attributes();
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[?64;1;2;6;22c");
    }

    #[test]
    fn test_da2() {
        let mut core = TerminalCore::new(80, 24);
        let len = core.handle_secondary_device_attributes();
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[>41;1;0c");
    }

    #[test]
    fn test_response_ptr_len() {
        let mut core = TerminalCore::new(80, 24);
        core.handle_primary_device_attributes();
        let ptr = core.get_response_ptr();
        let len = core.get_response_len();
        assert!(!ptr.is_null());
        assert!(len > 0);
    }

    #[test]
    fn test_dsr_large_position() {
        let mut core = TerminalCore::new(1000, 1000);
        core.set_cursor(999, 999);
        let len = core.handle_device_status_report(6);
        let resp = &core.response_buffer[..len as usize];
        assert_eq!(resp, b"\x1b[1000;1000R");
    }
}
