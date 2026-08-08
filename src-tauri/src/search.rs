//! In-terminal text search over scrollback + viewport.
//!
//! Port of the WebView build's `src/terminal/search/search-state.ts`
//! (incremental search, regex / case toggles, 200 ms timeout, 10 000-char
//! line cap, zero-width-match skip, wrap-around navigation) and the
//! search half of `src/terminal-app/handlers/search.ts` (search target =
//! scrollback + screen, scroll-to-match formula).
//!
//! The native build improves on the WebView in one way: matches are run
//! against **soft-wrap-joined logical lines** rather than each physical
//! row, so a query that straddles a wrap boundary (or the
//! scrollback↔viewport seam) is found. This reuses the same logical-line
//! idea as [`crate::links`], extended to walk scrollback rows as well as
//! the viewport. A match's logical char range is resolved back into one
//! or more physical `(abs_row, col_start, col_end)` segments — one per
//! physical row it touches — which the renderer paints as highlight
//! rectangles.
//!
//! "Absolute row" numbering matches the scroll model in
//! [`crate::app`]: `0..scrollback_len` are scrollback rows (0 = oldest),
//! `scrollback_len + r` is viewport row `r`. The renderer converts an
//! absolute row to a screen row via the live scroll offset.

use term_core::terminal_core::TerminalCore;

use crate::logical_line::{LogicalLine, Segment};

/// Maximum search execution time. Mirrors `SEARCH_TIMEOUT_MS`.
const SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// Error surfaced when the match loop exceeds [`SEARCH_TIMEOUT`]. Mirrors
/// the WebView build's `search-state.ts` "Search timed out" message so the
/// two builds report the same text.
const SEARCH_TIMEOUT_MESSAGE: &str = "Search timed out";

/// Logical lines between timeout checks. Mirrors `TIMEOUT_CHECK_INTERVAL`.
const TIMEOUT_CHECK_INTERVAL: usize = 100;

/// Maximum logical-line length (in `char`s) to search; longer lines are
/// truncated, matching `MAX_SEARCH_LINE_LENGTH` + the WebView's `slice`.
const MAX_SEARCH_LINE_LENGTH: usize = 10000;

/// One physical highlight segment: an inclusive-exclusive column span on
/// a single absolute row (`col_start <= col < col_end`). A match that
/// crosses a soft-wrap boundary yields one segment per physical row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSegment {
    pub abs_row: u32,
    pub col_start: u16,
    pub col_end: u16,
}

impl From<Segment<u32>> for MatchSegment {
    fn from(seg: Segment<u32>) -> Self {
        MatchSegment {
            abs_row: seg.row,
            col_start: seg.col_start,
            col_end: seg.col_end,
        }
    }
}

/// A single search hit: the physical cell segments it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub segments: Vec<MatchSegment>,
}

/// Search state: query, options, results, and navigation cursor. Mirrors
/// `SearchStateManager`.
#[derive(Debug, Clone)]
pub struct SearchState {
    /// Whether the search bar overlay is shown.
    pub visible: bool,
    /// The current query text.
    pub query: String,
    /// Treat the query as a regular expression rather than literal text.
    pub is_regex: bool,
    /// Case-sensitive matching. Defaults to `false` (case insensitive),
    /// matching the WebView default.
    pub case_sensitive: bool,
    /// Resolved matches in document order (top of scrollback → bottom of
    /// viewport, then left → right within a logical line).
    pub matches: Vec<SearchMatch>,
    /// Index into `matches` of the currently-focused hit, or `-1` when
    /// there is no current match. Stored as `i32` to mirror the WebView's
    /// `-1` sentinel.
    pub current_index: i32,
    /// Set to `Some("Invalid regex pattern")` when the query is an
    /// unparseable regex; `None` otherwise.
    pub error: Option<String>,
    /// Cached soft-wrap logical-line document (scrollback + viewport) for
    /// the active tab's buffer. Built lazily by [`Self::execute`] and reused
    /// across keystrokes: only `query` / `is_regex` / `case_sensitive`
    /// change between most `execute` calls, and those do not alter the
    /// document, so re-running just the regex match over the cache avoids
    /// re-decoding the whole scrollback (up to 50 000 rows) per keystroke.
    /// Invalidated by [`Self::mark_buffer_dirty`] when the terminal buffer
    /// changes (see `App::on_pty_output`), and dropped on `clear`.
    doc_cache: Vec<LogicalLine<u32>>,
    /// Whether [`Self::doc_cache`] is stale and must be rebuilt on the next
    /// [`Self::execute`]. Starts `true` so the first search builds it.
    doc_dirty: bool,
    /// Cached compiled regex plus the `(query, is_regex, case_sensitive)`
    /// key it was built from. [`Self::execute`] re-runs the match over the
    /// cached document on every keystroke / auto re-search; when the query
    /// text and the two option flags are unchanged (the auto-re-search case,
    /// where only the buffer moved) this avoids paying a fresh
    /// `RegexBuilder::build` per frame. Dropped on `clear` / `close`.
    regex_cache: Option<RegexCacheEntry>,
    /// Test-only count of actual `RegexBuilder::build` calls, used to assert
    /// the cache short-circuits recompilation. Not reset by `clear` so a
    /// test can count compilations across the whole search lifecycle.
    #[cfg(test)]
    regex_compile_count: u32,
    /// Test-only override of [`SEARCH_TIMEOUT`]. When `Some`, the match loop
    /// uses this (typically near-zero) budget so a test can drive the
    /// timeout branch deterministically without a huge document.
    #[cfg(test)]
    test_timeout_override: Option<std::time::Duration>,
}

/// A compiled regex paired with the inputs that produced it. A cache hit
/// requires all three to match the current [`SearchState`] fields, since
/// each independently changes the compiled pattern (`query` is the source,
/// `is_regex` toggles literal-escaping, `case_sensitive` flips the
/// `case_insensitive` flag).
#[derive(Debug, Clone)]
struct RegexCacheEntry {
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    regex: regex::Regex,
}

impl Default for SearchState {
    fn default() -> Self {
        SearchState {
            visible: false,
            query: String::new(),
            is_regex: false,
            case_sensitive: false,
            matches: Vec::new(),
            current_index: 0,
            error: None,
            doc_cache: Vec::new(),
            // No document built yet → the first execute must build it.
            doc_dirty: true,
            regex_cache: None,
            #[cfg(test)]
            regex_compile_count: 0,
            #[cfg(test)]
            test_timeout_override: None,
        }
    }
}

impl SearchState {
    /// Open the overlay. Idempotent re-show: keeps the existing query /
    /// matches so re-pressing the search chord re-focuses without losing
    /// state (the focus + select-all is handled in the UI layer).
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Close the overlay and clear all search state. Mirrors
    /// `handleSearchClose` (`clear()` + hide).
    pub fn close(&mut self) {
        self.visible = false;
        self.clear();
    }

    /// Reset query, matches, and error. Mirrors `clear`. Also drops the
    /// cached logical-line document so a re-opened overlay rebuilds it
    /// against the (possibly changed) buffer.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_index = -1;
        self.error = None;
        self.doc_cache = Vec::new();
        self.doc_dirty = true;
        self.regex_cache = None;
    }

    /// Invalidate the cached logical-line document. Called when the active
    /// tab's terminal buffer changes (PTY output, scroll, resize) while the
    /// overlay is open, so the next [`Self::execute`] rebuilds it. Matching
    /// (next / prev navigation) does not rebuild, so this is a no-op cost
    /// when the bar is idle.
    pub fn mark_buffer_dirty(&mut self) {
        self.doc_dirty = true;
    }

    /// Whether a buffer change has left the cached document (and therefore
    /// the resolved [`Self::matches`] / their `abs_row`s) stale while the
    /// overlay is showing live results. Used by the frame loop to decide
    /// whether an auto re-search is needed so highlights follow the text
    /// as new PTY output pushes rows into scrollback. Returns `false` for an
    /// empty query (nothing to re-resolve) or a hidden overlay (no highlights
    /// painted), so the common idle / typing-only path skips the rebuild.
    pub fn needs_research(&self) -> bool {
        self.visible && self.doc_dirty && !self.query.is_empty()
    }

    /// The current match, if any. Mirrors `getCurrentMatch`.
    pub fn current_match(&self) -> Option<&SearchMatch> {
        if self.current_index < 0 {
            return None;
        }
        self.matches.get(self.current_index as usize)
    }

    /// Advance to the next match, wrapping around. Mirrors `nextMatch`.
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len() as i32;
        self.current_index = (self.current_index + 1).rem_euclid(len);
    }

    /// Step to the previous match, wrapping around. Mirrors `prevMatch`.
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len() as i32;
        self.current_index = (self.current_index - 1).rem_euclid(len);
    }

    /// Re-run the search against `core` with the current query / options.
    /// Mirrors `executeSearch`, but over soft-wrap-joined logical lines
    /// spanning scrollback + viewport. User-driven edits (query / option
    /// change) call this; the focus resets to the first match.
    pub fn execute(&mut self, core: &TerminalCore) {
        self.execute_inner(core, false);
    }

    /// Like [`Self::execute`] but preserves the navigation cursor across the
    /// re-resolve. Used by the per-frame auto re-search (buffer moved but the
    /// user did not retype), where snapping the current match back to `0`
    /// would jitter the N/M indicator on every PTY chunk. The old
    /// `current_index` is clamped into the new `matches` range.
    pub fn execute_preserving_current(&mut self, core: &TerminalCore) {
        self.execute_inner(core, true);
    }

    /// Shared search body. `preserve_current` selects between the
    /// reset-to-first (user edit) and clamp-existing (auto re-search)
    /// navigation policies.
    ///
    /// The logical-line document is cached (see [`Self::doc_cache`]): a
    /// keystroke that only changes the query / options reuses it and re-runs
    /// just the regex match, skipping the scrollback re-decode. The cache is
    /// rebuilt when [`Self::mark_buffer_dirty`] flagged the buffer as
    /// changed (or on the first call after `clear`). The compiled regex is
    /// likewise cached (see [`Self::regex_cache`]) so an auto re-search over
    /// an unchanged query skips `RegexBuilder::build`.
    fn execute_inner(&mut self, core: &TerminalCore, preserve_current: bool) {
        // Remember the focus so the clamp-existing policy can restore it; the
        // body below resets `current_index` to `-1` while it rebuilds.
        let previous_index = self.current_index;
        self.matches.clear();
        self.current_index = -1;
        self.error = None;

        // Refresh the cached document only when the buffer actually changed
        // since the last build. This is the per-keystroke fast path: an
        // incremental query edit leaves `doc_dirty == false`.
        if self.doc_dirty {
            self.doc_cache = build_logical_lines(core);
            self.doc_dirty = false;
        }

        if self.query.is_empty() {
            return;
        }

        // Compile (or reuse) the regex. Plain queries are escaped; the case
        // flag maps to `RegexBuilder::case_insensitive`. Mirrors the
        // WebView's `new RegExp(escaped, "gi" | "g")`. On an unparseable
        // regex this sets `self.error` and returns the prior cache untouched.
        let re = match self.compiled_regex() {
            Ok(re) => re,
            Err(()) => {
                self.error = Some("Invalid regex pattern".to_string());
                return;
            }
        };

        // Match into a local vec first so the `&self.doc_cache` borrow does
        // not collide with the `&mut self.matches` assignment.
        let mut matches: Vec<SearchMatch> = Vec::new();
        let start = std::time::Instant::now();
        #[cfg(test)]
        let timeout = self.test_timeout_override.unwrap_or(SEARCH_TIMEOUT);
        #[cfg(not(test))]
        let timeout = SEARCH_TIMEOUT;

        for (i, line) in self.doc_cache.iter().enumerate() {
            // Timeout guard: check every 100 logical lines (matching the
            // WebView's per-100-line cadence). On timeout, drop all
            // matches, surface an error (SPEC.md: search timed out), and
            // bail — mirrors `executeSearch`'s reset.
            if i > 0 && i % TIMEOUT_CHECK_INTERVAL == 0 && start.elapsed() > timeout {
                self.matches.clear();
                self.current_index = -1;
                self.error = Some(SEARCH_TIMEOUT_MESSAGE.to_string());
                return;
            }

            if line.text.is_empty() {
                continue;
            }

            // Truncate over-long logical lines to bound single-line
            // regex backtracking (ReDoS). The WebView slices the string;
            // here we cap the searchable char count and clamp the regex
            // input to that prefix's byte length.
            let search_text: &str = if line.cells.len() > MAX_SEARCH_LINE_LENGTH {
                let byte_end = line.byte_offset_of_char(MAX_SEARCH_LINE_LENGTH);
                &line.text[..byte_end]
            } else {
                &line.text
            };

            // Incremental byte→char cursor so each match's char offset is
            // computed in O(line) total rather than O(matches * line) —
            // same trick as `links::find_url_covering`.
            let mut last_byte = 0usize;
            let mut last_char = 0usize;

            for m in re.find_iter(search_text) {
                if m.start() == m.end() {
                    // Zero-width match: `find_iter` already advances past
                    // it without looping, so we simply ignore it. Mirrors
                    // the WebView's `lastIndex++` skip.
                    continue;
                }
                debug_assert!(m.start() >= last_byte);
                last_char += search_text[last_byte..m.start()].chars().count();
                let start_char = last_char;
                let match_chars = search_text[m.start()..m.end()].chars().count();
                last_char += match_chars;
                last_byte = m.end();
                let end_char = start_char + match_chars;

                let segments: Vec<MatchSegment> = line
                    .char_range_to_segments(start_char, end_char)
                    .into_iter()
                    .map(MatchSegment::from)
                    .collect();
                if !segments.is_empty() {
                    matches.push(SearchMatch { segments });
                }
            }
        }

        self.matches = matches;
        if !self.matches.is_empty() {
            self.current_index = if preserve_current {
                // Clamp the prior focus into the new range so the auto
                // re-search keeps the user's place; a fresh result set
                // (previous_index < 0) falls back to the first hit.
                let len = self.matches.len() as i32;
                previous_index.clamp(0, len - 1)
            } else {
                0
            };
        }
    }

    /// Compile the current `(query, is_regex, case_sensitive)` into a regex,
    /// reusing [`Self::regex_cache`] when those three inputs are unchanged.
    /// Returns a cheap clone of the cached/compiled `Regex` (the program is
    /// `Arc`-shared internally) so the caller can hold it without keeping a
    /// borrow on `self` across the match loop. `Err(())` on an unparseable
    /// regex (the cache is left untouched).
    fn compiled_regex(&mut self) -> Result<regex::Regex, ()> {
        let hit = self.regex_cache.as_ref().is_some_and(|c| {
            c.query == self.query
                && c.is_regex == self.is_regex
                && c.case_sensitive == self.case_sensitive
        });
        if hit {
            return Ok(self
                .regex_cache
                .as_ref()
                .expect("hit checked")
                .regex
                .clone());
        }
        let pattern = if self.is_regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .build()
            .map_err(|_| ())?;
        #[cfg(test)]
        {
            self.regex_compile_count += 1;
        }
        self.regex_cache = Some(RegexCacheEntry {
            query: self.query.clone(),
            is_regex: self.is_regex,
            case_sensitive: self.case_sensitive,
            regex: re.clone(),
        });
        Ok(re)
    }

    /// Test-only accessor: number of cached logical lines. `0` after a
    /// `clear` / fresh state (cache not yet built). Lets a test assert the
    /// document was (re)built or reused.
    #[cfg(test)]
    pub(crate) fn doc_cache_len(&self) -> usize {
        self.doc_cache.len()
    }

    /// Test-only accessor: total number of `RegexBuilder::build` calls this
    /// state has performed. A cache hit in [`Self::compiled_regex`] does not
    /// bump it, so a test can assert that an option-unchanged re-`execute`
    /// reused the compiled regex instead of recompiling.
    #[cfg(test)]
    pub(crate) fn regex_compile_count(&self) -> u32 {
        self.regex_compile_count
    }
}

/// Decode the document (scrollback then viewport) directly into soft-wrap
/// logical lines over the shared [`LogicalLine`] model (absolute `u32` row
/// coordinates), in a single pass. Each physical row is folded into `lines`
/// as it is decoded: a row whose `wrapped` flag is set is appended to the
/// previous logical line rather than starting a new one. The empty-cell →
/// space substitution and per-char physical mapping live in
/// [`LogicalLine::push_row`].
///
/// Unlike the earlier two-pass form, no intermediate `Vec<PhysRow>` of the
/// whole document is materialized. The only per-row temporary is the
/// `(grapheme, width)` cell vector for the row currently being folded
/// (`get_scrollback_row_cells`'s return, or the viewport-row decode below),
/// which is dropped before the loop advances to the next row — so peak
/// memory is `O(doc)` for the logical lines plus `O(1 row)` for the
/// in-flight cells, not `O(doc)` twice.
fn build_logical_lines(core: &TerminalCore) -> Vec<LogicalLine<u32>> {
    let cols = core.cols();
    let rows = core.rows();
    let scrollback_len = core.get_scrollback_length();
    let mut lines: Vec<LogicalLine<u32>> = Vec::new();
    let mut abs_row: u32 = 0;

    // Fold one physical row's `(grapheme, width)` cells into `lines` at the
    // given absolute row, joining onto the previous logical line when
    // `wrapped` (and a predecessor exists — `wrapped` on the very first row
    // has nothing to attach to, so it starts a fresh line).
    let push_phys = |lines: &mut Vec<LogicalLine<u32>>,
                     abs_row: u32,
                     cells: &[(String, u16)],
                     wrapped: bool| {
        let continues = wrapped && !lines.is_empty();
        if !continues {
            lines.push(LogicalLine::new());
        }
        let line = lines.last_mut().expect("at least one line pushed");
        line.push_row(abs_row, cells.iter().map(|(c, w)| (c.as_str(), *w)));
    };

    // Scrollback rows: term_core's accessor owns the packed-format decode
    // and already drops width-0 continuation halves. The returned `Vec` is
    // dropped at the end of each iteration.
    for idx in 0..scrollback_len {
        let cells = core.get_scrollback_row_cells(idx);
        let wrapped = core.get_scrollback_line_wrapped(idx);
        push_phys(&mut lines, abs_row, &cells, wrapped);
        abs_row += 1;
    }

    // Viewport rows: decode per-cell via `get_cell_char` / `get_cell_width`.
    // The first viewport row's `get_line_wrapped(0)` is true when it
    // continues the last scrollback row, so the scrollback↔viewport seam
    // joins into one logical line. The per-row `cells` vector is reused
    // (cleared between rows) so only one row's worth lives at a time.
    let mut cells: Vec<(String, u16)> = Vec::with_capacity(cols as usize);
    for r in 0..rows {
        cells.clear();
        for c in 0..cols {
            // Skip the trailing half of a wide glyph (width 0), same as
            // links::build_logical_line, so the char→cell map stays aligned.
            if core.get_cell_width(c, r) == 0 {
                continue;
            }
            let ch = core.get_cell_char(c, r);
            let width = core.get_cell_width(c, r).max(1) as u16;
            cells.push((ch, width));
        }
        push_phys(&mut lines, abs_row, &cells, core.get_line_wrapped(r));
        abs_row += 1;
    }

    lines
}

/// Compute the scroll offset (rows back from live) that brings `abs_row`
/// roughly to the vertical center of the viewport, or `None` when the row
/// is already visible at the current offset. Mirrors `scrollToCurrentMatch`
/// in `handlers/search.ts`, translated to the native offset coordinate
/// system (offset = `scrollback_len - top_visible_abs_row`).
///
/// `scrollback_len` is the live scrollback length, `rows` the viewport
/// height, `current_offset` the live `App::scroll_offset()`.
pub fn scroll_offset_for_match(
    abs_row: u32,
    scrollback_len: u32,
    rows: u16,
    current_offset: u32,
) -> Option<u32> {
    // Top visible absolute row at the current offset. `offset` counts
    // rows back from live, and the live (offset 0) top row is
    // `scrollback_len`, so the visible window is
    // `[scrollback_len - offset, scrollback_len - offset + rows)`.
    let visible_start = scrollback_len.saturating_sub(current_offset);
    let visible_end = visible_start + rows as u32;
    if abs_row >= visible_start && abs_row < visible_end {
        return None; // already visible
    }
    // Center the match: WebView uses
    // `max(0, scrollbackLength - matchLineIndex + floor(rows/2))`.
    let half = (rows / 2) as i64;
    let target = scrollback_len as i64 - abs_row as i64 + half;
    Some(target.max(0) as u32)
}

#[cfg(test)]
mod tests;
