/// Row operation impl methods for TerminalCore.
///
/// Provides row-level operations: clear_line, clear_line_range, get_line_text,
/// is_line_empty, line wrap flag accessors, shift_rows_up/down, copy_row, fill_row_default.
use crate::cell::*;
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    // ── Line operations ──────────────────────────────────

    pub fn clear_line(&mut self, row: u16) {
        if row >= self.rows {
            return;
        }
        let bce = self.bce_cell();
        let abs = self.viewport_abs(row);
        let cols = self.cols as usize;
        let base = abs * cols;
        if base + cols > self.ring_cells.len() {
            return;
        }
        for i in base..base + cols {
            self.ring_cells[i] = bce;
        }
        self.ring_wrapped[abs] = false;
        let abs32 = abs as u32;
        overflow_clear_row(&mut self.overflow, abs32);
        overflow_ridx_clear_row(&mut self.overflow_ridx, abs32);
        self.mark_row_dirty(row);
    }

    pub fn clear_line_range(&mut self, row: u16, start_col: u16, end_col: u16) {
        if row >= self.rows {
            return;
        }
        let bce = self.bce_cell();
        let start = start_col.min(self.cols) as usize;
        let end = end_col.min(self.cols) as usize;
        let abs = self.viewport_abs(row);
        let cols = self.cols as usize;
        let base = abs * cols;
        if base + cols > self.ring_cells.len() {
            return;
        }
        for i in base + start..base + end {
            self.ring_cells[i] = bce;
        }
        let abs32 = abs as u32;
        overflow_clear_range(&mut self.overflow, abs32, start_col as u32, end_col as u32);
        overflow_ridx_clear_range(
            &mut self.overflow_ridx,
            abs32,
            start_col as u32,
            end_col as u32,
        );
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
        let abs = self.viewport_abs(row);
        let cols = self.cols as usize;
        let base = abs * cols;
        if base + cols > self.ring_cells.len() {
            return true;
        }
        for col in 0..cols {
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

        // Defensive: verify ring_cells can hold all viewport rows
        if self.rows as usize * cols > self.ring_cells.len() {
            return;
        }

        // Clear overflow for rows that will be overwritten (deleted range)
        for r in start_row..start_row + count {
            let abs = self.viewport_abs(r) as u32;
            overflow_clear_row(&mut self.overflow, abs);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs);
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
                // Move overflow entries using reverse index for O(1) row lookup
                let src_abs_u32 = src_abs as u32;
                let dst_abs_u32 = dst_abs as u32;
                if let Some(src_cols) = self.overflow_ridx.remove(&src_abs_u32) {
                    for &c in &src_cols {
                        if let Some(v) = self.overflow.remove(&(c, src_abs_u32)) {
                            self.overflow.insert((c, dst_abs_u32), v);
                        }
                    }
                    self.overflow_ridx.insert(dst_abs_u32, src_cols);
                }
            }
        }
        // Clear vacated rows at bottom
        let bce = self.bce_cell();
        for row in (end_row + 1 - count)..=end_row {
            let abs = self.viewport_abs(row);
            let base = abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = bce;
            }
            self.ring_wrapped[abs] = false;
            let abs32 = abs as u32;
            overflow_clear_row(&mut self.overflow, abs32);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs32);
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

        // Defensive: verify ring_cells can hold all viewport rows
        if self.rows as usize * cols > self.ring_cells.len() {
            return;
        }

        // Clear overflow for rows that will be overwritten (bottom range)
        for r in (end_row + 1 - count)..=end_row {
            let abs = self.viewport_abs(r) as u32;
            overflow_clear_row(&mut self.overflow, abs);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs);
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
            // Move overflow entries using reverse index for O(1) row lookup
            let src_abs_u32 = src_abs as u32;
            let dst_abs_u32 = dst_abs as u32;
            if let Some(src_cols) = self.overflow_ridx.remove(&src_abs_u32) {
                for &c in &src_cols {
                    if let Some(v) = self.overflow.remove(&(c, src_abs_u32)) {
                        self.overflow.insert((c, dst_abs_u32), v);
                    }
                }
                self.overflow_ridx.insert(dst_abs_u32, src_cols);
            }
        }
        // Clear vacated rows at top
        let bce = self.bce_cell();
        for row in start_row..start_row + count {
            let abs = self.viewport_abs(row);
            let base = abs * cols;
            for i in base..base + cols {
                self.ring_cells[i] = bce;
            }
            self.ring_wrapped[abs] = false;
            let abs32 = abs as u32;
            overflow_clear_row(&mut self.overflow, abs32);
            overflow_ridx_clear_row(&mut self.overflow_ridx, abs32);
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
        if src_base + cols > self.ring_cells.len() || dst_base + cols > self.ring_cells.len() {
            return;
        }
        for i in 0..cols {
            self.ring_cells[dst_base + i] = self.ring_cells[src_base + i];
        }
        self.ring_wrapped[dst_abs] = self.ring_wrapped[src_abs];

        // Copy overflow entries using reverse index for O(1) lookup
        let dst_abs_u32 = dst_abs as u32;
        let src_abs_u32 = src_abs as u32;
        overflow_clear_row(&mut self.overflow, dst_abs_u32);
        overflow_ridx_clear_row(&mut self.overflow_ridx, dst_abs_u32);
        if let Some(src_cols) = self.overflow_ridx.get(&src_abs_u32) {
            let mut dst_cols = Vec::with_capacity(src_cols.len());
            for &c in src_cols {
                if let Some(v) = self.overflow.get(&(c, src_abs_u32)) {
                    self.overflow.insert((c, dst_abs_u32), v.clone());
                    dst_cols.push(c);
                }
            }
            if !dst_cols.is_empty() {
                self.overflow_ridx.insert(dst_abs_u32, dst_cols);
            }
        }

        self.mark_row_dirty(dst_row);
    }

    pub fn fill_row_default(&mut self, row: u16) {
        self.clear_line(row);
    }
}
