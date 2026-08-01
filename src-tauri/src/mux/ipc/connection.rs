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
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::handlers::{
    apply_fair_permit_to_front_deferred_item, flush_deferred_output, handle_attach,
    handle_create_window, handle_destroy_pane, handle_destroy_window, handle_move_window,
    handle_read_pane, handle_rename_window, handle_request_pane_snapshot, handle_resize,
    handle_send_text, handle_set_visibility, handle_switch_window, handle_wait_agent_state,
};
use super::protocol::*;
use super::reattach::detach_session_panes;
use super::statusbar::{StatusBarEngine, execute_command};
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

    // Status bar engine setup
    let active_pane_id: super::statusbar::SharedActivePaneId =
        Arc::new(std::sync::Mutex::new(None));
    let pane_cwd_map: super::statusbar::SharedPaneCwdMap =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut statusbar_engine = StatusBarEngine::new(active_pane_id.clone(), pane_cwd_map.clone());

    // Send initial error message if settings failed to load
    if let Some(err_msg) = statusbar_engine.initial_error_update() {
        let _ = framed.send(err_msg).await;
    }

    // Set up status bar timers only if enabled and templates contain variables
    let statusbar_enabled = statusbar_engine.is_enabled();
    let mut render_interval = if statusbar_enabled && statusbar_engine.has_template_variables() {
        Some(statusbar_engine.render_interval())
    } else {
        None
    };
    let command_intervals = if statusbar_enabled {
        statusbar_engine.command_intervals()
    } else {
        Vec::new()
    };

    // Create per-command timers using mpsc channel for aggregation.
    // Each command timer runs as a separate task, sending its name when it fires.
    let (cmd_tick_tx, mut cmd_tick_rx) = mpsc::channel::<String>(16);
    for (name, dur) in command_intervals {
        // Trigger immediate first execution so status bar populates without waiting
        let _ = cmd_tick_tx.try_send(name.clone());
        let tx = cmd_tick_tx.clone();
        let cmd_name = name.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(dur);
            interval.tick().await; // skip first immediate tick (already sent above)
            loop {
                interval.tick().await;
                if tx.send(cmd_name.clone()).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(cmd_tick_tx); // Drop the original sender; spawned tasks hold clones

    // Channel for receiving command execution results from spawned tasks
    let (cmd_result_tx, mut cmd_result_rx) = mpsc::channel::<(String, Option<String>)>(16);

    // Per-command JoinHandle for single-flight control: skip if previous execution is still running
    let mut cmd_handles: std::collections::HashMap<String, tokio::task::JoinHandle<()>> =
        std::collections::HashMap::new();

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

    // Message + output loop using select! to handle both directions concurrently
    loop {
        // Build a future for the render timer (if enabled)
        let render_tick = async {
            if let Some(ref mut interval) = render_interval {
                interval.tick().await;
            } else {
                // Never resolves if disabled
                std::future::pending::<()>().await;
            }
        };

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

        tokio::select! {
            // biased: prioritize client messages (PtyInput) over PTY output
            biased;

            msg = framed.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        // Track active pane from PtyInput and SwitchWindow messages
                        if msg.msg_type == MessageType::PtyInput
                            || msg.msg_type == MessageType::SwitchWindow
                        {
                            *active_pane_id.lock().unwrap() = Some(msg.pane_id);
                        }

                        if let Err(should_break) = route_message(
                            msg,
                            &session_manager,
                            &mut framed,
                            &pane_output_tx,
                            &mut active_session_id,
                            &shutdown_tx,
                            &mut statusbar_engine,
                            &pane_cwd_map,
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
                let _ = framed.send(resp).await;
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
            chunk = pane_output_rx.recv() => {
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

                    // Batch send: feed all into buffer, then flush once.
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
                    let mut send_err = false;
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
                        if framed.feed(msg).await.is_err() {
                            log::warn!("pty-batch feed error: merged_count={}", merged_count);
                            send_err = true;
                            break;
                        }
                    }
                    let flush_failed = framed.flush().await.is_err();
                    // Reap each exited pane from the daemon's own SessionManager
                    // *regardless* of whether delivery to the client succeeded:
                    // the PTY genuinely exited, so the empty window / session must
                    // be removed and the daemon shut down once the last pane is
                    // gone. Gating this on a successful flush would re-open the
                    // zombie-pane bug under a client-drop race — the GUI window
                    // closing at the same moment the last shell exits via Ctrl+D
                    // both delivers the exit chunk and fails the client write, so
                    // a success-gated reap would skip cleanup and leave a session
                    // that never auto-shuts-down / can't be `mux kill`ed. Mirror
                    // the explicit `DestroyPane` cleanup fully — including dropping
                    // the pane's cwd entry from the status-bar map so it can't
                    // resolve a stale cwd for a dead pane.
                    for pane_id in exited_panes {
                        pane_cwd_map.lock().unwrap().remove(&pane_id);
                        handle_destroy_pane(pane_id, &session_manager, &shutdown_tx).await;
                    }
                    if send_err || flush_failed {
                        if !send_err {
                            log::warn!("pty-batch flush error: merged_count={}", merged_count);
                        }
                        break;
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
                    // spawned task per deferral).
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
            _ = render_tick => {
                if let Some(update_msg) = statusbar_engine.render() {
                    if framed.send(update_msg).await.is_err() {
                        break;
                    }
                }
            }
            Some(cmd_name) = cmd_tick_rx.recv() => {
                // Single-flight: skip if previous execution is still running
                if let Some(handle) = cmd_handles.get(&cmd_name) {
                    if !handle.is_finished() {
                        log::debug!("Skipping command '{}': previous execution still running", cmd_name);
                        continue;
                    }
                }
                if let Some(executable) = statusbar_engine.get_command_executable(&cmd_name) {
                    let tx = cmd_result_tx.clone();
                    let cwd = statusbar_engine.active_cwd();
                    let name = cmd_name.clone();
                    let handle = tokio::spawn(async move {
                        let result = execute_command(&executable, &cwd).await;
                        let _ = tx.send((name, result)).await;
                    });
                    cmd_handles.insert(cmd_name, handle);
                }
            }
            Some((name, output)) = cmd_result_rx.recv() => {
                statusbar_engine.update_command_cache(&name, output);
            }
            notification = notify_rx.recv() => {
                match notification {
                    Ok(msg) => {
                        // Forward cross-client notification (e.g., CLI SwitchWindow) to GUI
                        log::info!("Forwarding notification to GUI: {:?} pane={}", msg.msg_type, msg.pane_id);
                        if msg.msg_type == MessageType::SwitchWindow {
                            *active_pane_id.lock().unwrap() = Some(msg.pane_id);
                            if statusbar_engine.is_enabled() {
                                // Send SwitchWindow + status bar update as a batch
                                if framed.feed(msg).await.is_err() {
                                    break;
                                }
                                let update_msg = statusbar_engine.force_render();
                                if framed.send(update_msg).await.is_err() {
                                    break;
                                }
                            } else if framed.send(msg).await.is_err() {
                                break;
                            }
                        } else {
                            // AC-7 (Design "Announcement delivery"): once an
                            // `Upgrading` frame is actually written to THIS
                            // socket, acknowledge it so `prepare_upgrade` can
                            // observe delivery (not merely queueing) before
                            // the runtime is torn down. Every other message
                            // type never touches the slot.
                            let is_upgrading = msg.msg_type == MessageType::Upgrading;
                            if framed.send(msg).await.is_err() {
                                break;
                            }
                            if is_upgrading {
                                let ack_tx = upgrade_ack_slot.lock().unwrap().clone();
                                if let Some(ack_tx) = ack_tx {
                                    let _ = ack_tx.try_send(());
                                }
                            }
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
                                if framed.send(msg).await.is_err() {
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
        // framed.next() wins over kick_fut in the select!, or where the socket
        // fails mid-eviction and we exit without observing the kick.
        detach_session_panes(&session_manager, active_session_id, &pane_output_tx).await;

        log::info!(
            "Client disconnected, session {} panes detached",
            active_session_id
        );
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
async fn handle_upgrade_request<S>(
    framed: &mut Framed<S, MuxCodec>,
    upgrade_tx: &UpgradeSignalSender,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
        let _ = framed.send(resp).await;
        return;
    }

    match tokio::time::timeout(UPGRADE_PREPARE_TIMEOUT, reply_rx).await {
        Ok(recv_result) => {
            if let Some(msg) = upgrade_reply_to_message(recv_result) {
                let _ = framed.send(msg).await;
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
            let _ = framed.send(resp).await;
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
/// (`handle_connection`, the `msg = framed.next() =>` arm), so any call it
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
/// The opportunistic `flush_deferred_output` call at the top (task0002) is
/// defensive: `handle_connection`'s own drain of `pane_output_rx` is the
/// primary trigger (capacity can only free there), but giving every
/// incoming client message a chance to progress the queue too closes the
/// residual edge case of PTY output going quiet right after a deferral.
#[allow(clippy::too_many_arguments)]
async fn route_message<S>(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    statusbar_engine: &mut StatusBarEngine,
    pane_cwd_map: &super::statusbar::SharedPaneCwdMap,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
    kick_rx: &mut Option<oneshot::Receiver<()>>,
    visible_state: &Arc<AtomicBool>,
    upgrade_tx: &UpgradeSignalSender,
    deferred_output: &mut DeferredOutputQueue,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
                framed,
                pane_output_tx,
                *active_session_id,
                title_tx,
                notification_tx,
                agent_status_tx,
                pane_exit_sender,
            )
            .await?;
            // Register pane cwd Arcs for newly created panes
            register_session_pane_cwds(session_manager, *active_session_id, pane_cwd_map).await;
        }
        MessageType::Attach => {
            handle_attach(
                msg,
                session_manager,
                framed,
                pane_output_tx,
                active_session_id,
                title_tx,
                kick_rx,
                visible_state,
            )
            .await?;
            // Register pane cwd Arcs for all panes in the new session
            register_session_pane_cwds(session_manager, *active_session_id, pane_cwd_map).await;
            // Send status bar content immediately after attach
            if statusbar_engine.is_enabled() {
                let update_msg = statusbar_engine.force_render();
                if framed.send(update_msg).await.is_err() {
                    return Err(false);
                }
            }
        }
        MessageType::Detach => {
            log::info!("Client requested detach");
            let resp = MuxMessage::control(MessageType::Detached, 0, &());
            let _ = framed.send(resp).await;
            return Err(true);
        }
        MessageType::DestroyPane => {
            pane_cwd_map.lock().unwrap().remove(&msg.pane_id);
            handle_destroy_pane(msg.pane_id, session_manager, shutdown_tx).await;
        }
        MessageType::SwitchWindow => {
            handle_switch_window(msg.pane_id, session_manager).await;
            // Force status bar re-render with new pane's cwd
            if statusbar_engine.is_enabled() {
                let update_msg = statusbar_engine.force_render();
                if framed.send(update_msg).await.is_err() {
                    return Err(false);
                }
            }
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
        MessageType::RequestStatusUpdate => {
            let update_msg = statusbar_engine.force_render();
            if framed.send(update_msg).await.is_err() {
                return Err(false);
            }
        }
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
            handle_upgrade_request(framed, upgrade_tx).await;
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

/// Register pane cwd Arcs from session_manager into pane_cwd_map.
/// Called once per pane creation / reattach (very rare), not per output chunk.
async fn register_session_pane_cwds(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_cwd_map: &super::statusbar::SharedPaneCwdMap,
) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        let mut map = pane_cwd_map.lock().unwrap();
        for window in session.windows.values() {
            for pane in window.panes.values() {
                map.entry(pane.id).or_insert_with(|| pane.cwd.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// genuinely, continuously saturated rather than momentarily. While
    /// that saturation is ongoing: a `RequestPaneSnapshot` for pane B is
    /// deferred, then a `PtyInput` for pane B is sent. Asserts, all within
    /// a bounded `tokio::time::timeout`:
    /// - AC-4: the `PtyInput` is processed (the pane's writer observes the
    ///   exact bytes) and pane A's `PtyOutput` keeps arriving (the
    ///   connection's own drain arm keeps running, not stalled).
    /// - AC-3: pane B's deferred `Snapshot` is still delivered — not lost,
    ///   and not starved indefinitely by pane A's continuous producers.
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
                pane_id: PANE_B,
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
        let mut saw_snapshot_for_pane_b = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !(saw_output_for_pane_a && saw_snapshot_for_pane_b) {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "AC-3/AC-4: connection must keep forwarding pane A's output AND \
                 deliver pane B's deferred snapshot within a bounded time despite \
                 sustained saturating output; saw_output_for_pane_a={saw_output_for_pane_a} \
                 saw_snapshot_for_pane_b={saw_snapshot_for_pane_b}"
            );
            let msg = tokio::time::timeout(remaining, client.next())
                .await
                .expect("must not hang")
                .expect("stream must not end")
                .expect("frame must decode");
            match (msg.msg_type, msg.pane_id) {
                (MessageType::PtyOutput, PANE_A) => saw_output_for_pane_a = true,
                (MessageType::Snapshot, PANE_B) => saw_snapshot_for_pane_b = true,
                _ => {}
            }
        }

        // AC-4: the PtyInput must have been processed (the write is
        // synchronous inside `route_message`, strictly before any later
        // frame the loop above already observed).
        assert_eq!(
            *captured_input.lock().unwrap(),
            b"hi",
            "AC-4: PtyInput for pane B must be processed while pane A's output \
             saturates the channel"
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
}
