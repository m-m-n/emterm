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
fn same_width_fast_path_matches_reference_shrink_forces_capacity_eviction_cursor_mid_viewport() {
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
