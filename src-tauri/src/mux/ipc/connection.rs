//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::codec::{Framed, FramedRead, FramedWrite};

use super::codec::MuxCodec;
use super::handlers::{
    apply_fair_permit_to_front_deferred_item, flush_deferred_output, handle_attach,
    handle_create_window, handle_destroy_pane, handle_destroy_window, handle_move_window,
    handle_read_pane, handle_rename_window, handle_request_pane_snapshot, handle_resize,
    handle_send_text, handle_set_visibility, handle_switch_window, handle_wait_agent_state,
};
use super::outbound::{
    OUTBOUND_QUEUE_CAPACITY, OutboundHandle, PendingOutboundReserve, ReplySink,
    arm_pending_outbound_reserve, run_outbound_writer, try_admit_outbound_frames,
};
use super::protocol::*;
use super::reattach::detach_session_panes;
use crate::mux::daemon::{SharedUpgradeAckSlot, UpgradeSignal, UpgradeSignalSender};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatusReportSender, ChunkKind, DeferredOutputQueue, NotificationSender, PtyOutputChunk,
    SharedPaneExitSender, TitleChangeSender,
};

/// Handshake timeout: client must send Hello within this duration.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Bound on how long a CLI client's `Upgrade` request waits for the accept
/// loop to finish preparation (probe + snapshot) before giving up and
/// reporting a timeout. Generous because the probe shells out to a
/// candidate binary and the snapshot may capture nontrivial scrollback.
const UPGRADE_PREPARE_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of PTY output chunks to drain per select! iteration.
/// Balances batch efficiency (fewer syscalls) against input responsiveness
/// (returning to select! to check for PtyInput). At 64 chunks × 65KB max
/// each, worst-case batch memory is ~4MB (transient, freed after flush).
const DRAIN_BATCH_LIMIT: usize = 64;

/// Bound on the best-effort teardown flush after the GUI loop exits
/// (task0001, Design invariant 7): once the loop breaks (kick, client EOF,
/// error) and drops its own `outbound_tx`, the outbound writer drains
/// whatever was already admitted and exits; this bounds how long
/// `handle_connection` waits for that drain before abandoning it and
/// returning anyway. Short and named — this is cleanup after the
/// connection is already ending, not a correctness-critical wait.
const OUTBOUND_TEARDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// Starvation-guard quota (G2 rework, mux-window-switch-output-hang
/// task0004, review round 3 findings `dd23cfc388062939`/`5c01ffb8d53dc9f7`).
///
/// Bound on how many CONSECUTIVE `select!` iterations the client-message
/// arm (`client_reader.next()`, task0001 rename — was `framed.next()`
/// before the read/write split) may win while deferred output work (a fair
/// reservation, or a non-empty `deferred_output`) is outstanding, before it
/// is excluded from exactly one iteration so the reservation/drain arms are
/// guaranteed to be POLLED at least once. See the design-decision doc at
/// `msg = client_reader.next()`'s guard, below, for why this exists and why
/// it is a bounded round-robin rather than either arm unconditionally
/// winning.
const CLIENT_MSG_STARVATION_QUOTA: u32 = 8;

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
fn allow_client_message_arm(
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
fn has_unforwarded_pane_output(
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
fn next_client_msg_starvation_count(
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
type PendingDeferredReserve = std::pin::Pin<
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
fn arm_pending_deferred_reserve(
    pending: &mut Option<PendingDeferredReserve>,
    deferred_output: &DeferredOutputQueue,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) {
    if pending.is_none() && !deferred_output.is_empty() {
        let tx = pane_output_tx.clone();
        *pending = Some(Box::pin(async move { tx.reserve_owned().await }));
    }
}

/// Handle a new client connection through handshake and message loop.
#[allow(clippy::too_many_arguments)]
pub async fn handle_connection<S>(
    stream: S,
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    daemon_title_tx: TitleChangeSender,
    daemon_notification_tx: NotificationSender,
    daemon_agent_status_tx: AgentStatusReportSender,
    daemon_pane_exit_sender: SharedPaneExitSender,
    upgrade_tx: UpgradeSignalSender,
    upgrade_ack_slot: SharedUpgradeAckSlot,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut framed = Framed::new(stream, MuxCodec::new());

    // Wait for Hello with timeout to prevent idle connection DoS
    let hello_result = tokio::time::timeout(HANDSHAKE_TIMEOUT, framed.next()).await;
    let hello = match hello_result {
        Ok(Some(Ok(msg))) if msg.msg_type == MessageType::Hello => {
            match msg.decode_payload::<HelloMsg>() {
                Some(h) => h,
                None => {
                    log::warn!("Invalid Hello payload");
                    return;
                }
            }
        }
        Ok(_) => {
            log::warn!("Expected Hello message, disconnecting");
            return;
        }
        Err(_) => {
            log::warn!("Handshake timeout, disconnecting");
            return;
        }
    };

    // Validate protocol version
    if hello.protocol_version != PROTOCOL_VERSION {
        let reject = WelcomeMsg::Rejected {
            reason: format!(
                "Protocol version mismatch: client={}, server={}",
                hello.protocol_version, PROTOCOL_VERSION
            ),
        };
        let msg = MuxMessage::control(MessageType::Welcome, 0, &reject);
        let _ = framed.send(msg).await;
        return;
    }

    // Subscribe to notify_tx before building Welcome so any RenameWindow
    // broadcast emitted between snapshot construction and message-loop entry
    // is captured rather than lost.
    let mut notify_rx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().subscribe()
    };

    // Send Welcome with session list, auto-creating default session if none exist
    let welcome = {
        let mut mgr = session_manager.lock().await;
        if mgr.is_empty() {
            mgr.create_session("default".to_string());
        }
        WelcomeMsg::Accepted {
            server_version: PROTOCOL_VERSION,
            sessions: mgr.session_list(),
        }
    };
    let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
    if framed.send(msg).await.is_err() {
        return;
    }

    log::info!(
        "Client connected: {:?}, protocol v{}",
        hello.client_type,
        hello.protocol_version
    );

    // CLI clients: serve session list + optionally process one control message.
    // Skip reattach and full message loop to avoid stealing panes from GUI.
    if hello.client_type == ClientType::Cli {
        handle_cli_client(
            &mut framed,
            &session_manager,
            &shutdown_tx,
            &daemon_title_tx,
            &daemon_notification_tx,
            &daemon_agent_status_tx,
            &daemon_pane_exit_sender,
            &upgrade_tx,
        )
        .await;
        return;
    }

    // Determine active session: use first session (auto-created "default")
    let mut active_session_id: u32 = {
        let mgr = session_manager.lock().await;
        let id = mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1);
        id
    };

    // Shared channel: all pane reader threads send output here (via
    // blocking_send on their own native OS thread — never a self-block risk
    // for THIS task), and the select! loop below forwards it to the client.
    // `route_message`'s `RequestPaneSnapshot` / `SetVisibility` arms can also
    // enqueue a chunk onto this SAME channel from within this connection's
    // own task; see the audit note on `route_message` below for why neither
    // of those enqueues is allowed to be a blocking
    // `pane_output_tx.send(...).await` / `.reserve().await` (mux-window-
    // switch-output-hang task0001).
    let (pane_output_tx, mut pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    // Reuse the daemon-level title sender so OSC title updates flow to the
    // daemon task regardless of connection lifetime. GUI delivery of
    // RenameWindow happens via notify_rx, which is populated by the daemon
    // task when it updates window.name.
    let title_tx = daemon_title_tx;
    // Daemon-lifetime notification sender: panes created on this connection
    // forward Detached-pane OSC 9 notifications through it; the daemon
    // notification task relays them to the GUI client (FR2).
    let notification_tx = daemon_notification_tx;
    // Daemon-lifetime agent-status report sender: panes created on this
    // connection forward raw agent-status OSC payloads through it,
    // regardless of attach state (SPEC FR3).
    let agent_status_tx = daemon_agent_status_tx;
    // Daemon-lifetime pane-exit sender: panes created on this connection emit
    // their pane_id here on PTY EOF (regardless of attach state) so the daemon
    // reap task can reap them authoritatively (FR1/FR2). Fixed at pane
    // creation; never swapped on detach.
    let pane_exit_sender = daemon_pane_exit_sender;

    // NOTE: Reattach data is NOT sent here. The client must send an Attach
    // message after its output stream is ready. This eliminates the timing
    // dependency where reattach data could arrive before the client is listening.

    // Kick channel: set by handle_attach. Fires with Ok(()) when another
    // client attaches to the same session and evicts us. Drop-without-send
    // (Err) is treated as a no-op so that cleanly switching sessions does
    // not kick ourselves off.
    let mut kick_rx: Option<oneshot::Receiver<()>> = None;
    let mut was_kicked = false;

    // Per-connection effective-visible state (FR3, FR7). Initially true so
    // newly-attached clients receive PTY output immediately. Updated by
    // SetVisibility messages and consulted on reattach (collect_reattach_data
    // re-evaluates output_target after a session switch).
    let visible_state: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));

    // Connection-owned, explicitly-bounded backlog of chunks / visibility
    // resumes deferred while `pane_output_tx` is momentarily full
    // (mux-window-switch-output-hang task0002 rework — see
    // `DeferredOutputQueue`'s doc). Flushed via `flush_deferred_output`
    // immediately after the loop's own drain of `pane_output_rx` below (the
    // only place capacity on that channel is ever freed) and, defensively,
    // at the top of `route_message` so a newly-arrived client message also
    // gives the queue a chance to progress.
    let mut deferred_output = DeferredOutputQueue::new();

    // Fair-reservation state for `deferred_output` (mux-window-switch-output-
    // hang task0003 rework, AC-3/G2) — see `arm_pending_deferred_reserve`'s
    // doc for why this exists alongside `flush_deferred_output`'s ordinary
    // try-based retries rather than replacing them.
    let mut pending_deferred_reserve: Option<PendingDeferredReserve> = None;

    // Starvation-guard counter for the client-message arm (G2 rework,
    // AC-2) — see `CLIENT_MSG_STARVATION_QUOTA`'s doc and the
    // design-decision comment at `msg = client_reader.next()`'s guard
    // below.
    let mut consecutive_client_msgs_while_deferred: u32 = 0;

    // task0001 (FR1/FR3): from here on this connection NEVER touches the
    // socket's write side directly again — every client-bound frame
    // instead funnels through `outbound_tx`, a bounded admission queue
    // drained independently by a dedicated writer task
    // (`run_outbound_writer`). This is what lets the client-message arm
    // below keep being polled even while the socket's send buffer is
    // saturated — the residual freeze this task closes (Design "Problem
    // (current shape)").
    //
    // Splitting via `Framed::into_parts()` (not `Framed::into_inner()`)
    // preserves any bytes already read into `framed`'s internal buffer but
    // not yet decoded into a full message, and any bytes fed but not yet
    // flushed — `into_inner()` would silently drop them, corrupting the
    // read/write stream if e.g. the client's Hello and a following message
    // arrived in the same read.
    let parts = framed.into_parts();
    let (read_half, write_half) = tokio::io::split(parts.io);
    let mut client_reader = FramedRead::new(read_half, MuxCodec::new());
    *client_reader.read_buffer_mut() = parts.read_buf;
    let mut writer_sink = FramedWrite::new(write_half, MuxCodec::new());
    *writer_sink.write_buffer_mut() = parts.write_buf;

    let (outbound_tx, outbound_rx) = mpsc::channel::<MuxMessage>(OUTBOUND_QUEUE_CAPACITY);
    let mut writer_task_handle = tokio::spawn(run_outbound_writer(
        writer_sink,
        outbound_rx,
        upgrade_ack_slot,
    ));
    let mut writer_task_failed = false;

    // Drain-arm outbound state (FR1/FR4): frames the drain arm consumed
    // from `pane_output_rx` but could not admit into `outbound_tx` without
    // blocking. Bounded by construction — the drain arm is gated off (see
    // the `chunk = pane_output_rx.recv()` arm's guard below) while this is
    // non-empty, so it can never grow past one drain batch's worth
    // (`DRAIN_BATCH_LIMIT`, ~4MB — see `OUTBOUND_QUEUE_CAPACITY`'s doc for
    // the combined worst-case accounting).
    let mut outbound_remainder: VecDeque<MuxMessage> = VecDeque::new();
    let mut pending_outbound_reserve: Option<PendingOutboundReserve> = None;

    // Message + output loop using select! to handle both directions concurrently
    loop {
        // Kick future: resolves when our kick_rx fires. `Ok(())` means
        // another client attached to this session and evicted us. `Err(_)`
        // means the Sender was dropped — in practice this occurs when the
        // active session is destroyed while we're still attached (the
        // session's `active_client_kick` drops along with the MuxSession).
        // Both cases are terminal for this connection; `None` stays pending.
        let kick_fut = async {
            match kick_rx.as_mut() {
                Some(rx) => rx.await,
                None => {
                    std::future::pending::<Result<(), tokio::sync::oneshot::error::RecvError>>()
                        .await
                }
            }
        };

        // G2 rework (AC-2, review round 3 findings `dd23cfc388062939` /
        // `5c01ffb8d53dc9f7`) — starvation guard computed BEFORE the
        // `select!` so both the client-message arm's guard and the
        // post-`select!` bookkeeping below see the SAME snapshot of
        // "was there deferred work outstanding going into this
        // iteration". See the design-decision doc at the client-message
        // arm's own guard for the full reasoning.
        let has_deferred_work = pending_deferred_reserve.is_some() || !deferred_output.is_empty();
        // G4 rework (AC-4, review round 4 finding `e6ac2a334424ebd7`):
        // extend the raw admission-only signal above with the receiver's
        // own channel backlog — see `has_unforwarded_pane_output`'s doc for
        // why admission alone is not enough. This is the value actually fed
        // to the guard and to the post-`select!` bookkeeping below.
        //
        // task0001 (Design "Starvation-guard interaction"): a held outbound
        // remainder or an armed outbound-capacity reservation is likewise
        // outstanding output work — the SAME class of "admitted but not yet
        // delivered" gap `has_unforwarded_pane_output` already closes for
        // `pane_output_tx`, now extended to the outbound socket queue this
        // task adds, so continuous client traffic cannot starve draining
        // the outbound remainder either.
        let has_pending_output = has_unforwarded_pane_output(has_deferred_work, &pane_output_rx)
            || !outbound_remainder.is_empty()
            || pending_outbound_reserve.is_some();
        // Review round (finding `c1-outbound-remainder-starvation`): a held
        // `outbound_remainder` must NOT unconditionally keep the client arm
        // enabled — doing so defeated the starvation guard below for as
        // long as the remainder was outstanding (which, per
        // `OUTBOUND_QUEUE_CAPACITY`, is routine under multi-pane output),
        // silently starving PTY output for the entire span. The earlier
        // concern this short-circuit was guarding against — "excluding the
        // client arm on a quota-exhausted iteration could leave `select!`
        // with no arm capable of becoming ready" — is now moot: the
        // `outbound_permit_result` arm is listed BEFORE this one (see its
        // own placement doc just below), so under `biased` it is polled,
        // and therefore registered as a semaphore waiter, on every single
        // iteration regardless of whether the client arm is enabled. The
        // guard below is the ordinary one; no remainder-specific override
        // is needed.
        let allow_client_arm =
            allow_client_message_arm(has_pending_output, consecutive_client_msgs_while_deferred);
        let mut took_client_arm = false;

        tokio::select! {
            // biased: text order below IS the priority order, subject to
            // the starvation guard on the very first arm (`allow_client_arm`,
            // computed just above). See that arm's own doc for the full G2
            // design decision.
            biased;

            // Review round (finding `c1-outbound-remainder-starvation`):
            // listed FIRST, ahead of the client-message arm below, so it is
            // polled — and therefore registered as a waiter on
            // `outbound_tx`'s semaphore — on every single loop iteration,
            // independent of whatever `allow_client_arm` decides for the
            // client arm. Mirrors the `permit_result` arm's own placement
            // requirement relative to the drain arm (see that arm's doc
            // below), applied one layer further out: a `try_send`-only
            // retry can never win fairly against a reader thread's
            // `blocking_send` waiter already parked on the channel, so this
            // fair `reserve_owned()` future is the only thing that can ever
            // drain `outbound_remainder`, and it must be polled unconditionally
            // for that guarantee to hold. Costs nothing when not armed
            // (`if pending_outbound_reserve.is_some()` short-circuits to
            // `Pending` instantly) or when armed but not yet resolved (ditto)
            // — the client arm and every arm below still get serviced this
            // same iteration.
            outbound_permit_result = async {
                pending_outbound_reserve.as_mut().unwrap().await
            }, if pending_outbound_reserve.is_some() => {
                pending_outbound_reserve = None;
                match outbound_permit_result {
                    Ok(permit) => {
                        if let Some(frame) = outbound_remainder.pop_front() {
                            let _ = permit.send(frame);
                        } else {
                            drop(permit);
                        }
                    }
                    Err(_) => {
                        log::warn!(
                            "outbound queue closed while an outbound-remainder reservation \
                             was pending; dropping the remaining outbound backlog (the \
                             writer's own completion is what actually tears this \
                             connection down, AC-6)"
                        );
                        outbound_remainder.clear();
                    }
                }
                // More frames may remain (only one is applied per resolved
                // reservation) — re-arm immediately, mirroring
                // `arm_pending_deferred_reserve`'s own re-arm.
                arm_pending_outbound_reserve(
                    &mut pending_outbound_reserve,
                    &outbound_remainder,
                    &outbound_tx,
                );
            }

            // G2 rework (AC-2, review round 3 findings `dd23cfc388062939` /
            // `5c01ffb8d53dc9f7`) — DESIGN DECISION: which invariant wins.
            //
            // task0003 established the fair `reserve_owned()` arm further
            // below (`permit_result`) must be listed BEFORE the drain arm
            // so it can ever be polled under saturation. Round 3 found
            // THIS arm has the identical structural problem one level
            // further out: under `biased`, a client that keeps
            // `client_reader.next()` continuously ready (buffered messages
            // arriving faster than one per loop iteration) wins EVERY
            // iteration, so every arm below it — the fair reservation AND
            // the plain drain arm — is never even POLLED, regardless of
            // what work either could otherwise make progress on. Left
            // unaddressed, that silently reintroduces the exact "output
            // stops flowing" bug this whole feature exists to close, one
            // layer further out than task0003's fix reached.
            //
            // Neither documented invariant — "prioritize client messages
            // for input latency" vs. "deferred output must make bounded
            // progress" — can simply win outright: always prioritizing
            // input reopens the starvation this arm's guard exists to
            // close; always prioritizing output turns every pending
            // deferral into extra input latency for every OTHER keystroke
            // too, not just the one that triggered it. Resolution: a
            // bounded round-robin (quota), not an unconditional priority.
            // Input latency wins for up to `CLIENT_MSG_STARVATION_QUOTA`
            // consecutive iterations; once that many have run back-to-back
            // WHILE there is pending output outstanding (`has_pending_output`,
            // computed above this `select!` via `has_unforwarded_pane_output`
            // — task0005 rework, G4/AC-4: broadened from the connection's own
            // deferred-output bookkeeping to also cover the receiver-side
            // channel backlog, so an item already admitted into
            // `pane_output_tx` but not yet drained still counts, see that
            // function's own doc), this arm is excluded for exactly one
            // iteration via the `allow_client_arm` guard — forcing `select!`
            // past it to whichever of the reservation/drain arms actually has
            // work, since by construction at least one of them is guaranteed
            // ready or about to be (a `try_send`/`try_reserve` only defers
            // when the channel was observed full, and a non-empty receiver
            // backlog makes the drain arm itself immediately ready). The very
            // next iteration reverts to prioritizing client messages: the
            // counter is reset to 0 the moment ANY other arm wins (see the
            // bookkeeping right after this `select!` block), so the
            // one-iteration penalty never compounds and ordinary traffic (no
            // pending output) sees ZERO behavior change — `allow_client_arm`
            // is unconditionally `true` whenever `has_pending_output` is
            // `false`. Net effect: pending output is guaranteed at least one
            // poll at least once every `CLIENT_MSG_STARVATION_QUOTA + 1`
            // iterations — a bounded number, as AC-2/AC-4 require.
            msg = client_reader.next(), if allow_client_arm => {
                took_client_arm = true;
                match msg {
                    Some(Ok(msg)) => {
                        if let Err(should_break) = route_message(
                            msg,
                            &session_manager,
                            &outbound_tx,
                            &pane_output_tx,
                            &mut active_session_id,
                            &shutdown_tx,
                            &title_tx,
                            &notification_tx,
                            &agent_status_tx,
                            &pane_exit_sender,
                            &mut kick_rx,
                            &visible_state,
                            &upgrade_tx,
                            &mut deferred_output,
                        ).await {
                            if should_break {
                                break;
                            }
                        }
                        // route_message's own defensive flush (or a handler
                        // it called, e.g. RequestPaneSnapshot/SetVisibility)
                        // may have left `deferred_output` non-empty against a
                        // still-full channel — arm the fair reservation so
                        // that work cannot be starved (AC-3/G2).
                        arm_pending_deferred_reserve(
                            &mut pending_deferred_reserve,
                            &deferred_output,
                            &pane_output_tx,
                        );
                    }
                    Some(Err(e)) => {
                        log::warn!("Connection error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            kick_result = kick_fut => {
                let reason = match kick_result {
                    Ok(()) => "evicted by newer client",
                    Err(_) => "active session destroyed",
                };
                log::info!(
                    "Client disconnecting ({}) from session {}; sending Detached",
                    reason, active_session_id
                );
                was_kicked = true;
                let resp = MuxMessage::control(MessageType::Detached, 0, &());
                // task0001: admission only (best-effort, mirrors the
                // pre-task0001 `let _ = framed.send(...).await;` shape —
                // errors ignored either way). This is the last thing this
                // arm does before `break`, so awaiting admission here does
                // not reintroduce the residual freeze (no other arm needs
                // to run concurrently at this exact point); the teardown
                // code after the loop gives whatever gets admitted here a
                // bounded, best-effort flush (Design invariant 7).
                let _ = outbound_tx.send(resp).await;
                break;
            }
            // task0001 (AC-6): the outbound writer only ever completes
            // DURING an active loop iteration if it hit a socket
            // write/flush failure (its graceful-shutdown exit only happens
            // after THIS loop has already broken and dropped `outbound_tx`
            // — see the teardown code after the loop) — so any completion
            // observed here is fatal, mirroring the pre-task0001
            // `send_err`/`flush_failed` break in the drain arm, just moved
            // to where the failure is actually detected now.
            writer_result = &mut writer_task_handle => {
                writer_task_failed = true;
                match writer_result {
                    Ok(()) => log::warn!(
                        "mux outbound writer exited during an active connection (socket \
                         write/flush failure); tearing the connection down"
                    ),
                    Err(join_err) => {
                        log::warn!("mux outbound writer task panicked: {}", join_err);
                    }
                }
                break;
            }
            // task0003 (AC-3/G2): polled only while a fair reservation is in
            // flight (the `if` guard is re-checked every loop iteration).
            // MUST be listed before the `chunk = pane_output_rx.recv()` arm
            // below: under `biased`, `select!` polls branches in TEXT ORDER
            // and stops at the first one that resolves — with the reader
            // thread(s) saturating the channel, `pane_output_rx.recv()` is
            // essentially ALWAYS ready, so if it were listed first this arm
            // would never even get POLLED (never mind resolve), and could
            // therefore never register itself as a waiter on
            // `pane_output_tx`'s semaphore in the first place. Being polled
            // first here costs nothing when not armed (`Pending` is instant)
            // and when armed but not yet resolved (ditto) — this connection
            // keeps servicing every other arm (client messages, the kick
            // signal, the drain arm below) while the reservation is still
            // pending; a `select!` arm's future is only ever POLLED, never
            // awaited to completion outside the macro.
            permit_result = async {
                pending_deferred_reserve.as_mut().unwrap().await
            }, if pending_deferred_reserve.is_some() => {
                pending_deferred_reserve = None;
                match permit_result {
                    Ok(permit) => {
                        apply_fair_permit_to_front_deferred_item(
                            &mut deferred_output,
                            permit,
                            &pane_output_tx,
                            &session_manager,
                            active_session_id,
                            &visible_state,
                        )
                        .await;
                    }
                    Err(_) => {
                        log::warn!(
                            "pane_output_tx closed while a fair deferred-item reservation \
                             was pending; dropping the remaining deferred backlog"
                        );
                        deferred_output.clear();
                    }
                }
                // More work may remain (only one item is applied per
                // resolved reservation) — re-arm immediately rather than
                // waiting for the next drain/message to do it.
                arm_pending_deferred_reserve(
                    &mut pending_deferred_reserve,
                    &deferred_output,
                    &pane_output_tx,
                );
            }
            // task0001 (FR1/FR5): arm gated off while an outbound remainder
            // is held ("arm fires only when the loop is not already
            // holding an unsent remainder" — Design "Reworked drain-arm
            // flow" step 1) — the `outbound_permit_result` arm above is
            // what drains that remainder; this arm resumes once it is
            // empty again.
            chunk = pane_output_rx.recv(), if outbound_remainder.is_empty() => {
                if let Some(first) = chunk {
                    let batch_start = std::time::Instant::now();

                    // Drain: collect all pending chunks non-blocking (up to limit)
                    let mut chunks = vec![first];
                    while chunks.len() < DRAIN_BATCH_LIMIT {
                        match pane_output_rx.try_recv() {
                            Ok(c) => chunks.push(c),
                            Err(_) => break,
                        }
                    }
                    let drained_count = chunks.len();

                    // Merge consecutive same-pane chunks to reduce IPC frames
                    let merged = merge_consecutive_chunks(chunks);
                    let merged_count = merged.len();
                    let total_bytes: usize = merged.iter().map(|c| c.data.len()).sum();

                    if drained_count >= DRAIN_BATCH_LIMIT {
                        log::warn!(
                            "pty-batch-full: drained={} (limit hit) | merged={} | {}bytes",
                            drained_count, merged_count, total_bytes
                        );
                    } else if drained_count > 1 {
                        log::info!(
                            "pty-batch: drained={} | merged={} | {}bytes",
                            drained_count, merged_count, total_bytes
                        );
                    }

                    // Classify each merged chunk into its wire frame (FR1)
                    // and reap any PTY-exit chunk IMMEDIATELY, at
                    // consumption time — task0001 AC-4/invariant 5: the
                    // reap must not wait on the frame's eventual
                    // admission/delivery outcome, since delivery is no
                    // longer synchronously observable in this arm (the
                    // writer task owns that now). This preserves "reap
                    // regardless of delivery success" (see the original
                    // rationale below, now reworded for the admission
                    // step) by moving the checkpoint earlier: consumption,
                    // not a send attempt, is the only synchronous point
                    // this arm still has.
                    //
                    // FR1: each merged chunk is encoded according to its
                    // `ChunkKind` discriminator. `ChunkKind::Snapshot` chunks
                    // (produced by `handle_request_pane_snapshot`) ship as
                    // `MessageType::Snapshot` so the client routes them through
                    // `apply_mux_message::Snapshot|SnapshotRestore` and the
                    // `build_from_snapshot` + `scrollback_bypass` fast path.
                    // `ChunkKind::PtyOutput` (the default for the reader thread,
                    // resume path, and PTY exit signal) ships as
                    // `MessageType::PtyOutput`. Empty-data PtyOutput chunks
                    // still mean "PTY exited" (the merge layer never folds
                    // empty chunks, and the snapshot path never emits empty).
                    let mut frames: Vec<MuxMessage> = Vec::with_capacity(merged_count);
                    let mut exited_panes: Vec<u32> = Vec::new();
                    for chunk in merged {
                        let msg = match chunk.kind {
                            ChunkKind::Snapshot => {
                                MuxMessage::snapshot(chunk.pane_id, chunk.data)
                            }
                            ChunkKind::PtyOutput => {
                                if chunk.data.is_empty() {
                                    log::info!("PTY exited for pane {}", chunk.pane_id);
                                    exited_panes.push(chunk.pane_id);
                                    let exit_msg = PtyExitedMsg { exit_code: Some(0) };
                                    MuxMessage::control(
                                        MessageType::PtyExited,
                                        chunk.pane_id,
                                        &exit_msg,
                                    )
                                } else {
                                    MuxMessage::pty_output(chunk.pane_id, chunk.data)
                                }
                            }
                        };
                        frames.push(msg);
                    }
                    // Reap each exited pane from the daemon's own SessionManager
                    // *regardless* of whether delivery to the client succeeds:
                    // the PTY genuinely exited, so the empty window / session must
                    // be removed and the daemon shut down once the last pane is
                    // gone. Gating this on a successful flush would re-open the
                    // zombie-pane bug under a client-drop race — the GUI window
                    // closing at the same moment the last shell exits via Ctrl+D
                    // both delivers the exit chunk and fails the client write, so
                    // a success-gated reap would skip cleanup and leave a session
                    // that never auto-shuts-down / can't be `mux kill`ed. Mirror
                    // the explicit `DestroyPane` cleanup fully.
                    for pane_id in exited_panes {
                        handle_destroy_pane(pane_id, &session_manager, &shutdown_tx).await;
                    }

                    // Admission (FR1, invariant 1): non-blocking. Admits as
                    // many frames as the outbound queue currently has room
                    // for, in order; a non-empty remainder is held as loop
                    // state and the fair capacity-acquisition future is
                    // armed so it is serviced ahead of any newer chunk
                    // (FR3) — this arm never awaits outbound capacity at
                    // this point position, which is exactly the self-block
                    // this task removes.
                    let remainder = try_admit_outbound_frames(&outbound_tx, frames);
                    if !remainder.is_empty() {
                        outbound_remainder = remainder;
                        arm_pending_outbound_reserve(
                            &mut pending_outbound_reserve,
                            &outbound_remainder,
                            &outbound_tx,
                        );
                    }

                    let elapsed = batch_start.elapsed();
                    if elapsed.as_millis() > 50 {
                        log::warn!(
                            "slow-pty-batch: {}ms | drained={} merged={} | {}bytes",
                            elapsed.as_millis(), drained_count, merged_count, total_bytes
                        );
                    }

                    // mux-window-switch-output-hang task0002: this drain is
                    // the ONLY place capacity on `pane_output_tx` is ever
                    // freed, so it is also the right place to retry anything
                    // deferred while the channel was momentarily full (see
                    // `DeferredOutputQueue`'s doc for why this beats a
                    // spawned task per deferral). Unchanged trigger point
                    // (task0001 Design invariant 3): runs regardless of
                    // whether the outbound admission above left a
                    // remainder — this is about `pane_output_tx` capacity,
                    // which this arm just freed by consuming, not about
                    // outbound-socket capacity.
                    flush_deferred_output(
                        &mut deferred_output,
                        &pane_output_tx,
                        &session_manager,
                        active_session_id,
                        &visible_state,
                    )
                    .await;
                    // task0003 (AC-3/G2): if the ordinary try-based flush
                    // above still leaves work queued, the channel is likely
                    // saturated by a PTY reader thread's own `blocking_send`
                    // waiters, which a `try_send`/`try_reserve` retry can
                    // never win fairly — arm the fair reservation instead.
                    arm_pending_deferred_reserve(
                        &mut pending_deferred_reserve,
                        &deferred_output,
                        &pane_output_tx,
                    );
                }
            }
            notification = notify_rx.recv() => {
                match notification {
                    Ok(msg) => {
                        // Forward cross-client notification (e.g., CLI SwitchWindow) to GUI
                        log::info!("Forwarding notification to GUI: {:?} pane={}", msg.msg_type, msg.pane_id);
                        // task0001: admission only (Design invariant 1 —
                        // "MAY await admission", not the drain arm). The
                        // Upgrading-ack fire (AC-7 / NFR4) now happens
                        // inside the writer task itself, once the frame is
                        // actually written AND flushed — see
                        // `outbound::run_outbound_writer`'s doc — rather
                        // than here at admission time.
                        if outbound_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        log::warn!(
                            "notify_rx lagged: {} notifications dropped; resyncing window names from session_list",
                            skipped
                        );
                        let list = session_manager.lock().await.session_list();
                        for sess in &list {
                            for win in &sess.windows {
                                let payload = RenameWindowMsg { name: win.name.clone() };
                                let msg = MuxMessage::control(
                                    MessageType::RenameWindow,
                                    win.id,
                                    &payload,
                                );
                                if outbound_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log::warn!("notify_rx closed; exiting connection loop");
                        break;
                    }
                }
            }
        }

        // G2 rework (AC-2): update the starvation-guard counter for the
        // NEXT iteration. Only "the client arm ran while deferred work was
        // already outstanding going into THIS iteration" counts against
        // the quota; every other outcome — a different arm ran, or there
        // was no deferred work to begin with — resets it, so the
        // one-iteration penalty above never compounds and ordinary
        // traffic is never penalized once the backlog clears. Extracted
        // into `next_client_msg_starvation_count` (medium finding
        // connection.rs:820, review round 4) so the increment/reset
        // arithmetic is unit-testable deterministically, mirroring
        // `allow_client_message_arm`'s own extraction rationale.
        consecutive_client_msgs_while_deferred = next_client_msg_starvation_count(
            took_client_arm,
            has_pending_output,
            consecutive_client_msgs_while_deferred,
        );
    }

    // Switch all panes in the active session to detached buffering mode.
    // This prevents pty_reader_loop from racing with the next connection's
    // collect_reattach_data when the output_target is still Connected(dead_tx).
    //
    // Skipped when we were kicked by another attaching client: in that case
    // the newer client has already taken ownership of the panes, and running
    // detach_session_panes would immediately clobber their Connected state
    // back to Detached, stranding them.
    if was_kicked {
        log::info!(
            "Client disconnecting (kicked), leaving session {} panes attached to new client",
            active_session_id
        );
    } else {
        log::info!(
            "Client disconnecting, detaching panes for session {}",
            active_session_id
        );
        // Identity-scoped: detach only panes still owned by our pane_output_tx.
        // Belt-and-suspenders with `was_kicked`: protects against races where
        // client_reader.next() wins over kick_fut in the select!, or where
        // the socket fails mid-eviction and we exit without observing the
        // kick.
        detach_session_panes(&session_manager, active_session_id, &pane_output_tx).await;

        log::info!(
            "Client disconnected, session {} panes detached",
            active_session_id
        );
    }

    // task0001 (Design invariant 7): a socket write/flush failure inside
    // the writer already tore this loop down via the `writer_result` arm
    // above (AC-6) — the writer has already exited, so there is nothing
    // further to flush. Otherwise, drop every sender THIS loop itself
    // still holds — the plain `outbound_tx` and any in-flight
    // outbound-remainder reservation (which owns its own internal clone;
    // clearing `pending_outbound_reserve` drops it) — so
    // `run_outbound_writer`'s `outbound_rx.recv()` naturally drains
    // whatever was ALREADY admitted (FIFO, one final pass) and then
    // observes the channel closed and exits gracefully. A bounded wait on
    // its `JoinHandle` gives that best-effort final flush a chance to land
    // before this connection fully tears down. Frames still sitting in
    // `outbound_remainder` (never admitted at all) are NOT covered by this
    // guarantee — see Design invariant 7's "already-admitted frames"
    // wording.
    if !writer_task_failed {
        drop(pending_outbound_reserve.take());
        drop(outbound_tx);
        match tokio::time::timeout(OUTBOUND_TEARDOWN_FLUSH_TIMEOUT, writer_task_handle).await {
            Ok(Ok(())) => {}
            Ok(Err(join_err)) => {
                log::warn!(
                    "mux outbound writer task panicked during teardown: {}",
                    join_err
                );
            }
            Err(_) => {
                log::warn!(
                    "mux outbound writer teardown flush timed out after {:?}; abandoning \
                     any remaining queued output",
                    OUTBOUND_TEARDOWN_FLUSH_TIMEOUT
                );
            }
        }
    }
}

/// Handle a CLI client after handshake.
///
/// Reads at most one control message (e.g., CreateWindow), processes it,
/// sends a response, and disconnects. If no message arrives within 5 seconds,
/// disconnects gracefully (this is the normal `mux ls` path).
#[allow(clippy::too_many_arguments)]
async fn handle_cli_client<S>(
    framed: &mut Framed<S, MuxCodec>,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    daemon_title_tx: &TitleChangeSender,
    daemon_notification_tx: &NotificationSender,
    daemon_agent_status_tx: &AgentStatusReportSender,
    daemon_pane_exit_sender: &SharedPaneExitSender,
    upgrade_tx: &UpgradeSignalSender,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // Wait for one optional control message with timeout
    let msg_result = tokio::time::timeout(Duration::from_secs(5), framed.next()).await;

    let msg = match msg_result {
        Ok(Some(Ok(msg))) => msg,
        Ok(Some(Err(e))) => {
            log::warn!("CLI client read error: {}", e);
            return;
        }
        Ok(None) | Err(_) => {
            // Connection closed or timeout - normal for ls/kill commands
            log::info!("CLI client served (no control message), disconnecting");
            return;
        }
    };

    log::info!("CLI client control message: {:?}", msg.msg_type);

    // Determine active session for the control message
    let active_session_id = {
        let mgr = session_manager.lock().await;
        let id = mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1);
        id
    };

    // Create a temporary pane output channel (CLI doesn't stream PTY output)
    let (pane_output_tx, _pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    match msg.msg_type {
        MessageType::CreateWindow => {
            let _ = handle_create_window(
                &msg,
                session_manager,
                framed,
                &pane_output_tx,
                active_session_id,
                daemon_title_tx,
                daemon_notification_tx,
                daemon_agent_status_tx,
                daemon_pane_exit_sender,
            )
            .await;

            // Log the CLI-initiated window creation
            log_cli_window_creation(session_manager, active_session_id).await;
        }
        MessageType::SwitchWindow => {
            let target_id = msg.pane_id;
            handle_switch_window(target_id, session_manager).await;
            // Broadcast to GUI clients so they switch windows too.
            // Resolve the active pane_id of the target window for the GUI.
            let notify_pane_id = {
                let mgr = session_manager.lock().await;
                // Try as pane_id first, then as window_id (same logic as handle_switch_window)
                if let Some((sid, wid)) = mgr.find_pane(target_id) {
                    mgr.get_session(sid)
                        .and_then(|s| s.windows.get(&wid))
                        .and_then(|w| w.active_pane_id)
                        .unwrap_or(target_id)
                } else if let Some(sid) = mgr.find_window_session(target_id) {
                    mgr.get_session(sid)
                        .and_then(|s| s.windows.get(&target_id))
                        .and_then(|w| w.active_pane_id)
                        .unwrap_or(target_id)
                } else {
                    target_id
                }
            };
            let notify_msg = MuxMessage {
                msg_type: MessageType::SwitchWindow,
                pane_id: notify_pane_id,
                payload: vec![],
            };
            let mgr = session_manager.lock().await;
            let _ = mgr.notify_tx().send(notify_msg);
        }
        MessageType::PtyInput => {
            let pane_id = msg.pane_id;
            let mgr = session_manager.lock().await;
            if let Some((session_id, window_id)) = mgr.find_pane(pane_id) {
                if let Some(session) = mgr.get_session(session_id) {
                    if let Some(window) = session.windows.get(&window_id) {
                        if let Some(pane) = window.panes.get(&pane_id) {
                            if let Err(e) = pane.write_input(&msg.payload) {
                                log::warn!(
                                    "CLI send-keys: failed to write to pane {}: {}",
                                    pane_id,
                                    e
                                );
                            }
                        }
                    }
                }
            } else {
                log::warn!("CLI send-keys: pane {} not found", pane_id);
            }
        }
        MessageType::ReadPane => match handle_read_pane(&msg, session_manager).await {
            Ok(result) => {
                let resp = MuxMessage::control(MessageType::ReadPaneResult, 0, &result);
                let _ = framed.send(resp).await;
            }
            Err(err) => {
                let resp = MuxMessage::control(MessageType::AgentApiError, 0, &err);
                let _ = framed.send(resp).await;
            }
        },
        MessageType::SendText => match handle_send_text(&msg, session_manager).await {
            Ok(result) => {
                let resp = MuxMessage::control(MessageType::SendTextResult, 0, &result);
                let _ = framed.send(resp).await;
            }
            Err(err) => {
                let resp = MuxMessage::control(MessageType::AgentApiError, 0, &err);
                let _ = framed.send(resp).await;
            }
        },
        MessageType::WaitAgentState => {
            // May block (server-side, bounded by the request's own
            // `timeout_ms`) awaiting a qualifying state change — safe here
            // because each connection runs in its own spawned task
            // (`daemon::run_daemon`'s accept loop), so this does not stall
            // other clients.
            match handle_wait_agent_state(&msg, session_manager).await {
                Ok(result) => {
                    let resp = MuxMessage::control(MessageType::WaitAgentStateResult, 0, &result);
                    let _ = framed.send(resp).await;
                }
                Err(err) => {
                    let resp = MuxMessage::control(MessageType::AgentApiError, 0, &err);
                    let _ = framed.send(resp).await;
                }
            }
        }
        MessageType::Shutdown => {
            log::info!("CLI client requested daemon shutdown");
            let _ = shutdown_tx.send(true);
        }
        MessageType::Upgrade => {
            log::info!("CLI client requested mux daemon upgrade");
            handle_upgrade_request(framed, upgrade_tx).await;
        }
        _ => {
            log::warn!(
                "CLI client sent unsupported message type: {:?}",
                msg.msg_type
            );
            let err = ErrorMsg {
                message: "Unsupported CLI control message".to_string(),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
        }
    }

    log::info!("CLI client control message processed, disconnecting");
}

/// Convert the accept loop's reply to a `MessageType::Upgrade` request into
/// the message (if any) this connection should send back to the client.
/// `None` means "no explicit reply" (successful preparation -- the
/// connection is simply dropped once the process is replaced,
/// IMPLEMENTATION.md D2). `Some` carries an `Error` control message: either
/// the abort reason reported by the accept loop (AC-4), or a generic
/// message when the reply channel closed without an answer (the accept
/// loop dropped it, e.g. mid-shutdown).
///
/// Extracted as a pure function so the CLI-client wiring's translation
/// logic is unit-testable without a live connection or a real accept loop.
fn upgrade_reply_to_message(
    result: Result<Result<(), String>, oneshot::error::RecvError>,
) -> Option<MuxMessage> {
    let reason = match result {
        Ok(Ok(())) => return None,
        Ok(Err(reason)) => reason,
        Err(_) => "mux daemon closed the upgrade request unexpectedly".to_string(),
    };
    let err = ErrorMsg { message: reason };
    Some(MuxMessage::control(MessageType::Error, 0, &err))
}

/// Handle a `MessageType::Upgrade` request on `framed`: signal the accept
/// loop, wait (bounded) for its reply, and send back whatever
/// [`upgrade_reply_to_message`] says (an `Error` frame on abort/timeout, or
/// nothing at all on success — the connection is simply dropped once the
/// process is replaced, IMPLEMENTATION.md D2).
///
/// Shared by BOTH connection kinds that may request an upgrade: a CLI
/// connection (`handle_cli_client`, the real `emterm mux upgrade`
/// subcommand's path) and a persistent GUI connection's message loop
/// (`route_message`) — FR1's wire message is not scoped to one client type,
/// and duplicating this logic per call site is exactly the kind of drift
/// this task exists to close (task0009 rework).
///
/// `reply` is generic over [`ReplySink`] (task0001), exactly as
/// `handlers::handle_create_window` is: the CLI-client path passes its
/// still-undivided `Framed` sink, the GUI loop (`route_message`) passes an
/// [`OutboundHandle`] wrapping the outbound admission queue.
async fn handle_upgrade_request<R: ReplySink>(reply: &mut R, upgrade_tx: &UpgradeSignalSender) {
    let (reply_tx, reply_rx) = oneshot::channel();
    if upgrade_tx
        .send(UpgradeSignal { reply: reply_tx })
        .await
        .is_err()
    {
        log::warn!("Upgrade request dropped: accept loop unavailable");
        let err = ErrorMsg {
            message: "mux daemon cannot process upgrade requests right now".to_string(),
        };
        let resp = MuxMessage::control(MessageType::Error, 0, &err);
        let _ = reply.send_reply(resp).await;
        return;
    }

    match tokio::time::timeout(UPGRADE_PREPARE_TIMEOUT, reply_rx).await {
        Ok(recv_result) => {
            if let Some(msg) = upgrade_reply_to_message(recv_result) {
                let _ = reply.send_reply(msg).await;
            } else {
                log::info!("Upgrade preparation succeeded; daemon is replacing itself");
            }
        }
        Err(_) => {
            log::warn!("Upgrade preparation timed out");
            let err = ErrorMsg {
                message: "mux daemon did not respond to the upgrade request in time".to_string(),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = reply.send_reply(resp).await;
        }
    }
}

/// Log CLI-initiated window creation for debugging.
async fn log_cli_window_creation(session_manager: &Arc<Mutex<SessionManager>>, session_id: u32) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        let window_names: Vec<String> = session.windows.values().map(|w| w.name.clone()).collect();
        log::info!(
            "CLI created window in session {} '{}': windows = {:?}",
            session_id,
            session.name,
            window_names
        );
    }
}

/// Route a single message to the appropriate handler.
///
/// Returns `Err(true)` when the connection should be closed,
/// `Err(false)` on a non-fatal send error, and `Ok(())` otherwise.
///
/// ### Audit note (mux-window-switch-output-hang task0001, reworked task0002)
///
/// This function runs inside the connection's own `select!` loop
/// (`handle_connection`, the `msg = client_reader.next() =>` arm — renamed
/// from `framed.next()` by task0001's read/write split), so any call it
/// makes into a handler that performs a *blocking* send/reserve against
/// `pane_output_tx` would self-deadlock the connection: that channel's ONLY
/// consumer is this same task's `pane_output_rx.recv()` arm, which cannot
/// run while this task is suspended somewhere below `route_message`.
/// Audited every arm below for exactly that shape; two were found and
/// fixed (neither performs a blocking send/reserve on `pane_output_tx`
/// anymore):
/// - `RequestPaneSnapshot` -> `handle_request_pane_snapshot`, which used to
///   `pane_output_tx.send(...).await` — now uses
///   `pane::enqueue_pane_output_chunk` (try_send, deferring onto
///   `deferred_output` — a connection-owned, bounded `DeferredOutputQueue` —
///   only when the channel is momentarily full; see that type's doc).
/// - `SetVisibility` -> `handle_set_visibility`'s visibility-resume loop,
///   which used to `pane_output_tx.reserve().await` per pane — now uses
///   `try_reserve()` with the same `deferred_output` deferral on `Full`.
///
/// Every other arm below either does not touch `pane_output_tx` at all, or
/// only clones it for storage (`Attach`, `CreateWindow` — the actual PTY
/// output send for those happens later, off this task, on the pane's
/// reader thread via `blocking_send`, which blocks that native OS thread
/// only, not this connection task).
///
/// **Correction (mux-window-switch-output-hang task0004 rework, review
/// round 3 finding `22251d51cc98261e`):** the claim above — "blocks that
/// native OS thread only, not this connection task" — was FALSE for
/// `pty_spawn.rs`'s EOF branch prior to this rework: it held the pane's
/// `output_target` std mutex across its own `blocking_send`, unlike the
/// data path a few lines below it in the same file (which already released
/// the guard first, with an explicit "release lock before blocking_send to
/// avoid deadlock" comment). task0003's fair-permit path made that mutex
/// reachable from THIS connection task too (`resume_pane_with_permit`,
/// `pane.rs`, takes the same std mutex synchronously, no `.await` yield
/// point) — so a reader thread parked in the EOF branch's `blocking_send`
/// could block this connection task's own `lock()` call, which could only
/// be released by this same task's drain arm running, which cannot happen
/// while blocked there: a cross-thread self-deadlock, reachable via the
/// `SetVisibility` arm's resume path. Fixed in `pty_spawn.rs` (the EOF
/// branch now releases the guard before sending, mirroring the data path);
/// the claim above is accurate again for both PTY-output send paths.
///
/// The opportunistic `flush_deferred_output` call at the top (task0002) is
/// defensive: `handle_connection`'s own drain of `pane_output_rx` is the
/// primary trigger (capacity can only free there), but giving every
/// incoming client message a chance to progress the queue too closes the
/// residual edge case of PTY output going quiet right after a deferral.
///
/// `outbound_tx` (task0001): every reply/frame this function (or a handler
/// it calls) sends goes through the GUI loop's outbound admission queue —
/// this function is only ever called from that loop, never the CLI-client
/// path, so it is no longer generic over a raw stream type at all.
#[allow(clippy::too_many_arguments)]
async fn route_message(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    outbound_tx: &mpsc::Sender<MuxMessage>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
    kick_rx: &mut Option<oneshot::Receiver<()>>,
    visible_state: &Arc<AtomicBool>,
    upgrade_tx: &UpgradeSignalSender,
    deferred_output: &mut DeferredOutputQueue,
) -> Result<(), bool> {
    flush_deferred_output(
        deferred_output,
        pane_output_tx,
        session_manager,
        *active_session_id,
        visible_state,
    )
    .await;

    match msg.msg_type {
        MessageType::CreateWindow => {
            handle_create_window(
                &msg,
                session_manager,
                &mut OutboundHandle::new(outbound_tx),
                pane_output_tx,
                *active_session_id,
                title_tx,
                notification_tx,
                agent_status_tx,
                pane_exit_sender,
            )
            .await?;
        }
        MessageType::Attach => {
            handle_attach(
                msg,
                session_manager,
                outbound_tx,
                pane_output_tx,
                active_session_id,
                title_tx,
                kick_rx,
                visible_state,
            )
            .await?;
        }
        MessageType::Detach => {
            log::info!("Client requested detach");
            let resp = MuxMessage::control(MessageType::Detached, 0, &());
            let _ = outbound_tx.send(resp).await;
            return Err(true);
        }
        MessageType::DestroyPane => {
            handle_destroy_pane(msg.pane_id, session_manager, shutdown_tx).await;
        }
        MessageType::SwitchWindow => {
            handle_switch_window(msg.pane_id, session_manager).await;
        }
        MessageType::RenameWindow => {
            handle_rename_window(msg, session_manager).await;
        }
        MessageType::MoveWindow => {
            handle_move_window(msg, session_manager).await;
        }
        MessageType::DestroyWindow => {
            handle_destroy_window(msg.pane_id, session_manager, shutdown_tx).await;
        }
        MessageType::Resize => {
            handle_resize(msg, session_manager).await;
        }
        // The former mux status-bar GUI→daemon request (opcode 0x17, see
        // `mux_ipc::protocol`'s reserved-opcode comment) was retired by
        // mux-status-bar-removal task0001; that opcode no longer decodes
        // into a `MuxMessage` at all (`MuxCodec::decode` discards it with
        // a warn log before this match ever runs — see `codec.rs`), so it
        // falls through to the wildcard arm below like any other
        // unrecognized message type.
        MessageType::RequestPaneSnapshot => {
            // WARN-level entry log so release builds capture whether the
            // request even reached the daemon. The reply is logged inside
            // handle_request_pane_snapshot once the snapshot is built.
            log::warn!("RequestPaneSnapshot: received for pane {}", msg.pane_id);
            handle_request_pane_snapshot(
                &msg,
                *active_session_id,
                session_manager,
                pane_output_tx,
                deferred_output,
            )
            .await?;
        }
        MessageType::SetVisibility => {
            let payload = match SetVisibilityPayload::from_payload(&msg.payload) {
                Some(p) => p,
                None => {
                    log::warn!("SetVisibility: empty payload, ignoring");
                    return Ok(());
                }
            };
            handle_set_visibility(
                payload.visible,
                session_manager,
                *active_session_id,
                pane_output_tx,
                visible_state,
                deferred_output,
            )
            .await;
        }
        MessageType::PtyInput => {
            let pane_id = msg.pane_id;
            let mgr = session_manager.lock().await;
            if let Some((session_id, window_id)) = mgr.find_pane(pane_id) {
                if let Some(session) = mgr.get_session(session_id) {
                    if let Some(window) = session.windows.get(&window_id) {
                        if let Some(pane) = window.panes.get(&pane_id) {
                            if let Err(e) = pane.write_input(&msg.payload) {
                                log::warn!("Failed to write to pane {}: {}", pane_id, e);
                            }
                        }
                    }
                }
            }
        }
        MessageType::Upgrade => {
            // FR1/FR3 (task0009 rework): a persistent GUI connection may
            // request an upgrade too, not only the short-lived `emterm mux
            // upgrade` CLI connection — same handling as the CLI path,
            // factored into `handle_upgrade_request` so the two never drift.
            log::info!("GUI client requested mux daemon upgrade");
            handle_upgrade_request(&mut OutboundHandle::new(outbound_tx), upgrade_tx).await;
        }
        _ => {
            log::debug!(
                "Unhandled {:?} for pane {} ({} bytes)",
                msg.msg_type,
                msg.pane_id,
                msg.payload.len()
            );
        }
    }
    Ok(())
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
fn merge_consecutive_chunks(chunks: Vec<PtyOutputChunk>) -> Vec<PtyOutputChunk> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── G2 starvation guard (AC-2, mux-window-switch-output-hang task0004
    // rework, review round 3 findings `dd23cfc388062939`/
    // `5c01ffb8d53dc9f7`): deterministic coverage for
    // `allow_client_message_arm`, with no live connection, timing, or
    // scheduling involved — this is what actually pins the
    // bounded-iterations guarantee AC-2 requires (see that function's own
    // doc for why the accompanying live-connection test cannot reliably
    // force the underlying race either way). ──

    /// When there is no deferred work outstanding, the client arm is
    /// always allowed — the counter is irrelevant, including past the
    /// quota. Ordinary traffic (no pending deferral) sees ZERO behavior
    /// change from this guard.
    #[test]
    fn allow_client_message_arm_true_when_no_deferred_work_regardless_of_counter() {
        assert!(allow_client_message_arm(false, 0));
        assert!(allow_client_message_arm(false, CLIENT_MSG_STARVATION_QUOTA));
        assert!(allow_client_message_arm(
            false,
            CLIENT_MSG_STARVATION_QUOTA + 100
        ));
    }

    /// While deferred work IS outstanding: the client arm remains allowed
    /// for exactly `CLIENT_MSG_STARVATION_QUOTA` consecutive wins, then is
    /// excluded on the very next one — the bounded-iterations guarantee
    /// AC-2 requires. Regression shape: reverting the guard to
    /// unconditionally `true` (the pre-G2-fix behavior) makes this fail at
    /// `n == CLIENT_MSG_STARVATION_QUOTA` (confirmed during development).
    #[test]
    fn allow_client_message_arm_true_for_quota_iterations_then_excludes_on_the_next() {
        for n in 0..CLIENT_MSG_STARVATION_QUOTA {
            assert!(
                allow_client_message_arm(true, n),
                "iteration {n} (< quota) must still allow the client arm"
            );
        }
        assert!(
            !allow_client_message_arm(true, CLIENT_MSG_STARVATION_QUOTA),
            "the quota'th consecutive iteration must exclude the client arm so the \
             reservation/drain arms are guaranteed a poll"
        );
        // Past the quota stays excluded too (the counter only grows further
        // if some other bug let it; the guard must not start allowing
        // again just because the count exceeded the quota by more).
        assert!(!allow_client_message_arm(
            true,
            CLIENT_MSG_STARVATION_QUOTA + 5
        ));
    }

    // ── G4 (AC-4, mux-window-switch-output-hang task0005, review round 4
    // finding `e6ac2a334424ebd7`): deterministic coverage for
    // `has_unforwarded_pane_output` — the queue -> channel -> client
    // transition. A real `tokio::sync::mpsc` channel is used (so
    // `Receiver::is_empty()` reflects genuine channel state), but nothing
    // here depends on a live connection, timing, or scheduling. ──

    /// The exact gap this rework closes: an item already admitted into
    /// `pane_output_tx` (so the connection's OWN deferred-output bookkeeping
    /// — `has_deferred_work` — has already gone back to `false`) but not yet
    /// drained by the `chunk = pane_output_rx.recv()` arm must still count
    /// as pending output. Before this rework, `has_deferred_work` alone
    /// would have reported `false` here, letting the client-message arm's
    /// guard `allow_client_message_arm` return `true` unconditionally.
    #[tokio::test]
    async fn has_unforwarded_pane_output_true_when_channel_holds_an_undelivered_item_even_with_deferred_bookkeeping_clear()
     {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
        tx.send(PtyOutputChunk::pty_output(1, vec![1, 2, 3]))
            .await
            .unwrap();
        assert!(
            has_unforwarded_pane_output(false, &rx),
            "AC-4: an item admitted into the channel but not yet drained must still \
             count as pending output, even when the connection's own deferred-output \
             bookkeeping (`has_deferred_work`) alone already reports none outstanding"
        );
    }

    /// The negative case: an empty channel AND no deferred bookkeeping means
    /// genuinely nothing is pending — the guard must not be held open
    /// forever by a stale/incorrect signal.
    #[tokio::test]
    async fn has_unforwarded_pane_output_false_when_channel_and_deferred_bookkeeping_both_clear() {
        let (_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
        assert!(!has_unforwarded_pane_output(false, &rx));
    }

    /// `has_deferred_work == true` short-circuits to `true` regardless of
    /// channel state — the ORIGINAL signal (reservation pending / deferred
    /// queue non-empty) must keep working exactly as before; this rework
    /// only ADDS a second way to be `true`, it never removes the first.
    #[tokio::test]
    async fn has_unforwarded_pane_output_true_when_deferred_work_outstanding_even_with_empty_channel()
     {
        let (_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
        assert!(has_unforwarded_pane_output(true, &rx));
    }

    // ── AC-5 (mux-window-switch-output-hang task0005, medium finding
    // connection.rs:820, review round 4): deterministic coverage for
    // `next_client_msg_starvation_count`'s increment/reset arithmetic. ──

    /// The client arm running while pending output was already outstanding
    /// increments the count.
    #[test]
    fn next_client_msg_starvation_count_increments_when_client_arm_ran_with_pending_output() {
        assert_eq!(
            next_client_msg_starvation_count(true, true, 0),
            1,
            "first consecutive win increments from 0"
        );
        assert_eq!(
            next_client_msg_starvation_count(true, true, 3),
            4,
            "a subsequent consecutive win increments further"
        );
    }

    /// A different arm winning resets the count to 0, regardless of how
    /// high it had climbed — the one-iteration exclusion never compounds.
    #[test]
    fn next_client_msg_starvation_count_resets_when_a_different_arm_won() {
        assert_eq!(
            next_client_msg_starvation_count(false, true, CLIENT_MSG_STARVATION_QUOTA),
            0
        );
    }

    /// No pending output outstanding resets the count to 0 even if the
    /// client arm itself won — ordinary traffic (no deferral pending) sees
    /// ZERO behavior change from this guard.
    #[test]
    fn next_client_msg_starvation_count_resets_when_no_pending_output_outstanding() {
        assert_eq!(
            next_client_msg_starvation_count(true, false, CLIENT_MSG_STARVATION_QUOTA),
            0
        );
    }

    /// Full cycle: the counter climbs for exactly `CLIENT_MSG_STARVATION_QUOTA`
    /// consecutive client-arm wins (each still allowed by
    /// `allow_client_message_arm`), reaches the quota (now excluded), and
    /// then — once a different arm wins the excluded iteration — resets to 0
    /// and the client arm is allowed again. Wires the two extracted pure
    /// functions together exactly as `handle_connection`'s loop does, without
    /// any live connection.
    #[test]
    fn next_client_msg_starvation_count_full_quota_cycle_then_reset() {
        let mut count = 0u32;
        for _ in 0..CLIENT_MSG_STARVATION_QUOTA {
            assert!(
                allow_client_message_arm(true, count),
                "must still be allowed below the quota"
            );
            count = next_client_msg_starvation_count(true, true, count);
        }
        assert_eq!(count, CLIENT_MSG_STARVATION_QUOTA);
        assert!(
            !allow_client_message_arm(true, count),
            "quota reached: the client arm must be excluded for exactly one iteration"
        );
        // The excluded iteration: some other arm wins (`took_client_arm ==
        // false`), resetting the counter for the next round.
        count = next_client_msg_starvation_count(false, true, count);
        assert_eq!(count, 0, "the excluded iteration resets the counter");
        assert!(
            allow_client_message_arm(true, count),
            "the very next iteration reverts to prioritizing client messages"
        );
    }

    fn chunk(pane_id: u32, data: &[u8]) -> PtyOutputChunk {
        PtyOutputChunk::pty_output(pane_id, data.to_vec())
    }

    fn exit_chunk(pane_id: u32) -> PtyOutputChunk {
        PtyOutputChunk::pty_output(pane_id, Vec::new())
    }

    fn snapshot_chunk(pane_id: u32, data: &[u8]) -> PtyOutputChunk {
        PtyOutputChunk::snapshot(pane_id, data.to_vec())
    }

    #[test]
    fn merge_single_chunk() {
        let chunks = vec![chunk(1, b"hello")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"hello");
    }

    #[test]
    fn merge_same_pane_consecutive() {
        let chunks = vec![chunk(1, b"hel"), chunk(1, b"lo"), chunk(1, b"!")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"hello!");
    }

    #[test]
    fn merge_different_panes_not_merged() {
        let chunks = vec![chunk(1, b"a"), chunk(2, b"b"), chunk(1, b"c")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"a");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"b");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"c");
    }

    #[test]
    fn merge_exit_signal_not_merged() {
        let chunks = vec![chunk(1, b"data"), exit_chunk(1)];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].data, b"data");
        assert!(merged[1].data.is_empty());
    }

    #[test]
    fn merge_exit_signal_mid_batch() {
        // pane 1 data, pane 1 exit, pane 1 data (from new process or leftover)
        let chunks = vec![chunk(1, b"before"), exit_chunk(1), chunk(1, b"after")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].data, b"before");
        assert!(merged[1].data.is_empty());
        assert_eq!(merged[2].data, b"after");
    }

    #[test]
    fn merge_mixed_pane_ordering_preserved() {
        // Interleaved panes: A, B, A, B — ordering must be preserved
        let chunks = vec![
            chunk(1, b"a1"),
            chunk(1, b"a2"),
            chunk(2, b"b1"),
            chunk(2, b"b2"),
            chunk(1, b"a3"),
            chunk(3, b"c1"),
        ];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"a1a2");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"b1b2");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"a3");
        assert_eq!(merged[3].pane_id, 3);
        assert_eq!(merged[3].data, b"c1");
    }

    // ── merge / drain efficiency metrics (IPC frame-count regression guard) ──
    //
    // The daemon drains up to `DRAIN_BATCH_LIMIT` chunks per select! iteration
    // and merges consecutive same-pane chunks into one before sending. Each
    // surviving merged chunk becomes one IPC frame (one base64-encoded APC/OSC
    // envelope on the bridge transport), so "output chunk count" is the
    // frame-count metric the perf work optimizes. These tests pin the
    // input-vs-output chunk reduction deterministically (counts only, no
    // timing) so a regression that stops coalescing is caught.

    /// Total bytes are conserved across the merge (no data lost/duplicated)
    /// while the chunk count collapses — the core efficiency invariant.
    fn total_bytes(chunks: &[PtyOutputChunk]) -> usize {
        chunks.iter().map(|c| c.data.len()).sum()
    }

    #[test]
    fn merge_efficiency_single_pane_n_to_one() {
        // N consecutive same-pane chunks → exactly 1 IPC frame.
        for n in [2usize, 8, 64] {
            let chunks: Vec<PtyOutputChunk> = (0..n).map(|_| chunk(1, b"abcd")).collect();
            let bytes_in = total_bytes(&chunks);
            let merged = merge_consecutive_chunks(chunks);
            assert_eq!(
                merged.len(),
                1,
                "{n} consecutive same-pane chunks must merge to 1 frame"
            );
            // Frame-count reduction: N inputs → 1 output.
            assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost in merge");
            assert_eq!(merged[0].data.len(), n * 4, "all payloads concatenated");
        }
    }

    #[test]
    fn merge_efficiency_only_consecutive_same_pane_collapses() {
        // Mixed panes: only *runs* of the same pane collapse. Six input chunks
        // across runs [1,1,1 | 2,2 | 1] → three output frames (one per run).
        let chunks = vec![
            chunk(1, b"a"),
            chunk(1, b"b"),
            chunk(1, b"c"),
            chunk(2, b"d"),
            chunk(2, b"e"),
            chunk(1, b"f"),
        ];
        let bytes_in = total_bytes(&chunks);
        let input_count = chunks.len();
        let merged = merge_consecutive_chunks(chunks);

        // 6 input chunks → 3 output frames (a 50% frame reduction here).
        assert_eq!(input_count, 6);
        assert_eq!(
            merged.len(),
            3,
            "only consecutive same-pane runs collapse; interleaving stays split"
        );
        assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost");
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"abc");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"de");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"f");
    }

    #[test]
    fn merge_efficiency_alternating_panes_no_reduction() {
        // Worst case: strictly alternating panes never collapse, so the frame
        // count is unchanged (input count == output count). This pins the
        // lower bound of the optimization (merge never *increases* frames).
        let chunks = vec![
            chunk(1, b"a"),
            chunk(2, b"b"),
            chunk(1, b"c"),
            chunk(2, b"d"),
        ];
        let input_count = chunks.len();
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            input_count,
            "alternating panes cannot merge; frame count is unchanged"
        );
    }

    #[test]
    fn merge_efficiency_full_drain_batch_single_pane() {
        // A full drain batch (DRAIN_BATCH_LIMIT chunks) from one busy pane —
        // the bulk-output hot path — collapses to a single IPC frame. This is
        // the headline win the perf work relies on: 64 drained chunks → 1 send.
        let chunks: Vec<PtyOutputChunk> = (0..DRAIN_BATCH_LIMIT)
            .map(|_| chunk(42, &[0u8; 1024]))
            .collect();
        let bytes_in = total_bytes(&chunks);
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            1,
            "a full {DRAIN_BATCH_LIMIT}-chunk single-pane drain merges to 1 frame"
        );
        assert_eq!(total_bytes(&merged), bytes_in);
        assert_eq!(
            merged[0].data.len(),
            DRAIN_BATCH_LIMIT * 1024,
            "merged frame carries the whole batch"
        );
    }

    /// Phase 2 (FR1, FR5): a `Snapshot`-kind chunk inserted between
    /// same-pane `PtyOutput` chunks MUST NOT be folded into either
    /// neighbour. The on-wire framing for `Snapshot` is
    /// `MessageType::Snapshot` while neighbours are `MessageType::PtyOutput`;
    /// collapsing them would smuggle snapshot bytes into a live-input frame
    /// (or vice versa) and break the routing to the off-thread replay path.
    #[test]
    fn merge_does_not_fold_across_kind() {
        let chunks = vec![
            chunk(1, b"pre1"),
            chunk(1, b"pre2"),
            snapshot_chunk(1, b"SNAPSHOT"),
            chunk(1, b"post"),
        ];
        let merged = merge_consecutive_chunks(chunks);
        // Expected: [merged-PtyOutput("pre1pre2"), Snapshot("SNAPSHOT"), PtyOutput("post")]
        assert_eq!(merged.len(), 3, "kind boundary must split the run");
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].kind, ChunkKind::PtyOutput);
        assert_eq!(merged[0].data, b"pre1pre2");
        assert_eq!(merged[1].pane_id, 1);
        assert_eq!(merged[1].kind, ChunkKind::Snapshot);
        assert_eq!(merged[1].data, b"SNAPSHOT");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].kind, ChunkKind::PtyOutput);
        assert_eq!(merged[2].data, b"post");
    }

    /// Phase 2 (FR5): two consecutive `Snapshot`-kind chunks for the same
    /// pane MUST remain separate frames. Each `RequestPaneSnapshot` reply
    /// is one snapshot payload; concatenating two snapshot payloads on the
    /// wire would produce a malformed single frame whose recipient cannot
    /// segment them. (In practice the daemon only emits one snapshot per
    /// request, but the merge logic must not assume that.)
    #[test]
    fn merge_does_not_collapse_consecutive_snapshots() {
        let chunks = vec![snapshot_chunk(1, b"SNAP-A"), snapshot_chunk(1, b"SNAP-B")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 2, "snapshots are never coalesced");
        assert_eq!(merged[0].data, b"SNAP-A");
        assert_eq!(merged[0].kind, ChunkKind::Snapshot);
        assert_eq!(merged[1].data, b"SNAP-B");
        assert_eq!(merged[1].kind, ChunkKind::Snapshot);
    }

    #[test]
    fn merge_efficiency_exit_signals_stay_separate() {
        // Exit signals (empty data) are never merged, so they always cost their
        // own frame even amid same-pane data. Pin this so the frame-count model
        // accounts for them: 3 data chunks + 1 exit (same pane) → 2 frames.
        let chunks = vec![
            chunk(1, b"x"),
            chunk(1, b"y"),
            chunk(1, b"z"),
            exit_chunk(1),
        ];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            2,
            "same-pane data collapses to 1 frame; the exit signal is a 2nd frame"
        );
        assert_eq!(merged[0].data, b"xyz");
        assert!(
            merged[1].data.is_empty(),
            "exit signal preserved separately"
        );
    }

    // ---- mux-daemon-hot-upgrade task0004: MessageType::Upgrade CLI reply
    // translation ----
    //
    // `upgrade_reply_to_message` is the pure core of `handle_cli_client`'s
    // `MessageType::Upgrade` arm: given the accept loop's reply, decide what
    // (if anything) goes back to the client. Extracted so these branches are
    // unit-testable without a live connection or a real accept loop.

    /// Successful preparation produces no explicit reply (the connection is
    /// simply dropped once the process is replaced, IMPLEMENTATION.md D2).
    #[test]
    fn upgrade_reply_to_message_success_is_none() {
        assert!(upgrade_reply_to_message(Ok(Ok(()))).is_none());
    }

    /// AC-4: an abort reason reported by the accept loop becomes an `Error`
    /// control message carrying that exact reason.
    #[test]
    fn upgrade_reply_to_message_abort_reason_becomes_error_message() {
        let msg = upgrade_reply_to_message(Ok(Err("disk full".to_string())))
            .expect("an abort reason must produce a reply message");
        assert_eq!(msg.msg_type, MessageType::Error);
        let payload: ErrorMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.message, "disk full");
    }

    /// A closed reply channel (accept loop dropped it without answering,
    /// e.g. mid-shutdown) still produces a client-facing `Error` message
    /// rather than silently dropping the connection with no explanation.
    #[tokio::test]
    async fn upgrade_reply_to_message_channel_closed_becomes_generic_error() {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), String>>();
        drop(reply_tx);
        let recv_result = reply_rx.await;
        assert!(recv_result.is_err(), "dropped sender must yield RecvError");

        let msg = upgrade_reply_to_message(recv_result)
            .expect("a closed reply channel must still produce a reply message");
        assert_eq!(msg.msg_type, MessageType::Error);
        let payload: ErrorMsg = msg.decode_payload().unwrap();
        assert!(!payload.message.is_empty());
    }

    // ── mux-window-switch-output-hang task0003 rework: connection-level
    // coverage for AC-3 (starvation-freedom) and AC-4 (FR2 progress under a
    // pending deferred snapshot) — review round 2 findings
    // `dda847f76f68fea7`/`9361b9b42c69fb92` (round 1 also raised this; every
    // prior test in this feature drove handlers directly and called
    // `flush_deferred_output` by hand instead of the real `select!` loop). ──

    use crate::mux::session::pane::{DetachReason, MuxPane, PaneOutputTarget, SharedOutputTarget};
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::Ordering as StdOrdering;

    fn no_ack_slot() -> SharedUpgradeAckSlot {
        Arc::new(StdMutex::new(None))
    }

    struct CapturingWriter(Arc<StdMutex<Vec<u8>>>);
    impl std::io::Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// task0001 (AC-4/AC-6): a `tokio::io::DuplexStream` wrapper whose
    /// WRITE side (`poll_write`/`poll_flush`) starts unconditionally
    /// failing once `fail_writes` is flipped `true` — deterministic,
    /// test-controlled "the socket write failed" injection, sharper than
    /// a saturated-but-not-broken duplex (AC-1's own harness): reads keep
    /// working (delegated straight through) so the handshake/attach
    /// sequence still completes normally before the test flips the flag.
    struct FailableWriteStream {
        inner: tokio::io::DuplexStream,
        fail_writes: Arc<std::sync::atomic::AtomicBool>,
    }

    impl tokio::io::AsyncRead for FailableWriteStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for FailableWriteStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.fail_writes.load(StdOrdering::Relaxed) {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "test-injected write failure",
                )));
            }
            std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.fail_writes.load(StdOrdering::Relaxed) {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "test-injected write failure",
                )));
            }
            std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
    }

    /// Read frames off `client` until a `PaneCreated` has been seen for
    /// every id in `expected_pane_ids` (ignoring every other frame type
    /// in between, e.g. `SnapshotRestore`).
    async fn drain_until_pane_created(
        client: &mut Framed<tokio::io::DuplexStream, MuxCodec>,
        expected_pane_ids: &[u32],
    ) {
        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while seen.len() < expected_pane_ids.len() {
            let msg = tokio::time::timeout(Duration::from_secs(5), client.next())
                .await
                .expect("must not hang waiting for reattach frames")
                .expect("stream must not end")
                .expect("frame must decode");
            if msg.msg_type == MessageType::PaneCreated && expected_pane_ids.contains(&msg.pane_id)
            {
                seen.insert(msg.pane_id);
            }
        }
    }

    /// AC-3/AC-4: drives the REAL `handle_connection` `select!` loop over a
    /// duplex stream (mirroring `mux::daemon`'s own `handle_connection`
    /// spawn test). Pane A's channel is saturated by 8 background OS threads
    /// calling `blocking_send` exactly the way `pty_spawn.rs`'s reader
    /// thread does — the scenario `flush_deferred_output`'s `try_send`/
    /// `try_reserve` retries lose to systematically (G2). A single producer
    /// thread and small payloads were tried first and did not reliably
    /// reproduce the starvation (this test's own red/green history: it
    /// failed reliably, and only with, this many concurrent producers and
    /// this payload size before the AC-3 fix existed) — 8 threads racing
    /// continuously and a 32KB payload per chunk make the channel
    /// genuinely, continuously saturated rather than momentarily.
    ///
    /// G4 rework (AC-4, mux-window-switch-output-hang task0004, review
    /// round 3 finding `30cdb3b4400888fc`) — ACTUALLY IMPLEMENTED in
    /// task0005 (review round 4 findings `e725f0403d431092` /
    /// `9078d44a6c2897ec` / `ead53217898d6933`): task0004's own doc comment
    /// and `test-docs/mux-window-switch-output-hang/task0004.tests.yaml`
    /// AC-4 entry claimed this exact rework had already happened and had
    /// been observed red-then-green. Neither was true — the code still sent
    /// `RequestPaneSnapshot` for `PANE_B`, the assertion was still a
    /// post-loop check, and no ordering flag existed anywhere in this file.
    /// Round 4 caught the discrepancy directly against the source; see
    /// `test-docs/mux-window-switch-output-hang/task0004.tests.yaml`'s AC-4
    /// entry (corrected by task0005) for the full account. What follows is
    /// the rework as actually implemented and observed red-then-green under
    /// task0005: the `RequestPaneSnapshot` now targets PANE_A ITSELF — the
    /// very pane whose reader threads are saturating the channel — while
    /// `PtyInput` still targets the DIFFERENT pane B. This is SPEC Unit Test
    /// 1's exact composition ("input to a *different* pane keeps flowing
    /// while [the requesting] pane's snapshot is pending") AND SPEC Edge
    /// case 1 ("snapshot requested for the exact pane producing the
    /// high-volume output") at once — the pre-task0005 version of this test
    /// requested PANE_B's own snapshot, so cross-pane input-vs-
    /// snapshot-pending was never actually exercised. `input_processed_
    /// before_snapshot` proves ORDER, not just eventual truth, for pane B's
    /// input: captured at the exact moment the `Snapshot(A)` frame is
    /// observed rather than re-derived after the loop ends, a build that
    /// processes B's `PtyInput` only AFTER delivering A's snapshot (instead
    /// of interleaved with it, as `route_message` does synchronously)
    /// cannot pass this assertion merely by the time the test ends.
    /// `output_flowed_before_snapshot` is WEAKER than that (task0006
    /// rework, review round 5): it only proves SOME `PtyOutput(PANE_A)`
    /// frame was observed before the `Snapshot(A)` frame, which the
    /// pre-request channel saturation (8 background producer threads
    /// already flooding pane A's channel before `RequestPaneSnapshot` is
    /// even sent) guarantees trivially, regardless of whether the
    /// connection keeps draining pane A's output correctly WHILE the
    /// deferred snapshot is pending. It does not, by itself, distinguish
    /// "kept draining throughout the wait" from "some output happened to
    /// already be buffered/in flight before the request landed" — closing
    /// that gap would require tagging payloads pre/post-request and
    /// asserting on the tagged sequence, which this task's scope does not
    /// include.
    /// Confirmed load-bearing by reverting JUST the wait-loop match arm back
    /// to `(Snapshot, PANE_B)` (the pre-task0005 target) while leaving
    /// everything else — the retargeted `RequestPaneSnapshot`, the
    /// at-the-moment `input_processed_before_snapshot`/
    /// `output_flowed_before_snapshot` capture — as implemented: the test
    /// hung and panicked ("must not hang") since the real `Snapshot(A)`
    /// frame the server actually sends is never matched. Restored the
    /// correct `(Snapshot, PANE_A)` arm afterward. The "strict same-moment
    /// ordering" dimension specifically — capturing the flags at the exact
    /// instant the `Snapshot(A)` frame is observed, rather than re-deriving
    /// them after the loop ends — could NOT independently be forced red in
    /// this environment (see `test-docs/mux-window-switch-output-hang/
    /// task0005.tests.yaml`'s AC-1 entry for the honest account of why: by
    /// this test's own construction, pane B's `PtyInput` is processed
    /// almost immediately, so input landing before the snapshot is
    /// guaranteed by timing, not something a regression could plausibly
    /// invert here). Recorded accordingly — only the retargeting/matching
    /// revert is claimed as observed red.
    #[tokio::test]
    async fn connection_level_deferred_snapshot_survives_sustained_saturation_and_input_keeps_flowing()
     {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        const PANE_B: u32 = 2;
        let captured_input: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        {
            let mut mgr = session_manager.lock().await;
            let sid = mgr.create_session("default".to_string());
            let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);

            let target_b: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_b = MuxPane::new_test_with_writer(
                PANE_B,
                80,
                24,
                target_b,
                Box::new(CapturingWriter(captured_input.clone())),
            );
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_b);
        }

        let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(handle_connection(
            server_stream,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());

        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);
        let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
        let session_id = match welcome_payload {
            WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
            WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
        };

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A, PANE_B]).await;

        // Extract the connection's OWN `pane_output_tx` clone now that pane
        // A is Connected through it (installed by `collect_reattach_data`
        // during the Attach above) — the REAL channel `handle_connection`'s
        // own `select!` loop drains, not a stand-in.
        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        // Background OS thread saturating pane A's channel via
        // `blocking_send` — exactly the shape `pty_spawn.rs`'s reader
        // thread uses (AC-3's stated scenario).
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut producers = Vec::new();
        for _ in 0..8 {
            let stop_clone = stop.clone();
            let producer_tx = owned_tx.clone();
            producers.push(std::thread::spawn(move || {
                while !stop_clone.load(StdOrdering::Relaxed) {
                    if producer_tx
                        .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }

        // Give the producer time to actually saturate the channel (a
        // parked `blocking_send` waiter) before issuing the request this
        // test is about.
        tokio::time::sleep(Duration::from_millis(100)).await;

        client
            .send(MuxMessage {
                msg_type: MessageType::RequestPaneSnapshot,
                pane_id: PANE_A,
                payload: Vec::new(),
            })
            .await
            .unwrap();
        client
            .send(MuxMessage {
                msg_type: MessageType::PtyInput,
                pane_id: PANE_B,
                payload: b"hi".to_vec(),
            })
            .await
            .unwrap();

        let mut saw_output_for_pane_a = false;
        let mut saw_snapshot_for_pane_a = false;
        // AC-4 (task0005 rework, review round 4 finding G1): captured the
        // FIRST time the Snapshot(A) frame is observed, not re-derived
        // after the loop ends — see this test's own doc for why a
        // post-loop check cannot distinguish "processed before delivery"
        // from "processed only because the test kept reading afterward".
        let mut input_processed_before_snapshot = false;
        let mut output_flowed_before_snapshot = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !(saw_output_for_pane_a && saw_snapshot_for_pane_a) {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "AC-3/AC-4: connection must keep forwarding pane A's output AND \
                 deliver pane A's own deferred snapshot within a bounded time despite \
                 sustained saturating output; saw_output_for_pane_a={saw_output_for_pane_a} \
                 saw_snapshot_for_pane_a={saw_snapshot_for_pane_a}"
            );
            let msg = tokio::time::timeout(remaining, client.next())
                .await
                .expect("must not hang")
                .expect("stream must not end")
                .expect("frame must decode");
            match (msg.msg_type, msg.pane_id) {
                (MessageType::PtyOutput, PANE_A) => saw_output_for_pane_a = true,
                (MessageType::Snapshot, PANE_A) => {
                    // Record ordering at the EXACT moment this frame is
                    // observed — captured_input/saw_output_for_pane_a may
                    // both still change AFTER this point in a buggy build,
                    // so evaluating them here (rather than post-loop) is
                    // what actually proves "strictly before", not just
                    // "eventually true by the time the test ends".
                    input_processed_before_snapshot = *captured_input.lock().unwrap() == b"hi";
                    output_flowed_before_snapshot = saw_output_for_pane_a;
                    saw_snapshot_for_pane_a = true;
                }
                _ => {}
            }
        }

        // AC-4: at least one PtyOutput(PANE_A) frame must have already been
        // observed before the Snapshot(A) frame. Weaker than it may look
        // (task0006 rework, review round 5): the pre-request channel
        // saturation guarantees this trivially, so it does not by itself
        // distinguish "kept draining throughout the deferred-snapshot
        // wait" from "some output was already buffered/in flight before
        // the request landed" — see this test's own doc.
        assert!(
            output_flowed_before_snapshot,
            "AC-4: at least one PtyOutput(PANE_A) frame must be observed STRICTLY BEFORE \
             its deferred snapshot is delivered"
        );
        // AC-4: the PtyInput for pane B must have already been processed
        // (the write is synchronous inside `route_message`) STRICTLY
        // BEFORE pane A's deferred snapshot was delivered — not merely by
        // the time this loop happens to end.
        assert!(
            input_processed_before_snapshot,
            "AC-4: PtyInput for pane B must be processed STRICTLY BEFORE pane A's \
             deferred snapshot is delivered, not merely by the time the test ends"
        );

        stop.store(true, StdOrdering::Relaxed);
        drop(client);
        conn_task.abort();
        // Dropping `client` closes the duplex pair, which fails the
        // connection's own sends/receives and ends `handle_connection`,
        // which drops `pane_output_rx` — every subsequent `blocking_send`
        // on `producer_tx` then observes `Closed` and the thread exits.
        // `spawn_blocking` so this async test doesn't itself block on a
        // native thread join.
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                for p in producers {
                    p.join().unwrap();
                }
            }),
        )
        .await
        .expect("producer threads must exit once the channel closes")
        .expect("spawn_blocking must not panic");
    }

    /// AC-2 (G2 rework, mux-window-switch-output-hang task0004, review
    /// round 3 findings `dd23cfc388062939`/`5c01ffb8d53dc9f7`): a client
    /// that keeps the server's client-message arm (`client_reader.next()`,
    /// task0001 rename) CONTINUOUSLY ready must not be able to starve the
    /// fair-reservation/drain machinery forever. Pane A's
    /// channel is saturated exactly as in the sibling test above; a
    /// `RequestPaneSnapshot` for pane A is deferred against the full
    /// channel, and then — for the ENTIRE wait — a genuinely CONCURRENT
    /// background task keeps sending harmless `PtyInput` messages for
    /// pane B as fast as the socket accepts them, so the server's
    /// client-message arm is essentially always immediately ready to be
    /// polled. Before the G2 fix, `biased` ordering meant this arm won
    /// EVERY iteration under that pressure, so the fair-reservation arm
    /// (and the drain arm behind it) was never even POLLED — the deferred
    /// snapshot would starve forever. Asserts the snapshot is still
    /// delivered within a bounded timeout despite the continuous traffic.
    ///
    /// Honesty note (development history): a real socket's send/read
    /// timing has enough natural jitter that this end-to-end test was NOT
    /// observed to reliably fail even with the G2 guard reverted to
    /// unconditionally `true` — the exact race the finding describes
    /// depends on genuinely gapless message arrival, which real
    /// scheduling does not reliably force either way in this environment.
    /// This test still exercises the real connection path end-to-end and
    /// guards against gross regressions (e.g. the mechanism being removed
    /// or the connection genuinely deadlocking), but the DETERMINISTIC
    /// proof of the bounded-iterations guarantee is
    /// `allow_client_message_arm`'s own unit tests, above in this module.
    ///
    /// Honesty note 2 (mux-window-switch-output-hang task0005, review round
    /// 4 finding `bc0e5ae9c626fb31`, G2): this test's own cleanup used to
    /// `flood_task.await` BEFORE `conn_task.abort()`, which could hang the
    /// whole `cargo test` process if the flood's `send()` happened to be
    /// parked at that exact moment (see the fix's own comment at the
    /// cleanup site below). Reverting to that exact ordering and re-running
    /// this test (including 5 repeated runs) in this development
    /// environment did NOT reproduce a hang — the same class of
    /// real-socket timing jitter noted above evidently also affects
    /// whether the flood's `send()` is caught mid-flight at cleanup time.
    /// The reordering is kept as a structural fix (aborting the connection
    /// first is correct regardless of whether this exact interleaving is
    /// forced in any given run), but this is recorded honestly as NOT
    /// independently red-confirmed in this environment, rather than
    /// claimed as observed.
    ///
    /// Uses a REAL TCP loopback connection (`TcpStream::into_split()`),
    /// not this file's usual `tokio::io::duplex()` in-memory pair: this
    /// test's own red/green history — a single duplex stream split either
    /// via `Framed::split()` or the generic `tokio::io::split()` was
    /// observed, empirically, to serialize the flood task's sends and the
    /// main task's reads behind an internal lock, starving the read side
    /// regardless of the server's own scheduling (a test-harness artifact,
    /// not the G2 mechanism this test exists to exercise — that variant
    /// hung even WITH the G2 fix in place). A real socket's split halves
    /// are natively lock-free and full-duplex, which is what genuine
    /// concurrent send-while-read requires here.
    ///
    /// Multi-threaded runtime (unlike this file's other connection-level
    /// tests): the flood task, the connection task, and the main task's
    /// own reads are three independently-progressing actors here; a
    /// single-threaded (`current_thread`) runtime's cooperative
    /// scheduling was observed, empirically, to occasionally starve one of
    /// them of a turn entirely under sustained flood pressure. Separate OS
    /// worker threads remove that test-harness-only risk.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connection_level_deferred_snapshot_delivered_despite_continuous_client_traffic() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        const PANE_B: u32 = 2;
        {
            let mut mgr = session_manager.lock().await;
            let sid = mgr.create_session("default".to_string());
            let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);

            let target_b: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_b = MuxPane::new_test(PANE_B, 80, 24, target_b);
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_b);
        }

        // Real TCP loopback (not an in-memory `tokio::io::duplex()` pair):
        // the flood task below needs to send CONTINUOUSLY while the main
        // task reads CONCURRENTLY, and splitting either a combined
        // `Framed` (`.split()`) or a raw duplex stream (`tokio::io::split`)
        // for that was observed, empirically, to serialize reads and
        // writes behind an internal lock and starve the read side
        // regardless of the server's own scheduling — a test-harness
        // artifact unrelated to the G2 mechanism this test exists to
        // exercise. `TcpStream::into_split()` provides genuinely
        // independent, lock-free read/write halves (a real socket is
        // natively full-duplex), avoiding that pitfall.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback listener");
        let addr = listener.local_addr().unwrap();

        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let session_manager_for_conn = session_manager.clone();
        let conn_task = tokio::spawn(async move {
            let (server_stream, _peer) = listener.accept().await.expect("accept loopback conn");
            handle_connection(
                server_stream,
                session_manager_for_conn,
                shutdown_tx,
                title_tx,
                notification_tx,
                agent_status_tx,
                pane_exit_sender,
                upgrade_tx,
                no_ack_slot(),
            )
            .await;
        });

        let client_stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect loopback client");
        let _ = client_stream.set_nodelay(true);
        let (client_read_half, client_write_half) = client_stream.into_split();
        let mut client_writer =
            tokio_util::codec::FramedWrite::new(client_write_half, MuxCodec::new());
        let mut client_reader =
            tokio_util::codec::FramedRead::new(client_read_half, MuxCodec::new());

        client_writer
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client_reader.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);
        let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
        let session_id = match welcome_payload {
            WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
            WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
        };

        client_writer
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        // Inlined equivalent of the sibling test's `drain_until_pane_created`
        // helper (which is typed against a combined `Framed`, not the
        // split `FramedRead` half used here).
        {
            let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let expected_pane_ids = [PANE_A, PANE_B];
            while seen.len() < expected_pane_ids.len() {
                let msg = tokio::time::timeout(Duration::from_secs(5), client_reader.next())
                    .await
                    .expect("must not hang waiting for reattach frames")
                    .expect("stream must not end")
                    .expect("frame must decode");
                if msg.msg_type == MessageType::PaneCreated
                    && expected_pane_ids.contains(&msg.pane_id)
                {
                    seen.insert(msg.pane_id);
                }
            }
        }

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut producers = Vec::new();
        for _ in 0..8 {
            let stop_clone = stop.clone();
            let producer_tx = owned_tx.clone();
            producers.push(std::thread::spawn(move || {
                while !stop_clone.load(StdOrdering::Relaxed) {
                    if producer_tx
                        .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }

        // Give the producers time to actually saturate the channel before
        // issuing the request this test is about.
        tokio::time::sleep(Duration::from_millis(100)).await;

        client_writer
            .send(MuxMessage {
                msg_type: MessageType::RequestPaneSnapshot,
                pane_id: PANE_A,
                payload: Vec::new(),
            })
            .await
            .unwrap();

        // Continuous client traffic: a genuinely CONCURRENT background
        // task floods harmless `PtyInput` for pane B as fast as the socket
        // accepts sends, for as long as the main task is still waiting
        // below — the loopback TCP connection's independent read/write
        // halves (see this test's doc) make this safe, unlike a split
        // in-memory duplex stream.
        let flood_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flood_stop_clone = flood_stop.clone();
        let flood_task = tokio::spawn(async move {
            while !flood_stop_clone.load(StdOrdering::Relaxed) {
                if client_writer
                    .send(MuxMessage {
                        msg_type: MessageType::PtyInput,
                        pane_id: PANE_B,
                        payload: b"x".to_vec(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut saw_snapshot_for_pane_a = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !saw_snapshot_for_pane_a {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "AC-2: pane A's deferred snapshot must be delivered within a bounded \
                 time despite CONTINUOUS client-message traffic (the starvation this \
                 arm's quota guard exists to bound)"
            );
            let msg = tokio::time::timeout(remaining, client_reader.next())
                .await
                .expect("must not hang")
                .expect("stream must not end")
                .expect("frame must decode");
            if msg.msg_type == MessageType::Snapshot && msg.pane_id == PANE_A {
                saw_snapshot_for_pane_a = true;
            }
        }

        flood_stop.store(true, StdOrdering::Relaxed);
        stop.store(true, StdOrdering::Relaxed);
        // G2 rework (review round 4 finding `bc0e5ae9c626fb31`): `abort()`
        // the connection task BEFORE awaiting `flood_task`, and wrap that
        // join in a timeout, mirroring the sibling test's `drop(client)` ->
        // `abort()` -> timeout-wrapped join. Pre-fix, `flood_task.await`
        // (which owns `client_writer`, so the main body cannot `drop` it to
        // unstick the flood loop) sat BEFORE `conn_task.abort()`: the flood
        // loop only rechecks its stop flag after an in-flight `send()`
        // completes, and in this test's steady state that send is parked —
        // the server has already stopped reading (it drains only up to the
        // one Snapshot(A) frame the wait loop above needed) and blocks
        // forever in its own `framed.flush().await`, so the flood's `send`
        // never completes either. `cargo test` has no per-test timeout, so
        // that ordering could hang the whole suite. Aborting the connection
        // task FIRST drops its half of the TCP socket, which fails the
        // flood task's in-flight `send()` (broken pipe / reset) and lets it
        // exit; the timeout is a bounded fallback in case OS-level socket
        // teardown is slower than expected in some environment, rather than
        // relying on it alone.
        conn_task.abort();
        tokio::time::timeout(Duration::from_secs(5), flood_task)
            .await
            .expect("flood task must exit once the connection is aborted")
            .expect("flood task must not panic");
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                for p in producers {
                    p.join().unwrap();
                }
            }),
        )
        .await
        .expect("producer threads must exit once the channel closes")
        .expect("spawn_blocking must not panic");
    }

    // ── task0001 (mux-connection-input-freeze): AC-1 regression test ──

    /// AC-1 (FR1/FR5): the daemon connection task's drain arm must not park
    /// the WHOLE `select!` loop when the socket's send buffer is full. This
    /// is VERIFICATION TS1 / the regression test for the residual freeze
    /// this feature closes.
    ///
    /// Floods pane A's output channel via background `blocking_send`
    /// threads (same shape as the sibling task0003/task0004 tests above)
    /// over a SMALL duplex whose server->client capacity saturates almost
    /// immediately once the client stops reading, then sends `PtyInput`
    /// for a DIFFERENT pane (B) and asserts it is processed — observable
    /// directly at pane B's captured writer, NOT via a reply frame (the
    /// client never reads again after draining the reattach handshake, so
    /// a reply-frame-based assertion could never resolve either way) —
    /// within the named 5s timeout.
    ///
    /// Pre-task0001 this test fails: the drain arm's
    /// `framed.feed`/`framed.flush` calls block the WHOLE connection task
    /// once the duplex buffer fills (nobody is reading), so `framed.next()`
    /// never gets polled again and pane B's `PtyInput` is never processed
    /// — the test's own timeout fires.
    #[tokio::test]
    async fn connection_level_client_input_processed_while_outbound_socket_saturated() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        const PANE_B: u32 = 2;
        let captured_input: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        {
            let mut mgr = session_manager.lock().await;
            let sid = mgr.create_session("default".to_string());
            let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);

            let target_b: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_b = MuxPane::new_test_with_writer(
                PANE_B,
                80,
                24,
                target_b,
                Box::new(CapturingWriter(captured_input.clone())),
            );
            mgr.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_b);
        }

        // Small duplex: comfortably fits the handshake/reattach exchange
        // but saturates almost immediately once the flood begins and the
        // client stops reading (matches the sibling AC-3/AC-4 tests'
        // duplex sizing above, for the identical reason).
        let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(handle_connection(
            server_stream,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());

        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);
        let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
        let session_id = match welcome_payload {
            WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
            WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
        };

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A, PANE_B]).await;

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        // Background OS threads saturating pane A's channel via
        // `blocking_send` — same shape as the sibling AC-3/AC-4 tests
        // above (mirrors `pty_spawn.rs`'s reader thread).
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut producers = Vec::new();
        for _ in 0..8 {
            let stop_clone = stop.clone();
            let producer_tx = owned_tx.clone();
            producers.push(std::thread::spawn(move || {
                while !stop_clone.load(StdOrdering::Relaxed) {
                    if producer_tx
                        .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }

        // The client deliberately stops reading from here on (AC-1's own
        // scenario): give the flood time to genuinely saturate the
        // outbound socket (small duplex, nobody draining it) before
        // sending the probe message.
        tokio::time::sleep(Duration::from_millis(200)).await;

        client
            .send(MuxMessage {
                msg_type: MessageType::PtyInput,
                pane_id: PANE_B,
                payload: b"hi".to_vec(),
            })
            .await
            .expect(
                "client->server direction is independent of the saturated \
                 server->client direction",
            );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if *captured_input.lock().unwrap() == b"hi" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "AC-1: PtyInput for pane B must be processed within the named 5s \
                 timeout despite pane A's output channel and the outbound socket \
                 both being saturated"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        stop.store(true, StdOrdering::Relaxed);
        drop(client);
        conn_task.abort();
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                for p in producers {
                    p.join().unwrap();
                }
            }),
        )
        .await
        .expect("producer threads must exit once the channel closes")
        .expect("spawn_blocking must not panic");
    }

    /// AC-3 (FR4): when the outbound admission path is saturated, the
    /// connection STOPS consuming `pane_output_rx` — upstream backpressure
    /// is directly observable (the channel's own reported capacity drops
    /// by exactly what was sent and then stays put) — rather than
    /// continuing to drain it into some other, unbounded buffer.
    ///
    /// The writer task ALWAYS eagerly dequeues whatever `outbound_tx`
    /// currently holds the instant it is scheduled (freeing that channel
    /// capacity immediately, independent of how long the actual socket
    /// write of that dequeued batch then takes) — so a single slow-to-flush
    /// chunk alone does not keep `outbound_tx` looking "full" to a `try_send`
    /// arriving later. Saturating `outbound_tx` itself (`OUTBOUND_QUEUE_CAPACITY`
    /// = 2) therefore needs more DISTINCT frames admitted in ONE drain-arm
    /// iteration than fit — three different, non-mergeable pane ids' chunks
    /// queued into `pane_output_rx` back-to-back (no `.await` yield point in
    /// between: each `send()` resolves immediately since `pane_output_tx`
    /// has ample capacity, so the writer task never gets scheduled in
    /// between on this test's single-threaded runtime) does that
    /// deterministically. Only pane A is a REAL registered pane (needed for
    /// Attach); the other two ids are fictional — `PtyOutput` classification
    /// builds a frame straight from the chunk's own `pane_id`/`data`, with
    /// no `SessionManager` lookup, so this is a faithful, minimal probe of
    /// the admission mechanism alone.
    #[tokio::test]
    async fn connection_level_stops_consuming_pane_output_rx_when_outbound_queue_saturated() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        let session_id;
        {
            let mut mgr = session_manager.lock().await;
            session_id = mgr.create_session("default".to_string());
            let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);
        }

        // Tiny duplex, exactly as AC-5's test: a single chunk past this is
        // enough to get the writer's flush genuinely stuck.
        let (server_stream, client_stream) = tokio::io::duplex(256);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(handle_connection(
            server_stream,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());
        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A]).await;

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        const PANE_B: u32 = 2;
        const PANE_C: u32 = 3;

        let capacity_before = owned_tx.capacity();

        // Phase 1: get the writer's own `flush()` genuinely, permanently
        // stuck (client never reads again from here on) — ONE oversized
        // chunk against the 256-byte duplex. This alone does NOT yet
        // saturate `outbound_tx` itself (the writer eagerly dequeues the
        // instant it is scheduled, freeing that one channel slot
        // immediately — see this test's own doc), but it DOES mean the
        // writer never calls `recv()` again afterward, so whatever gets
        // admitted next stays admitted (and any remainder stays held) for
        // good.
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
            .await
            .expect("pane_output_tx must accept the saturating chunk");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            owned_tx.capacity(),
            capacity_before,
            "the saturating chunk must have been consumed (capacity restored) — \
             the writer is now stuck flushing it, not the channel holding it"
        );

        // Phase 2: three DISTINCT (non-mergeable — different pane ids)
        // chunks, queued into `pane_output_rx` back-to-back with NO
        // intervening yield point: each `send()` resolves immediately
        // (`pane_output_tx` has ample capacity), so nothing else gets
        // scheduled in between. The drain arm's next run sees all three at
        // once and tries to admit three distinct frames into `outbound_tx`
        // (capacity 2, both slots free since the writer already dequeued
        // phase 1's chunk) in ONE synchronous, non-blocking pass — the 3rd
        // has nowhere to go and becomes `outbound_remainder`. Because the
        // writer is stuck on phase 1's flush (client never reads), it never
        // calls `recv()` again, so this remainder is never cleared either.
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
            .await
            .expect("pane_output_tx must accept the first chunk");
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
            .await
            .expect("pane_output_tx must accept the second chunk");
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_C, vec![b'c'; 16]))
            .await
            .expect("pane_output_tx must accept the third chunk");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            owned_tx.capacity(),
            capacity_before,
            "AC-3: all three chunks must have been CONSUMED from pane_output_rx \
             (capacity restored) — the unsent one lives in the connection's own \
             outbound_remainder state, not left sitting in pane_output_rx"
        );

        // AC-3: with a genuine remainder held, `pane_output_rx` must now
        // sit UNCONSUMED — the drain arm's own guard (`if
        // outbound_remainder.is_empty()`) excludes it from the loop.
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, vec![b'd'; 16]))
            .await
            .expect("pane_output_tx must still accept sends up to its own capacity");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            owned_tx.capacity(),
            capacity_before - 1,
            "AC-3: a chunk sent while an outbound remainder is held must sit \
             UNCONSUMED in pane_output_rx (capacity down by exactly the one \
             chunk sent), not be drained into some other, unbounded buffer"
        );

        // And stays put — the drain arm has genuinely STOPPED, not merely
        // fallen behind.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            owned_tx.capacity(),
            capacity_before - 1,
            "AC-3: pane_output_rx's backlog must remain UNCONSUMED (stable, not \
             shrinking) while the outbound path stays saturated — this IS the \
             upstream backpressure propagation FR4 requires"
        );

        drop(client);
        conn_task.abort();
    }

    /// AC-4 (NFR4): a PTY-exit chunk consumed by the drain arm still reaps
    /// the pane even when delivery of the corresponding `PtyExited` frame
    /// to the client CANNOT succeed — reap is decided at consumption time
    /// (Design invariant 5), independent of the outbound writer's outcome.
    ///
    /// Uses [`FailableWriteStream`] rather than a saturated-but-recoverable
    /// duplex (AC-1's harness): flipping `fail_writes` makes delivery
    /// genuinely, permanently impossible from that point on, which is a
    /// sharper proof than "delivery hasn't happened YET" — it can never
    /// happen at all, so a reap gated on delivery succeeding would never
    /// fire.
    #[tokio::test]
    async fn connection_level_pty_exit_reaps_pane_even_when_delivery_can_never_succeed() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        let session_id;
        {
            let mut mgr = session_manager.lock().await;
            session_id = mgr.create_session("default".to_string());
            let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);
        }

        let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
        let fail_writes = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wrapped_server = FailableWriteStream {
            inner: server_stream,
            fail_writes: fail_writes.clone(),
        };
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(handle_connection(
            wrapped_server,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());
        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A]).await;

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        // From here on, delivery of ANY client-bound frame is permanently
        // impossible.
        fail_writes.store(true, StdOrdering::Relaxed);

        // PTY-exit signal: empty-data chunk.
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, Vec::new()))
            .await
            .expect("pane_output_tx must still accept the exit chunk");

        // AC-4: the pane must be reaped (removed from the SessionManager)
        // within a bounded time, even though the resulting `PtyExited`
        // frame can never be delivered.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if session_manager.lock().await.is_empty() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "AC-4: the exited pane must be reaped within a bounded time \
                 regardless of whether delivery to the client can succeed"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        drop(client);
        conn_task.abort();
    }

    /// AC-5 (NFR4): the `Upgrading` ack does not fire while the frame is
    /// merely QUEUED behind a saturated socket, and fires once it has
    /// actually been written AND flushed (socket drained) — not merely
    /// admitted into the outbound queue.
    ///
    /// Deliberately a SMALL saturation, unlike AC-1/AC-2's multi-thread
    /// flood: a single oversized chunk against a tiny duplex is enough to
    /// get the writer's `flush()` genuinely stuck (nothing drains it while
    /// the client doesn't read), and keeps the backlog the client must
    /// drain to observe the ack small and bounded — a multi-thread flood
    /// here would keep re-winning the outbound queue's fair admission
    /// ahead of the Upgrading notification for as long as it kept
    /// producing, making the bound on "how much has to drain before the
    /// ack fires" open-ended rather than the single small chunk this test
    /// actually needs to prove the ordering.
    #[tokio::test]
    async fn connection_level_upgrading_ack_fires_only_after_flush_not_at_admission() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        let session_id;
        {
            let mut mgr = session_manager.lock().await;
            session_id = mgr.create_session("default".to_string());
            let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
            let target_a: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                    reason: DetachReason::NetworkDetach,
                    owner: None,
                }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
            mgr.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);
        }

        // Tiny duplex: still comfortably fits the handshake/attach
        // exchange (the client drains those promptly), but a single
        // few-KB chunk sent afterward already exceeds it.
        let (server_stream, client_stream) = tokio::io::duplex(256);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        // Real ack channel, installed into the slot up front (mirrors
        // `mux::daemon::prepare_upgrade`'s own wiring).
        let (ack_tx, mut ack_rx) = mpsc::channel::<()>(4);
        let upgrade_ack_slot: SharedUpgradeAckSlot = Arc::new(StdMutex::new(Some(ack_tx)));

        let conn_task = tokio::spawn(handle_connection(
            server_stream,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            upgrade_ack_slot,
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());
        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A]).await;

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        // A single chunk (a few KB, well past the 256-byte duplex) is
        // enough to get the writer's `flush()` genuinely stuck once the
        // client stops reading — no background flood needed.
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
            .await
            .expect("pane_output_tx must accept the saturating chunk");
        // Give the drain arm / writer time to actually reach the stuck
        // `flush()` before forwarding the notification.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Forward the Upgrading announcement (mirrors
        // `mux::daemon::prepare_upgrade`'s own construction) while the
        // socket is saturated.
        {
            let mgr = session_manager.lock().await;
            let upgrading = MuxMessage {
                msg_type: MessageType::Upgrading,
                pane_id: 0,
                payload: Vec::new(),
            };
            let _ = mgr.notify_tx().send(upgrading);
        }

        // AC-5: the ack must NOT have fired yet — the frame is, at best,
        // queued behind a saturated socket.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            ack_rx.try_recv().is_err(),
            "AC-5: the Upgrading ack must not fire while the frame is queued behind \
             a saturated socket"
        );

        // Resume client reads until the Upgrading frame itself is
        // observed — only the one saturating chunk plus the Upgrading
        // frame need to drain, a small, bounded backlog.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "must observe the Upgrading frame within a bounded time once reads resume"
            );
            let msg = tokio::time::timeout(remaining, client.next())
                .await
                .expect("must not hang")
                .expect("stream must not end")
                .expect("frame must decode");
            if msg.msg_type == MessageType::Upgrading {
                break;
            }
        }

        // AC-5: now that the frame has actually been written AND flushed,
        // the ack must fire, within a bounded time.
        tokio::time::timeout(Duration::from_secs(5), ack_rx.recv())
            .await
            .expect("must not hang waiting for the ack")
            .expect("ack channel must not be closed");

        drop(client);
        conn_task.abort();
    }

    /// AC-6: a socket write failure terminates the connection loop with
    /// today's outcome — the loop exits and panes detach via the existing
    /// teardown path (`detach_session_panes`, NOT the reap path AC-4
    /// covers: the pane itself never exited here, only the socket did).
    #[tokio::test]
    async fn connection_level_socket_write_failure_terminates_loop_and_detaches_panes() {
        let session_manager = Arc::new(Mutex::new(SessionManager::new()));
        const PANE_A: u32 = 1;
        let session_id;
        let target_a: SharedOutputTarget;
        {
            let mut mgr = session_manager.lock().await;
            session_id = mgr.create_session("default".to_string());
            let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
            target_a = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                reason: DetachReason::NetworkDetach,
                owner: None,
            }));
            let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a.clone());
            mgr.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane_a);
        }

        let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
        let fail_writes = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wrapped_server = FailableWriteStream {
            inner: server_stream,
            fail_writes: fail_writes.clone(),
        };
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(handle_connection(
            wrapped_server,
            session_manager.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        ));

        let mut client = Framed::new(client_stream, MuxCodec::new());
        client
            .send(MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ))
            .await
            .unwrap();
        let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang on Welcome")
            .expect("stream must not end")
            .expect("frame must decode");
        assert_eq!(welcome.msg_type, MessageType::Welcome);

        client
            .send(MuxMessage::control(
                MessageType::Attach,
                0,
                &AttachMsg { session_id },
            ))
            .await
            .unwrap();
        drain_until_pane_created(&mut client, &[PANE_A]).await;
        assert!(matches!(
            *target_a.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));

        let owned_tx: mpsc::Sender<PtyOutputChunk> = {
            let mgr = session_manager.lock().await;
            let pane = mgr
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .get(&PANE_A)
                .unwrap();
            match &*pane.output_target.lock().unwrap() {
                PaneOutputTarget::Connected(tx) => tx.clone(),
                PaneOutputTarget::Detached { .. } => {
                    panic!("pane A must be Connected after attach, still Detached")
                }
            }
        };

        // Inject the write failure, then produce SOME output so the
        // writer actually attempts (and fails) a write.
        fail_writes.store(true, StdOrdering::Relaxed);
        owned_tx
            .send(PtyOutputChunk::pty_output(PANE_A, b"trigger".to_vec()))
            .await
            .expect("pane_output_tx must still accept the chunk");

        // AC-6: the connection loop must exit (the spawned task
        // completes) within a bounded time.
        tokio::time::timeout(Duration::from_secs(5), conn_task)
            .await
            .expect("connection loop must terminate after a socket write failure")
            .expect("handle_connection task must not panic");

        // AC-6: panes detach via the existing teardown path — the pane
        // itself never exited (only the socket did), so it must be back
        // to Detached, not reaped.
        assert!(
            matches!(*target_a.lock().unwrap(), PaneOutputTarget::Detached { .. }),
            "AC-6: a socket write failure must detach panes via the existing \
             teardown path, the same outcome as today"
        );
        assert!(
            !session_manager.lock().await.is_empty(),
            "AC-6: the pane itself never exited, so it must NOT be reaped — only \
             detached"
        );

        drop(client);
    }
}
