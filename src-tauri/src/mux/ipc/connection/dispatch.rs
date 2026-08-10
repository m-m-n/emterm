//! Post-handshake message dispatch: the single-shot CLI-client control
//! loop, upgrade-request relay, and the GUI-loop message router.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use mux_ipc::protocol::{ErrorMsg, MessageType, MuxMessage, SetVisibilityPayload};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::codec::Framed;

use super::UPGRADE_PREPARE_TIMEOUT;
use crate::mux::daemon::{UpgradeSignal, UpgradeSignalSender};
use crate::mux::ipc::codec::MuxCodec;
use crate::mux::ipc::handlers::{
    flush_deferred_output, handle_attach, handle_create_window, handle_destroy_pane,
    handle_destroy_window, handle_move_window, handle_read_pane, handle_rename_window,
    handle_request_pane_snapshot, handle_resize, handle_send_text, handle_set_visibility,
    handle_switch_window, handle_wait_agent_state,
};
use crate::mux::ipc::outbound::{OutboundAdmission, OutboundHandle, ReplySink};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatusReportSender, DeferredOutputQueue, NotificationSender, PtyOutputChunk,
    SharedPaneExitSender, TitleChangeSender,
};

/// Handle a CLI client after handshake.
///
/// Reads at most one control message (e.g., CreateWindow), processes it,
/// sends a response, and disconnects. If no message arrives within 5 seconds,
/// disconnects gracefully (this is the normal `mux ls` path).
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_cli_client<S>(
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
pub(super) fn upgrade_reply_to_message(
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
pub(super) async fn route_message(
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
