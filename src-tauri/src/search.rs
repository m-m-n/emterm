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
    let mut push_phys = |lines: &mut Vec<LogicalLine<u32>>,
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
mod tests {
    use super::*;
    use term_core::terminal_core::TerminalCore;

    fn core_with(cols: u16, rows: u16, scrollback: u32, input: &[u8]) -> TerminalCore {
        let mut core = TerminalCore::new(cols, rows, scrollback);
        core.process_pty_data(input);
        core
    }

    fn search(
        core: &TerminalCore,
        query: &str,
        is_regex: bool,
        case_sensitive: bool,
    ) -> SearchState {
        let mut s = SearchState::default();
        s.query = query.to_string();
        s.is_regex = is_regex;
        s.case_sensitive = case_sensitive;
        s.execute(core);
        s
    }

    /// Flatten a match's segments to `(abs_row, col_start, col_end)` tuples.
    fn seg_tuples(m: &SearchMatch) -> Vec<(u32, u16, u16)> {
        m.segments
            .iter()
            .map(|s| (s.abs_row, s.col_start, s.col_end))
            .collect()
    }

    #[test]
    fn plain_text_finds_matches() {
        let core = core_with(80, 3, 100, b"hello world\r\nfoo hello bar\r\nno match here");
        let s = search(&core, "hello", false, false);
        assert_eq!(s.matches.len(), 2);
        // Row 0 (scrollback_len == 0): "hello" at col 0..5.
        assert_eq!(seg_tuples(&s.matches[0]), vec![(0, 0, 5)]);
        // Row 1: "foo hello bar" → "hello" at col 4..9.
        assert_eq!(seg_tuples(&s.matches[1]), vec![(1, 4, 9)]);
        assert_eq!(s.current_index, 0);
    }

    #[test]
    fn case_insensitive_default() {
        let core = core_with(80, 3, 100, b"Hello World\r\nHELLO\r\nhello");
        let s = search(&core, "hello", false, false);
        assert_eq!(s.matches.len(), 3);
    }

    #[test]
    fn case_sensitive_matches_exact() {
        let core = core_with(80, 3, 100, b"Hello World\r\nHELLO\r\nhello");
        let s = search(&core, "hello", false, true);
        assert_eq!(s.matches.len(), 1);
        // Only the lowercase "hello" on viewport row 2.
        assert_eq!(s.matches[0].segments[0].abs_row, 2);
    }

    #[test]
    fn regex_anchored_alternation() {
        let core = core_with(
            80,
            3,
            100,
            b"error: something\r\nwarning: careful\r\ninfo: ok",
        );
        let s = search(&core, "^(error|warning):", true, false);
        assert_eq!(s.matches.len(), 2);
    }

    #[test]
    fn invalid_regex_sets_error_no_matches() {
        let core = core_with(80, 3, 100, b"test");
        let s = search(&core, "[invalid", true, false);
        assert_eq!(s.matches.len(), 0);
        assert_eq!(s.error.as_deref(), Some("Invalid regex pattern"));
    }

    #[test]
    fn zero_width_regex_skipped_no_infinite_loop() {
        // `a*` matches empty at many positions; none should be recorded,
        // and the search must terminate.
        let core = core_with(80, 3, 100, b"bbb");
        let s = search(&core, "a*", true, false);
        assert_eq!(s.matches.len(), 0);
        assert!(s.error.is_none());
    }

    #[test]
    fn wide_char_col_resolution() {
        // A double-width CJK glyph before the match shifts physical cols.
        let core = core_with(80, 3, 100, "あ hello".as_bytes());
        assert_eq!(core.get_cell_width(0, 0), 2);
        let s = search(&core, "hello", false, false);
        assert_eq!(s.matches.len(), 1);
        // "あ" (cols 0-1) + space (col 2) → "hello" starts at col 3.
        assert_eq!(seg_tuples(&s.matches[0]), vec![(0, 3, 8)]);
    }

    #[test]
    fn match_spanning_wrapped_rows() {
        // 10-col grid: "helloworld12" wraps; search "world1" straddles the
        // wrap boundary between physical row 0 and row 1.
        let core = core_with(10, 3, 100, b"helloworld12");
        assert!(core.get_line_wrapped(1), "row 1 should be a continuation");
        let s = search(&core, "world12", false, false);
        assert_eq!(s.matches.len(), 1, "wrap-spanning match found");
        let rows: std::collections::HashSet<u32> = s.matches[0]
            .segments
            .iter()
            .map(|seg| seg.abs_row)
            .collect();
        assert!(
            rows.contains(&0) && rows.contains(&1),
            "segments cover both physical rows: {:?}",
            s.matches[0].segments
        );
    }

    #[test]
    fn match_spanning_scrollback_viewport_seam() {
        // Force a 2-row viewport and overflow so a wrapped logical line
        // crosses from scrollback into the viewport. "abcdefghij" fills
        // row 0; "klmno" continues. Pushing more rows evicts the head
        // into scrollback while keeping the wrapped flag, so the logical
        // line spans the seam.
        let core = core_with(10, 2, 100, b"abcdefghijklmno\r\nx\r\ny");
        assert!(
            core.get_scrollback_length() >= 1,
            "head moved to scrollback"
        );
        let s = search(&core, "ghijkl", false, false);
        assert_eq!(s.matches.len(), 1, "seam-spanning match found");
        // The two segments live on consecutive absolute rows.
        let mut rows: Vec<u32> = s.matches[0]
            .segments
            .iter()
            .map(|seg| seg.abs_row)
            .collect();
        rows.sort_unstable();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], rows[0] + 1);
    }

    #[test]
    fn multiple_matches_single_line() {
        let core = core_with(80, 3, 100, b"aaa bbb aaa ccc aaa");
        let s = search(&core, "aaa", false, false);
        assert_eq!(s.matches.len(), 3);
        assert_eq!(seg_tuples(&s.matches[0]), vec![(0, 0, 3)]);
        assert_eq!(seg_tuples(&s.matches[1]), vec![(0, 8, 11)]);
        assert_eq!(seg_tuples(&s.matches[2]), vec![(0, 16, 19)]);
    }

    #[test]
    fn navigation_wraps_around() {
        let core = core_with(80, 3, 100, b"aaa\r\nbbb\r\naaa");
        let mut s = search(&core, "aaa", false, false);
        assert_eq!(s.matches.len(), 2);
        assert_eq!(s.current_index, 0);
        s.next_match();
        assert_eq!(s.current_index, 1);
        s.next_match();
        assert_eq!(s.current_index, 0); // wrap forward
        s.prev_match();
        assert_eq!(s.current_index, 1); // wrap backward
    }

    #[test]
    fn empty_query_no_matches_no_error() {
        let core = core_with(80, 3, 100, b"hello");
        let s = search(&core, "", false, false);
        assert_eq!(s.matches.len(), 0);
        assert!(s.error.is_none());
        assert_eq!(s.current_index, -1);
    }

    #[test]
    fn close_clears_state() {
        let core = core_with(80, 3, 100, b"hello");
        let mut s = search(&core, "hello", false, false);
        assert_eq!(s.matches.len(), 1);
        s.open();
        assert!(s.visible);
        s.close();
        assert!(!s.visible);
        assert!(s.query.is_empty());
        assert_eq!(s.matches.len(), 0);
        assert_eq!(s.current_index, -1);
        assert!(s.error.is_none());
    }

    // ── scroll-to-match coordinate translation ───────────────

    #[test]
    fn scroll_offset_none_when_already_visible() {
        // scrollback_len 100, rows 24, live (offset 0): visible rows are
        // [100, 124). Row 110 is visible → no scroll.
        assert_eq!(scroll_offset_for_match(110, 100, 24, 0), None);
    }

    #[test]
    fn scroll_offset_centers_match_above_viewport() {
        // Match at abs row 50, scrollback_len 100, rows 24, live: not
        // visible (visible starts at 100). Target = 100 - 50 + 12 = 62.
        assert_eq!(scroll_offset_for_match(50, 100, 24, 0), Some(62));
    }

    #[test]
    fn scroll_offset_clamps_to_zero() {
        // A match inside the viewport range but tested at an offset that
        // makes it invisible: abs row 120, len 100, rows 24, offset 50 →
        // visible [50, 74); 120 not visible. Target = 100 - 120 + 12 =
        // -8 → clamped to 0.
        assert_eq!(scroll_offset_for_match(120, 100, 24, 50), Some(0));
    }

    // ── logical-line document cache ──────────────────────────

    #[test]
    fn cache_reused_when_only_query_changes() {
        // Build the document from `core`, then re-run `execute` against a
        // *different* (empty) core without marking the buffer dirty. If the
        // cache is reused the second match still resolves against the
        // original document; a rebuild would scan the empty core and find
        // nothing. This proves `build_logical_lines` is not re-invoked.
        let core = core_with(80, 3, 100, b"hello world\r\nhello again");
        let empty = TerminalCore::new(80, 3, 100);

        let mut s = SearchState::default();
        s.query = "hello".to_string();
        s.execute(&core);
        assert_eq!(s.matches.len(), 2);
        let built = s.doc_cache_len();
        assert!(built > 0, "first execute builds the document cache");

        // Query-only change: re-run against the empty core. The cache must
        // be reused, so the matches still come from the original document.
        s.query = "world".to_string();
        s.execute(&empty);
        assert_eq!(
            s.matches.len(),
            1,
            "cache reused: match resolved against original document, not the empty core"
        );
        assert_eq!(
            s.doc_cache_len(),
            built,
            "document cache size unchanged (no rebuild)"
        );
    }

    #[test]
    fn cache_rebuilt_after_mark_buffer_dirty() {
        let core = core_with(80, 3, 100, b"hello world");
        let empty = TerminalCore::new(80, 3, 100);

        let mut s = SearchState::default();
        s.query = "hello".to_string();
        s.execute(&core);
        assert_eq!(s.matches.len(), 1);

        // Flag the buffer as changed, then re-run against the empty core.
        // The cache must be rebuilt from the empty core, dropping the match.
        s.mark_buffer_dirty();
        s.execute(&empty);
        assert_eq!(
            s.matches.len(),
            0,
            "after mark_buffer_dirty the cache is rebuilt from the new (empty) core"
        );
    }

    #[test]
    fn cache_reused_across_option_toggle() {
        // Toggling case sensitivity is an option-only change: the document
        // is unchanged, so the cache is reused and matching re-runs.
        let core = core_with(80, 3, 100, b"Hello hello");
        let empty = TerminalCore::new(80, 3, 100);

        let mut s = SearchState::default();
        s.query = "hello".to_string();
        s.case_sensitive = false;
        s.execute(&core);
        assert_eq!(s.matches.len(), 2, "case-insensitive matches both");

        s.case_sensitive = true;
        s.execute(&empty);
        assert_eq!(
            s.matches.len(),
            1,
            "case-sensitive re-match over the reused cache finds only lowercase"
        );
    }

    // ── auto re-search gating (needs_research) ───────────────

    #[test]
    fn needs_research_gated_by_visible_dirty_and_query() {
        let mut s = SearchState::default();
        // Default: not visible, dirty, empty query → no research.
        assert!(!s.needs_research(), "hidden overlay never researches");

        s.open();
        s.query = "x".to_string();
        // visible + dirty (fresh state starts dirty) + non-empty query.
        assert!(
            s.needs_research(),
            "visible dirty non-empty query researches"
        );

        // Empty query: nothing to re-resolve even while visible + dirty.
        s.query.clear();
        assert!(!s.needs_research(), "empty query skips research");

        // Non-empty query but clean cache: no buffer change pending.
        s.query = "x".to_string();
        s.doc_dirty = false;
        assert!(!s.needs_research(), "clean cache skips research");

        // Dirty again but hidden: no highlights painted → no research.
        s.doc_dirty = true;
        s.visible = false;
        assert!(!s.needs_research(), "hidden overlay skips research");
    }

    #[test]
    fn dirty_then_execute_picks_up_new_occurrence() {
        // New PTY output that contains a fresh occurrence of the query must
        // be reflected after an auto re-search (execute after the dirty
        // flag), so the highlight set tracks the current buffer.
        let mut core = TerminalCore::new(80, 3, 100);
        core.process_pty_data(b"needle\r\n");

        let mut s = SearchState::default();
        s.open();
        s.query = "needle".to_string();
        s.execute(&core);
        assert_eq!(s.matches.len(), 1);

        // A second "needle" arrives in a later line.
        core.process_pty_data(b"more needle here\r\n");
        s.mark_buffer_dirty();
        assert!(s.needs_research());
        s.execute(&core);
        assert_eq!(
            s.matches.len(),
            2,
            "auto re-search reflects the new occurrence in the current buffer"
        );
        // current_index resets to the first hit; N/M display may change.
        assert_eq!(s.current_index, 0);
    }

    #[test]
    fn dirty_then_execute_shifts_abs_row_after_eviction() {
        // When scrollback overflows its capacity, the oldest rows are
        // evicted and every surviving row's absolute index shifts down. An
        // auto re-search must re-resolve the match to its new (lower)
        // abs_row so the highlight follows the text instead of drifting.
        // Capacity 5 fills, then overflows so eviction (not just spill)
        // occurs while "needle" survives.
        let mut core = TerminalCore::new(80, 2, 5);
        // Three filler lines, then "needle" (so it is not the oldest row),
        // then more lines to fill the scrollback to capacity.
        core.process_pty_data(b"f0\r\nf1\r\nf2\r\nneedle\r\ng0\r\ng1\r\n");

        let mut s = SearchState::default();
        s.open();
        s.query = "needle".to_string();
        s.execute(&core);
        assert_eq!(s.matches.len(), 1, "needle resolved before eviction");
        let row_before = s.matches[0].segments[0].abs_row;
        assert!(row_before > 0, "needle starts above scrollback index 0");

        // Push more lines: scrollback (cap 5) overflows, evicting the
        // oldest rows and shifting needle's absolute index down.
        core.process_pty_data(b"h0\r\nh1\r\n");
        s.mark_buffer_dirty();
        s.execute(&core);
        assert_eq!(s.matches.len(), 1, "needle still present after eviction");
        let row_after = s.matches[0].segments[0].abs_row;
        assert!(
            row_after < row_before,
            "eviction shifted abs_row down: {row_before} -> {row_after}"
        );
        assert_eq!(s.current_index, 0);
    }

    // ── compiled-regex cache (H6) ────────────────────────────

    #[test]
    fn regex_recompiled_only_when_query_or_options_change() {
        let core = core_with(80, 3, 100, b"hello world\r\nHELLO again");

        let mut s = SearchState::default();
        s.query = "hello".to_string();
        s.execute(&core);
        assert_eq!(s.regex_compile_count(), 1, "first execute compiles once");

        // Auto re-search over an unchanged query / options must reuse the
        // compiled regex (only the buffer moved). Mark the buffer dirty to
        // force the document rebuild path without touching the regex key.
        s.mark_buffer_dirty();
        s.execute(&core);
        assert_eq!(
            s.regex_compile_count(),
            1,
            "unchanged query/options reuse the cached regex"
        );

        // Changing the query recompiles.
        s.query = "world".to_string();
        s.execute(&core);
        assert_eq!(s.regex_compile_count(), 2, "query change recompiles");

        // Toggling case sensitivity recompiles (different flag).
        s.case_sensitive = true;
        s.execute(&core);
        assert_eq!(
            s.regex_compile_count(),
            3,
            "case-sensitivity toggle recompiles"
        );

        // Toggling is_regex recompiles (literal-escaping differs).
        s.is_regex = true;
        s.execute(&core);
        assert_eq!(s.regex_compile_count(), 4, "is_regex toggle recompiles");
    }

    #[test]
    fn clear_drops_regex_cache() {
        let core = core_with(80, 3, 100, b"hello");
        let mut s = SearchState::default();
        s.query = "hello".to_string();
        s.execute(&core);
        assert_eq!(s.regex_compile_count(), 1);

        // `clear` drops the cache; the same query recompiles afterwards.
        s.clear();
        s.query = "hello".to_string();
        s.execute(&core);
        assert_eq!(
            s.regex_compile_count(),
            2,
            "clear dropped the cache, forcing a recompile"
        );
    }

    // ── timeout error (F4) ───────────────────────────────────

    #[test]
    fn timeout_sets_error_and_clears_matches() {
        // Build a document long enough to cross a TIMEOUT_CHECK_INTERVAL
        // boundary (the guard only fires at i % 100 == 0), then force a
        // near-zero timeout so the first boundary check trips the branch.
        let mut input = Vec::new();
        for _ in 0..(TIMEOUT_CHECK_INTERVAL + 5) {
            input.extend_from_slice(b"needle\r\n");
        }
        let core = core_with(80, 3, 500, &input);

        let mut s = SearchState::default();
        s.query = "needle".to_string();
        s.test_timeout_override = Some(std::time::Duration::ZERO);
        s.execute(&core);

        assert!(s.matches.is_empty(), "timeout drops all matches");
        assert_eq!(s.current_index, -1, "timeout resets the navigation cursor");
        assert_eq!(
            s.error.as_deref(),
            Some(SEARCH_TIMEOUT_MESSAGE),
            "timeout surfaces the WebView-parity message"
        );
        assert_eq!(SEARCH_TIMEOUT_MESSAGE, "Search timed out");
    }

    // ── preserve-current navigation (H5) ─────────────────────

    #[test]
    fn execute_preserving_current_clamps_focus() {
        let core = core_with(80, 4, 100, b"aaa\r\naaa\r\naaa\r\naaa");
        let mut s = SearchState::default();
        s.query = "aaa".to_string();
        s.execute(&core);
        assert_eq!(s.matches.len(), 4);
        // Navigate to the third hit.
        s.next_match();
        s.next_match();
        assert_eq!(s.current_index, 2);

        // Re-resolve preserving current: same 4 matches, focus kept.
        s.mark_buffer_dirty();
        s.execute_preserving_current(&core);
        assert_eq!(s.matches.len(), 4);
        assert_eq!(s.current_index, 2, "preserved focus across re-resolve");
    }

    #[test]
    fn execute_preserving_current_clamps_to_smaller_match_set() {
        // Start with 4 matches and focus on the last, then re-resolve a
        // buffer with fewer matches; the clamp pins the focus to the new
        // last index instead of dangling out of range.
        let core = core_with(80, 4, 100, b"aaa\r\naaa\r\naaa\r\naaa");
        let mut s = SearchState::default();
        s.query = "aaa".to_string();
        s.execute(&core);
        s.current_index = 3;

        let fewer = core_with(80, 2, 100, b"aaa\r\naaa");
        s.mark_buffer_dirty();
        s.execute_preserving_current(&fewer);
        assert_eq!(s.matches.len(), 2);
        assert_eq!(
            s.current_index, 1,
            "focus clamped into the smaller match set"
        );
    }

    #[test]
    fn execute_resets_current_to_first() {
        // The user-driven path still snaps focus back to the first hit.
        let core = core_with(80, 4, 100, b"aaa\r\naaa\r\naaa\r\naaa");
        let mut s = SearchState::default();
        s.query = "aaa".to_string();
        s.execute(&core);
        s.current_index = 3;
        s.mark_buffer_dirty();
        s.execute(&core);
        assert_eq!(s.current_index, 0, "user edit resets focus to first");
    }
}
