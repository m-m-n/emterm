use super::*;

// ── task0001 D7: head/middle/tail generalization (resize-marker-dense
// tail rescue) ──────────────────────────────────────────────────────

/// AC-1 / AC-4 (task0001, D7): reproduces the measured "resize-marker-
/// dense scrollback tail" shape (see task0001's SPEC.md References) — a
/// large HEAD already at the target dims, followed by a dense cluster
/// of resize markers (dims oscillating between two values BELOW the
/// target, never reaching or exceeding it — the D7 safety condition)
/// whose own content is tiny, followed by a small qualifying tail back
/// at the target. Confirms:
///
/// - AC-1: the split engages (`scrollback_populated == false`) even
///   though the RAW "prefix" (head + cluster, the only definition of
///   that word before D7) exceeds every pre-D7 threshold: `k` (head +
///   cluster segment count) exceeds `BYPASS_PREFIX_MAX_SEGMENTS`, the
///   raw prefix byte length exceeds `BYPASS_PREFIX_MAX_BYTES`, and the
///   suffix does not dominate that raw prefix. D7 recognizes that only
///   the small MIDDLE (the cluster itself) needs non-bypass fidelity —
///   the HEAD can fold into bypass too.
/// - AC-4: the resulting viewport + cursor are byte-identical to the
///   fully synchronous reference (`build_scrollback_only_from_snapshot`)
///   for the SAME payload, `evicted_total` matches, and
///   `scrollback_populated` carries its usual meaning (`false` for the
///   bypass-engaged replay, `true` for the reference) — this fix does
///   not special-case this shape into a different, non-equivalent fast
///   path.
///
/// Confirmed to fail pre-fix (D7): reverting to `stable_target_suffix_start`
/// alone (no `h` / `leading_target_run_len`) makes `k` land at the
/// START of the HEAD (the head's own segment no longer counts as
/// "stable" once ANY later segment diverges, under the old trailing-
/// run-only definition) — `k` exceeds `BYPASS_PREFIX_MAX_SEGMENTS`, the
/// raw prefix byte length exceeds `BYPASS_PREFIX_MAX_BYTES`, the suffix
/// does not dominate it, `bypass_split` is `false`, and
/// `scrollback_populated` comes back `true`.
#[test]
fn head_plus_marker_cluster_engages_the_split_and_matches_reference() {
    let cols: u16 = 80;
    let target_rows: u16 = 30;
    let cluster_rows_a: u16 = 24;
    let cluster_rows_b: u16 = 26;

    // HEAD: a single large segment already AT the target — the bulk of
    // the pane's real history, well over BYPASS_PREFIX_MAX_BYTES (64
    // KiB) on its own so the OLD whole-prefix byte gate would already
    // reject this shape.
    let head_filler = b"head history line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: target_rows,
    }];
    while payload.len() <= 96 * 1024 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();
    assert!(
        head_len > 64 * 1024,
        "test prerequisite: HEAD alone must exceed BYPASS_PREFIX_MAX_BYTES"
    );

    // MIDDLE: a dense cluster of exactly BYPASS_PREFIX_MAX_SEGMENTS
    // resize markers, dims oscillating between two values below the
    // target, tiny content between them — combined with the HEAD
    // segment, `k` (head + cluster) is BYPASS_PREFIX_MAX_SEGMENTS + 1,
    // one past the OLD gate's bound, while the cluster's OWN segment
    // count (`middle_segment_count`) sits exactly AT the bound.
    let cluster_segment_count = BYPASS_PREFIX_MAX_SEGMENTS;
    let cluster_filler = b"x\r\n";
    for i in 0..cluster_segment_count {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: if i % 2 == 0 {
                cluster_rows_a
            } else {
                cluster_rows_b
            },
        });
        payload.extend_from_slice(cluster_filler);
    }
    let middle_len = payload.len() - head_len;
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: the cluster's OWN content must clear \
         BYPASS_PREFIX_MAX_BYTES for D7 to have anything to rescue"
    );

    // TAIL: small, just over BYPASS_SUFFIX_MIN_BYTES, back at the
    // target — dominates the MIDDLE (D7's new gate) but NOT the raw
    // head+cluster prefix (the OLD gate's dominance check).
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail history line padded out a bit for size\r\n";
    while payload.len() - head_len - middle_len < 4096 + 512 {
        payload.extend_from_slice(tail_filler);
    }
    let suffix_len = payload.len() - head_len - middle_len;
    assert!(
        suffix_len >= middle_len,
        "test prerequisite: suffix must dominate the MIDDLE (D7's gate)"
    );
    assert!(
        suffix_len < head_len + middle_len,
        "test prerequisite: suffix must NOT dominate the raw head+cluster \
         prefix (the OLD gate's dominance check must still fail here)"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let scrollback_lines = 10_000u32;
    let bypass_replay = TerminalCore::build_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("bypass-path build not cancelled");
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("reference build not cancelled");

    // AC-1: the split must engage despite the raw head+cluster prefix
    // failing every pre-D7 threshold.
    assert!(
        !bypass_replay.scrollback_populated,
        "AC-1: the split must engage (scrollback_populated == false) for \
         a resize-marker-dense tail behind an already-at-target HEAD — \
         got scrollback_populated == true (D7 did not rescue this shape)"
    );
    assert!(
        reference.scrollback_populated,
        "test prerequisite: the fully synchronous reference always \
         populates scrollback"
    );

    // AC-4: viewport + cursor equivalence with the fully synchronous
    // reference, and matching evicted_total (the split must not merely
    // look right on the grid while silently corrupting bookkeeping).
    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "AC-4: the head/middle/tail split's viewport + cursor must match \
         the fully synchronous reference for the marker-cluster shape"
    );
    assert_eq!(
        bypass_replay.evicted_total, reference.evicted_total,
        "AC-4: the split must preserve evicted_total byte-identically"
    );
}

/// AC-3 (task0004, D11, review round-1 rework findings
/// `474e01ad8c29e7f0` / `96f7205be52fece8` / `1adb07864f11618f`): the
/// "one past" side of the `h > 0` tier's own boundary — the mirror of
/// `head_plus_marker_cluster_engages_the_split_and_matches_reference`
/// above (identical HEAD/cluster/tail shape, verified `h > 0`, i.e. the
/// fold succeeds and `BYPASS_PREFIX_MAX_SEGMENTS` is the operative
/// bound) but with `BYPASS_PREFIX_MAX_SEGMENTS + 1` cluster segments —
/// must NOT engage the split. Before this test, the `h > 0` tier's
/// bound had only an "at bound engages" pin, not a "one past rejects"
/// one — this closes that gap (AC-3: "boundary unit tests pass at
/// exactly-at (engages) and one-past (rejects) for every operative
/// bound").
///
/// Confirmed to fail if the `h > 0` tier's segment-count gate were
/// dropped or widened: this cluster's own byte content stays tiny
/// (well under `BYPASS_PREFIX_MAX_BYTES`) and the tail dominates it, so
/// only the segment-count condition rejects this shape.
#[test]
fn head_plus_marker_cluster_one_past_the_fold_succeeded_bound_does_not_engage_the_split() {
    let cols: u16 = 80;
    let target_rows: u16 = 30;
    let cluster_rows_a: u16 = 24;
    let cluster_rows_b: u16 = 26;

    // HEAD: a single large segment already AT the target — same shape
    // as the "at bound" companion test above.
    let head_filler = b"head history line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: target_rows,
    }];
    while payload.len() <= 96 * 1024 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();

    // MIDDLE: BYPASS_PREFIX_MAX_SEGMENTS + 1 resize markers — one past
    // the `h > 0` tier's own bound — dims oscillating between two
    // values below the target (same row-bounded shape as the "at
    // bound" companion, so `h` still folds to `1`; only the COUNT
    // differs).
    let cluster_segment_count = BYPASS_PREFIX_MAX_SEGMENTS + 1;
    let cluster_filler = b"x\r\n";
    for i in 0..cluster_segment_count {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: if i % 2 == 0 {
                cluster_rows_a
            } else {
                cluster_rows_b
            },
        });
        payload.extend_from_slice(cluster_filler);
    }
    let middle_len = payload.len() - head_len;
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: the cluster's OWN content must clear \
         BYPASS_PREFIX_MAX_BYTES so only the segment-count gate rejects \
         this shape"
    );

    // TAIL: dominates the MIDDLE, same sizing as the "at bound"
    // companion test.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail history line padded out a bit for size\r\n";
    while payload.len() - head_len - middle_len < 4096 + 512 {
        payload.extend_from_slice(tail_filler);
    }
    let suffix_len = payload.len() - head_len - middle_len;
    assert!(
        suffix_len >= middle_len,
        "test prerequisite: suffix must dominate the MIDDLE so only the \
         segment-count gate rejects this shape"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        replay.scrollback_populated,
        "AC-3: a fold-succeeded (h > 0) MIDDLE with BYPASS_PREFIX_MAX_SEGMENTS \
         + 1 segments, one past its own bound, must NOT engage the split \
         even with a dominating suffix — scrollback_populated must be \
         true, not false"
    );
}

/// AC-1 (task0004, D8, review round-1 rework finding `b21749c5f2bd1006`):
/// the mirror of `head_plus_marker_cluster_engages_the_split_and_matches_reference`
/// above, but in the direction the SPEC's own root cause actually
/// takes: the settled target is the SMALLER (status-bar-visible) size,
/// so the pre-settling HEAD — and roughly half the resize-marker
/// cluster — sits at rows ABOVE the target, not below it. A large HEAD
/// already at a size N > target_rows (predating the storm), followed
/// by a dense cluster oscillating between N and the (smaller) target,
/// whose own content is tiny, followed by a small qualifying tail back
/// at the target. Confirms:
///
/// - AC-1: the split engages (`scrollback_populated == false`) even
///   though the RAW "prefix" (head + cluster) exceeds every pre-D7
///   threshold, exactly as the below-target companion test proves for
///   the other direction — `leading_uniform_run_len`'s HEAD need not
///   open at `target_rows` itself, only at SOME uniform size the
///   cluster stays within.
/// - AC-4-equivalence: the resulting viewport + cursor and
///   `evicted_total` match the fully synchronous reference.
///
/// Confirmed to fail pre-fix (task0004 D8): reverting to
/// `leading_target_run_len` (HEAD must open AT `target_rows`) makes `h`
/// land at `0` for this shape (the HEAD opens at `N`, not
/// `target_rows`), so `middle_segment_count` and the raw prefix byte
/// length are exactly the head+cluster totals again — both exceed
/// their bounds, the tail does not dominate them, `bypass_split` is
/// `false`, and `scrollback_populated` comes back `true`.
#[test]
fn head_plus_marker_cluster_above_target_engages_the_split_and_matches_reference() {
    let cols: u16 = 80;
    let target_rows: u16 = 24;
    let head_rows: u16 = 30;
    let cluster_rows_below: u16 = 24;

    // HEAD: a single large segment already at `head_rows` (N), the size
    // the pane held BEFORE the resize storm — well over
    // `BYPASS_PREFIX_MAX_BYTES` (64 KiB) on its own so the OLD
    // whole-prefix byte gate would already reject this shape.
    let head_filler = b"head history line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: head_rows,
    }];
    while payload.len() <= 96 * 1024 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();
    assert!(
        head_len > 64 * 1024,
        "test prerequisite: HEAD alone must exceed BYPASS_PREFIX_MAX_BYTES"
    );

    // MIDDLE: a dense cluster of exactly BYPASS_PREFIX_MAX_SEGMENTS
    // resize markers, dims oscillating between the target (BELOW
    // head_rows) and head_rows itself — never exceeding head_rows, the
    // D8 safety condition — ending on `head_rows` (NOT the target) so
    // the "settling" drop happens right at the k boundary, not inside
    // the cluster.
    let cluster_segment_count = BYPASS_PREFIX_MAX_SEGMENTS;
    let cluster_filler = b"x\r\n";
    for i in 0..cluster_segment_count {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: if i % 2 == 0 {
                cluster_rows_below
            } else {
                head_rows
            },
        });
        payload.extend_from_slice(cluster_filler);
    }
    assert_eq!(
        cluster_segment_count % 2,
        0,
        "test prerequisite: an even cluster length ends on the odd \
         index (head_rows), not the target"
    );
    let middle_len = payload.len() - head_len;
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: the cluster's OWN content must clear \
         BYPASS_PREFIX_MAX_BYTES for D8 to have anything to rescue"
    );

    // TAIL: small, just over BYPASS_SUFFIX_MIN_BYTES, back at the
    // (smaller) target — dominates the MIDDLE (D8's gate) but NOT the
    // raw head+cluster prefix (the OLD gate's dominance check).
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail history line padded out a bit for size\r\n";
    while payload.len() - head_len - middle_len < 4096 + 512 {
        payload.extend_from_slice(tail_filler);
    }
    let suffix_len = payload.len() - head_len - middle_len;
    assert!(
        suffix_len >= middle_len,
        "test prerequisite: suffix must dominate the MIDDLE (D8's gate)"
    );
    assert!(
        suffix_len < head_len + middle_len,
        "test prerequisite: suffix must NOT dominate the raw head+cluster \
         prefix (the OLD gate's dominance check must still fail here)"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let scrollback_lines = 10_000u32;
    let bypass_replay = TerminalCore::build_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("bypass-path build not cancelled");
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("reference build not cancelled");

    // AC-1: the split must engage despite the raw head+cluster prefix
    // failing every pre-D7 threshold, in the ABOVE-target direction.
    assert!(
        !bypass_replay.scrollback_populated,
        "AC-1: the split must engage (scrollback_populated == false) for \
         a resize-marker-dense tail oscillating ABOVE the settled target \
         behind an already-larger HEAD — got scrollback_populated == \
         true (D8 did not rescue the above-target direction)"
    );
    assert!(
        reference.scrollback_populated,
        "test prerequisite: the fully synchronous reference always \
         populates scrollback"
    );

    // AC-4-equivalence: viewport + cursor equivalence with the fully
    // synchronous reference, and matching evicted_total.
    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "the head/middle/tail split's viewport + cursor must match the \
         fully synchronous reference for the above-target marker-cluster \
         shape"
    );
    assert_eq!(
        bypass_replay.evicted_total, reference.evicted_total,
        "the split must preserve evicted_total byte-identically"
    );
}

/// AC-1/AC-2 (mux-tab-switch-bypass-refix task0001, D10, review finding
/// `b6a60c440da70e79`): reproduces the SPEC-cited measured bug shape's
/// TOPOLOGY — 31 total replay segments (1 HEAD + 26 MIDDLE + 4
/// stable-target TAIL), `k = 27` (`stable_target_suffix_start` lands
/// right after the MIDDLE cluster), `middle_segment_count = 26` after
/// head fold — with adjacent MIDDLE dims always differing and both
/// values sitting AT-OR-ABOVE the settled target (the D8 direction: a
/// `visible_row_count` 0→1 transition SHRINKS the grid, so pre-settling
/// dims sit at the LARGER size). Mirrors
/// `head_plus_marker_cluster_above_target_engages_the_split_and_matches_reference`
/// above, but at the SPEC's own cluster size (26) instead of the
/// generic `BYPASS_PREFIX_MAX_SEGMENTS` boundary, and with the SPEC's
/// own 31-segment/`k=27` total shape (4 TAIL segments, not 1).
///
/// Like that pre-existing test, this deterministic (non-timing) unit
/// test keeps its HEAD just over `BYPASS_PREFIX_MAX_BYTES` (64 KiB)
/// rather than the SPEC's full ~2 MiB scale — this file already gates
/// genuinely 2 MiB-scale payloads behind `#[ignore]` (e.g.
/// `measure_reparse_cost_2mib` below) to keep the default `cargo test
/// --lib` run fast. The ~2 MiB fidelity and the latency-ceiling
/// assertion live in the new release bench this task adds to
/// `bench.rs`,
/// `measured_26_segment_middle_cluster_bench_2mib_matches_bypass_engaged_cost`
/// (AC-4).
///
/// AC-1: confirmed to fail against the pre-fix gate
/// (`BYPASS_PREFIX_MAX_SEGMENTS == 24`) — `middle_segment_count` (26)
/// exceeds it, so `bypass_split` is `false` and `scrollback_populated`
/// comes back `true` (whole-drain fallback) instead of the `false` this
/// test asserts. Observed failure before this task's fix: `assertion
/// failed: !bypass_replay.scrollback_populated`.
///
/// AC-2: equivalence-checked against
/// `build_scrollback_only_from_snapshot` — viewport/cursor parity via
/// `grid_fingerprint`, matching `evicted_total`, and the discriminating
/// `scrollback_populated` signal — following the same pattern as
/// `head_plus_marker_cluster_engages_the_split_and_matches_reference`.
#[test]
fn measured_26_segment_middle_cluster_engages_the_split_and_matches_reference() {
    let cols: u16 = 80;
    let target_rows: u16 = 24;
    let head_rows: u16 = 30;

    // HEAD: a single large segment already at `head_rows` (the
    // pre-settling size) — well over `BYPASS_PREFIX_MAX_BYTES` (64 KiB)
    // on its own, matching the measured shape's HEAD-dominant payload.
    let head_filler = b"head history line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: head_rows,
    }];
    while payload.len() <= 96 * 1024 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();
    assert!(
        head_len > 64 * 1024,
        "test prerequisite: HEAD alone must exceed BYPASS_PREFIX_MAX_BYTES"
    );

    // MIDDLE: the SPEC-measured 26-segment cluster — adjacent dims
    // always differ, oscillating between `target_rows` and `head_rows`
    // (both at-or-above the settled target), ending on `head_rows` (an
    // even count, matching the above-target companion test's parity
    // note above) so the drop to `target_rows` happens at the k
    // boundary, not inside the cluster.
    const MEASURED_MIDDLE_SEGMENT_COUNT: usize = 26;
    let cluster_filler = b"x\r\n";
    for i in 0..MEASURED_MIDDLE_SEGMENT_COUNT {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: if i % 2 == 0 { target_rows } else { head_rows },
        });
        payload.extend_from_slice(cluster_filler);
    }
    assert_eq!(
        MEASURED_MIDDLE_SEGMENT_COUNT % 2,
        0,
        "test prerequisite: an even cluster length ends on the odd \
         index (head_rows), not the target"
    );
    let middle_len = payload.len() - head_len;
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: the cluster's OWN content must clear \
         BYPASS_PREFIX_MAX_BYTES for the fold to have anything to rescue"
    );

    // TAIL: 4 segments at the target dims (31 total segments, matching
    // the SPEC's own count), together well over BYPASS_SUFFIX_MIN_BYTES
    // and dominating the MIDDLE (but not the raw head+cluster prefix).
    let middle_end = payload.len();
    for _ in 0..4 {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let seg_start = payload.len();
        while payload.len() - seg_start < 1152 {
            payload.extend_from_slice(head_filler);
        }
    }
    let suffix_len = payload.len() - middle_end;
    assert_eq!(
        segments.len(),
        31,
        "test prerequisite: 1 HEAD + 26 MIDDLE + 4 TAIL = 31 total \
         segments, matching the SPEC-measured shape"
    );
    assert!(
        suffix_len >= 4096,
        "test prerequisite: suffix must clear BYPASS_SUFFIX_MIN_BYTES"
    );
    assert!(
        suffix_len >= middle_len,
        "test prerequisite: suffix must dominate the MIDDLE"
    );
    assert!(
        suffix_len < head_len + middle_len,
        "test prerequisite: suffix must NOT dominate the raw \
         head+cluster prefix (the pre-D7 gate's dominance check must \
         still fail here)"
    );

    // Test prerequisite: k must land at 27 and middle_segment_count at
    // 26 — the shape's own defining numbers, not merely "the split
    // engaged for some other reason".
    assert_eq!(
        stable_target_suffix_start(cols, target_rows, &segments),
        27,
        "test prerequisite: k must be 27, matching the SPEC-measured shape"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let scrollback_lines = 10_000u32;
    let bypass_replay = TerminalCore::build_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("bypass-path build not cancelled");
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("reference build not cancelled");

    // AC-1: the split must engage despite the raw head+cluster prefix
    // failing every pre-D7 threshold, for the SPEC-measured 26-segment
    // MIDDLE shape (31 total segments, k = 27).
    assert!(
        !bypass_replay.scrollback_populated,
        "AC-1: the split must engage (scrollback_populated == false) for \
         the SPEC-measured 26-segment MIDDLE shape (31 total segments, \
         k = 27) — got scrollback_populated == true (middle_segment_count \
         26 exceeds the gate's bound)"
    );
    assert!(
        reference.scrollback_populated,
        "test prerequisite: the fully synchronous reference always \
         populates scrollback"
    );

    // AC-2: viewport + cursor equivalence with the fully synchronous
    // reference, and matching evicted_total.
    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "AC-2: the head/middle/tail split's viewport + cursor must match \
         the fully synchronous reference for the measured 26-segment \
         MIDDLE shape"
    );
    assert_eq!(
        bypass_replay.evicted_total, reference.evicted_total,
        "AC-2: the split must preserve evicted_total byte-identically"
    );
}

/// AC-2 (task0004, review round-1 rework finding `6a02ed7e1b606588`):
/// reproduces the finding's own example — a small target HEAD, a small
/// column-change MIDDLE, and a large target TAIL — a shape that engaged
/// the split BEFORE D7 was introduced. Confirms `head_fold_safe == false`
/// (a column change is always unsafe to fold, regardless of row bounds)
/// degrades `h` to `0` rather than abandoning the split entirely: with
/// `h == 0`, `middle_len == split_at` and `middle_segment_count == k`,
/// so the pre-D7 gates (byte length, segment count, suffix dominance)
/// are evaluated on exactly the shape they always were, and the split
/// still engages.
///
/// Confirmed to fail pre-fix (task0004 D9): with `head_fold_safe` ANDed
/// directly into `bypass_split` (the pre-task0004 code), `h == 1`
/// (the small target HEAD) makes `middle_is_row_bounded` reject the
/// column-changing MIDDLE, `head_fold_safe` is `false`,
/// `bypass_split` is `false` (the WHOLE split, not just the fold, is
/// abandoned), and `scrollback_populated` comes back `true` — the full
/// non-bypass drain this fix exists to avoid for a shape that used to
/// be fast.
#[test]
fn column_change_middle_degrades_head_fold_but_still_engages_the_split() {
    let cols: u16 = 80;
    let other_cols: u16 = 100;
    let target_rows: u16 = 24;

    // HEAD: small, already at the target.
    let head_filler = b"head\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: target_rows,
    }];
    while payload.len() < 2048 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();

    // MIDDLE: a single COLUMN-CHANGING segment (same row count, but
    // different columns) — always unsafe to fold behind a bypassed
    // HEAD regardless of any row-count reasoning (see
    // `middle_is_row_bounded`'s doc).
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols: other_cols,
        rows: target_rows,
    });
    let middle_filler = b"mid\r\n";
    while payload.len() - head_len < 2048 {
        payload.extend_from_slice(middle_filler);
    }
    let middle_len = payload.len() - head_len;
    let prefix_len = head_len + middle_len;
    assert!(
        prefix_len <= 64 * 1024,
        "test prerequisite: the combined head+middle prefix must clear \
         BYPASS_PREFIX_MAX_BYTES for the pre-D7 gates to accept it"
    );

    // TAIL: back at the target, dominating the combined prefix.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail history line padded out a bit for size\r\n";
    while payload.len() - prefix_len < 8192 {
        payload.extend_from_slice(tail_filler);
    }
    let suffix_len = payload.len() - prefix_len;
    assert!(
        suffix_len >= prefix_len,
        "test prerequisite: the tail must dominate the combined prefix"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols,
        target_rows,
        10_000,
        &payload,
        &segments,
        &never,
    )
    .expect("reference build not cancelled");

    // AC-2: the split must still engage — the column change degrades
    // the HEAD fold, not the whole split.
    assert!(
        !replay.scrollback_populated,
        "AC-2: a column-change MIDDLE behind a small target HEAD, with a \
         dominating target TAIL, must still engage the split \
         (scrollback_populated == false) — an unsafe HEAD fold must \
         degrade `h` to 0, not abandon the split entirely"
    );
    assert_eq!(
        grid_fingerprint(&replay.core),
        grid_fingerprint(&reference.core),
        "the degraded (h == 0) split must match the fully synchronous \
         reference for the column-change-in-the-middle shape"
    );
    assert_eq!(
        replay.evicted_total, reference.evicted_total,
        "the split must preserve evicted_total byte-identically"
    );
}
