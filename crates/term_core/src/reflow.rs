/// Reflow logic for terminal resize with line wrapping.
///
/// Handles joining wrapped physical lines into logical lines and
/// re-splitting them at a new column width, preserving cursor position
/// and overflow strings.
use std::collections::VecDeque;

use crate::cell::*;
use crate::char_table::CharTable;
use crate::slim_cell::{SlimCell, cell_to_slim, slim_overflow_str, slim_to_cell};
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
    /// Same-width resize: adjusts row count without re-wrapping any cell
    /// content — a same-width resize can never change how a physical line
    /// wraps, since wrapping is purely a function of COLUMN width, which is
    /// unchanged here. Touches only the rows that actually cross the
    /// viewport/scrollback boundary as a result of the height change —
    /// never the rows that stay untouched on either side of that boundary.
    ///
    /// D1 (round-10 rework, mux-render-corruption task0010): this REPLACES
    /// an implementation that unconditionally ran a full
    /// `reflow_drain` + `repopulate_ring_from_lines` round trip —
    /// decompressing EVERY accumulated scrollback row into `Cell` and then
    /// re-interning EVERY retained row into brand-new `StyleTable`/
    /// `CharTable`s — even though a same-width resize never needs to
    /// re-wrap anything. That round trip's cost grew with the SIZE OF
    /// ACCUMULATED SCROLLBACK, not with the height CHANGE itself, which is
    /// what made a resize storm's replay cost scale (super-linearly, since
    /// scrollback itself grows across the storm) with segment count — see
    /// `crates/term_core/src/bench.rs`'s
    /// `segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`,
    /// whose storm shape alternates ROWS only (cols fixed), the exact cost
    /// this rework removes. Confirmed to fail without this fix: reverting
    /// this function to call `resize_same_width_reference` unconditionally
    /// reproduces the multi-second storm latency that same bench measures.
    ///
    /// The `skip`/`keep`/cursor-tracking arithmetic below is copied
    /// VERBATIM from [`Self::resize_same_width_reference`] — it depends
    /// only on ROW COUNTS (`scrollback_slim.len()`, `old_rows`, `new_rows`,
    /// `scrollback_capacity`, `cursor_row`), never on cell content, so
    /// keeping it exact costs nothing. What differs is HOW the resulting
    /// row membership (which rows end up in scrollback vs. viewport vs.
    /// dropped outright — the reference's cursor-visibility logic can drop
    /// a bounded number of the NEWEST rows too, not just the oldest, when
    /// capacity eviction would otherwise scroll the cursor out of view; see
    /// `trailing_drop` below) gets REALIZED: instead of decompressing and
    /// re-interning the whole drained sequence, only the rows whose side of
    /// the viewport/scrollback boundary actually changes get compressed
    /// (viewport → scrollback, shrink direction) or decompressed
    /// (scrollback → viewport, grow direction); rows that stay on the same
    /// side are moved with a plain `Cell` copy (viewport → viewport) or are
    /// not touched at all (scrollback → scrollback).
    ///
    /// Falls back to [`Self::resize_same_width_reference`] — the always-
    /// correct, O(total-content) path this replaces — for the one case
    /// this fast path does not attempt: `skip` (rows dropped from the
    /// front of scrollback to fit the new capacity) exceeding the CURRENT
    /// scrollback length, i.e. eviction reaching into rows that are still
    /// part of the CURRENT viewport. That needs a shrinking scrollback
    /// CAPACITY (not row count — capacity is fixed for the whole duration
    /// of a snapshot replay) big enough that the height change alone
    /// cannot compensate, in the SAME call that also changes the row
    /// count; it is never exercised by replay and vanishingly rare live.
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

        let sc = self.scrollback_slim.len();
        let total_lines = sc + old_rows;
        let cursor_abs = sc + cursor_row;
        let total_capacity = scrollback_capacity + new_rows_usize;
        let keep = total_lines.min(total_capacity);
        let desired_vp_row = cursor_row.min(new_rows_usize.saturating_sub(1));
        let skip = if total_lines <= total_capacity {
            0
        } else {
            let ideal = cursor_abs
                .saturating_sub(keep.saturating_sub(new_rows_usize))
                .saturating_sub(desired_vp_row);
            ideal.min(total_lines.saturating_sub(keep))
        };

        if skip > sc {
            // See doc comment: needs a shrinking CAPACITY big enough to eat
            // into the current viewport in the same call — never hit
            // during replay, rare live. Defer to the always-correct path
            // rather than extending the fast path to a case that costs
            // nothing in the scenario this rework targets.
            return self.resize_same_width_reference(
                new_rows,
                scrollback_lines,
                cursor_col,
                cursor_row,
            );
        }

        // Cursor tracking — identical expressions to the reference
        // implementation; depends only on the row counts above, never on
        // cell content.
        let cursor_abs_new = cursor_abs.saturating_sub(skip);
        let vp_start_new = keep.saturating_sub(new_rows_usize);
        let new_cursor_row = cursor_abs_new.saturating_sub(vp_start_new);

        // How many of the OLDEST rows in the combined [old scrollback, old
        // viewport] sequence never make it into the new state at all —
        // not moved to scrollback, not kept in the viewport (the
        // reference's cursor-visibility `skip` can choose to drop some of
        // the NEWEST content instead of the oldest; see the doc comment).
        // Bounded by `skip_min`, which is itself bounded by the row-count
        // delta, never by accumulated scrollback size.
        let skip_min = total_lines.saturating_sub(total_capacity);
        let trailing_drop = skip_min.saturating_sub(skip);
        // Rows of the CURRENT viewport that still participate — the
        // bottom `trailing_drop` rows are the ones being dropped outright.
        let eov = old_rows.saturating_sub(trailing_drop);

        // Drop the oldest `skip` scrollback rows — never touches the
        // viewport, never touches a row that survives.
        for _ in 0..skip {
            if let Some(old) = self.scrollback_slim.pop_front() {
                self.release_slim_row(&old);
            }
            self.scrollback_wrapped.pop_front();
        }
        let rsc = self.scrollback_slim.len(); // == sc - skip

        let new_total = new_rows_usize * cols;
        let mut new_grid = vec![Cell::EMPTY; new_total];
        let mut new_wrapped = vec![false; new_rows_usize];
        let mut new_overflow = OverflowTable::new();

        if eov >= new_rows_usize {
            // SHRINK (or unchanged height): the top `shrink_amount` rows of
            // the surviving viewport portion move into scrollback
            // (compressed — O(shrink_amount), never touches existing
            // scrollback rows); the bottom `new_rows` become the new
            // viewport (a plain `Cell` copy, no reflow).
            let shrink_amount = eov - new_rows_usize;
            for r in 0..shrink_amount {
                let abs = self.viewport_abs(r as u16);
                let base = abs * cols;
                let abs32 = abs as u32;
                let mut slim_row: Vec<SlimCell> = Vec::with_capacity(cols);
                for c in 0..cols {
                    let cell = self.ring_cells[base + c];
                    let overflow_str = if cell.is_overflow() {
                        self.overflow.get(&(c as u32, abs32)).cloned()
                    } else {
                        None
                    };
                    slim_row.push(cell_to_slim(
                        &cell,
                        overflow_str.as_deref(),
                        &mut self.styles,
                        &mut self.chars,
                    ));
                }
                let wrapped = self.ring_wrapped[abs];
                self.scrollback_slim.push_back(slim_row);
                self.scrollback_wrapped.push_back(wrapped);
            }
            for r in 0..new_rows_usize {
                let old_abs = self.viewport_abs((shrink_amount + r) as u16);
                let old_base = old_abs * cols;
                let new_base = r * cols;
                new_grid[new_base..new_base + cols]
                    .copy_from_slice(&self.ring_cells[old_base..old_base + cols]);
                new_wrapped[r] = self.ring_wrapped[old_abs];
                if !self.overflow.is_empty() {
                    for c in 0..cols {
                        if let Some(s) = self.overflow.get(&(c as u32, old_abs as u32)) {
                            new_overflow.insert((c as u32, r as u32), s.clone());
                        }
                    }
                }
            }
        } else {
            // GROW: pull the `grow_amount` most-recent rows back OUT of
            // scrollback (decompressed — O(grow_amount), the rest of
            // scrollback is never touched) into the TOP of the new
            // (taller) viewport, immediately followed by the surviving
            // old viewport (`eov` rows, a plain `Cell` copy). If there
            // isn't enough combined history to fill `new_rows` at all
            // (`keep < new_rows` — see `Self::resize_same_width_reference`'s
            // `vp_start = keep.saturating_sub(new_rows)` saturating to 0),
            // the shortfall is left as trailing BLANK rows at the BOTTOM
            // (`new_grid`'s pre-initialized `Cell::EMPTY` / `wrapped:
            // false` defaults for the untouched tail) — matching the
            // reference's own behavior of breaking out of its fill loop
            // once `keep` lines are placed, rather than shifting existing
            // content down to make room for padding above it.
            let grow_amount = new_rows_usize - eov;
            let pulled = grow_amount.min(rsc);
            for i in 0..pulled {
                let slim_row = self
                    .scrollback_slim
                    .pop_back()
                    .expect("pulled <= scrollback_slim.len()");
                let wrapped = self.scrollback_wrapped.pop_back().unwrap_or(false);
                let vp_row = pulled - 1 - i;
                let base = vp_row * cols;
                for c in 0..cols {
                    let slim = slim_row.get(c).copied().unwrap_or(SlimCell::EMPTY);
                    let cell = slim_to_cell(&slim, &self.styles, &self.chars);
                    if cell.is_overflow() {
                        let s = slim_overflow_str(&slim, &self.chars).to_string();
                        new_overflow.insert((c as u32, vp_row as u32), s);
                    }
                    new_grid[base + c] = cell;
                }
                new_wrapped[vp_row] = wrapped;
                self.release_slim_row(&slim_row);
            }
            for r in 0..eov {
                let old_abs = self.viewport_abs(r as u16);
                let old_base = old_abs * cols;
                let new_r = pulled + r;
                let new_base = new_r * cols;
                new_grid[new_base..new_base + cols]
                    .copy_from_slice(&self.ring_cells[old_base..old_base + cols]);
                new_wrapped[new_r] = self.ring_wrapped[old_abs];
                if !self.overflow.is_empty() {
                    for c in 0..cols {
                        if let Some(s) = self.overflow.get(&(c as u32, old_abs as u32)) {
                            new_overflow.insert((c as u32, new_r as u32), s.clone());
                        }
                    }
                }
            }
        }

        self.ring_cells = new_grid;
        self.ring_wrapped = new_wrapped;
        self.ring_head = 0;
        self.ring_size = new_rows_usize;
        self.ring_capacity = scrollback_capacity + new_rows_usize;
        self.scrollback_capacity = scrollback_capacity;
        self.overflow = new_overflow;
        self.overflow_ridx = overflow_ridx_rebuild(&self.overflow);

        (cursor_col, new_cursor_row)
    }

    /// Reference (pre-round-10) same-width resize: full `reflow_drain` +
    /// `repopulate_ring_from_lines` round trip. Kept as (a) the fallback
    /// [`Self::resize_same_width`] defers to for the rare case it does not
    /// attempt, and (b) the equivalence baseline this task's tests compare
    /// the fast path against — see `reflow::tests::same_width_fast_path_matches_reference_*`.
    pub(crate) fn resize_same_width_reference(
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

    // ── D1 (round-10 rework, mux-render-corruption task0010): same-width
    // fast path vs. reference equivalence ────────────────────────────
    //
    // "Equivalence is the gate, not an assumption" (task0010's plan) — each
    // test below builds two IDENTICAL cores from the same byte payload,
    // resizes one via the real `resize_reflow` entry point (which now
    // calls the fast `resize_same_width`) and the other via
    // `resize_via_reference` (which calls `resize_same_width_reference`,
    // the pre-round-10 full-`reflow_drain` implementation kept
    // specifically as this comparison baseline), then asserts the full
    // observable state — viewport text + wrapped flags, cursor, scrollback
    // text + wrapped flags, eviction bookkeeping, and intern-table live
    // counts — is identical. `same_width_fast_path_matches_reference_grow_with_little_scrollback_pads_bottom`
    // is confirmed to fail without the fix in this file: an earlier
    // version of the fast path padded a too-small grown viewport at the
    // TOP instead of the BOTTOM, which this test caught (and which also
    // broke `test_resize_reflow_same_width_grow` /
    // `test_overflow_survives_same_width_resize` /
    // `terminal_core::tests::test_resize_grow_shrink_rows`, all pre-existing
    // tests in this suite).

    /// Mirrors `ring_buffer.rs::resize_reflow`, but calls the reference
    /// same-width implementation instead of the fast path — the
    /// comparison baseline for the tests below.
    fn resize_via_reference(
        core: &mut TerminalCore,
        new_cols: u16,
        new_rows: u16,
        scrollback_lines: u32,
    ) {
        let cursor_col = core.cursor.col as usize;
        let cursor_row = core.cursor.row as usize;
        let (final_col, final_row) = if new_cols == core.cols {
            core.resize_same_width_reference(new_rows, scrollback_lines, cursor_col, cursor_row)
        } else {
            core.resize_full_reflow(new_cols, new_rows, scrollback_lines, cursor_col, cursor_row)
        };
        core.resize_post_cleanup(new_cols, new_rows);
        core.cursor.col = (final_col as u16).min(new_cols.saturating_sub(1));
        core.cursor.row = (final_row as u16).min(new_rows.saturating_sub(1));
    }

    /// Full observable fingerprint: viewport text + wrapped flags per row,
    /// cursor position, scrollback text + wrapped flags per row, eviction
    /// bookkeeping, and intern-table live-entry counts (a proxy for "no
    /// leaked or double-freed refcounts" — the same property
    /// `test_post_reflow_intern_tables_match_rebuild` checks against a
    /// from-scratch rebuild).
    #[allow(clippy::type_complexity)]
    fn full_fingerprint(
        core: &TerminalCore,
    ) -> (
        Vec<(String, bool)>,
        u16,
        u16,
        Vec<(String, bool)>,
        u32,
        u64,
        usize,
        usize,
    ) {
        let mut viewport = Vec::with_capacity(core.rows() as usize);
        for r in 0..core.rows() {
            let mut line = String::new();
            for c in 0..core.cols() {
                line.push_str(&core.get_cell_char(c, r));
            }
            viewport.push((line, core.get_line_wrapped(r)));
        }
        let sb_len = core.get_scrollback_length();
        let mut scrollback = Vec::with_capacity(sb_len as usize);
        for i in 0..sb_len {
            scrollback.push((
                core.get_scrollback_text(i),
                core.get_scrollback_line_wrapped(i),
            ));
        }
        (
            viewport,
            core.get_cursor_col(),
            core.get_cursor_row(),
            scrollback,
            sb_len,
            core.get_scrollback_evicted_total(),
            core.styles.live_entries(),
            core.chars.live_entries(),
        )
    }

    /// Builds a payload mixing: varying-color scrolled lines (real
    /// scrollback growth with distinct interned styles), a ZWJ family
    /// emoji every 7th line (overflow-table content moving between
    /// viewport and scrollback), and a trailing run with no CR/LF long
    /// enough to wrap across multiple physical rows (exercises `wrapped`
    /// flags).
    fn build_rich_payload(cols: usize, scrolled_lines: usize) -> Vec<u8> {
        let zwj = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        let mut payload = Vec::new();
        for i in 0..scrolled_lines {
            payload.extend_from_slice(
                format!(
                    "\x1b[38;2;{};{};{}m",
                    (i * 7) % 256,
                    (i * 13) % 256,
                    (i * 29) % 256
                )
                .as_bytes(),
            );
            if i % 7 == 3 {
                payload.extend_from_slice(zwj.as_bytes());
            }
            payload.extend_from_slice(format!("line-{i:05}").as_bytes());
            payload.extend_from_slice(b"\r\n");
        }
        payload.extend_from_slice(b"\x1b[0m");
        let long_line: String = (0..(cols * 2 + 3))
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        payload.extend_from_slice(long_line.as_bytes());
        payload
    }

    /// Runs one grow/shrink scenario through both paths and asserts
    /// identical results.
    fn assert_fast_path_matches_reference(
        cols: u16,
        old_rows: u16,
        new_rows: u16,
        scrollback_before: u32,
        scrollback_after: u32,
        scrolled_lines: usize,
        pin_cursor_row: Option<u16>,
    ) {
        let payload = build_rich_payload(cols as usize, scrolled_lines);

        let mut fast = TerminalCore::new(cols, old_rows, scrollback_before);
        fast.process_pty_data_fully(&payload);
        let mut reference = TerminalCore::new(cols, old_rows, scrollback_before);
        reference.process_pty_data_fully(&payload);

        // Sanity: identical starting state before either resize runs.
        assert_eq!(
            full_fingerprint(&fast),
            full_fingerprint(&reference),
            "test setup produced non-identical starting cores"
        );

        if let Some(row) = pin_cursor_row {
            let col = fast.get_cursor_col();
            let clamped = row.min(old_rows.saturating_sub(1));
            fast.set_cursor(col, clamped);
            reference.set_cursor(col, clamped);
        }

        fast.resize_reflow(cols, new_rows, scrollback_after);
        resize_via_reference(&mut reference, cols, new_rows, scrollback_after);

        assert_eq!(
            full_fingerprint(&fast),
            full_fingerprint(&reference),
            "fast path diverged from reference for cols={cols} old_rows={old_rows} \
             new_rows={new_rows} sb_before={scrollback_before} sb_after={scrollback_after} \
             scrolled_lines={scrolled_lines} pin_cursor_row={pin_cursor_row:?}"
        );
    }

    #[test]
    fn same_width_fast_path_matches_reference_grow_with_ample_scrollback() {
        assert_fast_path_matches_reference(20, 5, 12, 500, 500, 80, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_shrink_with_ample_scrollback() {
        assert_fast_path_matches_reference(20, 12, 5, 500, 500, 80, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_grow_with_little_scrollback_pads_bottom() {
        // Total content (little scrolled history + a small old viewport)
        // is smaller than the grown viewport — the "not enough history"
        // shape. Confirmed to fail without the fix: an earlier version of
        // this rework placed the shortfall as BLANK padding at the TOP of
        // the new viewport (shifting existing content down), which this
        // assertion catches by comparing against the reference's actual
        // behavior (shortfall left as blank rows at the BOTTOM).
        assert_fast_path_matches_reference(20, 3, 15, 500, 500, 2, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_grow_with_zero_capacity() {
        assert_fast_path_matches_reference(20, 5, 12, 0, 0, 20, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_shrink_forces_capacity_eviction_cursor_at_bottom() {
        // Enough scrolled content to overrun a small capacity; cursor left
        // at its natural resting position (bottom row) after heavy
        // output — the "resize storm" shape this task's bench measures.
        assert_fast_path_matches_reference(20, 30, 24, 40, 40, 300, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_shrink_forces_capacity_eviction_cursor_mid_viewport()
    {
        // Cursor pinned well inside the new (smaller) viewport while
        // capacity eviction is also in play — exercises the reference's
        // cursor-visibility `trailing_drop` branch (dropping some of the
        // NEWEST rows instead of only the oldest) that the fast path has
        // to reproduce rather than assume away.
        assert_fast_path_matches_reference(20, 30, 10, 40, 40, 300, Some(2));
    }

    #[test]
    fn same_width_fast_path_matches_reference_capacity_shrinks_to_zero_same_row_count() {
        // Row count UNCHANGED, only scrollback capacity drops to 0 (the
        // shape `test_reflow_rebuilds_tables_drops_stale_entries` checks
        // via `resize_reflow` directly) — drives it through the
        // fast-vs-reference comparison too, including intern-table counts.
        assert_fast_path_matches_reference(10, 6, 6, 20, 0, 15, None);
    }

    #[test]
    fn same_width_fast_path_matches_reference_unchanged_height_is_a_no_op_shape() {
        assert_fast_path_matches_reference(20, 10, 10, 200, 200, 50, None);
    }

    #[test]
    fn same_width_fast_path_falls_back_to_reference_when_capacity_eviction_reaches_the_viewport() {
        // A capacity collapse (large -> 1) combined with a large height
        // shrink, engineered so `skip` (rows dropped to fit the new
        // capacity) would exceed the CURRENT scrollback length — the one
        // case `resize_same_width` declines to attempt itself and defers
        // to `resize_same_width_reference` for. Still must match the
        // reference exactly (it degrades TO the reference, not an
        // approximation of it).
        assert_fast_path_matches_reference(20, 40, 3, 5, 1, 60, Some(0));
    }
}
