//! Shared pane-side handles: channel sender type aliases, agent-status
//! state and waiters, the title-sink shadow parser, and pane dimension
//! bookkeeping.

use super::*;
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

    pub(super) fn new(cols: u16, rows: u16) -> Self {
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

    pub(super) fn set(&self, cols: u16, rows: u16) {
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
