use super::*;

// ── task0003 AC-9 (D6, review round-2 finding `893241823258fce3`):
// a row-count-GROWING marker inside a bypass-path (>= 64 KiB) snapshot
// must produce the SAME viewport fingerprint as the synchronous path ──

/// Demonstrates the divergence this fix closes: WITHOUT the D6 pre-scan
/// (i.e. bypass stays engaged across a row-growing mid-drain resize),
/// the grown rows come up blank instead of pulling real history —
/// diverging from the synchronous reference. With the fix,
/// `build_from_snapshot` (bypass path) matches
/// `build_scrollback_only_from_snapshot` (synchronous path) exactly.
#[test]
fn build_from_snapshot_bypass_path_matches_sync_path_across_row_growing_marker() {
    let cols: u16 = 80;
    let small_rows: u16 = 10;
    let grown_rows: u16 = 40;
    let mut payload: Vec<u8> = Vec::new();
    // Constructed AT the grown size, so the FIRST transition (below) is
    // a shrink — history then accumulates in scrollback at the small
    // size (this is also where the bulk padding lives, so the payload
    // comfortably clears the 64 KiB off-thread bypass threshold), and
    // the SECOND transition grows back to the construction size (a
    // no-op for the implicit final restore). Nothing follows the grow,
    // so the fingerprint comparison below looks at the just-grown
    // viewport directly — content added AFTER the grow would scroll
    // the transient post-grow state out of view before this test ever
    // inspects it, hiding the divergence being tested for.
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: small_rows,
    }];
    // History produced at the SMALL size, comfortably more than
    // `small_rows` so there is real content sitting in scrollback for
    // the upcoming growth to pull back up into the viewport.
    for i in 0..3000u32 {
        payload.extend_from_slice(
            format!("small-size scroll line {i} padded for size\r\n").as_bytes(),
        );
    }
    // The row-count-GROWING transition under test (AC-9): the viewport
    // widens back to the construction size and should pull rows back
    // up from the scrollback history just produced above.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: grown_rows,
    });
    assert!(
        payload.len() >= 64 * 1024,
        "payload must be >= 64 KiB to match AC-9's bypass-path scenario, got {}",
        payload.len()
    );

    let never = std::sync::atomic::AtomicBool::new(false);

    // Reference: synchronous (non-bypass) path.
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols, grown_rows, 5000, &payload, &segments, &never,
    )
    .expect("reference build not cancelled");

    // Under test: bypass path (`build_from_snapshot`) — D6 downgrades
    // out of the bypass for this payload because it contains a
    // row-growing transition.
    let bypass_replay =
        TerminalCore::build_from_snapshot(cols, grown_rows, 5000, &payload, &segments, &never)
            .expect("bypass-path build not cancelled");

    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "bypass-path viewport must match the synchronous path across a row-growing transition"
    );
}

/// D5'' (task0005 rework, review round-4 finding `697d8dc2b88dcddc`): a
/// transition that changes ONLY cols (rows constant throughout,
/// including at the implicit final restore-to-target) must ALSO
/// downgrade out of the bypass, just like a row-growing transition —
/// `resize_reflow` re-wraps `scrollback_slim` + the viewport together
/// whenever EITHER dimension changes, so a cols-only resize under
/// bypass reflows against an artificially empty scrollback and can
/// diverge from the synchronous path exactly like the row-growth case
/// D6 (task0003) already closed.
///
/// Replaces `build_from_snapshot_stays_bypassed_for_a_cols_only_marker`
/// (round-4 finding: that test asserted only `scrollback_count() == 0`,
/// which is true of EVERY bypassed replay regardless of correctness and
/// therefore could never detect this divergence — unlike the other D6
/// tests, which compare `grid_fingerprint` against the synchronous
/// path). This version does that comparison.
///
/// One long autowrapping logical line with NO CR/LF at all, so it wraps
/// continuously across far more physical rows than the small viewport
/// holds — most of it must live in scrollback by the time the cols-only
/// transition runs, which is exactly the history a cols-only reflow
/// needs to re-split correctly at the new width.
///
/// Confirmed to fail pre-fix: reverting `segments_trigger_resize` to
/// only check row growth (the old `segments_has_row_growth` behavior)
/// keeps this payload's replay bypassed, and the resulting
/// `grid_fingerprint` diverges from the synchronous reference — the
/// reflow only has the last `rows` viewport lines to re-wrap (bypass
/// keeps `scrollback_slim` empty), not the full autowrapped history that
/// actually produced them.
#[test]
fn build_from_snapshot_bypass_path_matches_sync_path_across_cols_only_marker() {
    let cols_a: u16 = 80;
    let cols_b: u16 = 40;
    let rows: u16 = 10;
    let long_line: String = (0..cols_a as usize * 1000)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let mut payload: Vec<u8> = long_line.into_bytes();
    assert!(
        payload.len() >= 64 * 1024,
        "must clear the off-thread bypass-path threshold (AC-7), got {}",
        payload.len()
    );

    // Only cols changes; rows stay the same throughout, including the
    // implicit final restore (both segments name the same `rows`).
    let segments = [
        ReplaySegment {
            offset: 0,
            cols: cols_a,
            rows,
        },
        ReplaySegment {
            offset: payload.len() as u32,
            cols: cols_b,
            rows,
        },
    ];
    // A little more content after the transition so the resize is
    // actually applied (`replay_segments` only resizes a segment that
    // has content to feed).
    payload.extend_from_slice(b"tail content after the cols-only resize");

    let never = std::sync::atomic::AtomicBool::new(false);
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols_a, rows, 5000, &payload, &segments, &never,
    )
    .expect("reference build not cancelled");
    let bypass_replay =
        TerminalCore::build_from_snapshot(cols_a, rows, 5000, &payload, &segments, &never)
            .expect("bypass-path build not cancelled");

    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "a cols-only transition must downgrade out of the bypass just \
         like a row-growing one — the reflow needs real scrollback \
         history to re-wrap correctly regardless of which dimension \
         changed"
    );
}

/// AC-5 (round-6 rework D1'''; strengthened round-7, review round-6
/// finding `d139e51d1c8d03c8`): the ACTUAL prefix/suffix split —
/// `build_from_snapshot`'s "ordinary switch" shape (a HEAD segment
/// differing from the target, `MuxPane::new`'s spawn-size marker,
/// followed by a bulk tail already AT the target — the pane's real
/// history) — produces a grid/cursor identical to the fully
/// synchronous reference (`build_scrollback_only_from_snapshot`) for
/// the SAME payload, even though the split engages bypass for the
/// suffix (verified, not assumed, per the task's AC-5 mandate).
///
/// Round-7 rework: the HEAD is now longer than the viewport (spawn_rows
/// = 24) so `restore_bypass_invariant_after_reflow`'s fold-in path
/// actually runs (round-6's 5-line HEAD left `scrollback_slim` empty
/// after the prefix, so that call's `leaked == 0` early-return path was
/// the ONLY one ever exercised — the fold-in bookkeeping this split
/// depends on was never actually tested). The comparison against the
/// reference now also covers `evicted_total` and the full
/// `prompt_marks` / `fold_marks` lists (previously only
/// `grid_fingerprint`, i.e. viewport + cursor), and the payload embeds
/// OSC 133 A/B/C/D marks in BOTH the prefix and the suffix — this is
/// also the direct regression test for D4'''' (review round-6 finding
/// `0bed3c30e41e2389`): the PREFIX's B mark must have its command text
/// captured into `bypass_b_mark_texts` even though it fires before
/// `enable_snapshot_bypass` runs.
///
/// Distinguishes itself from `..._row_growing_marker` /
/// `..._cols_only_marker` above: those fixtures have NO content after
/// their last transition (the split has nothing to engage bypass for,
/// `bypass_split == false`), so they exercise the "no benefit" fallback
/// path unchanged. This fixture's tail is deliberately >=
/// `BYPASS_SUFFIX_MIN_BYTES` so the split actually activates — confirmed
/// below by asserting `scrollback_populated` differs between the two
/// paths, which is the discriminating signal that this test exercises
/// the split and not merely the pre-existing whole-drain-downgrade
/// fallback.
///
/// Confirmed to fail pre-fix (D4''''): reverting `capture_bypass_b_marks`
/// (restoring the `scrollback_bypass`-gated capture) makes the
/// `bypass_b_mark_texts` assertion for the PREFIX's B mark fail — that
/// mark fires before `enable_snapshot_bypass`, so `scrollback_bypass`
/// was still `false` at the time and the text was never captured, and
/// by the time the consumer looks, the prefix's real scrollback row
/// has already been folded into virtual bookkeeping and is
/// unrecoverable.
#[test]
fn bypass_split_matches_reference_viewport_and_cursor_for_ordinary_switch() {
    let cols: u16 = 80;
    let spawn_rows: u16 = 24;
    let target_rows: u16 = 30;

    // HEAD: OSC 133 A/B/C/D around a command, THEN enough filler lines
    // to scroll the B-marked row well past the 24-row viewport within
    // the prefix's OWN replay — long enough that
    // `restore_bypass_invariant_after_reflow` actually folds non-empty
    // real scrollback (`leaked > 0`), not the trivial `leaked == 0`
    // early return round-6's 5-line HEAD only ever exercised.
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(b"\x1b]133;A\x07$ prefix-cmd\x1b]133;B\x07");
    payload.extend_from_slice(b"\r\n\x1b]133;C\x07prefix cmd output\r\n\x1b]133;D;0\x07");
    for i in 0..40u32 {
        payload.extend_from_slice(format!("prefix filler line {i}\r\n").as_bytes());
    }
    let head_len = payload.len() as u32;

    // TAIL: OSC 133 A/B/C/D around a DIFFERENT command plus a fold
    // begin/end pair, then the pane's real history, already at the
    // target size — comfortably over `BYPASS_SUFFIX_MIN_BYTES` (4096)
    // so the split actually engages bypass for it, and large enough
    // (with a small scrollback capacity) to force real scrolling /
    // eviction, the exact mechanism the split's viewport/cursor
    // equivalence claim depends on.
    payload.extend_from_slice(b"\x1b]133;A\x07$ suffix-cmd\x1b]133;B\x07");
    payload.extend_from_slice(b"\r\n\x1b]133;C\x07suffix cmd output\r\n");
    payload.extend_from_slice(b"\x1b]777;emterm;fold;begin\x07folded suffix text\r\n");
    payload.extend_from_slice(b"\x1b]777;emterm;fold;end\x07\x1b]133;D;0\x07");
    for i in 0..500u32 {
        payload.extend_from_slice(
            format!("pane history line {i} padded out a bit for size\r\n").as_bytes(),
        );
    }
    assert!(
        (payload.len() as u32 - head_len) >= 4096,
        "test prerequisite: the tail must clear BYPASS_SUFFIX_MIN_BYTES \
         for the split to actually engage, got {}",
        payload.len() as u32 - head_len
    );

    let segments = [
        ReplaySegment {
            offset: 0,
            cols,
            rows: spawn_rows,
        },
        ReplaySegment {
            offset: head_len,
            cols,
            rows: target_rows,
        },
    ];

    let never = std::sync::atomic::AtomicBool::new(false);
    let scrollback_lines = 200u32;
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("reference build not cancelled");
    let bypass_replay = TerminalCore::build_from_snapshot(
        cols,
        target_rows,
        scrollback_lines,
        &payload,
        &segments,
        &never,
    )
    .expect("bypass-path build not cancelled");

    // Discriminate: the split must actually have engaged bypass for the
    // tail — `scrollback_populated` is `false` exactly when SOME part
    // of the replay ran under bypass (real scrollback content was
    // skipped), vs the fully synchronous reference, which is always
    // `true`. Without this check, the equivalence assertions below
    // could vacuously pass because BOTH paths took the identical
    // whole-drain, non-bypass route (the `..._marker` tests' shape) —
    // this discriminator is what actually distinguishes them.
    assert!(
        !bypass_replay.scrollback_populated,
        "the split must engage bypass for the tail (scrollback_populated \
         == false) — if this is true, the split silently fell back to \
         the whole-drain path and this test is not exercising D1'''"
    );
    assert!(
        reference.scrollback_populated,
        "test prerequisite: the fully synchronous reference always \
         populates scrollback"
    );
    // Test prerequisite (round-7): the prefix must actually have
    // produced real scrollback the fold-in path had to convert — a
    // higher `evicted_total`/mark count than a trivial payload would
    // confirm the prefix's OWN reflow really scrolled past the
    // viewport, not merely that the suffix did.
    assert!(
        reference.evicted_total > 0 || reference.prompt_marks.len() >= 4,
        "test prerequisite: fixture must exercise real scrolling / \
         mark stamping in both phases"
    );

    assert_eq!(
        grid_fingerprint(&bypass_replay.core),
        grid_fingerprint(&reference.core),
        "the prefix/suffix split's viewport + cursor must match the \
         fully synchronous reference for the ordinary-switch shape"
    );
    // AC-5: the split must not silently corrupt eviction accounting or
    // the prompt/fold mark lists it hands the caller for
    // `backfill_prompt_marks` / `backfill_fold_marks` — round-6's
    // 5-line HEAD never exercised `restore_bypass_invariant_after_reflow`'s
    // non-trivial fold-in path, so a regression there could have passed
    // silently.
    assert_eq!(
        bypass_replay.evicted_total, reference.evicted_total,
        "the split must preserve evicted_total byte-identically"
    );
    assert_eq!(
        bypass_replay.prompt_marks, reference.prompt_marks,
        "the split must preserve prompt_marks byte-identically \
         (kind, abs_row, evicted_total, exit_code)"
    );
    assert_eq!(
        bypass_replay.fold_marks, reference.fold_marks,
        "the split must preserve fold_marks byte-identically"
    );

    // D4'''' regression: both the PREFIX's and the SUFFIX's B mark
    // command text must be captured into `bypass_b_mark_texts` — the
    // prefix's fires before `enable_snapshot_bypass` runs, so gating
    // capture on `scrollback_bypass` alone (round-6) missed it.
    let b_marks: Vec<_> = bypass_replay
        .prompt_marks
        .iter()
        .filter(|m| m.kind == b'B')
        .collect();
    assert_eq!(
        b_marks.len(),
        2,
        "test prerequisite: exactly one B mark in the prefix and one \
         in the suffix, got {b_marks:?}"
    );
    let prefix_b_abs_row = b_marks[0].abs_row;
    let suffix_b_abs_row = b_marks[1].abs_row;
    assert!(
        bypass_replay
            .bypass_b_mark_texts
            .get(&prefix_b_abs_row)
            .is_some_and(|text| text.contains("prefix-cmd")),
        "the PREFIX's B mark command text must be captured into \
         bypass_b_mark_texts even though it fires before bypass is \
         enabled — got {:?}",
        bypass_replay.bypass_b_mark_texts.get(&prefix_b_abs_row)
    );
    assert!(
        bypass_replay
            .bypass_b_mark_texts
            .get(&suffix_b_abs_row)
            .is_some_and(|text| text.contains("suffix-cmd")),
        "the SUFFIX's B mark command text must be captured into \
         bypass_b_mark_texts — got {:?}",
        bypass_replay.bypass_b_mark_texts.get(&suffix_b_abs_row)
    );
}

/// D5''''' (round-8 rework, review round-7 findings `7c70216c5a5d5c24`
/// / `a4f4e36fef377d05`): a deterministic, non-ignored regression
/// pinning the split gate's boundary — the ONLY prior assertion of
/// this shape lived in an `#[ignore]`d timing bench
/// (`bench.rs::large_prefix_small_suffix_bench_does_not_engage_the_split`),
/// so deleting the gate left the normal `cargo test` run green.
///
/// Pins the side of the boundary where the split must NOT engage: a
/// prefix JUST OVER `BYPASS_PREFIX_MAX_BYTES` (64 KiB), even paired
/// with a suffix that clears (and dominates) `BYPASS_SUFFIX_MIN_BYTES`.
/// The companion test below pins the other side (prefix AT/under the
/// byte bound, with a dominating suffix, DOES engage).
///
/// Confirmed to fail pre-fix: deleting the `split_at <=
/// BYPASS_PREFIX_MAX_BYTES` gate entirely makes this payload's split
/// engage (since the suffix here easily clears `BYPASS_SUFFIX_MIN_BYTES`
/// and dominates the prefix), so `scrollback_populated` comes back
/// `false` and the assertion below fails.
#[test]
fn prefix_just_over_the_byte_bound_does_not_engage_the_split() {
    let cols: u16 = 100;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    let filler = b"prefix line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: other_rows,
    }];
    while payload.len() <= 64 * 1024 {
        payload.extend_from_slice(filler);
    }
    let prefix_len = payload.len();
    assert!(
        prefix_len > 64 * 1024,
        "test prerequisite: prefix must exceed BYPASS_PREFIX_MAX_BYTES"
    );

    // Suffix: at the target dims, large enough to DOMINATE the prefix
    // (bigger than it) — isolates the BYTE-BOUND gate as the ONLY
    // reason this must not split (the "suffix dominates" and
    // "segment count" gates are satisfied here).
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    while payload.len() - prefix_len <= prefix_len {
        payload.extend_from_slice(filler);
    }

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        replay.scrollback_populated,
        "a prefix just over BYPASS_PREFIX_MAX_BYTES must not engage the \
         split, even with a dominating suffix — scrollback_populated \
         must be true (whole-drain fallback), not false"
    );
}

/// D5''''' companion (see the test above): a prefix AT/under
/// `BYPASS_PREFIX_MAX_BYTES`, with a suffix that DOMINATES it (at least
/// as large) and a segment count within `BYPASS_PREFIX_MAX_SEGMENTS`,
/// DOES engage the split — pins the other side of the boundary so a
/// future change cannot silently turn the gate into "never split".
#[test]
fn prefix_at_the_byte_bound_with_a_dominating_suffix_engages_the_split() {
    let cols: u16 = 100;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    let filler = b"prefix line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: other_rows,
    }];
    // A small prefix, comfortably under the byte bound and with a
    // single segment (comfortably under BYPASS_PREFIX_MAX_SEGMENTS).
    while payload.len() < 8192 {
        payload.extend_from_slice(filler);
    }
    let prefix_len = payload.len();
    assert!(prefix_len <= 64 * 1024, "test prerequisite");

    // Suffix: at the target dims, larger than the prefix (dominates)
    // and clears BYPASS_SUFFIX_MIN_BYTES.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    while payload.len() - prefix_len < prefix_len.max(4096) * 2 {
        payload.extend_from_slice(filler);
    }

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        !replay.scrollback_populated,
        "a small prefix with a dominating suffix (both within the byte \
         and segment-count bounds) must engage the split — \
         scrollback_populated must be false"
    );
}

/// D5''''' (round-8 rework, review round-7 finding `a4f4e36fef377d05`):
/// the EXACT repro the finding names — a prefix AT the byte bound (64
/// KiB) with only a small suffix JUST over `BYPASS_SUFFIX_MIN_BYTES`
/// (4096, ~16x smaller than the prefix) must NOT engage the split. The
/// byte-only gate alone (`split_at <= BYPASS_PREFIX_MAX_BYTES` AND
/// `suffix_len >= BYPASS_SUFFIX_MIN_BYTES`) is satisfied by this
/// payload — only the NEW "suffix must dominate" requirement
/// (`suffix_len >= split_at`) rejects it.
///
/// Confirmed to fail pre-fix: reverting to the byte-only gate (dropping
/// `suffix_len >= split_at`) makes this payload's split engage —
/// `scrollback_populated` comes back `false` and the assertion below
/// fails.
#[test]
fn prefix_at_byte_bound_with_non_dominating_suffix_does_not_engage_the_split() {
    let cols: u16 = 100;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    let filler = b"prefix line padded out a bit for size\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: other_rows,
    }];
    // Prefix: right at BYPASS_PREFIX_MAX_BYTES (64 KiB) — never add a
    // chunk that would push it OVER the bound.
    while payload.len() + filler.len() <= 64 * 1024 {
        payload.extend_from_slice(filler);
    }
    let prefix_len = payload.len();
    assert!(
        prefix_len <= 64 * 1024,
        "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES"
    );

    // Suffix: just over BYPASS_SUFFIX_MIN_BYTES (4096) — clears the
    // absolute floor but is dwarfed by the prefix (does NOT dominate).
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: target_rows,
    });
    while payload.len() - prefix_len < 4096 + 512 {
        payload.extend_from_slice(filler);
    }
    let suffix_len = payload.len() - prefix_len;
    assert!(
        suffix_len >= 4096,
        "test prerequisite: suffix must clear BYPASS_SUFFIX_MIN_BYTES"
    );
    assert!(
        suffix_len < prefix_len,
        "test prerequisite: suffix must NOT dominate the prefix"
    );

    let never = std::sync::atomic::AtomicBool::new(false);
    let replay =
        TerminalCore::build_from_snapshot(cols, target_rows, 10_000, &payload, &segments, &never)
            .expect("not cancelled");
    assert!(
        replay.scrollback_populated,
        "a prefix at the byte bound with a small, non-dominating \
         suffix must not engage the split — scrollback_populated must \
         be true (whole-drain fallback), not false"
    );
}

/// D5''''' (round-8 rework, review round-7 finding `a4f4e36fef377d05`):
/// the segment-count bound — a prefix with MORE than the operative
/// bound's worth of segments must not engage the split, even when its
/// byte length is tiny and its suffix dominates (both of which are
/// otherwise sufficient).
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72`): this shape's rows alternate without ever
/// matching `target_rows` from the front, so — exactly like its
/// companion `prefix_at_the_segment_count_bound_with_a_dominating_suffix_engages_the_split_no_head`
/// below — `h` degrades to `0`, so the OPERATIVE bound is
/// [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`] (the `h == 0` tier),
/// not [`BYPASS_PREFIX_MAX_SEGMENTS`]. Retargeted accordingly (the
/// assertion held either way before D11 split the bound in two, since
/// `segment_count` here already exceeded both).
///
/// Confirmed to fail pre-fix: dropping the segment-count gate entirely
/// makes this payload's split engage — `scrollback_populated` comes
/// back `false` and the assertion below fails.
#[test]
fn prefix_with_too_many_segments_does_not_engage_the_split_regardless_of_byte_length() {
    let cols: u16 = 100;
    let target_rows: u16 = 30;
    let other_rows: u16 = 24;

    let segment_count = BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD + 1;
    let filler = b"tiny\r\n";
    let mut payload: Vec<u8> = Vec::new();
    let mut segments = Vec::with_capacity(segment_count + 1);
    for i in 0..segment_count {
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
        "test prerequisite: prefix must clear BYPASS_PREFIX_MAX_BYTES \
         despite the excess segment count"
    );

    // Suffix: large enough to dominate the (tiny) prefix and clear
    // BYPASS_SUFFIX_MIN_BYTES.
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
        replay.scrollback_populated,
        "a prefix with more than BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD \
         segments (h == 0 tier) must not engage the split, even with a \
         tiny byte length and a dominating suffix — scrollback_populated \
         must be true, not false"
    );
}
