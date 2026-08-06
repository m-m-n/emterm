//! Outbound admission path for a GUI connection's client-bound frames
//! (task0001, mux-connection-input-freeze, FR1/FR3/FR4/NFR4).
//!
//! Once `handle_connection`'s GUI message loop starts, exactly ONE
//! component ever touches the connection socket's write side: the writer
//! task spawned here ([`run_outbound_writer`]). Every producer inside the
//! loop — the drain arm, `route_message` replies, reattach frames,
//! notification forwards, the kick arm's `Detached` — admits its frame(s)
//! into a single bounded FIFO queue instead (`outbound_tx` /
//! `OutboundHandle`), never touching the socket directly. This is what lets
//! the connection loop's client-message arm keep being polled even while
//! the socket's send buffer is saturated — the residual freeze this task
//! closes (see the task plan's Design "Problem (current shape)").

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
/// batches (~8MB) — the drain arm's own "held remainder" state adds at
/// most one more batch on top (~4MB, see `handle_connection`'s
/// `outbound_remainder`) — staying in the same order of magnitude as
/// today's existing ~4MB `DRAIN_BATCH_LIMIT` bound, not a multiple of it.
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
/// (task0001).
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

/// The GUI loop's outbound admission handle: wraps a borrowed clone of the
/// admission queue's sender so a handler can send a reply frame through the
/// SAME single ordered path every other client-bound frame in the loop uses
/// (Design "Admission path"). Handlers "MAY await admission" here (Design
/// invariant 1) — this is admission into a queue the writer drains
/// independently, not a direct socket write, so it is bounded by outbound
/// queue capacity rather than by the client's own read rate. Only the
/// drain arm itself (`handle_connection`) is forbidden from awaiting here;
/// see [`OUTBOUND_QUEUE_CAPACITY`]'s doc for that arm's own non-blocking
/// mechanism.
pub(super) struct OutboundHandle<'a> {
    tx: &'a mpsc::Sender<MuxMessage>,
}

impl<'a> OutboundHandle<'a> {
    pub(super) fn new(tx: &'a mpsc::Sender<MuxMessage>) -> Self {
        Self { tx }
    }
}

impl ReplySink for OutboundHandle<'_> {
    async fn send_reply(&mut self, msg: MuxMessage) -> Result<(), ()> {
        self.tx.send(msg).await.map_err(|_| ())
    }
}

/// A fair, in-flight reservation on `outbound_tx`, used to service a held
/// outbound remainder once the queue is momentarily full (mirrors
/// `connection::PendingDeferredReserve`/`arm_pending_deferred_reserve`
/// exactly — same starvation class, same fix shape, applied to the new
/// outbound queue this task adds instead of `pane_output_tx`).
pub(super) type PendingOutboundReserve = Pin<
    Box<
        dyn std::future::Future<
                Output = Result<mpsc::OwnedPermit<MuxMessage>, mpsc::error::SendError<()>>,
            > + Send,
    >,
>;

/// Try to admit every frame in `frames`, IN ORDER, into `outbound_tx`
/// without blocking (FR1, Design invariant 1: the drain arm must never
/// await outbound capacity at a point position). Returns the frames that
/// could not be admitted, in original order — the caller holds these as a
/// remainder (bounded by one drain batch — see [`OUTBOUND_QUEUE_CAPACITY`]'s
/// doc) until outbound capacity frees.
///
/// Pure and independently testable (no live connection needed), mirroring
/// this module's sibling extracted-pure-function style
/// (`connection::allow_client_message_arm` etc.).
pub(super) fn try_admit_outbound_frames(
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
/// still has unsent frames and none is already in flight.
///
/// CRITICAL placement requirement (mirrors
/// `connection::arm_pending_deferred_reserve`'s own doc): in
/// `handle_connection`'s `biased;` `select!`, the arm polling this
/// reservation MUST be listed BEFORE the `chunk = pane_output_rx.recv()`
/// drain arm, for the identical reason — under sustained saturation the
/// drain arm's own gate (`if outbound_remainder.is_empty()`) keeps it from
/// running while a remainder is held, but an un-polled reservation future
/// never registers itself as a semaphore waiter in the first place, so
/// ordering still matters for this arm to ever make progress relative to
/// whichever arm is listed first among the ready ones.
pub(super) fn arm_pending_outbound_reserve(
    pending: &mut Option<PendingOutboundReserve>,
    remainder: &VecDeque<MuxMessage>,
    outbound_tx: &mpsc::Sender<MuxMessage>,
) {
    if pending.is_none() && !remainder.is_empty() {
        let tx = outbound_tx.clone();
        *pending = Some(Box::pin(async move { tx.reserve_owned().await }));
    }
}

/// The dedicated outbound writer (FR1/FR3): the ONLY component that ever
/// touches the connection socket's write side once the GUI message loop
/// starts. Owns `sink` (the write half of the client transport, split via
/// `tokio::io::split` in `handle_connection`) and `outbound_rx` (the
/// bounded admission queue every client-bound frame producer in the loop
/// feeds). Drains independently of the connection loop's own scheduling:
/// its progress depends only on the socket, never on which `select!` arm
/// the loop happens to be polling.
///
/// Exits in exactly two ways:
/// - A write/flush failure: logs and returns immediately (AC-6, Design
///   invariant 7). `handle_connection` observes this via its own
///   `writer_task_handle`'s completion, itself a `select!` arm, and tears
///   the connection down.
/// - `outbound_rx` closes (every `Sender` clone dropped) AND is drained:
///   this only happens after `handle_connection`'s own loop has already
///   broken and dropped its `outbound_tx` — graceful shutdown, draining
///   whatever was already admitted first (best-effort teardown flush,
///   Design invariant 7).
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

    /// AC-3 (FR4): when the outbound path is full, the admitter STOPS —
    /// unsent frames are returned as a remainder, in original order,
    /// rather than buffered without bound. This is the mechanism that lets
    /// `handle_connection`'s drain arm stop consuming `pane_output_rx`
    /// (upstream backpressure observable) instead of growing an unbounded
    /// queue.
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
    /// holding; `handle_connection`'s separate writer-completion arm is
    /// what actually tears the connection down (AC-6).
    #[tokio::test]
    async fn try_admit_outbound_frames_empty_remainder_when_channel_closed() {
        let (tx, rx) = mpsc::channel::<MuxMessage>(4);
        drop(rx);
        let remainder = try_admit_outbound_frames(&tx, vec![frame(1), frame(2)]);
        assert!(remainder.is_empty());
    }

    /// `arm_pending_outbound_reserve` only arms when there is a non-empty
    /// remainder and nothing already in flight; it is a no-op otherwise —
    /// mirrors `connection::arm_pending_deferred_reserve`'s own tested
    /// contract.
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
}
