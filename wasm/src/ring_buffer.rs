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
/// Currently unused: scroll optimization is disabled for diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
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
            let offset = abs * self.cols as usize + col as usize;
            // Defensive: verify offset is within ring_cells bounds
            if offset < self.ring_cells.len() {
                Some(offset)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get the base offset for a viewport row in ring_cells.
    /// Returns 0 as fallback if the computed offset exceeds ring_cells bounds.
    #[inline]
    pub(crate) fn viewport_row_base(&self, row: u16) -> usize {
        let abs = self.viewport_abs(row);
        let base = abs * self.cols as usize;
        if base + (self.cols as usize) > self.ring_cells.len() {
            return 0;
        }
        base
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
    /// The `bg` parameter specifies the background color for new cells (BCE).
    pub(crate) fn ring_push_blank(&mut self, bg: PackedColor) {
        let cols = self.cols as usize;

        // Compute new_abs speculatively before mutating state
        let (new_abs, grow) = if self.ring_size < self.ring_capacity {
            ((self.ring_head + self.ring_size) % self.ring_capacity, true)
        } else {
            (self.ring_head, false)
        };

        // Defensive: verify bounds BEFORE committing state changes
        let base = new_abs * cols;
        if base + cols > self.ring_cells.len() {
            // Invariant violation: ring_cells is too small for the computed index.
            // Do NOT mutate ring_size/ring_head to keep state consistent.
            return;
        }

        // Commit state mutation only after bounds check passes
        if grow {
            self.ring_size += 1;
        } else {
            self.ring_head = (self.ring_head + 1) % self.ring_capacity;
        }
        let slice = &mut self.ring_cells[base..base + cols];

        // Fast path: default bg → zero memory then set only the 3 non-zero bytes per cell.
        // WASM memory.fill is very efficient for bulk zeroing.
        if bg == PackedColor::DEFAULT {
            unsafe {
                std::ptr::write_bytes(slice.as_mut_ptr(), 0, cols);
            }
            for cell in slice.iter_mut() {
                cell.char_data[0] = b' ';
                cell.char_len = 1;
                cell.width = 1;
            }
        } else {
            let mut bce = Cell::EMPTY;
            bce.bg = bg;
            slice.fill(bce);
        }

        self.ring_wrapped[new_abs] = false;
        if !self.overflow.is_empty() {
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
            let bg = self.cursor.bg;
            for _ in 0..count {
                self.ring_push_blank(bg);
            }
            // TODO: Scroll optimization disabled for diagnosis.
            // Always fall back to full redraw to isolate rendering issues.
            self.scroll_event = None;
            self.mark_all_dirty();
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
        if base + cols > self.ring_cells.len() {
            // Invariant violation: return empty row data
            web_sys::console::warn_1(
                &format!(
                    "[WARN][WASM] pack_row_abs invariant violation: abs={}, cols={}, base+cols={}, ring_cells.len={}, capacity={}, ring_head={}",
                    abs, cols, base + cols, self.ring_cells.len(), self.ring_cells.len() / cols, self.ring_head
                )
                .into(),
            );
            return Vec::new();
        }
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

#[cfg(test)]
mod tests {
    use crate::cell::PackedColor;
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
        core.ring_push_blank(PackedColor::DEFAULT);
        assert_eq!(core.scrollback_count(), 1);
        assert_eq!(core.ring_size, 4);
        // Old row 0 ("A") is now in scrollback, viewport row 0 is old row 1
        assert_eq!(core.get_cell_char(0, 0), " "); // old row 1 was empty
    }

    #[test]
    fn test_ring_push_blank_at_capacity_evicts() {
        let mut core = TerminalCore::new(10, 3, 2);
        // capacity=5, fill to capacity
        core.ring_push_blank(PackedColor::DEFAULT); // size=4, scrollback=1
        core.ring_push_blank(PackedColor::DEFAULT); // size=5, scrollback=2 (at capacity)
        assert_eq!(core.ring_size, 5);
        assert_eq!(core.scrollback_count(), 2);
        // Next push should evict oldest
        core.ring_push_blank(PackedColor::DEFAULT);
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

    #[test]
    fn test_ring_push_blank_clears_ridx() {
        let mut core = TerminalCore::new(10, 3, 2); // 2 scrollback lines
        let long = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert!(!core.overflow_ridx.is_empty());

        // Push enough blanks to evict the overflow row
        for _ in 0..5 {
            core.ring_push_blank(PackedColor::DEFAULT);
        }
        // The original row should have been evicted
        assert!(core.overflow.is_empty());
        assert!(core.overflow_ridx.is_empty());
    }

    // ── Scroll event tests ──────────────────────────────────

    #[test]
    fn test_scroll_up_full_screen_count1_marks_all_dirty() {
        // Scroll optimization is disabled: full-screen scroll always marks
        // all rows dirty instead of emitting a scroll event.
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();
        assert!(core.scroll_event.is_none());

        core.scroll_up_internal(1);

        // No scroll event (optimization disabled)
        assert!(core.scroll_event.is_none());

        // All rows should be dirty
        assert!(core.is_row_dirty(0));
        assert!(core.is_row_dirty(12));
        assert!(core.is_row_dirty(22));
        assert!(core.is_row_dirty(23));
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

        // Scroll optimization is disabled, so scroll_up_internal does not
        // emit a scroll event.  Verify clear_scroll_event is safe on None.
        core.scroll_up_internal(1);
        assert!(core.scroll_event.is_none());
        assert_eq!(core.get_scroll_event_direction(), 0);
        assert_eq!(core.get_scroll_event_count(), 0);

        core.clear_scroll_event();
        assert!(core.scroll_event.is_none());
        assert_eq!(core.get_scroll_event_direction(), 0);
        assert_eq!(core.get_scroll_event_count(), 0);
    }

    #[test]
    fn test_scroll_up_marks_all_dirty_regardless_of_pre_existing() {
        // Scroll optimization is disabled: mark_all_dirty overrides any
        // pre-existing dirty bits.
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.mark_row_dirty(15);
        core.mark_row_dirty(20);

        core.scroll_up_internal(1);

        assert!(core.scroll_event.is_none());
        // All rows should be dirty
        for row in 0..24 {
            assert!(core.is_row_dirty(row), "row {} should be dirty", row);
        }
    }

    #[test]
    fn test_scroll_up_marks_all_dirty_with_row0() {
        // Scroll optimization is disabled: all rows become dirty after scroll
        // regardless of pre-existing state.
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.mark_row_dirty(0);
        core.mark_row_dirty(10);

        core.scroll_up_internal(1);

        // All rows should be dirty
        for row in 0..24 {
            assert!(core.is_row_dirty(row), "row {} should be dirty", row);
        }
    }

    #[test]
    fn test_shift_dirty_down_by_one_across_word_boundary() {
        // Test that bits shift correctly across u64 word boundaries
        let mut core = TerminalCore::new(80, 128, 100); // 128 rows = 2 u64 words
        core.clear_dirty();

        // Mark row 64 dirty (first bit of second word)
        core.mark_row_dirty(64);

        core.shift_dirty_down_by_one();

        // Should shift to row 63 (last bit of first word)
        assert!(
            core.is_row_dirty(63),
            "row 63 should be dirty (shifted from 64)"
        );
        assert!(
            !core.is_row_dirty(64),
            "row 64 should not be dirty (shifted to 63)"
        );
    }

    // ── BCE scroll tests ────────────────────────────────────

    #[test]
    fn test_bce_ring_push_blank() {
        let mut core = TerminalCore::new(10, 3, 5);
        let green = PackedColor::indexed(2);
        core.ring_push_blank(green);
        // The new blank line is now the last viewport row (row 2)
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
            assert_eq!(bg, green, "col {col}");
        }
    }

    #[test]
    fn test_bce_scroll_up_full_screen() {
        let mut core = TerminalCore::new(10, 3, 5);
        for r in 0..3 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_cursor_bg(1, 2, 0, 0); // green
        core.scroll_up_internal(1);
        // New bottom row (row 2) should have green bg
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
            assert_eq!(bg, PackedColor::indexed(2), "col {col}");
        }
    }

    #[test]
    fn test_bce_scroll_down() {
        let mut core = TerminalCore::new(10, 3, 0);
        for r in 0..3 {
            for c in 0..10 {
                core.set_cell_ascii(c, r, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
            }
        }
        core.set_cursor_bg(1, 2, 0, 0); // green
        core.scroll_down_internal(1);
        // New top row (row 0) should have green bg (via shift_rows_down BCE)
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
            assert_eq!(bg, PackedColor::indexed(2), "col {col}");
        }
    }

    #[test]
    fn test_bce_ring_push_blank_default() {
        let mut core = TerminalCore::new(10, 3, 5);
        core.ring_push_blank(PackedColor::DEFAULT);
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
            assert_eq!(bg, PackedColor::DEFAULT);
        }
    }
}
