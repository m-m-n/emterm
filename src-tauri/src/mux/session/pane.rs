//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::agent_status::{AgentState, AgentStatusEvent};
use crate::mux::scrollback_buffer::{DEFAULT_SCROLLBACK_CAPACITY, ScrollbackRingBuffer};
use crate::mux::snapshot_bytes::build_resume_snapshot_bytes;
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

/// Channel carrying a raw agent-status OSC 777 payload string (the full
/// `emterm;agent-status;…` body, `agent_status::parse`'s input contract)
/// from a pane's reader thread to the daemon-level agent-status task
/// (`pane_id`, `payload`).
///
/// Unlike `NotificationSender`, this must be forwarded regardless of attach
/// state (SPEC FR3: the daemon owns per-pane agent-status state
/// unconditionally, not only while detached) — mirroring the daemon-lifetime
/// `TitleChangeSender` wiring rather than the Detached-only notification
/// scanner.
pub type AgentStatusReportSender = mpsc::Sender<(PaneId, String)>;

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
    /// Pane was switched (back) into Connected mode. The handler must send
    /// `snapshot` on the channel before any subsequent reader chunk.
    ResumeWithSnapshot { snapshot: Vec<u8> },
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
                    EvalResult::ResumeWithSnapshot {
                        snapshot: encoded_snapshot,
                    }
                }
            }
        }
    }
}

/// FR9 race-free Detached -> Connected resume.
///
/// The caller obtains an `mpsc::Permit` for `pane_output_tx` *outside* the
/// pane lock (via `Sender::reserve().await`), then hands it in here. This
/// function holds the pane's `output_target` mutex for the full lifetime of
/// (build snapshot, send via permit, swap to `Connected`). Because the PTY
/// reader thread also takes the same `output_target` mutex before its
/// `try_send` / `blocking_send`, the reader cannot push a live chunk between
/// the snapshot enqueue and the Connected swap — the snapshot is guaranteed
/// to land first in the channel's FIFO.
///
/// `Permit::send` is consumed and infallible (the slot is already reserved),
/// so the entire sequence runs under the std mutex without `await`.
///
/// Returns `ResumeOutcome::NoChange` when the pane is not eligible to
/// resume (already Connected, owner mismatch, or `NetworkDetach` still
/// active). The caller should drop the permit on `NoChange` to release the
/// reserved slot.
pub fn resume_pane_with_permit(
    pane: &MuxPane,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
    permit: mpsc::Permit<'_, PtyOutputChunk>,
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
fn clamp_dims_to_wire_domain(cols: u16, rows: u16) -> (u16, u16) {
    let (cols, rows) = term_core::terminal_core::clamp_resize_dims(cols, rows);
    if (cols as u32) * (rows as u32) <= mux_ipc::protocol::MAX_SEGMENT_CELLS {
        return (cols, rows);
    }
    let max_rows = (mux_ipc::protocol::MAX_SEGMENT_CELLS / cols as u32).max(1) as u16;
    (cols, rows.min(max_rows))
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
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
        let (cols, rows) = clamp_dims_to_wire_domain(cols, rows);
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
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
            scrollback: Arc::new(StdMutex::new(scrollback)),
            dims: Arc::new(PaneDims::new(cols, rows)),
        }
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
    pub fn apply_agent_status_event(&self, event: AgentStatusEvent) -> u64 {
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
    }

    /// Mark PTY as exited.
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
        self.master = None;
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
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(new_shadow_parser(rows, cols))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            notification_sender: Arc::new(StdMutex::new(None)),
            agent_status: Arc::new(StdMutex::new(AgentStatus::default())),
            agent_status_report_sender: Arc::new(StdMutex::new(None)),
            agent_waiters: Arc::new(StdMutex::new(Vec::new())),
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
    /// still far above `MAX_SEGMENT_CELLS` (1,000,000) the wire decoder
    /// accepts. `rows` must clamp FURTHER, down to
    /// `MAX_SEGMENT_CELLS / cols`, preserving `cols` at the per-axis max.
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
        let pane = MuxPane::new(1, u16::MAX, u16::MAX, target, writer, pair.master);
        let expected_cols = term_core::terminal_core::RESIZE_MARKER_MAX_COLS;
        let expected_rows = (mux_ipc::protocol::MAX_SEGMENT_CELLS / expected_cols as u32) as u16;
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
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);

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
        let pane = MuxPane::new(1, 80, 24, target, writer, pair.master);
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
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);

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
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);

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
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);
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
            EvalResult::ResumeWithSnapshot { snapshot } => {
                let snapshot = decode_snapshot_content(&snapshot);
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
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let outcome = resume_pane_with_permit(&pane, &b_tx, permit);
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
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let first_outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
        let second_outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
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
            EvalResult::ResumeWithSnapshot { snapshot } => {
                assert!(
                    snapshot
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
}
