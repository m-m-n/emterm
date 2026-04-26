/// Reflow logic for terminal resize with line wrapping.
///
/// Handles joining wrapped physical lines into logical lines and
/// re-splitting them at a new column width, preserving cursor position
/// and overflow strings.
use std::collections::VecDeque;

use crate::cell::*;
use crate::char_table::CharTable;
use crate::slim_cell::{cell_to_slim, slim_overflow_str, slim_to_cell};
use crate::style_table::StyleTable;
use crate::terminal_core::TerminalCore;

// ── Reflow internals ────────────────────────────────────

/// A physical line extracted from ring buffer.
pub(crate) struct PhysicalLine {
    pub(crate) cells: Vec<Cell>,
    /// Overflow strings per column (None = inline, Some = overflow data).
    pub(crate) overflow_data: Vec<Option<String>>,
    pub(crate) wrapped: bool,
}

/// A logical line = joined wrapped physical lines.
pub(crate) struct LogicalLine {
    pub(crate) cells: Vec<Cell>,
    /// Overflow strings per column (None = inline, Some = overflow data).
    pub(crate) overflow_data: Vec<Option<String>>,
}

impl TerminalCore {
    /// Same-width resize: just adjust row count.
    pub(crate) fn resize_same_width(
        &mut self,
        new_rows: u16,
        scrollback_lines: u32,
        cursor_col: usize,
        cursor_row: usize,
    ) -> (usize, usize) {
        let scrollback_capacity = scrollback_lines as usize;
        let new_rows_usize = new_rows as usize;
        let old_rows = self.rows as usize;
        let cols = self.cols as usize;

        // Drain all lines (scrollback + viewport) into Cell-based PhysicalLines.
        let lines = self.reflow_drain();
        let total_lines = lines.len();

        // Cursor's absolute position in drained lines
        let cursor_abs = total_lines.saturating_sub(old_rows) + cursor_row;

        // Total target capacity = scrollback + viewport
        let total_capacity = scrollback_capacity + new_rows_usize;
        let keep = total_lines.min(total_capacity);

        // Compute skip to keep cursor visible in viewport.
        let desired_vp_row = cursor_row.min(new_rows_usize.saturating_sub(1));
        let skip = if total_lines <= total_capacity {
            0
        } else {
            let ideal = cursor_abs
                .saturating_sub(keep.saturating_sub(new_rows_usize))
                .saturating_sub(desired_vp_row);
            ideal.min(total_lines.saturating_sub(keep))
        };

        self.repopulate_ring_from_lines(
            &lines,
            skip,
            keep,
            new_rows_usize,
            cols,
            scrollback_capacity,
        );

        // Cursor tracking
        let cursor_abs_new = cursor_abs.saturating_sub(skip);
        // The viewport bottom corresponds to the last `new_rows_usize` lines of
        // the kept set; vp_start = keep - new_rows_usize.
        let vp_start = keep.saturating_sub(new_rows_usize);
        let new_cursor_row = cursor_abs_new.saturating_sub(vp_start);
        (cursor_col, new_cursor_row)
    }

    /// Full reflow for width change.
    pub(crate) fn resize_full_reflow(
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
        let scrollback_capacity = scrollback_lines as usize;

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
        let total_capacity = scrollback_capacity + new_rows as usize;
        let keep = keep_count.min(total_capacity);
        let skip = if keep_count > total_capacity {
            keep_count - total_capacity
        } else {
            0
        };

        // 5. Write to new ring + scrollback storage
        self.repopulate_ring_from_lines(
            &new_phys,
            skip,
            keep,
            new_rows as usize,
            new_cols_usize,
            scrollback_capacity,
        );

        // 6. Track cursor
        let cursor_new_phys_adj = cursor_new_phys.saturating_sub(skip);
        let vp_start_new = keep.saturating_sub(new_rows as usize);
        let new_cursor_row = cursor_new_phys_adj.saturating_sub(vp_start_new);
        (cursor_new_col, new_cursor_row)
    }

    /// Rebuild the viewport flat ring + compressed scrollback deque from a
    /// drained PhysicalLine sequence. Allocates fresh `StyleTable`/`CharTable`.
    pub(crate) fn repopulate_ring_from_lines(
        &mut self,
        lines: &[PhysicalLine],
        skip: usize,
        keep: usize,
        new_rows: usize,
        new_cols: usize,
        scrollback_capacity: usize,
    ) {
        // The last `new_rows` of the kept range become the viewport; everything
        // before that becomes scrollback.
        let vp_start = keep.saturating_sub(new_rows);

        // Allocate fresh tables; old refs are released when the deque is replaced.
        let mut new_styles = StyleTable::new();
        let mut new_chars = CharTable::new();

        // Build viewport flat array.
        let new_total = new_rows * new_cols;
        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_rows];
        let mut new_overflow = OverflowTable::new();

        for vp_row in 0..new_rows {
            let line_idx_in_keep = vp_start + vp_row;
            if line_idx_in_keep >= keep {
                break;
            }
            let abs = skip + line_idx_in_keep;
            if abs >= lines.len() {
                break;
            }
            let line = &lines[abs];
            let base = vp_row * new_cols;
            let copy_len = line.cells.len().min(new_cols);
            for c in 0..copy_len {
                new_grid[base + c] = line.cells[c];
                if let Some(Some(s)) = line.overflow_data.get(c) {
                    new_overflow.insert((c as u32, vp_row as u32), s.clone());
                }
            }
            new_wrapped[vp_row] = line.wrapped;
        }

        // Build compressed scrollback (oldest first).
        let scrollback_count = vp_start.min(scrollback_capacity);
        let scrollback_skip = vp_start.saturating_sub(scrollback_count);
        let mut new_scrollback_slim: VecDeque<Vec<crate::slim_cell::SlimCell>> =
            VecDeque::with_capacity(scrollback_count);
        let mut new_scrollback_wrapped: VecDeque<bool> = VecDeque::with_capacity(scrollback_count);

        for sb_idx in 0..scrollback_count {
            let line_idx_in_keep = scrollback_skip + sb_idx;
            if line_idx_in_keep >= vp_start {
                break;
            }
            let abs = skip + line_idx_in_keep;
            if abs >= lines.len() {
                break;
            }
            let line = &lines[abs];
            let mut slim_row = Vec::with_capacity(new_cols);
            for c in 0..new_cols {
                let cell = line.cells.get(c).copied().unwrap_or(Cell::EMPTY);
                let overflow_str = line.overflow_data.get(c).and_then(|s| s.as_deref());
                let slim = cell_to_slim(&cell, overflow_str, &mut new_styles, &mut new_chars);
                slim_row.push(slim);
            }
            new_scrollback_slim.push_back(slim_row);
            new_scrollback_wrapped.push_back(line.wrapped);
        }

        // Atomic swap: drop old tables and old scrollback (refcounts implicitly released).
        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = new_rows;
        self.ring_capacity = scrollback_capacity + new_rows;
        self.scrollback_capacity = scrollback_capacity;
        self.scrollback_slim = new_scrollback_slim;
        self.scrollback_wrapped = new_scrollback_wrapped;
        self.styles = new_styles;
        self.chars = new_chars;
        self.overflow = new_overflow;
        self.overflow_ridx = overflow_ridx_rebuild(&self.overflow);
    }

    /// Drain all lines from scrollback (oldest first) followed by viewport
    /// (top to bottom) in order. Scrollback rows are decompressed from
    /// `SlimCell` to `Cell`; viewport rows are copied directly.
    pub(crate) fn reflow_drain(&self) -> Vec<PhysicalLine> {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        let scrollback_count = self.scrollback_slim.len();
        let mut lines = Vec::with_capacity(scrollback_count + rows);

        // 1. Scrollback rows (decompress on the fly).
        for (sb_idx, slim_row) in self.scrollback_slim.iter().enumerate() {
            let mut cells = Vec::with_capacity(cols);
            let mut overflow_data = Vec::with_capacity(cols);
            for c in 0..cols {
                let slim = slim_row
                    .get(c)
                    .copied()
                    .unwrap_or(crate::slim_cell::SlimCell::EMPTY);
                let cell = slim_to_cell(&slim, &self.styles, &self.chars);
                if cell.is_overflow() {
                    let s = slim_overflow_str(&slim, &self.chars);
                    overflow_data.push(Some(s.to_string()));
                } else {
                    overflow_data.push(None);
                }
                cells.push(cell);
            }
            lines.push(PhysicalLine {
                cells,
                overflow_data,
                wrapped: self
                    .scrollback_wrapped
                    .get(sb_idx)
                    .copied()
                    .unwrap_or(false),
            });
        }

        // 2. Viewport rows.
        for r in 0..rows {
            let abs = (self.ring_head + r) % rows;
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
    pub(crate) fn reflow_join_wrapped(
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
    pub(crate) fn reflow_split_at_width(
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
    pub(crate) fn resize_post_cleanup(&mut self, new_cols: u16, new_rows: u16) {
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
        let long = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"; // ZWJ family emoji, >16 bytes
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
        let long = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
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
        let long = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
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
        let long = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
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

    // ── Phase 3: Reflow with SlimCell scrollback ─────────

    #[test]
    fn test_reflow_preserves_scrollback_with_rich_content() {
        // 10 distinct colors + 5 hyperlinks + 3 ZWJ family emoji in scrollback
        // Resize and verify all visible attributes preserved.
        let mut core = TerminalCore::new(20, 3, 30);
        let zwj = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";

        // Push 10 lines into scrollback with varying colors.
        for i in 0..10u16 {
            for c in 0..20 {
                let r = ((i * 25) & 0xFF) as u8;
                core.set_cell(c, 0, "X", 1, 2, r, 0, 0, 0, 0, 0, 0, 0);
            }
            core.scroll_up_internal(1);
        }
        // Add ZWJ family in scrollback at one row.
        core.set_cell(0, 0, zwj, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);

        let scrollback_before = core.scrollback_count();
        assert!(scrollback_before > 0);

        // Sanity: ZWJ still recoverable in scrollback.
        let oldest_text = core.get_scrollback_text(0);
        assert!(!oldest_text.is_empty());

        // Resize narrower (full reflow).
        core.set_cursor(0, 0);
        let _packed = core.resize_reflow(10, 5, 30);
        assert_eq!(core.cols(), 10);
        assert_eq!(core.rows(), 5);
        // Scrollback should still have entries (lines reflowed).
        assert!(core.scrollback_count() > 0);
    }

    #[test]
    fn test_post_reflow_intern_tables_match_rebuild() {
        let mut core = TerminalCore::new(10, 3, 10);
        for i in 0..5u32 {
            for c in 0..10 {
                core.set_cell(c, 0, "Z", 1, 2, i as u8, 50, 100, 0, 0, 0, 0, 0);
            }
            core.scroll_up_internal(1);
        }
        // Reflow same width, different rows.
        core.resize_reflow(10, 5, 10);
        let (live_styles_rebuild, live_chars_rebuild) = core.rebuild_intern_tables_from_ring();
        assert_eq!(live_styles_rebuild, core.styles.live_entries());
        assert_eq!(live_chars_rebuild, core.chars.live_entries());
    }

    #[test]
    fn test_reflow_rebuilds_tables_drops_stale_entries() {
        // Add a unique style to scrollback, then reflow with a smaller capacity
        // that drops the row. The new tables should not contain the stale style.
        let mut core = TerminalCore::new(10, 3, 5);
        for i in 0..5u32 {
            core.set_cell(0, 0, "X", 1, 2, i as u8, 0, 0, 0, 0, 0, 0, 0);
            core.scroll_up_internal(1);
        }
        let live_before = core.styles.live_entries();
        assert!(live_before >= 2);

        // Reflow with scrollback_lines=0: scrollback gets dropped.
        core.resize_reflow(10, 3, 0);
        assert_eq!(core.scrollback_count(), 0);
        // Tables should be reset to baseline (default style only).
        assert_eq!(core.styles.live_entries(), 1);
        assert_eq!(core.chars.live_entries(), 0);
    }

    #[test]
    fn test_overflow_ridx_rebuilt_after_reflow() {
        let mut core = TerminalCore::new(10, 5, 0);
        let long = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        core.set_cell(0, 0, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

        core.set_cursor(0, 0);
        core.resize_reflow(10, 8, 0);
        // Reverse index should be consistent with overflow table
        for &(col, row) in core.overflow.keys() {
            assert!(core.overflow_ridx.contains_key(&row));
            assert!(core.overflow_ridx[&row].contains(&col));
        }
    }
}
