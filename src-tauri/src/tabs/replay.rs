//! Replay / off-thread swap machinery for [`Tab`]: frame reset for a
//! replay payload, off-thread snapshot replay dispatch, pending-switch
//! polling, and the second-pass scrollback restore worker.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use term_core::terminal_core::ReplaySegment;

use super::{
    PendingScrollbackRestore, PendingSwitch, ScrollbackBuild, ScrollbackRestoreOutcome,
    SwapOutcome, Tab, drain_marks,
};

impl Tab {
    /// Rebuild the absolute-row frame from a replay payload.
    ///
    /// Shared by every code path that swaps in a fresh `term_core` frame:
    /// `Snapshot` / `SnapshotRestore` replay the daemon-captured bytes, and
    /// `PaneCreated` calls with an empty payload to reset the tab when a new
    /// mux window becomes active. Centralising the recipe keeps the
    /// callers in lockstep — adding a future field (e.g. another
    /// frame-keyed cache) only needs to land here, not at every site.
    ///
    /// Recipe:
    /// - clear prompts (they referenced the discarded frame's rows)
    /// - rebuild the fold manager with the current fold-enable preference
    /// - drop any in-flight custom-fold `begin` (belonged to the discarded frame)
    /// - lock the core, `reset_and_replay`, drain marks
    /// - update `evicted_baseline` and call `backfill_marks` so
    ///   `backfill_prompt_marks` latches `pending_frame_reset` (App::pump_all
    ///   reads that latch to drop the now-stale absolute-row selection /
    ///   press anchor — without this, a selection from the previous frame
    ///   addresses rows that no longer mean the same thing)
    ///
    /// The alt-screen state needs no reseed here: `term_core::reset` returns
    /// the core to the primary buffer and the replay re-derives the
    /// authoritative `MODE_ALT_SCREEN` bit, which `App::pump_all` reads
    /// directly each pump.
    ///
    /// Returns the mode actions accumulated during the replay so a caller
    /// (e.g. Snapshot's debug log) can use them.
    ///
    /// `segments` (task0004 round-4 rework D1'): structural dimension
    /// segments decoded from the wire payload
    /// (`mux_ipc::protocol::decode_snapshot_payload`) — the sole authority
    /// for which dimensions applied to which bytes of `payload`. An empty
    /// slice (a `PaneCreated` blank-reset call, or an older daemon's
    /// snapshot with no segment field) degrades to single-dimension replay
    /// (AC-11).
    pub(super) fn reset_frame_for_replay(
        &mut self,
        payload: &[u8],
        segments: &[ReplaySegment],
    ) -> Vec<u8> {
        self.reset_frame_prompts_folds();
        let (actions, evicted_total, pending_marks, pending_fold_marks) = {
            let mut c = self.core.lock();
            let actions = c.reset_and_replay_segments(payload, segments);
            // Discard any device responses (DA1 / DSR / XTWINOPS / …) that
            // historic queries baked into the snapshot bytes generated during
            // replay. The originating program is long gone; leaving the bytes
            // in `response_buffer` would let the next live `take_response`
            // (see `apply_active_pane_output`) deliver them to the live
            // shell's stdin, where zsh/zle interprets `\x1b[?` as an unbound
            // key-binding prefix and inserts the remaining `65;1;4;22c` at
            // the prompt on the user's first keystroke after the switch.
            let _ = c.take_response();
            let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
            (actions, evicted_total, pending_marks, pending_fold_marks)
        };
        self.apply_replay_reconcile(evicted_total, pending_marks, pending_fold_marks);
        actions
    }

    /// Frame-discard half of the replay recipe: drop the prompt / fold
    /// state that addressed the *outgoing* frame's rows. Shared by the
    /// synchronous [`Self::reset_frame_for_replay`] and the off-thread
    /// dispatch (which does this at dispatch time, before the worker has
    /// produced the new core, so the stale trackers never outlive the
    /// dispatch). The displayed core itself is NOT touched here — the
    /// off-thread path keeps showing the outgoing pane until the swap.
    fn reset_frame_prompts_folds(&mut self) {
        self.prompts.clear();
        self.folds = Self::new_fold_manager(self.fold_enabled);
        self.pending_fold_begin = None;
    }

    /// Dispatch an off-thread snapshot replay for `target_pane`: do the
    /// frame-discard portion now (so the stale prompt/fold trackers don't
    /// outlive the dispatch), read the displayed core's current grid size,
    /// spawn a one-shot worker that builds a fresh core at that grid and
    /// full-drain replays `payload`, and install the [`PendingSwitch`].
    ///
    /// The displayed core is deliberately NOT reset here — the outgoing pane
    /// stays on screen until `App::pump_all` swaps the worker-built core in.
    /// Replaces (supersedes) any prior in-flight switch on this tab targeting
    /// a DIFFERENT pane; the prior worker's result is dropped when its
    /// `done` sender is dropped with the old `PendingSwitch`.
    ///
    /// FR8 (task0003; task0006 redesign narrows this to same-pane
    /// SNAPSHOT dedup only — see `PendingSwitch::pending_resize` for the
    /// resize case, which no longer calls back in here): when
    /// `pending_switch` already targets the SAME `target_pane` — a second
    /// `Snapshot`/`SnapshotRestore` for the pane arriving before the first
    /// has swapped — this does NOT spawn a second worker right away. It
    /// cancels the in-flight one (as always) and stashes the request in
    /// `pending_redispatch` instead; `poll_pending_switch` installs a fresh
    /// worker for whichever request is LATEST there the next time it runs
    /// (the same pump tick). This collapses any number of same-pane
    /// duplicate snapshot fetches into exactly one actual build, so an
    /// intermediate, already-superseded fetch's replay is never paid for,
    /// and only the final request's replay ever completes. `Tab::resize`
    /// no longer calls this fn at all for a same-pane in-flight switch
    /// (review round-1 finding `64baa639d71792f9`) — see
    /// `PendingSwitch::pending_resize`'s doc for why re-dispatching a
    /// resize through here defeated the bypass split.
    pub(super) fn dispatch_offthread_replay(
        &mut self,
        target_pane: u32,
        payload: Vec<u8>,
        segments: Vec<ReplaySegment>,
    ) {
        // Supersede any in-flight worker: signal it to bail at the next chunk
        // boundary so workers do not pile up under a rapid switch / resize
        // storm. The old `PendingSwitch` (and its receiver) is dropped when
        // `self.pending_switch` is overwritten below (or, for the same-pane
        // coalesce case, when `poll_pending_switch` later takes it).
        let same_pane_in_flight = if let Some(old) = self.pending_switch.as_ref() {
            old.cancel.store(true, Ordering::Relaxed);
            old.target_pane == target_pane
        } else {
            false
        };
        if same_pane_in_flight {
            // FR8 (task0006 redesign, review round-1 findings
            // `7ed0ba7335376c20` / `ebc9de26bb15fcb1`): decide the
            // live_queue discard/keep question HERE, at coalesce time, not
            // later at poll time. `Tab::resize` no longer re-dispatches
            // through this branch (see `PendingSwitch::pending_resize`), so
            // every arrival here is a genuinely NEW `Snapshot`/
            // `SnapshotRestore` frame for the pane — matching the
            // pre-task0003 baseline where a same-pane snapshot replaced
            // `pending_switch` (and its `live_queue`) immediately,
            // synchronously, in the same call that decoded it. `pending_switch`
            // itself stays alive here (only the coalesced BUILD is
            // deferred to the next poll), so clearing the queue now — then
            // leaving `pending_switch` untouched — means any live output
            // arriving AFTER this point keeps accumulating correctly
            // against it, and `poll_pending_switch` can inherit whatever is
            // left unconditionally (no payload comparison needed, see that
            // fn's doc / review round-1 finding `5b1878c41d3e02d6`).
            if let Some(pending) = self.pending_switch.as_mut() {
                pending.live_queue.clear();
                pending.queued_bytes = 0;
            }
            self.pending_redispatch = Some((target_pane, payload, segments));
            return;
        }
        // A dispatch for a different pane (or no in-flight switch at all)
        // supersedes any coalesced same-pane request outright — it belonged
        // to a switch this tab is no longer completing.
        self.pending_redispatch = None;
        // FR5 / NFR4: a new off-thread switch makes any in-flight 2nd-pass
        // scrollback restore stale (the live core is about to be reset to a
        // different snapshot, so the rebuilt scrollback would be against an
        // unrelated baseline). Cancel + drop so the worker bails at the next
        // chunk boundary and the receiver is gone before this fn returns.
        if let Some(old) = self.pending_scrollback_restore.take() {
            old.cancel.store(true, Ordering::Relaxed);
            log::warn!(
                "scrollback restore cancelled (superseded by new switch) for tab {:?}",
                self.title
            );
        }
        self.reset_frame_prompts_folds();
        let (cols, rows, scrollback_lines) = {
            let c = self.core.lock();
            (c.cols(), c.rows(), c.scrollback_capacity())
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_payload = payload.clone();
        let worker_segments = segments.clone();
        // One-shot worker: pure build off the UI thread. `build_from_snapshot`
        // returns `None` if cancelled mid-parse — then there is nothing to
        // send. A successful build's `send` failure (receiver dropped because
        // the switch was superseded) is ignored. A panic inside the build
        // drops `tx`, which the main thread observes as `Err(Disconnected)`
        // and handles via the synchronous reparse fallback (FR7).
        let spawn_result = std::thread::Builder::new()
            .name("mux-snapshot-replay".into())
            .spawn(move || {
                if let Some(replay) = term_core::terminal_core::TerminalCore::build_from_snapshot(
                    cols,
                    rows,
                    scrollback_lines,
                    &worker_payload,
                    &worker_segments,
                    &worker_cancel,
                ) {
                    let _ = tx.send(replay);
                    // task0004 D4/AC-3: pull the event loop out of
                    // `ControlFlow::Wait` so `poll_pending_switch` observes
                    // this swap on the next `about_to_wait` pass instead of
                    // waiting for an unrelated event (input, PTY output on
                    // another tab, …). Mirrors the existing PTY reader
                    // thread's wake call in `pty::reader_loop`.
                    crate::wakeup::wake();
                }
            });
        match spawn_result {
            Ok(_) => {
                #[cfg(test)]
                {
                    self.offthread_spawn_count += 1;
                }
                self.pending_switch = Some(PendingSwitch {
                    target_pane,
                    cols,
                    rows,
                    done: rx,
                    live_queue: Vec::new(),
                    queued_bytes: 0,
                    payload,
                    segments,
                    cancel,
                    pending_resize: None,
                });
            }
            Err(e) => {
                // Spawn failure (thread/resource exhaustion) must not crash
                // the UI thread (the synchronous path it replaces never did).
                // Reparse synchronously now — a one-off block, accepted — and
                // install no pending switch. `reset_frame_prompts_folds` above
                // already cleared the trackers; `reset_frame_for_replay`
                // repeats that (a no-op on the now-empty state) plus replays.
                log::warn!(
                    "mux off-thread replay worker spawn failed ({e}); \
                     synchronous reparse fallback for tab {:?}",
                    self.title
                );
                self.reset_frame_for_replay(&payload, &segments);
                self.pending_switch = None;
            }
        }
    }

    /// Non-blockingly poll the in-flight off-thread snapshot replay and, when
    /// the worker has finished, swap the built core in, replay the queued
    /// target-pane live output in arrival order, and reconcile the
    /// absolute-row trackers. Called once per owning tab from
    /// `App::pump_all` (not gated to the active tab), so background-tab
    /// swaps apply too.
    ///
    /// Returns:
    /// - `SwapOutcome::Idle` — no pending switch.
    /// - `SwapOutcome::Pending` — worker still parsing; keep showing the
    ///   outgoing pane.
    /// - `SwapOutcome::Swapped` — the core was swapped + reconciled this
    ///   call; the caller drives the active-tab post-loop reconciliation
    ///   (per-pane scroll restore + selection-on-frame-reset + full redraw).
    /// - the fallback (worker panic) also returns `Swapped`: the latest
    ///   target is reparsed synchronously here (FR7), so from the caller's
    ///   perspective the swap completed this pump.
    ///
    /// FR8 (task0003; task0006 redesign): before touching the in-flight
    /// worker's channel at all, install a fresh worker for any coalesced
    /// `pending_redispatch` (a duplicate-snapshot re-dispatch stashed by
    /// `dispatch_offthread_replay`'s same-pane branch) — the in-flight
    /// worker this supersedes is dropped without ever being observed,
    /// whether it would have completed or disconnected.
    pub(crate) fn poll_pending_switch(&mut self) -> SwapOutcome {
        if let Some((target_pane, payload, segments)) = self.pending_redispatch.take() {
            let (queued, queued_bytes) = match self.pending_switch.take() {
                Some(old) => {
                    old.cancel.store(true, Ordering::Relaxed);
                    // FR7/FR8 (task0006 redesign, review round-1 findings
                    // `7ed0ba7335376c20` / `5b1878c41d3e02d6`): the
                    // discard/keep decision for `live_queue` was already
                    // made at COALESCE time
                    // (`dispatch_offthread_replay`'s same-pane branch
                    // clears it there), so `old.live_queue` here already
                    // holds exactly "output queued since the last
                    // coalesce" — always safe to inherit unconditionally.
                    // This also removes the O(n) full-payload byte
                    // comparison round-1's fix required here on every
                    // poll (finding `5b1878c41d3e02d6`); the invariant
                    // that `pending_redispatch`'s pane always matches
                    // `pending_switch`'s (it is only ever populated by the
                    // same-pane coalesce branch) is asserted rather than
                    // branched on.
                    debug_assert_eq!(
                        old.target_pane, target_pane,
                        "pending_redispatch's pane must match pending_switch's \
                         (dispatch_offthread_replay invariant)"
                    );
                    (old.live_queue, old.queued_bytes)
                }
                None => (Vec::new(), 0),
            };
            // `self.pending_switch` is `None` here, so this dispatch always
            // takes the "install a fresh worker" path below, never the
            // same-pane coalesce branch (which would otherwise loop back
            // into `pending_redispatch` forever).
            self.dispatch_offthread_replay(target_pane, payload, segments);
            return match self.pending_switch.as_mut() {
                Some(p) => {
                    p.live_queue = queued;
                    p.queued_bytes = queued_bytes;
                    SwapOutcome::Pending
                }
                // Rare: the worker thread failed to spawn and
                // `dispatch_offthread_replay`'s own fallback already
                // reparsed synchronously and applied it — the switch is
                // already visually complete this pump. The queued live
                // output predating this re-dispatch has no home to land in
                // (mirrors the pre-existing gap in `Tab::resize`'s
                // redispatch branch on the same rare spawn-failure path).
                None => SwapOutcome::Swapped,
            };
        }
        let Some(pending) = self.pending_switch.as_ref() else {
            return SwapOutcome::Idle;
        };
        match pending.done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => SwapOutcome::Pending,
            Ok(replay) => {
                // Take ownership of the queued live output + payload before
                // dropping the pending state.
                let pending = self.pending_switch.take().expect("just matched Some");
                self.apply_offthread_swap(
                    replay,
                    pending.live_queue,
                    pending.payload,
                    pending.segments,
                    pending.pending_resize,
                );
                SwapOutcome::Swapped
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // FR7: the worker panicked. Reparse the latest target's
                // snapshot synchronously via the legacy path (a one-off
                // main-thread block, accepted), then apply the queued live
                // output as ordinary output so nothing is lost.
                log::warn!(
                    "mux off-thread replay worker for tab {:?} disconnected; \
                     falling back to synchronous reparse",
                    self.title
                );
                let pending = self.pending_switch.take().expect("just matched Some");
                self.reset_frame_for_replay(&pending.payload, &pending.segments);
                self.apply_queued_live_output(pending.live_queue);
                SwapOutcome::Swapped
            }
        }
    }

    /// Swap the worker-built `replay.core` into this tab, replay `live_queue`
    /// in arrival order, and reconcile the absolute-row trackers so the
    /// result is identical to a contiguous synchronous parse of
    /// `snapshot ++ live`.
    ///
    /// The core is replaced *inside* the existing `Arc<Mutex<…>>` (not the
    /// `Arc` itself) so the renderer's shared handle stays valid. The
    /// snapshot's drained marks/actions/eviction (captured by the worker
    /// from a freshly-reset core, counter 0) reconcile exactly like the
    /// synchronous `reset_frame_for_replay`; the live output is then
    /// backfilled at its own post-replay eviction total so an eviction that
    /// happened while applying the queue shifts the snapshot marks down by
    /// the right delta.
    ///
    /// `pending_resize` (FR7, task0006 redesign, review round-1 finding
    /// `64baa639d71792f9`) is `PendingSwitch::pending_resize`: a grid
    /// resize that raced this switch, deferred by `Tab::resize` rather than
    /// forcing a re-dispatch at the new target (which would have defeated
    /// the bypass split — the payload's own recorded segments reflect the
    /// worker's ORIGINAL dispatch-time target, never a resize that landed
    /// after the fact). Applied here, on the freshly-swapped core, via the
    /// same already-bypass-aware `TerminalCore::resize` an ordinary
    /// interactive resize uses (see `TerminalCore::resize`'s own handling
    /// of `scrollback_bypass`) — so it costs (and behaves) exactly as if
    /// the user had resized right after an unraced switch landed, BEFORE
    /// the queued live output (which was produced with the daemon already
    /// aware of the new grid, since `Tab::resize` broadcasts the `Resize`
    /// control frame unconditionally, before this switch's swap) is
    /// replayed onto it.
    fn apply_offthread_swap(
        &mut self,
        replay: term_core::terminal_core::SnapshotReplay,
        live_queue: Vec<Vec<u8>>,
        payload: Vec<u8>,
        segments: Vec<ReplaySegment>,
        pending_resize: Option<(u16, u16)>,
    ) {
        // Move out the pre-captured B-mark texts BEFORE partial-moving
        // `replay.core` (field ordering matters for partial moves).
        let bypass_b_mark_texts = replay.bypass_b_mark_texts;
        // D3' (task0004 round-4 rework, review round-3 finding
        // `b235e4dbc61cc4ba`): whether THIS 1st-pass replay already
        // populated `scrollback_slim` — either because the bypass was off
        // to begin with, or because `build_from_snapshot_inner`'s D6 guard
        // downgraded out of the bypass for this payload (a row-count-growing
        // segment transition). Captured before the partial move below.
        let scrollback_populated = replay.scrollback_populated;
        // 1. Swap the built core in (renderer's Arc stays valid), transplanting
        //    the pre-swap wiring onto it FIRST so the live core is never
        //    observable (even momentarily, under this same lock) without its
        //    callbacks / app-layer OSC registration:
        //      - the old core's `callbacks` moves onto the worker-built core.
        //        An old core with no callbacks (edge case) yields
        //        `new_core.callbacks = None` — already `TerminalCore::new`'s
        //        default, so no panic.
        //      - the mux inband OSC param is re-registered on the new core
        //        with the same call `Tab::new` makes, so the swapped-in core
        //        ends up behaviorally identical to a never-swapped tab core.
        {
            let mut live = self.core.lock();
            let mut new_core = replay.core;
            new_core.callbacks = live.callbacks.take();
            new_core.register_osc_app_param(
                mux_ipc::protocol::MUX_OSC_PARAM,
                crate::callbacks::OSC_MUX_INBAND,
            );
            *live = new_core;
            // Discard any device responses (DA1 / DSR / XTWINOPS / …) left
            // pending on the worker-built core before it goes live —
            // explicit, call-site-local mirror of the synchronous
            // `reset_frame_for_replay`'s discard (tmux-startup-query-
            // response-leak task0003, review round-1 finding
            // `8bebc1e532a1b597`). As of this task, `TerminalCore::
            // build_from_snapshot_inner` already drains `response_buffer`
            // before returning `SnapshotReplay` (see that function, and
            // commit `4380805c`), so this call observes an already-empty
            // buffer and is a no-op today; it is kept so the invariant
            // "the off-thread swap never delivers a replay-generated
            // response" is asserted explicitly at THIS call site rather
            // than depended on implicitly via term_core internals — a
            // future change to the worker's build (e.g. the ordered
            // multi-response store, IMPLEMENTATION.md D5) cannot silently
            // reopen this leak without also changing this line.
            let _ = live.take_response();
            // FR7 (task0006 redesign): apply a resize that raced this
            // switch, now that the built core is in place. See this fn's
            // doc and `PendingSwitch::pending_resize` for why this is
            // deferred to here instead of being baked into the worker's
            // own build target.
            if let Some((rcols, rrows)) = pending_resize {
                if (live.cols(), live.rows()) != (rcols, rrows) {
                    live.resize(rcols, rrows);
                }
            }
        }
        // 2. Stash the bypass texts so `backfill_prompt_marks` (called
        //    from inside `apply_replay_reconcile`) can populate
        //    `resolved_b_mark_texts` for each B mark it processes.
        self.pending_bypass_b_mark_texts = bypass_b_mark_texts;
        // 3. Reconcile the snapshot half first: install the fresh baseline,
        //    latch the frame reset, backfill the snapshot's marks.
        //    (Frame-discard of prompts/folds already happened at dispatch
        //    time in `dispatch_offthread_replay`; the alt-screen state is the
        //    core's MODE_ALT_SCREEN bit, read directly by App::pump_all.)
        self.apply_replay_reconcile(replay.evicted_total, replay.prompt_marks, replay.fold_marks);
        // 4. Clear pending_bypass_b_mark_texts now that the snapshot reconcile
        //    has consumed it. resolved_b_mark_texts is intentionally kept: it
        //    holds snapshot-era B mark texts that live D marks (arriving in step
        //    5) still need to look up via register_osc133_fold_region_at_idx.
        //    Row collisions are handled in backfill_prompt_marks: a live B mark
        //    on the same abs_row evicts the stale snapshot-era entry, so live
        //    always wins on collision without clearing the whole map here.
        self.pending_bypass_b_mark_texts.clear();
        // 5. Apply the queued live output in order, as ordinary post-snapshot
        //    output (NOT a reset). This re-runs the same drain/backfill the
        //    `PtyOutput` arm would have, so prompt/fold marks and eviction
        //    arriving during the gap are honored. The bypass maps are now empty
        //    so live B marks go through the normal scrollback lookup path.
        self.apply_queued_live_output(live_queue);
        // 6. Spawn the 2nd-pass scrollback restore worker (bypass-off
        //    rebuild) — but ONLY if the 1st-pass replay did NOT already
        //    populate scrollback (D3', review round-3 finding
        //    `b235e4dbc61cc4ba`). Spawning it unconditionally after a replay
        //    that already populated `scrollback_slim` (the D6 bypass
        //    downgrade, or a bypass-off build to begin with) would prepend
        //    the SAME history a second time via `apply_scrollback_restore`'s
        //    merge, duplicating it up to the ring's full capacity. This runs
        //    the same parse off-thread without the SlimCell compression
        //    bypass so `scrollback_slim` ends up populated;
        //    `apply_scrollback_restore` later merges that into the live
        //    core. We supersede any prior in-flight restore on this tab
        //    (NFR4 — one in-flight 2nd-pass per tab); the prior worker
        //    observes cancel at the next chunk boundary.
        if scrollback_populated {
            log::debug!(
                "1st-pass replay already populated scrollback for tab {:?}; \
                 skipping the 2nd-pass restore worker (D3')",
                self.title
            );
        } else {
            self.spawn_scrollback_restore(payload, segments);
        }
    }

    /// Best-effort cancellation of any in-flight 2nd-pass scrollback restore
    /// worker. Sets the worker's shared `cancel` flag so it bails at the next
    /// chunk boundary — drop alone does NOT fire the flag (the worker holds
    /// an `Arc<AtomicBool>` independently of the receiver). Used by the
    /// `window_host.rs` `CloseRequested` shutdown sweep before
    /// `self.app.tabs.clear()` drops the receivers; bounds wasted worker CPU
    /// on shutdown. No-op when no restore is in flight.
    pub(crate) fn cancel_pending_scrollback_restore(&self) {
        if let Some(p) = self.pending_scrollback_restore.as_ref() {
            p.cancel.store(true, Ordering::Relaxed);
            log::info!(
                "scrollback restore cancelled (shutdown) for tab {:?}",
                self.title
            );
        }
    }

    /// Non-blockingly poll the 2nd-pass scrollback restore handoff (FR4,
    /// NFR3, NFR7). Mirror of [`Self::poll_pending_switch`] but for the
    /// bypass-off scrollback rebuild.
    ///
    /// Returns one of [`ScrollbackRestoreOutcome`]:
    /// - `Idle` — no restore is in flight.
    /// - `Pending` — worker is still rebuilding (do not block).
    /// - `Merged` — the rebuilt scrollback was merged into the live core;
    ///   the caller marks the tab `changed` and (for the active tab)
    ///   `active_changed` (search overlay rebuild).
    /// - `Failed` — the worker disconnected (panic) or the cancel arm
    ///   observed `Disconnected` after a supersede; state is cleared.
    pub(crate) fn poll_pending_scrollback_restore(&mut self) -> ScrollbackRestoreOutcome {
        let Some(pending) = self.pending_scrollback_restore.as_ref() else {
            return ScrollbackRestoreOutcome::Idle;
        };
        match pending.done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => ScrollbackRestoreOutcome::Pending,
            Ok(build) => {
                let pending = self
                    .pending_scrollback_restore
                    .take()
                    .expect("just matched Some");
                self.apply_scrollback_restore(build, pending.base_evicted_total);
                ScrollbackRestoreOutcome::Merged
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // FR7: worker panicked or was cancelled mid-parse (the
                // `build_scrollback_only_from_snapshot` returned `None`, so
                // it never sent and the sender dropped). No synchronous
                // fallback — the 1st-pass swap is already correct, the user
                // just sees no history. Clear state.
                log::warn!(
                    "scrollback restore worker for tab {:?} disconnected; clearing state",
                    self.title
                );
                self.pending_scrollback_restore = None;
                ScrollbackRestoreOutcome::Failed
            }
        }
    }

    /// Merge the rebuilt scrollback into the live core (FR3 + FR8).
    ///
    /// FR3: between the 1st-pass swap and the 2nd-pass arrival, live PTY
    /// output may have pushed some rows into the (initially empty) live
    /// scrollback and evicted others. Those rows were ALREADY present at
    /// the tail of the rebuilt scrollback, so prepending the whole rebuilt
    /// scrollback would duplicate them. The fix: trim the trailing
    /// `live_growth = live_now - base_evicted_total` rows from the rebuilt
    /// scrollback before merging — those tail rows are the ones the live
    /// drain re-emitted from the snapshot tail.
    ///
    /// FR8: the merge consumes only the rebuilt scrollback (slim cells +
    /// wrapped + tables) — `prompt_marks`, `fold_marks`, and
    /// `bypass_b_mark_texts` from the 2nd-pass replay are intentionally
    /// dropped without touching the live core's mark trackers. Marks were
    /// already drained from the 1st-pass replay in `apply_replay_reconcile`
    /// and from the queued live output in `apply_queued_live_output`; the
    /// 2nd-pass would emit the same marks a second time, which is exactly
    /// what FR8 forbids. Discarding the 2nd-pass marks here is the
    /// implementation of the mark-non-duplication invariant.
    fn apply_scrollback_restore(&mut self, build: ScrollbackBuild, base_evicted_total: u64) {
        let rebuilt_evicted_at_end = build.evicted_total_at_end;
        // FR3 trim arithmetic + merge happen inside a single lock window so
        // a concurrent `pump` cannot race with the scrollback length read.
        let (merged_rows, live_growth, live_now) = {
            let mut live = self.core.lock();
            let live_now = live.get_scrollback_evicted_total();
            let live_growth = live_now.saturating_sub(base_evicted_total) as usize;
            let merged = live.merge_scrollback_from(build.rebuilt_core, live_growth);
            (merged, live_growth, live_now)
        };
        log::info!(
            "scrollback restored for tab {:?}: {merged_rows} rows prepended \
             (live_growth={live_growth}, base_evicted_total={base_evicted_total}, \
              live_now={live_now}, rebuilt_evicted={rebuilt_evicted_at_end})",
            self.title
        );
    }

    /// Spawn the 2nd-pass scrollback restore worker (FR1, NFR3, NFR7).
    /// Captures `base_evicted_total` from the now-settled live core, clones
    /// the payload, spawns a worker thread that calls
    /// `build_scrollback_only_from_snapshot`, and installs
    /// `PendingScrollbackRestore`. On spawn failure: `log::warn` + no state
    /// installed (FR7 — the 1st-pass swap is already correct, the user just
    /// gets no history).
    fn spawn_scrollback_restore(&mut self, payload: Vec<u8>, segments: Vec<ReplaySegment>) {
        // Supersede any prior in-flight restore (NFR4) — the freshly-swapped
        // core is the new authoritative state, the prior restore's rebuilt
        // scrollback would be against a now-stale baseline.
        if let Some(old) = self.pending_scrollback_restore.as_ref() {
            old.cancel.store(true, Ordering::Relaxed);
            log::warn!(
                "scrollback restore cancelled (superseded by newer off-thread swap) for tab {:?}",
                self.title
            );
        }
        let (cols, rows, scrollback_lines, base_evicted_total) = {
            let c = self.core.lock();
            (
                c.cols(),
                c.rows(),
                c.scrollback_capacity(),
                c.get_scrollback_evicted_total(),
            )
        };
        if scrollback_lines == 0 {
            log::info!(
                "scrollback restore skipped (scrollback disabled) for tab {:?}",
                self.title
            );
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_payload = payload;
        let worker_segments = segments;
        let payload_len = worker_payload.len();
        let spawn_result = std::thread::Builder::new()
            .name("mux-scrollback-restore".into())
            .spawn(move || {
                if let Some(replay) =
                    term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
                        cols,
                        rows,
                        scrollback_lines,
                        &worker_payload,
                        &worker_segments,
                        &worker_cancel,
                    )
                {
                    let _ = tx.send(ScrollbackBuild {
                        rebuilt_core: replay.core,
                        evicted_total_at_end: replay.evicted_total,
                    });
                    // task0004 D4/AC-3: same rationale as the snapshot-replay
                    // worker above — wake the loop so
                    // `poll_pending_scrollback_restore` observes the merge
                    // promptly under `ControlFlow::Wait`.
                    crate::wakeup::wake();
                }
            });
        match spawn_result {
            Ok(_) => {
                log::info!(
                    "scrollback restore worker spawned for tab {:?}, payload {payload_len} B",
                    self.title
                );
                self.pending_scrollback_restore = Some(PendingScrollbackRestore {
                    done: rx,
                    base_evicted_total,
                    cancel,
                });
            }
            Err(e) => {
                // FR7: thread/resource exhaustion at spawn is non-fatal; the
                // 1st-pass swap is already correct, the user just gets no
                // scrollback restored. The state is intentionally not
                // installed so the next poll observes Idle.
                log::warn!(
                    "scrollback restore worker spawn failed ({e}) for tab {:?}; \
                     scrollback will not be restored",
                    self.title
                );
            }
        }
    }

    /// Replay a pending switch's queued live output onto the (already
    /// swapped or reparsed) displayed core, in arrival order, exactly as the
    /// `PtyOutput` arm would have for each chunk: feed the bytes, route any
    /// device response, backfill marks.
    pub(super) fn apply_queued_live_output(&mut self, live_queue: Vec<Vec<u8>>) {
        for payload in live_queue {
            let (evicted_total, prompt_marks, fold_marks, device_response) = {
                let mut c = self.core.lock();
                c.process_pty_data_fully(&payload);
                let device_response = c.take_response();
                let (evicted_total, prompt_marks, fold_marks) = drain_marks(&mut c);
                (evicted_total, prompt_marks, fold_marks, device_response)
            };
            if !device_response.is_empty() {
                self.write_device_response(device_response);
            }
            self.backfill_marks(evicted_total, prompt_marks, fold_marks);
        }
    }

    /// Main-thread reconcile half of the replay recipe, shared by the
    /// synchronous path and the off-thread swap. Given the marks/eviction
    /// total drained from the *replayed* core (the synchronous core for the
    /// sync path, the worker-built core for the off-thread path), latch
    /// `pending_frame_reset`, install the fresh `evicted_baseline`, and
    /// backfill the marks. The alt-screen state is the core's authoritative
    /// `MODE_ALT_SCREEN` bit (read by `App::pump_all`), so it needs no reseed
    /// here.
    ///
    /// The eviction total comes from a freshly-reset core (counter 0), so
    /// `backfill_prompt_marks`'s in-band detector
    /// (`evicted_total < self.evicted_baseline`) cannot fire — the latch is
    /// set unconditionally because the helper's contract is "the previous
    /// frame was discarded" regardless of eviction counts.
    fn apply_replay_reconcile(
        &mut self,
        evicted_total: u64,
        prompt_marks: Vec<term_core::terminal_core::PendingPromptMark>,
        fold_marks: Vec<term_core::terminal_core::PendingFoldMark>,
    ) {
        self.pending_frame_reset = true;
        self.evicted_baseline = evicted_total;
        self.backfill_marks(evicted_total, prompt_marks, fold_marks);
    }
}
