//! Deferred-output drain control for the GUI connection loop: the
//! client-message starvation guard, drain-batch arming, and coalescing
//! of consecutive PTY output chunks.

use tokio::sync::mpsc;

use super::CLIENT_MSG_STARVATION_QUOTA;
use crate::mux::session::pane::{ChunkKind, DeferredOutputQueue, PtyOutputChunk};

/// Pure decision function for the G2 starvation guard: whether the
/// client-message arm (`client_reader.next()`) is allowed to be included in
/// THIS `select!` iteration.
///
/// Extracted out of `handle_connection`'s loop body (rather than inlined at
/// the call site) specifically so the quota/reset arithmetic is
/// unit-testable deterministically, with no live connection, timing, or
/// scheduling involved — see this module's `tests` for the direct coverage
/// (`allow_client_message_arm_true_when_no_deferred_work_regardless_of_counter`
/// / `allow_client_message_arm_true_for_quota_iterations_then_excludes_on_the_next`).
/// The accompanying LIVE connection-level regression test
/// (`connection_level_deferred_snapshot_delivered_despite_continuous_client_traffic`)
/// exercises the real `select!` loop end-to-end, but — as that test's own
/// doc explains — the underlying starvation race depends on genuinely
/// gapless message arrival, which real scheduling/network timing does not
/// reliably force either way; THIS function is what actually pins the
/// bounded-iterations guarantee AC-2 requires.
///
/// `has_deferred_work` is the boolean [`has_unforwarded_pane_output`]
/// computes (task0005 rework, G4/AC-4: originally just
/// `pending_deferred_reserve.is_some() || !deferred_output.is_empty()`,
/// extended to also cover the receiver-side channel backlog — see that
/// function's doc for why), computed by the caller once per iteration.
/// `consecutive_client_msgs_while_deferred` is the running count of how
/// many iterations in a row the client arm has already won while deferred
/// work was outstanding.
pub(super) fn allow_client_message_arm(
    has_deferred_work: bool,
    consecutive_client_msgs_while_deferred: u32,
) -> bool {
    !has_deferred_work || consecutive_client_msgs_while_deferred < CLIENT_MSG_STARVATION_QUOTA
}

/// G4 rework (AC-4, mux-window-switch-output-hang task0005, review round 4
/// finding `e6ac2a334424ebd7`): the signal actually fed into
/// [`allow_client_message_arm`] (and the post-`select!` starvation-counter
/// bookkeeping in `handle_connection`).
///
/// ### Why this exists (the gap this closes)
///
/// The ORIGINAL signal — `pending_deferred_reserve.is_some() ||
/// !deferred_output.is_empty()` — observes only the connection-owned
/// deferred-output BOOKKEEPING (the fair-reservation future and the
/// `DeferredOutputQueue`), not whether the item it admitted has actually
/// reached the client yet. The moment a snapshot (or any deferred chunk) is
/// successfully admitted into `pane_output_tx` — whether immediately via
/// `flush_deferred_output`'s `try_send`/`try_reserve`, or via
/// `apply_fair_permit_to_front_deferred_item`'s owned permit — BOTH
/// `pending_deferred_reserve` and `deferred_output` can go
/// empty/`None` on the very same iteration, even though the item is still
/// sitting unforwarded in `pane_output_rx`'s own internal buffer, waiting
/// for the `chunk = pane_output_rx.recv()` arm to actually drain and send
/// it to the client. Under CONTINUOUSLY-ready client messages, the ORIGINAL
/// signal going false right at that moment would let `allow_client_arm`
/// return `true` unconditionally on the next iteration (`has_deferred_work
/// == false` short-circuits the quota entirely) — the biased client-message
/// arm could then win indefinitely, and the already-admitted item would
/// never even let the drain arm get POLLED (never mind resolve). The quota
/// protected ADMISSION into the channel, not DELIVERY out of it — the exact
/// gap this closes.
///
/// ### The fix
///
/// Fold in `!pane_output_rx.is_empty()` — the receiver's own backlog is the
/// direct, always-available signal for "something is queued in the channel
/// that has not yet been forwarded to the client", regardless of whether it
/// arrived via this connection's own deferred-output path or via a PTY
/// reader thread's direct `try_send`/`blocking_send`. This also means
/// ordinary (non-deferred) high-volume PTY output gets the same starvation
/// protection under continuous client traffic — not just this feature's own
/// deferred-snapshot path — which is the correct, broader reading of "queued
/// output must not be starved" this whole feature exists to establish.
pub(super) fn has_unforwarded_pane_output(
    has_deferred_work: bool,
    pane_output_rx: &mpsc::Receiver<PtyOutputChunk>,
) -> bool {
    has_deferred_work || !pane_output_rx.is_empty()
}

/// Pure state-transition for the G2 starvation-guard counter
/// (`consecutive_client_msgs_while_deferred`), extracted from
/// `handle_connection`'s post-`select!` bookkeeping (medium finding
/// connection.rs:820, review round 4) so the increment/reset arithmetic is
/// unit-testable deterministically — mirroring why [`allow_client_message_arm`]
/// itself was extracted.
///
/// Only "the client arm ran while pending output was already outstanding
/// going into THIS iteration" (`took_client_arm && has_pending_output`)
/// increments the count; every other outcome — a different arm ran, or
/// there was no pending output to begin with — resets it to 0, so the
/// one-iteration exclusion `allow_client_message_arm` applies once the quota
/// is hit never compounds, and ordinary traffic (no pending output) is never
/// penalized.
pub(super) fn next_client_msg_starvation_count(
    took_client_arm: bool,
    has_pending_output: bool,
    previous_count: u32,
) -> u32 {
    if took_client_arm && has_pending_output {
        previous_count.saturating_add(1)
    } else {
        0
    }
}

/// A fair, in-flight reservation on a connection's `pane_output_tx`, used
/// only to service `deferred_output` when the ordinary `try_send`/
/// `try_reserve`-based flush (`handlers::flush_deferred_output`) cannot make
/// progress (mux-window-switch-output-hang task0003 rework, AC-3/G2 —
/// review round 2 findings `7e47bd5fe31dc720`/`2aec511b92102c24`).
///
/// Boxed + `dyn` because the concrete `async move { ... }` future type
/// differs at every construction site; erasing it lets this live in a
/// plain `Option` field across `select!` iterations. `Send` is required
/// because `handle_connection` itself is spawned via `tokio::spawn`.
pub(super) type PendingDeferredReserve = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<mpsc::OwnedPermit<PtyOutputChunk>, mpsc::error::SendError<()>>,
            > + Send,
    >,
>;

/// Arm a fair reservation on `pane_output_tx` if `deferred_output` still has
/// work and none is already in flight.
///
/// ### Why (AC-3/G2)
///
/// `flush_deferred_output`'s `try_send`/`try_reserve` retries never join
/// tokio's semaphore waiter queue, so while `pty_spawn.rs`'s reader thread
/// has a `blocking_send` parked there, every freed permit is handed to that
/// waiter directly — `try_send` observes zero capacity essentially always,
/// a systematic priority inversion (review round 2, `2aec511b92102c24`/
/// `7e47bd5fe31dc720`), not an occasional race. Polling
/// `pane_output_tx.clone().reserve_owned()` as its own `select!` arm (see
/// `handle_connection` below) joins that SAME FIFO waiter queue, so this
/// connection's own deferred work is serviced within a bounded number of
/// reader-thread sends — without ever blocking this task (a `select!` arm's
/// future is only ever polled, never awaited to completion outside the
/// macro, so the connection keeps handling every other arm while this one
/// is still pending).
///
/// CRITICAL placement requirement: in `handle_connection`'s `biased;`
/// `select!`, the arm polling this reservation MUST be listed BEFORE the
/// `chunk = pane_output_rx.recv()` arm. `select!` under `biased` polls
/// branches in text order and stops at the first one that resolves; under
/// sustained saturation `pane_output_rx.recv()` is essentially ALWAYS ready,
/// so if it were listed first this reservation's future would never even
/// get POLLED (never mind resolve) — and an un-polled future never
/// registers itself as a waiter on the semaphore in the first place, so it
/// would wait forever regardless of how "fair" `reserve_owned()` itself is.
/// This is not a hypothetical: the connection-level regression test in this
/// module's `tests` (`connection_level_deferred_snapshot_survives_sustained_saturation_and_input_keeps_flowing`)
/// caught exactly this ordering bug during development — the mechanism
/// below is correct, but was originally wired up AFTER the drain arm and
/// therefore never actually ran.
pub(super) fn arm_pending_deferred_reserve(
    pending: &mut Option<PendingDeferredReserve>,
    deferred_output: &DeferredOutputQueue,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) {
    if pending.is_none() && !deferred_output.is_empty() {
        let tx = pane_output_tx.clone();
        *pending = Some(Box::pin(async move { tx.reserve_owned().await }));
    }
}
/// Merge consecutive PTY output chunks from the same pane into a single chunk.
///
/// Preserves ordering across panes. Empty-data chunks (PTY exit signals) are
/// never merged — they remain as separate entries to ensure correct exit
/// handling.
///
/// `kind` is part of the merge key (FR1, FR5): a `Snapshot`-tagged chunk is
/// emitted as one `MessageType::Snapshot` frame regardless of size and MUST
/// NOT be coalesced with adjacent `PtyOutput` chunks — folding would smuggle
/// snapshot bytes into a live-input frame (or vice versa) and break the
/// routing to `apply_mux_message::Snapshot`. Two consecutive `Snapshot`
/// chunks for the same pane also stay separate so each snapshot reply is one
/// IPC frame.
pub(super) fn merge_consecutive_chunks(chunks: Vec<PtyOutputChunk>) -> Vec<PtyOutputChunk> {
    if chunks.len() <= 1 {
        return chunks;
    }
    let mut merged: Vec<PtyOutputChunk> = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.data.is_empty() {
            // Exit signal: never merge
            merged.push(chunk);
        } else if let Some(last) = merged.last_mut() {
            // Only fold when pane, kind, and non-emptiness all match AND the
            // kind is `PtyOutput` (snapshot chunks are framed standalone).
            if last.pane_id == chunk.pane_id
                && !last.data.is_empty()
                && last.kind == chunk.kind
                && chunk.kind == ChunkKind::PtyOutput
            {
                last.data.extend_from_slice(&chunk.data);
            } else {
                merged.push(chunk);
            }
        } else {
            merged.push(chunk);
        }
    }
    merged
}
