use super::*;
use term_core::terminal_core::TerminalCore;

fn build_core(input: &[&[u8]]) -> TerminalCore {
    let mut core = TerminalCore::new(10, 3, 100);
    for chunk in input {
        core.process_pty_data(chunk);
    }
    core
}

fn build_wide_core(cols: u16, input: &[&[u8]]) -> TerminalCore {
    let mut core = TerminalCore::new(cols, 3, 100);
    for chunk in input {
        core.process_pty_data(chunk);
    }
    core
}

/// Build a collapsed [`crate::fold::FoldLayout`] wrapping `[start, end)`
/// so `resolve` can be exercised on the fold-aware path. The visible
/// window passed to `build_layout` is irrelevant to `region_at_line`,
/// which answers from the collapsed snapshot over the full row range.
fn collapsed_layout(start: u32, end: u32) -> crate::fold::FoldLayout {
    let mut fm = crate::fold::FoldManager::new();
    fm.register_osc133_region(start, end, "cmd".to_string(), Some(0));
    fm.toggle_fold(start);
    fm.build_layout(end + 8, 8, 0)
}

#[test]
fn single_row_selection_resolves() {
    let core = build_core(&[b"hello"]);
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 0, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    assert_eq!(sel.resolve(&core, None), "hello");
}

#[test]
fn multi_row_selection_joins_with_newline() {
    let core = build_core(&[b"ab\r\ncd\r\nef"]);
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 1 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    assert_eq!(sel.resolve(&core, None), "ab\ncd\nef");
}

#[test]
fn selection_handles_reverse_anchor_extent() {
    let core = build_core(&[b"hello"]);
    let sel = Selection {
        anchor: Pos { row: 0, col: 4 },
        extent: Pos { row: 0, col: 0 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 4 },
    };
    assert_eq!(sel.resolve(&core, None), "hello");
}

#[test]
fn trailing_spaces_trimmed() {
    let mut core = TerminalCore::new(10, 1, 10);
    core.process_pty_data(b"hi");
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 0, col: 9 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    assert_eq!(sel.resolve(&core, None), "hi");
}

#[test]
fn contains_handles_endpoints() {
    let sel = Selection {
        anchor: Pos { row: 0, col: 2 },
        extent: Pos { row: 1, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 2 },
    };
    assert!(sel.contains(0, 2));
    assert!(sel.contains(1, 3));
    assert!(sel.contains(0, 9)); // anything after start on row 0
    assert!(sel.contains(1, 0));
    assert!(!sel.contains(0, 1));
    assert!(!sel.contains(2, 0));
}

#[test]
fn contains_uses_absolute_rows() {
    // A selection pinned to absolute rows 100..=102 ignores screen-local
    // rows; only the matching abs rows return true.
    let sel = Selection {
        anchor: Pos { row: 100, col: 0 },
        extent: Pos { row: 102, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 100, col: 0 },
    };
    assert!(sel.contains(100, 0));
    assert!(sel.contains(101, 9));
    assert!(sel.contains(102, 4));
    assert!(!sel.contains(99, 0));
    assert!(!sel.contains(102, 5));
    assert!(!sel.contains(0, 0));
}

#[test]
fn word_boundary_finds_alnum_run() {
    let core = build_core(&[b"foo bar"]);
    // Cursor in the middle of "foo" → covers 0..=2.
    assert_eq!(word_boundary(&core, 0, 1), (0, 2));
    // Cursor on the space → collapses to that cell.
    assert_eq!(word_boundary(&core, 0, 3), (3, 3));
    // Cursor in "bar" → covers 4..=6.
    assert_eq!(word_boundary(&core, 0, 5), (4, 6));
}

#[test]
fn word_boundary_includes_underscore() {
    let core = build_core(&[b"foo_bar baz"]);
    // Underscore is a word character.
    assert_eq!(word_boundary(&core, 0, 3), (0, 6));
}

#[test]
fn word_boundary_collapses_on_empty_cell() {
    let core = build_core(&[b"hi"]);
    // Column 5 is past the written text — empty cells aren't words.
    assert_eq!(word_boundary(&core, 0, 5), (5, 5));
}

#[test]
fn extend_in_character_mode_moves_extent_only() {
    let core = build_core(&[b"hello world"]);
    let mut sel = Selection::new(Pos { row: 0, col: 2 });
    sel.extend(Pos { row: 0, col: 6 }, &core);
    assert_eq!(sel.anchor, Pos { row: 0, col: 2 });
    assert_eq!(sel.extent, Pos { row: 0, col: 6 });
}

#[test]
fn extend_in_word_mode_snaps_to_word_boundaries() {
    // 20-cell row so "foo bar baz" doesn't wrap.
    let core = build_wide_core(20, &[b"foo bar baz"]);
    // Anchor inside "foo", extent inside "baz".
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 1 }, SelectionMode::Word);
    sel.extend(Pos { row: 0, col: 9 }, &core);
    // Range should cover the outer edges: 0 ("f" in foo) → 10 ("z" in baz).
    let (start, end) = sel.ordered();
    assert_eq!(start, Pos { row: 0, col: 0 });
    assert_eq!(end, Pos { row: 0, col: 10 });
}

#[test]
fn extend_in_word_mode_handles_reverse_drag() {
    let core = build_wide_core(20, &[b"foo bar baz"]);
    // Anchor in "baz", drag back to "foo".
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 9 }, SelectionMode::Word);
    sel.extend(Pos { row: 0, col: 1 }, &core);
    let (start, end) = sel.ordered();
    assert_eq!(start, Pos { row: 0, col: 0 });
    assert_eq!(end, Pos { row: 0, col: 10 });
}

#[test]
fn extend_in_line_mode_covers_full_rows() {
    let core = build_core(&[b"row0\r\nrow1\r\nrow2"]);
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 3 }, SelectionMode::Line);
    sel.extend(Pos { row: 2, col: 1 }, &core);
    let (start, end) = sel.ordered();
    assert_eq!(start.col, 0);
    assert_eq!(end.col, core.cols() - 1);
    assert_eq!(start.row, 0);
    assert_eq!(end.row, 2);
}

/// Push enough `\r\n`-terminated lines through a small-viewport core to
/// move the first lines into scrollback, returning the core. The first
/// `n` written lines end up as scrollback rows `0..n`, and the live
/// viewport holds the tail.
fn build_scrollback_core() -> TerminalCore {
    // 3-row viewport, generous scrollback ceiling. Six lines → the first
    // three evict into scrollback (abs rows 0..3), the last three stay
    // live (abs rows 3..6, where scrollback_len == 3).
    let mut core = TerminalCore::new(10, 3, 100);
    core.process_pty_data(b"sb0\r\nsb1\r\nsb2\r\nlive0\r\nlive1\r\nlive2");
    core
}

#[test]
fn resolve_spans_scrollback_and_viewport_boundary() {
    let core = build_scrollback_core();
    assert_eq!(
        core.get_scrollback_length(),
        3,
        "first 3 lines in scrollback"
    );
    // Select from scrollback row 2 ("sb2") through the first live row
    // ("live0") at abs row 3 — a range that crosses the scrollback ↔
    // viewport boundary.
    let sel = Selection {
        anchor: Pos { row: 2, col: 0 },
        extent: Pos { row: 3, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 2, col: 0 },
    };
    assert_eq!(sel.resolve(&core, None), "sb2\nlive0");
}

#[test]
fn resolve_full_scrollback_through_live_tail() {
    let core = build_scrollback_core();
    // Whole buffer: abs rows 0..=5.
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 5, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    assert_eq!(
        sel.resolve(&core, None),
        "sb0\nsb1\nsb2\nlive0\nlive1\nlive2"
    );
}

#[test]
fn word_boundary_in_scrollback_row() {
    // "foo bar" written, then pushed into scrollback by two more lines.
    let mut core = TerminalCore::new(20, 2, 100);
    core.process_pty_data(b"foo bar\r\nx\r\ny");
    assert!(core.get_scrollback_length() >= 1);
    assert_eq!(core.get_scrollback_text(0), "foo bar");
    // Cursor in "foo" (scrollback abs row 0) → covers 0..=2.
    assert_eq!(word_boundary(&core, 0, 1), (0, 2));
    // Cursor in "bar" → covers 4..=6.
    assert_eq!(word_boundary(&core, 0, 5), (4, 6));
    // Cursor on the space → collapses.
    assert_eq!(word_boundary(&core, 0, 3), (3, 3));
}

#[test]
fn shift_rows_down_keeps_whole_selection() {
    // Both endpoints sit above the eviction boundary → both shift.
    let mut sel = Selection {
        anchor: Pos { row: 10, col: 2 },
        extent: Pos { row: 14, col: 5 },
        mode: SelectionMode::Character,
        origin: Pos { row: 10, col: 2 },
    };
    assert!(sel.shift_rows_down(3));
    assert_eq!(sel.anchor, Pos { row: 7, col: 2 });
    assert_eq!(sel.extent, Pos { row: 11, col: 5 });
}

#[test]
fn shift_rows_down_clamps_partially_evicted_endpoint() {
    // The anchor (row 2) falls below the eviction boundary (delta 5);
    // it clamps to (0, 0) while the extent (row 8) shifts to row 3.
    let mut sel = Selection {
        anchor: Pos { row: 2, col: 4 },
        extent: Pos { row: 8, col: 6 },
        mode: SelectionMode::Character,
        origin: Pos { row: 2, col: 4 },
    };
    assert!(sel.shift_rows_down(5));
    assert_eq!(sel.anchor, Pos { row: 0, col: 0 });
    assert_eq!(sel.extent, Pos { row: 3, col: 6 });
}

#[test]
fn shift_rows_down_drops_fully_evicted_selection() {
    // Both endpoints fell off the top → the selection is gone.
    let mut sel = Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 4, col: 0 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    };
    assert!(!sel.shift_rows_down(5));
}

#[test]
fn sanitize_strips_embedded_bracket_end() {
    assert_eq!(sanitize_bracket_sequences("abc\x1b[201~def"), "abcdef");
    // Multiple occurrences.
    assert_eq!(
        sanitize_bracket_sequences("\x1b[201~a\x1b[201~b\x1b[201~"),
        "ab"
    );
    // No occurrence: untouched.
    assert_eq!(sanitize_bracket_sequences("plain"), "plain");
}

#[test]
fn bracketed_paste_wraps_when_enabled() {
    let got = bracketed_paste("hello", true);
    assert_eq!(got, "\x1b[200~hello\x1b[201~");
}

#[test]
fn bracketed_paste_passthrough_when_disabled() {
    let got = bracketed_paste("hello", false);
    assert_eq!(got, "hello");
}

#[test]
fn bracketed_paste_sanitizes_body() {
    // Embedded end marker is stripped before the wrap, so the inner
    // text cannot terminate the bracketed paste prematurely.
    let got = bracketed_paste("evil\x1b[201~payload", true);
    assert_eq!(got, "\x1b[200~evilpayload\x1b[201~");
    // Even when disabled, the sanitization still applies (defense in depth).
    let got2 = bracketed_paste("evil\x1b[201~payload", false);
    assert_eq!(got2, "evilpayload");
}

/// Build a tall-viewport core so the 6 written lines all stay live
/// (scrollback_len == 0), giving stable absolute rows 0..6.
fn build_tall_core(input: &[u8]) -> TerminalCore {
    let mut core = TerminalCore::new(10, 8, 100);
    core.process_pty_data(input);
    core
}

#[test]
fn resolve_with_no_fold_layout_is_unchanged() {
    // `None` fold layout copies every row in range, including what would
    // be a collapsed body — the pre-fold behavior.
    let core = build_tall_core(b"r0\r\nr1\r\nr2\r\nr3\r\nr4");
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 4, col: 1 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    assert_eq!(sel.resolve(&core, None), "r0\nr1\nr2\nr3\nr4");
}

#[test]
fn resolve_skips_collapsed_body_keeps_summary_line() {
    // Region [1, 4): row 1 is the summary/start (the command line) and is
    // kept; rows 2 and 3 are the hidden body and are skipped. Rows 0 and 4
    // sit outside the region and are kept.
    let core = build_tall_core(b"r0\r\nr1\r\nr2\r\nr3\r\nr4");
    let layout = collapsed_layout(1, 4);
    let sel = Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 4, col: 1 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    };
    // Body rows r2/r3 dropped; summary row r1 and the outside rows kept.
    assert_eq!(sel.resolve(&core, Some(&layout)), "r0\nr1\nr4");
}

#[test]
fn resolve_collapsed_summary_only_selection() {
    // A selection entirely on the summary row copies just that row — the
    // summary line is never skipped.
    let core = build_tall_core(b"r0\r\nr1\r\nr2\r\nr3\r\nr4");
    let layout = collapsed_layout(1, 4);
    let sel = Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 1, col: 1 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    };
    assert_eq!(sel.resolve(&core, Some(&layout)), "r1");
}

// --- Pivot-anchored multi-extension (task0001) ---------------------------
//
// The pre-fix `extend` overwrote the anchor with the snapped range start,
// so on the *second* extension the origin word / line was already gone and
// could no longer be kept in range. Every test below extends at least
// twice (or drags away and back) — the exact shape that let the bug slip
// past the single-extend tests above.

/// AC-1: two consecutive word extensions onto a word on the row above keep
/// the range spanning that upper word's start through the origin word's end.
#[test]
fn word_mode_repeated_extend_above_keeps_origin_word_end() {
    // Row 0 = "top" (cols 0..=2); row 1 = "bottomword" (cols 0..=9).
    let core = build_wide_core(20, &[b"top\r\nbottomword"]);
    // Origin pressed inside the row-1 word.
    let mut sel = Selection::new_with_mode(Pos { row: 1, col: 3 }, SelectionMode::Word);
    // Two extensions onto "top" on the row above.
    sel.extend(Pos { row: 0, col: 1 }, &core);
    sel.extend(Pos { row: 0, col: 2 }, &core);
    // Upper word's start (row 0, col 0) → origin word's end (row 1, col 9).
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 0 }, Pos { row: 1, col: 9 })
    );
}

/// AC-2: dragging away and back inside the origin word collapses the range
/// to exactly the origin word.
#[test]
fn word_mode_extend_away_then_back_yields_origin_word() {
    // "foo bar baz": bar = cols 4..=6, baz = cols 8..=10.
    let core = build_wide_core(20, &[b"foo bar baz"]);
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 5 }, SelectionMode::Word);
    // Away to "baz", then back inside "bar".
    sel.extend(Pos { row: 0, col: 9 }, &core);
    sel.extend(Pos { row: 0, col: 5 }, &core);
    // Exactly the origin word "bar".
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 4 }, Pos { row: 0, col: 6 })
    );
}

/// AC-3: extending above then below the origin word yields the origin
/// word's start through the lower word's end.
#[test]
fn word_mode_extend_above_then_below_spans_origin_start_to_lower_end() {
    // Row 0 "up" (0..=1), row 1 "middle" (0..=5, origin), row 2 "downword"
    // (0..=7).
    let core = build_wide_core(20, &[b"up\r\nmiddle\r\ndownword"]);
    let mut sel = Selection::new_with_mode(Pos { row: 1, col: 2 }, SelectionMode::Word);
    sel.extend(Pos { row: 0, col: 0 }, &core); // above
    sel.extend(Pos { row: 2, col: 3 }, &core); // below, past the origin word
    // Origin word start (row 1, col 0) → lower word end (row 2, col 7).
    assert_eq!(
        sel.ordered(),
        (Pos { row: 1, col: 0 }, Pos { row: 2, col: 7 })
    );
}

/// AC-4: extending to an earlier word on the same row yields that word's
/// start through the origin word's end.
#[test]
fn word_mode_extend_to_earlier_word_keeps_origin_word_end() {
    let core = build_wide_core(20, &[b"foo bar baz"]);
    // Origin in "bar" (cols 4..=6); drag back to "foo" (cols 0..=2).
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 5 }, SelectionMode::Word);
    sel.extend(Pos { row: 0, col: 1 }, &core);
    // "foo" start (0) → origin word "bar" end (6).
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 0 }, Pos { row: 0, col: 6 })
    );
}

/// Test-note edge case: a word extension onto a whitespace cell collapses
/// that endpoint to the cell while the origin word's edge is kept.
#[test]
fn word_mode_extend_onto_whitespace_keeps_origin_edge() {
    // "foo bar": foo = cols 0..=2, space at col 3.
    let core = build_wide_core(20, &[b"foo bar"]);
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 1 }, SelectionMode::Word);
    sel.extend(Pos { row: 0, col: 3 }, &core); // onto the space
    // Origin word "foo" start (0) kept; far endpoint collapses to the space.
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 0 }, Pos { row: 0, col: 3 })
    );
}

/// Test-note edge case: origin sitting in scrollback with the pointer in
/// the live viewport still pivots on the scrollback word.
#[test]
fn word_mode_origin_in_scrollback_pointer_in_viewport() {
    // sb rows 0..=2 = "sb0".."sb2"; live rows 3..=5 = "live0".."live2".
    let core = build_scrollback_core();
    assert_eq!(core.get_scrollback_length(), 3);
    // Origin inside "sb0" (scrollback abs row 0, cols 0..=2).
    let mut sel = Selection::new_with_mode(Pos { row: 0, col: 1 }, SelectionMode::Word);
    // Extend into "live0" in the viewport (abs row 3, cols 0..=4).
    sel.extend(Pos { row: 3, col: 2 }, &core);
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 0 }, Pos { row: 3, col: 4 })
    );
}

/// AC-5: repeated line extensions up / down / back always cover full rows
/// including the origin row; returning to the origin row yields it alone.
#[test]
fn line_mode_repeated_extend_always_covers_origin_row() {
    let core = build_core(&[b"row0\r\nrow1\r\nrow2"]);
    let last = core.cols() - 1;
    // Origin pressed on row 1.
    let mut sel = Selection::new_with_mode(Pos { row: 1, col: 2 }, SelectionMode::Line);
    // Up to row 0: full rows 0..=1 (origin row 1 included).
    sel.extend(Pos { row: 0, col: 3 }, &core);
    assert_eq!(
        sel.ordered(),
        (Pos { row: 0, col: 0 }, Pos { row: 1, col: last })
    );
    // Down to row 2: full rows 1..=2 (origin row 1 included).
    sel.extend(Pos { row: 2, col: 1 }, &core);
    assert_eq!(
        sel.ordered(),
        (Pos { row: 1, col: 0 }, Pos { row: 2, col: last })
    );
    // Back onto the origin row: the origin row alone.
    sel.extend(Pos { row: 1, col: 4 }, &core);
    assert_eq!(
        sel.ordered(),
        (Pos { row: 1, col: 0 }, Pos { row: 1, col: last })
    );
}

/// NFR1 regression: character mode never mutates the anchor across
/// repeated extensions — only the free endpoint moves.
#[test]
fn character_mode_repeated_extend_keeps_anchor() {
    let core = build_core(&[b"hello world"]);
    let mut sel = Selection::new(Pos { row: 0, col: 2 });
    sel.extend(Pos { row: 0, col: 6 }, &core);
    sel.extend(Pos { row: 0, col: 8 }, &core);
    assert_eq!(sel.anchor, Pos { row: 0, col: 2 });
    assert_eq!(sel.extent, Pos { row: 0, col: 8 });
    assert_eq!(sel.origin, Pos { row: 0, col: 2 });
}

/// AC-6: scrollback eviction shifts the origin pivot along with both
/// endpoints (same row delta / clamp), and a fully evicted selection is
/// still dropped.
#[test]
fn shift_rows_down_shifts_origin_with_endpoints() {
    let mut sel = Selection {
        anchor: Pos { row: 5, col: 0 },
        extent: Pos { row: 9, col: 9 },
        mode: SelectionMode::Word,
        origin: Pos { row: 7, col: 3 },
    };
    assert!(sel.shift_rows_down(4));
    assert_eq!(sel.anchor, Pos { row: 1, col: 0 });
    assert_eq!(sel.extent, Pos { row: 5, col: 9 });
    assert_eq!(sel.origin, Pos { row: 3, col: 3 });
}

/// AC-6 (clamp): when the top endpoint and the origin both fall below the
/// eviction boundary they clamp to (0, 0) together.
#[test]
fn shift_rows_down_clamps_origin_with_partial_eviction() {
    let mut sel = Selection {
        anchor: Pos { row: 2, col: 1 },
        extent: Pos { row: 10, col: 4 },
        mode: SelectionMode::Word,
        origin: Pos { row: 3, col: 2 },
    };
    assert!(sel.shift_rows_down(5));
    assert_eq!(sel.anchor, Pos { row: 0, col: 0 });
    assert_eq!(sel.origin, Pos { row: 0, col: 0 });
    assert_eq!(sel.extent, Pos { row: 5, col: 4 });
}

/// AC-6 (drop): a fully evicted word selection (origin included) is
/// dropped exactly as a character selection is.
#[test]
fn shift_rows_down_drops_fully_evicted_word_selection() {
    let mut sel = Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 3, col: 0 },
        mode: SelectionMode::Word,
        origin: Pos { row: 2, col: 0 },
    };
    assert!(!sel.shift_rows_down(5));
}
