//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::agent_status::{AgentState, AgentStatusEvent};
use crate::agent_status_exit_latch::AgentStatusExitLatch;
use crate::mux::scrollback_buffer::{
    DEFAULT_SCROLLBACK_CAPACITY, MAX_DIM_MARKERS, ScrollbackRingBuffer,
};
use crate::mux::session::child_reaper;
use crate::mux::snapshot_bytes::build_resume_snapshot_bytes;
use crate::prompts::PromptMarkKind;
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::{HIDDEN_PASSTHROUGH_CAPACITY_MUX, RawPassthroughBuffer};

/// Encode `(bytes, segments)` into the D1' wire format
/// (`mux_ipc::protocol::encode_snapshot_payload`) for a `Snapshot`-kind
/// `PtyOutputChunk` (task0004 round-4 rework). Thin wrapper converting the
/// mux-layer's plain `(usize, u16, u16)` segment tuples into
/// `mux_ipc::protocol::DimSegment` at the wire boundary.
pub(in crate::mux) fn encode_snapshot_segments(
    bytes: &[u8],
    segments: &[(usize, u16, u16)],
) -> Vec<u8> {
    let dim_segments: Vec<mux_ipc::protocol::DimSegment> = segments
        .iter()
        .map(|&(offset, cols, rows)| mux_ipc::protocol::DimSegment {
            offset: offset as u32,
            cols,
            rows,
        })
        .collect();
    mux_ipc::protocol::encode_snapshot_payload(&dim_segments, bytes)
}

/// Pane identifier.
pub type PaneId = u32;

/// Channel for pane title change notifications (pane_id, new_title).
pub type TitleChangeSender = mpsc::Sender<(PaneId, String)>;

/// Swappable title sender shared between the reader thread and the connection handler.
/// Set to Some(tx) when a GUI client is connected; None when detached.
pub type SharedTitleSender = Arc<StdMutex<Option<TitleChangeSender>>>;

/// Channel for OSC 9 desktop-notification messages detected on a pane's
/// background (Detached) output (pane_id, message).
pub type NotificationSender = mpsc::Sender<(PaneId, String)>;

/// Daemon-lifetime notification sender shared with each pane reader thread.
/// Unlike `SharedTitleSender`, this is always populated (the daemon-level
/// channel lives as long as the daemon), so a Detached pane can surface a
/// notification even when no GUI client is currently attached.
pub type SharedNotificationSender = Arc<StdMutex<Option<NotificationSender>>>;

/// Channel carrying a pane's `PaneId` from its reader thread to the daemon
/// reap task when the pane's PTY reaches EOF.
///
/// Distinct from `NotificationSender = mpsc::Sender<(PaneId, String)>` (the
/// OSC-notification relay): this carries only a bare `PaneId` and drives pane
/// reap, not desktop notifications (NFR4).
pub type PaneExitSender = mpsc::Sender<PaneId>;

/// Daemon-lifetime pane-exit sender shared with each pane reader thread.
///
/// Follows the `SharedNotificationSender` shape exactly: an
/// `Arc<Mutex<Option<_>>>` so it can be cloned into reader threads and left
/// `None` in CLI / test paths. Unlike the pane output target, this sender is
/// fixed at pane creation and is **never** swapped on attach/detach — that is
/// what lets a detached pane still notify the daemon on EOF (M1).
pub type SharedPaneExitSender = Arc<StdMutex<Option<PaneExitSender>>>;

/// One item flowing through the daemon-lifetime per-pane agent-status
/// channel (task0003, SPEC FR4): either a raw agent-status OSC 777 report
/// body, or a live OSC 133 prompt mark. Both travel through the SAME
/// channel (not two independently-scheduled ones) so the daemon's single
/// consuming task (`mux::daemon::run_agent_status_task`) applies both in
/// the exact relative order the reader thread observed them on the PTY
/// stream — two separate channels/tasks could race and reorder a `Set`
/// relative to a `D`/`A` pair from the same PTY read, which SPEC FR4
/// explicitly forbids.
#[derive(Debug, Clone)]
pub enum AgentStatusFeedItem {
    /// A raw `agent-status` OSC 777 report body (the full
    /// `emterm;agent-status;…` payload, `agent_status::parse`'s input
    /// contract).
    Report(String),
    /// A live, main-screen-observed OSC 133 mark (task0001's
    /// `AgentStatusExitLatch` input). The reader thread only ever sends
    /// marks it captured on the LIVE PTY stream while the pane was on the
    /// main screen — never marks reconstructed for snapshot/replay/
    /// reattach purposes, and never alt-screen-suppressed marks (SPEC FR5).
    Osc133Mark(PromptMarkKind),
}

/// Channel carrying agent-status-relevant events for a pane (raw OSC 777
/// report bodies AND live OSC 133 marks, SPEC FR4 — see
/// [`AgentStatusFeedItem`]) from a pane's reader thread to the daemon-level
/// agent-status task (`pane_id`, item).
///
/// Unlike `NotificationSender`, this must be forwarded regardless of attach
/// state (SPEC FR3: the daemon owns per-pane agent-status state
/// unconditionally, not only while detached) — mirroring the daemon-lifetime
/// `TitleChangeSender` wiring rather than the Detached-only notification
/// scanner.
pub type AgentStatusReportSender = mpsc::Sender<(PaneId, AgentStatusFeedItem)>;

/// Daemon-lifetime agent-status report sender shared with each pane reader
/// thread. Follows the `SharedNotificationSender` / `SharedPaneExitSender`
/// shape: populated once at pane creation and never swapped.
pub type SharedAgentStatusReportSender = Arc<StdMutex<Option<AgentStatusReportSender>>>;

/// A pane's agent-status state (SPEC FR3): the most recently accepted
/// report (or none), plus a monotonically increasing revision. Every
/// ACCEPTED report — set, clear, or a same-state re-report — increments
/// `revision`; a rejected report is never applied (see
/// `MuxPane::apply_agent_status_event`, which is only ever called with an
/// `AgentStatusEvent` a caller already validated via `agent_status::parse`).
/// State is in-memory only and is discarded when the owning `MuxPane` is
/// dropped (pane destroy / PtyExited reap).
#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub state: Option<AgentState>,
    pub name: Option<String>,
    pub revision: u64,
}

/// Thread-safe shared reference to a pane's agent-status state.
pub type SharedAgentStatus = Arc<StdMutex<AgentStatus>>;

/// Thread-safe shared reference to a pane's inferred-clear latch (task0001
/// `AgentStatusExitLatch`, SPEC FR1/FR2). One instance per pane, alongside
/// `agent_status`/`agent_waiters`: created with the pane, discarded when
/// the pane is (no special-case pane-lifecycle handling needed — the whole
/// agent-status entry already goes away with the pane).
///
/// task0003 AC-7: named/shaped identically to the sibling `SharedAgentStatus`
/// (`Arc<StdMutex<_>>` around a plain, `Copy`-able state struct) so
/// task0004's hot-upgrade carry-across can locate and clone this field on
/// `MuxPane` without needing to read this task's plan.
pub type SharedAgentStatusExitLatch = Arc<StdMutex<AgentStatusExitLatch>>;

/// A registered `WaitAgentState` request awaiting a qualifying state change
/// (task0004, IMPLEMENTATION.md "Wait implementation"). Level-triggered:
/// fires when `states` contains the pane's current
/// [`AgentStatus::state`] AND (if set) the current revision exceeds
/// `after_revision`. `states` is stored in the CORE `AgentState` type (this
/// module's `AgentStatus::state` type) so matching needs no per-check wire
/// conversion; the wire `mux_ipc::protocol::AgentState` only appears at the
/// request/response boundary in `mux::ipc::handlers`.
///
/// `responder` is `Option` so a firing/cleanup pass can `.take()` the
/// owned `Sender` out of a `&mut` iteration (`oneshot::Sender::send`
/// consumes `self`).
pub struct AgentWaiter {
    pub states: Vec<AgentState>,
    pub after_revision: Option<u64>,
    pub responder: Option<tokio::sync::oneshot::Sender<AgentWaitOutcome>>,
}

/// Outcome delivered to a registered [`AgentWaiter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWaitOutcome {
    /// The waiter's condition was satisfied.
    Matched { state: AgentState, revision: u64 },
    /// The pane was destroyed while the waiter was pending.
    PaneGone,
}

/// Thread-safe shared reference to a pane's registered agent-state waiters.
pub type SharedAgentWaiters = Arc<StdMutex<Vec<AgentWaiter>>>;

/// Callback sink recording the most recent OSC 0/2 window title.
///
/// vt100 0.16 removed `Screen::title()` in favor of the callback API;
/// the reader loop drains this sink via `take_title()` after each
/// `process()` call.
#[derive(Default)]
pub struct TitleSink {
    title: Option<String>,
}

impl TitleSink {
    /// Take the title set since the last call, if any.
    pub fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }
}

impl vt100::Callbacks for TitleSink {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).into_owned());
    }
}

/// Shadow VT100 parser with title-change reporting.
pub type ShadowParser = vt100::Parser<TitleSink>;

/// Thread-safe shared reference to a shadow VT100 parser.
pub type SharedShadowParser = Arc<StdMutex<ShadowParser>>;

/// Construct a shadow parser at the given size (scrollback 0).
pub fn new_shadow_parser(rows: u16, cols: u16) -> ShadowParser {
    vt100::Parser::new_with_callbacks(rows, cols, 0, TitleSink::default())
}

/// Lock-free, shared snapshot of a pane's CURRENT dimensions (task0003 D5,
/// review round-2 finding `0bebe3e6f7b416dd`).
///
/// `MuxPane::resize` updates this at the same point it records the resize
/// marker (holding the scrollback lock); the PTY reader thread reads it
/// (via [`Self::get`], no lock contention with the scrollback mutex)
/// immediately after each `read()` call returns and passes the result to
/// [`crate::mux::scrollback_buffer::ScrollbackRingBuffer::attribute_write`]
/// so a chunk gets attributed to the dimensions actually in effect when it
/// was PRODUCED, rather than trusting write-time lock ordering alone.
///
/// This closes the residual half of the ordering guarantee
/// `MuxPane::resize`'s scrollback-lock holding established: that mechanism
/// only ensures bytes the reader appends AFTER a resize land after the
/// marker; it does not help bytes the reader had ALREADY READ (but not yet
/// appended) before the resize call started — those could previously be
/// appended after the marker (since appending needs the same lock the
/// resize call holds first) and get misattributed to the NEW dimensions
/// even though they were produced under the OLD ones.
///
/// task0004 round-4 rework (review round-3 finding `ae43417cee647afa`):
/// `cols` and `rows` are packed into a SINGLE `AtomicU32` (`cols` in the
/// high 16 bits, `rows` in the low 16 bits) rather than two independent
/// `AtomicU16`s. Two independent atomics let a concurrent [`Self::get`]
/// observe a torn pair — e.g. `(old cols, new rows)` — that never actually
/// existed as a real dimension pair, if it raced with [`Self::set`] between
/// the two stores; that torn pair would then be recorded as a resize
/// marker for a size the pane was never actually at. A single `AtomicU32`
/// makes every `get`/`set` a single load/store, so no intermediate,
/// never-real state is ever observable.
#[derive(Debug)]
pub struct PaneDims {
    packed: std::sync::atomic::AtomicU32,
}

impl PaneDims {
    fn pack(cols: u16, rows: u16) -> u32 {
        ((cols as u32) << 16) | (rows as u32)
    }

    fn unpack(packed: u32) -> (u16, u16) {
        ((packed >> 16) as u16, packed as u16)
    }

    fn new(cols: u16, rows: u16) -> Self {
        Self {
            packed: std::sync::atomic::AtomicU32::new(Self::pack(cols, rows)),
        }
    }

    /// Current `(cols, rows)` — a single atomic load, so the pair observed
    /// is always one that was actually `set` together, never a torn mix of
    /// an old and a new value.
    pub fn get(&self) -> (u16, u16) {
        Self::unpack(self.packed.load(std::sync::atomic::Ordering::Acquire))
    }

    fn set(&self, cols: u16, rows: u16) {
        self.packed
            .store(Self::pack(cols, rows), std::sync::atomic::Ordering::Release);
    }
}

/// Shared reference to a pane's [`PaneDims`], cloned into the reader thread.
pub type SharedPaneDims = Arc<PaneDims>;

/// Write `data` to a PTY through a cloned writer handle (see
/// [`MuxPane::writer_handle`]), without going through a `MuxPane`
/// reference or any surrounding lock.
///
/// Locks `writer`'s `std::sync::Mutex` for the full `write_all` + `flush`
/// (matching `MuxPane::write_input`'s atomicity contract exactly — this is
/// the shared implementation both go through). Because handle clones share
/// the same underlying mutex, two concurrent calls against handles cloned
/// from the same pane still serialize here (task0011 AC-5: no interleaving
/// between concurrent sends to the same pane).
pub fn write_via_writer_handle(
    writer: &Arc<StdMutex<Box<dyn Write + Send>>>,
    data: &[u8],
) -> std::io::Result<()> {
    let mut w = writer
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    w.write_all(data)?;
    w.flush()
}

/// Lock the shadow parser, recovering from a poisoned mutex.
///
/// vt100 has internal panics (wide-character bookkeeping can `unwrap` a
/// `None` mid-`process`). If the reader thread panics while holding this
/// lock, the mutex stays poisoned for the lifetime of the daemon and every
/// later `lock().unwrap()` on the attach/resize path panics too, making the
/// session permanently unattachable. Recovering the guard trades a possibly
/// stale snapshot for a daemon that keeps serving attaches.
pub fn lock_shadow_parser(
    parser: &StdMutex<ShadowParser>,
) -> std::sync::MutexGuard<'_, ShadowParser> {
    parser
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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

/// Why a pane is currently detached. Combines `NetworkDetach`
/// (no client connected / kicked / explicit detach) with
/// `HiddenByVisibility` (client connected but reported hidden).
///
/// A pane stays Detached until **all** active reasons clear:
/// - hidden -> visible resolves the `HiddenByVisibility` bit
/// - reattach resolves the `NetworkDetach` bit
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachReason {
    NetworkDetach,
    HiddenByVisibility,
    Both,
}

impl DetachReason {
    pub fn has_network(self) -> bool {
        matches!(self, DetachReason::NetworkDetach | DetachReason::Both)
    }

    pub fn has_hidden(self) -> bool {
        matches!(self, DetachReason::HiddenByVisibility | DetachReason::Both)
    }

    /// Combine two reasons (set union).
    pub fn combine(a: Self, b: Self) -> Self {
        match (
            a.has_network() || b.has_network(),
            a.has_hidden() || b.has_hidden(),
        ) {
            (true, true) => DetachReason::Both,
            (true, false) => DetachReason::NetworkDetach,
            (false, true) => DetachReason::HiddenByVisibility,
            (false, false) => DetachReason::HiddenByVisibility,
        }
    }

    /// Clear the `NetworkDetach` bit. Returns `None` when the result is empty.
    pub fn clear_network(self) -> Option<Self> {
        match self {
            DetachReason::NetworkDetach => None,
            DetachReason::HiddenByVisibility => Some(DetachReason::HiddenByVisibility),
            DetachReason::Both => Some(DetachReason::HiddenByVisibility),
        }
    }

    /// Clear the `HiddenByVisibility` bit. Returns `None` when the result is empty.
    pub fn clear_hidden(self) -> Option<Self> {
        match self {
            DetachReason::HiddenByVisibility => None,
            DetachReason::NetworkDetach => Some(DetachReason::NetworkDetach),
            DetachReason::Both => Some(DetachReason::NetworkDetach),
        }
    }
}

/// Where the PTY reader thread sends output data.
///
/// When a GUI client is connected, output goes directly to the channel.
/// When disconnected, the reader still drains the PTY but suppresses sends;
/// the scrollback ring (Phase B: `MuxPane::scrollback`) captures recent
/// bytes for replay on reattach.
///
/// `Detached` carries identity (`owner`) and cause (`reason`) so a second
/// connection cannot reclaim a pane that the first connection put into
/// `HiddenByVisibility`. `owner = None` marks system-origin detaches (e.g.
/// pane spawned before any client attached, or PTY EOF fallback) which any
/// connection may reclaim.
pub enum PaneOutputTarget {
    /// Connected: send output to the GUI via channel.
    Connected(mpsc::Sender<PtyOutputChunk>),
    /// Detached: the reader keeps draining the PTY into the per-pane
    /// scrollback (`MuxPane::scrollback`) and `raw_passthrough`; no
    /// channel send happens until the pane returns to `Connected`.
    Detached {
        reason: DetachReason,
        owner: Option<mpsc::Sender<PtyOutputChunk>>,
    },
}

/// Thread-safe shared reference to a pane's output target.
pub type SharedOutputTarget = Arc<StdMutex<PaneOutputTarget>>;

/// Thread-safe shared reference to a pane's scrollback ring buffer.
pub type SharedScrollback = Arc<StdMutex<ScrollbackRingBuffer>>;

/// Result of `evaluate_output_target`. Carries the resume snapshot bytes
/// when the pane transitions Detached -> Connected so the handler can
/// enqueue them on the same channel before the next reader chunk lands.
pub enum EvalResult {
    /// No state change required; output_target is still correct.
    Unchanged,
    /// Pane was switched into Detached buffering mode.
    SwitchedToDetached,
    /// Pane was switched (back) into Connected mode. The handler MUST send
    /// `chunk` on the pane's output channel before any subsequent reader
    /// chunk.
    ///
    /// D6''''' (round-8 rework, review round-7 finding `426db84173e6b792`):
    /// `chunk` is a `PtyOutputChunk` already tagged `ChunkKind::Snapshot`
    /// (via `PtyOutputChunk::snapshot`, same as the sibling
    /// `resume_pane_with_permit` path) — NOT the default `PtyOutput` kind.
    /// This is enforced by the TYPE, not just documented: there is no way
    /// for a caller to extract raw bytes here and send them as a plain
    /// `PtyOutputChunk::pty_output(...)` instead, which is what round 1
    /// finding `20b2bed0aaf48f94` fixed for `resume_pane_with_permit` (a
    /// `PtyOutput`-tagged send here would render the `EMSNAP2` envelope +
    /// binary segment table literally on screen instead of being decoded —
    /// review round-4 finding `5299d50f586b8cb8`'s failure mode). This
    /// branch is currently unreached by any production caller (the sole
    /// production call site, `handlers.rs`'s `evaluate_output_target(...,
    /// false, false, ...)`, only ever drives a Connected -> Detached
    /// transition) — kept correct for the day a caller needs the
    /// visible-resume path through THIS function instead of
    /// `resume_pane_with_permit`.
    ResumeWithSnapshot { chunk: PtyOutputChunk },
}

/// Outcome of `resume_pane_with_permit`. Mirrors the in-lock decision so
/// callers can branch on the side effects (snapshot enqueued + Connected
/// swap, or no transition) without re-locking the pane.
pub enum ResumeOutcome {
    /// Pane was Detached and resolved cleanly. The snapshot was sent via
    /// the supplied permit and the target was swapped to `Connected`.
    Resumed,
    /// No transition: pane is already Connected, owner mismatches, or
    /// `NetworkDetach` is still active (only reattach can clear it).
    NoChange,
}

/// Decide the correct `output_target` for `pane` given the current network
/// detach flag and the connection-scoped visible flag, and apply the
/// transition in place.
///
/// Identity-scoped on both edges:
/// - Connected -> Detached: only flip panes whose current `Connected(tx)`
///   is `same_channel(owned_tx)`. The new `Detached` records `owned_tx` as
///   `owner` so a different connection cannot reclaim it via SetVisibility.
/// - Detached -> Connected: only resume when `owner` matches `owned_tx`
///   (or `owner == None`, system origin), AND the resolved reason becomes
///   empty after the caller's transition (`HiddenByVisibility` clears on
///   `visible=true`; `NetworkDetach` only clears via the reattach path).
pub fn evaluate_output_target(
    pane: &MuxPane,
    network_detach: bool,
    visible: bool,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
) -> EvalResult {
    let mut target = pane.output_target.lock().unwrap();
    let new_reason = match (network_detach, visible) {
        (true, true) => Some(DetachReason::NetworkDetach),
        (false, false) => Some(DetachReason::HiddenByVisibility),
        (true, false) => Some(DetachReason::Both),
        (false, true) => None,
    };
    match &mut *target {
        PaneOutputTarget::Connected(current_tx) => {
            let owned_by_caller = current_tx.same_channel(owned_tx);
            match new_reason {
                None => EvalResult::Unchanged,
                Some(reason) if owned_by_caller => {
                    *target = PaneOutputTarget::Detached {
                        reason,
                        owner: Some(owned_tx.clone()),
                    };
                    EvalResult::SwitchedToDetached
                }
                Some(_) => EvalResult::Unchanged,
            }
        }
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return EvalResult::Unchanged;
            }
            // Resolve current reason against the caller's transition.
            let resolved = if visible {
                reason.clear_hidden()
            } else {
                Some(*reason)
            };
            let resolved = match (network_detach, resolved) {
                (true, Some(r)) => Some(DetachReason::combine(r, DetachReason::NetworkDetach)),
                (true, None) => Some(DetachReason::NetworkDetach),
                (false, r) => r,
            };
            match resolved {
                Some(r) => {
                    *reason = r;
                    if owner.is_none() {
                        *owner = Some(owned_tx.clone());
                    }
                    EvalResult::Unchanged
                }
                None => {
                    // Phase C FR5 order: clear → scrollback → (alt-only) shadow.
                    // Routes through `build_resume_snapshot_bytes` so the
                    // strip + main/alt split logic stays in lockstep with the
                    // visibility-resume SSOT (`resume_pane_with_permit` uses
                    // the same helper). Scrollback is read WITHOUT clearing
                    // (FR6: the buffer lives for the lifetime of the pane);
                    // the helper passes it through
                    // `strip_replayable_rich_content` so the resume does not
                    // re-spawn viewers / re-render inline images. Skip
                    // `contents_formatted()` entirely for main-buffer panes:
                    // the helper would drop the slice anyway, so we avoid
                    // both the computation and the longer shadow-parser
                    // lock hold.
                    //
                    // NOTE: this branch is currently unreachable in
                    // production — `handle_set_visibility` is the only
                    // production caller of `evaluate_output_target` and it
                    // always passes `visible == false`. Kept on the SSOT so a
                    // future `visible == true` call site picks up the
                    // strip / main-alt-split contract for free.
                    // D7'' (task0005 rework, review round-4 finding
                    // `5ba2063e993baf6c`): the shadow parser's own size
                    // tracks every `MuxPane::resize` call, so it is the
                    // pane's dims AT THE MOMENT this snapshot is assembled
                    // — what `screen_bytes` was actually produced at.
                    let (screen_bytes, alt_screen, current_dims) = {
                        let parser = lock_shadow_parser(&pane.shadow_parser);
                        let alt = parser.screen().alternate_screen();
                        let screen_bytes = if alt {
                            parser.screen().contents_formatted()
                        } else {
                            Vec::new()
                        };
                        let (rows, cols) = parser.screen().size();
                        (screen_bytes, alt, (cols, rows))
                    };
                    let (buffered, buffered_segments) =
                        pane.scrollback.lock().unwrap().read_segments();
                    {
                        // raw_passthrough is drained + cleared (so it does
                        // not leak across detach cycles) but NOT concatenated
                        // — replaying captured image / Markdown OSC sequences
                        // would re-spawn viewers / re-render inline images.
                        let mut buf = pane.raw_passthrough.lock().unwrap();
                        let _ = buf.read_all();
                        buf.clear();
                    }
                    let (snapshot, snapshot_segments) = build_resume_snapshot_bytes(
                        &buffered,
                        &buffered_segments,
                        &screen_bytes,
                        alt_screen,
                        current_dims,
                    );
                    let encoded_snapshot = encode_snapshot_segments(&snapshot, &snapshot_segments);
                    // D6''' (round-6 rework, review round-5 finding
                    // `89b58cd82d7aa713`): this producer used to enqueue
                    // unconditionally after only LOGGING an oversize
                    // snapshot — the connection codec then rejects any
                    // frame over `MAX_SNAPSHOT_FRAME_PAYLOAD` and the
                    // connection loop ends, so "visible in the log" was not
                    // actually a safe degradation. Enforce the size policy
                    // FOR REAL here: on oversize, fail recoverably by
                    // leaving the pane Detached (never transition to
                    // Connected, never hand back a doomed-to-be-rejected
                    // frame) rather than changing replay semantics by
                    // sending something the codec will tear the connection
                    // down over.
                    if !mux_ipc::protocol::fits_single_snapshot_frame(encoded_snapshot.len()) {
                        log::error!(
                            "visibility-resume: pane {} snapshot {}B exceeds the \
                             single-frame limit ({}B); staying detached rather \
                             than enqueuing a frame the codec would reject",
                            pane.id,
                            encoded_snapshot.len(),
                            mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD
                        );
                        return EvalResult::Unchanged;
                    }
                    *target = PaneOutputTarget::Connected(owned_tx.clone());
                    // D6''''' (round-8 rework, review round-7 finding
                    // `426db84173e6b792`): tag as `ChunkKind::Snapshot`
                    // right here — mirroring `resume_pane_with_permit`'s own
                    // `PtyOutputChunk::snapshot(...)` call (round-1 finding
                    // `20b2bed0aaf48f94`) — so the caller has no way to send
                    // this as a plain `PtyOutput` chunk.
                    EvalResult::ResumeWithSnapshot {
                        chunk: PtyOutputChunk::snapshot(pane.id, encoded_snapshot),
                    }
                }
            }
        }
    }
}

/// A `pane_output_tx` permit obtained either the FAST way (`try_reserve()`
/// or `reserve()`, borrowed from a live `&Sender`) or the FAIR way
/// (`reserve_owned()`, owned — mux-window-switch-output-hang task0003
/// rework, AC-3/G2: `mux::ipc::connection` polls a `reserve_owned()` future
/// as its own `select!` arm to avoid the starvation a `try_reserve`-only
/// retry suffers while a PTY reader thread has a `blocking_send` parked on
/// the same channel). `resume_pane_with_permit` accepts either so the same
/// atomic (build snapshot, send, swap to `Connected`) logic serves both
/// callers without duplicating it per permit type.
pub enum AnyPermit<'a> {
    Borrowed(mpsc::Permit<'a, PtyOutputChunk>),
    Owned(mpsc::OwnedPermit<PtyOutputChunk>),
}

impl AnyPermit<'_> {
    /// Consume the permit to send `chunk`. Infallible either way — the slot
    /// was already reserved — mirroring `mpsc::Permit::send`'s own contract.
    fn send(self, chunk: PtyOutputChunk) {
        match self {
            AnyPermit::Borrowed(p) => p.send(chunk),
            AnyPermit::Owned(p) => {
                // `OwnedPermit::send` hands back the underlying `Sender`;
                // callers here already hold their own clone, so it is
                // dropped.
                let _ = p.send(chunk);
            }
        }
    }
}

/// FR9 race-free Detached -> Connected resume.
///
/// The caller obtains a permit for `pane_output_tx` *outside* the pane lock
/// (via `Sender::reserve().await`, `try_reserve()`, or the fair
/// `reserve_owned()` — see [`AnyPermit`]), then hands it in here. This
/// function holds the pane's `output_target` mutex for the full lifetime of
/// (build snapshot, send via permit, swap to `Connected`). Because the PTY
/// reader thread also takes the same `output_target` mutex before its
/// `try_send` / `blocking_send`, the reader cannot push a live chunk between
/// the snapshot enqueue and the Connected swap — the snapshot is guaranteed
/// to land first in the channel's FIFO.
///
/// `AnyPermit::send` is consumed and infallible (the slot is already
/// reserved), so the entire sequence runs under the std mutex without
/// `await`.
///
/// Returns `ResumeOutcome::NoChange` when the pane is not eligible to
/// resume (already Connected, owner mismatch, or `NetworkDetach` still
/// active). The caller should drop the permit on `NoChange` to release the
/// reserved slot.
pub fn resume_pane_with_permit(
    pane: &MuxPane,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
    permit: AnyPermit<'_>,
) -> ResumeOutcome {
    let mut target = pane.output_target.lock().unwrap();
    match &mut *target {
        PaneOutputTarget::Connected(_) => ResumeOutcome::NoChange,
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return ResumeOutcome::NoChange;
            }
            let resolved = reason.clear_hidden();
            if let Some(r) = resolved {
                *reason = r;
                if owner.is_none() {
                    *owner = Some(owned_tx.clone());
                }
                return ResumeOutcome::NoChange;
            }
            // Phase C FR5 order: clear → scrollback → (alt-only) shadow.
            // Routes through `build_resume_snapshot_bytes` so the strip +
            // main/alt split logic stays in lockstep with the reattach /
            // on-demand snapshot SSOT (`build_snapshot_bytes_with_layout`).
            // Skip `contents_formatted()` entirely for main-buffer panes:
            // the helper would drop the slice anyway, so we avoid both the
            // computation and the longer shadow-parser lock hold.
            // D7'' (task0005 rework, review round-4 finding
            // `5ba2063e993baf6c`): the shadow parser's own size tracks
            // every `MuxPane::resize` call, so it is the pane's dims AT THE
            // MOMENT this snapshot is assembled — what `screen` was
            // actually produced at.
            let (screen, alt_screen, current_dims) = {
                let parser = lock_shadow_parser(&pane.shadow_parser);
                let alt = parser.screen().alternate_screen();
                let screen_bytes = if alt {
                    parser.screen().contents_formatted()
                } else {
                    Vec::new()
                };
                let (rows, cols) = parser.screen().size();
                (screen_bytes, alt, (cols, rows))
            };
            let (buffered, buffered_segments) = pane.scrollback.lock().unwrap().read_segments();
            {
                // raw_passthrough is drained + cleared (so it does not leak
                // across detach cycles) but NOT concatenated — replaying the
                // captured image / Markdown OSC sequences would re-spawn
                // viewers on every visibility resume.
                let mut buf = pane.raw_passthrough.lock().unwrap();
                let _ = buf.read_all();
                buf.clear();
            }
            let (snapshot, snapshot_segments) = build_resume_snapshot_bytes(
                &buffered,
                &buffered_segments,
                &screen,
                alt_screen,
                current_dims,
            );
            let encoded_snapshot = encode_snapshot_segments(&snapshot, &snapshot_segments);
            // D6''' (round-6 rework, review round-5 finding
            // `89b58cd82d7aa713`): see the parallel check in
            // `evaluate_output_target`'s `ResumeWithSnapshot` branch above —
            // same shared policy. On oversize, drop the reserved `permit`
            // WITHOUT sending (releasing its slot) and return `NoChange`
            // instead of `Resumed` — the pane stays Detached (fail
            // recoverably) rather than being handed a frame the codec will
            // reject, which previously tore the whole connection down.
            //
            // D3'''' (round-7 rework, review round-6 finding
            // `46c29c2c65970d26`): reachability of a PERMANENT freeze here
            // was disputed between round-6 reviewers — settled as
            // RECOVERABLE, not permanent. This branch's early `return`
            // happens BEFORE `*target` or `*reason` is ever touched, so the
            // pane is left in EXACTLY the state it was in on entry
            // (`Detached { HiddenByVisibility, .. }`). `handle_set_visibility`
            // (the only production caller) re-invokes this function for
            // every non-exited pane on every `visible -> true` edge that
            // is not itself a repeated no-op (its `prev == visible` guard
            // only suppresses a SECOND consecutive `true`, not a
            // false-then-true cycle) — so a later hide -> show toggle (the
            // client minimizing/restoring, or switching panes away and
            // back) unconditionally retries this exact call, and succeeds
            // once whatever inflated the snapshot (typically an
            // extreme-dimension alt-screen `contents_formatted()` dump,
            // D6'''' narrows the dimensions that can produce one) is no
            // longer true. See
            // `resume_pane_with_permit_recovers_after_oversize_condition_clears`
            // for the regression test pinning this: the pane is never left
            // detached with visibility latched on forever.
            if !mux_ipc::protocol::fits_single_snapshot_frame(encoded_snapshot.len()) {
                log::error!(
                    "visibility-resume: pane {} snapshot {}B exceeds the \
                     single-frame limit ({}B); staying detached rather \
                     than enqueuing a frame the codec would reject",
                    pane.id,
                    encoded_snapshot.len(),
                    mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD
                );
                drop(permit);
                return ResumeOutcome::NoChange;
            }
            // review round-1 rework, finding `20b2bed0aaf48f94`: tag this as
            // a Snapshot-kind chunk (not the default PtyOutput) so the mux
            // connection drain (`mux::ipc::connection`) sends it as
            // `MessageType::Snapshot` on the wire. The client's
            // `apply_mux_message::Snapshot|SnapshotRestore` arm decodes the
            // structural dimension segments (task0004 round-4 rework D1')
            // and routes through `reset_and_replay_segments`, resizing per
            // segment instead of scanning the payload for markers — the
            // plain `PtyOutput` live path used before this fix does not, so
            // a resize-spanning visibility-resume snapshot replayed
            // coordinate-drifted content just like the reattach path did.
            permit.send(PtyOutputChunk::snapshot(pane.id, encoded_snapshot));
            *target = PaneOutputTarget::Connected(owned_tx.clone());
            ResumeOutcome::Resumed
        }
    }
}

/// A pane's child-process reference (task plan task0007, IMPLEMENTATION.md
/// D6): either an owned PTY-library child handle (a freshly spawned pane,
/// task0001 SPEC FR2) or a bare process id (a pane restored from a handoff,
/// whose owned handle could not be rebuilt after the process image was
/// replaced). [`MuxPane::mark_exited`] routes to the matching
/// [`child_reaper`] entry point for whichever variant is present, so a
/// restored pane and a freshly spawned pane are indistinguishable to the
/// rest of the daemon.
pub enum PaneChild {
    /// A freshly spawned pane's owned child handle.
    Owned(Box<dyn portable_pty::Child + Send + Sync>),
    /// A process id belonging to a pane restored from a handoff. Unix only:
    /// the reaping path this variant routes to
    /// ([`child_reaper::spawn_reaper_pid`]) is Unix-only (task plan Design —
    /// "Guard the process-id path to Unix").
    #[cfg(unix)]
    ProcessId(u32),
}

/// A single terminal pane with its PTY and communication channels.
pub struct MuxPane {
    pub id: PaneId,
    pub cols: u16,
    pub rows: u16,
    /// Shared output target (reader thread writes here, connection handler swaps on detach/reattach).
    pub output_target: SharedOutputTarget,
    /// Writer handle for sending input to the PTY.
    writer: Option<Arc<StdMutex<Box<dyn Write + Send>>>>,
    /// Master PTY handle, retained after reader/writer extraction for resize support.
    master: Option<Box<dyn MasterPty + Send>>,
    /// A pane's child-process reference (task0001 SPEC FR2; task plan
    /// task0007, IMPLEMENTATION.md D6), retained for the pane's lifetime and
    /// taken + handed to the matching [`child_reaper`] entry point on
    /// [`Self::mark_exited`]. `None` for panes constructed without a
    /// spawned/restored child (`new_test` / `new_test_with_writer`, and this
    /// module's own tests that open a real PTY but never call
    /// `spawn_command`).
    child: Option<PaneChild>,
    /// Whether this pane's PTY has exited.
    pub exited: bool,
    /// Shadow VT100 parser for screen state tracking (used for reattach restoration).
    pub shadow_parser: SharedShadowParser,
    /// Cached working directory from OSC 7 detection.
    pub cwd: Arc<StdMutex<Option<String>>>,
    /// Cached title from OSC 0/2 detection (set by pty_reader_loop).
    pub title: Arc<StdMutex<Option<String>>>,
    /// Swappable title change sender (reader thread reads from this on each title change).
    pub title_sender: SharedTitleSender,
    /// Daemon-lifetime notification sender. The reader thread forwards OSC 9
    /// desktop notifications detected on Detached output through this channel
    /// so the daemon can relay them to the GUI client.
    pub notification_sender: SharedNotificationSender,
    /// This pane's agent-status state (SPEC FR3): current report + revision.
    pub agent_status: SharedAgentStatus,
    /// Daemon-lifetime agent-status report sender. The reader thread forwards
    /// raw agent-status OSC payload strings through this channel (regardless
    /// of attach state) so the daemon can validate, apply, and broadcast them.
    pub agent_status_report_sender: SharedAgentStatusReportSender,
    /// Registered `WaitAgentState` waiters for this pane (task0004). See
    /// [`AgentWaiter`].
    pub agent_waiters: SharedAgentWaiters,
    /// This pane's inferred-clear latch (task0001 `AgentStatusExitLatch`,
    /// SPEC FR1/FR2). Fed by [`Self::apply_agent_status_event`] (explicit
    /// Set/Clear) and [`Self::record_live_osc133_mark`] (live OSC 133
    /// marks); a fired inferred clear is applied through the SAME path as
    /// an explicit `Clear` — no parallel clear logic.
    pub agent_status_exit_latch: SharedAgentStatusExitLatch,
    /// Per-pane raw passthrough buffer for image / Markdown OSC sequences
    /// captured while the pane is detached (network detach OR client hidden).
    /// Drained into the reattach / resume snapshot.
    pub raw_passthrough: Arc<StdMutex<RawPassthroughBuffer>>,
    /// Stateful scanner that pulls passthrough sequences out of the raw PTY
    /// byte stream. Lives alongside `raw_passthrough` because partial
    /// sequences span multiple reader chunks.
    pub passthrough_scanner: Arc<StdMutex<PassthroughScanner>>,
    /// Per-pane scrollback ring buffer. Holds recent PTY bytes for replay
    /// on reattach. Phase B: the reader only writes here while the pane is
    /// detached; Phase C will switch to always-on writes so pre-detach
    /// scrollback is also retained.
    pub scrollback: SharedScrollback,
    /// Lock-free snapshot of this pane's current dimensions (task0003 D5).
    /// See [`PaneDims`] for the ordering guarantee this closes.
    pub dims: SharedPaneDims,
}

/// Clamp `(cols, rows)` to the domain the wire decoder ACTUALLY accepts for
/// a snapshot segment (D6'''', round-7 rework, review round-6 finding
/// `6cefb1dd16c126b6`).
///
/// `term_core::terminal_core::clamp_resize_dims` bounds each axis
/// independently to `1..=4096`, but `mux_ipc::protocol`'s segment decoder
/// additionally rejects any segment whose `cols * rows` product exceeds
/// `MAX_SEGMENT_CELLS` (1_000_000) — a legitimate-looking size like
/// 2000x600 (product 1,200,000) or the per-axis ceiling itself
/// (4096x4096 = 16,777,216) clears the per-axis clamp untouched but is
/// `Malformed` on the wire. Before this fix, `MuxPane::new` / `resize`
/// validated ONLY the per-axis domain, so a pane could be created/resized
/// to dimensions whose own initial segment (or a later resize segment) its
/// own peer would reject when decoding the resulting snapshot — the
/// producer's accepted domain and the decoder's accepted domain had
/// drifted apart even though both are meant to share one contract.
///
/// When the per-axis-clamped product still exceeds the ceiling, `rows` is
/// reduced to fit under it (preserving `cols`, since column count is
/// usually the more consequential axis for terminal rendering) rather than
/// rejecting the resize outright — mirroring `clamp_resize_dims`'s own
/// "clamp, don't reject" policy so callers never need special-case error
/// handling for an oversized resize request.
///
/// D3''''' (round-8 rework, review round-7 finding `1d1b6b6297e3b6a0`):
/// `pub(crate)` so `tabs.rs` (the GUI client side, same crate) can apply the
/// IDENTICAL clamp before resizing its own `TerminalCore` and before sending
/// the `Resize` control message — a deterministic, PURE function of
/// `(cols, rows)` shared by both ends means the client's core and the
/// daemon's PTY always agree on the accepted dimensions without any wire
/// round-trip acknowledgment: same function, same input, same output.
pub(crate) fn clamp_dims_to_wire_domain(cols: u16, rows: u16) -> (u16, u16) {
    let (cols, rows) = term_core::terminal_core::clamp_resize_dims(cols, rows);
    if (cols as u32) * (rows as u32) <= PRODUCER_SEGMENT_CELL_BUDGET {
        return (cols, rows);
    }
    let max_rows = (PRODUCER_SEGMENT_CELL_BUDGET / cols as u32).max(1) as u16;
    (cols, rows.min(max_rows))
}

/// The largest number of `DimSegment`s a single daemon-assembled snapshot
/// can ever contain: every surviving `dim_markers` entry
/// ([`MAX_DIM_MARKERS`]), plus at most one synthesized head segment for a
/// single cap eviction (`ScrollbackRingBuffer::read_segments`, D1'''''),
/// plus at most one trailing screen-dump segment for an alt-screen pane
/// (`build_snapshot_bytes_with_layout`'s D7'' segment).
///
/// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
/// with `MAX_DIM_MARKERS` raised to 62, this now evaluates to EXACTLY the
/// wire decoder's own `mux_ipc::protocol::MAX_SEGMENTS` (64) ceiling, not
/// comfortably under it — 62 was chosen as `MAX_SEGMENTS - 2` specifically
/// so the largest daemon-producible segment list exactly saturates, never
/// exceeds, what the decoder accepts. `decode_snapshot_payload_typed`
/// rejects only `count > MAX_SEGMENTS` (strictly greater), so a `count ==
/// MAX_SEGMENTS` payload still decodes as `Structured` — see
/// `largest_daemon_producible_segment_list_round_trips_cleanly` and
/// `largest_real_producer_segment_list_round_trips_cleanly` (the latter
/// drives the REAL ring → snapshot → encode → decode path, not just this
/// constant, closing review round-8 finding `45033eaafbdf8e25`).
const MAX_DAEMON_SNAPSHOT_SEGMENTS: u64 = MAX_DIM_MARKERS as u64 + 2;

/// Producer-side per-segment cell budget (D4''''', round-8 rework, review
/// round-7 finding `4bc6ab813edd6d22`, independently confirmed by
/// `codex:architecture`).
///
/// `clamp_dims_to_wire_domain` previously bounded every segment's own
/// product to the decoder's PER-SEGMENT ceiling
/// (`mux_ipc::protocol::MAX_SEGMENT_CELLS`, 1,000,000) alone — but the
/// decoder ALSO enforces a CUMULATIVE ceiling across every segment in one
/// payload (`MAX_CUMULATIVE_SEGMENT_CELLS`). A daemon snapshot can
/// legitimately carry up to [`MAX_DAEMON_SNAPSHOT_SEGMENTS`] segments, each
/// individually within `MAX_SEGMENT_CELLS` but at `1,000,000` cells apiece
/// that would sum to far past the cumulative ceiling, so the daemon's own
/// peer would reject the ENTIRE snapshot as `Malformed` and the pane would
/// keep showing stale content after every switch (the exact symptom this
/// feature exists to fix). Deriving the producer's per-segment budget from
/// the SAME cumulative ceiling divided by the largest segment count the
/// daemon can ever emit guarantees a daemon-recorded snapshot's segments
/// can never sum past it, regardless of how many distinct dimensions were
/// recorded. `.min(...)` with the decoder's own per-segment ceiling is a
/// belt-and-braces bound (the derived value is already well under it).
///
/// D1'''''' (round-9 rework, review round-8 findings `6082de4e619d7f51` /
/// `45033eaafbdf8e25`): `MAX_DAEMON_SNAPSHOT_SEGMENTS` rose from 26 to 64
/// alongside the marker cap raise, which — left against the OLD
/// `MAX_CUMULATIVE_SEGMENT_CELLS` (8,000,000) — would have shrunk this
/// derived budget from 307,692 to 125,000, a real regression for a large
/// display at a small font (`MAX_SEGMENT_CELLS`'s own doc estimates "a few
/// hundred thousand cells" for that case — already above 125,000).
/// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` was raised in lockstep
/// (32,000,000, its own doc) so this derived budget comes out to 500,000 —
/// ABOVE the pre-round-9 value, not below it. See
/// `producer_segment_cell_budget_fits_a_real_large_terminal` for an
/// executable check that a realistically large terminal size still fits
/// unclamped (AC-2).
const PRODUCER_SEGMENT_CELL_BUDGET: u32 = {
    let derived = mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS / MAX_DAEMON_SNAPSHOT_SEGMENTS;
    let capped = if derived > mux_ipc::protocol::MAX_SEGMENT_CELLS as u64 {
        mux_ipc::protocol::MAX_SEGMENT_CELLS as u64
    } else {
        derived
    };
    capped as u32
};

/// Control sequence that switches a `vt100` shadow parser onto the
/// alternate screen (DECSET 1049), fed ahead of a restored alt-screen
/// dump by [`MuxPane::from_restored`] (mux-hot-upgrade-alt-screen
/// task0002, SPEC FR6).
const ALT_SCREEN_ENTER: &[u8] = b"\x1b[?1049h";

/// Enforce the D1 alt-screen dump size cap (IMPLEMENTATION.md,
/// mux-hot-upgrade-alt-screen): a `dump` whose length exceeds
/// `mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD` (16 MiB minus the frame
/// header — the largest a dump could ever be delivered to a client through
/// the reattach snapshot anyway) is replaced with an empty one; a dump at
/// or under the cap is returned untouched. `pane_id` is only used for the
/// warn-level log line the D1 policy requires (AC-7) — this function does
/// not touch the pane itself, which is what keeps the cap DECISION (as
/// opposed to the shadow-parser read that produces `dump`) unit-testable
/// against a synthetic byte vector, without allocating a real 16 MiB
/// screen (task plan Test Notes AC-7).
fn cap_alt_screen_dump(pane_id: PaneId, dump: Vec<u8>) -> Vec<u8> {
    if dump.len() <= mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD {
        return dump;
    }
    log::warn!(
        "upgrade snapshot: pane {} alt-screen dump ({} bytes) exceeds the D1 cap \
         ({} bytes); storing an empty dump with the alt-screen flag preserved true",
        pane_id,
        dump.len(),
        mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD,
    );
    Vec::new()
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    ///
    /// `child` is the shell child-process handle `spawn_command` returned
    /// (task0001 SPEC FR2), retained for the pane's lifetime and reaped on
    /// [`Self::mark_exited`]. `None` for panes constructed without spawning
    /// a command (`new_test` / `new_test_with_writer`, and direct-PTY test
    /// call sites in this module's own tests).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    ) -> Self {
        // task0004 round-4 rework (review round-3 finding `b546481e9c2fcc85`):
        // validate the pane's INITIAL dimensions against the SAME domain
        // `resize()` enforces before recording them anywhere. Without this,
        // a caller passing an out-of-domain size would have its initial
        // segment silently clamped/rejected only at REPLAY time,
        // misaligning the earliest retained content's attributed
        // dimensions from what was actually recorded here.
        //
        // D6'''' (round-7 rework, review round-6 finding `6cefb1dd16c126b6`):
        // `clamp_dims_to_wire_domain` also bounds the PRODUCT to what the
        // wire decoder accepts (`MAX_SEGMENT_CELLS`), not just each axis —
        // see its doc for why the per-axis clamp alone let this drift.
        let (clamped_cols, clamped_rows) = clamp_dims_to_wire_domain(cols, rows);
        // D3''''' (round-8 rework, review round-7 finding `1d1b6b6297e3b6a0`,
        // AC-5): `master` was already opened by the caller (`spawn_pty`) at
        // the ORIGINAL, possibly out-of-domain `cols`/`rows` — BEFORE the
        // clamp above runs. Without this, the recorded fields / shadow
        // parser / initial segment would describe dimensions the PTY itself
        // does not actually have (pane creation recording a value the PTY
        // never had). Resize the PTY to the accepted dims whenever the
        // clamp actually changed something, so it can never disagree with
        // what gets recorded below.
        if (clamped_cols, clamped_rows) != (cols, rows) {
            if let Err(e) = master.resize(portable_pty::PtySize {
                rows: clamped_rows,
                cols: clamped_cols,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                log::warn!(
                    "MuxPane::new: failed to resize PTY {} to clamped dims \
                     {}x{} (requested {}x{}): {}",
                    id,
                    clamped_cols,
                    clamped_rows,
                    cols,
                    rows,
                    e
                );
            }
        }
        // D4'''''' (round-9 rework, review round-8 finding
        // `7be271b2ead1bf07`, independently confirmed by
        // `codex:architecture`): read the PTY's ACTUAL size back rather than
        // trusting `clamped_cols`/`clamped_rows` unconditionally. Before this
        // fix, a `master.resize()` failure above was only logged — the PANE
        // still recorded the clamped values as if the PTY had reached them,
        // so every byte the child subsequently produces would be attributed
        // to a size the PTY never actually had, reproducing the exact
        // resize-interleaved coordinate drift this whole feature exists to
        // fix. The sibling `Self::resize` rolls `self.dims` back to the
        // PTY's last-known-good size on the same failure (D7'', task0005);
        // this constructor has no "last known good" to roll back to, so it
        // asks the PTY directly instead. `get_size()` failing too is
        // vanishingly unlikely immediately after a successful `openpty` —
        // fall back to the clamped values in that exceptional case (still
        // in-domain, unlike the caller's raw request).
        let (cols, rows) = match master.get_size() {
            Ok(actual) => (actual.cols, actual.rows),
            Err(e) => {
                log::error!(
                    "MuxPane::new: failed to query PTY {}'s actual size after \
                     a resize attempt; recording the clamped dims {}x{} \
                     on a best-effort basis: {}",
                    id,
                    clamped_cols,
                    clamped_rows,
                    e
                );
                (clamped_cols, clamped_rows)
            }
        };
        let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
        // IMPLEMENTATION.md D1/D2 (task0001): record the pane's INITIAL
        // dimensions as the very first scrollback bytes, mirroring what
        // `resize()` writes at every later transition. Without this, a
        // replay has no marker to resize into before the EARLIEST retained
        // segment (produced under THESE dims), reproducing the
        // resize-interleaved coordinate drift for that leading segment even
        // when every later resize is correctly marked.
        //
        // `write_resize_marker` (not a plain `write`) also records this
        // marker's offset so `ScrollbackRingBuffer::read_all` can
        // reconstruct it after ring wraparound evicts it — review round-1
        // rework, findings `81947e02402b5ace` / `ee93d8be8823e5d7`.
        scrollback.write_resize_marker(cols, rows);
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: Some(master),
            child: child.map(PaneChild::Owned),
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(scrollback)),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }

    /// Create a new pane whose child is referenced by a raw process id
    /// rather than an owned handle (task plan task0007, IMPLEMENTATION.md
    /// D6): a pane restored from a handoff, whose PTY-library child handle
    /// did not survive the process replacement. Unix only, mirroring
    /// [`PaneChild::ProcessId`]'s own gating.
    ///
    /// Identical to [`Self::new`] in every other respect (dimension
    /// clamping, initial scrollback marker, and every other field) — only
    /// the child reference differs, so a restored pane and a freshly
    /// spawned pane are indistinguishable to the rest of the daemon.
    #[cfg(unix)]
    pub fn new_with_process_id(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        pid: u32,
    ) -> Self {
        let mut pane = Self::new(id, cols, rows, output_target, writer, master, None);
        pane.child = Some(PaneChild::ProcessId(pid));
        pane
    }

    /// Write input data to the PTY.
    pub fn write_input(&self, data: &[u8]) -> std::io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"))?;
        write_via_writer_handle(writer, data)
    }

    /// Clone this pane's PTY-writer handle, if the pane still has one
    /// (`None` once [`MuxPane::mark_exited`] has run).
    ///
    /// task0011 lock hygiene: callers that need to perform a synchronous
    /// PTY write WITHOUT holding the `SessionManager` lock for the
    /// duration (e.g. `handle_send_text`) resolve the pane under the
    /// manager lock, clone this handle, release the manager lock, and
    /// write through the cloned handle via [`write_via_writer_handle`].
    /// Because the clone is an `Arc` over the SAME `std::sync::Mutex` as
    /// `self.writer`, concurrent writes — whether via `write_input` or via
    /// a cloned handle — still serialize on that mutex for the full
    /// write+flush, preserving atomic-per-request write semantics.
    pub fn writer_handle(&self) -> Option<Arc<StdMutex<Box<dyn Write + Send>>>> {
        self.writer.clone()
    }

    /// The underlying PTY's ACTUAL current size, as the kernel reports it —
    /// not `self.cols`/`self.rows` (the daemon's own recorded bookkeeping).
    /// AC-5 regression test observer (D3''''', round-8 rework, review
    /// round-7 finding `1d1b6b6297e3b6a0`): lets a test confirm `MuxPane::new`
    /// actually resized the PTY to match the clamped dims it records, rather
    /// than recording a value the PTY never had. `None` once the pane's
    /// master has been dropped ([`Self::mark_exited`]).
    #[cfg(test)]
    fn master_size(&self) -> Option<portable_pty::PtySize> {
        self.master.as_ref().and_then(|m| m.get_size().ok())
    }

    /// Whether this pane currently holds a child handle (task0001 TS-2
    /// observer): `true` from construction with `Some(child)` until
    /// [`Self::mark_exited`] takes it.
    #[cfg(test)]
    fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Resize the PTY to the given dimensions.
    ///
    /// task0003 D2 (review round-2 finding `602e685494248cbb`): `cols` /
    /// `rows` are clamped to the domain the marker decoder accepts BEFORE
    /// anything else — so the PTY's actual size, the shadow parser, and the
    /// recorded marker can never disagree with what a replay is willing to
    /// honor. D6'''' (round-7 rework, review round-6 finding
    /// `6cefb1dd16c126b6`): that domain is `clamp_dims_to_wire_domain`'s,
    /// not `clamp_resize_dims`'s alone — see its doc.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let (cols, rows) = clamp_dims_to_wire_domain(cols, rows);
        let master = self
            .master
            .as_ref()
            .ok_or_else(|| "PTY master closed".to_string())?;
        // IMPLEMENTATION.md D1/D2 (task0001), structural since task0004
        // round-4 rework D1': record a resize segment into the scrollback
        // ring's `dim_markers` side channel (`write_resize_marker`) so a
        // later replay resizes its core to match the dimensions the
        // FOLLOWING bytes were produced for. This is the fix for the
        // resize-interleaved scrollback replay coordinate drift (PROBE D,
        // `tmp/apt-progress-bar-regression-2026-07-09.md`): without it, a
        // scrollback recording spanning a resize replays into a core fixed
        // at one row count and misinterprets DECSTBM / CUP coordinates
        // recorded for the other, mixing content from two logical output
        // lines onto one row. Recorded directly into `self.scrollback`'s
        // `dim_markers` (not fed through the PTY reader thread) since it is
        // synthesized here, not real PTY output — no bytes are written, so
        // there is nothing for PTY-sourced content to collide with (see
        // `ScrollbackRingBuffer::read_segments`). Only recorded when the
        // dimensions actually change, so a redundant Resize message does
        // not pollute the stream.
        //
        // Ordering (review round-1 rework, finding `83bed291fb779f52`,
        // high): `master.resize()` triggers SIGWINCH, and the PTY reader
        // thread can observe the child's response and try to append it to
        // `self.scrollback` concurrently. Pre-fix, `master.resize()` ran
        // OUTSIDE any lock, leaving a real window for the reader thread to
        // win the scrollback lock and record post-resize bytes BEFORE the
        // marker — replay would then interpret them under the OLD
        // dimensions, reproducing the exact coordinate drift this marker
        // exists to prevent. Holding `self.scrollback`'s lock across BOTH
        // the PTY-visible resize and the marker write establishes a single
        // ordering owner: the reader thread needs this SAME lock to append
        // anything, so it cannot record a single byte until AFTER the
        // marker is in place.
        if self.cols != cols || self.rows != rows {
            let mut scrollback = self.scrollback.lock().unwrap();
            // task0004 round-4 rework (review round-3 finding
            // `5ac1a5171a1e6a58`): publish the new dims via `self.dims`
            // BEFORE `master.resize()` sends SIGWINCH — still inside this
            // same scrollback-locked section. `master.resize()` is what
            // makes the child observe the new size and start producing
            // output at it; if the reader thread's `read()` returns that
            // output and calls `PaneDims::get()` in the gap BETWEEN
            // `master.resize()` and the OLD placement of this `set()` call
            // (after resize + marker write), it would see the OLD dims and
            // misattribute genuinely-new-size content via
            // `ScrollbackRingBuffer::attribute_write`'s correction path —
            // the opposite of what that path exists to prevent. Publishing
            // first closes that window: by the time the child could
            // possibly react to SIGWINCH, `PaneDims` already reports the
            // size the reaction was produced under.
            let (old_cols, old_rows) = (self.cols, self.rows);
            self.dims.set(cols, rows);
            if let Err(e) = master.resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                // D7'' (task0005 rework, review round-4 finding
                // `ef9ab1689853785c`, medium): `master.resize()` failing
                // means the PTY never actually changed size — but `self.dims`
                // was already published above (needed to close the OTHER
                // race this ordering fixes). Left as-is, `PaneDims` would
                // keep reporting a size the PTY was never at; the reader
                // thread reads it on every chunk and hands it to
                // `ScrollbackRingBuffer::attribute_write`, which — seeing a
                // mismatch against the ring's last-recorded dims — would
                // record a CORRECTIVE marker for those bogus dims, and every
                // later chunk in this pane's scrollback would be attributed
                // to a size that never existed. Roll `self.dims` back to the
                // size the PTY still actually has (still inside this same
                // scrollback-locked section, so no reader-thread read can
                // observe the bogus dims and misattribute against them
                // between the `set` above and this rollback) before
                // returning the error. `self.cols`/`self.rows` were never
                // updated on this path (still assigned after this whole
                // `if`/`else`), so they already agree with the rollback.
                self.dims.set(old_cols, old_rows);
                return Err(format!("PTY resize failed: {}", e));
            }
            scrollback.write_resize_marker(cols, rows);
        } else {
            master
                .resize(portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("PTY resize failed: {}", e))?;
        }
        self.cols = cols;
        self.rows = rows;
        let mut parser = lock_shadow_parser(&self.shadow_parser);
        let resized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parser.screen_mut().set_size(rows, cols);
        }));
        if resized.is_err() {
            // vt100 panicked mid-resize; its internal state may be torn.
            // Rebuild so subsequent output re-populates the shadow screen.
            *parser = new_shadow_parser(rows, cols);
            log::error!(
                "pane {}: shadow parser panicked during resize; parser reset",
                self.id
            );
        }
        Ok(())
    }

    /// Apply an ACCEPTED agent-status report (SPEC FR3): update
    /// state/name and increment revision. Returns the resulting revision.
    ///
    /// Callers must only invoke this with an `AgentStatusEvent` that
    /// `agent_status::parse` already returned `Some` for — a rejected
    /// (`None`) parse must never reach here, which is what leaves state and
    /// revision untouched on rejection (AC-2).
    ///
    /// task0003 (SPEC FR1): every accepted event also feeds this pane's
    /// inferred-clear latch — `Set` arms it (new generation), `Clear`
    /// disarms it. This is the SINGLE place an explicit Set/Clear reaches
    /// the latch, so [`Self::record_live_osc133_mark`] (the live OSC 133
    /// feed) and this method can never drift out of sync with each other.
    pub fn apply_agent_status_event(&self, event: AgentStatusEvent) -> u64 {
        let is_clear = matches!(event, AgentStatusEvent::Clear);
        let revision = {
            let mut status = self.agent_status.lock().unwrap();
            match event {
                AgentStatusEvent::Set { state, name } => {
                    status.state = Some(state);
                    status.name = name;
                }
                AgentStatusEvent::Clear => {
                    status.state = None;
                    status.name = None;
                }
            }
            status.revision += 1;
            status.revision
        };
        let mut latch = self.agent_status_exit_latch.lock().unwrap();
        if is_clear {
            latch.record_clear();
        } else {
            latch.record_set();
        }
        revision
    }

    /// Record a live, main-screen-observed OSC 133 mark for this pane
    /// (task0001 `AgentStatusExitLatch`, SPEC FR1/FR2) and, if it completes
    /// an armed `D`→`A` transition, apply the resulting inferred clear
    /// through the EXACT SAME path an explicit `Clear` report takes
    /// ([`Self::apply_agent_status_event`]) — same revision increment,
    /// same downstream broadcast the caller drives from the returned
    /// revision, no parallel clear logic (FR2).
    ///
    /// Callers (the daemon's live PTY reader path for this pane) must only
    /// ever pass marks that are LIVE and main-screen-observed, in true
    /// arrival order relative to this pane's Set/Clear reports (FR4/FR5) —
    /// this method has no way to tell a live mark from a replayed one, so
    /// that guarantee is entirely the caller's responsibility.
    ///
    /// Returns `Some(revision)` when an inferred clear fired; `None` when
    /// the mark produced no state change (AC-2/AC-3: e.g. an `A` with no
    /// preceding `D`, or any mark while disarmed).
    pub fn record_live_osc133_mark(&self, kind: PromptMarkKind) -> Option<u64> {
        let fired = {
            let mut latch = self.agent_status_exit_latch.lock().unwrap();
            latch.record_mark(kind)
        };
        if fired {
            Some(self.apply_agent_status_event(AgentStatusEvent::Clear))
        } else {
            None
        }
    }

    /// Mark PTY as exited (task0001 SPEC FR3/FR4, task plan D2; task plan
    /// task0007, IMPLEMENTATION.md D6).
    ///
    /// Clears the writer/master (dropping the master delivers the hangup to
    /// the shell) and — when a child reference is present — takes it out of
    /// the pane and hands it off to the matching [`child_reaper`] entry
    /// point for a bounded, off-thread reap: an owned handle goes through
    /// [`child_reaper::spawn_reaper`] (unchanged, task0001), a process id
    /// (a pane restored from a handoff) goes through
    /// [`child_reaper::spawn_reaper_pid`] (task0007). Taking the reference
    /// here is the multi-call gate: a second `mark_exited` (concurrent
    /// teardown paths racing, e.g. destroy-pane racing graceful-shutdown, or
    /// the PTY-EOF reap racing an explicit destroy) finds no reference and
    /// starts no second reap.
    ///
    /// Returns immediately: performs no waiting of any kind, so callers
    /// holding the `SessionManager` lock are never blocked on the child's
    /// exit (NFR1).
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
        self.master = None;
        match self.child.take() {
            Some(PaneChild::Owned(child)) => {
                child_reaper::spawn_reaper(self.id, child);
            }
            #[cfg(unix)]
            Some(PaneChild::ProcessId(pid)) => {
                child_reaper::spawn_reaper_pid(self.id, pid);
            }
            None => {}
        }
    }

    /// Raw fd number of this pane's PTY master (task0003 AC-2 snapshot
    /// accessor): the daemon clears its `FD_CLOEXEC` flag and records this
    /// number so the descriptor survives `execve`. `None` once the master
    /// has been dropped ([`Self::mark_exited`]) or for a pane with no
    /// master at all (test-only construction).
    #[cfg(unix)]
    pub fn master_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.master.as_ref().and_then(|m| m.as_raw_fd())
    }

    /// This pane's shell child process id (task0003 AC-2 / IMPLEMENTATION.md
    /// D6 snapshot accessor), regardless of whether the child is an owned
    /// handle ([`PaneChild::Owned`], a freshly spawned pane) or a bare
    /// process id ([`PaneChild::ProcessId`], a pane itself previously
    /// restored from an earlier handoff). `None` once the child has been
    /// reaped ([`Self::mark_exited`]) or for a pane with no child reference
    /// at all (test-only construction).
    pub fn child_pid(&self) -> Option<u32> {
        match &self.child {
            Some(PaneChild::Owned(child)) => child.process_id(),
            #[cfg(unix)]
            Some(PaneChild::ProcessId(pid)) => Some(*pid),
            None => None,
        }
    }

    /// Capture this pane's alternate-screen state for the hot-upgrade
    /// handoff document (mux-hot-upgrade-alt-screen task0002, SPEC FR5/FR7/
    /// FR8): `(alt_screen, dump)`, where `dump` is the shadow parser's
    /// formatted alternate-screen contents when `alt_screen` is true, empty
    /// otherwise.
    ///
    /// Implements the SAME main/alt split `build_snapshot_bytes` already
    /// uses (`crate::mux::snapshot_bytes`): under the shadow-parser lock,
    /// read whether the parser is on the alternate screen, and only take
    /// the (expensive) `contents_formatted()` dump when it is. This is the
    /// ONE helper both `snapshot_pane` (initial capture) and
    /// `refresh_live_agent_state` (task0006 re-capture) call, so the split
    /// logic never forks between the two call sites.
    ///
    /// The D1 size cap (IMPLEMENTATION.md) is enforced here: a dump whose
    /// length exceeds `mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD` is
    /// replaced with an empty one — the flag stays true (restore still
    /// enters the alternate screen, just blank — IMPLEMENTATION.md D1) — and
    /// a warn-level log line names the pane id and the oversize length
    /// (AC-7).
    ///
    /// Callers are responsible for the "exited pane contributes flag false"
    /// rule (task plan Design "Capture"): this method reports the shadow
    /// parser's CURRENT state regardless of `self.exited`, since an exited
    /// pane's parser reflects whatever it last observed, which is not the
    /// contract callers want for an exited pane's document entry.
    pub fn capture_alt_state(&self) -> (bool, Vec<u8>) {
        let (alt_screen, dump) = {
            let parser = lock_shadow_parser(&self.shadow_parser);
            let alt = parser.screen().alternate_screen();
            let dump = if alt {
                parser.screen().contents_formatted()
            } else {
                Vec::new()
            };
            (alt, dump)
        };
        if !alt_screen {
            return (false, Vec::new());
        }
        (true, cap_alt_screen_dump(self.id, dump))
    }

    /// Construct a live pane around an adopted master, for restore from a
    /// handoff document (task0003 AC-1/AC-3/AC-4/AC-5). Builds on
    /// [`Self::new_with_process_id`] / [`Self::new`] for every field their
    /// construction already gets right (dimension clamping, child
    /// reference, the wire-up new panes get) and then overwrites `cwd` /
    /// `title` / agent-status / scrollback with the restored values instead
    /// of the empty ones those constructors start with. The shadow parser's
    /// view is rebuilt by REPLAYING the restored scrollback
    /// (IMPLEMENTATION.md D8) rather than expecting serialised parser
    /// state — mirroring the panic-recovery convention `pty_reader_loop`
    /// already uses for live output.
    ///
    /// `alt_screen` / `alt_screen_dump` (mux-hot-upgrade-alt-screen
    /// task0002, SPEC FR6): after the scrollback replay above, when
    /// `alt_screen` is true, the alternate-screen-enter control sequence
    /// (`ESC [?1049h`) followed by `alt_screen_dump` is fed into the SAME
    /// shadow parser under the SAME panic guard as the scrollback replay —
    /// so a formerly-alt-screen pane's parser reports the alternate screen
    /// as active again, with the dump's content on it and the replayed
    /// scrollback beneath. `alt_screen == false` feeds nothing extra:
    /// byte-identical to the pre-task0002 behavior. `alt_screen == true`
    /// with an empty `alt_screen_dump` (the D1 overflow shape) still enters
    /// the alternate screen, just blank.
    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    pub fn from_restored(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        scrollback: ScrollbackRingBuffer,
        cwd: Option<String>,
        title: Option<String>,
        agent_status: AgentStatus,
        restored_child_pid: Option<u32>,
        alt_screen: bool,
        alt_screen_dump: Vec<u8>,
    ) -> Self {
        let pane = match restored_child_pid {
            Some(pid) => {
                Self::new_with_process_id(id, cols, rows, output_target, writer, master, pid)
            }
            None => Self::new(id, cols, rows, output_target, writer, master, None),
        };

        *pane.cwd.lock().unwrap() = cwd;
        *pane.title.lock().unwrap() = title;
        *pane.agent_status.lock().unwrap() = agent_status;

        let replay_bytes = scrollback.read_all();
        *pane.scrollback.lock().unwrap() = scrollback;
        if !replay_bytes.is_empty() {
            let mut parser = lock_shadow_parser(&pane.shadow_parser);
            let (parser_rows, parser_cols) = parser.screen().size();
            let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parser.process(&replay_bytes);
            }));
            if processed.is_err() {
                *parser = new_shadow_parser(parser_rows, parser_cols);
                log::error!(
                    "pane {}: shadow parser panicked while replaying {} bytes of \
                     restored scrollback; parser reset",
                    id,
                    replay_bytes.len()
                );
            }
        }

        if alt_screen {
            let mut parser = lock_shadow_parser(&pane.shadow_parser);
            let (parser_rows, parser_cols) = parser.screen().size();
            let mut alt_bytes = Vec::with_capacity(ALT_SCREEN_ENTER.len() + alt_screen_dump.len());
            alt_bytes.extend_from_slice(ALT_SCREEN_ENTER);
            alt_bytes.extend_from_slice(&alt_screen_dump);
            let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parser.process(&alt_bytes);
            }));
            if processed.is_err() {
                *parser = new_shadow_parser(parser_rows, parser_cols);
                log::error!(
                    "pane {}: shadow parser panicked while replaying {} bytes of \
                     restored alt-screen state; parser reset",
                    id,
                    alt_bytes.len()
                );
            }
        }

        pane
    }

    /// Construct an already-exited pane directly from restored parts
    /// (task0003 AC-6/AC-7): no descriptor is adopted — either the
    /// document recorded no live descriptor (AC-6), or restore could not
    /// adopt the one it recorded (AC-7) — and no child reference is
    /// retained, since an exited pane has nothing left to reap.
    pub fn from_restored_exited(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        scrollback: ScrollbackRingBuffer,
        cwd: Option<String>,
        title: Option<String>,
        agent_status: AgentStatus,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: None,
            master: None,
            child: None,
            exited: true,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(cwd)),
            title: Arc::new(StdMutex::new(title)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(agent_status)),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(scrollback)),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }

    /// Create a pane without a PTY master, for testing only.
    #[cfg(test)]
    pub fn new_test(id: PaneId, cols: u16, rows: u16, output_target: SharedOutputTarget) -> Self {
        Self::new_test_with_writer(id, cols, rows, output_target, Box::new(std::io::sink()))
    }

    /// Like [`Self::new_test`], but with a caller-supplied writer so tests
    /// can capture exactly what `write_input` sends (e.g. `SendText`'s "no
    /// trailing newline added" contract, task0004 AC-2).
    #[cfg(test)]
    pub fn new_test_with_writer(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: None,
            child: None,
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            agent_status_exit_latch: Arc::new(StdMutex::new(AgentStatusExitLatch::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(ScrollbackRingBuffer::new(
                DEFAULT_SCROLLBACK_CAPACITY,
            ))),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output_target() -> SharedOutputTarget {
        let (tx, _rx) = mpsc::channel(1);
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
    }

    /// Decode a `Snapshot`-kind chunk's / `EvalResult::ResumeWithSnapshot`'s
    /// wire-encoded bytes (task0004 round-4 rework D1',
    /// `mux_ipc::protocol::decode_snapshot_payload`) back into plain
    /// content bytes, discarding the structural segment header — used by
    /// tests that only care about the ANSI content layout.
    fn decode_snapshot_content(data: &[u8]) -> Vec<u8> {
        mux_ipc::protocol::decode_snapshot_payload(data).1.to_vec()
    }

    // ── AgentStatus (SPEC FR3, task0003 AC-1/AC-2/AC-6) ──────────────────

    #[test]
    fn test_new_pane_has_no_agent_status_and_revision_zero() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.name, None);
        assert_eq!(status.revision, 0);
    }

    /// AC-1: a Set event updates state/name and increments revision.
    #[test]
    fn test_apply_agent_status_event_set_updates_state_and_increments_revision() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);

        let revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
        assert_eq!(revision, 1);

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, Some(AgentState::Working));
        assert_eq!(status.name.as_deref(), Some("claude"));
        assert_eq!(status.revision, 1);
    }

    /// AC-1: a Clear event empties state/name and increments revision.
    #[test]
    fn test_apply_agent_status_event_clear_empties_state_and_increments_revision() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Blocked,
            name: Some("agent".to_string()),
        });

        let revision = pane.apply_agent_status_event(AgentStatusEvent::Clear);
        assert_eq!(revision, 2);

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.name, None);
        assert_eq!(status.revision, 2);
    }

    /// AC-2: a same-state re-report still increments revision (it is only
    /// ever invoked for an ACCEPTED event; "same state" is not itself a
    /// rejection reason).
    #[test]
    fn test_apply_agent_status_event_same_state_re_report_increments_revision() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        let r1 = pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        });
        let r2 = pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        });
        assert_eq!(r1, 1);
        assert_eq!(r2, 2);
    }

    /// AC-2: rejected sequences (parse returning `None`) never reach
    /// `apply_agent_status_event`, so state/revision are naturally
    /// untouched. This test pins that contract at the call-site level: a
    /// caller that only calls `apply_agent_status_event` for `Some(event)`
    /// leaves state/revision alone when `agent_status::parse` rejects.
    #[test]
    fn test_rejected_parse_never_reaches_apply_leaves_state_untouched() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Idle,
            name: None,
        });

        // Simulate the caller's contract: a rejected report is never
        // applied.
        let rejected = crate::agent_status::parse("emterm;agent-status;v=1;state=bogus");
        assert_eq!(rejected, None);
        if let Some(event) = rejected {
            pane.apply_agent_status_event(event);
        }

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, Some(AgentState::Idle));
        assert_eq!(status.revision, 1);
    }

    /// AC-6: pane destroy discards agent-status state — `MuxWindow::remove_pane`
    /// drops the `MuxPane` (and its `Arc<Mutex<AgentStatus>>`) entirely, so a
    /// removed pane's status is gone, not merely reset.
    #[test]
    fn test_pane_removal_discards_agent_status() {
        use super::super::window::MuxWindow;

        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Done,
            name: Some("agent".to_string()),
        });
        let status_handle = pane.agent_status.clone();
        assert_eq!(Arc::strong_count(&status_handle), 2, "pane + our clone");

        let mut window = MuxWindow::new(1, "w".to_string());
        window.add_pane(pane);
        let removed = window.remove_pane(1);
        assert!(removed.is_some());
        drop(removed);

        // The pane (and its only other Arc handle to `agent_status`) is
        // gone; only our test-held clone remains.
        assert_eq!(
            Arc::strong_count(&status_handle),
            1,
            "agent_status must be discarded along with the destroyed pane"
        );
    }

    // ── task0003: inferred-clear latch wiring (SPEC FR1/FR2/FR3) ─────────

    /// AC-7: a freshly created pane's inferred-clear latch is present and
    /// disarmed (mirrors "new pane has no agent-status", the sibling field
    /// this one is shaped after).
    #[test]
    fn test_new_pane_has_disarmed_latch() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        assert_eq!(
            *pane.agent_status_exit_latch.lock().unwrap(),
            AgentStatusExitLatch::new()
        );
    }

    /// AC-1 (pane-level): `Set` then live `D` then live `A` fires the
    /// inferred clear through `apply_agent_status_event`'s exact effects —
    /// state becomes `None` and the revision increments exactly once more.
    #[test]
    fn test_record_live_osc133_mark_set_then_d_then_a_fires_inferred_clear() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        let set_revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
        assert_eq!(set_revision, 1);

        let d_result = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
        assert_eq!(d_result, None, "a lone D must not fire a clear");

        let a_result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(a_result, Some(2), "D followed by A must fire exactly once");

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.name, None);
        assert_eq!(status.revision, 2);
    }

    /// AC-2 (pane-level): `Set` followed only by live `A` (no `D`) leaves
    /// state unchanged — no inferred clear, no revision bump.
    #[test]
    fn test_record_live_osc133_mark_a_without_prior_d_is_a_noop() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Blocked,
            name: None,
        });

        let result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(result, None);

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, Some(AgentState::Blocked));
        assert_eq!(status.revision, 1);
    }

    /// AC-3 (pane-level): an explicit `Clear` disarms the latch, so a
    /// subsequent live `D`/`A` pair does not produce a second/duplicate
    /// clear or a second revision increment.
    #[test]
    fn test_record_live_osc133_mark_after_explicit_clear_is_a_noop() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Done,
            name: None,
        });
        let clear_revision = pane.apply_agent_status_event(AgentStatusEvent::Clear);
        assert_eq!(clear_revision, 2);

        let d_result = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
        assert_eq!(d_result, None);
        let a_result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(a_result, None);

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 2, "no third revision from D/A after Clear");
    }

    /// AC-4: OSC 133 marks captured on scrollback content REPLAYED for a
    /// reattach/visibility-resume snapshot (`resume_pane_with_permit`, the
    /// real production snapshot-construction path — not a hand-rolled
    /// substitute) never drive the latch. Even with a full `Set` -> `D`
    /// -> `A` byte sequence sitting in scrollback, building and sending a
    /// resume snapshot from it must leave `agent_status` exactly as the
    /// explicit report left it.
    #[tokio::test]
    async fn test_resume_snapshot_construction_with_osc133_bytes_in_scrollback_never_fires_latch()
     {
        let (owned_tx, _rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(9, 80, 24, target.clone());

        // A full Set -> D -> A OSC 133 byte sequence, literally present in
        // scrollback content (as it would be after a real shell session) —
        // nothing strips OSC 133 bytes from scrollback (it is not a
        // viewer-launch sequence), so this is exactly what a replay would
        // carry.
        pane.scrollback
            .lock()
            .unwrap()
            .write(b"$ claude\r\n\x1b]133;D\x07\x1b]133;A\x07$ ");
        let set_revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
        assert_eq!(set_revision, 1);

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::Resumed));

        // The real snapshot-construction path ran (and — for a sanity
        // check that this test actually exercised the D/A bytes — the
        // scrollback content in fact contains them), yet the latch/state
        // must be untouched by it.
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(
            status.state,
            Some(AgentState::Working),
            "snapshot/replay construction must never fire the inferred-clear latch"
        );
        assert_eq!(status.revision, 1, "no extra revision from snapshot assembly");
        assert_eq!(
            *pane.agent_status_exit_latch.lock().unwrap(),
            {
                let mut expected = AgentStatusExitLatch::new();
                expected.record_set();
                expected
            },
            "the latch must still be exactly what the explicit Set left it as"
        );
    }

    // ── task0004 round-4 rework (review round-3 finding `b546481e9c2fcc85`):
    // pane creation validates dims against the same domain resize() uses ──

    /// AC-6: `MuxPane::new` clamps out-of-domain dimensions through the
    /// SAME path `resize()` uses (`clamp_dims_to_wire_domain`), instead of
    /// storing the caller's raw values unvalidated. Uses a real PTY (like
    /// the existing `test_new_pane_records_initial_dims_marker_in_scrollback`)
    /// since the test-only `new_test`/`new_test_with_writer` constructors
    /// are a separate, simplified path that does not call `MuxPane::new`
    /// at all.
    ///
    /// Confirmed to fail pre-fix: before this change, `MuxPane::new` stored
    /// `cols`/`rows` directly (no clamp call at all), so passing `(0, 0)`
    /// left `pane.cols == 0` — outside `clamp_resize_dims`'s `1..=4096`
    /// domain that this task's replay path assumes dimensions never
    /// violate.
    ///
    /// D6'''' (round-7 rework, review round-6 finding `6cefb1dd16c126b6`):
    /// `u16::MAX` per axis clamps to `RESIZE_MARKER_MAX_COLS` /
    /// `RESIZE_MARKER_MAX_ROWS` (4096 each) — a product of 16,777,216,
    /// still far above the wire decoder's per-segment ceiling. `rows` must
    /// clamp FURTHER, preserving `cols` at the per-axis max.
    ///
    /// D4''''' (round-8 rework, review round-7 finding `4bc6ab813edd6d22`):
    /// the product ceiling this now clamps to is
    /// `PRODUCER_SEGMENT_CELL_BUDGET` (derived from the decoder's
    /// CUMULATIVE budget), not the decoder's raw per-segment
    /// `MAX_SEGMENT_CELLS` (1,000,000) — see that constant's doc.
    #[cfg(unix)]
    #[test]
    fn new_pane_clamps_out_of_domain_dimensions() {
        let pty_system = portable_pty::native_pty_system();
        // `portable_pty` itself may reject a literal 0x0 openpty size on
        // some platforms, so this drives the clamp with an OVERSIZED value
        // instead (still out of `clamp_resize_dims`'s domain) to keep the
        // PTY open call itself valid.
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let pane = MuxPane::new(1, u16::MAX, u16::MAX, target, writer, pair.master, None);
        let expected_cols = term_core::terminal_core::RESIZE_MARKER_MAX_COLS;
        let expected_rows = (PRODUCER_SEGMENT_CELL_BUDGET / expected_cols as u32) as u16;
        assert_eq!(
            (pane.cols, pane.rows),
            (expected_cols, expected_rows),
            "oversized dimensions must clamp down to the wire domain \
             (per-axis max, THEN product ceiling), matching \
             clamp_dims_to_wire_domain"
        );
        assert!(
            (pane.cols as u32) * (pane.rows as u32) <= mux_ipc::protocol::MAX_SEGMENT_CELLS,
            "clamped dims must never exceed MAX_SEGMENT_CELLS as a product"
        );
        // The clamped dims are ALSO what gets recorded structurally (the
        // initial segment `MuxPane::new` writes) — not the caller's raw,
        // out-of-domain values.
        let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert_eq!(segments, vec![(0usize, expected_cols, expected_rows)]);
        // AC-5 (D3''''', round-8 rework, review round-7 finding
        // `1d1b6b6297e3b6a0`): the PTY itself was opened at (80, 24) — a
        // DIFFERENT size than the clamped dims recorded above. `MuxPane::new`
        // must resize the ACTUAL PTY to match what it records, not leave it
        // at whatever size the caller happened to open it at.
        //
        // Confirmed to fail pre-fix: before this change, `MuxPane::new`
        // never resized `master` at all, so `master_size()` would still
        // report the PTY's ORIGINAL open size (80, 24) — disagreeing with
        // the (4096, 244) this test records above.
        let actual = pane
            .master_size()
            .expect("PTY master must still be present");
        assert_eq!(
            (actual.cols, actual.rows),
            (expected_cols, expected_rows),
            "MuxPane::new must resize the underlying PTY to the CLAMPED \
             dims it records, not leave it at whatever size the caller \
             originally opened it at"
        );
    }

    /// AC-8 (D6'''', round-7 rework, review round-6 finding
    /// `6cefb1dd16c126b6`): dimensions the daemon accepts always produce a
    /// snapshot segment the wire decoder ALSO accepts — round-trips
    /// `clamp_dims_to_wire_domain`'s output through the REAL
    /// `mux_ipc::protocol` encode/decode path, not just an inline product
    /// check, so a drift between the two crates' notions of "in domain"
    /// would surface here even if a future change duplicated the ceiling
    /// incorrectly instead of sharing `MAX_SEGMENT_CELLS`.
    ///
    /// Confirmed to fail pre-fix: before `clamp_dims_to_wire_domain`
    /// existed, `clamp_resize_dims(2000, 600)` returned `(2000, 600)`
    /// unchanged (both axes are within `1..=4096`) — a product of
    /// 1,200,000, which `mux_ipc::protocol`'s segment decoder rejects as
    /// `Malformed` (`> MAX_SEGMENT_CELLS`). This test's "round-trips
    /// cleanly" assertion would fail against that dimension pair.
    #[test]
    fn clamp_dims_to_wire_domain_output_always_decodes_cleanly() {
        for (raw_cols, raw_rows) in [
            (2000u16, 600u16), // in-per-axis-domain, over-product (the finding's exact repro)
            (4096, 4096),      // both axes at the per-axis max
            (u16::MAX, u16::MAX),
            (1, 1),
            (80, 24),
        ] {
            let (cols, rows) = clamp_dims_to_wire_domain(raw_cols, raw_rows);
            assert!(
                (cols as u32) * (rows as u32) <= mux_ipc::protocol::MAX_SEGMENT_CELLS,
                "clamp_dims_to_wire_domain({raw_cols}, {raw_rows}) = \
                 ({cols}, {rows}) still exceeds MAX_SEGMENT_CELLS as a product"
            );
            // Round-trip through the REAL wire encode/decode, mirroring
            // what a snapshot carrying this pane's initial segment does.
            let segments = [mux_ipc::protocol::DimSegment {
                offset: 0,
                cols,
                rows,
            }];
            let payload = mux_ipc::protocol::encode_snapshot_payload(&segments, b"x");
            let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&payload);
            assert!(
                matches!(
                    decoded,
                    mux_ipc::protocol::DecodedSnapshotPayload::Structured { .. }
                ),
                "clamp_dims_to_wire_domain({raw_cols}, {raw_rows}) = \
                 ({cols}, {rows}) produced a segment the wire decoder \
                 rejected as Malformed: {decoded:?}"
            );
        }
    }

    /// AC-6 (D4''''', round-8 rework, review round-7 finding
    /// `4bc6ab813edd6d22`, independently confirmed by `codex:architecture`):
    /// the LARGEST segment list the daemon can actually produce — every one
    /// of `MAX_DAEMON_SNAPSHOT_SEGMENTS` segments at the producer's own
    /// per-segment cell budget — decodes successfully, not `Malformed`.
    /// This test builds the segment LIST from the constants themselves
    /// (a structural/tautological check); `largest_real_producer_segment_
    /// list_round_trips_cleanly` below drives the REAL ring → snapshot →
    /// encode → decode path instead, so a future drift between these
    /// constants and what the producer actually emits is still caught
    /// (review round-8 finding `45033eaafbdf8e25`, AC-7).
    ///
    /// Confirmed to fail pre-fix: before D4''''' existed,
    /// `clamp_dims_to_wire_domain` bounded every segment to the decoder's
    /// PER-SEGMENT ceiling alone (`MAX_SEGMENT_CELLS`, 1,000,000) — a full
    /// `MAX_DAEMON_SNAPSHOT_SEGMENTS`-segment list at that size sums to far
    /// more than `MAX_CUMULATIVE_SEGMENT_CELLS`, so
    /// `decode_snapshot_payload_typed` would return `Malformed` for this
    /// exact payload and the assertion below would fail.
    #[test]
    fn largest_daemon_producible_segment_list_round_trips_cleanly() {
        let (cols, rows) = clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
        let segment_count = MAX_DAEMON_SNAPSHOT_SEGMENTS as usize;
        let segments: Vec<mux_ipc::protocol::DimSegment> = (0..segment_count)
            .map(|i| mux_ipc::protocol::DimSegment {
                offset: i as u32,
                cols,
                rows,
            })
            .collect();
        let content = vec![b'x'; segment_count];
        let payload = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
        let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&payload);
        match decoded {
            mux_ipc::protocol::DecodedSnapshotPayload::Structured {
                segments: decoded_segments,
                ..
            } => {
                assert_eq!(decoded_segments.len(), segment_count);
            }
            other => panic!(
                "the largest segment list the daemon can produce \
                 ({segment_count} segments at {cols}x{rows}) must decode as \
                 Structured, not {other:?}"
            ),
        }
    }

    /// AC-7, D5'''''' (round-9 rework, review round-8 finding
    /// `45033eaafbdf8e25`): drives the REAL producer path — a real
    /// `ScrollbackRingBuffer` → `read_segments` → `build_snapshot_bytes` →
    /// `encode_snapshot_segments` → `decode_snapshot_payload_typed` — at
    /// the LARGEST shape the daemon can actually produce (the cap
    /// saturated with exactly one eviction, so `read_segments` synthesizes
    /// a head segment, plus a trailing alt-screen segment), instead of
    /// `largest_daemon_producible_segment_list_round_trips_cleanly`'s
    /// structural check, which builds its segment list from
    /// `MAX_DAEMON_SNAPSHOT_SEGMENTS`/`PRODUCER_SEGMENT_CELL_BUDGET`
    /// themselves and so cannot detect either constant drifting from what
    /// the real producer emits.
    ///
    /// Confirmed to fail pre-fix: reverting
    /// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` to its pre-
    /// round-9 value (8,000,000) while leaving `MAX_DIM_MARKERS` at 62
    /// (`MAX_DAEMON_SNAPSHOT_SEGMENTS` == 64) derives a per-segment budget
    /// of 125,000 — the `assert_eq!` on `(cols, rows)` below (asserting
    /// this test's 700×700 == 490,000-cell shape survives
    /// `clamp_dims_to_wire_domain` UNCLAMPED) fails first, surfacing the
    /// drift instead of masking it behind a silently-smaller recorded
    /// size.
    #[test]
    fn largest_real_producer_segment_list_round_trips_cleanly() {
        let (cols, rows) = clamp_dims_to_wire_domain(700, 700);
        assert_eq!(
            (cols, rows),
            (700, 700),
            "test prerequisite: this shape must fit PRODUCER_SEGMENT_CELL_BUDGET \
             unclamped, or this test no longer drives the LARGEST real shape \
             the producer can emit"
        );

        // Saturate `dim_markers` with exactly ONE cap eviction: MAX_DIM_MARKERS
        // + 1 real resize markers, each separated by real content so none
        // coalesce (`write_resize_marker` only coalesces when the offset is
        // UNCHANGED since the last entry).
        let content_per_step: &[u8] = b"real-producer-step;";
        let step_count = MAX_DIM_MARKERS + 1;
        let capacity = step_count * content_per_step.len() + 4096;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        for _ in 0..step_count {
            rb.write_resize_marker(cols, rows);
            rb.write(content_per_step);
        }
        let (raw, segments) = rb.read_segments();
        assert_eq!(
            segments.len(),
            MAX_DIM_MARKERS + 1,
            "test prerequisite: exactly one cap eviction must synthesize the \
             head segment (D1''''')"
        );

        // Trailing alt-screen dump segment (D7''): non-empty `screen` plus a
        // non-empty `scrollback_segments` appends one more segment at
        // `current_dims`, reaching the daemon's true maximum.
        let screen = vec![b'S'; 100];
        let (payload_bytes, snapshot_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &raw,
            &segments,
            &screen,
            true,
            (cols, rows),
        );
        assert_eq!(
            snapshot_segments.len(),
            MAX_DAEMON_SNAPSHOT_SEGMENTS as usize,
            "test prerequisite: the trailing alt-screen segment must be \
             present, reaching MAX_DAEMON_SNAPSHOT_SEGMENTS"
        );

        let wire_payload = encode_snapshot_segments(&payload_bytes, &snapshot_segments);
        let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&wire_payload);
        match decoded {
            mux_ipc::protocol::DecodedSnapshotPayload::Structured {
                segments: decoded_segments,
                ..
            } => {
                assert_eq!(
                    decoded_segments.len(),
                    MAX_DAEMON_SNAPSHOT_SEGMENTS as usize
                );
            }
            other => panic!(
                "the largest segment list the REAL producer path emits \
                 ({} segments at {cols}x{rows}) must decode as Structured, \
                 not {other:?}",
                MAX_DAEMON_SNAPSHOT_SEGMENTS
            ),
        }
    }

    /// AC-2 (round-9 rework, review round-8 finding `6082de4e619d7f51`):
    /// raising `MAX_DIM_MARKERS` (and so `MAX_DAEMON_SNAPSHOT_SEGMENTS`)
    /// must not shrink `PRODUCER_SEGMENT_CELL_BUDGET` underneath a REAL
    /// large terminal size — not just avoid `Malformed` decodes for a
    /// synthetic worst case. A large display at a small font
    /// (`mux_ipc::protocol::MAX_SEGMENT_CELLS`'s own doc: "a few hundred
    /// thousand cells") must fit unclamped.
    ///
    /// Confirmed to fail pre-fix: reverting
    /// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` to its pre-
    /// round-9 value (8,000,000) against the raised
    /// `MAX_DAEMON_SNAPSHOT_SEGMENTS` (64) derives a 125,000-cell budget —
    /// every shape below (all comfortably under the pre-round-9 307,692
    /// budget, which real terminal sizes were already expected to fit
    /// under) exceeds 125,000 and gets silently clamped, failing the
    /// assertion.
    #[test]
    fn producer_segment_cell_budget_fits_a_real_large_terminal() {
        for (cols, rows) in [(400u16, 900u16), (700, 700), (1000, 500)] {
            let (clamped_cols, clamped_rows) = clamp_dims_to_wire_domain(cols, rows);
            assert_eq!(
                (clamped_cols, clamped_rows),
                (cols, rows),
                "a real large terminal size ({cols}x{rows}, {} cells) must \
                 fit PRODUCER_SEGMENT_CELL_BUDGET ({PRODUCER_SEGMENT_CELL_BUDGET}) \
                 without being clamped down",
                cols as u32 * rows as u32
            );
        }
    }

    // ── task0004 round-4 rework (review round-3 finding `ae43417cee647afa`):
    // PaneDims packs cols/rows into a single AtomicU32 ───────────────────

    /// Pack/unpack round-trips for boundary values, including the shared
    /// max the decoder accepts and adjacent-but-distinguishable pairs
    /// (guards against a swapped high/low half).
    #[test]
    fn pane_dims_pack_unpack_round_trips_boundary_values() {
        for (cols, rows) in [
            (1u16, 1u16),
            (80, 24),
            (4096, 4096),
            (65535, 65535),
            (1, 65535),
            (65535, 1),
        ] {
            let dims = PaneDims::new(cols, rows);
            assert_eq!(dims.get(), (cols, rows));
        }
    }

    /// `set` followed by `get` always observes the LATEST pair, never a mix
    /// of an old and new value — trivially true for a single atomic, but
    /// pinned here as the observable contract this field's whole design
    /// exists to guarantee (review round-3 finding `ae43417cee647afa`).
    #[test]
    fn pane_dims_set_then_get_observes_the_latest_pair_atomically() {
        let dims = PaneDims::new(80, 24);
        assert_eq!(dims.get(), (80, 24));
        dims.set(120, 40);
        assert_eq!(
            dims.get(),
            (120, 40),
            "must never observe a mix like (80, 40) or (120, 24)"
        );
    }

    #[test]
    fn test_resize_fails_without_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        let result = pane.resize(120, 40);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("PTY master closed"));
        // Dimensions should not change on error
        assert_eq!(pane.cols, 80);
        assert_eq!(pane.rows, 24);
    }

    // ── D7'' (task0005 rework, review round-4 finding `ef9ab1689853785c`):
    // a failed `master.resize()` must not leave `PaneDims` advanced ───────

    /// Test double whose `resize` always fails, so `MuxPane::resize` can be
    /// exercised past the point where a REAL master is already open (unlike
    /// `test_resize_fails_without_master`, which only covers the "no master
    /// at all" branch).
    #[cfg(unix)]
    struct FailingResizeMaster;

    #[cfg(unix)]
    impl portable_pty::MasterPty for FailingResizeMaster {
        fn resize(&self, _size: portable_pty::PtySize) -> Result<(), anyhow::Error> {
            Err(anyhow::anyhow!("simulated resize failure"))
        }
        fn get_size(&self) -> Result<portable_pty::PtySize, anyhow::Error> {
            Ok(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
        }
        fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>, anyhow::Error> {
            Err(anyhow::anyhow!("not supported in test double"))
        }
        fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>, anyhow::Error> {
            Err(anyhow::anyhow!("not supported in test double"))
        }
        fn process_group_leader(&self) -> Option<libc::pid_t> {
            None
        }
        fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
            None
        }
    }

    /// D7'': a PTY resize failure must leave `PaneDims` (and
    /// `self.cols`/`self.rows`) unchanged — not advanced to the size the
    /// PTY never actually reached. Left advanced, the reader thread would
    /// read the bogus new size on its very next chunk and hand it to
    /// `ScrollbackRingBuffer::attribute_write`, which — seeing a mismatch
    /// against the ring's last-recorded dims — would record a CORRECTIVE
    /// marker for dimensions the PTY was never actually at, misattributing
    /// every later chunk in this pane's scrollback.
    ///
    /// Confirmed to fail pre-fix: before the rollback, `self.dims.set(cols,
    /// rows)` ran unconditionally before `master.resize()`'s early return,
    /// so a resize failure left `pane.dims.get()` reporting the NEW
    /// (never-applied) size while `pane.cols`/`pane.rows` stayed at the OLD
    /// size — this test's `dims.get()` assertion would then observe
    /// `(120, 40)` instead of the expected `(80, 24)`.
    #[cfg(unix)]
    #[test]
    fn resize_failure_rolls_back_published_dims() {
        let target = make_output_target();
        let mut pane = MuxPane::new(
            1,
            80,
            24,
            target,
            Box::new(std::io::sink()),
            Box::new(FailingResizeMaster),
            None,
        );
        let before = pane.dims.get();
        assert_eq!(before, (80, 24));

        let result = pane.resize(120, 40);
        assert!(result.is_err(), "resize must surface the PTY failure");

        assert_eq!(
            pane.dims.get(),
            before,
            "PaneDims must roll back to the size the PTY actually still has \
             after a failed resize — a stale published size would \
             misattribute every later chunk"
        );
        assert_eq!(pane.cols, 80);
        assert_eq!(pane.rows, 24);

        // No corrective marker should have been recorded either — the
        // ring's only segment is still the pane's initial construction
        // dims.
        let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
    }

    /// AC-6, D4'''''' (round-9 rework, review round-8 finding
    /// `7be271b2ead1bf07`, independently confirmed by `codex:architecture`):
    /// when the corrective `master.resize()` inside `MuxPane::new` FAILS,
    /// the pane must record the PTY's ACTUAL size (queried via
    /// `get_size()`), not the clamped values it never reached — mirroring
    /// `MuxPane::resize`'s own rollback on the same failure just above
    /// (D7'', task0005). Reuses `FailingResizeMaster` (this module,
    /// `get_size()` reports a fixed `(80, 24)`, `resize()` always errors).
    ///
    /// Confirmed to fail pre-fix: before this change, `MuxPane::new`
    /// recorded `(clamped_cols, clamped_rows)` unconditionally once the
    /// resize attempt returned (log-and-continue), so this test — whose
    /// `FailingResizeMaster.resize()` always errors — would have left
    /// `pane.cols`/`pane.rows` at the CLAMPED
    /// `(RESIZE_MARKER_MAX_COLS, ...)` values instead of the simulated
    /// PTY's real, never-changed `(80, 24)`, and the initial scrollback
    /// segment would describe a size the PTY does not have.
    #[cfg(unix)]
    #[test]
    fn new_pane_records_actual_pty_size_when_resize_fails() {
        let target = make_output_target();
        // u16::MAX is out of domain — triggers the clamp-then-resize path;
        // the resize call always fails via `FailingResizeMaster`.
        let pane = MuxPane::new(
            1,
            u16::MAX,
            u16::MAX,
            target,
            Box::new(std::io::sink()),
            Box::new(FailingResizeMaster),
            None,
        );
        assert_eq!(
            (pane.cols, pane.rows),
            (80, 24),
            "when the corrective resize fails, MuxPane::new must record the \
             PTY's ACTUAL size (FailingResizeMaster's get_size(), (80, 24)), \
             not the clamped values it never reached"
        );
        let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16)],
            "the initial scrollback segment must match what the PTY \
             actually has, not the refused clamp"
        );
    }

    #[test]
    fn test_mark_exited_clears_writer_and_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        assert!(!pane.exited);

        pane.mark_exited();
        assert!(pane.exited);

        // Writing should fail after exit
        let result = pane.write_input(b"hello");
        assert!(result.is_err());
    }

    // ── Child handle retention + reap (task0001) ───────────────────────────

    /// A child double that never reports an exit and is deliberately slow
    /// to respond to any query — used to prove `mark_exited` never
    /// synchronously touches the child at all (TS-10, NFR1). If a future
    /// regression made `mark_exited` call any `Child`/`ChildKiller` method
    /// itself, this double's artificial delay would make that regression
    /// obvious in the timing assertion below. The reap this double is
    /// eventually handed off to runs on a detached background thread, so
    /// its slowness never blocks test completion.
    #[derive(Debug)]
    struct SlowExitChild;

    impl portable_pty::ChildKiller for SlowExitChild {
        fn kill(&mut self) -> std::io::Result<()> {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            unimplemented!("not exercised by this test")
        }
    }

    impl portable_pty::Child for SlowExitChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Ok(None)
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Ok(portable_pty::ExitStatus::with_exit_code(0))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    /// AC-2 (TS-1): `mark_exited` on a pane with no child handle (the
    /// `new_test` construction, which never had a child to begin with)
    /// starts no reap and does not panic.
    #[test]
    fn mark_exited_on_childless_pane_does_not_panic() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        assert!(!pane.has_child());

        pane.mark_exited(); // must not panic

        assert!(pane.exited);
    }

    /// AC-3 (TS-2): `mark_exited` removes the child handle from the pane —
    /// a second call (concurrent teardown paths racing) finds no handle,
    /// does not panic, and starts no second reap.
    #[cfg(unix)]
    #[test]
    fn mark_exited_removes_child_handle_and_second_call_is_a_noop() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(
            1,
            80,
            24,
            target,
            writer,
            pair.master,
            Some(Box::new(SlowExitChild)),
        );
        assert!(pane.has_child());

        pane.mark_exited();
        assert!(
            !pane.has_child(),
            "the handle must be removed so a second mark_exited starts no second reap"
        );

        // A second call must find nothing and not panic.
        pane.mark_exited();
        assert!(!pane.has_child());
    }

    /// AC-3, NFR1 (TS-10): `mark_exited` returns promptly even when the
    /// pane holds a child whose exit-status/kill/wait calls are
    /// deliberately slow — proving it hands the child off to the reaper
    /// rather than waiting on it itself. A wide margin (well below the
    /// double's multi-second delay) keeps this assertion CI-safe.
    #[cfg(unix)]
    #[test]
    fn mark_exited_returns_promptly_even_with_a_slow_to_reap_child() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(
            1,
            80,
            24,
            target,
            writer,
            pair.master,
            Some(Box::new(SlowExitChild)),
        );

        let started = std::time::Instant::now();
        pane.mark_exited();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "mark_exited must return promptly regardless of the child's own \
             responsiveness — its runtime must be independent of the \
             child's exit behavior (NFR1)"
        );
    }

    // ── Process-id based child (task plan task0007, IMPLEMENTATION.md D6) ──

    /// Poll `/proc/<pid>` until the pid is gone entirely — the outcome once
    /// the background reaper `mark_exited` hands the process id off to has
    /// actually collected it. Mirrors `child_reaper`'s own
    /// `assert_pid_reaped` test helper (kept local here since that one is
    /// private to its own module).
    #[cfg(unix)]
    fn assert_pid_eventually_reaped(pid: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "pid {pid} should have been reaped via the process-id path, \
                     but /proc/{pid} still exists"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// AC-4: a pane holding a process id, when marked exited, is reaped
    /// through the process-id path (confirmed by the OS-level `/proc` check
    /// below, not just the pane's own bookkeeping) and ends in the same
    /// observable state as a pane holding an owned handle — compare
    /// `test_mark_exited_clears_writer_and_master` and
    /// `mark_exited_removes_child_handle_and_second_call_is_a_noop`: `exited`
    /// set, writer/master released, and the child reference cleared so a
    /// second `mark_exited` is a no-op.
    #[cfg(unix)]
    #[test]
    fn mark_exited_on_pane_with_process_id_reaps_via_pid_path_and_matches_observable_state() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();

        let child = std::process::Command::new("true")
            .spawn()
            .expect("failed to spawn test child process");
        let pid = child.id();

        let mut pane = MuxPane::new_with_process_id(1, 80, 24, target, writer, pair.master, pid);
        assert!(pane.has_child());
        assert!(!pane.exited);

        pane.mark_exited();

        assert!(pane.exited);
        assert!(
            !pane.has_child(),
            "the process id reference must be cleared so a second mark_exited is a no-op"
        );
        assert!(
            pane.write_input(b"hello").is_err(),
            "writer must be released, matching the owned-handle path's observable state"
        );

        // A second call must find nothing and not panic (mirrors
        // `mark_exited_removes_child_handle_and_second_call_is_a_noop`).
        pane.mark_exited();
        assert!(!pane.has_child());

        assert_pid_eventually_reaped(pid);
        // Do not call `child.wait()` — the pid was already reaped via the
        // process-id path above.
        drop(child);
    }

    #[test]
    fn test_write_input_to_sink() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        // sink() writer always succeeds
        assert!(pane.write_input(b"hello world").is_ok());
    }

    #[test]
    fn test_channel_backpressure_full() {
        // Channel capacity 1: second send should fail with Full
        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        // First send succeeds
        assert!(tx.try_send(PtyOutputChunk::pty_output(1, vec![1])).is_ok());
        // Second send hits backpressure (channel full)
        let result = tx.try_send(PtyOutputChunk::pty_output(1, vec![2]));
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Full(_)) => {} // expected
            _ => panic!("Expected Full error"),
        }
    }

    #[test]
    fn test_channel_closed_detection() {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(PTY_CHANNEL_CAPACITY);
        drop(rx); // Close receiver
        let result = tx.try_send(PtyOutputChunk::pty_output(1, vec![1]));
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Closed(_)) => {} // expected
            _ => panic!("Expected Closed error"),
        }
    }

    /// Phase 1 ergonomics: `pty_output(...)` tags as `PtyOutput`,
    /// `snapshot(...)` tags as `Snapshot`. Default reader / resume callers
    /// keep `kind == PtyOutput`; only the snapshot handler opts into
    /// `kind == Snapshot`. Verifies the discriminator is honored by the
    /// two named constructors.
    #[test]
    fn test_chunk_kind_constructors_round_trip() {
        let live = PtyOutputChunk::pty_output(1, b"abc".to_vec());
        assert_eq!(live.pane_id, 1);
        assert_eq!(live.data, b"abc");
        assert_eq!(live.kind, ChunkKind::PtyOutput);

        let snap = PtyOutputChunk::snapshot(2, b"snapshot-bytes".to_vec());
        assert_eq!(snap.pane_id, 2);
        assert_eq!(snap.data, b"snapshot-bytes");
        assert_eq!(snap.kind, ChunkKind::Snapshot);
    }

    #[test]
    fn test_bounded_channel_capacity_constant() {
        // Verify the constant is reasonable (not too small, not too large)
        assert!(PTY_CHANNEL_CAPACITY >= 64);
        assert!(PTY_CHANNEL_CAPACITY <= 4096);
    }

    #[cfg(unix)]
    #[test]
    fn test_resize_with_real_pty() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();

        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

        let result = pane.resize(120, 40);
        assert!(result.is_ok());
        assert_eq!(pane.cols, 120);
        assert_eq!(pane.rows, 40);
    }

    // ── resize marker recording (task0001, IMPLEMENTATION.md D1/D2) ──────

    /// `MuxPane::new` records the pane's INITIAL dimensions as the very
    /// first scrollback bytes, so a replay always has a marker to resize
    /// into before the earliest retained segment.
    #[cfg(unix)]
    #[test]
    fn test_new_pane_records_initial_dims_marker_in_scrollback() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
        let (bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert!(bytes.is_empty(), "no content bytes were ever written");
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16)],
            "the initial dims must be recorded structurally, not as bytes"
        );
    }

    /// A resize that actually changes dimensions records a marker with the
    /// NEW dimensions into the pane's scrollback ring.
    #[cfg(unix)]
    #[test]
    fn test_resize_records_marker_in_scrollback_when_dims_change() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

        pane.resize(120, 40).unwrap();

        let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert!(
            segments
                .iter()
                .any(|&(_, cols, rows)| (cols, rows) == (120, 40)),
            "resize must record a segment with the new dimensions: {segments:?}"
        );
    }

    /// A no-op resize (same dimensions as current) must NOT record a
    /// redundant marker — only `MuxPane::new`'s initial marker is present.
    #[cfg(unix)]
    #[test]
    fn test_resize_same_dims_does_not_record_extra_marker() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

        pane.resize(80, 24).unwrap(); // same dims as construction

        let (bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
        assert!(bytes.is_empty());
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16)],
            "a no-op resize must not add a second segment"
        );
    }

    /// review round-1 rework, finding 83bed291fb779f52 (high) / task0002
    /// AC-4: `resize()` must hold the scrollback lock across BOTH the
    /// PTY-visible resize and the marker write, establishing a single
    /// ordering owner against a concurrent scrollback writer (the PTY
    /// reader thread). Proven deterministically: while a competing thread
    /// holds `pane.scrollback`'s lock (standing in for a reader thread's
    /// in-flight append), `resize()` must be unable to complete — if it
    /// could, that would mean it never needed the lock across its whole
    /// body, reopening the exact race the fix closes.
    #[cfg(unix)]
    #[test]
    fn test_resize_holds_scrollback_lock_establishing_ordering_with_reader_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
        let scrollback = pane.scrollback.clone();

        // Hold the scrollback lock from the TEST thread first, standing in
        // for the PTY reader thread's write() call already in flight.
        let guard = scrollback.lock().unwrap();

        let resize_done = Arc::new(AtomicBool::new(false));
        let rd = resize_done.clone();
        let resizer = std::thread::spawn(move || {
            let result = pane.resize(120, 40);
            rd.store(true, Ordering::SeqCst);
            (pane, result)
        });

        // resize() must NOT be able to complete while the lock is held —
        // pre-fix, master.resize() ran outside any lock and the whole call
        // could finish freely here.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !resize_done.load(Ordering::SeqCst),
            "resize() must block on the scrollback lock, establishing that \
             no concurrent write can land ahead of its marker"
        );

        drop(guard);
        let (pane, result) = resizer.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(pane.cols, 120);
        assert_eq!(pane.rows, 40);
    }

    /// Build a `Detached` target with a `NetworkDetach`-only reason and
    /// `owner = None` (system origin), matching the daemon's pre-attach state.
    fn detached_system_target() -> SharedOutputTarget {
        Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }))
    }

    /// TS-12: detached + visible -> stays Detached.
    #[test]
    fn test_evaluate_output_target_network_detached_visible_stays_detached() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target = detached_system_target();
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, true, true, &owned_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
    }

    /// TS-13: identity-scoped Connected -> Detached.
    #[test]
    fn test_evaluate_output_target_identity_scoped_connected_to_detached() {
        let (owner_tx, _rx) = mpsc::channel(16);
        let (other_tx, _other_rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(other_tx)));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, false, &owner_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    #[test]
    fn test_evaluate_output_target_owner_can_detach() {
        let (owner_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owner_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, false, &owner_tx);
        assert!(matches!(result, EvalResult::SwitchedToDetached));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must be set");
                assert!(owner.same_channel(&owner_tx));
            }
            _ => panic!("expected Detached"),
        }
    }

    /// TS-14 (revised): Detached -> Connected returns snapshot bytes that
    /// route through `build_resume_snapshot_bytes` (the visibility-resume
    /// SSOT). For a main-buffer pane (shadow_parser never entered alt-screen)
    /// the helper drops the daemon vt100 `contents_formatted()` slice and
    /// rebuilds the visible viewport from scrollback alone — same
    /// main/alt split contract as the reattach path. Captured
    /// raw_passthrough must NOT appear (replaying it would re-spawn
    /// viewers / re-render inline images) and the buffer must still be
    /// drained + cleared.
    #[test]
    fn test_evaluate_output_target_detached_to_connected_returns_snapshot() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        pane.scrollback.lock().unwrap().write(b"buffered-from-ring");
        pane.shadow_parser.lock().unwrap().process(b"hello-shadow");
        pane.raw_passthrough
            .lock()
            .unwrap()
            .append(b"\x1b_Gi=1;ZZ\x1b\\");
        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        match result {
            EvalResult::ResumeWithSnapshot { chunk } => {
                // D6''''' (AC-9): the chunk must already be tagged
                // Snapshot-kind, not the default PtyOutput — a caller
                // sending it as PtyOutput would render the raw envelope
                // literally instead of decoding it.
                assert_eq!(chunk.kind, ChunkKind::Snapshot);
                let snapshot = decode_snapshot_content(&chunk.data);
                assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
                let s = String::from_utf8_lossy(&snapshot);
                assert!(
                    snapshot
                        .windows(b"buffered-from-ring".len())
                        .any(|w| w == b"buffered-from-ring"),
                    "snapshot must include ring data"
                );
                // Main-buffer pane: the daemon vt100 dump must NOT appear in
                // the snapshot. `build_resume_snapshot_bytes` follows the
                // main/alt split — the client rebuilds the visible viewport
                // from scrollback alone.
                assert!(
                    !snapshot
                        .windows(b"hello-shadow".len())
                        .any(|w| w == b"hello-shadow"),
                    "main-buffer resume snapshot must omit the shadow screen dump"
                );
                assert!(
                    !s.contains("\u{1b}_Gi=1"),
                    "snapshot must NOT include captured passthrough"
                );
            }
            _ => panic!("expected ResumeWithSnapshot"),
        }
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        assert_eq!(pane.raw_passthrough.lock().unwrap().len(), 0);
    }

    /// D6''' (round-6 rework, review round-5 finding `89b58cd82d7aa713`):
    /// mirrors `test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit`
    /// for `evaluate_output_target`'s parallel `ResumeWithSnapshot` branch
    /// — an oversize encoded snapshot must not transition the pane to
    /// Connected at all.
    #[test]
    fn test_evaluate_output_target_stays_detached_when_snapshot_exceeds_frame_limit() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(2, 80, 24, target.clone());
        let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
        *pane.scrollback.lock().unwrap() =
            crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
        pane.scrollback
            .lock()
            .unwrap()
            .write(&vec![b'x'; oversize_capacity]);

        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        assert!(
            matches!(result, EvalResult::Unchanged),
            "oversize snapshot must not resume the pane"
        );
        assert!(
            matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
            "pane must stay Detached rather than swap to Connected with an \
             unsendable snapshot"
        );
    }

    #[test]
    fn test_evaluate_output_target_already_connected_visible_no_op() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        assert!(matches!(result, EvalResult::Unchanged));
    }

    /// F6 regression: connection A puts a pane into HiddenByVisibility
    /// (Detached, owner=A). Connection B then calls SetVisibility(true) with
    /// its own tx — must NOT reclaim the pane.
    #[test]
    fn test_evaluate_output_target_other_connection_cannot_reclaim_hidden() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let (b_tx, _b_rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &b_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must remain A");
                assert!(
                    owner.same_channel(&a_tx),
                    "pane must still be owned by connection A"
                );
            }
            _ => panic!("expected Detached, got Connected"),
        }
    }

    /// F6: same connection's hide -> show round trip restores Connected.
    #[test]
    fn test_evaluate_output_target_same_connection_hide_show_roundtrip() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(a_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let r1 = evaluate_output_target(&pane, false, false, &a_tx);
        assert!(matches!(r1, EvalResult::SwitchedToDetached));

        let r2 = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(r2, EvalResult::ResumeWithSnapshot { .. }));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    /// F6: when both NetworkDetach and HiddenByVisibility are active,
    /// SetVisibility(true) only clears the hidden bit. The pane stays
    /// Detached because the network reason is still active. Only the reattach
    /// path may clear `NetworkDetach`.
    #[test]
    fn test_evaluate_output_target_both_reasons_visible_keeps_detached() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::Both,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, .. } => {
                assert_eq!(
                    *reason,
                    DetachReason::NetworkDetach,
                    "hidden bit cleared but network bit stays"
                );
            }
            _ => panic!("expected Detached"),
        }
    }

    /// F6: system-origin Detached (`owner = None`, reason = NetworkDetach)
    /// is NOT cleared by `evaluate_output_target` — the `NetworkDetach` bit
    /// only resolves through the reattach path. Until then, the pane stays
    /// Detached even when the caller asserts `visible = true`. The owner
    /// slot is adopted so a subsequent visibility transition is matched
    /// against the correct connection.
    #[test]
    fn test_evaluate_output_target_system_origin_stays_detached_until_reattach() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target = detached_system_target();
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::NetworkDetach);
                let owner = owner.as_ref().expect("owner adopted from caller");
                assert!(owner.same_channel(&a_tx));
            }
            _ => panic!("expected Detached"),
        }
    }

    /// F2: `resume_pane_with_permit` must enqueue the snapshot via the
    /// caller-supplied permit and only swap to Connected after the send.
    /// The pane mutex is held for the full sequence, so a reader thread
    /// taking the same mutex cannot push a live chunk between the two
    /// steps. This test asserts the post-conditions.
    #[tokio::test]
    async fn test_resume_pane_with_permit_sends_then_swaps() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(7, 80, 24, target.clone());
        pane.scrollback.lock().unwrap().write(b"ring-data");
        pane.shadow_parser.lock().unwrap().process(b"resume-shadow");
        pane.raw_passthrough
            .lock()
            .unwrap()
            .append(b"\x1b_Gi=7;PASS\x1b\\");

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::Resumed));

        // Target switched to Connected.
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));

        // Snapshot is on the channel.
        let chunk = rx.try_recv().expect("snapshot enqueued under pane lock");
        assert_eq!(chunk.pane_id, 7);
        let content = decode_snapshot_content(&chunk.data);
        assert!(content.starts_with(b"\x1b[H\x1b[2J"));
        // Captured passthrough must NOT be replayed (would re-render the image).
        let needle_passthrough = b"\x1b_Gi=7;PASS\x1b\\";
        assert!(
            !content
                .windows(needle_passthrough.len())
                .any(|w| w == needle_passthrough),
            "snapshot must NOT contain captured passthrough"
        );
        // Plain-text ring history is still restored.
        assert!(
            content
                .windows(b"ring-data".len())
                .any(|w| w == b"ring-data"),
            "snapshot must contain ring data"
        );
        // Main-buffer pane (shadow_parser never entered alt-screen): the
        // daemon vt100 `contents_formatted()` dump must NOT appear in the
        // snapshot. The client rebuilds the visible viewport from scrollback
        // alone — this is the resume-path counterpart of the main/alt split
        // in `build_snapshot_bytes`.
        assert!(
            !content
                .windows(b"resume-shadow".len())
                .any(|w| w == b"resume-shadow"),
            "main-buffer resume snapshot must omit the shadow screen dump"
        );

        // raw_passthrough drained.
        assert!(pane.raw_passthrough.lock().unwrap().is_empty());

        // review round-1 rework, finding 20b2bed0aaf48f94: the resume
        // snapshot must be tagged Snapshot (not the default PtyOutput) so
        // the client routes it through the marker-interpreting
        // `reset_and_replay` path instead of the marker-blind live path.
        assert_eq!(chunk.kind, ChunkKind::Snapshot);
    }

    /// Companion to `test_resume_pane_with_permit_sends_then_swaps`: when
    /// the shadow parser is in alt-screen mode the resume snapshot DOES
    /// include the daemon vt100 dump (so the TUI surface is restored).
    /// Mirror of the alt branch in `build_snapshot_bytes` applied to the
    /// visibility-resume code path.
    #[tokio::test]
    async fn test_resume_pane_with_permit_includes_screen_for_alt_screen() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(11, 80, 24, target.clone());
        // Flip the shadow parser into alt-screen mode BEFORE feeding the
        // screen content so the resume builder follows the alt branch.
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
        pane.shadow_parser
            .lock()
            .unwrap()
            .process(b"ALT-RESUME-SHADOW");

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::Resumed));

        let chunk = rx.try_recv().expect("snapshot enqueued");
        assert_eq!(chunk.pane_id, 11);
        let content = decode_snapshot_content(&chunk.data);
        assert!(content.starts_with(b"\x1b[H\x1b[2J"));
        assert!(
            content
                .windows(b"ALT-RESUME-SHADOW".len())
                .any(|w| w == b"ALT-RESUME-SHADOW"),
            "alt-screen resume snapshot must include the shadow screen dump"
        );
    }

    /// F2: full Both reason cannot be cleared by `resume_pane_with_permit`
    /// alone — NetworkDetach stays. The permit is dropped without sending.
    #[tokio::test]
    async fn test_resume_pane_with_permit_keeps_detached_when_network_bit_set() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::Both,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(8, 80, 24, target.clone());

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::NoChange));

        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, .. } => {
                assert_eq!(*reason, DetachReason::NetworkDetach);
            }
            _ => panic!("expected Detached"),
        }
        assert!(rx.try_recv().is_err(), "no snapshot must be sent");
    }

    /// F2: connected pane is a no-op (already resumed).
    #[tokio::test]
    async fn test_resume_pane_with_permit_no_change_when_already_connected() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let pane = MuxPane::new_test(9, 80, 24, target.clone());

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::NoChange));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        assert!(rx.try_recv().is_err());
    }

    /// F2: owner mismatch (different connection's tx) must be NoChange.
    #[tokio::test]
    async fn test_resume_pane_with_permit_owner_mismatch_keeps_detached() {
        let (a_tx, _a_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let (b_tx, mut b_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(10, 80, 24, target.clone());

        let permit = b_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &b_tx, AnyPermit::Borrowed(permit));
        assert!(matches!(outcome, ResumeOutcome::NoChange));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
        assert!(b_rx.try_recv().is_err(), "no snapshot must reach B");
    }

    /// D6''' (round-6 rework, review round-5 finding `89b58cd82d7aa713`):
    /// an encoded snapshot too large for a single codec frame must NOT be
    /// enqueued — the pane stays Detached (fail recoverably) rather than
    /// being handed a frame `mux::ipc::connection`'s codec would reject
    /// (which previously tore the whole connection down).
    ///
    /// Confirmed to fail pre-fix: the oversize check only LOGGED and still
    /// unconditionally sent + swapped to Connected — this test's
    /// `ResumeOutcome::NoChange` / `PaneOutputTarget::Detached` /
    /// "nothing enqueued" assertions would all have failed.
    #[tokio::test]
    async fn test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(12, 80, 24, target.clone());
        // Replace the default (2 MiB) ring with one large enough to hold
        // content that, once encoded, exceeds `MAX_SNAPSHOT_FRAME_PAYLOAD`
        // (~16 MiB) — the default ring's own cap makes this unreachable
        // otherwise (real panes never approach the codec's frame limit).
        let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
        *pane.scrollback.lock().unwrap() =
            crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
        pane.scrollback
            .lock()
            .unwrap()
            .write(&vec![b'x'; oversize_capacity]);

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(
            matches!(outcome, ResumeOutcome::NoChange),
            "oversize snapshot must not resume the pane"
        );
        assert!(
            matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
            "pane must stay Detached rather than swap to Connected with an \
             unsendable snapshot"
        );
        assert!(
            rx.try_recv().is_err(),
            "no snapshot may reach the channel when it would exceed the \
             single-frame limit"
        );
    }

    /// D3'''' (round-7 rework, review round-6 finding `46c29c2c65970d26`):
    /// settles the reachability question the round-6 reviewers disagreed
    /// on — does an oversize resume failure freeze the pane permanently
    /// (`Detached { HiddenByVisibility }` forever), or does a later
    /// visibility cycle re-drive a successful resume once the oversize
    /// condition clears?
    ///
    /// `resume_pane_with_permit`'s oversize branch returns `NoChange`
    /// WITHOUT touching `*target` or `*reason` at all (see its body: the
    /// early return happens before any assignment) — the pane is left in
    /// EXACTLY the state it was in before the attempt. `handle_set_visibility`
    /// is the only production caller, and it re-invokes
    /// `resume_pane_with_permit` for every non-exited pane on EVERY
    /// `visible -> true` edge it does not short-circuit as a no-op (its
    /// `prev == visible` guard only suppresses a REPEATED `true` with no
    /// intervening `false`) — so a hide -> show cycle (a connection
    /// toggling visibility false then true again, e.g. the client
    /// minimizing and restoring the window) unconditionally retries this
    /// exact call. This test proves the retry actually recovers: the first
    /// call (oversize) leaves the pane detached, and a second call — after
    /// the condition that caused the oversize snapshot clears (mirroring
    /// what a later resize or scrollback eviction does in production) —
    /// resumes cleanly. The pane is therefore never left detached with
    /// visibility latched on FOREVER: recovery is reachable via the next
    /// visibility toggle, without any state-machine change being required.
    #[tokio::test]
    async fn resume_pane_with_permit_recovers_after_oversize_condition_clears() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(13, 80, 24, target.clone());
        let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
        *pane.scrollback.lock().unwrap() =
            crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
        pane.scrollback
            .lock()
            .unwrap()
            .write(&vec![b'x'; oversize_capacity]);

        // First attempt: oversize, must stay detached (same assertion as
        // `test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit`).
        let permit = owned_tx.reserve().await.expect("reserve permit");
        let first_outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(
            matches!(first_outcome, ResumeOutcome::NoChange),
            "first (oversize) attempt must not resume the pane"
        );
        assert!(
            matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
            "pane must stay Detached after the oversize attempt"
        );
        assert!(
            rx.try_recv().is_err(),
            "no snapshot may reach the channel on the oversize attempt"
        );

        // The condition that caused the oversize snapshot clears (e.g. a
        // later resize / scrollback eviction shrinks it back under the
        // frame limit) — the pane's own `output_target` was NEVER touched
        // by the failed attempt above, so it is still exactly
        // `Detached { HiddenByVisibility, owner }`.
        *pane.scrollback.lock().unwrap() =
            crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(4096);
        pane.scrollback.lock().unwrap().write(b"small content now");

        // Second attempt (what a hide -> show cycle re-drives): must
        // resume cleanly.
        let permit = owned_tx.reserve().await.expect("reserve permit");
        let second_outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
        assert!(
            matches!(second_outcome, ResumeOutcome::Resumed),
            "the retry must resume the pane once the oversize condition \
             has cleared — the pane must never stay detached forever"
        );
        assert!(
            matches!(*target.lock().unwrap(), PaneOutputTarget::Connected(_)),
            "pane must be Connected after the retry succeeds"
        );
        assert!(
            rx.try_recv().is_ok(),
            "the retry must enqueue a snapshot chunk"
        );
    }

    #[test]
    fn test_detach_reason_combine() {
        assert_eq!(
            DetachReason::combine(
                DetachReason::NetworkDetach,
                DetachReason::HiddenByVisibility
            ),
            DetachReason::Both
        );
        assert_eq!(
            DetachReason::combine(DetachReason::NetworkDetach, DetachReason::NetworkDetach),
            DetachReason::NetworkDetach
        );
        assert_eq!(
            DetachReason::combine(DetachReason::Both, DetachReason::HiddenByVisibility),
            DetachReason::Both
        );
    }

    #[test]
    fn test_detach_reason_clear_bits() {
        assert_eq!(DetachReason::NetworkDetach.clear_network(), None);
        assert_eq!(
            DetachReason::Both.clear_network(),
            Some(DetachReason::HiddenByVisibility)
        );
        assert_eq!(
            DetachReason::HiddenByVisibility.clear_network(),
            Some(DetachReason::HiddenByVisibility)
        );
        assert_eq!(DetachReason::HiddenByVisibility.clear_hidden(), None);
        assert_eq!(
            DetachReason::Both.clear_hidden(),
            Some(DetachReason::NetworkDetach)
        );
    }

    /// Regression for the vt100 0.15 panic that poisoned the shadow parser:
    /// a saved cursor (DECSC) outside the grid after a shrink resize was
    /// restored (DECRC) unclamped, and the next wide-character write hit an
    /// out-of-bounds `drawing_cell(pos).unwrap()`. vt100 0.16 clamps
    /// `saved_pos` in `set_size`, so this sequence must not panic.
    #[test]
    fn test_shadow_parser_survives_decrc_after_shrink_resize() {
        let mut parser = new_shadow_parser(24, 80);
        // Park the cursor near the bottom-right corner and save it (DECSC),
        // with wide characters at the edge.
        parser.process("\x1b[24;75Hあああ\x1b7".as_bytes());
        // Shrink the grid, restore the saved cursor (DECRC), then write
        // wide characters again.
        parser.screen_mut().set_size(10, 20);
        parser.process("\x1b8ああああああ".as_bytes());
        let (rows, cols) = parser.screen().size();
        assert_eq!((rows, cols), (10, 20));
    }

    /// OSC 0 / OSC 2 titles must surface through the TitleSink callback
    /// (vt100 0.16 removed `Screen::title()`).
    #[test]
    fn test_title_sink_reports_osc_titles() {
        let mut parser = new_shadow_parser(24, 80);
        assert_eq!(parser.callbacks_mut().take_title(), None);
        parser.process(b"\x1b]0;from-osc-0\x07");
        assert_eq!(
            parser.callbacks_mut().take_title().as_deref(),
            Some("from-osc-0")
        );
        // Drained after take.
        assert_eq!(parser.callbacks_mut().take_title(), None);
        parser.process(b"\x1b]2;from-osc-2\x07");
        assert_eq!(
            parser.callbacks_mut().take_title().as_deref(),
            Some("from-osc-2")
        );
    }

    /// Poison the shadow parser mutex by panicking while holding the lock.
    fn poison_shadow_parser(pane: &MuxPane) {
        let parser = pane.shadow_parser.clone();
        let _ = std::thread::spawn(move || {
            let _guard = parser.lock().unwrap();
            panic!("intentional poison");
        })
        .join();
        assert!(pane.shadow_parser.lock().is_err(), "mutex must be poisoned");
    }

    /// A poisoned shadow parser mutex must not panic the caller; the guard
    /// is recovered and the parser stays usable.
    #[test]
    fn test_lock_shadow_parser_recovers_from_poison() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx)));
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.shadow_parser.lock().unwrap().process(b"before-poison");
        poison_shadow_parser(&pane);

        let parser = lock_shadow_parser(&pane.shadow_parser);
        let contents = parser.screen().contents();
        assert!(contents.contains("before-poison"));
    }

    /// Reattach (Detached -> Connected) must still produce a snapshot after
    /// the shadow parser mutex was poisoned by a reader-thread panic.
    ///
    /// The main-buffer pane drops the shadow slice (same main/alt split as
    /// `build_resume_snapshot_bytes`), so we feed scrollback bytes instead
    /// and assert those survive the poisoned lock.
    #[test]
    fn test_evaluate_output_target_survives_poisoned_shadow_parser() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        pane.scrollback.lock().unwrap().write(b"ring-bytes-x");
        pane.shadow_parser.lock().unwrap().process(b"shadow-data");
        poison_shadow_parser(&pane);

        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        match result {
            EvalResult::ResumeWithSnapshot { chunk } => {
                assert_eq!(chunk.kind, ChunkKind::Snapshot);
                assert!(
                    chunk
                        .data
                        .windows(b"ring-bytes-x".len())
                        .any(|w| w == b"ring-bytes-x"),
                    "snapshot must include scrollback even after poisoned shadow lock"
                );
            }
            _ => panic!("expected ResumeWithSnapshot"),
        }
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    // ── task0003: snapshot accessors + restore constructors ───────────────

    /// A child double reporting a fixed, non-`None` process id — used to
    /// exercise [`MuxPane::child_pid`] (the `PaneChild::Owned` arm) without
    /// a real spawned process.
    #[derive(Debug)]
    struct FixedPidChild(u32);

    impl portable_pty::ChildKiller for FixedPidChild {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            unimplemented!("not exercised by these tests")
        }
    }

    impl portable_pty::Child for FixedPidChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(None)
        }
        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            Ok(portable_pty::ExitStatus::with_exit_code(0))
        }
        fn process_id(&self) -> Option<u32> {
            Some(self.0)
        }
        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    fn open_test_pty_pair() -> portable_pty::PtyPair {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        pty_system.openpty(size).unwrap()
    }

    /// AC-2 (snapshot groundwork): `master_raw_fd` reports the SAME fd
    /// number the underlying PTY master actually has.
    #[cfg(unix)]
    #[test]
    fn master_raw_fd_reports_the_ptys_actual_descriptor_number() {
        let pair = open_test_pty_pair();
        let expected_fd = pair.master.as_raw_fd().expect("PTY master must have an fd");
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
        assert_eq!(pane.master_raw_fd(), Some(expected_fd));
    }

    /// AC-2 (snapshot groundwork): `master_raw_fd` / `child_pid` both
    /// become `None` once the pane has exited (master dropped, child
    /// reaped) — an exited pane contributes no descriptor to snapshot.
    #[cfg(unix)]
    #[test]
    fn master_raw_fd_and_child_pid_are_none_after_mark_exited() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut pane = MuxPane::new(
            1,
            80,
            24,
            target,
            writer,
            pair.master,
            Some(Box::new(FixedPidChild(4242))),
        );
        assert_eq!(pane.child_pid(), Some(4242));
        pane.mark_exited();
        assert_eq!(pane.master_raw_fd(), None);
        assert_eq!(pane.child_pid(), None);
    }

    /// AC-2 (snapshot groundwork): `child_pid` reports the owned child
    /// double's process id verbatim (the `PaneChild::Owned` arm).
    #[cfg(unix)]
    #[test]
    fn child_pid_reports_the_owned_childs_process_id() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let pane = MuxPane::new(
            1,
            80,
            24,
            target,
            writer,
            pair.master,
            Some(Box::new(FixedPidChild(777))),
        );
        assert_eq!(pane.child_pid(), Some(777));
    }

    /// AC-2 (snapshot groundwork): `child_pid` reports a restored pane's
    /// bare process id verbatim (the `PaneChild::ProcessId` arm — task0007
    /// IMPLEMENTATION.md D6).
    #[cfg(unix)]
    #[test]
    fn child_pid_reports_a_restored_panes_process_id() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let pane = MuxPane::new_with_process_id(1, 80, 24, target, writer, pair.master, 5150);
        assert_eq!(pane.child_pid(), Some(5150));
    }

    /// AC-1/AC-4: `from_restored` sets cols/rows/cwd/title/agent-status and
    /// scrollback verbatim, and carries the given restored pid through the
    /// `PaneChild::ProcessId` path (task0007's reaping wiring).
    #[cfg(unix)]
    #[test]
    fn from_restored_sets_attributes_and_scrollback_verbatim() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
        scrollback.write(b"restored scrollback bytes");
        let mut agent_status = AgentStatus::default();
        agent_status.state = Some(AgentState::Working);
        agent_status.name = Some("claude".to_string());
        agent_status.revision = 3;

        let pane = MuxPane::from_restored(
            9,
            80,
            24,
            target,
            writer,
            pair.master,
            scrollback,
            Some("/home/user/project".to_string()),
            Some("zsh".to_string()),
            agent_status,
            Some(4242),
            false,
            Vec::new(),
        );

        assert_eq!(pane.id, 9);
        assert_eq!((pane.cols, pane.rows), (80, 24));
        assert!(!pane.exited);
        assert_eq!(
            pane.child_pid(),
            Some(4242),
            "restored pid flows through PaneChild::ProcessId"
        );
        assert_eq!(
            *pane.cwd.lock().unwrap(),
            Some("/home/user/project".to_string())
        );
        assert_eq!(*pane.title.lock().unwrap(), Some("zsh".to_string()));
        {
            let status = pane.agent_status.lock().unwrap();
            assert_eq!(status.state, Some(AgentState::Working));
            assert_eq!(status.name.as_deref(), Some("claude"));
            assert_eq!(status.revision, 3);
        }
        assert_eq!(
            pane.scrollback.lock().unwrap().read_all(),
            b"restored scrollback bytes"
        );
        // AC-6: flag false must behave byte-identically to today — no
        // extra alt-screen-enter sequence is fed, so the parser must never
        // report the alternate screen as active.
        assert!(
            !pane.shadow_parser.lock().unwrap().screen().alternate_screen(),
            "AC-6: from_restored with alt_screen=false must not activate the alternate screen"
        );
    }

    /// AC-5: `from_restored` with `alt_screen=true` feeds the
    /// alternate-screen-enter sequence plus the dump into the shadow parser
    /// AFTER the scrollback replay, so the parser reports the alternate
    /// screen active with the dump's content visible, while the replayed
    /// scrollback survives underneath on the main buffer (revealed by
    /// leaving the alt screen).
    #[cfg(unix)]
    #[test]
    fn from_restored_with_alt_screen_true_replays_dump_with_scrollback_beneath() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();
        let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
        scrollback.write(b"pre-alt scrollback line");

        let pane = MuxPane::from_restored(
            3,
            80,
            24,
            target,
            writer,
            pair.master,
            scrollback,
            None,
            None,
            AgentStatus::default(),
            None,
            true,
            b"ALT-DUMP-CONTENT".to_vec(),
        );

        {
            let parser = pane.shadow_parser.lock().unwrap();
            assert!(
                parser.screen().alternate_screen(),
                "AC-5: restore with alt_screen=true must report the alternate screen active"
            );
            let content = parser.screen().contents_formatted();
            assert!(
                content
                    .windows(b"ALT-DUMP-CONTENT".len())
                    .any(|w| w == b"ALT-DUMP-CONTENT"),
                "AC-5: the dump's content must be visible on the alt screen"
            );
        }

        // Leaving the alt screen (as a live reattach eventually would, or a
        // program exiting its TUI) must reveal the scrollback-replayed main
        // buffer beneath it, proving the two replays targeted separate
        // buffers exactly like a live pane's real ESC[?1049h/l pair would.
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049l");
        let main_screen = pane.shadow_parser.lock().unwrap();
        assert!(!main_screen.screen().alternate_screen());
        let main_content = main_screen.screen().contents_formatted();
        assert!(
            main_content
                .windows(b"pre-alt scrollback line".len())
                .any(|w| w == b"pre-alt scrollback line"),
            "AC-5: the replayed scrollback must still be present on the main screen \
             beneath the alt overlay"
        );
    }

    /// AC-6 (continued): `alt_screen=true` with an EMPTY dump (the D1
    /// overflow shape) must still activate the alternate screen — just with
    /// blank contents, since only the dump content degrades, never the
    /// mode flag.
    #[cfg(unix)]
    #[test]
    fn from_restored_with_alt_screen_true_and_empty_dump_yields_blank_active_alt_screen() {
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let target = make_output_target();

        let pane = MuxPane::from_restored(
            4,
            80,
            24,
            target,
            writer,
            pair.master,
            ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY),
            None,
            None,
            AgentStatus::default(),
            None,
            true,
            Vec::new(),
        );

        let parser = pane.shadow_parser.lock().unwrap();
        assert!(
            parser.screen().alternate_screen(),
            "AC-6: alt_screen=true with an empty dump must still activate the alternate screen"
        );
        assert!(
            parser.screen().contents().trim().is_empty(),
            "AC-6: an empty dump must yield a blank alternate screen"
        );
    }

    /// AC-3: `capture_alt_state` on a main-buffer pane (shadow parser never
    /// entered the alternate screen) records flag false and an empty dump.
    #[test]
    fn capture_alt_state_on_main_buffer_pane_returns_false_and_empty_dump() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        pane.shadow_parser
            .lock()
            .unwrap()
            .process(b"plain main-buffer content");

        let (alt, dump) = pane.capture_alt_state();

        assert!(!alt, "AC-3: a main-buffer pane must record flag false");
        assert!(
            dump.is_empty(),
            "AC-3: a main-buffer pane must record an empty dump"
        );
    }

    /// AC-3: `capture_alt_state` on an alt-screen pane records flag true and
    /// a dump equal to the parser's formatted alt-screen contents.
    #[test]
    fn capture_alt_state_on_alt_screen_pane_returns_true_and_the_formatted_alt_contents() {
        let target = make_output_target();
        let pane = MuxPane::new_test(2, 80, 24, target);
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
        pane.shadow_parser
            .lock()
            .unwrap()
            .process(b"ALT-SCREEN-CONTENT");

        let (alt, dump) = pane.capture_alt_state();

        assert!(alt, "AC-3: an alt-screen pane must record flag true");
        assert!(
            dump.windows(b"ALT-SCREEN-CONTENT".len())
                .any(|w| w == b"ALT-SCREEN-CONTENT"),
            "AC-3: the dump must contain the alt-screen content"
        );
        let expected = pane
            .shadow_parser
            .lock()
            .unwrap()
            .screen()
            .contents_formatted();
        assert_eq!(
            dump, expected,
            "AC-3: the dump must equal the parser's formatted alt-screen contents"
        );
    }

    /// AC-7: a dump AT the D1 cap (`MAX_SNAPSHOT_FRAME_PAYLOAD`) is stored
    /// untouched. Exercises the real boundary (task plan Test Notes AC-7):
    /// `cap_alt_screen_dump` takes a plain byte vector, so testing at the
    /// real cap needs no vt100 screen at an unreasonable size — just an
    /// allocation.
    #[test]
    fn cap_alt_screen_dump_returns_the_dump_untouched_at_the_cap() {
        let dump = vec![0xABu8; mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD];
        let result = cap_alt_screen_dump(1, dump.clone());
        assert_eq!(result, dump, "AC-7: a dump at the cap must be stored untouched");
    }

    /// AC-7 (continued): a dump exceeding the D1 cap by a single byte is
    /// replaced with an empty one (flag preservation is the caller's
    /// concern — `capture_alt_state` keeps `alt_screen=true` regardless of
    /// this function's outcome). The "warn-level log line naming the pane
    /// id and the oversize length" half of AC-7 is verified by inspection
    /// of the `log::warn!` call in `cap_alt_screen_dump` itself (matching
    /// this project's established convention for asserting on log output —
    /// see `mux::upgrade::tests::restore_handles_exited_and_unadoptable_panes_while_the_rest_of_the_tree_still_restores`
    /// for the equivalent precedent).
    #[test]
    fn cap_alt_screen_dump_returns_empty_when_the_dump_exceeds_the_cap() {
        let dump = vec![0xABu8; mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1];
        let result = cap_alt_screen_dump(42, dump);
        assert!(
            result.is_empty(),
            "AC-7: a dump exceeding the D1 cap must be replaced with an empty one"
        );
    }

    /// AC-5: a restored live pane can be written to and read from through
    /// its adopted master, demonstrated against a real PTY pair. The PTY's
    /// line discipline echoes input written to the master back to the
    /// master's own reader side, so a reader cloned from the master BEFORE
    /// it is handed to `from_restored` observes the written bytes.
    #[cfg(unix)]
    #[test]
    fn from_restored_pane_can_write_and_read_through_its_adopted_master() {
        use std::io::Read as _;
        let pair = open_test_pty_pair();
        let writer = pair.master.take_writer().unwrap();
        let mut master_reader = pair
            .master
            .try_clone_reader()
            .expect("master must support a reader clone");
        let target = make_output_target();
        let pane = MuxPane::from_restored(
            1,
            80,
            24,
            target,
            writer,
            pair.master,
            ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY),
            None,
            None,
            AgentStatus::default(),
            None,
            false,
            Vec::new(),
        );

        pane.write_input(b"restored-write\n").unwrap();

        let mut buf = [0u8; 64];
        let n = master_reader
            .read(&mut buf)
            .expect("master read must succeed");
        assert!(
            buf[..n]
                .windows(b"restored-write".len())
                .any(|w| w == b"restored-write"),
            "bytes written through the adopted master's writer must be readable \
             back through the adopted master (echoed by the PTY line discipline)"
        );
    }

    /// AC-6: `from_restored_exited` builds an already-exited pane that
    /// adopts no descriptor, while still restoring its non-descriptor
    /// attributes (cwd/title/agent-status/scrollback) verbatim.
    #[test]
    fn from_restored_exited_adopts_no_descriptor_and_is_marked_exited() {
        let target = make_output_target();
        let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
        scrollback.write(b"pre-exit scrollback");
        let pane = MuxPane::from_restored_exited(
            5,
            80,
            24,
            target,
            scrollback,
            Some("/tmp".to_string()),
            Some("bash".to_string()),
            AgentStatus::default(),
        );

        assert!(pane.exited);
        assert_eq!(pane.child_pid(), None);
        #[cfg(unix)]
        assert_eq!(pane.master_raw_fd(), None);
        assert_eq!(*pane.cwd.lock().unwrap(), Some("/tmp".to_string()));
        assert_eq!(*pane.title.lock().unwrap(), Some("bash".to_string()));
        assert_eq!(
            pane.scrollback.lock().unwrap().read_all(),
            b"pre-exit scrollback"
        );
        // Writing to an exited pane must fail (no writer).
        assert!(pane.write_input(b"x").is_err());
    }

    // ── enqueue_pane_output_chunk (mux-window-switch-output-hang task0001,
    // reworked task0002) ──
    //
    // AC-1/AC-2/AC-3: the fix's core mechanism. `enqueue_pane_output_chunk`
    // is deliberately a plain `fn` (not `async fn`), so it structurally
    // cannot suspend the calling task on channel capacity — the tests below
    // pin the OBSERVABLE behavior on top of that structural guarantee: the
    // fast path delivers synchronously, the slow path still returns without
    // blocking and defers into the connection-owned `DeferredOutputQueue`
    // rather than sending, a closed channel is handled without a panic (no
    // new unhandled error path), and the deferred queue itself is bounded
    // (task0002 AC-3/AC-4).

    /// AC-3 (fast path): with room in the channel, the chunk is enqueued
    /// synchronously — no deferral.
    #[test]
    fn enqueue_pane_output_chunk_fast_path_delivers_synchronously() {
        let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let mut deferred = DeferredOutputQueue::new();
        enqueue_pane_output_chunk(
            &tx,
            PtyOutputChunk::pty_output(1, b"hi".to_vec()),
            &mut deferred,
        );
        let chunk = rx.try_recv().expect("fast path must deliver synchronously");
        assert_eq!(chunk.data, b"hi");
        assert!(deferred.is_empty(), "fast path must not defer anything");
    }

    /// AC-1/AC-3 (task0002 rework): with the channel completely full,
    /// `enqueue_pane_output_chunk` must still return immediately (this IS
    /// the self-deadlock fix) by pushing the chunk onto `deferred` instead
    /// of sending it.
    ///
    /// task0003 rework (AC-5, review round 2 finding `6574d4221dcb5efe`):
    /// this test used to also hand-roll a `while let Some(item) = pop_front()
    /// { ... tx.try_send(...) ... }` loop here to "prove" FIFO delivery —
    /// that copy could diverge from (and did diverge from — it never
    /// exercised the `Full`-requeue or `Closed`-clear arms) the production
    /// `handlers::flush_deferred_output`. This module cannot call that
    /// `pub(super)` function (it lives in `mux::ipc`, a different module
    /// tree), so the flush-side proof now lives in
    /// `mux::ipc::handlers::tests` instead, calling the production function
    /// directly — see `handle_request_pane_snapshot_returns_promptly_when_own_pane_channel_full`
    /// and the dedicated `flush_deferred_output_*` tests there. This test is
    /// trimmed to what THIS module owns: that the enqueue itself defers
    /// without blocking.
    #[tokio::test]
    async fn enqueue_pane_output_chunk_full_channel_defers_without_blocking() {
        let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);
        tx.send(PtyOutputChunk::pty_output(1, b"a".to_vec()))
            .await
            .unwrap();
        tx.send(PtyOutputChunk::pty_output(1, b"b".to_vec()))
            .await
            .unwrap();
        assert!(
            tx.try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
                .is_err(),
            "test prerequisite: channel must be at capacity"
        );

        let mut deferred = DeferredOutputQueue::new();
        // This call must return immediately even though the channel is full
        // — it is a plain (non-async) function call, so there is no `.await`
        // point where it could suspend the test task either.
        enqueue_pane_output_chunk(
            &tx,
            PtyOutputChunk::snapshot(1, b"SNAP".to_vec()),
            &mut deferred,
        );
        assert_eq!(
            deferred.len(),
            1,
            "full channel must defer, not send, the chunk"
        );
        match deferred.pop_front() {
            Some(DeferredOutputItem::Chunk(chunk)) => {
                assert_eq!(chunk.pane_id, 1);
                assert_eq!(chunk.kind, ChunkKind::Snapshot);
                assert_eq!(chunk.data, b"SNAP");
            }
            other => panic!("expected a deferred Chunk, got {other:?}"),
        }

        // The two pre-existing chunks are still exactly as sent — enqueueing
        // never touched the channel's own contents.
        let c1 = rx.recv().await.expect("chunk a");
        assert_eq!(c1.data, b"a");
        let c2 = rx.recv().await.expect("chunk b");
        assert_eq!(c2.data, b"b");
    }

    /// A closed channel (client gone) is handled the same way the
    /// pre-existing blocking-send call sites handled it: logged and
    /// dropped, never a panic, and nothing is deferred (retrying a send that
    /// can only ever fail the same way would be pointless).
    #[test]
    fn enqueue_pane_output_chunk_closed_channel_does_not_panic() {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(1);
        drop(rx);
        let mut deferred = DeferredOutputQueue::new();
        enqueue_pane_output_chunk(
            &tx,
            PtyOutputChunk::pty_output(1, b"x".to_vec()),
            &mut deferred,
        );
        // Reaching here without a panic is the assertion.
        assert!(deferred.is_empty(), "a closed channel is not retried");
    }

    /// AC-4: `enqueue_pane_output_chunk` no longer spawns a task on its Full
    /// branch (task0002 rework), so it no longer depends on an active tokio
    /// runtime at all. This is a plain `#[test]` (deliberately NOT
    /// `#[tokio::test]` — there is no tokio runtime running here) hitting
    /// the Full branch directly: it must not panic, and the chunk must land
    /// in `deferred` exactly as it would inside a runtime.
    #[test]
    fn enqueue_pane_output_chunk_full_branch_does_not_panic_outside_tokio_runtime() {
        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        tx.try_send(PtyOutputChunk::pty_output(1, b"filler".to_vec()))
            .expect("fill the single slot");
        assert!(
            tx.try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
                .is_err()
        );

        let mut deferred = DeferredOutputQueue::new();
        enqueue_pane_output_chunk(
            &tx,
            PtyOutputChunk::pty_output(1, b"x".to_vec()),
            &mut deferred,
        );
        // Reaching here without a panic (no tokio runtime, no `Handle`
        // available) is the assertion.
        assert_eq!(deferred.len(), 1);
    }

    /// AC-4: this fix must not replace the bounded channel with an
    /// unconditionally-growing one. Pins the capacity constant so a future
    /// change to an unbounded mechanism is caught here.
    #[test]
    fn pty_channel_capacity_is_finite_and_unchanged() {
        assert_eq!(PTY_CHANNEL_CAPACITY, 256);
    }

    /// AC-2 (task0003 rework, review round 2 findings `4999311c8becf7eb`/
    /// `ac1d20218d320b08`): a repeated chunk for the SAME pane coalesces —
    /// the newer payload replaces the older one in place rather than
    /// growing the queue, and the survivor is the newest content (never the
    /// newest dropped in favour of the older one).
    #[test]
    fn deferred_output_queue_coalesces_repeated_chunk_for_same_pane_newest_wins() {
        let mut deferred = DeferredOutputQueue::new();
        deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V1".to_vec()));
        deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V2".to_vec()));
        assert_eq!(
            deferred.len(),
            1,
            "a second chunk for the same pane must coalesce, not add a second entry"
        );
        match deferred.pop_front() {
            Some(DeferredOutputItem::Chunk(chunk)) => {
                assert_eq!(
                    chunk.data, b"V2",
                    "the newest payload for the pane must survive"
                );
            }
            other => panic!("expected a Chunk, got {other:?}"),
        }
    }

    /// AC-5 (mux-window-switch-output-hang task0004 rework, review round 3
    /// finding `0830abe1c16ad0fb`): coalescing a repeated chunk for the SAME
    /// pane must preserve its QUEUE POSITION, not move it to the tail. With
    /// `[Chunk(pane 1), VisibilityResume(pane 1)]` queued, a second
    /// `RequestPaneSnapshot` for pane 1 must coalesce the `Chunk` IN PLACE
    /// (still first), NOT reorder into `[VisibilityResume, Chunk]` — the
    /// pre-fix `remove` + `push_back` behavior, which would let a stale,
    /// already-built `Chunk` overtake and overwrite a `VisibilityResume`'s
    /// fresher flush-time-built snapshot on the wire.
    #[test]
    fn deferred_output_queue_coalesce_preserves_position_ahead_of_a_later_visibility_resume() {
        let mut deferred = DeferredOutputQueue::new();
        deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"first".to_vec()));
        deferred.defer_visibility_resume(1);
        assert_eq!(deferred.len(), 2);

        // Second RequestPaneSnapshot for the SAME pane while both entries
        // are still queued: must coalesce the Chunk IN PLACE, not move it
        // to the tail.
        deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"second".to_vec()));
        assert_eq!(
            deferred.len(),
            2,
            "coalescing must not grow the queue past its pre-coalesce length"
        );

        match deferred.pop_front() {
            Some(DeferredOutputItem::Chunk(chunk)) => {
                assert_eq!(chunk.data, b"second", "the newest payload must survive");
            }
            other => panic!(
                "expected the coalesced Chunk to remain FIRST (position preserved), got {other:?}"
            ),
        }
        match deferred.pop_front() {
            Some(DeferredOutputItem::VisibilityResume(pane_id)) => assert_eq!(pane_id, 1),
            other => panic!("expected the VisibilityResume to remain SECOND, got {other:?}"),
        }
        assert!(deferred.pop_front().is_none());
    }

    /// AC-2 (mux-window-switch-output-hang task0006 rework, review round 5
    /// high findings `4043ee676f69ca15` / `1c8d86389ab4bf40`): the REVERSE
    /// order from the pinned test above. `[VisibilityResume(1)]` queued
    /// FIRST (the pane was resumed from hidden while the channel was full),
    /// THEN a `RequestPaneSnapshot` for the SAME pane arrives and defers a
    /// `Chunk` — since no `Chunk` entry exists yet for pane 1, this must
    /// INSERT the new Chunk immediately BEFORE the queued Resume, producing
    /// `[Chunk(1), VisibilityResume(1)]`, rather than dropping it
    /// (task0005's now-reverted fix) or appending it after the Resume. This
    /// ordering still yields newest-wins when the Resume's flush actually
    /// produces a fresher snapshot (the Resume's flush-time-built content
    /// lands LAST), while guaranteeing the `RequestPaneSnapshot` still gets
    /// answered when the Resume no-ops at flush time (pane already
    /// `Connected`, owner mismatch, a surviving `NetworkDetach` bit, or an
    /// oversize snapshot — see `defer_chunk`'s own doc) — the NORMAL case,
    /// since `handle_set_visibility` queues a Resume for every non-exited
    /// pane on a visible edge without checking whether it is actually
    /// detached-hidden. Delivery itself (the client still receiving a
    /// snapshot when the Resume no-ops) is exercised end-to-end by
    /// `mux::ipc::handlers::tests::flush_deferred_output_delivers_chunk_even_when_its_queued_visibility_resume_no_ops`
    /// — this queue-level test only pins the ORDERING (AC-2), since
    /// `DeferredOutputQueue` alone has no flush machinery or client channel
    /// to observe delivery through.
    #[test]
    fn deferred_output_queue_inserts_chunk_immediately_before_queued_visibility_resume_for_same_pane()
     {
        let mut deferred = DeferredOutputQueue::new();
        deferred.defer_visibility_resume(1);
        assert_eq!(deferred.len(), 1);

        deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"pending".to_vec()));
        assert_eq!(
            deferred.len(),
            2,
            "the Chunk must be INSERTED alongside the already-queued VisibilityResume, \
             not dropped — a dropped Chunk here means the client's RequestPaneSnapshot \
             gets no reply at all whenever the Resume later no-ops"
        );

        match deferred.pop_front() {
            Some(DeferredOutputItem::Chunk(chunk)) => {
                assert_eq!(
                    chunk.data, b"pending",
                    "the newly-deferred Chunk must survive"
                );
            }
            other => panic!(
                "expected the Chunk to be queued FIRST, immediately before the \
                 VisibilityResume, got {other:?}"
            ),
        }
        match deferred.pop_front() {
            Some(DeferredOutputItem::VisibilityResume(pane_id)) => assert_eq!(pane_id, 1),
            other => panic!(
                "expected the VisibilityResume to remain queued SECOND (not dropped, not \
                 overtaken), got {other:?}"
            ),
        }
        assert!(deferred.pop_front().is_none());
    }

    /// AC-2: once coalescing still leaves more than `MAX_DEFERRED_ITEMS`
    /// DISTINCT panes' chunks queued, the OLDEST surviving chunk is evicted
    /// — never the one just pushed (AC-2 forbids ever dropping the newest).
    #[test]
    fn deferred_output_queue_drops_oldest_distinct_pane_chunk_past_the_cap_never_the_newest() {
        let mut deferred = DeferredOutputQueue::new();
        let total = MAX_DEFERRED_ITEMS * 2;
        for pane_id in 0..(total as u32) {
            deferred.defer_chunk(PtyOutputChunk::pty_output(pane_id, vec![pane_id as u8]));
        }
        assert_eq!(
            deferred.len(),
            MAX_DEFERRED_ITEMS,
            "queue must never grow past the documented cap"
        );

        let mut surviving_pane_ids = Vec::new();
        while let Some(item) = deferred.pop_front() {
            match item {
                DeferredOutputItem::Chunk(chunk) => surviving_pane_ids.push(chunk.pane_id),
                other => panic!("expected only Chunk items, got {other:?}"),
            }
        }
        let expected: Vec<u32> = ((total - MAX_DEFERRED_ITEMS) as u32..total as u32).collect();
        assert_eq!(
            surviving_pane_ids, expected,
            "the oldest distinct-pane chunks must be evicted first, so only the \
             most-recently-deferred MAX_DEFERRED_ITEMS panes survive — including \
             the very last (newest) one pushed"
        );
    }

    /// AC-1 (task0003 rework, review round 2 findings `4999311c8becf7eb`/
    /// `ff58ab6fd17542f4`/`1d648d947b4dea8b`): visibility resumes are no
    /// longer subject to `MAX_DEFERRED_ITEMS` — a session with more
    /// non-exited panes than the (former, now chunk-only) cap must not
    /// strand any of them.
    #[test]
    fn deferred_output_queue_never_drops_visibility_resumes_past_the_former_cap() {
        let mut deferred = DeferredOutputQueue::new();
        let total = MAX_DEFERRED_ITEMS * 2 + 3;
        for pane_id in 0..(total as u32) {
            deferred.defer_visibility_resume(pane_id);
        }
        assert_eq!(
            deferred.len(),
            total,
            "distinct-pane visibility resumes must never be dropped for capacity"
        );
    }

    /// AC-1: a repeated resume request for the SAME pane deduplicates
    /// instead of growing the queue.
    #[test]
    fn deferred_output_queue_dedupes_repeated_visibility_resume_for_same_pane() {
        let mut deferred = DeferredOutputQueue::new();
        deferred.defer_visibility_resume(42);
        deferred.defer_visibility_resume(42);
        deferred.defer_visibility_resume(42);
        assert_eq!(
            deferred.len(),
            1,
            "repeated resume requests for the same pane must deduplicate"
        );
    }
}
