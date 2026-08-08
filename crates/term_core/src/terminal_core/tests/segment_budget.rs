use super::*;

// ── task0004 D11: the h == 0 tier's own segment-count boundary
// (review round-1 rework, findings `a1a06ed541045dd5` /
// `77da6aceb73b1a72` / `474e01ad8c29e7f0` / `96f7205be52fece8` /
// `1adb07864f11618f`) ────────────────────────────────────────────────

/// AC-1 (task0004, D11, findings `a1a06ed541045dd5` / `77da6aceb73b1a72`):
/// reproduces Defect A's own worst-case shape directly — a MIDDLE built
/// ENTIRELY from column-changing segments (every one differs from the
/// caller's target columns, so `leading_uniform_run_len` returns `h ==
/// 0` immediately: there is no leading run at `target_cols` to even
/// attempt folding), at EXACTLY `BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`
/// segments, behind a dominating target-dims suffix. Confirms the
/// tightened `h == 0` tier still ENGAGES at its own bound (the "at
/// bound" side of D11's boundary — companion to the "one past rejects"
/// side below).
///
/// This is the shape `a1a06ed541045dd5` / `77da6aceb73b1a72` found
/// unguarded pre-D11: every one of these segments pays
/// `TerminalCore::resize_full_reflow` (column change), not the
/// row-delta-bounded `resize_same_width` the pre-D11 doc's "same-width
/// by construction" claim assumed applied everywhere.
#[test]
fn h_zero_column_changing_middle_at_the_unfolded_bound_engages_the_split() {
    let cols: u16 = 80;
    let other_cols: u16 = 100;
    let target_rows: u16 = 24;

    // MIDDLE: EXACTLY BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD segments,
    // every one COLUMN-CHANGING relative to the caller's target — none
    // opens at `target_cols`, so `leading_uniform_run_len` returns `h ==
    // 0` on the first segment, before any row-based reasoning even
    // applies.
    let filler = b"x\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = Vec::with_capacity(BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD + 1);
    for _ in 0..BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols: other_cols,
            rows: target_rows,
        });
        payload.extend_from_slice(filler);
    }
    let middle_len = payload.len();
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: MIDDLE must clear BYPASS_PREFIX_MAX_BYTES"
    );

    // SUFFIX: back at the target, dominating the MIDDLE and clearing
    // BYPASS_SUFFIX_MIN_BYTES.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let suffix_filler = b"suffix line padded out a bit for size\r\n";
    while payload.len() - middle_len < 8192 {
        payload.extend_from_slice(suffix_filler);
    }
    let suffix_len = payload.len() - middle_len;
    assert!(
        suffix_len >= middle_len,
        "test prerequisite: suffix must dominate the MIDDLE"
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

    assert!(
        !replay.scrollback_populated,
        "AC-1: a MIDDLE with EXACTLY BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD \
         column-changing segments (h == 0 tier), behind a dominating \
         target-dims suffix, must still engage the split — \
         scrollback_populated must be false, not true"
    );
    assert_eq!(
        grid_fingerprint(&replay.core),
        grid_fingerprint(&reference.core),
        "the h == 0 split must match the fully synchronous reference \
         at the column-changing MIDDLE's own segment-count bound"
    );
    assert_eq!(
        replay.evicted_total, reference.evicted_total,
        "the split must preserve evicted_total byte-identically"
    );
}

/// AC-1 / AC-3 (task0004, D11, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72`): the "one past" side of the boundary above — a
/// MIDDLE built from `BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD + 1`
/// column-changing segments (still comfortably under the OLD, pre-D11
/// uniform bound of 62) must NOT engage the split.
///
/// Confirmed to fail pre-fix (D11): before the `h == 0` tier existed,
/// this shape's `middle_segment_count` (25) was checked against the
/// single `BYPASS_PREFIX_MAX_SEGMENTS` bound (62) — comfortably under
/// it — so the split engaged, exactly the unguarded worst case
/// `a1a06ed541045dd5` / `77da6aceb73b1a72` describe (this implementer
/// confirmed the pre-fix code path passes `middle_segment_count <= 62`
/// for this shape, engaging the split, before applying D11's tiering).
#[test]
fn h_zero_column_changing_middle_one_past_the_unfolded_bound_does_not_engage_the_split() {
    let cols: u16 = 80;
    let other_cols: u16 = 100;
    let target_rows: u16 = 24;

    let segment_count = BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD + 1;
    let filler = b"x\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = Vec::with_capacity(segment_count + 1);
    for _ in 0..segment_count {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols: other_cols,
            rows: target_rows,
        });
        payload.extend_from_slice(filler);
    }
    let middle_len = payload.len();
    assert!(
        middle_len <= 64 * 1024,
        "test prerequisite: MIDDLE must clear BYPASS_PREFIX_MAX_BYTES \
         despite the excess segment count"
    );

    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let suffix_filler = b"suffix line padded out a bit for size\r\n";
    while payload.len() - middle_len < 8192 {
        payload.extend_from_slice(suffix_filler);
    }

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        replay.scrollback_populated,
        "AC-1/AC-3: a MIDDLE with BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD \
         + 1 column-changing segments (h == 0 tier), one past its own \
         bound, must NOT engage the split even with a dominating \
         suffix — scrollback_populated must be true, not false"
    );
}

/// AC-5 (task0001, D7): confirms the pre-existing "ordinary" (no HEAD)
/// segment-count boundary behavior is unchanged by D7 — a prefix with
/// EXACTLY the operative bound's worth of segments, small in bytes,
/// paired with a dominating suffix, still engages the split (companion
/// to the pre-existing
/// `prefix_with_too_many_segments_does_not_engage_the_split_regardless_of_byte_length`,
/// which pins the "one past the bound rejects" side of the same
/// boundary). Both tests reference the bound symbolically, so
/// AC-3/D10 (mux-tab-switch-bypass-refix task0001) retargeted them to
/// the new bound (62, was 24) without editing either test body.
/// Distinguishes itself from
/// `head_plus_marker_cluster_engages_the_split_and_matches_reference`
/// above: `h == 0` here (the first segment does not open at the
/// target), so `middle_segment_count == k` exactly — this is the direct
/// regression guard that D7's `h` / `middle_segment_count` computation
/// reduces to the pre-D7 `k` byte-for-byte when there is no HEAD to
/// rescue, not merely "close" — i.e. D7 does not accidentally widen
/// acceptance beyond what AC-1's specific marker-cluster case requires.
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72`): `h == 0` here (as this doc already noted) means
/// the OPERATIVE bound is [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`]
/// (the `h == 0` tier), not [`BYPASS_PREFIX_MAX_SEGMENTS`] — retargeted
/// accordingly (AC-3: "boundary tests move with whatever bound is
/// operative").
#[test]
fn prefix_at_the_segment_count_bound_with_a_dominating_suffix_engages_the_split_no_head() {
    let cols: u16 = 100;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    let filler = b"tiny\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = Vec::with_capacity(BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD + 1);
    for i in 0..BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD {
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: if i % 2 == 0 {
                other_rows
            } else {
                other_rows + 1
            },
        });
        payload.extend_from_slice(filler);
    }
    let prefix_len = payload.len();
    assert!(
        prefix_len <= 64 * 1024,
        "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES"
    );

    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let suffix_filler = b"suffix line padded out a bit for size\r\n";
    while payload.len() - prefix_len < 8192 {
        payload.extend_from_slice(suffix_filler);
    }

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        !replay.scrollback_populated,
        "a prefix with EXACTLY BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD \
         segments (no HEAD, h == 0 tier), a tiny byte length, and a \
         dominating suffix must still engage the split — \
         scrollback_populated must be false, not true (D7 must not \
         accidentally narrow the pre-existing boundary)"
    );
}

/// AC-6 (task0001, NFR1 regression guard, D7): a genuinely large,
/// content-heavy MIDDLE (not a resize-marker cluster — real scrollback
/// content, a single segment) behind an already-at-target HEAD must
/// still NOT engage the split, even though a HEAD is present and D7's
/// safety condition (`middle_is_row_bounded`) holds for it. Proves D7's
/// head/middle generalization does not widen acceptance for the shape
/// `BYPASS_PREFIX_MAX_BYTES` exists to reject (see that constant's doc
/// history) merely because a small HEAD happens to precede it — only
/// the MIDDLE's own size decides, exactly as D2 (IMPLEMENTATION.md)
/// requires.
///
/// Confirmed to fail if D7 folded the HEAD in without ALSO re-checking
/// the MIDDLE's own size against `BYPASS_PREFIX_MAX_BYTES` (i.e. if it
/// only subtracted the head's byte length from the raw `split_at`
/// without gating `middle_len` itself): this ~96 KiB of real content
/// would otherwise engage the split behind the small head, paying the
/// 2nd-pass worker's full non-bypass cost a second time (NFR1).
#[test]
fn head_plus_large_content_heavy_middle_does_not_engage_the_split() {
    let cols: u16 = 80;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    // HEAD: small, already at the target.
    let head_filler = b"head line\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: target_rows,
    }];
    while payload.len() < 512 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();

    // MIDDLE: a SINGLE segment (well under BYPASS_PREFIX_MAX_SEGMENTS)
    // but genuinely large in bytes — real scrollback content, not a
    // sparse marker cluster.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: other_rows,
    });
    let middle_filler = b"real scrollback content line padded a bit\r\n";
    while payload.len() - head_len <= 96 * 1024 {
        payload.extend_from_slice(middle_filler);
    }
    let middle_len = payload.len() - head_len;
    assert!(
        middle_len > 64 * 1024,
        "test prerequisite: the MIDDLE alone must exceed \
         BYPASS_PREFIX_MAX_BYTES"
    );

    // TAIL: dominates the MIDDLE, back at the target.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail line padded out a bit for size\r\n";
    let tail_start = payload.len();
    while payload.len() - tail_start < middle_len + 4096 {
        payload.extend_from_slice(tail_filler);
    }

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        replay.scrollback_populated,
        "AC-6: a genuinely large, content-heavy MIDDLE behind a HEAD \
         must NOT engage the split, even though the HEAD itself is \
         safe to fold — scrollback_populated must be true (whole-drain \
         fallback), not false"
    );
}

/// Regression pin (task0003, prior-feature review round-2 critical
/// `5c6ae6b507b6f638`): the empty-MIDDLE degradation contract
/// (IMPLEMENTATION.md `Empty-MIDDLE degradation contract`) for the
/// `h == k` shape — the ENTIRE pre-suffix region is a single uniform
/// run at `(target_cols, R)` with `R` strictly above the caller's
/// target `rows`, so `leading_uniform_run_len` reports a candidate `h`
/// covering the whole region (`candidate_h == k`). Folding that
/// candidate in would leave an empty MIDDLE — `replay_segments` early-
/// returns for empty `segments` WITHOUT its final "resize back to the
/// caller's target" hop — so `candidate_h < k` must reject the fold and
/// degrade `h` to `0` (the pre-D7 whole-prefix path). The region is
/// sized well over `BYPASS_PREFIX_MAX_BYTES` (64 KiB) so that, once
/// degraded, it also fails the ordinary (no-HEAD) split gates on its
/// own merits and falls all the way back to the fully synchronous
/// whole-drain replay — the same replay the reference build performs.
///
/// Confirmed to fail pre-fix (before the `candidate_h < k` guard
/// existed): with the guard removed, `candidate_safe` accepts `h == k`
/// here (`candidate_h > 0`, `candidate_rows(R=30) >= rows(24)`, and
/// `middle_is_row_bounded` vacuously holds over the empty
/// `segments[candidate_h..k]` slice) regardless of the region's own
/// byte length — folding an empty MIDDLE in skips the
/// `BYPASS_PREFIX_MAX_BYTES` check entirely (it is evaluated against
/// `middle_len`, which is `0` for `h == k`). The HEAD then replays
/// under bypass at `head_rows == 30` and the core is never resized
/// back down — the round-2 finding's own empirically-confirmed
/// failure: requested `(80, 24)`, got `(80, 30)`, with
/// `scrollback_populated` coming back `false` (the split wrongly
/// reports itself engaged) instead of matching the reference build's
/// `true`. This test built that exact shape against the pre-fix guard
/// (locally, not committed) and observed precisely that divergence
/// before confirming the guard below prevents it.
#[test]
fn whole_prefix_uniform_head_run_degrades_empty_middle_fold_and_matches_reference() {
    let cols: u16 = 80;
    let target_rows: u16 = 24;
    let head_run_rows: u16 = 30; // R, strictly above target_rows

    // Pre-suffix region: a SINGLE segment (k == 1), uniform at
    // (cols, head_run_rows) — trivially a "single uniform run" whose
    // leading_uniform_run_len candidate covers the whole region
    // (candidate_h == k == 1). Sized well over BYPASS_PREFIX_MAX_BYTES
    // (64 KiB) so the degraded (h == 0) path's own gate rejects it too.
    let head_filler = b"head history line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: head_run_rows,
    }];
    while payload.len() <= 96 * 1024 {
        payload.extend_from_slice(head_filler);
    }
    let head_len = payload.len();
    assert!(
        head_len > 64 * 1024,
        "test prerequisite: the pre-suffix region must exceed \
         BYPASS_PREFIX_MAX_BYTES so the degraded path also rejects it"
    );

    // Qualifying stable target-dims suffix, just over
    // BYPASS_SUFFIX_MIN_BYTES.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    let tail_filler = b"tail history line padded out a bit for size\r\n";
    while payload.len() - head_len < 4096 + 512 {
        payload.extend_from_slice(tail_filler);
    }

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

    // AC-1: the built core must land at the CALLER-requested (cols,
    // rows) — not at the HEAD run's R.
    assert_eq!(
        bypass_replay.core.cols(),
        cols,
        "AC-1: the degraded empty-MIDDLE fold must still resize back to \
         the caller's target column count"
    );
    assert_eq!(
        bypass_replay.core.rows(),
        target_rows,
        "AC-1: the degraded empty-MIDDLE fold must still resize back to \
         the caller's target row count, not stay at the HEAD run's R \
         (the round-2 finding's failure: requested (80, 24), got \
         (80, 30))"
    );

    // AC-2: scrollback_populated must match the reference non-bypass
    // build of the identical payload/segments.
    assert_eq!(
        bypass_replay.scrollback_populated, reference.scrollback_populated,
        "AC-2: scrollback_populated must match the reference build's \
         value for this shape"
    );
    assert!(
        reference.scrollback_populated,
        "test prerequisite: the fully synchronous reference always \
         populates scrollback"
    );
}
