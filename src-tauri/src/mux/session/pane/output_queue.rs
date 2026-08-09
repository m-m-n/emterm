//! PTY output chunking: chunk kinds, the bounded pane-output channel,
//! and the deferred-output queue that absorbs bursts while a pane is
//! detached or backpressured.

use super::*;
/// Discriminator on `PtyOutputChunk` distinguishing live PTY bytes from a
/// `RequestPaneSnapshot` reply payload routed through the same per-pane
/// channel.
///
/// The connection drain (`mux::ipc::connection`) inspects `kind` when
/// encoding each chunk: `PtyOutput` produces a `MessageType::PtyOutput`
/// frame (the historical default), `Snapshot` produces a
/// `MessageType::Snapshot` frame so the client dispatches it to the
/// `apply_mux_message::Snapshot|SnapshotRestore` arm and the
/// `build_from_snapshot` + `scrollback_bypass` fast path.
///
/// Routing the snapshot reply through the existing `pane_output_tx`
/// channel preserves the FIFO ordering invariant documented at
/// `handle_request_pane_snapshot` (handlers.rs:394-414): bytes already
/// queued before the snapshot stay before it on the wire, bytes queued
/// after stay after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    /// Default: a live PTY-output chunk from the reader thread or the
    /// resume / reattach paths.
    PtyOutput,
    /// A `RequestPaneSnapshot` reply payload assembled by
    /// `handle_request_pane_snapshot`. Drained as `MessageType::Snapshot`.
    Snapshot,
}

/// PTY output chunk sent from the reader thread to the mux writer.
///
/// `kind` defaults to `ChunkKind::PtyOutput` via the `pty_output(...)`
/// constructor; only `handle_request_pane_snapshot` should call
/// `PtyOutputChunk::snapshot(...)` to produce a `Snapshot`-tagged chunk.
pub struct PtyOutputChunk {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
    pub kind: ChunkKind,
}

impl PtyOutputChunk {
    /// Construct a live PTY output chunk (default `kind == PtyOutput`).
    ///
    /// Used by the reader thread (`pty_spawn`), the resume path
    /// (`resume_pane_with_permit`), the reattach path, and the PTY-exit
    /// signal (empty `data`).
    pub fn pty_output(pane_id: PaneId, data: Vec<u8>) -> Self {
        Self {
            pane_id,
            data,
            kind: ChunkKind::PtyOutput,
        }
    }

    /// Construct a snapshot-reply chunk (`kind == Snapshot`).
    ///
    /// Only `handle_request_pane_snapshot` should use this constructor:
    /// the drain loop in `mux::ipc::connection` will encode the chunk as
    /// a `MessageType::Snapshot` frame, routing it to the client's
    /// `apply_mux_message::Snapshot|SnapshotRestore` arm.
    pub fn snapshot(pane_id: PaneId, data: Vec<u8>) -> Self {
        Self {
            pane_id,
            data,
            kind: ChunkKind::Snapshot,
        }
    }
}

/// Bounded channel capacity for PTY output per pane.
pub const PTY_CHANNEL_CAPACITY: usize = 256;

/// Bound on how many [`DeferredOutputItem::Chunk`] entries the
/// connection-owned [`DeferredOutputQueue`] retains while `pane_output_tx`
/// is momentarily full (mux-window-switch-output-hang task0002 rework,
/// AC-3/FR4). `PTY_CHANNEL_CAPACITY` bounds the channel itself; this bounds
/// the "equivalent bound elsewhere" FR4 requires for the deferred path,
/// which task0001's `tokio::spawn`-per-item design left entirely unbounded
/// (reviews/round1.yaml F6-F9).
///
/// task0003 rework (review round 2, findings `4999311c8becf7eb` /
/// `ac1d20218d320b08`): this cap now bounds `Chunk` entries ONLY.
/// [`DeferredOutputItem::VisibilityResume`] entries are deduplicated by
/// pane id instead (see `DeferredOutputQueue::defer_visibility_resume`) and
/// are NOT subject to this cap — round 2 found that sharing one cap let a
/// session with more non-exited panes than `MAX_DEFERRED_ITEMS` strand the
/// overflow panes `Detached` forever (no client-driven retry exists for a
/// dropped resume, unlike a `Chunk`, which the client can re-request).
///
/// Deliberately small: a full channel means the client (or the connection
/// itself) is not draining fast enough, so retaining a long backlog of
/// deferred chunks here would just move the memory-growth problem sideways
/// instead of bounding it. `Chunk` items retain their already-built payload
/// (up to a few MiB for a scrollback-bearing snapshot — see
/// `DEFAULT_SCROLLBACK_CAPACITY`), so keeping this cap small keeps the worst
/// case a low multiple of that rather than an open-ended backlog.
pub const MAX_DEFERRED_ITEMS: usize = 8;

/// One item held in the connection-owned [`DeferredOutputQueue`] while
/// `pane_output_tx` is momentarily full.
pub enum DeferredOutputItem {
    /// An already-built chunk (e.g. a `RequestPaneSnapshot` reply) that
    /// could not be `try_send`'d immediately. Retried verbatim on the next
    /// flush — its content does not depend on when it is actually
    /// delivered, only that it lands before anything deferred after it.
    Chunk(PtyOutputChunk),
    /// A visibility-resume attempt for this pane that could not get a
    /// permit immediately. Deliberately NOT a pre-built chunk: the resume
    /// snapshot must reflect the pane's (and the connection's
    /// `visible_state`'s) state at the moment capacity actually frees, not
    /// at the moment the request was deferred — see
    /// `handlers::flush_deferred_output`'s re-validation (AC-1/F1/F2/F3).
    VisibilityResume(PaneId),
}

impl std::fmt::Debug for DeferredOutputItem {
    /// Manual impl (rather than `#[derive(Debug)]`) so logging a dropped
    /// item never dumps a `Chunk`'s raw payload bytes — only its size.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeferredOutputItem::Chunk(c) => f
                .debug_struct("Chunk")
                .field("pane_id", &c.pane_id)
                .field("kind", &c.kind)
                .field("bytes", &c.data.len())
                .finish(),
            DeferredOutputItem::VisibilityResume(pane_id) => {
                f.debug_tuple("VisibilityResume").field(pane_id).finish()
            }
        }
    }
}

/// Connection-owned, explicitly-bounded FIFO of [`DeferredOutputItem`]s
/// awaiting capacity on a connection's `pane_output_tx`
/// (mux-window-switch-output-hang task0002 rework).
///
/// ### Why connection-owned (not a spawned task per item)
///
/// task0001's fix deferred capacity-waiting to an independently
/// `tokio::spawn`ed task per full-channel occurrence. Review round 1 found
/// three problems, all stemming from moving the wait off the connection
/// task entirely:
/// - the spawned task is not polled by the runtime the instant it is
///   spawned, so a producer that starts a plain (non-waiting) `try_send`
///   after the deferral can land ahead of it once capacity frees, breaking
///   order relative to already-built chunks (F4/F5);
/// - a visibility-resume spawned task re-validated nothing when it finally
///   ran, so a pane hidden again in the interim could be resumed to
///   `Connected` incorrectly (F1/F2/F3);
/// - nothing bounded how many such tasks (and, for the chunk path, how many
///   multi-MiB payloads) could be in flight at once (F6-F9).
///
/// Holding deferred items HERE instead — retried only from the connection's
/// own event loop (`handlers::flush_deferred_output`, called right after the
/// loop's own drain of `pane_output_rx`, the ONLY place capacity on this
/// channel is ever freed) — fixes all three: the retry happens on the very
/// next opportunity after capacity frees (no scheduler-dependent gap),
/// re-validation happens fresh at that moment (not baked in at defer time),
/// and the queue has a hard cap on `Chunk` backlog (`MAX_DEFERRED_ITEMS`,
/// task0003 rework: `VisibilityResume` entries are deduplicated by pane id
/// instead of sharing that cap — see `defer_visibility_resume`).
///
/// ### Overflow policy (AC-1/AC-2, task0003 rework of review round 2
/// findings `4999311c8becf7eb` / `ff58ab6fd17542f4` / `ac1d20218d320b08` /
/// `1d648d947b4dea8b`)
///
/// task0002's policy dropped whichever item arrived NEWEST once
/// `MAX_DEFERRED_ITEMS` was reached, applied uniformly to both item kinds.
/// Review round 2 found this actively harmful for both:
/// - a dropped `VisibilityResume` has no client-driven retry: a repeated
///   `SetVisibility(true)` is a no-op (`handle_set_visibility`'s
///   `prev == visible` guard), so the pane stayed `Detached` — silently
///   reintroducing the "output stops flowing" symptom this feature exists
///   to remove;
///   - a dropped `Chunk` (always a `RequestPaneSnapshot` reply in
///     production) discarded the client's MOST RECENT request in favour of
///     up to `MAX_DEFERRED_ITEMS` older ones — the worst possible choice
///     when the client just switched to a new window.
///
/// `defer_visibility_resume` now deduplicates by pane id and is never
/// capped (see its own doc for why that is still bounded in practice).
/// `defer_chunk` coalesces a new chunk over any existing queued chunk FOR
/// THE SAME PANE (a snapshot reply is fully self-contained, so only the
/// newest one for a given pane matters) — assigned IN PLACE so the queue
/// POSITION never changes (task0004 rework, AC-5, review round 3 finding
/// `0830abe1c16ad0fb`: task0003's `remove` + `push_back` moved a coalesced
/// chunk to the tail, breaking the FIFO invariant this doc declares) — and
/// only evicts across DIFFERENT panes' chunks once `MAX_DEFERRED_ITEMS`
/// distinct panes are queued — evicting the OLDEST such entry, never the
/// one just pushed. Both paths are SPEC-sanctioned (SPEC.md FR3, task0005
/// rework, G3/AC-3, review round 4 finding `329f746349f592e8`): same-pane
/// coalescing at ANY queue length, and this distinct-pane eviction once
/// `MAX_DEFERRED_ITEMS` distinct panes are pending (task0004 rework,
/// G3/AC-3 option (a); review round 3 finding `b4eee6700d643640`). FR3's
/// guarantee is "one snapshot per pane, reflecting its newest request, is
/// delivered", not a 1:1 request/reply correspondence — replacing a
/// still-queued, now-superseded reply with a newer one is not a dropped
/// delivery under that guarantee. Recovery for the distinct-pane eviction
/// case is the client re-issuing `RequestPaneSnapshot` on its next switch
/// to the evicted pane.
///
/// ### Ordering (AC-2/AC-3/F4/F5): what is, and is not, guaranteed
///
/// Items are flushed strictly in the order they were deferred — a plain
/// FIFO; the flush loop stops at the first item that still can't be sent
/// rather than skipping ahead, so a later item can never overtake an
/// earlier one FROM THIS QUEUE. Because the flush runs synchronously in the
/// same connection task, immediately upon the only event that can free
/// capacity, a deferred item is never overtaken by another producer that
/// has to go through this same connection's own scheduling to get its turn
/// (e.g. a later `RequestPaneSnapshot` on the same connection).
///
/// This does NOT extend to the PTY reader thread: `pty_spawn.rs`'s reader
/// runs on its own native OS thread and calls `try_send` / `blocking_send`
/// on `pane_output_tx` directly, entirely outside this queue. Correction
/// (task0003, AC-6): the pre-task0001 code did NOT have this same race in
/// equivalent form — its blocking `.send().await` joined tokio's semaphore
/// waiter queue at request time, so it was serviced fairly (FIFO) relative
/// to the reader thread's own `blocking_send` waiters (it deadlocked for an
/// entirely different reason: THAT wait itself could only ever be ended by
/// this same task's own drain arm, which cannot run while suspended there).
/// task0001/task0002's `try_send`/`try_reserve`-only retries do NOT join
/// that waiter queue at all, so while a reader-thread send is parked there,
/// every freed permit is handed to it directly and `try_send` observes zero
/// capacity essentially always — a systematic priority inversion (review
/// round 2, findings `7e47bd5fe31dc720` / `2aec511b92102c24`), not the
/// occasional race the task0002 doc previously described here.
///
/// task0003 closes the STARVATION half of that (AC-3): `mux::ipc::connection`
/// additionally polls `pane_output_tx.clone().reserve_owned()` as its own
/// `select!` arm whenever this queue is non-empty (listed BEFORE the
/// channel-drain arm in that `biased` `select!` — see that arm's doc for
/// why the ordering itself is load-bearing). That future joins the SAME
/// FIFO waiter queue every reader thread's `blocking_send` does (one per
/// pane sharing this connection's `pane_output_tx`), so a deferred item is
/// serviced within a bounded number of reader-thread sends — bounded by
/// however many reader threads this connection's panes have, all of which
/// terminate once their PTY exits — rather than indefinitely, without ever
/// blocking the connection task itself (a `select!` arm's future is only
/// ever polled, never awaited to completion outside the macro).
///
/// What task0003 does NOT close: the STRICT ordering guarantee against the
/// reader thread. Even with fair queuing, a live chunk the reader thread
/// produced AFTER a snapshot was requested can still be granted a permit
/// (and thus reach the client) ahead of that deferred snapshot, if the
/// reader's send happened to already be queued first. SPEC.md FR3 is
/// narrowed to say so explicitly (task0003 AC-6) — the guarantee that
/// survives is "one snapshot per pane, reflecting its newest request,
/// delivered within bounded time" (task0005 rework, G3/AC-3), not "never
/// reordered relative to the reader thread". Closing the ordering gap fully
/// would
/// require either routing the reader thread's sends through this same
/// queue (a `pty_spawn.rs` change, out of this task's scope) or a
/// client-observable generation number so a stale snapshot can be discarded
/// on arrival (a wire-protocol change in the `mux_ipc` crate, also out of
/// scope).
pub struct DeferredOutputQueue {
    items: std::collections::VecDeque<DeferredOutputItem>,
}

impl DeferredOutputQueue {
    pub fn new() -> Self {
        Self {
            items: std::collections::VecDeque::new(),
        }
    }

    /// Number of items currently queued (test/observability hook).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Defer an already-built chunk (see [`DeferredOutputItem::Chunk`]).
    ///
    /// task0003 rework (AC-2, review round 2 findings `4999311c8becf7eb` /
    /// `ac1d20218d320b08`): a chunk here is always a `RequestPaneSnapshot`
    /// reply in production, which is fully self-contained — only the NEWEST
    /// one for a given pane still matters to the client that asked for it.
    /// So a new chunk for a pane already resident in the queue REPLACES it
    /// (coalesce, newest wins) instead of adding a second entry. Only once
    /// coalescing still leaves `MAX_DEFERRED_ITEMS` DISTINCT panes' chunks
    /// queued does capacity pressure evict anything — and then it evicts the
    /// OLDEST surviving chunk, never the one just pushed (AC-2 forbids ever
    /// dropping the newest). If a [`DeferredOutputItem::VisibilityResume`]
    /// for this pane is ALREADY queued and no `Chunk` entry exists for it
    /// yet, the new chunk is INSERTED immediately BEFORE that Resume rather
    /// than dropped (mux-window-switch-output-hang task0006 rework, review
    /// round 5 high findings `4043ee676f69ca15` / `1c8d86389ab4bf40`,
    /// reverting task0005's drop-instead-of-queue fix — see the insertion
    /// site below for why that fix's premise did not hold).
    pub fn defer_chunk(&mut self, chunk: PtyOutputChunk) {
        let pane_id = chunk.pane_id;
        if let Some(pos) = self
            .items
            .iter()
            .position(|item| matches!(item, DeferredOutputItem::Chunk(c) if c.pane_id == pane_id))
        {
            // AC-5 fix (mux-window-switch-output-hang task0004 rework,
            // review round 3 finding `0830abe1c16ad0fb`): assign the new
            // content IN PLACE at the SAME queue position rather than
            // `remove` + `push_back`. The pre-fix remove-then-append moved
            // a coalesced chunk to the queue's TAIL, breaking the FIFO
            // invariant this type's own doc declares ("a later item can
            // never overtake an earlier one FROM THIS QUEUE") — concretely,
            // `[Chunk(P), VisibilityResume(P)]` followed by a second
            // `RequestPaneSnapshot` for P reordered into
            // `[VisibilityResume(P), Chunk(P')]`, so the resume's
            // freshly-built-at-flush-time snapshot would be delivered
            // FIRST and then immediately overwritten by the OLDER
            // already-built chunk arriving second — the opposite of
            // "newest wins". Assigning in place keeps the position (and
            // thus the delivery order relative to every other queued item)
            // unchanged; only the content is replaced.
            log::debug!(
                "deferred pane output queue: coalescing new chunk for pane {} over the \
                 previously-queued one IN PLACE at position {} (newest wins, position \
                 preserved — mux-window-switch-output-hang task0003 AC-2 / task0004 AC-5)",
                pane_id,
                pos,
            );
            self.items[pos] = DeferredOutputItem::Chunk(chunk);
            return;
        }

        // task0006 rework (review round 5, high findings `4043ee676f69ca15`
        // (comprehensive) / `1c8d86389ab4bf40` (architecture), plus the
        // spec-side medium "defer_chunk の VisibilityResume 併存時ドロップは
        // FR3 が列挙しない第3の破棄経路"): the REVERSE order from the case
        // above — no `Chunk` is queued yet for this pane, but a
        // `VisibilityResume` IS. task0005 dropped the redundant Chunk here
        // outright, on the premise that the Resume's flush
        // (`resume_pane_with_permit`, via `resolve_pane_and_resume`) always
        // builds a FRESH, current snapshot regardless of when it was queued.
        // That premise does not hold: `resume_pane_with_permit` returns
        // `ResumeOutcome::NoChange` WITHOUT sending anything when the pane
        // is already `Connected`, when the resolved owner does not match
        // the caller, or when `reason.clear_hidden()` still yields `Some`
        // (a `NetworkDetach` bit surviving a visible edge) — and its
        // sibling `evaluate_output_target` path additionally declines when
        // the built snapshot exceeds the single-frame limit.
        // `flush_deferred_output` also discards a Resume outright when
        // `visible_state` went false in the interim, and
        // `resolve_pane_and_resume` sends nothing when the session or pane
        // is gone by flush time. None of this is exotic:
        // `handle_set_visibility` queues a `VisibilityResume` for EVERY
        // non-exited pane in the session on a visible edge without
        // checking whether that pane is actually detached-hidden, so a
        // Resume that will no-op at flush time is the NORMAL case. Dropping
        // the Chunk in that case left the client's `RequestPaneSnapshot`
        // with no reply at all — the tab stayed stale until the next
        // unrelated output, exactly the symptom this feature exists to
        // remove.
        //
        // Fix: INSERT the Chunk immediately BEFORE the queued Resume
        // instead of dropping it. Flush order becomes
        // `Chunk (built now, at defer time)` -> `Resume (built fresh, at
        // flush time)`, so the newest content still wins when the Resume
        // actually produces a fresher snapshot (preserving task0005's
        // newest-wins intent), AND the request is still answered when the
        // Resume no-ops — the Chunk was already delivered by then. This is
        // a NEW distinct-pane `Chunk` entry (no `Chunk` for this `pane_id`
        // existed above), so it goes through the same `MAX_DEFERRED_ITEMS`
        // cap/eviction check as any other newly-admitted distinct-pane
        // chunk — applied BEFORE locating the Resume's position, since an
        // eviction earlier in the queue would otherwise invalidate that
        // position.
        let chunk_count = self
            .items
            .iter()
            .filter(|item| matches!(item, DeferredOutputItem::Chunk(_)))
            .count();
        if chunk_count >= MAX_DEFERRED_ITEMS {
            if let Some(oldest) = self
                .items
                .iter()
                .position(|item| matches!(item, DeferredOutputItem::Chunk(_)))
            {
                let dropped = self.items.remove(oldest);
                log::warn!(
                    "deferred pane output queue at capacity ({} chunks); dropping the \
                     OLDEST queued chunk {:?} to admit pane {}'s newer one \
                     (mux-window-switch-output-hang task0003 AC-2: never drop the newest; \
                     this eviction is a SPEC-sanctioned bounded-backlog policy, SPEC.md \
                     FR3's carve-out, task0004 G3/AC-3 option (a) — not a contradiction of \
                     FR3's delivery guarantee)",
                    MAX_DEFERRED_ITEMS,
                    dropped,
                    pane_id,
                );
            }
        }

        if let Some(resume_pos) = self.items.iter().position(
            |item| matches!(item, DeferredOutputItem::VisibilityResume(p) if *p == pane_id),
        ) {
            log::debug!(
                "deferred pane output queue: inserting a new chunk for pane {} immediately \
                 before its already-queued VisibilityResume (not dropping it — the Resume \
                 may no-op at flush time; mux-window-switch-output-hang task0006)",
                pane_id,
            );
            self.items
                .insert(resume_pos, DeferredOutputItem::Chunk(chunk));
            return;
        }

        self.items.push_back(DeferredOutputItem::Chunk(chunk));
    }

    /// Defer a visibility-resume attempt for `pane_id` (see
    /// [`DeferredOutputItem::VisibilityResume`]).
    ///
    /// task0003 rework (AC-1, review round 2 findings `4999311c8becf7eb` /
    /// `ff58ab6fd17542f4` / `1d648d947b4dea8b`): deduplicated by `pane_id` —
    /// a pane already awaiting resume does not get a second entry — and,
    /// once deduplicated, NEVER dropped for being past `MAX_DEFERRED_ITEMS`
    /// (that cap bounds `Chunk` backlog only, see its doc). A dropped resume
    /// has no client-driven retry path (`handle_set_visibility`'s
    /// `prev == visible` guard makes a repeated `SetVisibility(true)` a
    /// no-op), so losing one here would strand the pane `Detached` until an
    /// unrelated hide/show cycle — exactly the "output stops flowing"
    /// symptom this feature exists to remove. The set of distinct pane ids
    /// a single visibility edge can defer is itself finite and tiny (one
    /// entry per non-exited pane in the session, no payload), so leaving it
    /// uncapped does not reopen the unbounded-growth defect FR4 closed.
    pub fn defer_visibility_resume(&mut self, pane_id: PaneId) {
        let already_queued = self
            .items
            .iter()
            .any(|item| matches!(item, DeferredOutputItem::VisibilityResume(p) if *p == pane_id));
        if already_queued {
            return;
        }
        self.items
            .push_back(DeferredOutputItem::VisibilityResume(pane_id));
    }

    /// Pop the front item (used by the flush loop to take the next item to
    /// attempt).
    pub fn pop_front(&mut self) -> Option<DeferredOutputItem> {
        self.items.pop_front()
    }

    /// Put `item` back at the front (used by the flush loop when an item
    /// still can't be sent — preserves its place at the head of the queue
    /// for the next flush attempt).
    pub fn requeue_front(&mut self, item: DeferredOutputItem) {
        self.items.push_front(item);
    }

    /// Drop every remaining item (used once the channel is observed
    /// `Closed` — every future send attempt would fail the same way, so
    /// there is no reason to keep retaining the backlog).
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Default for DeferredOutputQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Enqueue `chunk` onto `tx` without ever letting the CALLING task block on
/// the channel's capacity (mux-window-switch-output-hang task0001; reworked
/// in task0002 to defer into a connection-owned [`DeferredOutputQueue`]
/// instead of an independently spawned task — see that type's doc for why).
///
/// ### Why this exists
///
/// `tx` (a connection's `pane_output_tx`) is drained by exactly one place:
/// the owning connection's own `select!` loop
/// (`mux::ipc::connection::handle_connection`, the `pane_output_rx.recv()`
/// arm). Every call site that matters for the bug this fixes runs FROM
/// INSIDE THAT SAME connection task (`route_message` ->
/// `handle_request_pane_snapshot`). A bare `tx.send(chunk).await` blocks the
/// CURRENT task until capacity frees — but the only thing able to free
/// capacity is that SAME task's own drain arm, which cannot run while the
/// task is suspended here. The task then self-deadlocks: no further client
/// messages are processed and no further PTY output for ANY pane on the
/// connection is forwarded (SPEC.md "Root Cause").
///
/// ### Mechanism
///
/// Fast path: `try_send` — non-blocking, succeeds immediately when the
/// channel has room (the overwhelmingly common case). The chunk lands at
/// the current tail of the queue, after every chunk already resident there.
///
/// Slow path: when the channel is momentarily full, `chunk` is pushed onto
/// `deferred` (bounded, AC-3) instead of being sent. The connection's own
/// event loop retries it via `handlers::flush_deferred_output` the next
/// time capacity frees — see [`DeferredOutputQueue`]'s doc for the ordering
/// guarantee this provides (and its documented residual limit).
///
/// This function is deliberately synchronous (not `async fn`) and never
/// spawns a task: it can never itself suspend on channel capacity (the
/// self-deadlock this exists to avoid), and — unlike task0001's
/// `tokio::spawn`-based version — it no longer depends on an active tokio
/// runtime at all, so hitting the Full branch outside one is not a panic
/// risk (AC-4; see the `outside_tokio_runtime` test below).
///
/// A closed channel (client gone) is logged and dropped — no new panic or
/// unhandled error path relative to the pre-existing
/// `if let Err(e) = ... send(...).await { log::warn!(...) }` handling this
/// replaces.
pub fn enqueue_pane_output_chunk(
    tx: &mpsc::Sender<PtyOutputChunk>,
    chunk: PtyOutputChunk,
    deferred: &mut DeferredOutputQueue,
) {
    match tx.try_send(chunk) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(chunk)) => {
            log::warn!(
                "pane {} output channel full ({} capacity); deferring enqueue \
                 (kind={:?}) to the connection's own deferred queue so this \
                 connection's own drain arm keeps running (mux-window-switch-output-hang \
                 task0001/task0002)",
                chunk.pane_id,
                PTY_CHANNEL_CAPACITY,
                chunk.kind,
            );
            deferred.defer_chunk(chunk);
        }
        Err(mpsc::error::TrySendError::Closed(chunk)) => {
            log::warn!(
                "pane {} output channel closed; dropping chunk (kind={:?})",
                chunk.pane_id,
                chunk.kind,
            );
        }
    }
}
