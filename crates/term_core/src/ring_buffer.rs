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

// ── Scrollback styled cell ───────────────────────────────

/// A single decoded scrollback cell carrying its grapheme, display width,
/// and style in the same packed representation the viewport accessors use.
///
/// `fg` / `bg` are `PackedColor::to_u32()` values (matching `get_cell_fg` /
/// `get_cell_bg`); `flags` is the raw `STYLE_*` bitset (matching
/// `get_cell_flags`). Lets a renderer paint scrollback rows through the same
/// style-resolution path it already uses for the live viewport without
/// re-decoding the [`get_scrollback_row_packed`] binary layout.
///
/// [`get_scrollback_row_packed`]: TerminalCore::get_scrollback_row_packed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackCell {
    /// The cell's grapheme. Empty cells yield an empty string.
    pub glyph: String,
    /// Display width in cells (`>= 1` for kept cells).
    pub width: u16,
    /// Foreground color packed as `PackedColor::to_u32()`.
    pub fg: u32,
    /// Background color packed as `PackedColor::to_u32()`.
    pub bg: u32,
    /// `STYLE_*` flag bitset.
    pub flags: u16,
}

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
        if self.scrollback_bypass {
            // Snapshot-replay bypass: skip the SlimCell intern + scrollback
            // deque push/pop (the per-row hot loop), but keep the externally
            // observable bookkeeping byte-identical to the live path by
            // advancing `virtual_scrollback_len` (capped at
            // `scrollback_capacity`) and `scrollback_evicted_total` (once
            // saturated). The overflow side-table for the evicted row is still
            // cleared so the next live-mode `ring_push_blank` starting from
            // this absolute row finds it clean.
            let evicted_abs32 = evicted_abs as u32;
            if !self.overflow.is_empty() {
                overflow_clear_row(&mut self.overflow, evicted_abs32);
                overflow_ridx_clear_row(&mut self.overflow_ridx, evicted_abs32);
            }
            if self.scrollback_capacity > 0 {
                if (self.virtual_scrollback_len as usize) < self.scrollback_capacity {
                    self.virtual_scrollback_len += 1;
                } else {
                    self.scrollback_evicted_total += 1;
                }
            }
        } else if self.scrollback_capacity > 0 {
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
                    self.scrollback_evicted_total += 1;
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

    /// Prepend a sequence of already-re-interned SlimCell rows onto the
    /// **front** (oldest end) of `scrollback_slim` / `scrollback_wrapped`,
    /// respecting `scrollback_capacity`.
    ///
    /// Used by [`crate::terminal_core::TerminalCore::merge_scrollback_from`]
    /// for the 2nd-pass scrollback restore: the caller has already
    /// re-interned the SlimCells against `self.styles` / `self.chars`, so
    /// each cell in `rows` carries valid `style_id` / `char_ref` references
    /// (with refcounts owned by the prepend call site).
    ///
    /// Input ordering: `rows[0]` is the oldest, `rows[N-1]` is the newest
    /// (so the rebuilt-core sequence, which is naturally oldest-first inside
    /// its own `VecDeque`, can be passed as-is).
    ///
    /// Capacity handling:
    /// - If `existing_len + rows.len() <= scrollback_capacity`, every
    ///   incoming row is inserted.
    /// - Else, the **front-most incoming rows** (the oldest of the rebuilt
    ///   half) are dropped via `release_slim_row` so the rest can fit. The
    ///   pre-existing rows in `self` are never touched — they reflect post-
    ///   bypass live drain and the caller has guaranteed they sit at the
    ///   newer end of the timeline.
    ///
    /// `scrollback_evicted_total` is intentionally NOT bumped: the dropped
    /// rows pre-date the bypass swap, so counting them as evictions would
    /// double-count against the live-drain counter the caller is
    /// reconciling against (NFR5).
    ///
    /// Returns the number of incoming rows actually inserted.
    pub(crate) fn prepend_scrollback_rows(
        &mut self,
        rows: Vec<Vec<SlimCell>>,
        wrapped: Vec<bool>,
    ) -> usize {
        debug_assert_eq!(
            rows.len(),
            wrapped.len(),
            "prepend_scrollback_rows: rows and wrapped lengths must match"
        );
        let mut rows = rows;
        let mut wrapped = wrapped;
        if rows.len() != wrapped.len() {
            // Defensive: in release builds, conform the shorter vec wins so we
            // never index past the end. Truncating to the min keeps the
            // refcount accounting symmetric (every kept slim row has its
            // wrapped flag and vice versa).
            let n = rows.len().min(wrapped.len());
            rows.truncate(n);
            wrapped.truncate(n);
        }
        if rows.is_empty() {
            return 0;
        }
        let capacity = self.scrollback_capacity;
        if capacity == 0 {
            // Scrollback disabled: every incoming row is a drop. The caller
            // re-interned them into `self.styles` / `self.chars`, so we still
            // need to dec_ref each one.
            for row in &rows {
                self.release_slim_row(row);
            }
            return 0;
        }
        let existing = self.scrollback_slim.len();
        let room = capacity.saturating_sub(existing);
        let inserted = rows.len().min(room);
        let drop_count = rows.len() - inserted;
        if drop_count > 0 {
            // The front-most `drop_count` incoming rows do not fit. Dec_ref
            // their cells before dropping the rows themselves so the intern
            // tables stay accurate.
            for row in rows.drain(0..drop_count) {
                self.release_slim_row(&row);
            }
            wrapped.drain(0..drop_count);
        }
        // Now push_front in reverse order so the oldest row ends up at the
        // front (the `front` end is the oldest slot in the deque).
        for (row, wrapped_flag) in rows.into_iter().zip(wrapped.into_iter()).rev() {
            self.scrollback_slim.push_front(row);
            self.scrollback_wrapped.push_front(wrapped_flag);
        }
        inserted
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

    /// Decode a scrollback row into `(grapheme, physical_width)` cells.
    /// Width-0 cells (the trailing half of a wide glyph) are dropped so the
    /// result aligns with the per-cell `get_cell_char` / `get_cell_width`
    /// viewport accessors. Empty cells yield an empty string; the width is
    /// always `>= 1` for the kept cells. This is the decode sibling of
    /// [`Self::pack_slim_row`] — kept in this file so the encode/decode of
    /// the same slim representation stay together.
    pub(crate) fn slim_row_cells(&self, slim_row: &[SlimCell]) -> Vec<(String, u16)> {
        let mut cells: Vec<(String, u16)> = Vec::with_capacity(slim_row.len());
        for slim in slim_row {
            if slim.width == 0 {
                continue;
            }
            let ch = if slim.is_char_table() {
                slim_overflow_str(slim, &self.chars).to_string()
            } else {
                let cell = slim_to_cell(slim, &self.styles, &self.chars);
                cell.get_char_inline().unwrap_or("").to_string()
            };
            cells.push((ch, slim.width as u16));
        }
        cells
    }

    /// Decode a scrollback row into styled cells. Like [`Self::slim_row_cells`]
    /// but additionally resolves each cell's interned style into the same
    /// packed representation the viewport `get_cell_fg` / `get_cell_bg` /
    /// `get_cell_flags` accessors return (`PackedColor::to_u32()` + the raw
    /// `u16` flag bitset). Width-0 continuation halves of wide glyphs are
    /// dropped so the result aligns with those viewport accessors.
    pub(crate) fn slim_row_cells_styled(&self, slim_row: &[SlimCell]) -> Vec<ScrollbackCell> {
        let mut cells: Vec<ScrollbackCell> = Vec::with_capacity(slim_row.len());
        for slim in slim_row {
            if slim.width == 0 {
                continue;
            }
            let ch = if slim.is_char_table() {
                slim_overflow_str(slim, &self.chars).to_string()
            } else {
                let cell = slim_to_cell(slim, &self.styles, &self.chars);
                cell.get_char_inline().unwrap_or("").to_string()
            };
            let style = self.styles.get_or_default(slim.style_id);
            cells.push(ScrollbackCell {
                glyph: ch,
                width: slim.width as u16,
                fg: style.fg.to_u32(),
                bg: style.bg.to_u32(),
                flags: style.flags,
            });
        }
        cells
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

    /// Get scrollback line decoded into `(grapheme, physical_width)` cells.
    /// index: 0 = oldest scrollback line.
    /// Returns an empty vec if index >= scrollback_count.
    pub(crate) fn scrollback_row_cells(&self, index: usize) -> Vec<(String, u16)> {
        match self.scrollback_slim.get(index) {
            Some(row) => self.slim_row_cells(row),
            None => Vec::new(),
        }
    }

    /// Get scrollback line decoded into styled cells (char + width + packed
    /// fg/bg + flags). index: 0 = oldest scrollback line.
    /// Returns an empty vec if index >= scrollback_count.
    pub(crate) fn scrollback_row_cells_styled(&self, index: usize) -> Vec<ScrollbackCell> {
        match self.scrollback_slim.get(index) {
            Some(row) => self.slim_row_cells_styled(row),
            None => Vec::new(),
        }
    }
}

// ── scrollback API (was wasm_bindgen) ────────────────────

impl TerminalCore {
    /// Get the number of scrollback lines.
    ///
    /// While the snapshot-replay bypass is on (see
    /// `TerminalCore::scrollback_bypass`), this returns the virtual count
    /// `virtual_scrollback_len` instead of `scrollback_count() as u32`,
    /// because the actual `scrollback_slim` deque was intentionally not
    /// populated. The virtual count evolves byte-identically to the live
    /// path's `scrollback_count()` (capped at `scrollback_capacity`), so
    /// the mark stamping site `abs_row = get_scrollback_length() +
    /// cursor.row` produces the same `abs_row` value either way.
    pub fn get_scrollback_length(&self) -> u32 {
        if self.scrollback_bypass {
            self.virtual_scrollback_len
        } else {
            self.scrollback_count() as u32
        }
    }

    /// Monotonic count of scrollback rows evicted from the oldest (front)
    /// end since construction, across both the automatic at-capacity path
    /// (`ring_push_blank`) and the explicit `evict_oldest_scrollback` API.
    /// `reset()` zeroes it. Consumers that hold absolute line indices read
    /// the delta versus their last observation to shift those indices down.
    pub fn get_scrollback_evicted_total(&self) -> u64 {
        self.scrollback_evicted_total
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

    /// Get a scrollback line decoded into `(grapheme, physical_width)`
    /// cells. index: 0 = oldest line. Width-0 continuation halves of wide
    /// glyphs are dropped so the result aligns with the per-cell
    /// `get_cell_char` / `get_cell_width` viewport accessors; empty cells
    /// yield an empty grapheme string. Returns an empty vec when
    /// index >= scrollback length. High-level replacement for callers that
    /// would otherwise re-implement the [`get_scrollback_row_packed`]
    /// binary layout to read cell text + width.
    ///
    /// [`get_scrollback_row_packed`]: Self::get_scrollback_row_packed
    pub fn get_scrollback_row_cells(&self, index: u32) -> Vec<(String, u16)> {
        self.scrollback_row_cells(index as usize)
    }

    /// Get a scrollback line decoded into styled [`ScrollbackCell`]s.
    /// index: 0 = oldest line. Each cell carries its grapheme, display width,
    /// and style packed identically to the viewport `get_cell_fg` /
    /// `get_cell_bg` (`PackedColor::to_u32()`) / `get_cell_flags` (`u16`
    /// bitset) accessors, so a renderer can resolve scrollback cells through
    /// the same style path it uses for the live viewport. Width-0
    /// continuation halves of wide glyphs are dropped to align with those
    /// accessors. Returns an empty vec when index >= scrollback length.
    pub fn get_scrollback_row_cells_styled(&self, index: u32) -> Vec<ScrollbackCell> {
        self.scrollback_row_cells_styled(index as usize)
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

    /// Bounded scrollback eviction (FR4): drop oldest scrollback rows until the
    /// scrollback length is at most `target_len`, releasing intern refcounts for
    /// each dropped row. Used by the cross-pane global scrollback budget
    /// enforcer to shed memory pressure from the oldest history.
    ///
    /// No-op when current length is already ≤ `target_len`. Returns the number
    /// of rows evicted.
    pub fn evict_oldest_scrollback(&mut self, target_len: u32) -> u32 {
        let target = target_len as usize;
        let current = self.scrollback_slim.len();
        if current <= target {
            return 0;
        }
        let to_evict = current - target;
        let mut evicted = 0u32;
        for _ in 0..to_evict {
            match self.scrollback_slim.pop_front() {
                Some(old) => {
                    self.release_slim_row(&old);
                    self.scrollback_wrapped.pop_front();
                    evicted += 1;
                }
                None => break,
            }
        }
        if evicted > 0 {
            self.scrollback_evicted_total += evicted as u64;
            self.mark_all_dirty();
        }
        evicted
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
mod tests;
