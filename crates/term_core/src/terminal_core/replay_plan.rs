//! Structural resize-segment replay planning (task0004 D1'):
//! pure helpers that classify a snapshot's segment list
//! (stable-suffix / uniform-head / row-bounded-middle) and the
//! BYPASS_* budget constants that bound the bypass prefix.

use super::{ReplaySegment, clamp_resize_dims};

// ── Structural resize-segment replay (task0004 round-4 rework, D1') ────
//
// Rounds 1-3 carried dimension changes as an in-band `OSC 777;emterm;resize;
// <cols>;<rows> BEL` byte marker (IMPLEMENTATION.md D1/D2, task0001),
// discovered at replay time by scanning the payload for that exact byte
// pattern. Every round's residual critical/high findings trace back to this
// choice: a marker embedded in the byte stream is, definitionally, also
// forgeable BY the byte stream — three attempts at filtering it out of
// PTY-sourced content each left a reconstruction path (splitting across
// filter batches, nesting inside a non-SIXEL DCS, concatenation after a
// strip pass, …).
//
// D1' removes the byte-scanning decoder entirely: dimensions are now
// supplied to replay as a structural [`ReplaySegment`] parameter (see
// [`TerminalCore::reset_and_replay_segments`] / [`TerminalCore::build_from_snapshot`]).
// No function in this module scans a byte buffer for a marker pattern any
// more, so there is nothing left for PTY output to forge.

/// D1''' (round-6 rework, superseding task0003 D6 / task0005 D5''
/// `segments_trigger_resize`): the index of the first segment in
/// `segments` such that IT and every segment after it already carries
/// `(target_cols, target_rows)` — i.e. the start of the trailing run that
/// needs no further resize once reached. Returns `segments.len()` when no
/// such run exists (including when `segments` is empty, which trivially
/// returns `0` — the whole, empty, list is "already stable" — see the
/// call site for how that degenerates to the pre-round-6 no-transition
/// fast path).
///
/// Used by [`TerminalCore::build_from_snapshot_inner`](super::TerminalCore::build_from_snapshot_inner) to split a replay
/// into a (possibly empty) non-bypass PREFIX — up to and including the
/// resize that reaches the target — and a bypass-eligible SUFFIX that, by
/// this function's own definition, contains no further resize. `k == 0`
/// is exactly the case the retired `segments_trigger_resize` reported as
/// `false` (no transition anywhere in the replay): every segment already
/// opens at the target, so the whole thing is "suffix".
///
/// `clamp_resize_dims` is applied per segment here so this predicate
/// agrees with what [`TerminalCore::replay_segments`](super::TerminalCore::replay_segments) will actually decide
/// (it clamps at the same point, D1''): an out-of-domain wire dimension
/// cannot make this predicate see a "change" that replay itself would
/// clamp away to a no-op, or vice versa.
pub(in crate::terminal_core) fn stable_target_suffix_start(
    target_cols: u16,
    target_rows: u16,
    segments: &[ReplaySegment],
) -> usize {
    let target = (target_cols, target_rows);
    let mut k = segments.len();
    while k > 0 && clamp_resize_dims(segments[k - 1].cols, segments[k - 1].rows) == target {
        k -= 1;
    }
    k
}

/// D7 (task0001, NFR1-safe rescue for a resize-marker-dense tail): the size
/// of the LEADING run of `segments` that already carries a UNIFORM `(cols,
/// rows)` — the front-end complement of [`stable_target_suffix_start`]
/// (which finds the analogous TRAILING run, always uniform at the CALLER's
/// target). Returns `(h, run_rows)`: `h` segments long, all at
/// `(target_cols, run_rows)`.
///
/// D8 (task0004, review round-1 rework, finding `b21749c5f2bd1006`): unlike
/// [`stable_target_suffix_start`], the run this looks for does NOT have to
/// be at the CALLER's `target_rows` — only at `target_cols` (a column
/// change anywhere is always unsafe to fold, so a HEAD whose own columns
/// differ from the caller's target can never help; see
/// `middle_is_row_bounded`'s doc). `run_rows` is whatever row count the run
/// itself settles on, taken from `segments[0]`. This is what makes a HEAD
/// that predates a resize storm — and so sits at the storm's LARGER,
/// pre-settling size, not the storm's smaller settled target — foldable:
/// `run_rows` becomes `middle_is_row_bounded`'s safety ceiling instead of
/// the caller's `target_rows`. When the HEAD happens to already be at the
/// caller's target (`run_rows == target_rows`, every pre-D8 shape), this
/// reduces to the original `leading_target_run_len` byte-for-byte.
///
/// `build_from_snapshot_inner` calls this only on `segments[..k]` (the
/// region `stable_target_suffix_start` calls "prefix"): a large, already-
/// uniform HEAD at the very front of that region would otherwise be swept
/// into an expensive non-bypass whole-drain replay merely because SOME
/// segment further along (still before the stable tail at `k`) diverges
/// from it — the exact shape a resize-marker cluster near (but not quite
/// at) the tail produces. Returns `(0, target_cols's caller-supplied
/// target_rows)`-shaped `(0, _)` when `segments` is empty or its first
/// entry's columns differ from `target_cols` — correctly reducing to
/// "nothing to rescue" for every shape with no uniform leading run at all.
///
/// `clamp_resize_dims` is applied per segment for the same reason
/// [`stable_target_suffix_start`] applies it: agreement with what
/// `TerminalCore::replay_segments` will actually decide.
pub(in crate::terminal_core) fn leading_uniform_run_len(
    target_cols: u16,
    segments: &[ReplaySegment],
) -> (usize, u16) {
    let Some(first) = segments.first() else {
        return (0, 0);
    };
    let (first_cols, first_rows) = clamp_resize_dims(first.cols, first.rows);
    if first_cols != target_cols {
        return (0, 0);
    }
    let run = (first_cols, first_rows);
    let mut h = 0;
    while h < segments.len() && clamp_resize_dims(segments[h].cols, segments[h].rows) == run {
        h += 1;
    }
    (h, first_rows)
}

/// D7 safety gate: is it correct to replay [`leading_uniform_run_len`]'s
/// HEAD under bypass ahead of `middle` (the genuinely resize-needing
/// region between the head and the stable tail)? `head_rows` is the HEAD's
/// own row count — [`leading_uniform_run_len`]'s `run_rows` — NOT
/// necessarily the caller's `target_rows` (D8, task0004).
///
/// The HEAD leaves the core at `(target_cols, head_rows)` with
/// `scrollback_slim` EMPTY — bypass discards its real row content, keeping
/// only a virtual count (see `TerminalCore::scrollback_bypass`). A
/// subsequent resize can only produce a WRONG result (relative to a full,
/// non-bypass replay) if it needs to READ that discarded content, which
/// happens in exactly two cases:
///
/// - A COLUMN change: `resize_full_reflow` re-wraps EVERY row currently
///   tracked (viewport + real scrollback) to the new width — the head's
///   rows are not among what's tracked, so a column change anywhere in
///   `middle` is unconditionally rejected here (mirrors D6's treatment of
///   column changes as always needing real history).
/// - A ROW-COUNT GROW past what `middle` has itself already pushed into
///   REAL scrollback since it started: `resize_same_width`'s grow branch
///   pulls the most recently evicted rows back via
///   `scrollback_slim.pop_back()`. Since `middle` starts at EXACTLY
///   `head_rows` (inherited from the head) and this gate requires every
///   segment's (clamped) row count to stay `<= head_rows`, any grow
///   within `middle` is, by induction, recovering rows a PRIOR shrink
///   WITHIN THE SAME `middle` region already pushed there — it can never
///   reach past `middle`'s own start for the head's (discarded) rows.
///
/// Returns `false` (unsafe to fold the head in) the moment either condition
/// is violated by any segment in `middle`.
pub(in crate::terminal_core) fn middle_is_row_bounded(
    target_cols: u16,
    head_rows: u16,
    middle: &[ReplaySegment],
) -> bool {
    middle.iter().all(|s| {
        let (c, r) = clamp_resize_dims(s.cols, s.rows);
        c == target_cols && r <= head_rows
    })
}

/// Minimum suffix size (bytes), per [`stable_target_suffix_start`], below
/// which `build_from_snapshot_inner`'s D1''' prefix/suffix split is not
/// worth its own overhead (an extra `replay_segments` call plus an
/// `enable_snapshot_bypass`/`disable_snapshot_bypass` round trip) — small
/// tails (a handful of post-resize lines, as several `..._marker`
/// regression fixtures construct) fall back to the whole-drain recipe
/// unchanged, keeping those fixtures byte-identical to the pre-round-6
/// behavior. A real "ordinary switch" suffix (the pane's actual history)
/// is orders of magnitude larger than this, so the gate never affects the
/// case NFR1 targets.
pub(in crate::terminal_core) const BYPASS_SUFFIX_MIN_BYTES: usize = 4096;

/// Maximum prefix size (bytes) for which `build_from_snapshot_inner`'s
/// D1''' prefix/suffix split is worth engaging (D5'''', round-7 rework,
/// review round-6 finding `e519916efd5fdc42`).
///
/// [`BYPASS_SUFFIX_MIN_BYTES`] alone gates on the SUFFIX being big enough
/// to be worth bypassing, but says nothing about the PREFIX's own cost:
/// the prefix always replays via the full, non-bypass
/// `replay_segments` — correct, but exactly as expensive per byte as the
/// whole-drain fallback this split exists to avoid. A payload that is
/// almost entirely prefix with only a small qualifying suffix (a large
/// multi-segment retained window with resizes scattered through most of
/// it, followed by a stable tail just over `BYPASS_SUFFIX_MIN_BYTES`)
/// would otherwise still engage the split: pay the full non-bypass cost
/// for the (huge) prefix as its "fast" first pass, discard that prefix's
/// real scrollback into virtual bookkeeping
/// (`restore_bypass_invariant_after_reflow`), report
/// `scrollback_populated: false`, and then pay THAT SAME non-bypass cost
/// a SECOND time when the background 2nd-pass worker
/// (`tabs.rs::apply_offthread_swap`) redoes the whole drain to actually
/// populate real scrollback — roughly doubling the work for a shape the
/// split cannot actually speed up (the ordinary-switch shape it targets
/// has a TINY prefix by construction). Below this bound, the prefix's own
/// cost is negligible either way (mirrors `tabs::OFFTHREAD_REPLAY_THRESHOLD_BYTES`'s
/// reasoning: 64 KiB is small enough that even a full non-bypass reflow
/// of it does not matter), so the split still engages whenever the
/// suffix qualifies.
///
/// D7 amendment (task0001): this bound is now checked against `middle_len`
/// (the byte span of `segments[h..k]`, per [`leading_uniform_run_len`]), not
/// the raw `split_at`/`k`-derived prefix span — when there is no rescuable
/// HEAD (`h == 0`, every pre-D7 shape), `middle_len == split_at` and this is
/// byte-identical to the original check.
///
/// D11 cross-reference (task0004, review round-1 rework, findings
/// `a1a06ed541045dd5` / `77da6aceb73b1a72`): this bound and
/// [`BYPASS_PREFIX_MAX_SEGMENTS`] / [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`]
/// form ONE cost budget for the MIDDLE, not two independent ones — the
/// segment bounds exist because a MIDDLE built from many small segments
/// still pays one reflow per segment regardless of how little of this
/// byte budget it uses (see their own docs), so raising this byte bound
/// changes what a single reflow at the segment bounds' worst case costs.
/// Re-measure both together (a release bench in `bench.rs`) before
/// changing either.
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_BYTES: usize = 64 * 1024;

/// Maximum number of segments the MIDDLE (`segments[h..k]`, per
/// [`leading_uniform_run_len`]) may contain for
/// `build_from_snapshot_inner`'s D1''' prefix/suffix split to be worth
/// engaging (D5''''', round-8 rework, review round-7 finding
/// `a4f4e36fef377d05`).
///
/// [`BYPASS_PREFIX_MAX_BYTES`] bounds the prefix's total BYTE length, but a
/// prefix built from many small segments still pays one full,
/// content-preserving reflow PER SEGMENT (`replay_segments`'s per-segment
/// resize), regardless of how few total bytes those segments cover — a
/// resize storm packs up to the daemon's own `MAX_DIM_MARKERS` (62, kept as
/// a literal here — `term_core` has no dependency on the mux daemon crate;
/// see `mux_ipc::protocol::MAX_SEGMENTS`'s doc for the same duplication) worth
/// of segments into a comparatively small byte span. Bounding segment COUNT
/// independent of byte length keeps that shape from silently slipping
/// through the byte-only gate.
///
/// D7 amendment (task0001, prior feature `mux-tab-switch-replay-latency`):
/// checked against `middle_segment_count` (`k - h`, per
/// [`leading_uniform_run_len`]), not the raw `k` — when there is no
/// rescuable HEAD (`h == 0`), `middle_segment_count == k` and this is
/// byte-identical to the original check.
///
/// D10 (mux-tab-switch-bypass-refix task0001, review finding
/// `b6a60c440da70e79`): raised from 24 to 62. `MAX_DIM_MARKERS` (the
/// daemon-side cap this bound duplicates as a literal, above) was
/// independently raised from 24 to 62 in a later round (see
/// `bench.rs::DAEMON_SEGMENT_CAP`'s doc) without updating THIS gate, so it
/// silently drifted stale at the daemon's PRIOR cap — the actually measured
/// bug shape (a 26-segment MIDDLE, comfortably under the daemon's CURRENT
/// 62-marker cap) was rejected by a bound calibrated to a cap the daemon no
/// longer enforces. Realigning the two restores the intended invariant
/// (this gate never rejects a shape the daemon itself could not have
/// produced) and, as a direct consequence, admits the measured shape.
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72` / `474e01ad8c29e7f0` / `96f7205be52fece8` /
/// `1adb07864f11618f`): this bound applies ONLY on the `h > 0` (HEAD-fold-
/// succeeded) path — see the tiering at this constant's call site in
/// `build_from_snapshot_inner`. The `h == 0` path (D9's fold-degradation
/// case) uses the separate, tighter
/// [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`] instead; see that
/// constant's doc for why. The paragraph below — "every MIDDLE segment
/// transition is a SAME-WIDTH resize" — is true ONLY of the `h > 0` tier
/// this constant now exclusively covers: `middle_is_row_bounded` (this
/// gate's companion safety check) has, by the time `h` is set to a
/// nonzero `candidate_h`, already verified `cols == target_cols` for
/// every one of `segments[h..k]`. Before D11, this bound applied
/// regardless of `h`, and the same doc sentence was FALSE for `h == 0`: a
/// column change does not degrade the whole HEAD fold and stop there — it
/// degrades `h` to `0` and MIDDLE (now `segments[0..k]`) still reaches
/// this gate, unchecked for column changes (`a1a06ed541045dd5`,
/// corroborated from the performance angle by `77da6aceb73b1a72`).
///
/// Top-end derivation (`474e01ad8c29e7f0`): 62 is not an arbitrary mirror
/// of the daemon's dim-marker record cap (`MAX_DIM_MARKERS`) — it is the
/// largest MIDDLE a fold-succeeded (`h > 0`) split can EVER contain for a
/// legal daemon snapshot. A daemon snapshot carries at most
/// `mux_ipc::protocol::MAX_SEGMENTS` (64) segments; a fold-succeeded
/// MIDDLE can never claim the mandatory HEAD (`candidate_h > 0`, at least
/// 1 segment) nor the mandatory SUFFIX (a split needs `k <
/// segments.len()`, at least 1 segment past the MIDDLE) — so the ceiling
/// is `MAX_SEGMENTS - 2` = 62, exactly this constant's value. The
/// `h == 0` tier is NOT bound by this same top-end reasoning (a `h == 0`
/// MIDDLE can legally reach 63, one shy of the wire cap, since it does
/// not need to give up a HEAD slot) — [`BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`]
/// is deliberately smaller than that reachable ceiling, as a COST choice,
/// not a shape-completeness one; see its own doc for the excluded top
/// slots this implies.
///
/// Purpose (`96f7205be52fece8` / `1adb07864f11618f`): for the `h > 0`
/// tier, this constant's PRIMARY role is now the cap-mirror derived above
/// (pinned exactly, see `bypass_prefix_max_segments_pin` below) — its
/// cost-bound role (below) is secondary, since `resize_same_width`'s
/// row-delta-bounded cost stays cheap even at the full 62. It is not dead
/// code even so: `build_from_snapshot_inner` accepts `segments` from any
/// caller, not only a daemon-shaped one (`term_core` has no runtime
/// dependency on `mux_ipc` — NFR5), so this condition still rejects an
/// `h > 0` MIDDLE built from more than 62 segments regardless of whether a
/// real daemon could ever produce one (e.g. test-constructed or
/// otherwise non-daemon-shaped input).
///
/// Why the cost stays cheap regardless (the rationale this bound was
/// originally introduced for: "each MIDDLE segment pays one reflow
/// regardless of its byte size"): every MIDDLE segment transition in the
/// `h > 0` tier is a SAME-WIDTH resize (see the D11 paragraph above).
/// `TerminalCore::resize_same_width` (`reflow.rs`,
/// D1, round-10 rework, `mux-render-corruption` task0010) bounds a
/// same-width resize's cost to the ROW-COUNT DELTA between the two
/// dimensions, not the size of scrollback accumulated so far — the
/// per-segment cost this bound exists to cap was, at the time of the
/// original 24-segment cap, dominated by an O(accumulated-content) reflow
/// that round-10 eliminated for exactly this same-width shape (see
/// `bench.rs::segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`'s
/// doc for the re-measured numbers: tens to ~164 ms across the 24-62
/// segment range post-round-10, vs. seconds pre-round-10).
/// [`BYPASS_PREFIX_MAX_BYTES`] and the suffix-dominance check (`suffix_len
/// >= middle_len`, NFR1, IMPLEMENTATION.md D-B) still bound the MIDDLE's
/// total byte cost regardless of how many segments it is split across, so
/// a genuinely expensive MIDDLE/prefix is rejected by those gates
/// independent of this one.
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_SEGMENTS: usize = 62;

/// Maximum number of segments the MIDDLE may contain when the HEAD fold
/// did NOT succeed (`h == 0` — D9's fold-degradation path in
/// `build_from_snapshot_inner`: a column change, a MIDDLE row count
/// exceeding the HEAD's own run rows, or an insufficient HEAD run row
/// count degrades `h` all the way to `0`).
///
/// D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5` /
/// `77da6aceb73b1a72`): split out of the single pre-D11
/// [`BYPASS_PREFIX_MAX_SEGMENTS`] bound. On the `h == 0` path, NOTHING has
/// verified the MIDDLE is same-width — `segments[0..k]` may contain
/// column-changing entries, each paying `TerminalCore::resize_full_reflow`
/// (cost proportional to the content accumulated in the (freshly
/// constructed, since `head_len == 0` here) core so far, i.e. bounded by
/// [`BYPASS_PREFIX_MAX_BYTES`] = 64 KiB total across the whole MIDDLE)
/// instead of the row-delta-bounded `resize_same_width`. Admitting up to
/// 62 segments on this path (pre-D11's mistaken uniform bound) means up
/// to 62 full reflows of that accumulated content, not 24 — an increase
/// this constant exists to undo.
///
/// The value (24) is deliberately the SAME value this gate used,
/// unconditionally, before D10 raised it to 62 — a value already
/// exercised (this file's `h == 0` boundary tests below) and, per this
/// bound's own historical role (identical reasoning to
/// [`BYPASS_PREFIX_MAX_BYTES`]'s doc: a bound small enough that even a
/// full non-bypass reflow of the content under it does not matter) does
/// not need new evidence to justify keeping it. It is a COST-POLICY
/// choice, independent of [`mux_ipc::protocol::MAX_SEGMENTS`] — unlike
/// [`BYPASS_PREFIX_MAX_SEGMENTS`], it carries NO wire-cap pin (see
/// `bypass_prefix_max_segments_pin`'s doc): raising or lowering it is a
/// deliberate cost decision, not drift, and a daemon snapshot's `h == 0`
/// MIDDLE could legally reach 63 (one shy of the wire cap) — the slots
/// between 24 and 63 are deliberately left out of scope on this path
/// (`474e01ad8c29e7f0`'s top-end concern, weakened here rather than made
/// true: the "this gate never rejects a shape the daemon itself could
/// have produced" invariant holds only for the `h > 0` tier above, not
/// this one).
pub(in crate::terminal_core) const BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD: usize = 24;
