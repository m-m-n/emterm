//! Snapshot / replay path of [`TerminalCore`]: reset-and-replay entry
//! points, structural segment replay, snapshot builds, snapshot-bypass
//! control, and detach-time scrollback merge.

use super::replay_plan::{
    BYPASS_PREFIX_MAX_BYTES, BYPASS_PREFIX_MAX_SEGMENTS, BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD,
    BYPASS_SUFFIX_MIN_BYTES, leading_uniform_run_len, middle_is_row_bounded,
    stable_target_suffix_start,
};
use super::{ReplaySegment, SnapshotReplay, TerminalCore, clamp_resize_dims};

impl TerminalCore {
    /// Reset the grid + parser to the post-construction state, then replay
    /// `bytes` so the resulting state reflects a fresh replay of that byte
    /// stream. Returns the mode actions accumulated during the replay (a
    /// snapshot captured while a full-screen app was running carries its
    /// buffer-switch sequences).
    ///
    /// Introduced for native-poc's mux-mode attach: after the daemon sends
    /// a `Snapshot`, the client wants to discard whatever the native PTY
    /// painted previously and paint the snapshot bytes from scratch. Uses
    /// the resume loop (`process_pty_data_fully`) — a single
    /// `process_pty_data` call would drop everything after the first
    /// buffer-switch sequence inside the snapshot.
    ///
    /// Equivalent to [`Self::reset_and_replay_segments`] with an empty
    /// segment list — a single, unsplit replay at `self`'s current
    /// dimensions (task0004 round-4 rework D1' / AC-11: the documented
    /// "no structural dimension info" degradation — this is what an older
    /// daemon's snapshot, or any caller with nothing to attribute, gets).
    pub fn reset_and_replay(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.reset_and_replay_segments(bytes, &[])
    }

    /// Reset the grid + parser to the post-construction state, then replay
    /// `bytes` under the dimensions `segments` describes structurally.
    ///
    /// This is the D1' replacement for the in-band `OSC 777;emterm;resize;…`
    /// marker byte scan (rounds 1-3, `mux::scrollback_buffer`
    /// `resize_marker_bytes` / `find_resize_marker`): dimensions are supplied
    /// HERE, as a caller-provided parameter, never discovered by scanning
    /// `bytes` for a recognizable pattern. No byte sequence appearing
    /// anywhere in `bytes` — however it is shaped, split, or nested — can
    /// therefore ever change what dimensions a replay applies; the forgery
    /// class rounds 1-3 spent three attempts trying to filter out of the
    /// byte stream is closed structurally instead (there is nothing left
    /// that scans for one).
    ///
    /// `segments` must be in ascending `offset` order (the caller's
    /// responsibility — mirrors the ordering invariant the daemon-side
    /// `dim_markers` structure already keeps). For each segment, in order,
    /// `self` is resized to `(segment.cols, segment.rows)` (only when they
    /// differ from the current size) and then fed the byte range from this
    /// segment's `offset` up to the NEXT segment's `offset` (or the end of
    /// `bytes` for the last segment). An `offset` past `bytes.len()` is
    /// clamped. After the last segment (or immediately, if `segments` is
    /// empty), `self` is resized back to its dimensions at the START of this
    /// call (the caller's requested / current pane size) if anything
    /// changed them, so a replay with any number of intervening resizes
    /// always ends at the size the caller asked for — matching the old
    /// marker-scan replay's contract exactly, just driven by `segments`
    /// instead of a byte scan.
    ///
    /// An empty `segments` reduces to a single unsplit
    /// `process_pty_data_fully_cancellable` call at `self`'s current
    /// dimensions — byte-for-byte identical to the pre-task0001 replay
    /// (task0001 AC-3 / task0004 AC-11).
    pub fn reset_and_replay_segments(
        &mut self,
        bytes: &[u8],
        segments: &[ReplaySegment],
    ) -> Vec<u8> {
        self.reset();
        // The non-cancellable entry point: delegates to the cancellable
        // drain with a flag that is never set, so `reset_and_replay_segments`
        // and `build_from_snapshot`'s cancellable drain share one replay
        // implementation and cannot drift. `NEVER` is never stored to, so
        // the drain always runs to completion and returns `Some` — the
        // unwrap cannot fail.
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let (final_cols, final_rows) = (self.cols, self.rows);
        self.replay_segments(bytes, segments, &NEVER, final_cols, final_rows)
            .expect("non-cancellable drain always completes")
    }

    /// Cancellable implementation shared by [`Self::reset_and_replay_segments`]
    /// and `build_from_snapshot_inner`. See
    /// [`Self::reset_and_replay_segments`] for the segment-driven replay
    /// contract; `cancel` is threaded straight through to each segment's
    /// `process_pty_data_fully_cancellable`, and a flag observed mid-drain
    /// aborts the whole replay and returns `None` (a superseded off-thread
    /// `build_from_snapshot` worker bails out at the next chunk boundary
    /// instead of finishing the parse).
    ///
    /// `final_cols`/`final_rows` is the size `self` is resized back to (if
    /// anything changed it) once every segment has replayed — the caller's
    /// requested / current pane size. task0004 (D8, review round-1 rework,
    /// finding `b21749c5f2bd1006`) made this an explicit parameter rather
    /// than `self.cols`/`self.rows` captured at entry: `build_from_snapshot_inner`'s
    /// MIDDLE sub-replay now calls this with `self` starting at the HEAD's
    /// own (possibly non-target) dimensions, but must still end at the
    /// TRUE caller target in one hop — capturing `self.cols`/`self.rows` at
    /// entry would resize back to the HEAD's dimensions instead, an extra
    /// (and potentially non-equivalent) resize hop the reference path never
    /// takes. `reset_and_replay_segments` passes its own `self.cols`/
    /// `self.rows` at entry, preserving this method's original behavior for
    /// every other caller.
    fn replay_segments(
        &mut self,
        bytes: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
        final_cols: u16,
        final_rows: u16,
    ) -> Option<Vec<u8>> {
        let target_cols = final_cols;
        let target_rows = final_rows;
        let mut actions = Vec::new();
        if segments.is_empty() {
            actions.extend(self.process_pty_data_fully_cancellable(bytes, cancel)?);
            return Some(actions);
        }
        // D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
        // `segments` is no longer guaranteed to have its first entry at
        // offset 0 — the daemon-side cap-eviction gap (2+ evicted
        // `dim_markers` entries) is now left unattributed by
        // `ScrollbackRingBuffer::read_segments` rather than synthesizing a
        // (potentially wrong) head segment for it. Replay that leading gap,
        // if any, BEFORE the first segment's own dims are applied — at
        // whatever dims `self` already has, which is exactly
        // `target_cols`/`target_rows` here since nothing has resized `self`
        // yet. This is what "leave the gap unattributed" cashes out to at
        // replay time: those bytes replay under the caller's TARGET size,
        // never silently dropped.
        let first_offset = (segments[0].offset as usize).min(bytes.len());
        if first_offset > 0 {
            actions
                .extend(self.process_pty_data_fully_cancellable(&bytes[..first_offset], cancel)?);
        }
        for (i, seg) in segments.iter().enumerate() {
            let start = (seg.offset as usize).min(bytes.len());
            let end = segments
                .get(i + 1)
                .map(|next| (next.offset as usize).min(bytes.len()))
                .unwrap_or(bytes.len());
            // The resize is applied ONLY when this segment actually has
            // content to feed (`end > start`) — mirroring the round-1
            // rework reflow-coalescing fix (finding `6ff208bbc674189c`): a
            // run of segments whose content ranges are all empty (their
            // offsets collapse together — no real bytes were ever recorded
            // at those intermediate dimensions) costs ZERO reflows for the
            // empty ones. Only the segment that actually has bytes to feed
            // pays a reflow, for its OWN dimensions — never one reflow per
            // segment regardless of content.
            if end > start {
                // D1'' (task0005 rework, review round-4 finding
                // `da834d05f3f18af4`, high): a decoded segment travels
                // straight from the wire (`mux_ipc::protocol::DimSegment`,
                // an untrusted `u16` pair) to here. Without this clamp,
                // `self.resize` allocates `(scrollback_capacity + rows) *
                // cols` cells unconditionally — a segment carrying
                // `cols == rows == 65535` requests roughly 4.3 billion
                // cells, and a zero dimension trips `resize_reflow`'s
                // `debug_assert!(cols > 0 && rows > 0)` (an underflow in
                // release builds). `clamp_resize_dims` is the SAME domain
                // the daemon-side producer already enforces
                // (`MuxPane::resize` / `MuxPane::new`) — applying it again
                // here means a segment can never resize this core outside
                // that domain regardless of what produced it (a forged
                // frame, a future encoder bug, or a daemon that predates
                // the producer-side clamp).
                let (seg_cols, seg_rows) = clamp_resize_dims(seg.cols, seg.rows);
                if (self.cols, self.rows) != (seg_cols, seg_rows) {
                    self.resize(seg_cols, seg_rows);
                }
                actions
                    .extend(self.process_pty_data_fully_cancellable(&bytes[start..end], cancel)?);
            }
        }
        if (self.cols, self.rows) != (target_cols, target_rows) {
            self.resize(target_cols, target_rows);
        }
        Some(actions)
    }

    /// Construct a fresh `TerminalCore` sized to `(cols, rows,
    /// scrollback_lines)`, full-drain replay `payload` into it, and return
    /// the built core together with the mode actions and the
    /// prompt / fold marks (plus the post-replay eviction total) drained
    /// during the replay.
    ///
    /// This is the **pure, off-thread half** of the mux snapshot-replay
    /// recipe: it owns and returns the core (no `&mut self`), installs no
    /// callbacks, and touches no GUI / thread-local state, so it can run on
    /// a worker thread and the result moved to the main thread. The
    /// returned bundle is observably identical to the in-place
    /// `reset_and_replay` + `drain` path on a core of the same size fed the
    /// same `payload` for the externally observable bookkeeping — the
    /// `evicted_total`, `prompt_marks` (`abs_row` + `evicted_total`), and
    /// `fold_marks` (`abs_row` + `evicted_total`) match byte-identically —
    /// and the viewport grid + cursor are byte-identical. The off-thread
    /// path and the synchronous path therefore reconcile from
    /// byte/grid-identical inputs.
    ///
    /// **Scrollback contents are intentionally not populated by the replay.**
    /// During the drain the per-row SlimCell compression (the dominant cost
    /// on a heavy `seq`-shaped payload) is bypassed; `core.scrollback_count()`
    /// is `0` on the returned core. The `scrollback_capacity` is the
    /// caller-requested `scrollback_lines`, so any live PTY appends to the
    /// returned core accumulate into scrollback exactly as they do today.
    /// The bypass keeps the observable bookkeeping byte-identical via an
    /// internal virtual scrollback length (see
    /// `TerminalCore::scrollback_bypass` / `virtual_scrollback_len`).
    ///
    /// A fresh `TerminalCore::new` is already in the post-`reset` state, so
    /// the extra `reset` inside `reset_and_replay` is a no-op here; it is
    /// kept so the off-thread builder and the synchronous path share the
    /// exact same replay entry point (`reset_and_replay`) and cannot drift.
    ///
    /// The drained marks/total are returned (rather than left on the core)
    /// because the caller backfills them into its own absolute-row trackers
    /// after the swap, exactly as the synchronous `drain_marks` site does.
    /// `cancel` lets a superseded off-thread worker abandon the parse at the
    /// next chunk boundary; when it is observed set mid-drain this returns
    /// `None` (the partially-built core is discarded by the caller).
    pub fn build_from_snapshot(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(
            cols,
            rows,
            scrollback_lines,
            payload,
            segments,
            cancel,
            true,
        )
    }

    /// Sibling of [`Self::build_from_snapshot`] that runs the same replay
    /// **with the snapshot bypass disabled**. The drained core therefore has
    /// its `scrollback_slim` / `scrollback_wrapped` populated up to
    /// `scrollback_lines` rows, which is what the 2nd-pass scrollback-restore
    /// worker needs to feed [`Self::merge_scrollback_from`].
    ///
    /// Observable bookkeeping matches the synchronous `reset_and_replay`
    /// path byte-identically (same `evicted_total`, same prompt/fold marks,
    /// same grid). `bypass_b_mark_texts` is empty because the bypass is off
    /// and the live scrollback is the source of truth for B-mark texts —
    /// the caller MUST ignore that field on the result (FR8).
    ///
    /// `cancel` semantics are identical to `build_from_snapshot`: a set flag
    /// observed mid-drain returns `None` and the partially-built core is
    /// discarded.
    pub fn build_scrollback_only_from_snapshot(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<SnapshotReplay> {
        Self::build_from_snapshot_inner(
            cols,
            rows,
            scrollback_lines,
            payload,
            segments,
            cancel,
            false,
        )
    }

    /// Shared inner helper for [`Self::build_from_snapshot`] (bypass on) and
    /// [`Self::build_scrollback_only_from_snapshot`] (bypass off). The two
    /// sibling entry points are thin wrappers that only differ in whether
    /// `enable_snapshot_bypass` is called, which keeps the recipe (reset →
    /// drain → take marks → assemble `SnapshotReplay`) in one place.
    fn build_from_snapshot_inner(
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        payload: &[u8],
        segments: &[ReplaySegment],
        cancel: &std::sync::atomic::AtomicBool,
        bypass: bool,
    ) -> Option<SnapshotReplay> {
        // D1''' (round-6 rework, review round-5 findings `abb36fa1ad4c89ea`
        // / `986a3881b2b97a16`): rounds 1-5 downgraded to the non-bypass
        // recipe for the WHOLE drain the moment ANY segment differed from
        // the target — correct, but it turns an ORDINARY switch into the
        // full non-bypass cost, because the daemon's spawn-size head
        // marker (`MuxPane::new`'s hardcoded 80x24) differs from the GUI's
        // actual grid until the ring evicts it (~2 MiB later). Measured:
        // 7ms segment-free vs 170-220ms for a single differing segment,
        // even though the divergence risk (see D6's history below) only
        // ever concerns the RESIZE moments themselves, not the (typically
        // much larger) bytes that follow the LAST one.
        //
        // Fix: split the replay at the start of the trailing run of
        // segments that ALREADY carry `(cols, rows)` — the caller's target
        // — via `stable_target_suffix_start`. Everything before that point
        // (the "prefix") replays WITHOUT bypass: full content-preserving
        // resizes, correct, priced only by the prefix's OWN content
        // (typically tiny — an ordinary switch's prefix is just the bytes
        // between the daemon spawning the pane and the GUI's first resize).
        // Once the core reaches the target dimensions, NOTHING in the
        // remaining segments changes them again BY CONSTRUCTION of the
        // split point, so the suffix — the pane's actual history, the part
        // that dominates payload size — replays under the fast bypass path
        // with zero resize risk. `bypass_split` below is `None` when
        // there's no prefix to speak of (`k == 0`, the pre-round-6 fast
        // path — segments already open at the target) or the suffix is too
        // small to bother (`BYPASS_SUFFIX_MIN_BYTES`), in which case this
        // falls back to the SAME whole-drain non-bypass recipe rounds 1-5
        // used whenever `k > 0` (still correct — see D6 below for why it
        // must be correct, not merely fast).
        //
        // Viewport / cursor equivalence: `ring_push_blank`'s bypass branch
        // differs from the non-bypass branch ONLY in whether the evicted
        // row's content is compressed into real scrollback or counted
        // virtually — both branches advance `ring_head` / clear the new
        // viewport bottom identically either way. Since the suffix (by
        // construction) contains no resize, the viewport + cursor it
        // produces are therefore byte-identical whether or not bypass is
        // engaged for it; only the SCROLLBACK CONTENT differs (virtual vs
        // real), which is exactly what `scrollback_populated` already
        // exists to flag to the caller (the 2nd-pass background worker,
        // `tabs.rs::apply_offthread_swap`, fills it in for real). AC-5
        // equivalence for the ACTUAL split is
        // `bypass_split_matches_reference_viewport_and_cursor_for_ordinary_switch`
        // (below this fn's tests); the pre-existing `..._row_growing_marker`
        // / `..._cols_only_marker` fixtures cover the "no benefit" (`k ==
        // segments.len()`, empty suffix) case unchanged.
        //
        // D6 (task0003, review round-2 finding `893241823258fce3`) / D5''
        // (task0005, review round-4 finding `697d8dc2b88dcddc`): the reason
        // a resize genuinely needs the non-bypass recipe at all — a
        // row-count-GROWING (or column-changing) mid-drain resize needs to
        // pull rows up from / re-wrap real scrollback, and the bypass keeps
        // `scrollback_slim` deliberately empty, so doing that resize WHILE
        // bypassed diverges from the synchronous path. This still applies
        // to every resize up to and including the one that reaches the
        // target — `stable_target_suffix_start` never lets the split
        // engage bypass any earlier than that.
        let k = if bypass {
            stable_target_suffix_start(cols, rows, segments)
        } else {
            0
        };
        let split_at = segments
            .get(k)
            .map(|s| (s.offset as usize).min(payload.len()))
            .unwrap_or(payload.len());
        // D5'''' (round-7 rework, review round-6 finding `e519916efd5fdc42`):
        // also gate on the PREFIX being cheap (`split_at` is exactly its
        // byte length) — see `BYPASS_PREFIX_MAX_BYTES`'s doc for why an
        // expensive prefix makes the split pay its own "fast path" cost
        // twice instead of once.
        //
        // D5''''' (round-8 rework, review round-7 finding
        // `a4f4e36fef377d05`): the byte-only gate above still let a payload
        // that is OVERWHELMINGLY prefix engage the split — a 64 KiB prefix
        // (right at `BYPASS_PREFIX_MAX_BYTES`, inclusive) paired with just
        // over `BYPASS_SUFFIX_MIN_BYTES` (4096) of suffix is ~94% prefix by
        // volume, yet both individual thresholds are satisfied. Additionally
        // require the SUFFIX to actually DOMINATE the prefix
        // (`suffix_len >= split_at`) — not merely clear an absolute floor —
        // and bound the PREFIX's own segment count
        // (`k <= BYPASS_PREFIX_MAX_SEGMENTS`), independent of its byte
        // length: a prefix built from many small segments still pays one
        // reflow per segment regardless of how few total bytes they cover.
        let suffix_len = payload.len() - split_at;
        // D7 (task0001, NFR1-safe rescue for a resize-marker-dense tail):
        // `k`/`split_at` above find where the split's SUFFIX may safely
        // start, but the region BEFORE that point ("prefix", historically
        // treated as one expensive, non-bypass whole) can itself contain a
        // large LEADING run of segments that are already uniform in size —
        // swept in only because SOME later segment (still before the
        // stable tail) diverges from it. A pane whose recorded scrollback
        // has a dense cluster of resize markers near its tail (dims
        // wobbling away from and back to a settled size, e.g. during
        // status-bar settling) produces exactly this shape: `k` and
        // `split_at` land far past the cluster's own small footprint
        // because they are computed from the LAST divergence, dragging a
        // huge, already-safe HEAD along with the genuinely resize-needing
        // MIDDLE. `h` finds that leading safe run so the two can be told
        // apart; seeing `h` in the byte length is why `middle_len <
        // split_at` in that shape even though `middle_segment_count` stays
        // close to `k`.
        //
        // D8 (task0004, review round-1 rework, finding `b21749c5f2bd1006`):
        // the HEAD's own leading run need not be AT THE CALLER'S TARGET
        // dims — `leading_uniform_run_len` admits any uniform (target_cols,
        // R) run, reporting the run's own row count `R` alongside its
        // length. This is what makes a marker cluster that oscillates
        // ABOVE the settled target (the SPEC's actual measured direction —
        // `visible_row_count` 0→1 SHRINKS the grid, so the pre-settling
        // HEAD sits at the LARGER size) foldable: `R` becomes the safety
        // ceiling `middle_is_row_bounded` checks against below, in place of
        // `target_rows`. When the HEAD genuinely opens at the target
        // (`R == rows`, every pre-D8 shape), this is byte-identical to the
        // original `leading_target_run_len`.
        //
        // `R >= rows` is required IN ADDITION to `middle_is_row_bounded`:
        // once MIDDLE finishes, `replay_segments` resizes straight back to
        // the caller's `rows` in one hop (see that method's doc) — a
        // transition `middle_is_row_bounded` never itself examines, since
        // it is implicit, not one of `segments[h..k]`'s own entries. That
        // final hop is only ever a `<=R` move (safe, by the same argument)
        // when `rows <= R`; without this check a single ordinary-sized
        // leading segment (e.g. the daemon's spawn-size marker, which
        // trivially satisfies "a uniform run of length 1") would be folded
        // as a HEAD whose `R` is BELOW the target, silently discarding real
        // content the final grow-to-target then has no way to recover.
        //
        // D9 (task0004, review round-1 rework, finding `6a02ed7e1b606588`):
        // if the resulting HEAD cannot be safely folded (a column change, a
        // row count in MIDDLE exceeding the HEAD's own `R`, or `R < rows`),
        // degrade `h` all the way to `0` — the pre-D7 computation — rather
        // than gating `bypass_split` on a separate `head_fold_safe` flag.
        // Only ABANDONING the fold (not the whole split) means a shape that
        // engaged the split before D7 (e.g. a small target HEAD, a small
        // column-change MIDDLE, and a large target TAIL) still engages it
        // here: with `h == 0`, `middle_len == split_at` and
        // `middle_segment_count == k`, matching the pre-D7 gates exactly.
        let (h, head_rows) = if bypass && k > 0 {
            let (candidate_h, candidate_rows) = leading_uniform_run_len(cols, &segments[..k]);
            let candidate_safe = candidate_h > 0
                // `h == k` would leave an EMPTY MIDDLE, and `replay_segments`
                // early-returns for empty `segments` WITHOUT its "resize back
                // to the caller's target" step — the core would stay at
                // `head_rows` forever. Only fold when a real MIDDLE remains
                // to carry that final hop.
                && candidate_h < k
                && candidate_rows >= rows
                && middle_is_row_bounded(cols, candidate_rows, &segments[candidate_h..k]);
            if candidate_safe {
                (candidate_h, candidate_rows)
            } else {
                (0, rows)
            }
        } else {
            (0, rows)
        };
        let head_len = if h > 0 {
            segments
                .get(h)
                .map(|s| (s.offset as usize).min(payload.len()))
                .unwrap_or(payload.len())
        } else {
            0
        };
        let middle_len = split_at - head_len;
        let middle_segment_count = k - h;
        // D10 (mux-tab-switch-bypass-refix task0001, review finding
        // `b6a60c440da70e79`): the actually measured bug shape's MIDDLE is
        // 26 segments, one past the round-8 gate's then-current bound (24)
        // — see `BYPASS_PREFIX_MAX_SEGMENTS`'s doc for why that bound had
        // silently drifted stale (the daemon-side cap it mirrors moved to
        // 62 in a later round without this gate following) and why
        // realigning it to the daemon's CURRENT cap does not reintroduce
        // the cost this bound exists to bound (NFR1).
        //
        // D11 (task0004, review round-1 rework, findings `a1a06ed541045dd5`
        // / `77da6aceb73b1a72`): D10's "same-width by construction" cost
        // rationale for 62 holds ONLY when `h > 0` — `middle_is_row_bounded`
        // has, by construction of `candidate_safe` above, already verified
        // every one of `segments[h..k]` is same-width in that case. When
        // `h == 0` (D9's fold-degradation path), NOTHING has verified
        // that — `segments[0..k]` can contain column-changing entries,
        // each paying `resize_full_reflow` (cost proportional to the
        // content accumulated within the MIDDLE so far) instead of the
        // row-delta-bounded `resize_same_width`. Apply the tighter,
        // independently-justified `BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD`
        // bound on that path instead — see its own doc and
        // `BYPASS_PREFIX_MAX_SEGMENTS`'s doc (both constants' cost budget
        // is shared with, and bounded by, `BYPASS_PREFIX_MAX_BYTES` below
        // regardless of tier).
        let segment_bound = if h > 0 {
            BYPASS_PREFIX_MAX_SEGMENTS
        } else {
            BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD
        };
        let bypass_split = bypass
            && k > 0
            && middle_segment_count <= segment_bound
            && suffix_len >= BYPASS_SUFFIX_MIN_BYTES
            && middle_len <= BYPASS_PREFIX_MAX_BYTES
            && suffix_len >= middle_len;
        // Whether `enable_snapshot_bypass` will actually be called anywhere
        // below — `k == 0` is the pre-round-6 "no transition at all" fast
        // path (segments already open at the target); `bypass_split` is the
        // D1''' prefix/suffix split. Both leave the core bypass-engaged by
        // the time the trailing bookkeeping below runs. Distinct from the
        // raw `bypass` parameter: a caller that requested bypass but whose
        // segments neither open at the target nor clear the split
        // threshold (`k > 0`, small tail) still gets the correct, but
        // non-bypassed, whole-drain replay — mirroring rounds 1-5's
        // downgrade for that shape exactly.
        let bypass_engaged = bypass_split || (bypass && k == 0);

        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        core.reset();
        // D4'''' (round-7 rework, review round-6 finding `0bed3c30e41e2389`):
        // set BEFORE any bytes are replayed (including the PREFIX, which
        // runs before `enable_snapshot_bypass` below) so a B mark emitted
        // during the prefix is captured just like one emitted during the
        // suffix — see `capture_bypass_b_marks`'s doc.
        core.capture_bypass_b_marks = bypass_engaged;

        let actions = if bypass_split {
            let (head_bytes, rest_bytes) = payload.split_at(head_len);
            let (middle_bytes, suffix_bytes) = rest_bytes.split_at(middle_len);
            // D7: `middle_segments` are `segments[h..k]` rebased so their
            // `offset`s are relative to `middle_bytes` (they were absolute
            // into `payload`, which starts `head_len` bytes earlier).
            let middle_segments: Vec<ReplaySegment> = segments[h..k]
                .iter()
                .map(|s| ReplaySegment {
                    offset: s.offset.saturating_sub(head_len as u32),
                    cols: s.cols,
                    rows: s.rows,
                })
                .collect();

            let mut actions = Vec::new();
            if head_len > 0 {
                // D8: the HEAD may open at `head_rows` rather than the
                // caller's `rows` (see the `h`/`head_rows` computation
                // above). `core` was just constructed + reset — completely
                // empty, no bytes replayed yet — so this resize is the
                // SAME operation the reference path performs for its own
                // first segment on an equally fresh core (see
                // `replay_segments`'s leading-gap handling): it cannot
                // diverge from the reference regardless of grow/shrink
                // direction, because there is no real content on either
                // side to lose. A shrink here deposits blank rows into
                // `scrollback_slim` for real (`resize`'s reflow is not
                // bypass-aware); fold them into the SAME virtual
                // bookkeeping the bypass path uses so `enable_snapshot_bypass`'s
                // "empty deque" precondition holds regardless of direction
                // (a no-op when the resize was a grow, which never adds to
                // `scrollback_slim`).
                if head_rows != rows {
                    core.resize(cols, head_rows);
                    core.restore_bypass_invariant_after_reflow();
                }
                // HEAD: every segment in `segments[..h]` already carries
                // `(cols, head_rows)` by construction of `h`, so — exactly
                // like the SUFFIX below — no further resize can occur
                // here; feed the bytes directly under bypass (cheap: no
                // SlimCell compression for content that was never going to
                // move dimensions). `scrollback_slim` is empty on entry
                // (either untouched, or just folded above), satisfying
                // `enable_snapshot_bypass`'s precondition.
                core.enable_snapshot_bypass();
                actions.extend(
                    match core.process_pty_data_fully_cancellable(head_bytes, cancel) {
                        Some(a) => a,
                        None => {
                            core.disable_snapshot_bypass();
                            return None;
                        }
                    },
                );
                // Suspend bypass for the MIDDLE (not `disable_snapshot_bypass`,
                // which would zero `virtual_scrollback_len` and lose the
                // HEAD's contribution to it) — `scrollback_slim` is still
                // empty (the head's own byte replay never touched it), so
                // there is nothing to fold at this transition; the fold
                // happens once, below, after the MIDDLE finishes.
                core.suspend_snapshot_bypass();
            }
            // MIDDLE: bypass is NOT enabled here (whether or not a HEAD ran
            // above), so this is a plain, full-fidelity replay — identical
            // to what the pre-D7 whole "prefix" replay did for
            // `segments[..k]`, just possibly starting partway through it.
            // D8: pass the TRUE caller target explicitly — `core` starts
            // this call at `head_rows`, not `rows`, whenever a HEAD ran
            // above, and `replay_segments`'s own "resize back to the
            // caller's target" step must land on `rows`, not `head_rows`,
            // in a SINGLE hop (see `replay_segments`'s doc for why this
            // must not be inferred from `core`'s dimensions at entry).
            let mut actions_middle =
                match core.replay_segments(middle_bytes, &middle_segments, cancel, cols, rows) {
                    Some(a) => a,
                    None => return None,
                };
            actions.append(&mut actions_middle);
            // Fold the MIDDLE's real scrollback into the SAME virtual
            // bookkeeping the bypass path uses (adding onto whatever the
            // HEAD already contributed to `virtual_scrollback_len`), so
            // `get_scrollback_length()` stays continuous across the phase
            // boundary and `enable_snapshot_bypass`'s "empty deque"
            // precondition holds.
            core.restore_bypass_invariant_after_reflow();
            core.enable_snapshot_bypass();
            // Suffix: every remaining segment already carries `(cols, rows)`
            // by construction of `k`, so no resize can occur here — feeding
            // the bytes directly (no segments) is equivalent to replaying
            // them via `replay_segments` and cheaper to compute.
            actions.extend(
                match core.replay_segments(suffix_bytes, &[], cancel, cols, rows) {
                    Some(a) => a,
                    None => {
                        // Cancelled mid-drain: leave the core consistent before
                        // discarding it via the `None` return (matches the
                        // non-split path below).
                        core.disable_snapshot_bypass();
                        return None;
                    }
                },
            );
            actions
        } else {
            if bypass_engaged {
                core.enable_snapshot_bypass();
            }
            match core.replay_segments(payload, segments, cancel, cols, rows) {
                Some(a) => a,
                None => {
                    // Cancelled mid-drain: leave the core consistent (clear the
                    // bypass) before discarding it via the `None` return so a
                    // debugger / panic handler that touches the dropped core
                    // doesn't see a half-set bypass.
                    if bypass_engaged {
                        core.disable_snapshot_bypass();
                    }
                    return None;
                }
            }
        };
        let evicted_total = core.get_scrollback_evicted_total();
        let prompt_marks = core.take_prompt_marks();
        let fold_marks = core.take_fold_marks();
        let bypass_b_mark_texts = core.take_bypass_b_mark_texts();
        // Discard any device responses (DA1 / DSR / XTWINOPS / …) generated by
        // historic queries baked into the snapshot bytes. Their originating
        // program is long gone; after the swap, the next live `take_response`
        // would otherwise pick them up and deliver a stale reply to the live
        // shell's stdin. Matches the synchronous `reset_frame_for_replay` path.
        let _ = core.take_response();
        if bypass_engaged {
            // Regression guard (review round-1 rework, finding
            // `1698d9b52a89e241`): `TerminalCore::resize` restores the
            // bypass invariant on every call made while bypass is active
            // (see that method), so `scrollback_slim` must always be empty
            // here. A future in-drain mutation path that populates
            // `scrollback_slim` WITHOUT going through `resize`'s restore
            // step would silently break the 2nd-pass merge's row-dedup
            // accounting; this makes that failure loud in tests instead.
            debug_assert!(
                core.scrollback_slim.is_empty(),
                "snapshot-replay bypass invariant violated: scrollback_slim \
                 is not empty before disable_snapshot_bypass (leaked {} rows)",
                core.scrollback_slim.len()
            );
            core.disable_snapshot_bypass();
        }
        // D3' (task0004 round-4 rework, review round-3 finding
        // `b235e4dbc61cc4ba`): `scrollback_populated` tells the caller
        // whether THIS replay actually populated `scrollback_slim` —
        // `!bypass_engaged` covers "bypass off by construction"
        // (`build_scrollback_only_from_snapshot`), "bypass downgraded for
        // this payload" (small/no stable tail, mirroring the old D6
        // row-growth guard), AND "bypass engaged for a suffix" (D1''',
        // partial — the prefix's real rows were folded into virtual
        // bookkeeping above, so `scrollback_slim` is empty end to end
        // regardless of which of these three shapes produced this result).
        Some(SnapshotReplay {
            core,
            actions,
            evicted_total,
            prompt_marks,
            fold_marks,
            bypass_b_mark_texts,
            scrollback_populated: !bypass_engaged,
        })
    }

    /// Enable the snapshot-replay bypass: subsequent `ring_push_blank`
    /// evictions skip the SlimCell intern + `scrollback_slim` push/pop work
    /// (the per-row hot loop), but still bump `virtual_scrollback_len` /
    /// `scrollback_evicted_total` so the observable bookkeeping is byte-
    /// identical to the live path on the same payload.
    ///
    /// Precondition (asserted): the scrollback deque is empty. This holds
    /// immediately after `reset()` on a freshly-constructed core (the
    /// original call site, inside `build_from_snapshot`) — AND after
    /// `restore_bypass_invariant_after_reflow` has folded a non-bypass
    /// PREFIX's real scrollback into `virtual_scrollback_len` (the D1'''
    /// round-6 rework call site inside `build_from_snapshot_inner`'s
    /// prefix/suffix split, where `virtual_scrollback_len` legitimately
    /// starts non-zero — carrying forward the prefix's real length so
    /// `get_scrollback_length()` stays continuous across the phase
    /// boundary). Only `scrollback_slim` itself is required empty here;
    /// `virtual_scrollback_len` is deliberately NOT asserted zero.
    pub(crate) fn enable_snapshot_bypass(&mut self) {
        assert!(
            self.scrollback_slim.is_empty(),
            "enable_snapshot_bypass requires an empty scrollback deque"
        );
        self.scrollback_bypass = true;
    }

    /// Disable the snapshot-replay bypass. Resets `virtual_scrollback_len`
    /// to zero so subsequent live operations on this core see the original
    /// `get_scrollback_length() == scrollback_count() as u32` semantics.
    /// `scrollback_evicted_total` is intentionally NOT touched — its
    /// monotonic semantics are part of the externally observable contract.
    pub(crate) fn disable_snapshot_bypass(&mut self) {
        self.virtual_scrollback_len = 0;
        self.scrollback_bypass = false;
        self.capture_bypass_b_marks = false;
    }

    /// Suspend the snapshot-replay bypass for the MIDDLE segment of a
    /// HEAD/MIDDLE/SUFFIX split (D7, task0001; D8, task0004) — the bypass
    /// state machine's third transition, named alongside
    /// [`Self::enable_snapshot_bypass`] / [`Self::disable_snapshot_bypass`]
    /// (task0004, review round-1 rework, finding `0e3a7dee5f50d788`).
    ///
    /// Unlike [`Self::disable_snapshot_bypass`], this does NOT zero
    /// `virtual_scrollback_len` or clear `capture_bypass_b_marks` — the
    /// HEAD's contribution to both must survive so `get_scrollback_length()`
    /// stays continuous once the MIDDLE begins folding its own real
    /// scrollback into the same bookkeeping via
    /// `restore_bypass_invariant_after_reflow`, and so a B mark emitted
    /// during the MIDDLE is captured exactly like one emitted during the
    /// HEAD or SUFFIX (see `capture_bypass_b_marks`'s doc).
    ///
    /// Precondition (asserted, debug only): `scrollback_slim` is empty —
    /// the HEAD's own byte replay never populates it for real (any resize
    /// needed to REACH the HEAD's dimensions on the fresh core happens
    /// BEFORE bypass is enabled, and is folded via
    /// `restore_bypass_invariant_after_reflow` at that point — see the
    /// `h`/`head_rows` computation in `build_from_snapshot_inner`).
    pub(crate) fn suspend_snapshot_bypass(&mut self) {
        debug_assert!(
            self.scrollback_slim.is_empty(),
            "suspend_snapshot_bypass invariant violated: the HEAD must \
             never populate real scrollback (leaked {} rows)",
            self.scrollback_slim.len()
        );
        self.scrollback_bypass = false;
    }

    /// Consume `other` and prepend its scrollback rows onto `self`,
    /// re-interning each cell's `style_id` (and `char_ref` when in
    /// `CharTable` mode) against `self.styles` / `self.chars` so the merged
    /// rows resolve against `self`'s own tables.
    ///
    /// Used by the 2nd-pass scrollback-restore worker (`tabs.rs::
    /// apply_scrollback_restore`): after `build_scrollback_only_from_snapshot`
    /// rebuilds the historical scrollback off-thread, this method merges
    /// the rebuilt scrollback into the live core. The bypass-on 1st-pass
    /// swap (see [`Self::build_from_snapshot`]) leaves
    /// `scrollback_slim` empty, and the merge restores it.
    ///
    /// FR3 trim: the caller passes `live_trim_rows`, the number of trailing
    /// rebuilt rows to drop before prepending. These correspond to scrollback
    /// rows that have already been re-emitted by the live drain between the
    /// 1st-pass swap and now; including them would duplicate rows after the
    /// merge.
    ///
    /// Preconditions:
    /// - `self.cols == other.cols` (else: log::warn + no-op; the rebuilt
    ///   rows would be the wrong width to render against this core's grid).
    ///
    /// Postconditions:
    /// - The trailing `live_trim_rows` rows of `other.scrollback_slim` /
    ///   `scrollback_wrapped` are dropped.
    /// - The remaining `other.scrollback_slim` / `scrollback_wrapped` rows
    ///   are re-interned and prepended onto `self.scrollback_slim` /
    ///   `scrollback_wrapped` (oldest-first ordering preserved).
    /// - If the combined length would exceed `self.scrollback_capacity`,
    ///   the front-most *incoming* rows are dropped (the oldest rebuilt
    ///   rows) — `self`'s existing rows are preserved (they reflect
    ///   post-bypass live drain).
    /// - `self.scrollback_evicted_total` is UNCHANGED. These rows pre-date
    ///   the bypass swap; bumping the counter would double-count against
    ///   already-emitted delta notifications (NFR5).
    /// - `other` is consumed and dropped at function end.
    ///
    /// Returns the number of rows actually inserted into `self` (the
    /// rebuilt count minus `live_trim_rows` minus any capacity-overflow
    /// drops). 0 on cols mismatch or when `live_trim_rows >= rebuilt_count`.
    pub fn merge_scrollback_from(&mut self, other: TerminalCore, live_trim_rows: usize) -> usize {
        if self.cols != other.cols {
            log::warn!(
                "merge_scrollback_from cols mismatch: self={} other={}; no-op",
                self.cols,
                other.cols
            );
            return 0;
        }
        let other_styles = other.styles;
        let other_chars = other.chars;
        let mut other_slim = other.scrollback_slim;
        let mut other_wrapped = other.scrollback_wrapped;
        // FR3: drop the trailing live-drain-collision rows before
        // re-interning so we never pay the intern cost on rows we know we
        // will throw away.
        let rebuilt_count = other_slim.len();
        if live_trim_rows >= rebuilt_count {
            // Full no-op: every row already collided with live drain.
            return 0;
        }
        let keep = rebuilt_count - live_trim_rows;
        other_slim.truncate(keep);
        other_wrapped.truncate(keep);
        // Capacity-aware pre-trim: prepend_scrollback_rows will drop the front-most
        // rows that exceed `scrollback_capacity - existing`. Doing this BEFORE the
        // re-intern loop avoids re-interning rows that get dec_ref'd immediately.
        // `live_trim_rows` (tail trim) is eviction-based; `existing` is length-based,
        // so a live ring that grew toward capacity without evicting still consumes
        // room here. The dropped rebuilt cells reference `other_styles` / `other_chars`
        // which are about to be dropped wholesale, so no dec_ref bookkeeping is needed.
        let existing = self.scrollback_slim.len();
        let room = self.scrollback_capacity.saturating_sub(existing);
        let keep_after_room = other_slim.len().min(room);
        let front_drop = other_slim.len() - keep_after_room;
        if front_drop > 0 {
            other_slim.drain(0..front_drop);
            other_wrapped.drain(0..front_drop);
        }
        // Re-intern the remaining rows. The per-cell flag dispatch mirrors
        // `release_slim_row` so refcount accounting stays symmetric across
        // the SlimCell-flag union.
        let mut reinterned_rows: Vec<Vec<crate::slim_cell::SlimCell>> =
            Vec::with_capacity(keep_after_room);
        for slim_row in other_slim.into_iter() {
            let mut new_row = Vec::with_capacity(slim_row.len());
            for slim in slim_row {
                let entry = other_styles.get_or_default(slim.style_id);
                let new_style_id = self.styles.intern(entry);
                let new_char_ref = if slim.is_char_table() {
                    let s = other_chars.get_or_default(slim.char_ref);
                    self.chars.intern(s)
                } else {
                    // INLINE_ASCII (packed UTF-8 bytes) or WIDE_CONT
                    // (unused) — copy `char_ref` as-is; CharTable is
                    // not touched.
                    slim.char_ref
                };
                new_row.push(crate::slim_cell::SlimCell {
                    char_ref: new_char_ref,
                    width: slim.width,
                    flags: slim.flags,
                    style_id: new_style_id,
                });
            }
            reinterned_rows.push(new_row);
        }
        let wrapped: Vec<bool> = other_wrapped.into_iter().collect();
        self.prepend_scrollback_rows(reinterned_rows, wrapped)
        // `other_styles` / `other_chars` drop here, releasing every
        // refcount they held over the rows we just re-interned (and over
        // the rows we trimmed before re-interning).
    }
}
