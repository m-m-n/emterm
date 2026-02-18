/// Ring Buffer operations for TerminalCore.
///
/// The ring buffer unifies viewport and scrollback into a single flat array.
/// Viewport = last `rows` lines, scrollback = lines before viewport.
///
/// Layout (capacity = scrollback_lines + rows):
/// ```text
/// ring_cells: [... scrollback ... | ... viewport ...]
///              ^ring_head (oldest)   (last N rows)
/// ```
///
/// Invariants:
/// - ring_size >= rows (viewport always fully populated)
/// - ring_size <= ring_capacity
/// - ring_head is index of oldest line (0 ≤ ring_head < ring_capacity)
/// - Scrollback count = ring_size - rows (≥ 0)
use crate::cell::*;
use crate::terminal_core::TerminalCore;

// ── Scroll Event ─────────────────────────────────────────

/// Direction of a scroll event for differential rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollDirection {
    Up,
}

/// Scroll event emitted by full-screen scroll for differential Canvas rendering.
/// Only emitted when full-screen scroll with count=1 (the common case).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollEvent {
    pub(crate) direction: ScrollDirection,
    pub(crate) count: u16,
}

impl TerminalCore {
    // ── Ring buffer index mapping ────────────────────────

    /// Map viewport row (0-based) to absolute ring line index.
    #[inline]
    pub(crate) fn viewport_abs(&self, row: u16) -> usize {
        (self.ring_head + self.ring_size - self.rows as usize + row as usize) % self.ring_capacity
    }

    /// Map scrollback index (0 = oldest) to absolute ring line index.
    #[inline]
    pub(crate) fn scrollback_abs(&self, index: usize) -> usize {
        (self.ring_head + index) % self.ring_capacity
    }

    /// Compute cell offset in ring_cells from absolute line index and column.
    #[cfg(test)]
    pub(crate) fn ring_cell_offset(&self, abs_line: usize, col: u16) -> usize {
        abs_line * self.cols as usize + col as usize
    }

    /// Map viewport (col, row) to cell offset in ring_cells.
    /// Returns None if out of bounds.
    #[inline]
    pub(crate) fn viewport_cell_offset(&self, col: u16, row: u16) -> Option<usize> {
        if col < self.cols && row < self.rows {
            let abs = self.viewport_abs(row);
            Some(abs * self.cols as usize + col as usize)
        } else {
            None
        }
    }

    /// Get the base offset for a viewport row in ring_cells.
    #[inline]
    pub(crate) fn viewport_row_base(&self, row: u16) -> usize {
        let abs = self.viewport_abs(row);
        abs * self.cols as usize
    }

    /// Get the number of scrollback lines.
    #[inline]
    pub(crate) fn scrollback_count(&self) -> usize {
        self.ring_size.saturating_sub(self.rows as usize)
    }

    // ── Ring buffer scroll operations ─────────────────────

    /// Push a blank line at the end of the ring buffer.
    /// If below capacity, ring_size grows (scrollback expands).
    /// If at capacity, ring_head advances (oldest scrollback evicted).
    pub(crate) fn ring_push_blank(&mut self) {
        let cols = self.cols as usize;
        if self.ring_size < self.ring_capacity {
            let new_abs = (self.ring_head + self.ring_size) % self.ring_capacity;
            let base = new_abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = Cell::EMPTY;
            }
            self.ring_wrapped[new_abs] = false;
            let abs32 = new_abs as u32;
            overflow_clear_row(&mut self.overflow, abs32);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs32);
            self.ring_size += 1;
        } else {
            // Reuse oldest line's slot
            let new_abs = self.ring_head;
            self.ring_head = (self.ring_head + 1) % self.ring_capacity;
            let base = new_abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = Cell::EMPTY;
            }
            self.ring_wrapped[new_abs] = false;
            let abs32 = new_abs as u32;
            overflow_clear_row(&mut self.overflow, abs32);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs32);
        }
    }

    /// Scroll up internally (WASM-internal, no TS bridge).
    /// Full screen: pushes top line(s) to scrollback via ring_push_blank.
    /// Scroll region: shifts rows within region only.
    ///
    /// For full-screen scroll with count=1 (the common case), emits a
    /// ScrollEvent and marks only the last row dirty instead of all rows.
    /// The frontend can use the scroll event to shift the canvas content
    /// and draw only the new row.
    pub(crate) fn scroll_up_internal(&mut self, count: u16) {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        let is_full_screen = top == 0 && bottom == self.rows.saturating_sub(1);
        let count = count.min(bottom - top + 1);

        if is_full_screen {
            for _ in 0..count {
                self.ring_push_blank();
            }
            if count == 1 && self.scroll_event.is_none() {
                // Differential rendering: single scroll with no pending event
                self.scroll_event = Some(ScrollEvent {
                    direction: ScrollDirection::Up,
                    count,
                });
                self.mark_row_dirty(bottom);
            } else {
                // Fallback: multi-scroll, count > 1, or pending event exists
                self.scroll_event = None;
                self.mark_all_dirty();
            }
        } else {
            self.shift_rows_up(top, bottom, count);
        }
    }

    /// Scroll down internally. No scrollback interaction.
    pub(crate) fn scroll_down_internal(&mut self, count: u16) {
        let top = self.scroll_region_top;
        let bottom = self.scroll_region_bottom;
        let count = count.min(bottom - top + 1);
        self.shift_rows_down(top, bottom, count);
    }

    // ── Internal packing helpers ─────────────────────────

    /// Pack a ring line (by absolute index) into binary format.
    /// Shared by viewport `get_row_packed` and scrollback access.
    pub(crate) fn pack_row_abs(&self, abs: usize) -> Vec<u8> {
        let cols = self.cols as usize;
        let base = abs * cols;
        let mut buf = Vec::with_capacity(cols * 12);
        for col in 0..self.cols {
            let cell = &self.ring_cells[base + col as usize];
            if cell.is_overflow() {
                let s = self
                    .overflow
                    .get(&(col as u32, abs as u32))
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

    /// Get text content of a ring line by absolute index.
    /// Shared by viewport `get_line_text` and scrollback access.
    pub(crate) fn line_text_abs(&self, abs: usize) -> String {
        let cols = self.cols as usize;
        let base = abs * cols;
        let mut text = String::new();
        for col in 0..self.cols {
            let cell = &self.ring_cells[base + col as usize];
            if cell.width > 0 {
                if cell.is_overflow() {
                    if let Some(s) = self.overflow.get(&(col as u32, abs as u32)) {
                        text.push_str(s);
                    }
                } else if let Some(s) = cell.get_char_inline() {
                    text.push_str(s);
                }
            }
        }
        text
    }

    // ── Scrollback access APIs (internal) ──────────────────

    /// Get scrollback line in packed binary format (same as get_row_packed).
    /// index: 0 = oldest scrollback line.
    /// Returns empty vec if index >= scrollback_count.
    pub(crate) fn scrollback_row_packed(&self, index: usize) -> Vec<u8> {
        if index >= self.scrollback_count() {
            return Vec::new();
        }
        let abs = self.scrollback_abs(index);
        self.pack_row_abs(abs)
    }

    /// Get scrollback line as text (trimmed of trailing whitespace).
    /// index: 0 = oldest scrollback line.
    /// Returns empty string if index >= scrollback_count.
    pub(crate) fn scrollback_text(&self, index: usize) -> String {
        if index >= self.scrollback_count() {
            return String::new();
        }
        let abs = self.scrollback_abs(index);
        let text = self.line_text_abs(abs);
        text.trim_end().to_string()
    }
}

// ── wasm_bindgen exports for scrollback ──────────────────

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl TerminalCore {
    /// Get the number of scrollback lines.
    pub fn get_scrollback_length(&self) -> u32 {
        self.scrollback_count() as u32
    }

    /// Get a scrollback line in packed binary format.
    /// index: 0 = oldest line.
    pub fn get_scrollback_row_packed(&self, index: u32) -> Vec<u8> {
        self.scrollback_row_packed(index as usize)
    }

    /// Get a scrollback line as text (trimmed).
    /// index: 0 = oldest line.
    pub fn get_scrollback_text(&self, index: u32) -> String {
        self.scrollback_text(index as usize)
    }

    /// Get the wrapped flag for a scrollback line.
    /// index: 0 = oldest line.
    pub fn get_scrollback_line_wrapped(&self, index: u32) -> bool {
        let sb_count = self.scrollback_count();
        let i = index as usize;
        if i >= sb_count {
            return false;
        }
        let abs = self.scrollback_abs(i);
        self.ring_wrapped[abs]
    }

    /// Clear scrollback buffer, retaining only viewport lines.
    /// Used by ED 3 (Erase Scrollback).
    pub fn clear_scrollback(&mut self) {
        let sb_count = self.scrollback_count();
        if sb_count == 0 {
            return;
        }
        // Compact: move viewport rows to the beginning of the ring
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let new_cells = {
            let mut cells = vec![Cell::EMPTY; self.ring_capacity * cols];
            for r in 0..rows {
                let old_abs = self.viewport_abs(r as u16);
                let old_base = old_abs * cols;
                let new_base = r * cols;
                cells[new_base..new_base + cols]
                    .copy_from_slice(&self.ring_cells[old_base..old_base + cols]);
            }
            cells
        };
        let new_wrapped = {
            let mut wrapped = vec![false; self.ring_capacity];
            for r in 0..rows {
                let old_abs = self.viewport_abs(r as u16);
                wrapped[r] = self.ring_wrapped[old_abs];
            }
            wrapped
        };
        self.ring_cells = new_cells;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = rows;
        self.mark_all_dirty();
    }

    /// Resize with reflow. Returns packed cursor: (col << 16) | row.
    /// scrollback_lines is the new scrollback capacity.
    pub fn resize_reflow(&mut self, new_cols: u16, new_rows: u16, scrollback_lines: u32) -> u32 {
        debug_assert!(new_cols > 0 && new_rows > 0);
        let cursor_col = self.cursor.col as usize;
        let cursor_row = self.cursor.row as usize;

        let (final_col, final_row) = if new_cols == self.cols {
            // Same width: adjust row count only
            self.resize_same_width(new_rows, scrollback_lines, cursor_col, cursor_row)
        } else {
            // Different width: full reflow
            self.resize_full_reflow(new_cols, new_rows, scrollback_lines, cursor_col, cursor_row)
        };

        // Common post-resize cleanup
        self.resize_post_cleanup(new_cols, new_rows);

        // Set cursor
        self.cursor.col = (final_col as u16).min(new_cols.saturating_sub(1));
        self.cursor.row = (final_row as u16).min(new_rows.saturating_sub(1));

        ((self.cursor.col as u32) << 16) | (self.cursor.row as u32)
    }

    /// Simple resize without reflow (for alternate buffer).
    pub fn resize_no_reflow(&mut self, new_cols: u16, new_rows: u16) {
        debug_assert!(new_cols > 0 && new_rows > 0);
        let old_cols = self.cols;
        let old_rows = self.rows;
        let new_capacity = new_rows as usize;
        let new_total = new_capacity * new_cols as usize;

        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_capacity];
        let copy_rows = old_rows.min(new_rows);
        let copy_cols = old_cols.min(new_cols);

        for row in 0..copy_rows {
            let old_abs = self.viewport_abs(row);
            let old_base = old_abs * old_cols as usize;
            let new_base = row as usize * new_cols as usize;
            for col in 0..copy_cols as usize {
                new_grid[new_base + col] = self.ring_cells[old_base + col];
            }
            new_wrapped[row as usize] = self.ring_wrapped[old_abs];
        }

        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = new_capacity;
        self.ring_capacity = new_capacity;

        self.resize_post_cleanup(new_cols, new_rows);

        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
    }
}

// ── Reflow internals ────────────────────────────────────

/// A physical line extracted from ring buffer.
struct PhysicalLine {
    cells: Vec<Cell>,
    /// Overflow strings per column (None = inline, Some = overflow data).
    overflow_data: Vec<Option<String>>,
    wrapped: bool,
}

/// A logical line = joined wrapped physical lines.
struct LogicalLine {
    cells: Vec<Cell>,
    /// Overflow strings per column (None = inline, Some = overflow data).
    overflow_data: Vec<Option<String>>,
}

impl TerminalCore {
    /// Same-width resize: just adjust row count.
    fn resize_same_width(
        &mut self,
        new_rows: u16,
        scrollback_lines: u32,
        cursor_col: usize,
        cursor_row: usize,
    ) -> (usize, usize) {
        let new_cap = scrollback_lines as usize + new_rows as usize;
        let new_rows_usize = new_rows as usize;
        let old_rows = self.rows as usize;
        let cols = self.cols as usize;

        // Drain all lines from old ring
        let lines = self.reflow_drain();
        let total_lines = lines.len();

        // Cursor's absolute position in drained lines
        let cursor_abs = total_lines.saturating_sub(old_rows) + cursor_row;

        // How many lines to keep: at most new_cap, at most total_lines
        let keep = total_lines.min(new_cap);

        // Compute skip to keep cursor visible in viewport.
        // Cursor's desired viewport row: min(cursor_row, new_rows - 1).
        let desired_vp_row = cursor_row.min(new_rows_usize.saturating_sub(1));
        // vp_start in kept range = keep - new_rows_usize
        // cursor must be at: skip + vp_start + desired_vp_row = cursor_abs
        // skip = cursor_abs - (keep - new_rows_usize) - desired_vp_row
        let skip = if total_lines <= new_cap {
            0 // All lines fit, no skip needed
        } else {
            let ideal = cursor_abs
                .saturating_sub(keep.saturating_sub(new_rows_usize))
                .saturating_sub(desired_vp_row);
            // Clamp: skip can't exceed total - keep, and can't be negative
            ideal.min(total_lines.saturating_sub(keep))
        };

        // New ring
        let new_total = new_cap * cols;
        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_cap];

        let mut new_overflow = OverflowTable::new();
        for (i, line) in lines.iter().skip(skip).take(keep).enumerate() {
            let base = i * cols;
            let copy_len = line.cells.len().min(cols);
            for c in 0..copy_len {
                new_grid[base + c] = line.cells[c];
                if let Some(Some(ref s)) = line.overflow_data.get(c) {
                    new_overflow.insert((c as u32, i as u32), s.clone());
                }
            }
            new_wrapped[i] = line.wrapped;
        }

        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        // Ensure ring_size >= new_rows (invariant)
        let actual_size = keep.max(new_rows_usize);
        self.ring_size = actual_size;
        self.ring_capacity = new_cap;
        self.overflow = new_overflow;
        self.overflow_ridx = overflow_ridx_rebuild(&self.overflow);

        // Cursor tracking
        let cursor_abs_new = cursor_abs.saturating_sub(skip);
        let vp_start = actual_size.saturating_sub(new_rows_usize);
        let new_cursor_row = cursor_abs_new.saturating_sub(vp_start);
        (cursor_col, new_cursor_row)
    }

    /// Full reflow for width change.
    fn resize_full_reflow(
        &mut self,
        new_cols: u16,
        new_rows: u16,
        scrollback_lines: u32,
        cursor_col: usize,
        cursor_row: usize,
    ) -> (usize, usize) {
        let old_cols = self.cols as usize;
        let old_rows = self.rows as usize;
        let new_cols_usize = new_cols as usize;
        let new_cap = scrollback_lines as usize + new_rows as usize;

        // 1. Drain all lines
        let phys_lines = self.reflow_drain();
        let total_phys = phys_lines.len();

        // Calculate cursor absolute position in physical lines
        let vp_start = total_phys.saturating_sub(old_rows);
        let cursor_phys_abs = vp_start + cursor_row;

        // 2. Join wrapped lines into logical lines, tracking cursor
        let (logical_lines, cursor_logical_idx, cursor_logical_col) =
            Self::reflow_join_wrapped(&phys_lines, cursor_phys_abs, cursor_col, old_cols);

        // 3. Split logical lines at new width, tracking cursor
        let (new_phys, cursor_new_phys, cursor_new_col) = Self::reflow_split_at_width(
            &logical_lines,
            new_cols_usize,
            cursor_logical_idx,
            cursor_logical_col,
        );

        // 4. Trim trailing empty lines from bottom if total exceeds capacity
        let mut keep_count = new_phys.len();
        // Trim empty lines from the bottom but never below new_rows
        while keep_count > new_rows as usize {
            let last = &new_phys[keep_count - 1];
            if last
                .cells
                .iter()
                .all(|c| c.width == 0 || (c.width == 1 && c.get_char_inline() == Some(" ")))
            {
                keep_count -= 1;
            } else {
                break;
            }
        }
        // Ensure at least new_rows lines
        keep_count = keep_count.max(new_rows as usize);
        // Cap at new capacity
        let keep = keep_count.min(new_cap);
        let skip = if keep_count > new_cap {
            keep_count - new_cap
        } else {
            0
        };

        // 5. Write to new ring buffer
        let new_total = new_cap * new_cols_usize;
        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_cap];

        let mut new_overflow = OverflowTable::new();
        for (i, line) in new_phys.iter().skip(skip).take(keep).enumerate() {
            let base = i * new_cols_usize;
            let copy_len = line.cells.len().min(new_cols_usize);
            for c in 0..copy_len {
                new_grid[base + c] = line.cells[c];
                if let Some(Some(ref s)) = line.overflow_data.get(c) {
                    new_overflow.insert((c as u32, i as u32), s.clone());
                }
            }
            new_wrapped[i] = line.wrapped;
        }

        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = keep;
        self.ring_capacity = new_cap;
        self.overflow = new_overflow;
        self.overflow_ridx = overflow_ridx_rebuild(&self.overflow);

        // 6. Track cursor
        let cursor_new_phys_adj = cursor_new_phys.saturating_sub(skip);
        let vp_start_new = keep.saturating_sub(new_rows as usize);
        let new_cursor_row = cursor_new_phys_adj.saturating_sub(vp_start_new);
        (cursor_new_col, new_cursor_row)
    }

    /// Drain all lines from ring buffer (scrollback + viewport) in order.
    /// Captures overflow strings from the overflow table for each cell.
    fn reflow_drain(&self) -> Vec<PhysicalLine> {
        let cols = self.cols as usize;
        let mut lines = Vec::with_capacity(self.ring_size);
        for i in 0..self.ring_size {
            let abs = (self.ring_head + i) % self.ring_capacity;
            let abs32 = abs as u32;
            let base = abs * cols;
            let mut cells = Vec::with_capacity(cols);
            let mut overflow_data = Vec::with_capacity(cols);
            for c in 0..cols {
                let cell = self.ring_cells[base + c];
                if cell.is_overflow() {
                    overflow_data.push(self.overflow.get(&(c as u32, abs32)).cloned());
                } else {
                    overflow_data.push(None);
                }
                cells.push(cell);
            }
            lines.push(PhysicalLine {
                cells,
                overflow_data,
                wrapped: self.ring_wrapped[abs],
            });
        }
        lines
    }

    /// Join consecutive wrapped physical lines into logical lines.
    /// Uses backward-reference semantics: wrapped=true on a line means
    /// "this line is a continuation of the previous line" (matching print handler).
    /// Returns (logical_lines, cursor_logical_index, cursor_col_in_logical_line).
    fn reflow_join_wrapped(
        phys_lines: &[PhysicalLine],
        cursor_phys: usize,
        cursor_col: usize,
        old_cols: usize,
    ) -> (Vec<LogicalLine>, usize, usize) {
        let mut logical: Vec<LogicalLine> = Vec::new();
        let mut cursor_logical_idx = 0;
        let mut cursor_logical_col = cursor_col;

        let mut i = 0;
        while i < phys_lines.len() {
            let mut cells = Vec::new();
            let mut oflow = Vec::new();
            let start_i = i;

            loop {
                let line_cells = &phys_lines[i].cells;
                let line_oflow = &phys_lines[i].overflow_data;
                // Check if next line is a continuation (backward ref: wrapped on continuation)
                let next_is_continuation = i + 1 < phys_lines.len() && phys_lines[i + 1].wrapped;
                let trimmed_len = if next_is_continuation {
                    // More continuation follows: keep full width
                    line_cells.len()
                } else {
                    // Last line of logical group: trim trailing spaces
                    let mut len = line_cells.len();
                    while len > 0 {
                        let c = &line_cells[len - 1];
                        if c.width == 0 || (c.width == 1 && c.get_char_inline() == Some(" ")) {
                            len -= 1;
                        } else {
                            break;
                        }
                    }
                    len
                };

                // Track cursor
                if i == cursor_phys {
                    cursor_logical_idx = logical.len();
                    cursor_logical_col = cells.len() + cursor_col;
                }

                cells.extend_from_slice(&line_cells[..trimmed_len]);
                oflow.extend_from_slice(&line_oflow[..trimmed_len]);

                i += 1;
                if !next_is_continuation {
                    break;
                }
            }

            // If cursor was on a later physical line in this group
            for j in start_i + 1..i {
                if j == cursor_phys {
                    cursor_logical_idx = logical.len();
                    cursor_logical_col = (j - start_i) * old_cols + cursor_col;
                }
            }

            logical.push(LogicalLine {
                cells,
                overflow_data: oflow,
            });
        }

        (logical, cursor_logical_idx, cursor_logical_col)
    }

    /// Re-split logical lines at new column width.
    /// Uses backward-reference semantics: wrapped=true on a line means
    /// "this line is a continuation of the previous line" (matching print handler).
    /// Returns (physical_lines, cursor_phys_index, cursor_col).
    fn reflow_split_at_width(
        logical_lines: &[LogicalLine],
        new_cols: usize,
        cursor_logical_idx: usize,
        cursor_logical_col: usize,
    ) -> (Vec<PhysicalLine>, usize, usize) {
        let mut phys: Vec<PhysicalLine> = Vec::new();
        let mut cursor_phys = 0;
        let mut cursor_col = 0;

        for (li, logical) in logical_lines.iter().enumerate() {
            let cells = &logical.cells;
            let oflow = &logical.overflow_data;
            let first_phys_of_group = phys.len();

            if cells.is_empty() {
                // Track cursor for empty logical line
                if li == cursor_logical_idx {
                    cursor_phys = phys.len();
                    cursor_col = 0;
                }
                phys.push(PhysicalLine {
                    cells: vec![Cell::EMPTY; new_cols],
                    overflow_data: vec![None; new_cols],
                    wrapped: false,
                });
                continue;
            }

            let mut col = 0; // current column in physical line
            let mut line_cells = Vec::with_capacity(new_cols);
            let mut line_oflow = Vec::with_capacity(new_cols);
            let mut ci = 0; // cell index in logical line

            while ci < cells.len() {
                let cell = &cells[ci];
                let w = cell.width.max(1) as usize;

                // Wide char at last column: pad and wrap
                if w == 2 && col == new_cols - 1 {
                    line_cells.push(Cell::EMPTY);
                    line_oflow.push(None);
                    // Flush line
                    while line_cells.len() < new_cols {
                        line_cells.push(Cell::EMPTY);
                        line_oflow.push(None);
                    }
                    if li == cursor_logical_idx && cursor_logical_col >= ci {
                        // Cursor was at or after this position in logical line
                        // Will be placed on next physical line
                    }
                    // Backward ref: continuation lines have wrapped=true
                    let is_continuation = phys.len() > first_phys_of_group;
                    phys.push(PhysicalLine {
                        cells: line_cells,
                        overflow_data: line_oflow,
                        wrapped: is_continuation,
                    });
                    line_cells = Vec::with_capacity(new_cols);
                    line_oflow = Vec::with_capacity(new_cols);
                    col = 0;
                    continue; // re-process this cell
                }

                // Track cursor
                if li == cursor_logical_idx && ci == cursor_logical_col {
                    cursor_phys = phys.len();
                    cursor_col = col;
                }

                line_cells.push(*cell);
                line_oflow.push(oflow.get(ci).cloned().flatten());
                col += 1;

                // Add placeholder for wide char
                if w == 2 && ci + 1 < cells.len() && cells[ci + 1].width == 0 {
                    line_cells.push(cells[ci + 1]);
                    line_oflow.push(None);
                    col += 1;
                    ci += 1;
                }
                ci += 1;

                // Line full: wrap
                if col >= new_cols && ci < cells.len() {
                    while line_cells.len() < new_cols {
                        line_cells.push(Cell::EMPTY);
                        line_oflow.push(None);
                    }
                    // Backward ref: continuation lines have wrapped=true
                    let is_continuation = phys.len() > first_phys_of_group;
                    phys.push(PhysicalLine {
                        cells: line_cells,
                        overflow_data: line_oflow,
                        wrapped: is_continuation,
                    });
                    line_cells = Vec::with_capacity(new_cols);
                    line_oflow = Vec::with_capacity(new_cols);
                    col = 0;
                }
            }

            // Track cursor if it's past end of logical line content
            if li == cursor_logical_idx && cursor_logical_col >= cells.len() {
                cursor_phys = phys.len();
                cursor_col = col.min(new_cols.saturating_sub(1));
            }

            // Pad and flush last physical line
            while line_cells.len() < new_cols {
                line_cells.push(Cell::EMPTY);
                line_oflow.push(None);
            }
            // Backward ref: continuation lines have wrapped=true
            let is_continuation = phys.len() > first_phys_of_group;
            phys.push(PhysicalLine {
                cells: line_cells,
                overflow_data: line_oflow,
                wrapped: is_continuation,
            });
        }

        (phys, cursor_phys, cursor_col)
    }

    /// Common post-resize cleanup.
    fn resize_post_cleanup(&mut self, new_cols: u16, new_rows: u16) {
        let old_cols = self.cols;

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

        // Resize dirty bitset and mark all dirty
        let dirty_words = (new_rows as usize + 63) / 64;
        self.dirty = vec![0; dirty_words];
        self.mark_all_dirty();

        // Overflow is now rebuilt by resize_same_width / resize_full_reflow;
        // no need to clear here.

        // Reset scroll region and print state
        self.scroll_region_top = 0;
        self.scroll_region_bottom = new_rows.saturating_sub(1);
        self.wrap_pending = false;
        self.grapheme_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Ring buffer index mapping tests ──────────────────

    #[test]
    fn test_viewport_abs_no_scrollback() {
        // With scrollback_lines=0: ring_capacity=rows, ring_head=0, ring_size=rows
        // viewport_abs(r) = (0 + rows - rows + r) % rows = r
        let core = TerminalCore::new(80, 24, 0);
        for r in 0..24u16 {
            assert_eq!(core.viewport_abs(r), r as usize);
        }
    }

    #[test]
    fn test_viewport_abs_with_scrollback_capacity() {
        // scrollback_lines=100: ring_capacity=124, ring_head=0, ring_size=24
        // viewport_abs(r) = (0 + 24 - 24 + r) % 124 = r
        let core = TerminalCore::new(80, 24, 100);
        for r in 0..24u16 {
            assert_eq!(core.viewport_abs(r), r as usize);
        }
    }

    #[test]
    fn test_scrollback_abs_basic() {
        let core = TerminalCore::new(80, 24, 100);
        // Initially no scrollback (ring_size == rows), but mapping still works
        assert_eq!(core.scrollback_abs(0), 0);
        assert_eq!(core.scrollback_abs(1), 1);
    }

    #[test]
    fn test_viewport_cell_offset_basic() {
        let core = TerminalCore::new(80, 24, 0);
        // With no scrollback: offset = row * cols + col (same as flat grid)
        assert_eq!(core.viewport_cell_offset(0, 0), Some(0));
        assert_eq!(core.viewport_cell_offset(5, 0), Some(5));
        assert_eq!(core.viewport_cell_offset(0, 1), Some(80));
        assert_eq!(core.viewport_cell_offset(79, 23), Some(23 * 80 + 79));
    }

    #[test]
    fn test_viewport_cell_offset_oob() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.viewport_cell_offset(80, 0), None);
        assert_eq!(core.viewport_cell_offset(0, 24), None);
        assert_eq!(core.viewport_cell_offset(80, 24), None);
    }

    #[test]
    fn test_viewport_row_base_no_scrollback() {
        let core = TerminalCore::new(10, 5, 0);
        assert_eq!(core.viewport_row_base(0), 0);
        assert_eq!(core.viewport_row_base(1), 10);
        assert_eq!(core.viewport_row_base(4), 40);
    }

    #[test]
    fn test_scrollback_count_initial() {
        let core = TerminalCore::new(80, 24, 100);
        assert_eq!(core.scrollback_count(), 0);
    }

    #[test]
    fn test_scrollback_count_no_capacity() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.scrollback_count(), 0);
    }

    #[test]
    fn test_ring_cell_offset() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.ring_cell_offset(0, 0), 0);
        assert_eq!(core.ring_cell_offset(0, 5), 5);
        assert_eq!(core.ring_cell_offset(3, 0), 3 * 80);
    }

    #[test]
    fn test_constructor_with_scrollback() {
        let core = TerminalCore::new(80, 24, 1000);
        assert_eq!(core.cols(), 80);
        assert_eq!(core.rows(), 24);
        assert_eq!(core.ring_capacity, 1024); // 1000 + 24
        assert_eq!(core.ring_size, 24);
        assert_eq!(core.ring_head, 0);
    }

    #[test]
    fn test_constructor_zero_scrollback_matches_flat() {
        let core = TerminalCore::new(10, 5, 0);
        assert_eq!(core.ring_capacity, 5);
        assert_eq!(core.ring_size, 5);
        assert_eq!(core.ring_head, 0);
        // All cells should be empty
        for r in 0..5 {
            assert!(core.is_line_empty(r));
        }
    }

    // ── Ring push / scroll internal tests ─────────────────

    #[test]
    fn test_ring_push_blank_grows_scrollback() {
        let mut core = TerminalCore::new(10, 3, 5);
        // capacity=8, initial size=3
        assert_eq!(core.scrollback_count(), 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.ring_push_blank();
        assert_eq!(core.scrollback_count(), 1);
        assert_eq!(core.ring_size, 4);
        // Old row 0 ("A") is now in scrollback, viewport row 0 is old row 1
        assert_eq!(core.get_cell_char(0, 0), " "); // old row 1 was empty
    }

    #[test]
    fn test_ring_push_blank_at_capacity_evicts() {
        let mut core = TerminalCore::new(10, 3, 2);
        // capacity=5, fill to capacity
        core.ring_push_blank(); // size=4, scrollback=1
        core.ring_push_blank(); // size=5, scrollback=2 (at capacity)
        assert_eq!(core.ring_size, 5);
        assert_eq!(core.scrollback_count(), 2);
        // Next push should evict oldest
        core.ring_push_blank();
        assert_eq!(core.ring_size, 5); // stays at capacity
        assert_eq!(core.scrollback_count(), 2); // still 2 (oldest evicted, newest added)
    }

    #[test]
    fn test_scroll_up_internal_full_screen() {
        let mut core = TerminalCore::new(10, 3, 5);
        // Fill viewport
        for r in 0..3 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.scroll_up_internal(1);
        // Row 0 should now have old row 1
        assert_eq!(core.get_cell_char(0, 0), "1");
        assert_eq!(core.get_cell_char(0, 1), "2");
        assert_eq!(core.get_cell_char(0, 2), " "); // new blank line
        assert_eq!(core.scrollback_count(), 1);
    }

    #[test]
    fn test_scroll_up_internal_region() {
        let mut core = TerminalCore::new(10, 5, 5);
        for r in 0..5 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_scroll_region(1, 3);
        core.scroll_up_internal(1);
        // Row 0 unchanged
        assert_eq!(core.get_cell_char(0, 0), "0");
        // Region shifted up: row 1 = old row 2, row 2 = old row 3, row 3 = blank
        assert_eq!(core.get_cell_char(0, 1), "2");
        assert_eq!(core.get_cell_char(0, 2), "3");
        assert_eq!(core.get_cell_char(0, 3), " ");
        // Row 4 unchanged
        assert_eq!(core.get_cell_char(0, 4), "4");
        // No scrollback growth (region scroll)
        assert_eq!(core.scrollback_count(), 0);
    }

    #[test]
    fn test_scroll_down_internal() {
        let mut core = TerminalCore::new(10, 5, 5);
        for r in 0..5 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.scroll_down_internal(1);
        assert_eq!(core.get_cell_char(0, 0), " "); // new blank
        assert_eq!(core.get_cell_char(0, 1), "0");
        assert_eq!(core.get_cell_char(0, 2), "1");
        assert_eq!(core.scrollback_count(), 0); // no scrollback for scroll down
    }

    // ── Scrollback access API tests ─────────────────────

    #[test]
    fn test_get_scrollback_length_initial() {
        let core = TerminalCore::new(10, 3, 5);
        assert_eq!(core.get_scrollback_length(), 0);
    }

    #[test]
    fn test_get_scrollback_length_after_scroll() {
        let mut core = TerminalCore::new(10, 3, 5);
        for r in 0..3 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.scroll_up_internal(1);
        assert_eq!(core.get_scrollback_length(), 1);
        core.scroll_up_internal(2);
        assert_eq!(core.get_scrollback_length(), 3);
    }

    #[test]
    fn test_get_scrollback_length_capped_at_capacity() {
        let mut core = TerminalCore::new(10, 3, 2);
        // capacity=5, scrollback max=2
        for _ in 0..10 {
            core.scroll_up_internal(1);
        }
        assert_eq!(core.get_scrollback_length(), 2);
    }

    #[test]
    fn test_get_scrollback_text_basic() {
        let mut core = TerminalCore::new(10, 3, 5);
        // Fill viewport row 0 with "Hello"
        for (i, ch) in "Hello".chars().enumerate() {
            core.set_cell(i as u16, 0, &ch.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // Scroll so row 0 goes to scrollback
        core.scroll_up_internal(1);
        assert_eq!(core.get_scrollback_length(), 1);
        assert_eq!(core.get_scrollback_text(0), "Hello");
    }

    #[test]
    fn test_get_scrollback_text_trims_trailing() {
        let mut core = TerminalCore::new(10, 3, 5);
        core.set_cell(0, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Rest of row is spaces
        core.scroll_up_internal(1);
        assert_eq!(core.get_scrollback_text(0), "X");
    }

    #[test]
    fn test_get_scrollback_text_oob_returns_empty() {
        let core = TerminalCore::new(10, 3, 5);
        assert_eq!(core.get_scrollback_text(0), "");
        assert_eq!(core.get_scrollback_text(999), "");
    }

    #[test]
    fn test_get_scrollback_row_packed_matches_viewport() {
        let mut core = TerminalCore::new(10, 3, 5);
        // Fill row 0 with "A" cells
        for c in 0..10 {
            core.set_cell_ascii(c, 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // Get packed before scrolling (viewport row 0)
        let before = core.get_row_packed(0);
        // Scroll so row 0 goes to scrollback
        core.scroll_up_internal(1);
        // Get scrollback row 0 packed
        let after = core.get_scrollback_row_packed(0);
        // Packed format should be identical
        assert_eq!(before, after);
    }

    #[test]
    fn test_get_scrollback_row_packed_oob_returns_empty() {
        let core = TerminalCore::new(10, 3, 5);
        assert!(core.get_scrollback_row_packed(0).is_empty());
        assert!(core.get_scrollback_row_packed(999).is_empty());
    }

    #[test]
    fn test_scrollback_ordering_oldest_first() {
        let mut core = TerminalCore::new(10, 3, 5);
        // Fill each viewport row with a different letter
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 1, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 2, "C", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // Scroll up 2 times: A and B go to scrollback
        core.scroll_up_internal(2);
        assert_eq!(core.get_scrollback_length(), 2);
        // index 0 = oldest = "A"
        assert_eq!(core.get_scrollback_text(0), "A");
        // index 1 = newer = "B"
        assert_eq!(core.get_scrollback_text(1), "B");
    }

    #[test]
    fn test_scrollback_eviction_oldest() {
        let mut core = TerminalCore::new(10, 3, 2);
        // capacity=5, scrollback_max=2
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1); // A in scrollback[0]
        core.set_cell(0, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1); // A=scrollback[0], B=scrollback[1]
        assert_eq!(core.get_scrollback_text(0), "A");
        assert_eq!(core.get_scrollback_text(1), "B");
        // One more scroll: A evicted, B becomes oldest
        core.set_cell(0, 0, "C", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
        assert_eq!(core.get_scrollback_length(), 2);
        assert_eq!(core.get_scrollback_text(0), "B");
        assert_eq!(core.get_scrollback_text(1), "C");
    }

    #[test]
    fn test_scroll_up_internal_full_screen_no_scrollback_capacity() {
        let mut core = TerminalCore::new(10, 3, 0);
        // scrollback_lines=0: capacity=3, same as rows
        for r in 0..3 {
            core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.scroll_up_internal(1);
        // Should scroll (evict immediately since at capacity)
        assert_eq!(core.get_cell_char(0, 0), "1");
        assert_eq!(core.get_cell_char(0, 1), "2");
        assert_eq!(core.get_cell_char(0, 2), " ");
        assert_eq!(core.scrollback_count(), 0); // no room for scrollback
    }

    // ── Reflow tests ─────────────────────────────────────

    #[test]
    fn test_resize_reflow_same_width_grow() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(2, 1);
        let packed = core.resize_reflow(10, 5, 0);
        assert_eq!(core.rows(), 5);
        assert_eq!(core.get_cell_char(0, 0), "A");
        // Cursor at same position
        let col = (packed >> 16) as u16;
        let row = (packed & 0xFFFF) as u16;
        assert_eq!(col, 2);
        assert_eq!(row, 1);
    }

    #[test]
    fn test_resize_reflow_same_width_shrink() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(0, 1, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(0, 0);
        let packed = core.resize_reflow(10, 3, 0);
        assert_eq!(core.rows(), 3);
        assert_eq!(core.get_cell_char(0, 0), "A");
        let row = (packed & 0xFFFF) as u16;
        assert_eq!(row, 0);
    }

    #[test]
    fn test_resize_reflow_wider_merges_wrapped() {
        let mut core = TerminalCore::new(5, 3, 0);
        // "ABCDE" on row 0 (5 cols)
        for (i, ch) in "ABCDE".chars().enumerate() {
            core.set_cell(i as u16, 0, &ch.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // "FGHIJ" on row 1
        for (i, ch) in "FGHIJ".chars().enumerate() {
            core.set_cell(i as u16, 1, &ch.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        // Mark row 1 as continuation of row 0 (backward ref: wrapped on continuation line)
        let abs1 = core.viewport_abs(1);
        core.ring_wrapped[abs1] = true;
        core.set_cursor(2, 1);

        // Resize to 10 cols: two wrapped lines merge into one
        let packed = core.resize_reflow(10, 3, 0);
        assert_eq!(core.cols(), 10);
        // Merged: "ABCDEFGHIJ" on row 0
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(4, 0), "E");
        assert_eq!(core.get_cell_char(5, 0), "F");
        assert_eq!(core.get_cell_char(9, 0), "J");
        // Cursor tracked: was at (2, 1) in 5-col = logical col 7
        let col = (packed >> 16) as u16;
        let row = (packed & 0xFFFF) as u16;
        assert_eq!(col, 7);
        assert_eq!(row, 0);
    }

    #[test]
    fn test_resize_reflow_narrower_splits_lines() {
        let mut core = TerminalCore::new(10, 3, 0);
        // "ABCDEFGHIJ" on row 0 (10 cols)
        for (i, ch) in "ABCDEFGHIJ".chars().enumerate() {
            core.set_cell(i as u16, 0, &ch.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        core.set_cursor(7, 0);

        // Resize to 5 cols: one line splits into two
        let packed = core.resize_reflow(5, 3, 0);
        assert_eq!(core.cols(), 5);
        // Row 0: "ABCDE"
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_char(4, 0), "E");
        // Row 1: "FGHIJ"
        assert_eq!(core.get_cell_char(0, 1), "F");
        assert_eq!(core.get_cell_char(4, 1), "J");
        // Cursor tracked: was at col 7 → logical col 7 → phys row 1, col 2
        let col = (packed >> 16) as u16;
        let row = (packed & 0xFFFF) as u16;
        assert_eq!(col, 2);
        assert_eq!(row, 1);
    }

    #[test]
    fn test_resize_reflow_with_scrollback() {
        let mut core = TerminalCore::new(10, 3, 5);
        // Fill and scroll to create scrollback
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1); // "A" goes to scrollback
        core.set_cell(0, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_scrollback_length(), 1);

        // Resize same width, scrollback preserved
        core.resize_reflow(10, 3, 5);
        assert_eq!(core.get_scrollback_text(0), "A");
        assert_eq!(core.get_cell_char(0, 0), "B");
    }

    #[test]
    fn test_resize_reflow_scroll_region_invalidated() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_scroll_region(1, 3);
        core.resize_reflow(10, 5, 0);
        assert_eq!(core.get_scroll_region_top(), 0);
        assert_eq!(core.get_scroll_region_bottom(), 4);
    }

    #[test]
    fn test_resize_no_reflow_basic() {
        let mut core = TerminalCore::new(10, 5, 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(3, 2);
        core.resize_no_reflow(15, 3);
        assert_eq!(core.cols(), 15);
        assert_eq!(core.rows(), 3);
        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cursor_col(), 3);
        assert_eq!(core.get_cursor_row(), 2);
    }

    #[test]
    fn test_resize_no_reflow_clamps_cursor() {
        let mut core = TerminalCore::new(10, 10, 0);
        core.set_cursor(8, 8);
        core.resize_no_reflow(5, 5);
        assert_eq!(core.get_cursor_col(), 4); // clamped
        assert_eq!(core.get_cursor_row(), 4); // clamped
    }

    #[test]
    fn test_resize_reflow_empty_lines_trimmed() {
        let mut core = TerminalCore::new(10, 5, 0);
        // Only row 0 has content, rows 1-4 are empty
        core.set_cell(0, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cursor(0, 0);
        // Resize narrower: shouldn't expand due to empty trailing lines
        core.resize_reflow(5, 3, 0);
        assert_eq!(core.get_cell_char(0, 0), "X");
    }

    // ── Phase 4: Reflow overflow preservation tests ──────

    #[test]
    fn test_overflow_survives_same_width_resize() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦"; // ZWJ family emoji, >16 bytes
        assert!(long.as_bytes().len() > 16);
        core.set_cell(0, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 2), long);

        // Same-width resize (row count change)
        core.set_cursor(0, 2);
        core.resize_reflow(10, 8, 0);
        // Overflow cell should survive
        assert_eq!(core.get_cell_char(0, 2), long);
        assert!(!core.overflow.is_empty());
        assert!(!core.overflow_ridx.is_empty());
    }

    #[test]
    fn test_overflow_survives_width_change_reflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), long);

        // Resize wider
        core.set_cursor(5, 0);
        core.resize_reflow(20, 5, 0);
        // Overflow cell should be preserved at new position
        assert_eq!(core.get_cell_char(0, 0), long);
        assert!(!core.overflow.is_empty());
    }

    #[test]
    fn test_overflow_survives_narrower_reflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), long);

        core.set_cursor(0, 0);
        core.resize_reflow(5, 5, 0);
        assert_eq!(core.get_cell_char(0, 0), long);
        assert!(!core.overflow.is_empty());
    }

    #[test]
    fn test_multiple_overflow_cells_survive_reflow() {
        let mut core = TerminalCore::new(20, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.set_cell(10, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(core.get_cell_char(0, 0), long);
        assert_eq!(core.get_cell_char(10, 0), long);
        assert_eq!(core.overflow.len(), 2);

        // Resize wider
        core.set_cursor(0, 0);
        core.resize_reflow(30, 5, 0);
        assert_eq!(core.get_cell_char(0, 0), long);
        assert_eq!(core.get_cell_char(10, 0), long);
        assert_eq!(core.overflow.len(), 2);
    }

    #[test]
    fn test_overflow_ridx_rebuilt_after_reflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        core.set_cursor(0, 0);
        core.resize_reflow(10, 8, 0);
        // Reverse index should be consistent with overflow table
        for &(col, row) in core.overflow.keys() {
            assert!(core.overflow_ridx.contains_key(&row));
            assert!(core.overflow_ridx[&row].contains(&col));
        }
    }

    #[test]
    fn test_ring_push_blank_clears_ridx() {
        let mut core = TerminalCore::new(10, 3, 2); // 2 scrollback lines
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.is_empty());

        // Push enough blanks to evict the overflow row
        for _ in 0..5 {
            core.ring_push_blank();
        }
        // The original row should have been evicted
        assert!(core.overflow.is_empty());
        assert!(core.overflow_ridx.is_empty());
    }

    // ── Scroll event tests ──────────────────────────────────

    #[test]
    fn test_scroll_up_full_screen_count1_emits_scroll_event() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();
        assert!(core.scroll_event.is_none());

        core.scroll_up_internal(1);

        // Should emit scroll event
        assert!(core.scroll_event.is_some());
        let evt = core.scroll_event.as_ref().unwrap();
        assert_eq!(evt.direction, super::ScrollDirection::Up);
        assert_eq!(evt.count, 1);

        // Should only mark the last row dirty (row 23)
        assert!(core.is_row_dirty(23));
        // Other rows should NOT be dirty
        assert!(!core.is_row_dirty(0));
        assert!(!core.is_row_dirty(12));
        assert!(!core.is_row_dirty(22));
    }

    #[test]
    fn test_scroll_up_full_screen_count_gt1_no_scroll_event() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.scroll_up_internal(3);

        // Should NOT emit scroll event (count > 1)
        assert!(core.scroll_event.is_none());
        // All rows should be dirty (fallback)
        assert!(core.is_row_dirty(0));
        assert!(core.is_row_dirty(12));
        assert!(core.is_row_dirty(23));
    }

    #[test]
    fn test_scroll_up_scroll_region_no_scroll_event() {
        let mut core = TerminalCore::new(80, 24, 100);
        // Set scroll region (not full screen)
        core.scroll_region_top = 5;
        core.scroll_region_bottom = 20;
        core.clear_dirty();

        core.scroll_up_internal(1);

        // Should NOT emit scroll event (scroll region, not full screen)
        assert!(core.scroll_event.is_none());
    }

    #[test]
    fn test_scroll_event_cleared_correctly() {
        let mut core = TerminalCore::new(80, 24, 100);

        core.scroll_up_internal(1);
        assert!(core.scroll_event.is_some());
        assert_eq!(core.get_scroll_event_direction(), 1);
        assert_eq!(core.get_scroll_event_count(), 1);

        core.clear_scroll_event();
        assert!(core.scroll_event.is_none());
        assert_eq!(core.get_scroll_event_direction(), 0);
        assert_eq!(core.get_scroll_event_count(), 0);
    }
}
