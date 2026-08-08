use super::*;

// ── backfill_prompt_marks ─────────────────────────────────

/// A tab whose `settings.fold_enabled` is `false`, used to assert the
/// fold gate seeds the per-tab `FoldManager` as disabled.
fn test_tab_fold_disabled() -> Tab {
    let settings = Settings {
        fold_enabled: false,
        ..Settings::default()
    };
    Tab::spawn_shell(
        "test",
        80,
        24,
        100,
        Arc::new(settings),
        None,
        None,
        Arc::new(NoopSink),
        None,
    )
}

#[test]
fn spawn_seeds_cursor_style_from_settings() {
    // AC-2: a tab spawned with `cursor_style: bar` reports
    // `get_cursor_style()` = 2 on its core (spawn-path seeding).
    let settings = Settings {
        cursor_style: crate::settings::CursorStyle::Bar,
        ..Settings::default()
    };
    let tab = Tab::spawn_shell(
        "test",
        80,
        24,
        100,
        Arc::new(settings),
        None,
        None,
        Arc::new(NoopSink),
        None,
    );
    assert_eq!(tab.core.lock().get_cursor_style(), 2);
}

#[test]
fn spawn_seeds_cursor_blink_from_settings() {
    // AC-4: existing spawn-path blink seeding behavior is preserved
    // (`cursor_blink: false` in settings -> core reports blink false
    // at spawn).
    let settings = Settings {
        cursor_blink: false,
        ..Settings::default()
    };
    let tab = Tab::spawn_shell(
        "test",
        80,
        24,
        100,
        Arc::new(settings),
        None,
        None,
        Arc::new(NoopSink),
        None,
    );
    assert!(!tab.core.lock().get_cursor_blink());
}

/// Build a prompt-start `PendingPromptMark` as `term_core` would emit
/// it: `abs_row` is the emit-time absolute row, `evicted_total` the
/// eviction counter at emit time.
fn pending_mark(abs_row: u32, evicted_total: u64) -> PendingPromptMark {
    PendingPromptMark {
        kind: b'A',
        abs_row,
        exit_code: None,
        evicted_total,
    }
}

#[test]
fn backfill_stamps_drained_marks_with_emit_row() {
    let mut tab = test_tab();
    // A single mark captured at absolute row 105, no eviction.
    tab.backfill_prompt_marks(0, vec![pending_mark(105, 0)]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(105));
}

#[test]
fn backfill_separates_multiple_marks_by_emit_row() {
    // The core regression this fix targets: several marks in one drain
    // must keep the distinct rows they were emitted on, not collapse.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_mark(10, 0),
            pending_mark(20, 0),
            pending_mark(30, 0),
        ],
    );
    assert_eq!(tab.prompts.find_prev_prompt(25), Some(20));
    assert_eq!(tab.prompts.find_next_prompt(15), Some(20));
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(30));
}

#[test]
fn backfill_prunes_stored_marks_before_pushing_new_ones() {
    // Marks stored in an earlier call are pruned by the eviction delta,
    // while a new mark captured in the *same* later frame (no eviction
    // after its own emit) lands at its emit-time row unchanged.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(0, vec![pending_mark(105, 0)]); // stored at 105
    // 50 rows evicted since baseline; the new mark fired *after* those
    // evictions, so its own evicted_total is already 50 → no extra shift.
    tab.backfill_prompt_marks(50, vec![pending_mark(110, 50)]);
    // Old mark shifted 105 → 55; new mark stays at 110.
    assert_eq!(tab.prompts.find_prev_prompt(60), Some(55));
    assert_eq!(tab.prompts.find_next_prompt(60), Some(110));
}

#[test]
fn backfill_normalizes_mark_evicted_after_its_emit() {
    // A mark fired early in a pump (evicted_total = 0, abs_row = 90),
    // then more output in the SAME pump evicted 30 rows, so the frame
    // observed at drain time is evicted_total = 30. The mark must shift
    // down by (30 - 0) = 30 → row 60, matching the post-pump frame.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(30, vec![pending_mark(90, 0)]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(60));
}

#[test]
fn backfill_mixed_evicted_totals_in_one_drain() {
    // Two marks from one pump: the first fired before any eviction, the
    // second after 20 rows were evicted. At drain the frame is at 20.
    // First: abs_row 50, evicted 0  → shift 20 → row 30.
    // Second: abs_row 45, evicted 20 → shift 0  → row 45.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(20, vec![pending_mark(50, 0), pending_mark(45, 20)]);
    assert_eq!(tab.prompts.find_prev_prompt(40), Some(30));
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(45));
}

#[test]
fn backfill_clears_marks_when_counter_goes_backwards() {
    // A reset (RIS) zeroes the core's eviction counter; stale marks
    // belong to the discarded line frame and must be dropped.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(70, vec![pending_mark(105, 70)]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(105));
    tab.backfill_prompt_marks(0, vec![]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), None);
}

#[test]
fn backfill_drops_mark_evicted_out_of_frame_in_same_pump() {
    // A mark fired at abs_row 10 (evicted_total 0), then the SAME pump
    // evicted 25 rows — more than the mark's depth. The mark's line no
    // longer exists in the frame, so it must be DROPPED, not clamped to
    // row 0 (which would plant a phantom prompt at the top of
    // scrollback that jump_to_prompt would navigate to).
    let mut tab = test_tab();
    tab.backfill_prompt_marks(25, vec![pending_mark(10, 0)]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), None);
    // Boundary: shift exactly equals abs_row → row 0 is still a real,
    // retained line (the new frame's first row), so it is kept.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(10, vec![pending_mark(10, 0)]);
    assert_eq!(tab.prompts.find_prev_prompt(u32::MAX), Some(0));
}

// ── OSC 133 fold-region registration ──────────────────────

#[test]
fn fold_region_registered_on_c_b_d_sequence() {
    // A → B → C → D in one batch: the D pairs with C as the region
    // bounds, and B supplies the command text. With abs_row >= the
    // viewport-top (scrollback empty, 24-row viewport), the B row is read
    // from the viewport — seed that row with the command text first.
    let mut tab = test_tab();
    // Put "ls -la" on viewport row 3 (= abs_row 3, scrollback empty).
    {
        let mut c = tab.core.lock();
        c.process_pty_data(b"\r\n\r\n\r\nls -la");
    }
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'B', 3, None),
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
        ],
    );
    // Region spans C..D = 4..9 with B's line text and the D exit code.
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.start_line, 4);
    assert_eq!(r.end_line, 9);
    assert_eq!(r.command_text.as_deref(), Some("ls -la"));
    assert_eq!(r.exit_code, Some(0));
    assert_eq!(r.id, "osc133:4");
}

#[test]
fn fold_region_not_registered_without_c() {
    // B → D with no C in between: no region (the WebView bails when no C
    // is found).
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'B', 3, None),
            pending_kind(b'D', 9, Some(1)),
        ],
    );
    assert!(tab.folds.get_region_at_line(3).is_none());
    assert!(tab.folds.get_region_at_line(9).is_none());
    assert!(!tab.folds.has_collapsed_regions());
}

#[test]
fn fold_region_consecutive_d_stops_search() {
    // C → D → D: the second D's reverse scan hits the first D before any
    // C, so it registers no second region. Only the first C↔D pairs.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
            pending_kind(b'D', 12, Some(2)),
        ],
    );
    // First region: 4..9.
    let r = tab.folds.get_region_at_line(4).expect("first region");
    assert_eq!(r.start_line, 4);
    assert_eq!(r.end_line, 9);
    assert_eq!(r.exit_code, Some(0));
    // Second D (row 12) finds no C before hitting the first D → no region
    // starting anywhere at/after row 9.
    assert!(tab.folds.get_region_at_line(10).is_none());
    assert!(tab.folds.get_region_at_line(12).is_none());
}

#[test]
fn fold_region_batch_d_indices_resolved_correctly() {
    // Three C→D pairs in one batch: each D must pair with its own preceding
    // C, not any other. This exercises the single-scan index resolution in
    // backfill_prompt_marks — previously each D ran its own rposition search.
    // Pattern: B0 C0 D0  B1 C1 D1  B2 C2 D2
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'B', 1, None),
            pending_kind(b'C', 2, None),
            pending_kind(b'D', 5, Some(0)),
            pending_kind(b'B', 6, None),
            pending_kind(b'C', 7, None),
            pending_kind(b'D', 10, Some(1)),
            pending_kind(b'B', 11, None),
            pending_kind(b'C', 12, None),
            pending_kind(b'D', 15, Some(2)),
        ],
    );
    // First region: C0(2) → D0(5).
    let r0 = tab
        .folds
        .get_region_at_line(2)
        .expect("region 0 registered");
    assert_eq!(r0.start_line, 2);
    assert_eq!(r0.end_line, 5);
    assert_eq!(r0.exit_code, Some(0));
    // Second region: C1(7) → D1(10).
    let r1 = tab
        .folds
        .get_region_at_line(7)
        .expect("region 1 registered");
    assert_eq!(r1.start_line, 7);
    assert_eq!(r1.end_line, 10);
    assert_eq!(r1.exit_code, Some(1));
    // Third region: C2(12) → D2(15).
    let r2 = tab
        .folds
        .get_region_at_line(12)
        .expect("region 2 registered");
    assert_eq!(r2.start_line, 12);
    assert_eq!(r2.end_line, 15);
    assert_eq!(r2.exit_code, Some(2));
}

#[test]
fn fold_region_without_b_has_empty_command_text() {
    // C → D with no preceding B: the region still registers, but its
    // command text is empty (the WebView leaves commandText == "").
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
    );
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.start_line, 4);
    assert_eq!(r.end_line, 9);
    assert_eq!(r.command_text.as_deref(), Some(""));
}

#[test]
fn fold_region_command_text_from_scrollback() {
    // When the B row lies in scrollback (abs_row < scrollback_len) the
    // command text is read via get_scrollback_text. Push enough lines to
    // move the B row into scrollback, then craft marks against the
    // resulting frame. A 2-row viewport keeps the math small.
    let mut tab = Tab::spawn_shell(
        "test",
        80,
        2,
        100,
        Arc::new(Settings::default()),
        None,
        None,
        Arc::new(NoopSink),
        None,
    );
    // "make build" on the first line, then push it into scrollback with
    // two more lines (2-row viewport → row 0 evicts to scrollback idx 0).
    {
        let mut c = tab.core.lock();
        c.process_pty_data(b"make build\r\nx\r\ny");
        assert!(c.get_scrollback_length() >= 1, "first line in scrollback");
        assert_eq!(c.get_scrollback_text(0), "make build");
    }
    // B at scrollback row 0; C/D in the live viewport (abs rows 1,2).
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'B', 0, None),
            pending_kind(b'C', 1, None),
            pending_kind(b'D', 2, Some(0)),
        ],
    );
    let r = tab.folds.get_region_at_line(1).expect("region registered");
    assert_eq!(r.command_text.as_deref(), Some("make build"));
    assert_eq!(r.start_line, 1);
    assert_eq!(r.end_line, 2);
}

#[test]
fn fold_region_rows_synced_with_eviction_prune() {
    // A region registered in one batch must shift down by the same
    // eviction delta that prunes the prompt marks, keeping fold rows in
    // the prompt frame. First batch registers 4..9; a later batch reports
    // 3 rows evicted, so the region re-bases to 1..6.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
    );
    assert_eq!(
        tab.folds.get_region_at_line(4).map(|r| r.start_line),
        Some(4)
    );
    // 3 rows evicted since baseline; the new (empty) batch triggers the
    // prune of both prompts and folds.
    tab.backfill_prompt_marks(3, vec![]);
    // The region re-based from 4..9 down to 1..6. Row 7 (formerly inside
    // 4..9) is now outside; row 1 is the new start.
    assert!(
        tab.folds.get_region_at_line(7).is_none(),
        "region no longer extends to the pre-prune rows"
    );
    let r = tab.folds.get_region_at_line(1).expect("region re-based");
    assert_eq!(r.start_line, 1);
    assert_eq!(r.end_line, 6);
    assert_eq!(r.id, "osc133:1");
}

#[test]
fn fold_region_dropped_when_head_evicted() {
    // A region whose C row falls off the top of scrollback is dropped by
    // prune_before_line (it spans the boundary), matching the prompt
    // prune's retain(row >= count).
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
    );
    // Evict 6 rows: the region 4..9 spans boundary 6 → dropped entirely.
    tab.backfill_prompt_marks(6, vec![]);
    assert!(tab.folds.get_region_at_line(0).is_none());
    assert!(tab.folds.get_region_at_line(3).is_none());
    assert!(!tab.folds.has_collapsed_regions());
}

#[test]
fn fold_regions_cleared_on_core_reset() {
    // A counter that moved backwards signals a core reset (RIS); fold
    // regions belong to the discarded frame and must be cleared along
    // with the prompt marks.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        8,
        vec![
            PendingPromptMark {
                kind: b'C',
                abs_row: 4,
                exit_code: None,
                evicted_total: 8,
            },
            PendingPromptMark {
                kind: b'D',
                abs_row: 9,
                exit_code: Some(0),
                evicted_total: 8,
            },
        ],
    );
    assert!(tab.folds.get_region_at_line(4).is_some());
    // Counter resets to 0 → clear.
    tab.backfill_prompt_marks(0, vec![]);
    assert!(tab.folds.get_region_at_line(4).is_none());
    assert!(!tab.folds.has_collapsed_regions());
}

#[test]
fn fold_region_end_to_end_osc133_bytes() {
    // Drive the whole pipeline with real OSC 133 byte sequences fed
    // through term_core (the way `pump` does): emit B (with the command
    // echoed on its row), C, run output, then D — and confirm a region
    // is registered with the command text and exit code. This exercises
    // term_core's mark capture + the native backfill registration path
    // together.
    let mut tab = test_tab();
    let (evicted_total, marks) = {
        let mut c = tab.core.lock();
        // Prompt start, command start, the command text, command exec,
        // two output lines, then command end with exit code 0.
        c.process_pty_data(
            b"\x1b]133;A\x07\x1b]133;B\x07echo hi\x1b]133;C\x07\r\nhi\r\n\x1b]133;D;0\x07",
        );
        c.flush_grapheme_buffer();
        let marks = c.take_prompt_marks();
        (c.get_scrollback_evicted_total(), marks)
    };
    // We should have captured A,B,C,D.
    assert_eq!(marks.len(), 4, "captured all four OSC 133 marks");
    tab.backfill_prompt_marks(evicted_total, marks);

    // Exactly one region was registered (the C↔D pair).
    let collapsed_before = tab.folds.has_collapsed_regions();
    assert!(!collapsed_before, "regions start expanded");
    // The B mark and C mark share the prompt row (no newline between B,
    // the echoed command, and C), so the region command text is the
    // prompt line "echo hi". Find the region by scanning for it.
    let region = (0..30)
        .filter_map(|row| tab.folds.get_region_at_line(row))
        .next()
        .cloned();
    let region = region.expect("a C→D region was registered");
    assert_eq!(region.source, crate::fold::FoldSource::Osc133);
    assert_eq!(region.exit_code, Some(0));
    assert!(
        region
            .command_text
            .as_deref()
            .unwrap_or("")
            .contains("echo hi"),
        "command text carries the B-row command: {:?}",
        region.command_text
    );
}

// ── OSC 777 custom fold-region registration ───────────────

use term_core::terminal_core::{FoldMarkKind, PendingFoldMark};

/// Build a custom-fold `PendingFoldMark` at `abs_row` with no eviction
/// (the common test frame: `evicted_total == 0`).
fn fold_mark(kind: FoldMarkKind, abs_row: u32, label: &str) -> PendingFoldMark {
    PendingFoldMark {
        kind,
        abs_row,
        evicted_total: 0,
        label: label.to_string(),
    }
}

#[test]
fn custom_fold_begin_end_registers_region() {
    // begin@4 → end@9 registers a custom region 4..9 with the label.
    let mut tab = test_tab();
    tab.backfill_fold_marks(
        0,
        vec![
            fold_mark(FoldMarkKind::Begin, 4, "Build Output"),
            fold_mark(FoldMarkKind::End, 9, ""),
        ],
    );
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.start_line, 4);
    assert_eq!(r.end_line, 9);
    assert_eq!(r.source, crate::fold::FoldSource::Custom);
    assert_eq!(r.label.as_deref(), Some("Build Output"));
    assert_eq!(r.id, "custom:4");
    // No pending begin remains after pairing.
    assert!(tab.pending_fold_begin.is_none());
}

#[test]
fn custom_fold_begin_end_across_drains() {
    // A begin in one drain pairs with an end in a later drain.
    let mut tab = test_tab();
    tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 4, "lbl")]);
    assert_eq!(tab.pending_fold_begin.as_ref().map(|p| p.0), Some(4));
    assert!(tab.folds.get_region_at_line(4).is_none(), "no region yet");
    tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::End, 9, "")]);
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.end_line, 9);
    assert_eq!(r.label.as_deref(), Some("lbl"));
}

#[test]
fn custom_fold_orphaned_end_ignored() {
    // An `end` with no pending begin registers nothing.
    let mut tab = test_tab();
    tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::End, 9, "")]);
    assert!(!tab.folds.has_collapsed_regions());
    assert!(tab.folds.get_region_at_line(9).is_none());
    assert!(tab.pending_fold_begin.is_none());
}

#[test]
fn custom_fold_consecutive_begins_last_wins() {
    // Two begins, then one end: the second begin overwrites the first
    // (WebView `pendingFoldBegins.set` clobber), so the region spans the
    // SECOND begin → end.
    let mut tab = test_tab();
    tab.backfill_fold_marks(
        0,
        vec![
            fold_mark(FoldMarkKind::Begin, 4, "first"),
            fold_mark(FoldMarkKind::Begin, 6, "second"),
            fold_mark(FoldMarkKind::End, 12, ""),
        ],
    );
    // No region starts at the first begin row 4.
    assert!(tab.folds.get_region_at_line(4).is_none());
    let r = tab
        .folds
        .get_region_at_line(6)
        .expect("region from 2nd begin");
    assert_eq!(r.start_line, 6);
    assert_eq!(r.end_line, 12);
    assert_eq!(r.label.as_deref(), Some("second"));
}

#[test]
fn custom_fold_empty_label_falls_back() {
    // An empty begin label registers as the FoldManager "..." fallback.
    let mut tab = test_tab();
    tab.backfill_fold_marks(
        0,
        vec![
            fold_mark(FoldMarkKind::Begin, 4, ""),
            fold_mark(FoldMarkKind::End, 9, ""),
        ],
    );
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.label.as_deref(), Some("..."));
}

#[test]
fn custom_fold_pair_across_eviction() {
    // begin captured before any eviction (abs_row 50, evicted 0), then
    // 20 rows evicted within the same drain so the frame is at 20. The
    // begin normalizes to row 30; the end (abs_row 45, evicted 20) stays
    // at 45. Region 30..45.
    let mut tab = test_tab();
    tab.backfill_fold_marks(
        20,
        vec![
            PendingFoldMark {
                kind: FoldMarkKind::Begin,
                abs_row: 50,
                evicted_total: 0,
                label: "x".to_string(),
            },
            PendingFoldMark {
                kind: FoldMarkKind::End,
                abs_row: 45,
                evicted_total: 20,
                label: String::new(),
            },
        ],
    );
    let r = tab.folds.get_region_at_line(30).expect("region registered");
    assert_eq!(r.start_line, 30);
    assert_eq!(r.end_line, 45);
    assert_eq!(r.label.as_deref(), Some("x"));
}

#[test]
fn custom_fold_pending_begin_pruned_by_eviction_drops_region() {
    // A pending begin at row 4 (from drain 1); drain 2 reports 6 rows
    // evicted (delta 6 > 4) via backfill_prompt_marks, so the begin's head
    // scrolled off the top. The begin is dropped, and a later end finds no
    // pending begin → no region. Mirrors the WebView "boundary-spanning
    // region is dropped" rule.
    let mut tab = test_tab();
    tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 4, "lbl")]);
    assert!(tab.pending_fold_begin.is_some());
    // Eviction prune runs inside backfill_prompt_marks (the callers always
    // invoke it before backfill_fold_marks). 6 rows evicted: begin row 4
    // < 6 → dropped.
    tab.backfill_prompt_marks(6, vec![]);
    assert!(
        tab.pending_fold_begin.is_none(),
        "pending begin past the eviction boundary is dropped"
    );
    tab.backfill_fold_marks(6, vec![fold_mark(FoldMarkKind::End, 9, "")]);
    assert!(!tab.folds.has_collapsed_regions());
}

#[test]
fn custom_fold_pending_begin_shifted_by_eviction() {
    // A pending begin at row 8 survives a 3-row eviction (8 >= 3), shifting
    // to row 5; a subsequent end at row 9 (in the post-prune frame) pairs
    // to register 5..9.
    let mut tab = test_tab();
    tab.backfill_fold_marks(0, vec![fold_mark(FoldMarkKind::Begin, 8, "lbl")]);
    tab.backfill_prompt_marks(3, vec![]);
    assert_eq!(tab.pending_fold_begin.as_ref().map(|p| p.0), Some(5));
    // The end mark was captured in the post-prune frame (evicted_total 3),
    // so it already addresses row 9 with no further shift.
    tab.backfill_fold_marks(
        3,
        vec![PendingFoldMark {
            kind: FoldMarkKind::End,
            abs_row: 9,
            evicted_total: 3,
            label: String::new(),
        }],
    );
    let r = tab.folds.get_region_at_line(5).expect("region registered");
    assert_eq!(r.start_line, 5);
    assert_eq!(r.end_line, 9);
}

#[test]
fn custom_fold_pending_begin_cleared_on_core_reset() {
    // A pending begin belongs to the pre-reset frame; a counter that moved
    // backwards (RIS) clears it along with the fold regions.
    let mut tab = test_tab();
    // Seed a registered region + a pending begin in a non-zero frame.
    tab.backfill_fold_marks(
        8,
        vec![
            PendingFoldMark {
                kind: FoldMarkKind::Begin,
                abs_row: 4,
                evicted_total: 8,
                label: "done".to_string(),
            },
            PendingFoldMark {
                kind: FoldMarkKind::End,
                abs_row: 9,
                evicted_total: 8,
                label: String::new(),
            },
        ],
    );
    // Set evicted_baseline to 8 so the next backwards counter triggers
    // the reset branch.
    tab.evicted_baseline = 8;
    tab.backfill_fold_marks(8, vec![fold_mark(FoldMarkKind::Begin, 11, "pending")]);
    assert!(tab.pending_fold_begin.is_some());
    assert!(tab.folds.get_region_at_line(4).is_some());
    // Counter resets to 0 → fold regions + pending begin cleared.
    tab.backfill_prompt_marks(0, vec![]);
    assert!(tab.folds.get_region_at_line(4).is_none());
    assert!(tab.pending_fold_begin.is_none());
}

#[test]
fn custom_fold_end_to_end_osc777_bytes() {
    // Drive the whole pipeline with real OSC 777 byte sequences fed
    // through term_core (the way `pump` does): begin with a label, some
    // output, then end — and confirm a custom region is registered.
    let mut tab = test_tab();
    let (evicted_total, fold_marks) = {
        let mut c = tab.core.lock();
        c.process_pty_data(
            b"\x1b]777;emterm;fold;begin;Compile\x07line1\r\nline2\r\n\x1b]777;emterm;fold;end\x07",
        );
        c.flush_grapheme_buffer();
        let fm = c.take_fold_marks();
        (c.get_scrollback_evicted_total(), fm)
    };
    assert_eq!(fold_marks.len(), 2, "captured begin + end");
    tab.backfill_fold_marks(evicted_total, fold_marks);
    let region = (0..30)
        .filter_map(|row| tab.folds.get_region_at_line(row))
        .next()
        .cloned()
        .expect("a custom region was registered");
    assert_eq!(region.source, crate::fold::FoldSource::Custom);
    assert_eq!(region.label.as_deref(), Some("Compile"));
    assert_eq!(region.start_line, 0, "begin captured on row 0");
    assert_eq!(region.end_line, 2, "end captured on row 2");
}

#[test]
fn custom_fold_suppressed_on_alt_screen_end_to_end() {
    // OSC 777 fold marks emitted on the alt screen are not captured by
    // term_core, so no region is registered (WebView isAlternateBuffer
    // guard parity).
    let mut tab = test_tab();
    let fold_marks = {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(
            b"\x1b[?1049h\x1b]777;emterm;fold;begin;x\x07\r\n\x1b]777;emterm;fold;end\x07\x1b[?1049l",
        );
        c.take_fold_marks()
    };
    assert!(fold_marks.is_empty(), "alt-screen fold marks not captured");
    tab.backfill_fold_marks(0, fold_marks);
    assert!(!tab.folds.has_collapsed_regions());
    assert!(tab.pending_fold_begin.is_none());
}

// ── fold_enabled settings gate ────────────────────────────

#[test]
fn fold_enabled_default_tab_is_enabled() {
    // Default settings (`fold_enabled = true`) seed an enabled manager.
    let tab = test_tab();
    assert!(tab.fold_enabled);
    assert!(tab.folds.is_enabled());
}

#[test]
fn fold_disabled_tab_seeds_disabled_manager() {
    // `settings.fold_enabled = false` seeds a manager whose `enabled`
    // flag is off, so fold clicks are gated.
    let tab = test_tab_fold_disabled();
    assert!(!tab.fold_enabled);
    assert!(!tab.folds.is_enabled());
}

#[test]
fn fold_disabled_tab_still_registers_osc133_regions() {
    // Region registration is independent of the enable gate: a disabled
    // tab still backfills C→D regions so re-enabling could fold them.
    let mut tab = test_tab_fold_disabled();
    {
        let mut c = tab.core.lock();
        c.process_pty_data(b"\r\n\r\n\r\nls -la");
    }
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'B', 3, None),
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
        ],
    );
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.start_line, 4);
    assert_eq!(r.end_line, 9);
    assert_eq!(r.command_text.as_deref(), Some("ls -la"));
}

#[test]
fn fold_disabled_tab_still_registers_custom_regions() {
    // Custom (OSC 777) region registration also runs while disabled.
    let mut tab = test_tab_fold_disabled();
    tab.backfill_fold_marks(
        0,
        vec![
            fold_mark(FoldMarkKind::Begin, 4, "Build Output"),
            fold_mark(FoldMarkKind::End, 9, ""),
        ],
    );
    let r = tab.folds.get_region_at_line(4).expect("region registered");
    assert_eq!(r.source, crate::fold::FoldSource::Custom);
    assert_eq!(r.label.as_deref(), Some("Build Output"));
}

#[test]
fn fold_disabled_tab_cannot_collapse() {
    // With folding disabled, toggling a registered region is a no-op:
    // no region ever becomes collapsed.
    let mut tab = test_tab_fold_disabled();
    tab.backfill_prompt_marks(
        0,
        vec![pending_kind(b'C', 4, None), pending_kind(b'D', 9, Some(0))],
    );
    assert!(tab.folds.get_region_at_line(4).is_some());
    assert!(!tab.folds.toggle_fold(4), "toggle gated while disabled");
    assert!(!tab.folds.get_region_at_line(4).unwrap().collapsed);
    assert!(!tab.folds.has_collapsed_regions());
}

#[test]
fn fold_disabled_preserved_across_core_reset() {
    // A core reset (backwards eviction counter) rebuilds the FoldManager
    // from scratch; the disabled state must survive the rebuild rather
    // than snapping back to the FoldManager default (`enabled = true`).
    let mut tab = test_tab_fold_disabled();
    tab.backfill_prompt_marks(
        8,
        vec![
            PendingPromptMark {
                kind: b'C',
                abs_row: 4,
                exit_code: None,
                evicted_total: 8,
            },
            PendingPromptMark {
                kind: b'D',
                abs_row: 9,
                exit_code: Some(0),
                evicted_total: 8,
            },
        ],
    );
    assert!(!tab.folds.is_enabled());
    // Counter resets to 0 → FoldManager rebuilt.
    tab.backfill_prompt_marks(0, vec![]);
    assert!(
        !tab.folds.is_enabled(),
        "disabled state survives the reset rebuild"
    );
}

#[test]
fn fold_enabled_preserved_across_core_reset() {
    // The enabled default likewise survives a reset rebuild.
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        8,
        vec![PendingPromptMark {
            kind: b'D',
            abs_row: 9,
            exit_code: Some(0),
            evicted_total: 8,
        }],
    );
    tab.backfill_prompt_marks(0, vec![]);
    assert!(tab.folds.is_enabled());
}

#[test]
fn fold_disabled_preserved_across_mux_snapshot() {
    // A mux snapshot replay also rebuilds the FoldManager; the disabled
    // state must carry over there too.
    let mut tab = test_tab_fold_disabled();
    assert!(!tab.folds.is_enabled());
    let msg = MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id: 0,
        payload: b"hello".to_vec(),
    };
    tab.apply_mux_message(msg);
    assert!(
        !tab.folds.is_enabled(),
        "disabled state survives the snapshot rebuild"
    );
}
