//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

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
    OUTBOUND_QUEUE_CAPACITY, OutboundAdmission, OutboundHandle, ReplySink, run_outbound_writer,
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
/// (task0001 Design invariant 7; task0003 AC-5 rework): once the loop
/// breaks (kick, client EOF, error), `OutboundAdmission::teardown_flush`
/// admits whatever remainder is still held (e.g. a kick arm's `Detached`,
/// appended to the tail of a held remainder) and then drops its sender, so
/// the outbound writer drains everything now-admitted and exits; this
/// single budget bounds BOTH that flush and the subsequent wait for the
/// writer's `JoinHandle` before `handle_connection` abandons it and
/// returns anyway. Short and named — this is cleanup after the connection
/// is already ending, not a correctness-critical wait.
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

    // Outbound admission (task0003, FR1/FR3/FR4 — consolidated single
    // component, see `outbound::OutboundAdmission`'s doc): owns the
    // bounded queue sender, the held remainder, and the in-flight fair
    // reservation together. EVERY client-bound frame producer on this loop
    // — the drain arm, the notify arm (single forward + Lagged resync,
    // gated off while holding), the kick arm's `Detached` (may append to a
    // held remainder), and `route_message`'s replies/reattach frames
    // (ordered blocking admission that drains any held remainder first) —
    // funnels through this ONE instance; the raw `mpsc::Sender` is never
    // reachable outside `outbound.rs`. Restored bound: while a remainder
    // is held, no producer can grow it in proportion to notification or
    // client-message traffic — the worst case is one drain batch
    // (`DRAIN_BATCH_LIMIT`, ~4MB) OR one Lagged resync's window count,
    // plus at most one appended kick `Detached` frame (see
    // `OUTBOUND_QUEUE_CAPACITY`'s doc for the combined worst-case
    // accounting).
    let mut admission = OutboundAdmission::new(outbound_tx);

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
        // task0001 (Design "Starvation-guard interaction"), task0003
        // rework: a held outbound remainder or an armed outbound-capacity
        // reservation is likewise outstanding output work — the SAME class
        // of "admitted but not yet delivered" gap `has_unforwarded_pane_output`
        // already closes for `pane_output_tx`, now extended to the
        // outbound admission component this task consolidates, so
        // continuous client traffic cannot starve draining the outbound
        // remainder either. `admission.is_holding()` is the single
        // predicate that replaces the old
        // `!outbound_remainder.is_empty() || pending_outbound_reserve.is_some()`
        // pair (see `OutboundAdmission`'s doc for why the two were always
        // kept in lockstep).
        let has_pending_output = has_unforwarded_pane_output(has_deferred_work, &pane_output_rx)
            || admission.is_holding();
        // Review round (finding `9537da352469bc68`): the guard must only
        // exclude the client arm when SOME OTHER arm can still make
        // progress without the client reading from the socket. A held
        // outbound remainder does NOT qualify: draining it requires the
        // `outbound_permit_result` arm to resolve, which in turn requires
        // the writer to feed+flush the socket, which in turn requires the
        // client to read — so on a quota-exhausted iteration with only a
        // remainder outstanding (no unforwarded pane output), excluding
        // the client arm would leave `select!` with zero arms able to
        // become ready without new client-side activity, re-creating the
        // exact freeze this task closes. `has_unforwarded_pane_output`
        // (deferred pane output waiting to be forwarded) DOES qualify: the
        // drain/permit machinery for `pane_output_tx` can progress on its
        // own once the corresponding permit or send resolves, independent
        // of the client. So the guard uses a narrower "excludable work"
        // signal than `has_pending_output` above (which intentionally
        // keeps counting the remainder for the starvation-counter
        // bookkeeping below `select!`).
        let excludable_work = has_unforwarded_pane_output(has_deferred_work, &pane_output_rx)
            && !admission.is_holding();
        let allow_client_arm =
            allow_client_message_arm(excludable_work, consecutive_client_msgs_while_deferred);
        let mut took_client_arm = false;

        tokio::select! {
            // biased: text order below IS the priority order, subject to
            // the starvation guard on the very first arm (`allow_client_arm`,
            // computed just above). See that arm's own doc for the full G2
            // design decision.
            biased;

            // Review round (finding `c1-outbound-remainder-starvation`):
            // listed FIRST, ahead of the client-message arm below, so it is
            // polled — and therefore registered as a waiter on the outbound
            // queue's semaphore — on every single loop iteration,
            // independent of whatever `allow_client_arm` decides for the
            // client arm. Mirrors the `permit_result` arm's own placement
            // requirement relative to the drain arm (see that arm's doc
            // below), applied one layer further out: a `try_send`-only
            // retry can never win fairly against a reader thread's
            // `blocking_send` waiter already parked on the channel, so this
            // fair `reserve_owned()` future is the only thing that can ever
            // drain `admission`'s held remainder, and it must be polled
            // unconditionally for that guarantee to hold. Costs nothing
            // when not armed (`if admission.has_pending_reserve()`
            // short-circuits to `Pending` instantly) or when armed but not
            // yet resolved (ditto) — the client arm and every arm below
            // still get serviced this same iteration.
            outbound_permit_result = admission.poll_pending_reserve(),
                if admission.has_pending_reserve() => {
                // `apply_reserve_result` clears/re-arms internally
                // (mirrors `arm_pending_deferred_reserve`'s own re-arm) —
                // see `OutboundAdmission`'s doc.
                admission.apply_reserve_result(outbound_permit_result);
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
                            &mut admission,
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
                // task0001 (best-effort, mirrors the pre-task0001
                // `let _ = framed.send(...).await;` shape — errors ignored
                // either way). FR3 (no overtaking): `push_or_admit` appends
                // to the tail of a held remainder (preserving arrival
                // order) or admits immediately when nothing is held — the
                // ONE frame this arm ever contributes (it breaks the loop
                // immediately after). This is the last thing this arm does
                // before `break`, so it does not reintroduce the residual
                // freeze.
                admission.push_or_admit(resp);
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
            // empty again. task0003: `admission.is_holding()` replaces the
            // old `outbound_remainder.is_empty()` check (see
            // `OutboundAdmission`'s doc for the equivalence).
            chunk = pane_output_rx.recv(), if !admission.is_holding() => {
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
                    // for, in order; a non-empty remainder is held inside
                    // `admission` and its fair capacity-acquisition future
                    // is armed so it is serviced ahead of any newer chunk
                    // (FR3) — this arm never awaits outbound capacity at
                    // this point position, which is exactly the self-block
                    // this task removes.
                    admission.try_admit(frames);

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
            // task0003 (FR4, Design invariant 3): gated off while a
            // remainder is held — SAME gate shape as the drain arm above.
            // Notifications then simply wait in the broadcast channel
            // (fixed capacity); overflow takes the existing Lagged resync
            // path once the arm resumes (a resync starting from an empty
            // remainder contributes at most the current window count, not
            // one frame per notification that arrived while gated off).
            // With this gate in place, a held remainder is GUARANTEED
            // empty on entry to this arm's body, so the single-forward
            // path's old "append to a non-empty remainder" branch is
            // unreachable and removed rather than left as dead code — see
            // `push_or_admit`'s own doc for why it is still needed for the
            // Lagged resync loop below (remainder CAN become non-empty
            // partway through that loop, across its own iterations).
            notification = notify_rx.recv(), if !admission.is_holding() => {
                match notification {
                    Ok(msg) => {
                        // Forward cross-client notification (e.g., CLI SwitchWindow) to GUI
                        log::info!("Forwarding notification to GUI: {:?} pane={}", msg.msg_type, msg.pane_id);
                        // task0001 (Design invariant 1 — "MAY await
                        // admission", not the drain arm). The Upgrading-ack
                        // fire (AC-7 / NFR4) now happens inside the writer
                        // task itself, once the frame is actually written
                        // AND flushed — see `outbound::run_outbound_writer`'s
                        // doc — rather than here at admission time.
                        //
                        // A closed outbound channel is not detected here
                        // (no synchronous send to observe the error on) —
                        // the `writer_result` arm above independently
                        // detects writer exit and tears the connection
                        // down on the next iteration, so no producer needs
                        // to re-detect it here.
                        admission.push_or_admit(msg);
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
                                // `push_or_admit` (not `try_admit`): the
                                // gate above only guarantees the remainder
                                // was empty when THIS arm started — the
                                // outbound queue can fill partway through
                                // this multi-frame resync loop, at which
                                // point later iterations must append rather
                                // than overtake.
                                admission.push_or_admit(msg);
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

    // task0001/task0003 (Design invariant 4, AC-5): a socket write/flush
    // failure inside the writer already tore this loop down via the
    // `writer_result` arm above (AC-6) — the writer has already exited, so
    // there is nothing further to flush. Otherwise: first perform a
    // bounded best-effort FIFO admission of whatever remainder `admission`
    // still holds (a kicked client's Detached, appended to the tail of a
    // held remainder by the kick arm above, is included here — so a
    // client that resumes reading receives the held frames followed by
    // Detached, in order), THEN release every sender this loop still
    // holds (`OutboundAdmission::teardown_flush` consumes `admission`,
    // dropping its sender) so `run_outbound_writer`'s `outbound_rx.recv()`
    // observes the channel closed and exits gracefully. Both steps run
    // inside the SAME named teardown-flush timeout: a slow/never-reading
    // client bounds the whole sequence, not just the writer join — frames
    // that do not fit within the budget are dropped exactly as the
    // pre-task0003 already-admitted-only flush would have dropped them
    // (best-effort, not absolute).
    if !writer_task_failed {
        let flush_and_join = async {
            admission.teardown_flush().await;
            writer_task_handle.await
        };
        match tokio::time::timeout(OUTBOUND_TEARDOWN_FLUSH_TIMEOUT, flush_and_join).await {
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
/// `admission` (task0001, task0003 rework): every reply/frame this
/// function (or a handler it calls) sends goes through the GUI loop's
/// SINGLE outbound admission component — this function is only ever
/// called from that loop, never the CLI-client path, so it is no longer
/// generic over a raw stream type at all. Every send below is therefore an
/// ordered BLOCKING admission (`OutboundAdmission::admit_blocking`, via
/// [`OutboundHandle`] or directly): it drains any held remainder first
/// (FR3, no overtaking), then admits its own frame(s) — see
/// `OutboundAdmission`'s doc for why blocking here is an accepted
/// carve-out (task0001 invariant 1) rather than the point-position
/// capacity-await Convention 1 forbids elsewhere.
#[allow(clippy::too_many_arguments)]
async fn route_message(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    admission: &mut OutboundAdmission,
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
                &mut OutboundHandle::new(admission),
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
                admission,
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
            let _ = admission.admit_blocking(vec![resp]).await;
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
            handle_upgrade_request(&mut OutboundHandle::new(admission), upgrade_tx).await;
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
mod tests;
