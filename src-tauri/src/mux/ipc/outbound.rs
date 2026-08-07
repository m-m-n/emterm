//! Outbound admission path for a GUI connection's client-bound frames
//! (task0001, mux-connection-input-freeze, FR1/FR3/FR4/NFR4; consolidated
//! into a single component by task0003 — see [`OutboundAdmission`]).
//!
//! Once `handle_connection`'s GUI message loop starts, exactly ONE
//! component ever touches the connection socket's write side: the writer
//! task spawned here ([`run_outbound_writer`]). Every producer inside the
//! loop — the drain arm, the notify arm (single forward + Lagged resync,
//! gated off while a remainder is held), the kick arm's `Detached`,
//! `route_message` replies (including via `ReplySink`/`OutboundHandle`),
//! `handle_attach`'s error reply, and `send_reattach_data`'s
//! `PaneCreated`/`SnapshotRestore` — admits its frame(s) through the SAME
//! [`OutboundAdmission`] instance instead of ever touching a raw
//! `mpsc::Sender<MuxMessage>` directly. `OutboundAdmission` OWNS that
//! sender; nothing outside this module (and the writer wiring in
//! `connection::handle_connection`) can reach it. This is what lets the
//! connection loop's client-message arm keep being polled even while the
//! socket's send buffer is saturated — the residual freeze task0001
//! closed — while ALSO guaranteeing (task0003): the held remainder can
//! never grow in proportion to notification/client-message traffic
//! (FR4), every producer's frames reach the wire in acceptance order with
//! no overtaking of a held remainder (FR3), and frames still held at loop
//! exit get a bounded best-effort delivery at teardown rather than being
//! silently dropped.

use std::collections::VecDeque;
use std::pin::Pin;

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, FramedWrite};

use super::codec::MuxCodec;
use super::protocol::{MessageType, MuxMessage};
use crate::mux::daemon::SharedUpgradeAckSlot;

/// Bound on the outbound admission queue (FR4).
///
/// Deliberately SMALL, not merely "small enough": a single admitted item
/// can itself be as large as one whole drain batch collapses to (all
/// consecutive same-pane chunks in a `DRAIN_BATCH_LIMIT`-sized batch merge
/// into ONE `MuxMessage` — see `connection::DRAIN_BATCH_LIMIT`'s doc, ~4MB
/// worst case). The queue's capacity is an ITEM count, not a byte bound, so
/// each extra slot can cost another ~4MB in the worst case, not a fraction
/// of it. Keeping capacity at 2 bounds worst-case QUEUED memory to ~2
/// batches (~8MB).
///
/// [`OutboundAdmission`]'s held remainder (task0003, restored bound) adds
/// AT MOST one more batch on top of that (~4MB): the drain arm only ever
/// holds a remainder while gated off (`is_holding()`), so its contribution
/// is bounded by one `DRAIN_BATCH_LIMIT`-sized drain; the notify arm is
/// gated off identically (never appends while a remainder is held — the
/// single-forward path is UNREACHABLE once a remainder exists, and a
/// resumed Lagged resync contributes at most the current window count,
/// itself bounded by however many windows exist, not by how many
/// notifications were sent while gated off); the kick arm may append
/// exactly one `Detached` frame before breaking the loop; and a
/// `route_message` reply/reattach producer (`admit_blocking`) never GROWS
/// the remainder at all — it only ever drains it. Total worst case stays
/// in the same order of magnitude as today's existing ~4MB
/// `DRAIN_BATCH_LIMIT` bound, not a multiple of it — restoring the
/// "bounded by construction" property this doc previously claimed but the
/// review-round-1 auto-fix loop had silently broken (mux-connection-input-
/// freeze task0003).
pub(super) const OUTBOUND_QUEUE_CAPACITY: usize = 2;

/// Bound on how many admitted frames the writer batches into a single
/// feed+flush cycle before yielding back to check for more (fewer flush
/// syscalls). In practice this rarely matters given
/// [`OUTBOUND_QUEUE_CAPACITY`]'s own small size — the writer can never see
/// more than that many items buffered ahead of it — but is named and
/// bounded regardless (FR4: no unbounded batching), independent of that
/// constant since the two protect different resources.
const WRITER_BATCH_LIMIT: usize = 32;

/// A destination for a single reply/frame send performed OUTSIDE the drain
/// arm (`route_message` replies, the CLI/GUI `Upgrade` reply): either the
/// CLI-client path's still-undivided `Framed` sink (out of scope for
/// task0001 — the CLI path completes before the GUI loop and keeps the
/// pre-task0001 undivided transport, IMPLEMENTATION.md "Out of Scope") or
/// the GUI loop's outbound admission queue via [`OutboundHandle`]
/// (task0001, task0003 rework).
///
/// A trait (not an enum) so each call site uses its own concrete type
/// without threading an unrelated `S: AsyncRead + AsyncWrite` type
/// parameter through the GUI path, which never touches a raw stream —
/// `route_message` and everything it calls are no longer generic over `S`
/// at all after this task (see `connection::route_message`).
pub(super) trait ReplySink {
    async fn send_reply(&mut self, msg: MuxMessage) -> Result<(), ()>;
}

impl<S> ReplySink for Framed<S, MuxCodec>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn send_reply(&mut self, msg: MuxMessage) -> Result<(), ()> {
        self.send(msg).await.map_err(|_| ())
    }
}

/// The GUI loop's outbound admission handle: wraps a borrowed
/// [`OutboundAdmission`] so a handler can send a reply frame through the
/// SAME single ordered admission path every other client-bound frame in
/// the loop uses (module doc "Admission path").
///
/// CONTRACT (task0003, the accepted carve-out task0001 invariant 1 already
/// named): admission here is NOT bounded by outbound queue capacity alone.
/// `OutboundAdmission::admit_blocking` first drains any held remainder,
/// then admits this reply's frame(s), blocking on outbound capacity as
/// needed — bounded only by the client's own read rate, exactly like a
/// direct socket write would be. `await`ing here from inside a `select!`
/// arm body can therefore park the whole connection task (including the
/// drain arm and every other `select!` arm) for as long as the client
/// stalls its reads. This is ACCEPTABLE for reply/reattach producers: they
/// only run from the client-message arm's own body, in response to a
/// message that arm already won, and the dedicated writer task (a SEPARATE
/// tokio task) keeps draining the outbound queue independently throughout
/// — so no OTHER `select!` arm's progress depends on this call returning.
/// Any call site that must keep the loop live under a stalled client — the
/// drain arm — MUST NOT use this handle; it uses
/// [`OutboundAdmission::try_admit`] instead (never blocks).
pub(super) struct OutboundHandle<'a> {
    admission: &'a mut OutboundAdmission,
}

impl<'a> OutboundHandle<'a> {
    pub(super) fn new(admission: &'a mut OutboundAdmission) -> Self {
        Self { admission }
    }
}

impl ReplySink for OutboundHandle<'_> {
    async fn send_reply(&mut self, msg: MuxMessage) -> Result<(), ()> {
        self.admission.admit_blocking(vec![msg]).await
    }
}

/// A fair, in-flight reservation on the outbound queue, used to service a
/// held outbound remainder once the queue is momentarily full (mirrors
/// `connection::PendingDeferredReserve`/`arm_pending_deferred_reserve`
/// exactly — same starvation class, same fix shape, applied to the
/// outbound queue instead of `pane_output_tx`).
type PendingOutboundReserve = Pin<
    Box<
        dyn std::future::Future<
                Output = Result<mpsc::OwnedPermit<MuxMessage>, mpsc::error::SendError<()>>,
            > + Send,
    >,
>;

/// Try to admit every frame in `frames`, IN ORDER, into `outbound_tx`
/// without blocking. Returns the frames that could not be admitted, in
/// original order.
///
/// Pure and independently testable (no live connection needed), mirroring
/// this module's sibling extracted-pure-function style
/// (`connection::allow_client_message_arm` etc.). Private: only
/// [`OutboundAdmission`] calls this — every OTHER producer goes through the
/// component, never this raw helper directly (AC-4).
fn try_admit_outbound_frames(
    outbound_tx: &mpsc::Sender<MuxMessage>,
    frames: Vec<MuxMessage>,
) -> VecDeque<MuxMessage> {
    let mut frames = frames.into_iter();
    while let Some(frame) = frames.next() {
        match outbound_tx.try_send(frame) {
            Ok(()) => continue,
            Err(mpsc::error::TrySendError::Full(frame)) => {
                let mut remainder = VecDeque::with_capacity(frames.len() + 1);
                remainder.push_back(frame);
                remainder.extend(frames);
                return remainder;
            }
            Err(mpsc::error::TrySendError::Closed(_frame)) => {
                // The writer is gone (the socket already failed).
                // `handle_connection`'s writer-completion `select!` arm
                // observes this independently and breaks the loop; nothing
                // more can ever be admitted, so there is no remainder worth
                // holding.
                return VecDeque::new();
            }
        }
    }
    VecDeque::new()
}

/// Arm a fair `reserve_owned()` future on `outbound_tx` if `remainder`
/// still has unsent frames and none is already in flight. Private: folded
/// into [`OutboundAdmission::arm_reserve`].
fn arm_pending_outbound_reserve(
    pending: &mut Option<PendingOutboundReserve>,
    remainder: &VecDeque<MuxMessage>,
    outbound_tx: &mpsc::Sender<MuxMessage>,
) {
    if pending.is_none() && !remainder.is_empty() {
        let tx = outbound_tx.clone();
        *pending = Some(Box::pin(async move { tx.reserve_owned().await }));
    }
}

/// The single outbound admission component (task0003, AC-1/AC-4): owns the
/// bounded queue sender, the held remainder, and the in-flight fair
/// reservation together, so every client-bound frame producer on the GUI
/// loop funnels through ONE ordered path. The raw `mpsc::Sender` is never
/// reachable outside this type — `connection::handle_connection` owns one
/// `OutboundAdmission` instance for the lifetime of its message loop and
/// hands `&mut` (or, for reply producers, ownership of a short-lived
/// [`OutboundHandle`] borrow) to every other producer (`handlers::
/// handle_attach`, `reattach::send_reattach_data`, `route_message`'s
/// replies).
///
/// ### Invariant: `is_holding()` tracks both fields in lockstep
///
/// `remainder` and `pending_reserve` are mutated ONLY by this type's own
/// methods, always synchronously together (never across an `.await`
/// point): whenever `remainder` transitions non-empty, [`Self::arm_reserve`]
/// is called in the same synchronous step, and whenever `remainder`
/// transitions back to empty (in [`Self::apply_reserve_result`] /
/// [`Self::admit_blocking`] / [`Self::teardown_flush`]), `pending_reserve`
/// is cleared alongside it. So `pending_reserve.is_some()` implies
/// `!remainder.is_empty()` at every point either is observed from outside
/// an in-progress method call — [`Self::is_holding`] is a single predicate
/// serving both the loop's drain/notify-arm gates (mirrors the old
/// `outbound_remainder.is_empty()` check) and the starvation-guard's
/// `has_pending_output`/`excludable_work` signals (mirrors the old
/// `!outbound_remainder.is_empty() || pending_outbound_reserve.is_some()`).
///
/// ### FIFO (FR3) — how each producer preserves it
///
/// - [`Self::try_admit`] (drain arm): caller must only call this while
///   `!is_holding()` (the loop's own gate enforces this); admits as many
///   frames as fit, holds the rest as the new remainder.
/// - [`Self::push_or_admit`] (kick arm's `Detached`; notify arm's single
///   forward / Lagged resync loop): appends to an already-held remainder's
///   TAIL when holding, otherwise behaves like `try_admit` for one frame.
///   Never overtakes.
/// - [`Self::admit_blocking`] (reply/reattach producers via
///   [`OutboundHandle`] / direct calls): drains any held remainder FIRST,
///   in order, then admits its own frames — so a reply/reattach frame can
///   never reach the wire ahead of older held output. Cancels
///   `pending_reserve` up front so exactly one mechanism (this call, not
///   the select! loop's fair-reservation arm) drains the remainder —
///   "the component owns that serialization" (task plan Design invariant
///   3).
pub(super) struct OutboundAdmission {
    tx: mpsc::Sender<MuxMessage>,
    remainder: VecDeque<MuxMessage>,
    pending_reserve: Option<PendingOutboundReserve>,
}

impl OutboundAdmission {
    pub(super) fn new(tx: mpsc::Sender<MuxMessage>) -> Self {
        Self {
            tx,
            remainder: VecDeque::new(),
            pending_reserve: None,
        }
    }

    /// "Is holding a remainder" — see the type doc's invariant note for why
    /// one boolean correctly serves both the loop's gates and the
    /// starvation-guard's bookkeeping.
    pub(super) fn is_holding(&self) -> bool {
        !self.remainder.is_empty() || self.pending_reserve.is_some()
    }

    /// Drain arm (FR1, non-blocking): admit `frames`, in order, without
    /// blocking. The caller MUST only invoke this while `!is_holding()`
    /// (the `select!` arm's own `if` guard enforces this — mirrors the old
    /// `chunk = pane_output_rx.recv(), if outbound_remainder.is_empty()`
    /// shape); this never awaits outbound capacity at this point position,
    /// which is exactly the self-block task0001 removes.
    pub(super) fn try_admit(&mut self, frames: Vec<MuxMessage>) {
        debug_assert!(
            self.remainder.is_empty() && self.pending_reserve.is_none(),
            "try_admit called while a remainder was already held — caller's gate is broken"
        );
        let remainder = try_admit_outbound_frames(&self.tx, frames);
        if !remainder.is_empty() {
            self.remainder = remainder;
            self.arm_reserve();
        }
    }

    /// Kick arm / notify arm (FR3, no overtaking): append `frame` to a
    /// held remainder's tail, or admit it immediately (non-blocking) when
    /// nothing is currently held. Used where the caller may run either
    /// while holding or not (the kick arm always; the notify arm's Lagged
    /// resync loop across its own iterations).
    pub(super) fn push_or_admit(&mut self, frame: MuxMessage) {
        if self.is_holding() {
            self.remainder.push_back(frame);
        } else {
            self.try_admit(vec![frame]);
        }
    }

    /// Whether a fair reservation is currently armed — the `select!` arm's
    /// own `if` guard (mirrors the pre-task0003
    /// `pending_outbound_reserve.is_some()` check).
    pub(super) fn has_pending_reserve(&self) -> bool {
        self.pending_reserve.is_some()
    }

    /// Poll the in-flight reservation to completion. Only ever called from
    /// the `select!` arm gated by [`Self::has_pending_reserve`] — panics
    /// (via `Option::expect`) if called with nothing armed, mirroring the
    /// pre-task0003 `pending_outbound_reserve.as_mut().unwrap()` shape
    /// (a caller bug, not a runtime condition).
    pub(super) async fn poll_pending_reserve(
        &mut self,
    ) -> Result<mpsc::OwnedPermit<MuxMessage>, mpsc::error::SendError<()>> {
        self.pending_reserve
            .as_mut()
            .expect("poll_pending_reserve called with no reservation armed")
            .await
    }

    /// Apply the result of a resolved fair reservation to the front of the
    /// remainder (mirrors the pre-task0003 `outbound_permit_result` arm
    /// body), then re-arm if more work remains.
    pub(super) fn apply_reserve_result(
        &mut self,
        result: Result<mpsc::OwnedPermit<MuxMessage>, mpsc::error::SendError<()>>,
    ) {
        self.pending_reserve = None;
        match result {
            Ok(permit) => {
                if let Some(frame) = self.remainder.pop_front() {
                    let _ = permit.send(frame);
                } else {
                    drop(permit);
                }
            }
            Err(_) => {
                log::warn!(
                    "outbound queue closed while an outbound-remainder reservation was \
                     pending; dropping the remaining outbound backlog (the writer's own \
                     completion is what actually tears this connection down)"
                );
                self.remainder.clear();
            }
        }
        self.arm_reserve();
    }

    fn arm_reserve(&mut self) {
        arm_pending_outbound_reserve(&mut self.pending_reserve, &self.remainder, &self.tx);
    }

    /// Reply/reattach producers (FR3 invariant 2; task0001 invariant 1's
    /// carve-out — see [`OutboundHandle`]'s doc for why blocking here is
    /// acceptable): drains any held remainder FIRST, in FIFO order, then
    /// admits `frames` — so these frames can never overtake older held
    /// output, and the remainder never ends up LARGER than before this
    /// call (it only ever shrinks, to empty, on success). Cancels
    /// `pending_reserve` up front so exactly one mechanism drains the
    /// remainder at a time (this call, not the `select!` loop's own fair-
    /// reservation arm) — "the component owns that serialization".
    ///
    /// `Err(())` means the outbound queue is closed (the writer already
    /// exited); the connection's own `writer_result` arm independently
    /// detects that and tears the connection down — callers propagate the
    /// error (typically as `Err(true)`, "close the connection") rather
    /// than re-deriving it.
    pub(super) async fn admit_blocking(&mut self, frames: Vec<MuxMessage>) -> Result<(), ()> {
        self.pending_reserve = None;
        while let Some(frame) = self.remainder.pop_front() {
            self.tx.send(frame).await.map_err(|_| ())?;
        }
        for frame in frames {
            self.tx.send(frame).await.map_err(|_| ())?;
        }
        Ok(())
    }

    /// Teardown (Design invariant 4): a bounded best-effort FIFO admission
    /// of whatever remainder is still held, then release every sender this
    /// component owns (by being consumed) so the writer observes the
    /// channel closed, drains anything already admitted, and exits.
    /// Callers wrap this in the existing named teardown-flush timeout
    /// (`connection::OUTBOUND_TEARDOWN_FLUSH_TIMEOUT`) alongside the
    /// writer `JoinHandle` await — a slow/never-reading client bounds this
    /// exactly as it already bounded the join wait, so frames that do not
    /// fit within the budget are dropped exactly as today's already-
    /// admitted-only flush would have dropped them (best-effort, not
    /// absolute).
    pub(super) async fn teardown_flush(mut self) {
        self.pending_reserve = None;
        while let Some(frame) = self.remainder.pop_front() {
            if self.tx.send(frame).await.is_err() {
                break;
            }
        }
        // `self.tx` (and everything else this component owns) drops here,
        // at scope end — the writer's `outbound_rx.recv()` then observes
        // the channel closed once it has drained whatever was already
        // admitted.
    }
}

/// The dedicated outbound writer (FR1/FR3): the ONLY component that ever
/// touches the connection socket's write side once the GUI message loop
/// starts. Owns `sink` (the write half of the client transport, split via
/// `tokio::io::split` in `handle_connection`) and `outbound_rx` (the
/// bounded admission queue every client-bound frame producer in the loop
/// feeds via [`OutboundAdmission`]). Drains independently of the
/// connection loop's own scheduling: its progress depends only on the
/// socket, never on which `select!` arm the loop happens to be polling.
///
/// Exits in exactly two ways:
/// - A write/flush failure: logs and returns immediately (AC-6, Design
///   invariant 7). `handle_connection` observes this via its own
///   `writer_task_handle`'s completion, itself a `select!` arm, and tears
///   the connection down.
/// - `outbound_rx` closes (every `Sender` clone dropped) AND is drained:
///   this only happens after `handle_connection`'s own loop has already
///   broken and its [`OutboundAdmission`] has been consumed by
///   [`OutboundAdmission::teardown_flush`] — graceful shutdown, draining
///   whatever was already admitted first (best-effort teardown flush,
///   Design invariant 4).
///
/// NFR4 (ack-after-flush): after a batch is successfully written AND
/// flushed, every `MessageType::Upgrading` frame in that batch fires the
/// upgrade-ack notification via `upgrade_ack_slot` — mirrors the
/// pre-task0001 inline check (`msg.msg_type == MessageType::Upgrading`,
/// checked right after `framed.send` succeeded) just relocated to the
/// point where delivery, not merely admission, is actually confirmed.
pub(super) async fn run_outbound_writer<W>(
    mut sink: FramedWrite<W, MuxCodec>,
    mut outbound_rx: mpsc::Receiver<MuxMessage>,
    upgrade_ack_slot: SharedUpgradeAckSlot,
) where
    W: AsyncWrite + Unpin,
{
    while let Some(first) = outbound_rx.recv().await {
        let mut batch = Vec::with_capacity(WRITER_BATCH_LIMIT);
        batch.push(first);
        while batch.len() < WRITER_BATCH_LIMIT {
            match outbound_rx.try_recv() {
                Ok(item) => batch.push(item),
                Err(_) => break,
            }
        }

        let mut upgrading_in_batch = false;
        let mut write_failed = false;
        for msg in batch {
            if msg.msg_type == MessageType::Upgrading {
                upgrading_in_batch = true;
            }
            if sink.feed(msg).await.is_err() {
                write_failed = true;
                break;
            }
        }
        if sink.flush().await.is_err() {
            write_failed = true;
        }
        if write_failed {
            log::warn!(
                "mux outbound writer: socket write/flush failed; exiting (the \
                 connection loop observes this via the writer task's own \
                 completion, AC-6)"
            );
            return;
        }
        if upgrading_in_batch {
            let ack_tx = upgrade_ack_slot.lock().unwrap().clone();
            if let Some(ack_tx) = ack_tx {
                let _ = ack_tx.try_send(());
            }
        }
    }
    log::debug!("mux outbound writer: admission queue closed, exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pane_id: u32) -> MuxMessage {
        MuxMessage::pty_output(pane_id, vec![b'x'; 4])
    }

    /// When the outbound queue has room for everything, every frame is
    /// admitted in order and the remainder is empty.
    #[tokio::test]
    async fn try_admit_outbound_frames_admits_all_when_capacity_available() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(8);
        let frames = vec![frame(1), frame(2), frame(3)];
        let remainder = try_admit_outbound_frames(&tx, frames);
        assert!(remainder.is_empty());
        assert_eq!(rx.try_recv().unwrap().pane_id, 1);
        assert_eq!(rx.try_recv().unwrap().pane_id, 2);
        assert_eq!(rx.try_recv().unwrap().pane_id, 3);
        assert!(rx.try_recv().is_err(), "no more frames expected");
    }

    /// When the outbound path is full, the admitter STOPS — unsent frames
    /// are returned as a remainder, in original order, rather than
    /// buffered without bound.
    #[tokio::test]
    async fn try_admit_outbound_frames_holds_remainder_in_order_when_queue_full() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let frames = vec![frame(1), frame(2), frame(3)];
        let remainder = try_admit_outbound_frames(&tx, frames);

        // The first frame fit; the rest are held, in order.
        assert_eq!(rx.try_recv().unwrap().pane_id, 1);
        assert!(rx.try_recv().is_err(), "queue capacity was only 1");
        let remainder: Vec<u32> = remainder.iter().map(|m| m.pane_id).collect();
        assert_eq!(remainder, vec![2, 3]);
    }

    /// A closed receiver (writer already gone) yields an empty remainder —
    /// nothing more can ever be admitted, so there is nothing worth
    /// holding.
    #[tokio::test]
    async fn try_admit_outbound_frames_empty_remainder_when_channel_closed() {
        let (tx, rx) = mpsc::channel::<MuxMessage>(4);
        drop(rx);
        let remainder = try_admit_outbound_frames(&tx, vec![frame(1), frame(2)]);
        assert!(remainder.is_empty());
    }

    /// `arm_pending_outbound_reserve` only arms when there is a non-empty
    /// remainder and nothing already in flight; it is a no-op otherwise.
    #[tokio::test]
    async fn arm_pending_outbound_reserve_only_arms_for_nonempty_remainder() {
        let (tx, _rx) = mpsc::channel::<MuxMessage>(4);

        let mut pending: Option<PendingOutboundReserve> = None;
        let empty: VecDeque<MuxMessage> = VecDeque::new();
        arm_pending_outbound_reserve(&mut pending, &empty, &tx);
        assert!(pending.is_none(), "must not arm for an empty remainder");

        let mut non_empty: VecDeque<MuxMessage> = VecDeque::new();
        non_empty.push_back(frame(1));
        arm_pending_outbound_reserve(&mut pending, &non_empty, &tx);
        assert!(pending.is_some(), "must arm for a non-empty remainder");

        // Already armed: a second call is a no-op (does not replace the
        // in-flight future).
        arm_pending_outbound_reserve(&mut pending, &non_empty, &tx);
        assert!(pending.is_some());
    }

    // ── OutboundAdmission (task0003 AC-1/AC-3/AC-4/AC-5): the consolidated
    // component's own contract, independent of a live connection. ──

    /// A fresh component holds nothing.
    #[tokio::test]
    async fn admission_is_holding_false_initially() {
        let (tx, _rx) = mpsc::channel::<MuxMessage>(4);
        let admission = OutboundAdmission::new(tx);
        assert!(!admission.is_holding());
    }

    /// `try_admit` admits everything it can, and `is_holding()` reports
    /// exactly the remainder it could not admit.
    #[tokio::test]
    async fn admission_try_admit_sets_is_holding_on_partial_admit() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        admission.try_admit(vec![frame(1), frame(2), frame(3)]);
        assert!(
            admission.is_holding(),
            "AC-1: frames past capacity must be held as a remainder"
        );
        assert_eq!(rx.try_recv().unwrap().pane_id, 1);
        assert!(rx.try_recv().is_err(), "queue capacity was only 1");
    }

    /// AC-1/FR4 (the core regression this task fixes): once a remainder is
    /// held, `push_or_admit` — the kick/notify producers' entry point —
    /// APPENDS to the tail rather than growing some separate unbounded
    /// structure, and the total held count is exactly what was appended,
    /// no more: repeated calls neither duplicate nor reorder.
    #[tokio::test]
    async fn admission_push_or_admit_appends_to_held_remainder_in_order() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        // Fill the queue's one slot, forcing a held remainder starting at
        // frame 2.
        admission.try_admit(vec![frame(1), frame(2)]);
        assert!(admission.is_holding());
        assert_eq!(rx.recv().await.unwrap().pane_id, 1);

        // Sustained "notification-like" traffic while held: each call
        // appends exactly one frame — never more, never fewer.
        for id in [3u32, 4, 5, 6, 7] {
            admission.push_or_admit(frame(id));
        }

        // Drain the held remainder (in order) via `admit_blocking` with no
        // new frames of its own, concurrently with a receiver that reads
        // it all back — proves both the ORDER and that nothing was lost
        // or duplicated.
        let drain_fut = admission.admit_blocking(Vec::new());
        let read_fut = async {
            let mut ids = Vec::new();
            for _ in 0..6 {
                ids.push(rx.recv().await.unwrap().pane_id);
            }
            ids
        };
        let (drain_result, ids) = tokio::join!(drain_fut, read_fut);
        drain_result.expect("admit_blocking must succeed while the receiver keeps draining");

        assert_eq!(
            ids,
            vec![2, 3, 4, 5, 6, 7],
            "AC-1/FR3: appended frames must stay in arrival order, with none dropped \
             or duplicated, regardless of how many were appended while held"
        );
    }

    /// AC-3 (no overtaking): `admit_blocking` drains any held remainder
    /// FIRST, in order, and only then admits its own new frames — a reply/
    /// reattach producer's frames can never reach the wire ahead of older
    /// held output.
    #[tokio::test]
    async fn admission_admit_blocking_drains_remainder_before_new_frames_in_order() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        admission.try_admit(vec![frame(1), frame(2), frame(3)]);
        assert!(admission.is_holding(), "frames 2 and 3 must be held");
        // Frame 1 was already admitted before the remainder existed, so
        // drain it out of the way — it is not part of THIS call's
        // ordering claim.
        assert_eq!(rx.recv().await.unwrap().pane_id, 1);

        let drain_fut = admission.admit_blocking(vec![frame(100), frame(101)]);
        let read_fut = async {
            let mut ids = Vec::new();
            for _ in 0..4 {
                ids.push(rx.recv().await.unwrap().pane_id);
            }
            ids
        };
        let (drain_result, ids) = tokio::join!(drain_fut, read_fut);
        drain_result.expect("admit_blocking must succeed while the receiver keeps draining");

        // 2 and 3 (the held remainder) must precede 100 and 101 (the new
        // reply frames).
        assert_eq!(
            ids,
            vec![2, 3, 100, 101],
            "AC-3: reply/reattach frames must arrive strictly after every held \
             remainder frame — no overtaking"
        );
        assert!(
            !admission.is_holding(),
            "admit_blocking must leave the remainder empty on success"
        );
    }

    /// task plan Design invariant 3 ("the component owns that
    /// serialization"): if a fair reservation was already armed when
    /// `admit_blocking` is called, it must be cancelled rather than left
    /// to race the blocking drain — otherwise the reservation could
    /// silently steal a permit that `admit_blocking`'s own drain loop
    /// needed, or apply a stale frame reference after the remainder has
    /// already been drained by the blocking path.
    #[tokio::test]
    async fn admission_admit_blocking_cancels_a_stale_pending_reserve() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        admission.try_admit(vec![frame(1), frame(2)]);
        assert!(
            admission.has_pending_reserve(),
            "test prerequisite: a reservation must be armed for a held remainder"
        );

        let drain_fut = admission.admit_blocking(vec![frame(100)]);
        let read_fut = async {
            let mut ids = Vec::new();
            for _ in 0..3 {
                ids.push(rx.recv().await.unwrap().pane_id);
            }
            ids
        };
        let (drain_result, ids) = tokio::join!(drain_fut, read_fut);
        drain_result.expect("admit_blocking must succeed");

        assert_eq!(ids, vec![1, 2, 100]);
        assert!(
            !admission.has_pending_reserve(),
            "admit_blocking must cancel any reservation it superseded, leaving no \
             stale future that could later apply a frame from an already-empty \
             remainder"
        );
    }

    /// AC-5 (teardown delivery): `teardown_flush` admits whatever
    /// remainder is still held, in order, into the channel before
    /// releasing the sender — a consumer draining the channel receives
    /// the held frames.
    #[tokio::test]
    async fn admission_teardown_flush_admits_held_remainder_in_order() {
        let (tx, mut rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        admission.try_admit(vec![frame(1), frame(2), frame(3)]);
        assert!(admission.is_holding());

        let flush_fut = admission.teardown_flush();
        let read_fut = async {
            let mut ids = Vec::new();
            for _ in 0..3 {
                ids.push(rx.recv().await.unwrap().pane_id);
            }
            ids
        };
        let (_, ids) = tokio::join!(flush_fut, read_fut);
        assert_eq!(ids, vec![1, 2, 3]);
        // The sender is dropped along with `admission` (consumed by
        // `teardown_flush`), so the receiver now observes the channel
        // closed.
        assert!(rx.recv().await.is_none());
    }

    /// AC-5 companion (never-reading consumer): `teardown_flush` against a
    /// channel with NO draining receiver does not hang forever when the
    /// caller bounds it externally (mirrors how `connection.rs` wraps this
    /// call in `OUTBOUND_TEARDOWN_FLUSH_TIMEOUT`) — this test pins that
    /// the future itself is a plain, cancellable `.await` chain (dropping
    /// it, as a `tokio::time::timeout` would, leaves no residual state to
    /// clean up).
    #[tokio::test]
    async fn admission_teardown_flush_is_cancellable_when_nothing_drains_the_channel() {
        let (tx, rx) = mpsc::channel::<MuxMessage>(1);
        let mut admission = OutboundAdmission::new(tx);
        admission.try_admit(vec![frame(1), frame(2), frame(3)]);
        // Fill the one channel slot too, so even the FIRST send inside
        // `teardown_flush` blocks (nobody ever reads `rx`).
        // `try_admit` already put frame(1) in the channel and held 2/3 as
        // remainder, so the channel is already full — no extra send
        // needed.
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            admission.teardown_flush(),
        )
        .await;
        assert!(
            result.is_err(),
            "with nobody draining the channel, teardown_flush must genuinely block \
             until the caller's own timeout cancels it — proving the bound comes \
             from the CALLER (connection.rs's OUTBOUND_TEARDOWN_FLUSH_TIMEOUT), not \
             from an internal deadline"
        );
        drop(rx);
    }
}
