use super::*;
use term_core::terminal_core::TerminalCore;

fn core_with(cols: u16, rows: u16, scrollback: u32, input: &[u8]) -> TerminalCore {
    let mut core = TerminalCore::new(cols, rows, scrollback);
    core.process_pty_data(input);
    core
}

fn search(core: &TerminalCore, query: &str, is_regex: bool, case_sensitive: bool) -> SearchState {
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
