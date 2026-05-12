/// Ring Buffer + scrollback storage for `TerminalCore`.
///
/// The viewport is held in a flat `Vec<Cell>` of length `rows × cols` and
/// rotated via `ring_head` so that scrolling does not have to copy cells.
///
/// Scrollback lives in a separate compressed deque
/// (`scrollback_slim: VecDeque<Vec<SlimCell>>`); rows are compressed from
/// `Cell` to `SlimCell` exactly when they cross the viewport→scrollback
/// boundary in `ring_push_blank`. Style and char attributes are
/// deduplicated through `StyleTable` / `CharTable` (see `slim_cell.rs`).
///
/// Layout:
/// ```text
/// scrollback_slim: VecDeque<Vec<SlimCell>>   (oldest at front)
/// ring_cells:      Vec<Cell> length = rows × cols (rotates by ring_head)
/// ```
///
/// Invariants:
/// - ring_cells.len() == rows × cols
/// - ring_size == rows (always; the viewport is always fully populated)
/// - ring_head ∈ [0, rows)
/// - scrollback_slim.len() == scrollback_wrapped.len() ≤ scrollback_capacity
use crate::cell::*;
use crate::slim_cell::{SlimCell, cell_to_slim, slim_overflow_str, slim_to_cell};
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

    /// Map viewport row (0-based) to absolute index in the rotating
    /// viewport ring (length = `rows`).
    #[inline]
    pub(crate) fn viewport_abs(&self, row: u16) -> usize {
        let rows = self.rows as usize;
        if rows == 0 {
            0
        } else {
            (self.ring_head + row as usize) % rows
        }
    }

    /// Compute cell offset in `ring_cells` for an absolute viewport row.
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

    /// Get the number of scrollback lines.
    #[inline]
    pub(crate) fn scrollback_count(&self) -> usize {
        self.scrollback_slim.len()
    }

    // ── Ring buffer scroll operations ─────────────────────

    /// Push a blank line at the bottom of the viewport, evicting the top
    /// viewport row into compressed scrollback (or dropping it if scrollback
    /// is at capacity / disabled).
    ///
    /// `bg` specifies the background color for the new blank cells (BCE).
    pub(crate) fn ring_push_blank(&mut self, bg: PackedColor) {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        if rows == 0 || cols == 0 {
            return;
        }

        // The row at `ring_head` is the current viewport top — about to be evicted.
        let evicted_abs = self.ring_head;
        let evicted_base = evicted_abs * cols;
        if evicted_base + cols > self.ring_cells.len() {
            return;
        }

        // ── Step 1: compress the evicted row into scrollback (if capacity > 0).
        if self.scrollback_capacity > 0 {
            // Pull overflow strings out of the OverflowTable for this absolute row.
            let evicted_abs32 = evicted_abs as u32;
            let mut slim_row: Vec<SlimCell> = Vec::with_capacity(cols);
            for c in 0..cols {
                let cell = self.ring_cells[evicted_base + c];
                let overflow_str = if cell.is_overflow() {
                    self.overflow.get(&(c as u32, evicted_abs32)).cloned()
                } else {
                    None
                };
                let slim = cell_to_slim(
                    &cell,
                    overflow_str.as_deref(),
                    &mut self.styles,
                    &mut self.chars,
                );
                slim_row.push(slim);
            }
            // Now that the data is interned, drop the side-table overflow entries.
            if !self.overflow.is_empty() {
                overflow_clear_row(&mut self.overflow, evicted_abs32);
                overflow_ridx_clear_row(&mut self.overflow_ridx, evicted_abs32);
            }
            let wrapped = self.ring_wrapped[evicted_abs];

            // If at capacity, drop the oldest scrollback row and release its refs.
            if self.scrollback_slim.len() >= self.scrollback_capacity {
                if let Some(old) = self.scrollback_slim.pop_front() {
                    self.release_slim_row(&old);
                }
                self.scrollback_wrapped.pop_front();
            }
            self.scrollback_slim.push_back(slim_row);
            self.scrollback_wrapped.push_back(wrapped);
        } else {
            // scrollback disabled: just clear overflow side-table for this row.
            let evicted_abs32 = evicted_abs as u32;
            if !self.overflow.is_empty() {
                overflow_clear_row(&mut self.overflow, evicted_abs32);
                overflow_ridx_clear_row(&mut self.overflow_ridx, evicted_abs32);
            }
        }

        // ── Step 2: rotate ring_head; the slot that was the top is now the
        // new viewport bottom (and we'll fill it with BCE blanks).
        self.ring_head = (self.ring_head + 1) % rows;

        // ── Step 3: clear the new viewport bottom (which is the slot we just rotated past).
        let new_bottom_abs = (self.ring_head + rows - 1) % rows;
        let new_base = new_bottom_abs * cols;
        let slice = &mut self.ring_cells[new_base..new_base + cols];

        if bg == PackedColor::DEFAULT {
            slice.fill(Cell::EMPTY);
        } else {
            let mut bce = Cell::EMPTY;
            bce.bg = bg;
            slice.fill(bce);
        }

        self.ring_wrapped[new_bottom_abs] = false;
        let new_bottom_abs32 = new_bottom_abs as u32;
        if !self.overflow.is_empty() {
            overflow_clear_row(&mut self.overflow, new_bottom_abs32);
            overflow_ridx_clear_row(&mut self.overflow_ridx, new_bottom_abs32);
        }
    }

    /// Decrement reference counts for every cell in a slim row about to be dropped.
    pub(crate) fn release_slim_row(&mut self, row: &[SlimCell]) {
        for slim in row {
            self.styles.dec_ref(slim.style_id);
            if slim.is_char_table() {
                self.chars.dec_ref(slim.char_ref);
            }
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
            if count == 1 {
                match self.scroll_event {
                    Some(ref mut e) if e.direction == ScrollDirection::Up => {
                        e.count += 1;
                    }
                    None => {
                        self.scroll_event = Some(ScrollEvent {
                            direction: ScrollDirection::Up,
                            count: 1,
                        });
                    }
                    _ => {
                        // Direction mismatch — fall back to full redraw
                        self.scroll_event = None;
                        self.mark_all_dirty();
                        return;
                    }
                }
                self.shift_dirty_down_by_one();
                self.mark_row_dirty(bottom);
            } else {
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

    /// Pack a viewport ring line (by absolute index) into binary format.
    pub(crate) fn pack_row_abs(&self, abs: usize) -> Vec<u8> {
        let cols = self.cols as usize;
        let base = abs * cols;
        if base + cols > self.ring_cells.len() {
            // Invariant violation: return empty row data
            log::warn!(
                "pack_row_abs invariant violation: abs={}, cols={}, base+cols={}, ring_cells.len={}, capacity={}, ring_head={}",
                abs,
                cols,
                base + cols,
                self.ring_cells.len(),
                self.ring_cells.len() / cols,
                self.ring_head
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
            Self::push_cell_attrs(&mut buf, cell);
        }
        buf
    }

    /// Pack a slim row (scrollback) into binary format identical to pack_row_abs.
    pub(crate) fn pack_slim_row(&self, slim_row: &[SlimCell]) -> Vec<u8> {
        let cols = self.cols as usize;
        let mut buf = Vec::with_capacity(cols * 12);
        for slim in slim_row.iter().take(cols) {
            let cell = slim_to_cell(slim, &self.styles, &self.chars);
            if cell.is_overflow() {
                let s = slim_overflow_str(slim, &self.chars);
                let bytes = s.as_bytes();
                let bytes = if bytes.is_empty() {
                    b" ".as_slice()
                } else {
                    bytes
                };
                let len = bytes.len();
                buf.push(0xFF);
                buf.push((len >> 8) as u8);
                buf.push(len as u8);
                buf.extend_from_slice(bytes);
            } else {
                let len = cell.char_len;
                buf.push(len);
                buf.extend_from_slice(&cell.char_data[..len as usize]);
            }
            Self::push_cell_attrs(&mut buf, &cell);
        }
        buf
    }

    /// Append non-char cell attributes (width, fg, bg, flags, hyperlink_id) to `buf`.
    fn push_cell_attrs(buf: &mut Vec<u8>, cell: &Cell) {
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
        // hyperlink_id: 2 bytes (little-endian)
        buf.push(cell.hyperlink_id as u8);
        buf.push((cell.hyperlink_id >> 8) as u8);
    }

    /// Get text content of a viewport ring line by absolute index.
    pub(crate) fn line_text_abs(&self, abs: usize) -> String {
        let cols = self.cols as usize;
        let base = abs * cols;
        if base + cols > self.ring_cells.len() {
            return String::new();
        }
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

    /// Get text content of a scrollback row.
    pub(crate) fn slim_row_text(&self, slim_row: &[SlimCell]) -> String {
        let mut text = String::new();
        for slim in slim_row {
            if slim.width == 0 {
                continue;
            }
            if slim.is_char_table() {
                text.push_str(slim_overflow_str(slim, &self.chars));
            } else {
                let cell = slim_to_cell(slim, &self.styles, &self.chars);
                if let Some(s) = cell.get_char_inline() {
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
        match self.scrollback_slim.get(index) {
            Some(row) => self.pack_slim_row(row),
            None => Vec::new(),
        }
    }

    /// Get scrollback line as text (trimmed of trailing whitespace).
    /// index: 0 = oldest scrollback line.
    /// Returns empty string if index >= scrollback_count.
    pub(crate) fn scrollback_text(&self, index: usize) -> String {
        match self.scrollback_slim.get(index) {
            Some(row) => self.slim_row_text(row).trim_end().to_string(),
            None => String::new(),
        }
    }
}

// ── scrollback API (was wasm_bindgen) ────────────────────

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
        self.scrollback_wrapped
            .get(index as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Clear scrollback buffer, retaining only viewport lines.
    /// Used by ED 3 (Erase Scrollback).
    pub fn clear_scrollback(&mut self) {
        if self.scrollback_slim.is_empty() {
            return;
        }
        // Release intern refcounts for every scrollback row.
        let drained: Vec<Vec<SlimCell>> = self.scrollback_slim.drain(..).collect();
        for row in &drained {
            self.release_slim_row(row);
        }
        self.scrollback_wrapped.clear();
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
        let new_total = new_rows as usize * new_cols as usize;

        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_rows as usize];
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

        // Drop scrollback (alt buffer doesn't preserve it).
        let drained: Vec<Vec<SlimCell>> = self.scrollback_slim.drain(..).collect();
        for row in &drained {
            self.release_slim_row(row);
        }
        self.scrollback_wrapped.clear();
        self.scrollback_capacity = 0;

        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = new_rows as usize;
        self.ring_capacity = new_rows as usize;

        self.resize_post_cleanup(new_cols, new_rows);

        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use crate::cell::PackedColor;
    use crate::ring_buffer::ScrollDirection;
    use crate::terminal_core::TerminalCore;

    // ── Ring buffer index mapping tests ──────────────────

    #[test]
    fn test_viewport_abs_no_scrollback() {
        // With scrollback_lines=0: ring_capacity=rows, ring_head=0
        // viewport_abs(r) = r
        let core = TerminalCore::new(80, 24, 0);
        for r in 0..24u16 {
            assert_eq!(core.viewport_abs(r), r as usize);
        }
    }

    #[test]
    fn test_viewport_abs_with_scrollback_capacity() {
        // scrollback_lines=100 — viewport ring is still sized for 24 rows.
        let core = TerminalCore::new(80, 24, 100);
        for r in 0..24u16 {
            assert_eq!(core.viewport_abs(r), r as usize);
        }
    }

    #[test]
    fn test_scrollback_count_initial_no_scrollback() {
        let core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.scrollback_count(), 0);
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
        assert_eq!(core.scrollback_capacity, 1000);
    }

    #[test]
    fn test_constructor_zero_scrollback_matches_flat() {
        let core = TerminalCore::new(10, 5, 0);
        assert_eq!(core.ring_capacity, 5);
        assert_eq!(core.ring_size, 5);
        assert_eq!(core.ring_head, 0);
        assert_eq!(core.scrollback_capacity, 0);
        // All cells should be empty
        for r in 0..5 {
            assert!(core.is_line_empty(r));
        }
    }

    // ── Ring push / scroll internal tests ─────────────────

    #[test]
    fn test_ring_push_blank_grows_scrollback() {
        let mut core = TerminalCore::new(10, 3, 5);
        assert_eq!(core.scrollback_count(), 0);
        core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.ring_push_blank(PackedColor::DEFAULT);
        assert_eq!(core.scrollback_count(), 1);
        // Old row 0 ("A") is now in scrollback, viewport row 0 is old row 1
        assert_eq!(core.get_cell_char(0, 0), " "); // old row 1 was empty
    }

    #[test]
    fn test_ring_push_blank_at_capacity_evicts() {
        let mut core = TerminalCore::new(10, 3, 2);
        // scrollback capacity = 2.
        core.ring_push_blank(PackedColor::DEFAULT); // scrollback = 1
        core.ring_push_blank(PackedColor::DEFAULT); // scrollback = 2 (at capacity)
        assert_eq!(core.scrollback_count(), 2);
        // Next push should evict oldest
        core.ring_push_blank(PackedColor::DEFAULT);
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

        // Push enough blanks to evict the overflow row out of viewport AND scrollback
        for _ in 0..5 {
            core.ring_push_blank(PackedColor::DEFAULT);
        }
        // Overflow side-table should be drained (the data was moved into CharTable
        // when the row was compressed). After eviction from scrollback the CharTable
        // refcount drops back to zero.
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

        let event = core.scroll_event.expect("scroll event should be Some");
        assert_eq!(event.direction, ScrollDirection::Up);
        assert_eq!(event.count, 1);

        assert!(!core.is_row_dirty(0));
        assert!(!core.is_row_dirty(12));
        assert!(!core.is_row_dirty(22));
        assert!(core.is_row_dirty(23));
    }

    #[test]
    fn test_scroll_up_full_screen_count_gt1_no_scroll_event() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.scroll_up_internal(3);

        assert!(core.scroll_event.is_none());
        assert!(core.is_row_dirty(0));
        assert!(core.is_row_dirty(12));
        assert!(core.is_row_dirty(23));
    }

    #[test]
    fn test_scroll_up_scroll_region_no_scroll_event() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.scroll_region_top = 5;
        core.scroll_region_bottom = 20;
        core.clear_dirty();

        core.scroll_up_internal(1);

        assert!(core.scroll_event.is_none());
    }

    #[test]
    fn test_scroll_event_cleared_correctly() {
        let mut core = TerminalCore::new(80, 24, 100);

        core.scroll_up_internal(1);
        let event = core.scroll_event.expect("scroll event should be Some");
        assert_eq!(event.direction, ScrollDirection::Up);
        assert_eq!(event.count, 1);
        assert_eq!(core.get_scroll_event_direction(), 1); // 1 = Up
        assert_eq!(core.get_scroll_event_count(), 1);

        core.clear_scroll_event();
        assert!(core.scroll_event.is_none());
        assert_eq!(core.get_scroll_event_direction(), 0);
        assert_eq!(core.get_scroll_event_count(), 0);
    }

    #[test]
    fn test_scroll_up_count1_accumulates_scroll_events() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.scroll_up_internal(1);
        core.scroll_up_internal(1);
        core.scroll_up_internal(1);

        let event = core.scroll_event.expect("scroll event should be Some");
        assert_eq!(event.direction, ScrollDirection::Up);
        assert_eq!(event.count, 3);

        assert!(core.is_row_dirty(23));
    }

    #[test]
    fn test_scroll_up_count1_shifts_dirty_and_marks_last() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.mark_row_dirty(15);
        core.mark_row_dirty(20);

        core.scroll_up_internal(1);

        assert!(!core.is_row_dirty(15), "row 15 should no longer be dirty");
        assert!(
            core.is_row_dirty(14),
            "row 14 should be dirty (shifted from 15)"
        );
        assert!(!core.is_row_dirty(20), "row 20 should no longer be dirty");
        assert!(
            core.is_row_dirty(19),
            "row 19 should be dirty (shifted from 20)"
        );
        assert!(core.is_row_dirty(23), "last row should be dirty");
    }

    #[test]
    fn test_scroll_up_count1_shifts_row0_dirty_away() {
        let mut core = TerminalCore::new(80, 24, 100);
        core.clear_dirty();

        core.mark_row_dirty(0);
        core.mark_row_dirty(10);

        core.scroll_up_internal(1);

        assert!(
            !core.is_row_dirty(0),
            "row 0 should not be dirty (shifted away)"
        );
        assert!(
            core.is_row_dirty(9),
            "row 9 should be dirty (shifted from 10)"
        );
        assert!(core.is_row_dirty(23), "last row should be dirty");
    }

    #[test]
    fn test_shift_dirty_down_by_one_across_word_boundary() {
        let mut core = TerminalCore::new(80, 128, 100); // 128 rows = 2 u64 words
        core.clear_dirty();

        core.mark_row_dirty(64);

        core.shift_dirty_down_by_one();

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

    // ── SlimCell-specific tests (Phase 2 NEW) ──────────────

    #[test]
    fn test_scrollback_dedup_same_style() {
        // 1 million cells with same style → StyleTable should hold 2 entries
        // (default + the one used).
        let mut core = TerminalCore::new(80, 1, 100);
        for _ in 0..50 {
            for c in 0..80 {
                core.set_cell(
                    c, 0, "A", 1, 2, 100, 150, 200, // RGB fg
                    0, 0, 0, 0, 0,
                );
            }
            core.scroll_up_internal(1);
        }
        // styles table should have exactly 2 entries: default + one custom
        assert_eq!(core.styles.live_entries(), 2);
    }

    #[test]
    fn test_scrollback_zero_no_slim_cells() {
        // scrollback_lines = 0 → no scrollback ever.
        let mut core = TerminalCore::new(10, 3, 0);
        for r in 0..3 {
            core.set_cell(0, r, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        for _ in 0..10 {
            core.scroll_up_internal(1);
        }
        assert_eq!(core.scrollback_count(), 0);
        assert_eq!(core.styles.live_entries(), 1); // only default
    }

    #[test]
    fn test_scrollback_overflow_zwj_round_trip() {
        // ZWJ family emoji in scrollback should survive via CharTable.
        let mut core = TerminalCore::new(10, 3, 5);
        let zwj = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, zwj, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
        let text = core.get_scrollback_text(0);
        assert!(text.contains(zwj), "expected to find {zwj}, got '{text}'");
        // CharTable should have the entry
        assert_eq!(core.chars.live_entries(), 1);
    }

    #[test]
    fn test_clear_scrollback_releases_refcounts() {
        let mut core = TerminalCore::new(10, 3, 5);
        let zwj = "👨‍👩‍👧‍👦";
        core.set_cell(0, 0, zwj, 2, 2, 100, 150, 200, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
        assert_eq!(core.scrollback_count(), 1);
        assert_eq!(core.chars.live_entries(), 1);
        assert_eq!(core.styles.live_entries(), 2);

        core.clear_scrollback();
        assert_eq!(core.scrollback_count(), 0);
        // Tables should be back to baseline
        assert_eq!(core.chars.live_entries(), 0);
        assert_eq!(core.styles.live_entries(), 1);
    }

    #[test]
    fn test_eviction_releases_refcounts() {
        let mut core = TerminalCore::new(10, 3, 2); // capacity 2 scrollback rows
        // Push 5 distinct rows; only the last 2 should remain.
        for i in 0..5u32 {
            // Use a unique style per row by varying RGB.
            core.set_cell(0, 0, "A", 1, 2, i as u8, 0, 0, 0, 0, 0, 0, 0);
            core.scroll_up_internal(1);
        }
        assert_eq!(core.scrollback_count(), 2);
        // StyleTable: default + (each cell uses 1 distinct style for col 0;
        // remaining 9 cols are blanks with default style). Live should be 2 + 1
        // = 3 (default + the 2 surviving styles for the kept rows). The other
        // 3 styles were evicted and their refcount went to 0.
        assert!(
            core.styles.live_entries() <= 3,
            "got {} live styles",
            core.styles.live_entries()
        );
    }
}
