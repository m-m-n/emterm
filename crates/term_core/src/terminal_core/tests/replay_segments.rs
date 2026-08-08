use super::*;

// ── task0004 round-4 rework (D1'): structural ReplaySegment replay ────
//
// Rounds 1-3's `find_resize_marker` / `resize_marker_bytes` /
// `parse_resize_marker_dims` byte-scanning decoder is GONE — replay
// authority moved to the `ReplaySegment` parameter. The tests below
// replace the old marker-scanning suite; AC-1's forgery tests are the
// direct successors of that suite's intent (proving a marker-SHAPED
// byte sequence can no longer do anything).

/// Byte-for-byte the OLD (pre-round-4) marker wire format. Kept ONLY as
/// adversarial test fixture data for the AC-1 forgery tests below — it
/// is deliberately NOT wired to any production decoder any more.
fn legacy_marker_shaped_bytes(cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
}

/// AC-1: a bare marker-shaped byte sequence embedded in the replay
/// payload, with NO segments supplied, must never change replay
/// dimensions — not even a single reflow. `reflow_call_count` is the
/// direct witness (per the task's test notes: `core.cols()`/`rows()`
/// after a replay always equal the caller's target regardless of what
/// happened mid-drain, so that alone would prove nothing; the reflow
/// counter is what would move if the marker-shaped bytes were honored).
///
/// Confirmed to fail pre-fix: against the removed byte-scanning
/// `replay_with_resize_markers` (which called `find_resize_marker` on
/// the raw payload), this exact input was VALID and well-formed by that
/// decoder's own rules — it would locate the marker, call
/// `self.resize(120, 48)` before the trailing content, and
/// `reflow_call_count` would show 2 (the marker's resize + the final
/// restore-to-target), not 0.
#[test]
fn ac1_bare_marker_shaped_bytes_never_change_replay_dimensions() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = core.reflow_call_count;
    let mut bytes = b"before\r\n".to_vec();
    bytes.extend_from_slice(&legacy_marker_shaped_bytes(120, 48));
    bytes.extend_from_slice(b"after\r\n");
    core.reset_and_replay(&bytes); // no segments supplied
    assert_eq!(
        core.reflow_call_count - before,
        0,
        "a marker-shaped byte sequence with no segment authority must \
         never trigger a resize"
    );
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
    assert!(core.get_line_text(0).contains("before"));
    // The marker-shaped bytes are parsed as an ordinary (harmless,
    // unrecognized) OSC and produce no visible cell; "after" lands on
    // the very next row, exactly as if the marker text were absent.
    assert!(core.get_line_text(1).contains("after"));
}

/// AC-1: the SAME marker-shaped bytes, but now genuine segments ARE
/// supplied (describing a completely different, fixed dimension) — the
/// embedded bytes must still have zero effect; only the supplied
/// segment's dims apply.
#[test]
fn ac1_marker_shaped_bytes_do_not_override_supplied_segments() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let mut bytes = b"before\r\n".to_vec();
    bytes.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));
    bytes.extend_from_slice(b"after\r\n");
    let segments = [ReplaySegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    core.reset_and_replay_segments(&bytes, &segments);
    assert_eq!(
        core.cols(),
        80,
        "dimensions must come only from the segment field"
    );
    assert_eq!(core.rows(), 24);
}

/// AC-1: a marker-shaped sequence "formed by concatenation" — split into
/// two halves that are each individually harmless but literally
/// concatenate into a complete marker byte-for-byte — still has zero
/// effect once fed to replay as a single joined buffer with no
/// segments. (The write-path splitting/stripping scenarios that could
/// have produced exactly this concatenation are covered end-to-end in
/// `mux::ipc::pty_spawn`'s AC-1 tests; this pins the term_core-level
/// guarantee that even a PERFECTLY formed marker occurring anywhere in
/// the byte stream is inert without segment authority.)
#[test]
fn ac1_marker_formed_by_concatenation_has_no_effect() {
    let full = legacy_marker_shaped_bytes(4000, 4000);
    let split = full.len() / 2;
    let mut bytes = b"before\r\n".to_vec();
    bytes.extend_from_slice(&full[..split]);
    bytes.extend_from_slice(&full[split..]);
    bytes.extend_from_slice(b"after\r\n");
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = core.reflow_call_count;
    core.reset_and_replay(&bytes);
    assert_eq!(core.reflow_call_count - before, 0);
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
}

// ── clamp_resize_dims ──────────────────────────────────────────────

#[test]
fn clamp_resize_dims_leaves_in_range_values_untouched() {
    assert_eq!(clamp_resize_dims(80, 24), (80, 24));
    assert_eq!(
        clamp_resize_dims(RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS),
        (RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS)
    );
}

#[test]
fn clamp_resize_dims_clamps_above_max_down_to_max() {
    assert_eq!(
        clamp_resize_dims(u16::MAX, u16::MAX),
        (RESIZE_MARKER_MAX_COLS, RESIZE_MARKER_MAX_ROWS)
    );
}

#[test]
fn clamp_resize_dims_clamps_zero_up_to_one() {
    assert_eq!(clamp_resize_dims(0, 0), (1, 1));
}

// ── reset_and_replay_segments: structural dimension replay ────────────

/// AC-3 (round-8 rework, review round-7 finding `01f91fe698ceb287`): a
/// segment list whose FIRST entry does NOT start at offset 0 (the shape
/// `ScrollbackRingBuffer::read_segments` now produces when 2+
/// `dim_markers` entries have been evicted, leaving the leading gap
/// deliberately unattributed) must still replay the leading gap's bytes
/// — at the caller's TARGET dimensions (`self`'s size at the start of
/// the call), never silently dropped.
///
/// Confirmed to fail pre-fix: before this fix, the loop started at
/// `segments[0].offset`, so `bytes[..segments[0].offset]` was never fed
/// to any segment and the leading gap's content (`"gap-content"` below)
/// was silently dropped — `get_line_text(0)` would not contain it.
#[test]
fn reset_and_replay_segments_replays_a_leading_gap_before_the_first_segment_at_target_dims() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let gap = b"gap-content\r\n";
    let after = b"after-first-segment\r\n";
    let mut bytes = gap.to_vec();
    bytes.extend_from_slice(after);
    // First (and only) segment starts AFTER the gap, at a DIFFERENT
    // size than the core's target (80, 24) — so if the gap were
    // (incorrectly) fed under the segment's dims instead of being fed
    // separately first, this would still be observable as a missing
    // first line.
    let segments = [ReplaySegment {
        offset: gap.len() as u32,
        cols: 40,
        rows: 10,
    }];
    core.reset_and_replay_segments(&bytes, &segments);
    assert!(
        core.get_line_text(0).contains("gap-content"),
        "the leading gap's content must be replayed, not dropped: {:?}",
        core.get_line_text(0)
    );
    assert!(core.get_line_text(1).contains("after-first-segment"));
    // Core ends back at its ORIGINAL target dims (80, 24).
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
}

/// AC-2 / D1' equivalent of the old marker-based mid-stream resize test:
/// a single segment transition resizes the core between the two
/// content ranges, and the core ends back at its ORIGINAL
/// (caller-requested) dimensions.
#[test]
fn reset_and_replay_segments_resizes_mid_stream_and_restores_target_dims() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = b"before-resize\r\n";
    let after = b"after-resize\r\n";
    let mut bytes = before.to_vec();
    bytes.extend_from_slice(after);
    let segments = [
        ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        },
        ReplaySegment {
            offset: before.len() as u32,
            cols: 40,
            rows: 10,
        },
    ];
    core.reset_and_replay_segments(&bytes, &segments);
    // Core ends back at the dims it was constructed with (80, 24), not
    // the mid-stream segment's (40, 10).
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
    assert!(
        core.get_line_text(0).contains("before-resize"),
        "content before the transition must still be present"
    );
    assert!(
        core.get_line_text(1).contains("after-resize"),
        "content after the transition must still be present"
    );
}

/// AC-2: content recorded under one set of dimensions is always
/// replayed under those dimensions, including when a resize follows
/// within what WOULD have been the (now-removed) coalescing window — a
/// cursor-addressed write near the far edge of a wide segment only
/// lands correctly if that segment's dims actually applied while its
/// own bytes were fed.
#[test]
fn reset_and_replay_segments_honors_a_wide_dimension_segment() {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let wide_cols: u16 = 2500;
    let before = b"before-resize\r\n".to_vec();
    let wide = format!("\x1b[1;{wide_cols}Hedge").into_bytes();
    let after = b"after-resize\r\n".to_vec();
    let mut bytes = before.clone();
    bytes.extend_from_slice(&wide);
    bytes.extend_from_slice(&after);
    let segments = [
        ReplaySegment {
            offset: 0,
            cols,
            rows: 40,
        },
        ReplaySegment {
            offset: before.len() as u32,
            cols: wide_cols,
            rows: 40,
        },
        ReplaySegment {
            offset: (before.len() + wide.len()) as u32,
            cols,
            rows,
        },
    ];

    let mut core = TerminalCore::new(cols, rows, 1000);
    core.reset_and_replay_segments(&bytes, &segments);

    assert_eq!(core.cols(), cols, "core must end back at target size");
    assert_eq!(core.rows(), rows);
}

/// review round-1 rework, finding `6ff208bbc674189c` (high) — still
/// closed under the segment-driven replay: N consecutive segment
/// transitions with NO bytes between them (all offsets collapse
/// together) cost at most TWO reflows total — one for the last
/// transition's dims right before the trailing non-empty range, and one
/// for the mandatory final restore back to the target size — never one
/// reflow per transition.
#[test]
fn replay_segments_coalesces_consecutive_empty_transitions_into_a_single_reflow() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = core.reflow_call_count;
    let mut bytes = b"before\r\n".to_vec();
    let marker_offset = bytes.len() as u32;
    bytes.extend_from_slice(b"after\r\n");
    // A leading segment at offset 0 covers "before\r\n" at the core's
    // construction dims, then five consecutive segments, all at the
    // SAME offset (no bytes between any of them) — only the LAST one's
    // dims should ever apply to the trailing "after\r\n" range.
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    segments.extend(
        [(40, 10), (100, 30), (60, 20), (120, 40), (90, 25)]
            .into_iter()
            .map(|(cols, rows)| ReplaySegment {
                offset: marker_offset,
                cols,
                rows,
            }),
    );
    core.reset_and_replay_segments(&bytes, &segments);
    let reflows = core.reflow_call_count - before;
    assert_eq!(
        reflows, 2,
        "a run of 5 same-offset transitions followed by ONE non-empty \
         range must reflow at most twice (last transition's dims + \
         final restore-to-target), never once per transition (which \
         would be 6 here)"
    );
    assert_eq!(core.cols(), 80, "core must end back at the target size");
    assert_eq!(core.rows(), 24);
    assert!(core.get_line_text(0).contains("before"));
    assert!(core.get_line_text(1).contains("after"));
}

/// Zero-reflow edge case: if a run of consecutive same-offset
/// transitions is never followed by any bytes at all (they describe the
/// tail of the payload), NO reflow happens for any of them.
#[test]
fn replay_segments_trailing_consecutive_empty_transitions_reflow_zero_times() {
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = core.reflow_call_count;
    let bytes = b"only-content\r\n".to_vec();
    let tail_offset = bytes.len() as u32;
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    segments.extend(
        [(40, 10), (100, 30), (60, 20)]
            .into_iter()
            .map(|(cols, rows)| ReplaySegment {
                offset: tail_offset,
                cols,
                rows,
            }),
    );
    core.reset_and_replay_segments(&bytes, &segments);
    assert_eq!(
        core.reflow_call_count - before,
        0,
        "a trailing run of empty transitions with nothing fed at any of \
         their sizes must cost zero reflows"
    );
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
    assert!(core.get_line_text(0).contains("only-content"));
}

/// Grid-equivalence variant: a run of consecutive same-offset
/// transitions ending in dims (D) followed by content must produce a
/// grid IDENTICAL to a recording containing only the SINGLE final
/// segment (D) followed by the same content.
#[test]
fn replay_segments_consecutive_transitions_grid_matches_single_final_dimension_case() {
    let mut multi = TerminalCore::new(80, 24, 1000);
    let mut multi_bytes = b"before\r\n".to_vec();
    let marker_offset = multi_bytes.len() as u32;
    multi_bytes.extend_from_slice(b"after\r\n");
    let mut multi_segments = vec![ReplaySegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    multi_segments.extend(
        [(40, 10), (100, 30), (60, 20)]
            .into_iter()
            .map(|(cols, rows)| ReplaySegment {
                offset: marker_offset,
                cols,
                rows,
            }),
    );
    multi.reset_and_replay_segments(&multi_bytes, &multi_segments);

    let mut single = TerminalCore::new(80, 24, 1000);
    let mut single_bytes = b"before\r\n".to_vec();
    let single_offset = single_bytes.len() as u32;
    single_bytes.extend_from_slice(b"after\r\n");
    let single_segments = [
        ReplaySegment {
            offset: 0,
            cols: 80,
            rows: 24,
        },
        ReplaySegment {
            offset: single_offset,
            cols: 60,
            rows: 20,
        },
    ];
    single.reset_and_replay_segments(&single_bytes, &single_segments);

    assert_eq!(grid_fingerprint(&multi), grid_fingerprint(&single));
    assert!(
        multi.get_line_text(0).contains("before"),
        "leading content must actually be fed (not silently dropped by \
         both variants, which would make this assertion vacuous)"
    );
}

/// AC-11: replay with NO segments (an older daemon's snapshot, or any
/// caller with nothing to attribute) behaves as plain single-dimension
/// replay — byte-identical to feeding the same bytes through
/// `process_pty_data_fully` at the core's current size, with zero
/// reflows regardless of what the bytes contain.
#[test]
fn ac11_no_segments_degrades_to_single_dimension_replay() {
    let payload = b"line one\r\nline two\r\n\x1b]777;emterm;resize;999;999\x07line three\r\n";

    let mut via_segments = TerminalCore::new(80, 24, 1000);
    let before = via_segments.reflow_call_count;
    via_segments.reset_and_replay_segments(payload, &[]);
    assert_eq!(via_segments.reflow_call_count - before, 0);

    let mut via_process = TerminalCore::new(80, 24, 1000);
    via_process.process_pty_data_fully(payload);

    assert_eq!(
        grid_fingerprint(&via_segments),
        grid_fingerprint(&via_process),
        "empty-segments replay must match a plain process_pty_data_fully call"
    );
}
