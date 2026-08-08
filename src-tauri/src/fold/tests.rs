use super::*;

// ── Registration ─────────────────────────────────────────

#[test]
fn register_osc133_sets_all_properties() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "ls -la".to_string(), Some(0));

    let r = fm.get_region_at_line(5).expect("region present");
    assert_eq!(r.source, FoldSource::Osc133);
    assert_eq!(r.start_line, 5);
    assert_eq!(r.end_line, 15);
    assert_eq!(r.command_text.as_deref(), Some("ls -la"));
    assert_eq!(r.exit_code, Some(0));
    assert_eq!(r.line_count, 10);
    assert!(!r.collapsed);
    assert_eq!(r.label, None);
    assert_eq!(r.id, "osc133:5");
}

#[test]
fn register_custom_sets_label_no_exit_code() {
    let mut fm = FoldManager::new();
    fm.register_custom_region(10, 30, "Build Output".to_string());

    let r = fm.get_region_at_line(10).expect("region present");
    assert_eq!(r.source, FoldSource::Custom);
    assert_eq!(r.label.as_deref(), Some("Build Output"));
    assert_eq!(r.exit_code, None);
    assert_eq!(r.command_text, None);
    assert_eq!(r.line_count, 20);
    assert_eq!(r.id, "custom:10");
}

#[test]
fn region_with_zero_lines_not_registered() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 5, "echo hi".to_string(), Some(0));
    assert!(fm.get_region_at_line(5).is_none());
}

#[test]
fn region_with_one_line_registered() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 6, "echo hi".to_string(), Some(0));
    let r = fm.get_region_at_line(5).expect("region present");
    assert_eq!(r.line_count, 1);
}

#[test]
fn osc133_without_exit_code() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "running...".to_string(), None);
    let r = fm.get_region_at_line(5).expect("region present");
    assert_eq!(r.exit_code, None);
}

#[test]
fn custom_empty_label_falls_back() {
    let mut fm = FoldManager::new();
    fm.register_custom_region(10, 20, String::new());
    let r = fm.get_region_at_line(10).expect("region present");
    assert_eq!(r.label.as_deref(), Some("..."));
}

#[test]
fn overlapping_region_does_not_overwrite() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    // 8..20 overlaps 5..15 → rejected.
    fm.register_osc133_region(8, 20, "second".to_string(), Some(1));
    let r = fm.get_region_at_line(5).expect("first still present");
    assert_eq!(r.command_text.as_deref(), Some("first"));
}

#[test]
fn touching_regions_do_not_overlap() {
    // Half-open ranges: 5..10 and 10..15 share no row, so both register.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
    fm.register_osc133_region(10, 15, "second".to_string(), Some(0));
    assert_eq!(
        fm.get_region_at_line(5).unwrap().command_text.as_deref(),
        Some("first")
    );
    assert_eq!(
        fm.get_region_at_line(10).unwrap().command_text.as_deref(),
        Some("second")
    );
}

#[test]
fn long_command_and_label_preserved() {
    let mut fm = FoldManager::new();
    let long_cmd = "a".repeat(200);
    fm.register_osc133_region(5, 15, long_cmd.clone(), Some(0));
    assert_eq!(
        fm.get_region_at_line(5).unwrap().command_text,
        Some(long_cmd)
    );

    let mut fm2 = FoldManager::new();
    let long_label = "b".repeat(200);
    fm2.register_custom_region(5, 15, long_label.clone());
    assert_eq!(fm2.get_region_at_line(5).unwrap().label, Some(long_label));
}

// ── Toggle ───────────────────────────────────────────────

#[test]
fn toggle_collapses_then_expands() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));

    assert!(fm.toggle_fold(5));
    assert!(fm.get_region_at_line(5).unwrap().collapsed);

    assert!(fm.toggle_fold(5));
    assert!(!fm.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn toggle_on_missing_line_returns_false() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert!(!fm.toggle_fold(20));
}

#[test]
fn toggle_on_interior_line_collapses_region() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert!(fm.toggle_fold(10));
    assert!(fm.get_region_at_line(5).unwrap().collapsed);
}

// ── get_region_at_line ───────────────────────────────────

#[test]
fn region_at_line_inside_returns_region() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert!(fm.get_region_at_line(5).is_some());
    assert!(fm.get_region_at_line(10).is_some());
    assert!(fm.get_region_at_line(14).is_some());
}

#[test]
fn region_at_line_outside_returns_none() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert!(fm.get_region_at_line(4).is_none());
    assert!(fm.get_region_at_line(15).is_none());
    assert!(fm.get_region_at_line(100).is_none());
}

#[test]
fn region_at_line_with_multiple_regions() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(20, 30, "second".to_string(), Some(1));

    assert_eq!(
        fm.get_region_at_line(10).unwrap().command_text.as_deref(),
        Some("first")
    );
    assert_eq!(
        fm.get_region_at_line(25).unwrap().command_text.as_deref(),
        Some("second")
    );
    assert!(fm.get_region_at_line(17).is_none());
}

#[test]
fn region_at_line_on_empty_returns_none() {
    let fm = FoldManager::new();
    assert!(fm.get_region_at_line(0).is_none());
    assert!(fm.get_region_at_line(100).is_none());
}

// ── get_collapsed_regions ────────────────────────────────

#[test]
fn collapsed_regions_only_collapsed_sorted() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(20, 30, "second".to_string(), Some(0));
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(40, 50, "third".to_string(), Some(0));

    fm.toggle_fold(5);
    fm.toggle_fold(40);

    let collapsed = fm.get_collapsed_regions();
    assert_eq!(collapsed.len(), 2);
    assert_eq!(collapsed[0].start_line, 5);
    assert_eq!(collapsed[1].start_line, 40);
}

#[test]
fn collapsed_cache_reflects_toggle_after_read() {
    // Reading builds the cache; a subsequent toggle must invalidate it so
    // the next read sees the new state.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert_eq!(fm.get_collapsed_regions().len(), 0);
    fm.toggle_fold(5);
    assert_eq!(fm.get_collapsed_regions().len(), 1);
    fm.toggle_fold(5);
    assert_eq!(fm.get_collapsed_regions().len(), 0);
}

#[test]
fn has_collapsed_regions_tracks_state() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    assert!(!fm.has_collapsed_regions());
    fm.toggle_fold(5);
    assert!(fm.has_collapsed_regions());
    fm.toggle_fold(5);
    assert!(!fm.has_collapsed_regions());
}

// ── Line mapping: display_line_to_actual ─────────────────

#[test]
fn display_to_actual_identity_when_no_collapse() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    // Registered but not collapsed → identity mapping.
    assert_eq!(fm.display_line_to_actual(0), 0);
    assert_eq!(fm.display_line_to_actual(10), 10);
    assert_eq!(fm.display_line_to_actual(20), 20);
}

#[test]
fn display_to_actual_one_fold() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);

    // Before the fold: identity.
    assert_eq!(fm.display_line_to_actual(0), 0);
    assert_eq!(fm.display_line_to_actual(4), 4);
    // Summary row.
    assert_eq!(fm.display_line_to_actual(5), 5);
    // First row after the summary skips the 9 hidden rows.
    assert_eq!(fm.display_line_to_actual(6), 15);
    assert_eq!(fm.display_line_to_actual(7), 16);
}

#[test]
fn display_to_actual_multiple_folds() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
    fm.toggle_fold(5);
    fm.toggle_fold(25);

    assert_eq!(fm.display_line_to_actual(4), 4);
    assert_eq!(fm.display_line_to_actual(5), 5);
    assert_eq!(fm.display_line_to_actual(6), 15);
    // Second fold's summary sits at display 16 (actual 25 - 9 hidden).
    assert_eq!(fm.display_line_to_actual(16), 25);
    // After both folds (18 hidden total).
    assert_eq!(fm.display_line_to_actual(17), 35);
}

#[test]
fn display_to_actual_out_of_range_above_all_folds() {
    // A display line below every fold start stays identity (no fold
    // contributes an offset because `actual < start_line` breaks early).
    let mut fm = FoldManager::new();
    fm.register_osc133_region(50, 60, "test".to_string(), Some(0));
    fm.toggle_fold(50);
    assert_eq!(fm.display_line_to_actual(3), 3);
}

// ── Line mapping: actual_line_to_display ─────────────────

#[test]
fn actual_to_display_one_fold() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);

    assert_eq!(fm.actual_line_to_display(0), 0);
    assert_eq!(fm.actual_line_to_display(4), 4);
    // Start of the fold = summary row.
    assert_eq!(fm.actual_line_to_display(5), 5);
    // Interior of a collapsed region collapses onto the summary row.
    assert_eq!(fm.actual_line_to_display(10), 5);
    // After the fold: actual 15 → display 6.
    assert_eq!(fm.actual_line_to_display(15), 6);
    assert_eq!(fm.actual_line_to_display(16), 7);
}

#[test]
fn round_trip_no_collapse_is_identity() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    for line in [0u32, 4, 5, 10, 14, 15, 20, 100] {
        let display = fm.actual_line_to_display(line);
        assert_eq!(fm.display_line_to_actual(display), line);
    }
}

#[test]
fn round_trip_single_collapse_outside_body() {
    // For rows outside the collapsed body, display→actual→display and
    // actual→display→actual both round-trip cleanly.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);
    // Actual rows outside [6, 15) (the hidden body) round-trip.
    for actual in [0u32, 4, 5, 15, 16, 30] {
        let display = fm.actual_line_to_display(actual);
        assert_eq!(
            fm.display_line_to_actual(display),
            actual,
            "actual {actual} did not round-trip (display {display})"
        );
    }
}

#[test]
fn round_trip_multiple_collapse_outside_body() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
    fm.toggle_fold(5);
    fm.toggle_fold(25);
    // Rows outside both hidden bodies round-trip.
    for actual in [0u32, 4, 5, 15, 16, 24, 25, 35, 36, 100] {
        let display = fm.actual_line_to_display(actual);
        assert_eq!(
            fm.display_line_to_actual(display),
            actual,
            "actual {actual} did not round-trip (display {display})"
        );
    }
}

#[test]
fn round_trip_adjacent_collapsed_regions() {
    // Two adjacent collapsed regions (5..10, 10..15). Each summary row
    // and each post-region row must round-trip.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
    fm.register_osc133_region(10, 15, "second".to_string(), Some(0));
    fm.toggle_fold(5);
    fm.toggle_fold(10);
    for actual in [0u32, 4, 5, 10, 15, 16, 50] {
        let display = fm.actual_line_to_display(actual);
        assert_eq!(
            fm.display_line_to_actual(display),
            actual,
            "actual {actual} did not round-trip (display {display})"
        );
    }
}

// ── Summary line queries ─────────────────────────────────

#[test]
fn is_summary_line_only_at_summary() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);
    assert!(fm.is_summary_line(5));
    assert!(!fm.is_summary_line(4));
    assert!(!fm.is_summary_line(6));
}

#[test]
fn summary_region_returns_region_for_summary_line() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test cmd".to_string(), Some(0));
    fm.toggle_fold(5);

    let r = fm.get_summary_region(5).expect("summary region present");
    assert_eq!(r.command_text.as_deref(), Some("test cmd"));
    assert!(fm.get_summary_region(4).is_none());
    assert!(fm.get_summary_region(6).is_none());
}

#[test]
fn summary_region_none_when_no_collapse() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    // Not collapsed → no summary rows at all.
    assert!(fm.get_summary_region(5).is_none());
}

// ── get_total_display_lines ──────────────────────────────

#[test]
fn total_display_lines() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    // No collapse: total unchanged.
    assert_eq!(fm.get_total_display_lines(100), 100);
    // Collapse hides 9 body rows.
    fm.toggle_fold(5);
    assert_eq!(fm.get_total_display_lines(100), 91);
}

// ── Pruning ──────────────────────────────────────────────

#[test]
fn prune_removes_old_and_rebases() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "old".to_string(), Some(0));
    fm.register_osc133_region(25, 35, "new".to_string(), Some(0));

    fm.prune_before_line(20);

    // Old region (5..15, before boundary 20) is gone; new region 25..35
    // re-bases to 5..15 and is re-keyed.
    let r = fm.get_region_at_line(5).expect("rebased region present");
    assert_eq!(r.command_text.as_deref(), Some("new"));
    assert_eq!(r.start_line, 5);
    assert_eq!(r.end_line, 15);
    assert_eq!(r.id, "osc133:5");
    assert!(fm.get_region_at_line(0).is_none());
}

#[test]
fn prune_adjusts_remaining_indices() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(20, 30, "test".to_string(), Some(0));
    fm.prune_before_line(10);

    let r = fm.get_region_at_line(10).expect("region present");
    assert_eq!(r.start_line, 10);
    assert_eq!(r.end_line, 20);
    assert_eq!(r.id, "osc133:10");
}

#[test]
fn prune_removes_region_spanning_boundary() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "spanning".to_string(), Some(0));
    fm.register_osc133_region(20, 30, "after".to_string(), Some(0));

    fm.prune_before_line(10);
    // 5..15 spans boundary 10 → removed; 20..30 → 10..20.
    assert_eq!(fm.get_collapsed_regions().len(), 0);
    let r = fm.get_region_at_line(10).expect("after region present");
    assert_eq!(r.command_text.as_deref(), Some("after"));
}

#[test]
fn prune_preserves_collapsed_state() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(20, 30, "test".to_string(), Some(0));
    fm.toggle_fold(20);

    fm.prune_before_line(10);
    let r = fm.get_region_at_line(10).expect("region present");
    assert!(r.collapsed);
}

#[test]
fn prune_rebases_custom_id() {
    // A custom region's re-keyed ID keeps the `custom:` prefix.
    let mut fm = FoldManager::new();
    fm.register_custom_region(20, 30, "label".to_string());
    fm.prune_before_line(10);
    let r = fm.get_region_at_line(10).expect("region present");
    assert_eq!(r.id, "custom:10");
    assert_eq!(r.label.as_deref(), Some("label"));
}

#[test]
fn prune_on_empty_does_not_panic() {
    let mut fm = FoldManager::new();
    fm.prune_before_line(10);
    assert!(fm.get_region_at_line(0).is_none());
}

// ── unfold_all ───────────────────────────────────────────

#[test]
fn unfold_all_expands_everything() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(20, 30, "second".to_string(), Some(0));
    fm.toggle_fold(5);
    fm.toggle_fold(20);
    assert_eq!(fm.get_collapsed_regions().len(), 2);

    fm.unfold_all();
    assert_eq!(fm.get_collapsed_regions().len(), 0);
    assert!(!fm.get_region_at_line(5).unwrap().collapsed);
    assert!(!fm.get_region_at_line(20).unwrap().collapsed);
}

#[test]
fn unfold_all_on_empty_does_not_panic() {
    let mut fm = FoldManager::new();
    fm.unfold_all();
    assert_eq!(fm.get_collapsed_regions().len(), 0);
}

// ── Enabled / disabled ───────────────────────────────────

#[test]
fn disabled_prevents_toggle() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.set_enabled(false);
    assert!(!fm.toggle_fold(5));
    assert!(!fm.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn set_enabled_false_unfolds_all() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);
    assert_eq!(fm.get_collapsed_regions().len(), 1);

    fm.set_enabled(false);
    assert_eq!(fm.get_collapsed_regions().len(), 0);
    // The region record itself survives the disable.
    assert!(fm.get_region_at_line(5).is_some());
}

#[test]
fn set_enabled_true_after_disabled_allows_toggle() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.set_enabled(false);
    fm.set_enabled(true);
    assert!(fm.toggle_fold(5));
    assert!(fm.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn is_enabled_reflects_state() {
    let mut fm = FoldManager::new();
    assert!(fm.is_enabled());
    fm.set_enabled(false);
    assert!(!fm.is_enabled());
    fm.set_enabled(true);
    assert!(fm.is_enabled());
}

// ── expand_region_containing ─────────────────────────────

#[test]
fn expand_region_containing_expands_collapsed() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);
    assert!(fm.get_region_at_line(5).unwrap().collapsed);

    assert!(fm.expand_region_containing(10));
    assert!(!fm.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn expand_region_containing_false_when_not_collapsed_or_outside() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    // In an expanded region → false.
    assert!(!fm.expand_region_containing(10));
    // Outside any region → false.
    assert!(!fm.expand_region_containing(20));
}

// ── Edge cases ───────────────────────────────────────────

#[test]
fn adjacent_regions_are_independent() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 10, "first".to_string(), Some(0));
    fm.register_osc133_region(10, 15, "second".to_string(), Some(0));

    fm.toggle_fold(5);
    assert!(fm.get_region_at_line(5).unwrap().collapsed);
    assert!(!fm.get_region_at_line(10).unwrap().collapsed);
}

// ── Forward-cursor build_layout boundaries ───────────────

#[test]
fn build_layout_cursor_region_just_before_window() {
    // Region 2..5 is collapsed and its summary (display line 2) is
    // entirely above display_start = 5. The forward cursor must skip it
    // so that rows inside the window get the correct actual lines.
    //
    // scrollback_len=10, viewport=5, offset=0.
    // total_actual=15, hidden=4 (line_count=5, hides 4), total_display=11.
    // display_start = 11 - 5 - 0 = 6.
    // display 6 → actual 6 + 4 = 10, display 7 → actual 11, ...
    let mut fm = FoldManager::new();
    fm.register_osc133_region(2, 7, "early".to_string(), Some(0)); // line_count=5, hides 4
    fm.toggle_fold(2);
    let layout = fm.build_layout(10, 5, 0);
    assert_eq!(layout.display_start, 6);
    assert_eq!(layout.rows.len(), 5);
    // Display 6..10 are all normal cells shifted by 4 hidden rows.
    for (r, kind) in layout.rows.iter().enumerate() {
        match kind {
            FoldRowKind::Cells { actual_line } => {
                assert_eq!(
                    *actual_line,
                    6 + 4 + r as u32,
                    "row {r}: expected actual {}, got {actual_line}",
                    6 + 4 + r as u32
                );
            }
            FoldRowKind::Summary { .. } => panic!("no summary expected at row {r}"),
        }
    }
}

#[test]
fn build_layout_cursor_region_spans_window_start() {
    // Region 0..20 is collapsed.  Even with scroll_offset placing
    // display_start inside the gap after the summary, the layout must not
    // emit a second summary row mid-window.
    //
    // total_actual = 30+5=35, hidden=19, total_display=16.
    // With scroll_offset=0: display_start = 16-5-0 = 11.
    // The collapsed region's summary is at display 0. display_start (11)
    // is deep inside the post-region area.  Rows 11..15 map to actual
    // 11+19=30, 31, 32, 33, 34.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(0, 20, "big".to_string(), Some(0)); // hides 19
    fm.toggle_fold(0);
    let layout = fm.build_layout(30, 5, 0);
    assert_eq!(layout.display_start, 11);
    for (r, kind) in layout.rows.iter().enumerate() {
        match kind {
            FoldRowKind::Cells { actual_line } => {
                assert_eq!(
                    *actual_line,
                    30 + r as u32,
                    "row {r} expected actual {}",
                    30 + r as u32
                );
            }
            FoldRowKind::Summary { .. } => panic!("unexpected summary at row {r}"),
        }
    }
}

#[test]
fn build_layout_cursor_summary_at_window_start() {
    // Region 0..5 collapsed; display_start lands exactly on the summary.
    // scrollback_len=10, viewport=5, offset such that display_start=0.
    // total_actual=15, hidden=4, total_display=11, offset=11-5=6.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(0, 5, "top".to_string(), Some(0));
    fm.toggle_fold(0);
    let layout = fm.build_layout(10, 5, 6);
    assert_eq!(layout.display_start, 0);
    // Row 0 must be the summary.
    match &layout.rows[0] {
        FoldRowKind::Summary { region } => assert_eq!(region.start_line, 0),
        other => panic!("expected summary, got {other:?}"),
    }
    // Rows 1..4 must be normal cells starting at actual 5.
    for (r, kind) in layout.rows[1..].iter().enumerate() {
        match kind {
            FoldRowKind::Cells { actual_line } => {
                assert_eq!(*actual_line, 5 + r as u32);
            }
            FoldRowKind::Summary { .. } => panic!("unexpected summary at row {}", r + 1),
        }
    }
}

#[test]
fn build_layout_cursor_two_consecutive_collapsed() {
    // Two adjacent collapsed regions 0..3 and 3..6 (each hides 2 rows).
    // With display_start=0, rows 0,1 are summaries; row 2 is actual 6.
    //
    // total_actual=10+4=14, hidden=4, total_display=10.
    // offset=10-4-0=6; but let's use large scroll_offset to pin display_start=0.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(0, 3, "first".to_string(), Some(0)); // hides 2
    fm.register_osc133_region(3, 6, "second".to_string(), Some(0)); // hides 2
    fm.toggle_fold(0);
    fm.toggle_fold(3);
    // total_actual=10+4=14, hidden=4, total_display=10.
    // display_start = 10-4-9999 saturates to 0.
    let layout = fm.build_layout(10, 4, 9999);
    assert_eq!(layout.display_start, 0);
    // Row 0 = summary for 0..3.
    match &layout.rows[0] {
        FoldRowKind::Summary { region } => assert_eq!(region.start_line, 0),
        other => panic!("row 0: expected summary, got {other:?}"),
    }
    // Row 1 = summary for 3..6.
    match &layout.rows[1] {
        FoldRowKind::Summary { region } => assert_eq!(region.start_line, 3),
        other => panic!("row 1: expected summary, got {other:?}"),
    }
    // Row 2 = actual 6.
    assert_eq!(layout.rows[2], FoldRowKind::Cells { actual_line: 6 });
    // Row 3 = actual 7.
    assert_eq!(layout.rows[3], FoldRowKind::Cells { actual_line: 7 });
}

// ── FoldLayout binary-search boundaries ──────────────────

#[test]
fn layout_region_at_line_boundary_before_region() {
    // actual_line == start_line - 1 must return None.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    assert!(layout.region_at_line(9).is_none());
}

#[test]
fn layout_region_at_line_boundary_at_start() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    assert!(layout.region_at_line(10).is_some());
}

#[test]
fn layout_region_at_line_boundary_at_end_exclusive() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    // end_line is exclusive.
    assert!(layout.region_at_line(20).is_none());
    assert!(layout.region_at_line(19).is_some());
}

#[test]
fn layout_actual_to_display_bsearch_row_before_region() {
    // actual_line just before a collapsed region: no offset applied.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0)); // hides 9
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    // actual 9 → display 9 (no collapsed region before it).
    assert_eq!(layout.actual_line_to_display(9), 9);
}

#[test]
fn layout_actual_to_display_bsearch_summary_row() {
    // actual_line == start_line of collapsed region → summary row.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0));
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    // No regions before this one: offset_before=0. Summary at display 10.
    assert_eq!(layout.actual_line_to_display(10), 10);
}

#[test]
fn layout_actual_to_display_bsearch_after_region() {
    // actual_line just after a collapsed region: offset = line_count - 1.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(10, 20, "r".to_string(), Some(0)); // hides 9
    fm.toggle_fold(10);
    let layout = fm.build_layout(30, 5, 0);
    // actual 20 → display 20 - 9 = 11.
    assert_eq!(layout.actual_line_to_display(20), 11);
}

#[test]
fn layout_actual_to_display_bsearch_two_regions() {
    // Two collapsed regions; verify prefix-sum is applied correctly.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0)); // hides 9
    fm.register_osc133_region(25, 35, "second".to_string(), Some(1)); // hides 9
    fm.toggle_fold(5);
    fm.toggle_fold(25);
    let layout = fm.build_layout(40, 5, 0);
    // Before first region.
    assert_eq!(layout.actual_line_to_display(4), 4);
    // Summary of first.
    assert_eq!(layout.actual_line_to_display(5), 5);
    // After first, before second: offset 9.
    assert_eq!(layout.actual_line_to_display(15), 6);
    assert_eq!(layout.actual_line_to_display(24), 15);
    // Summary of second: start 25 - 9 = 16.
    assert_eq!(layout.actual_line_to_display(25), 16);
    // After both regions: offset 18.
    assert_eq!(layout.actual_line_to_display(35), 17);
    assert_eq!(layout.actual_line_to_display(100), 82);
}

#[test]
fn layout_actual_to_display_matches_manager_bsearch() {
    // Confirm the binary-search implementation agrees with FoldManager
    // across a range of actual lines, including region boundaries.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 10, "a".to_string(), Some(0));
    fm.register_osc133_region(10, 15, "b".to_string(), Some(0));
    fm.register_osc133_region(20, 30, "c".to_string(), Some(0));
    fm.toggle_fold(5);
    fm.toggle_fold(10);
    fm.toggle_fold(20);
    let layout = fm.build_layout(40, 5, 0);
    for actual in [0u32, 4, 5, 9, 10, 14, 15, 19, 20, 29, 30, 50, 100] {
        assert_eq!(
            layout.actual_line_to_display(actual),
            fm.actual_line_to_display(actual),
            "actual {actual} mismatch between layout and manager"
        );
    }
}

// ── build_layout / FoldLayout ────────────────────────────

#[test]
fn build_layout_no_collapse_is_identity_window() {
    // No collapsed region: every screen row maps to a linear buffer row
    // starting at display_start (= total - viewport - offset).
    let mut fm = FoldManager::new();
    fm.register_osc133_region(2, 5, "test".to_string(), Some(0));
    // scrollback_len = 10, viewport = 4, offset = 0.
    // total_display = 14 (nothing hidden), display_start = 14 - 4 - 0 = 10.
    let layout = fm.build_layout(10, 4, 0);
    assert_eq!(layout.display_start, 10);
    assert_eq!(layout.rows.len(), 4);
    for (r, kind) in layout.rows.iter().enumerate() {
        match kind {
            FoldRowKind::Cells { actual_line } => assert_eq!(*actual_line, 10 + r as u32),
            FoldRowKind::Summary { .. } => panic!("no summary expected"),
        }
    }
}

#[test]
fn build_layout_collapsed_region_marks_summary_and_skips_body() {
    // Region 2..6 (4 rows) collapsed → hides 3 body rows. With
    // scrollback_len = 10, viewport = 8, offset large enough to show
    // from the top, the summary sits at display line 2.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(2, 6, "ls".to_string(), Some(0));
    fm.toggle_fold(2);
    // total_actual = 18, hidden = 3, total_display = 15.
    // Pick offset so display_start = 0: offset = 15 - 8 = 7.
    let layout = fm.build_layout(10, 8, 7);
    assert_eq!(layout.display_start, 0);
    assert_eq!(layout.rows.len(), 8);
    // Display lines 0,1 = actual 0,1.
    assert_eq!(layout.rows[0], FoldRowKind::Cells { actual_line: 0 });
    assert_eq!(layout.rows[1], FoldRowKind::Cells { actual_line: 1 });
    // Display line 2 = summary for region 2..6.
    match &layout.rows[2] {
        FoldRowKind::Summary { region } => {
            assert_eq!(region.start_line, 2);
            assert_eq!(region.command_text.as_deref(), Some("ls"));
        }
        other => panic!("expected summary, got {other:?}"),
    }
    // Display line 3 = actual 6 (first row after the hidden body).
    assert_eq!(layout.rows[3], FoldRowKind::Cells { actual_line: 6 });
    assert_eq!(layout.rows[4], FoldRowKind::Cells { actual_line: 7 });
}

#[test]
fn build_layout_display_start_saturates_at_zero() {
    // A scroll_offset larger than the content keeps display_start at 0
    // (saturating) rather than underflowing.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(2, 6, "x".to_string(), Some(0));
    fm.toggle_fold(2);
    let layout = fm.build_layout(10, 8, 9999);
    assert_eq!(layout.display_start, 0);
}

#[test]
fn fold_layout_region_at_line_only_collapsed() {
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    fm.toggle_fold(5);
    let layout = fm.build_layout(20, 5, 0);
    // Inside the collapsed body → Some.
    assert!(layout.region_at_line(5).is_some());
    assert!(layout.region_at_line(14).is_some());
    // Outside → None.
    assert!(layout.region_at_line(4).is_none());
    assert!(layout.region_at_line(15).is_none());
}

#[test]
fn fold_layout_region_at_line_excludes_expanded() {
    // An expanded region is not in the collapsed snapshot, so the
    // layout reports no region there (search must NOT skip its matches).
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "test".to_string(), Some(0));
    // Not collapsed.
    let layout = fm.build_layout(20, 5, 0);
    assert!(layout.region_at_line(10).is_none());
}

#[test]
fn fold_layout_actual_to_display_matches_manager() {
    // The immutable FoldLayout mapping agrees with FoldManager's.
    let mut fm = FoldManager::new();
    fm.register_osc133_region(5, 15, "first".to_string(), Some(0));
    fm.register_osc133_region(25, 35, "second".to_string(), Some(1));
    fm.toggle_fold(5);
    fm.toggle_fold(25);
    let layout = fm.build_layout(40, 5, 0);
    for actual in [0u32, 4, 5, 10, 15, 16, 24, 25, 30, 35, 36, 100] {
        assert_eq!(
            layout.actual_line_to_display(actual),
            fm.actual_line_to_display(actual),
            "actual {actual} mismatch"
        );
    }
}
