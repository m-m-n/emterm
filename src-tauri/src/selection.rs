//! Selection state + resolution.
//!
//! PoC scope: line-based selection only (no rectangular selection). The
//! selection is described by an anchor and an extent, both in **absolute
//! row** cell coordinates (the same frame `fold.rs` / `prompts.rs` /
//! `search.rs` use): absolute rows `0..scrollback_len` are scrollback, and
//! `scrollback_len + r` is live viewport row `r`. The row shown on screen
//! at `screen_row` resolves to absolute row `visible_start + screen_row`,
//! where `visible_start = scrollback_len - scroll_offset` (saturating). A
//! selection therefore stays pinned to its buffer content as the viewport
//! scrolls, and `resolve` copies the text that was selected rather than the
//! cells that happen to occupy those screen rows afterwards.
//!
//! `contains` answers "is this absolute-row cell currently selected?", and
//! `resolve` walks the terminal core (scrollback + live viewport) to
//! produce the text string for clipboard copy.
//!
//! Phase 6: `resolve` reads cells through `term_core::TerminalCore` instead
//! of the Phase 1 PoC's bespoke `Grid` type.
//!
//! Phase 4 sub-phase 4 adds:
//! - `SelectionMode { Character, Word, Line }` so double / triple click can
//!   snap the resolved range to a word or full line, matching xterm / VTE.
//! - `Selection::extend(pos, core)` which keeps the anchor pinned and walks
//!   the extent, snapping to word or line boundaries when the mode is not
//!   `Character`.
//! - `sanitize_bracket_sequences` + `bracketed_paste` helpers used by the
//!   paste path. A pasted body must not contain `\e[201~` (would otherwise
//!   close the bracketed paste prematurely, letting the inner content escape
//!   into command interpretation).

use term_core::terminal_core::TerminalCore;

/// Click-count-derived selection grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Free per-cell range (single click + drag).
    #[default]
    Character,
    /// Snap both endpoints to the word containing the anchor / extent.
    Word,
    /// Snap to the whole row(s).
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    /// Absolute buffer row: `0..scrollback_len` is scrollback,
    /// `scrollback_len + r` is live viewport row `r`.
    pub row: u32,
    pub col: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub extent: Pos,
    pub mode: SelectionMode,
    /// Immutable press position (the pivot) in absolute-row coordinates.
    /// Word / line extensions recompute both endpoints from `(origin, pointer)`
    /// so the originally clicked word / line always stays inside the range no
    /// matter how many motion events fire. Only scrollback eviction
    /// ([`shift_rows_down`]) ever moves it; `extend` never does.
    pub origin: Pos,
}

impl Selection {
    pub fn new(anchor: Pos) -> Self {
        Self {
            anchor,
            extent: anchor,
            mode: SelectionMode::Character,
            origin: anchor,
        }
    }

    pub fn new_with_mode(anchor: Pos, mode: SelectionMode) -> Self {
        Self {
            anchor,
            extent: anchor,
            mode,
            origin: anchor,
        }
    }

    /// Compute the normalized (start, end) where `start <= end` in
    /// reading order. Honors `mode`: word selections expand each endpoint
    /// to its word boundary; line selections cover whole rows.
    pub fn ordered(&self) -> (Pos, Pos) {
        let (a, b) = (self.anchor, self.extent);
        if (a.row, a.col) <= (b.row, b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Extend the selection so it spans the pointer at `pos`.
    ///
    /// `Character` mode moves only the free endpoint (`extent`), leaving the
    /// `anchor` pinned at the origin press cell. `Word` / `Line` modes
    /// recompute *both* endpoints from the pair `(origin, pointer)` against
    /// `core`, so the range is a pure function of the immutable origin and the
    /// latest pointer position — repeated extensions never drift the origin
    /// word / line out of the selection.
    pub fn extend(&mut self, pos: Pos, core: &TerminalCore) {
        match self.mode {
            SelectionMode::Character => {
                self.extent = pos;
            }
            SelectionMode::Word => {
                // Word boundaries for both the immutable origin and the live
                // pointer, looked up against the current core each time.
                let (origin_start_col, origin_end_col) =
                    word_boundary(core, self.origin.row, self.origin.col);
                let (pointer_start_col, pointer_end_col) = word_boundary(core, pos.row, pos.col);
                // Union of the two words in reading order: the earliest start
                // and the latest end (tuple ordering compares row then col).
                // When the pointer sits inside the origin word both pairs are
                // equal, so the range collapses to exactly the origin word.
                let (start_row, start_col) =
                    (self.origin.row, origin_start_col).min((pos.row, pointer_start_col));
                let (end_row, end_col) =
                    (self.origin.row, origin_end_col).max((pos.row, pointer_end_col));
                self.anchor = Pos {
                    row: start_row,
                    col: start_col,
                };
                self.extent = Pos {
                    row: end_row,
                    col: end_col,
                };
            }
            SelectionMode::Line => {
                let last_col = core.cols().saturating_sub(1);
                // Full rows from the lower of (origin row, pointer row) to the
                // higher; the origin row is always inside the covered span.
                let (top_row, bottom_row) = if self.origin.row <= pos.row {
                    (self.origin.row, pos.row)
                } else {
                    (pos.row, self.origin.row)
                };
                self.anchor = Pos {
                    row: top_row,
                    col: 0,
                };
                self.extent = Pos {
                    row: bottom_row,
                    col: last_col,
                };
            }
        }
    }

    /// Is the cell at absolute `(abs_row, col)` inside the selection
    /// (inclusive of both endpoints)?
    pub fn contains(&self, abs_row: u32, col: u16) -> bool {
        let (start, end) = self.ordered();
        if abs_row < start.row || abs_row > end.row {
            return false;
        }
        if start.row == end.row {
            return col >= start.col && col <= end.col;
        }
        if abs_row == start.row {
            return col >= start.col;
        }
        if abs_row == end.row {
            return col <= end.col;
        }
        true
    }

    /// Shift both endpoints (and the origin pivot) down by `delta` rows
    /// after a scrollback eviction. Returns `false` when the whole
    /// selection fell off the top of the frame (caller should drop it). An
    /// endpoint that underflows alone clamps to row 0 col 0.
    pub fn shift_rows_down(&mut self, delta: u32) -> bool {
        // Both endpoints scrolled off the top of scrollback: the entire
        // selection is gone. (The origin pivot sits between the endpoints,
        // so this drop rule stays keyed on the endpoints alone.)
        if self.anchor.row < delta && self.extent.row < delta {
            return false;
        }
        let shift = |p: &mut Pos| match p.row.checked_sub(delta) {
            Some(r) => p.row = r,
            None => {
                // This endpoint alone fell off the top; clamp it to the new
                // frame's first cell so the surviving range still covers
                // retained rows.
                p.row = 0;
                p.col = 0;
            }
        };
        shift(&mut self.anchor);
        shift(&mut self.extent);
        // The origin follows the same row delta / clamp as the endpoints so a
        // later word / line extension still pivots on the correct cell.
        shift(&mut self.origin);
        true
    }

    /// Resolve the selection against a terminal core into a plain text
    /// string. Each row is read in the absolute-row frame (scrollback or
    /// live viewport) via [`row_cells`], so the copied text matches the
    /// content that was selected regardless of how far the viewport has
    /// scrolled since. Lines are joined with `\n`. Trailing whitespace on
    /// each line is trimmed (typical terminal copy behavior).
    ///
    /// When `fold_layout` is `Some`, rows hidden inside a collapsed fold
    /// body are skipped so the copied text matches what is on screen. The
    /// region's summary/start line (the command line) is kept — it is the
    /// row the on-screen summary represents and the row `screen_row_to_abs`
    /// maps a summary click to. The WebView build copies hidden rows too;
    /// the native build is fold-aware here (same "native leads" pattern as
    /// the search logical-line merge).
    pub fn resolve(
        &self,
        core: &TerminalCore,
        fold_layout: Option<&crate::fold::FoldLayout>,
    ) -> String {
        let (start, end) = self.ordered();
        let cols = core.cols();
        let rows = core.rows();
        if rows == 0 || cols == 0 {
            return String::new();
        }
        // Clamp the absolute-row range to the live buffer (scrollback rows +
        // the viewport rows). The endpoints are already bounded by the input
        // paths, but a stale / over-wide extent must not make the loop walk an
        // unbounded range; rows past `max_row` hold no content. `start.row >
        // max_row` yields an empty `start.row..=end_row` range (no panic).
        let max_row = core.get_scrollback_length() + rows as u32 - 1;
        let end_row = end.row.min(max_row);
        // Stream the kept rows straight into one `String`, emitting a `\n`
        // before each kept row after the first. Streaming (rather than
        // collecting a `Vec<String>` then joining) keeps peak allocation at a
        // single copy of the selected text even for a scrollback-spanning
        // selection, while the `wrote_any` gate keeps the newline count
        // correct when collapsed-body rows are skipped mid-range.
        let mut out = String::new();
        let mut wrote_any = false;
        for row in start.row..=end_row {
            // Skip rows hidden inside a collapsed fold body. The region's
            // summary/start line (row == region.start_line, the command line)
            // is kept; only start_line+1..end_line (the hidden output) is
            // dropped, so copied text matches what is visible.
            if let Some(layout) = fold_layout {
                if let Some(region) = layout.region_at_line(row) {
                    if row > region.start_line {
                        continue;
                    }
                }
            }
            if wrote_any {
                out.push('\n');
            }
            wrote_any = true;
            let cells = row_cells(core, row);
            // Each absolute row's column extent is clamped to the row's own
            // length, since scrollback rows may have been stored at a width
            // that differs from the current `cols`. An empty row (off the
            // wrap / out of range) contributes a blank line.
            if !cells.is_empty() {
                let last = cells.len() - 1;
                let (c0, c1) = if start.row == end.row {
                    (start.col as usize, (end.col as usize).min(last))
                } else if row == start.row {
                    (start.col as usize, last)
                } else if row == end.row {
                    (0, (end.col as usize).min(last))
                } else {
                    (0, last)
                };
                if c0 <= c1 {
                    // Append this row's cells, then trim its trailing spaces
                    // (terminal copy convention) by truncating back to the
                    // trimmed length of the segment just written.
                    let line_start = out.len();
                    for cell in &cells[c0..=c1] {
                        if !cell.is_empty() {
                            out.push_str(cell);
                        }
                    }
                    let trimmed_len = out[line_start..].trim_end_matches(' ').len();
                    out.truncate(line_start + trimmed_len);
                }
            }
        }
        out
    }
}

/// Read one buffer row (scrollback or live viewport) as a col-indexed
/// vector of graphemes. Wide glyphs occupy their leading column; the
/// trailing columns they cover hold empty strings (mirrors the live
/// grid's `get_cell_char` behavior).
fn row_cells(core: &TerminalCore, abs_row: u32) -> Vec<String> {
    let scrollback_len = core.get_scrollback_length();
    if abs_row < scrollback_len {
        // `get_scrollback_row_cells` returns `(grapheme, width)` with the
        // width-0 continuation halves already dropped, so re-expand each
        // grapheme back into a col-indexed run: the grapheme on its leading
        // column followed by `width - 1` empty strings.
        let packed = core.get_scrollback_row_cells(abs_row);
        let mut out: Vec<String> = Vec::with_capacity(packed.len());
        for (glyph, width) in packed {
            out.push(glyph);
            for _ in 1..width.max(1) {
                out.push(String::new());
            }
        }
        out
    } else {
        let cols = core.cols();
        let rows = core.rows();
        // Range-check in u32 *before* narrowing: a stale row far past the
        // live grid must not truncate into a valid-looking screen row.
        let live_row = abs_row - scrollback_len;
        if live_row >= rows as u32 {
            return Vec::new();
        }
        let screen_row = live_row as u16;
        // Live viewport row: 0..cols already addresses every column directly
        // (the trailing half of a wide glyph reports width 0 and an empty
        // grapheme), so a straight scan produces the col-indexed form.
        let mut out: Vec<String> = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            out.push(core.get_cell_char(col, screen_row));
        }
        out
    }
}

/// Word boundary classifier. ASCII alnum + `_` are word chars; everything
/// else (including non-ASCII) defers to `char::is_alphanumeric`. Whitespace
/// and an empty cell terminate a word.
fn is_word_char(ch: &str) -> bool {
    if ch.is_empty() {
        return false;
    }
    let mut chars = ch.chars();
    let c = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if chars.next().is_some() {
        // Multi-codepoint cluster (emoji ZWJ, etc.) — treat as word.
        return !c.is_whitespace();
    }
    c == '_' || c.is_alphanumeric()
}

/// Find the (start_col, end_col) of the contiguous word covering absolute
/// (`abs_row`, `col`) on a single row. If the cell at the cursor is not a
/// word character, the range collapses to a single cell. Boundaries are
/// taken against the row's own length read through [`row_cells`], since a
/// scrollback row may have been stored at a width that differs from the
/// current `cols`.
pub(crate) fn word_boundary(core: &TerminalCore, abs_row: u32, col: u16) -> (u16, u16) {
    let scrollback_len = core.get_scrollback_length();
    if abs_row >= scrollback_len {
        // Live viewport row: scan outward lazily (bounded by cols) instead of
        // materializing the whole row, since get_cell_char gives O(1) per-cell
        // access. Matches the pre-absolute-row implementation's cost.
        let cols = core.cols();
        let rows = core.rows();
        if cols == 0 {
            return (col, col);
        }
        let live_row = abs_row - scrollback_len;
        if live_row >= rows as u32 {
            return (col, col);
        }
        let screen_row = live_row as u16;
        let col = col.min(cols - 1);
        if !is_word_char(&core.get_cell_char(col, screen_row)) {
            return (col, col);
        }
        let mut start = col;
        while start > 0 {
            if !is_word_char(&core.get_cell_char(start - 1, screen_row)) {
                break;
            }
            start -= 1;
        }
        let mut end = col;
        while end + 1 < cols {
            if !is_word_char(&core.get_cell_char(end + 1, screen_row)) {
                break;
            }
            end += 1;
        }
        (start, end)
    } else {
        // Scrollback row: no per-cell accessor exists, so materialize the row
        // once (glyphs are moved, not cloned) and scan the column vector.
        let cells = row_cells(core, abs_row);
        if cells.is_empty() {
            return (col, col);
        }
        let last = cells.len() - 1;
        let col = (col as usize).min(last);
        if !is_word_char(&cells[col]) {
            return (col as u16, col as u16);
        }
        let mut start = col;
        while start > 0 {
            if !is_word_char(&cells[start - 1]) {
                break;
            }
            start -= 1;
        }
        let mut end = col;
        while end < last {
            if !is_word_char(&cells[end + 1]) {
                break;
            }
            end += 1;
        }
        (start as u16, end as u16)
    }
}

/// Strip embedded bracketed-paste end markers from the body so a malicious
/// paste cannot terminate the wrapping prematurely. Replaces every
/// occurrence of `\e[201~` with the empty string.
pub fn sanitize_bracket_sequences(text: &str) -> String {
    text.replace("\x1b[201~", "")
}

/// Wrap `text` for transmission to the PTY. When `enabled` is true, the
/// payload is `ESC [ 200 ~ <sanitized body> ESC [ 201 ~`. Otherwise the
/// sanitized body is returned as-is. (We still sanitize even when
/// bracketing is off, so a pasted bracket-end marker never reaches the
/// shell as an injected control.)
pub fn bracketed_paste(text: &str, enabled: bool) -> String {
    let body = sanitize_bracket_sequences(text);
    if enabled {
        let mut out = String::with_capacity(body.len() + 12);
        out.push_str("\x1b[200~");
        out.push_str(&body);
        out.push_str("\x1b[201~");
        out
    } else {
        body
    }
}

#[cfg(test)]
mod tests;
