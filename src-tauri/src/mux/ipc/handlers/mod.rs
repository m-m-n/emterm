//! Message handlers for mux IPC commands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::outbound::{OutboundAdmission, ReplySink};
use super::protocol::*;
use super::pty_spawn::{register_pane_and_start_reader, spawn_pty};
use super::reattach::{
    build_shadow_parser_snapshot, collect_reattach_data, detach_session_panes, send_reattach_data,
};
use crate::agent_status::AgentState as CoreAgentState;
use crate::mux::daemon::{from_wire_state, to_wire_state};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatus, AgentStatusReportSender, AgentWaitOutcome, AgentWaiter, AnyPermit,
    DeferredOutputItem, DeferredOutputQueue, MuxPane, NotificationSender, PaneId, PtyOutputChunk,
    ResumeOutcome, SharedPaneExitSender, SharedScrollback, SharedShadowParser, TitleChangeSender,
    encode_snapshot_segments, evaluate_output_target, lock_shadow_parser, resume_pane_with_permit,
};

mod agent_api;
mod attach;

pub(in crate::mux) use agent_api::*;
pub(in crate::mux::ipc) use attach::*;

/// Spawn a PTY, create a pane, and start a reader thread for output streaming.
///
/// Decodes optional `CreateWindowPayload` from the message to set window name
/// and execute an initial command. Empty or missing payload defaults to
/// name="Terminal" with no command (backward compatible with GUI).
///
/// `reply` is generic over [`ReplySink`] (task0001): the CLI-client path
/// passes its still-undivided `Framed` sink directly, the GUI loop passes
/// an [`super::outbound::OutboundHandle`] wrapping the outbound admission
/// queue — same handler logic, same behavior per message, different
/// destination for the reply.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_create_window<R: ReplySink>(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    reply: &mut R,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: u32,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> Result<(), bool> {
    // Decode payload; empty/invalid payload -> defaults (backward compat)
    let payload = msg
        .decode_payload::<CreateWindowPayload>()
        .unwrap_or_default();

    let window_name = payload
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("Terminal")
        .to_string();

    // Pre-allocate the pane ID (and mint its public ID) before spawning the
    // PTY: EMTERM_PANE_ID must be set in the shell's environment at spawn
    // time (IMPLEMENTATION.md FR13), which is before the pane is
    // registered in the SessionManager.
    let (pane_id, public_pane_id) = {
        let mut mgr = session_manager.lock().await;
        let pane_id = mgr.alloc_pane_id();
        let public_pane_id = mgr.public_pane_id(pane_id);
        (pane_id, public_pane_id)
    };

    let spawned = match spawn_pty(80, 24, &public_pane_id) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{}", e);
            let err = ErrorMsg {
                message: format!("Failed to spawn PTY: {}", e),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = reply.send_reply(resp).await;
            return Ok(());
        }
    };

    let mut mgr = session_manager.lock().await;
    let window_id = match mgr.create_window(active_session_id, window_name.clone()) {
        Some(id) => id,
        None => {
            log::error!("Failed to create window in session {}", active_session_id);
            drop(mgr);
            let err = ErrorMsg {
                message: "Failed to create window".to_string(),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = reply.send_reply(resp).await;
            return Ok(());
        }
    };

    let pane_id = match register_pane_and_start_reader(
        &mut mgr,
        active_session_id,
        window_id,
        pane_id,
        80,
        24,
        spawned,
        pane_output_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
    ) {
        Some(id) => id,
        None => {
            log::error!("Failed to register pane in window {}", window_id);
            return Ok(());
        }
    };

    let command = payload.command.filter(|s| !s.is_empty());
    drop(mgr);

    // Write initial command to PTY after short delay for shell readiness
    if let Some(ref cmd) = command {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mgr = session_manager.lock().await;
        if let Some(session) = mgr.get_session(active_session_id) {
            if let Some(window) = session.windows.get(&window_id) {
                if let Some(pane) = window.panes.get(&pane_id) {
                    let cmd_with_newline = format!("{}\n", cmd);
                    if let Err(e) = pane.write_input(cmd_with_newline.as_bytes()) {
                        log::warn!("Failed to write initial command to pane {}: {}", pane_id, e);
                    }
                }
            }
        }
    }

    log::info!(
        "Created window {} '{}' with pane {} (PTY spawned{})",
        window_id,
        window_name,
        pane_id,
        if command.is_some() {
            ", command sent"
        } else {
            ""
        }
    );

    let resp = MuxMessage::control(MessageType::PaneCreated, pane_id, &pane_id);
    if reply.send_reply(resp).await.is_err() {
        return Err(true);
    }

    Ok(())
}

/// Destroy a pane, removing it from its window. Cleans up empty windows and sessions.
/// Signals daemon shutdown when all sessions become empty.
///
/// Visibility is `pub(in crate::mux)` (wider than the surrounding `pub(super)`)
/// so the daemon-level pane-exit reap task in `crate::mux::daemon` can drive
/// reap directly. Reaping is keyed on `pane_id` and ignores the pane's
/// `output_target`, so it covers the detached path and the connection-reset
/// race uniformly. Returns early (warn + no-op) when the pane is already gone,
/// which makes a double reap (Connected empty-chunk path + daemon task) safe.
pub(in crate::mux) async fn handle_destroy_pane(
    pane_id: PaneId,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) {
    log::info!("DestroyPane requested for pane {}", pane_id);

    let mut mgr = session_manager.lock().await;
    let (session_id, window_id) = match mgr.find_pane(pane_id) {
        Some(ids) => ids,
        None => {
            log::warn!("DestroyPane: pane {} not found", pane_id);
            return;
        }
    };

    // Remove pane from window (drops writer/master, closing PTY)
    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            if let Some(mut pane) = window.remove_pane(pane_id) {
                // AC-5 (task0004): pane destruction fails every pending
                // `WaitAgentState` waiter with `pane_gone` rather than
                // leaving them hanging until their timeout.
                fail_agent_waiters_pane_gone(&pane);
                pane.mark_exited();
                log::info!("Destroyed pane {}", pane_id);
            }

            if window.is_empty() {
                session.remove_window(window_id);
                log::info!(
                    "Removed empty window {} from session {}",
                    window_id,
                    session_id
                );

                if session.is_empty() {
                    mgr.remove_session(session_id);
                    log::info!("Removed empty session {}", session_id);

                    if mgr.is_empty() {
                        log::info!("All sessions empty, daemon shutting down");
                        let _ = shutdown_tx.send(true);
                    }
                }
            }
        }
    }
}

/// Rename a window, decoding the new name from the message payload.
///
/// The `id` field may be either a pane ID (from GUI OSC title sync) or a
/// window ID (from CLI). Tries pane lookup first; falls back to window lookup.
pub(super) async fn handle_rename_window(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) {
    let rename_msg: RenameWindowMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid RenameWindow payload");
            return;
        }
    };
    let id = msg.pane_id;

    let mut mgr = session_manager.lock().await;

    // Try as pane_id first (GUI sends active pane_id)
    if let Some((sid, wid)) = mgr.find_pane(id) {
        log::info!(
            "RenameWindow: pane {} -> window {} -> '{}'",
            id,
            wid,
            rename_msg.name
        );
        mgr.rename_window(sid, wid, rename_msg.name);
        return;
    }

    // Fall back to window_id
    if let Some(sid) = mgr.find_window_session(id) {
        log::info!("RenameWindow: window {} -> '{}'", id, rename_msg.name);
        mgr.rename_window(sid, id, rename_msg.name);
    } else {
        log::warn!("RenameWindow: id {} not found as pane or window", id);
    }
}

/// Move a window to a new position within its session's window order.
///
/// The `msg.pane_id` field must be a pane ID; the handler resolves it to
/// the containing session and window via `find_pane`. Unlike
/// `handle_switch_window`, this handler does NOT accept a bare window ID,
/// because window IDs are session-local and a global window-ID lookup
/// would be non-deterministic across sessions.
///
/// The daemon does NOT broadcast the new order to attached clients. The GUI
/// performs an optimistic local reorder before sending this message; the
/// daemon side catches up silently and any reattach handshake will then
/// reflect the authoritative order via `Welcome`.
pub(super) async fn handle_move_window(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) {
    let move_msg: MoveWindowMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid MoveWindow payload");
            return;
        }
    };
    let id = msg.pane_id;
    let target_index = move_msg.target_index as usize;

    let mut mgr = session_manager.lock().await;

    if let Some((sid, wid)) = mgr.find_pane(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            let changed = session.move_window(wid, target_index);
            log::info!(
                "MoveWindow: pane {} -> window {} -> index {} (changed={})",
                id,
                wid,
                target_index,
                changed
            );
        }
    } else {
        // Not logged at warn level: a stale pane_id arriving after window
        // destruction is normal (concurrent close + move from GUI).
        log::debug!("MoveWindow: pane id {} not found", id);
    }
}

/// Switch the active window in the session.
///
/// The `id` may be either a pane ID (from GUI) or a window ID (from CLI).
/// Tries pane lookup first; falls back to window lookup.
pub(super) async fn handle_switch_window(id: u32, session_manager: &Arc<Mutex<SessionManager>>) {
    let mut mgr = session_manager.lock().await;

    // Try as pane_id first (GUI sends pane_id)
    if let Some((sid, wid)) = mgr.find_pane(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.active_window_id = Some(wid);
            log::info!(
                "SwitchWindow: pane {} -> session {} active window -> {}",
                id,
                sid,
                wid
            );
        }
        return;
    }

    // Fall back to window_id (CLI sends window_id)
    if let Some(sid) = mgr.find_window_session(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.active_window_id = Some(id);
            log::info!("SwitchWindow: session {} active window -> {}", sid, id);
        }
    } else {
        log::warn!("SwitchWindow: id {} not found as pane or window", id);
    }
}

/// Destroy a window and all its panes, cleaning up empty sessions.
/// Signals daemon shutdown when all sessions become empty.
pub(super) async fn handle_destroy_window(
    window_id: u32,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) {
    log::info!("DestroyWindow requested for window {}", window_id);

    let mut mgr = session_manager.lock().await;

    let session_id = match mgr.find_window_session(window_id) {
        Some(id) => id,
        None => {
            log::warn!("DestroyWindow: window {} not found", window_id);
            return;
        }
    };

    // Mark all panes in the window as exited before removal
    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            for pane in window.panes.values_mut() {
                pane.mark_exited();
            }
        }
    }

    if let Some(session_empty) = mgr.remove_window(session_id, window_id) {
        log::info!("Removed window {} from session {}", window_id, session_id);
        if session_empty {
            mgr.remove_session(session_id);
            log::info!("Removed empty session {}", session_id);
            if mgr.is_empty() {
                log::info!("All sessions empty, daemon shutting down");
                let _ = shutdown_tx.send(true);
            }
        }
    }
}

/// Resize a pane's PTY to the requested dimensions.
pub(super) async fn handle_resize(msg: MuxMessage, session_manager: &Arc<Mutex<SessionManager>>) {
    let resize_msg: ResizeMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid Resize payload");
            return;
        }
    };

    let pane_id = msg.pane_id;
    let mut mgr = session_manager.lock().await;
    let (session_id, window_id) = match mgr.find_pane(pane_id) {
        Some(ids) => ids,
        None => {
            log::warn!("Resize: pane {} not found", pane_id);
            return;
        }
    };

    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            if let Some(pane) = window.panes.get_mut(&pane_id) {
                if let Err(e) = pane.resize(resize_msg.cols, resize_msg.rows) {
                    log::warn!("Resize pane {}: {}", pane_id, e);
                } else {
                    log::debug!(
                        "Resized pane {} to {}x{}",
                        pane_id,
                        resize_msg.cols,
                        resize_msg.rows
                    );
                }
            }
        }
    }
}

/// Handle RequestPaneSnapshot message by pushing a snapshot-tagged
/// `PtyOutputChunk` onto the shared pane output channel.
///
/// On-wire framing (FR1): the chunk carries `ChunkKind::Snapshot`, and the
/// connection drain (`mux::ipc::connection`) encodes it as a
/// `MessageType::Snapshot` frame. The client dispatches that frame through
/// the `apply_mux_message::Snapshot|SnapshotRestore` arm, which selects
/// `dispatch_offthread_replay` (payload ≥ 64 KiB) or `reset_frame_for_replay`
/// (synchronous), both backed by `TerminalCore::build_from_snapshot` with
/// `scrollback_bypass` — the fast path delivered by the
/// `snapshot-replay-perf` predecessor task.
///
/// Why the same channel (and not a direct `framed.send`): the PTY reader
/// thread updates `shadow_parser` *and* enqueues the raw bytes onto
/// `pane_output_tx`. If the snapshot bypassed the channel, pending PTY
/// chunks already in the queue — whose effects are already baked into the
/// snapshot state — would be delivered *after* the snapshot and re-applied
/// on top of it, producing duplicated/shifted output. Routing through the
/// same channel preserves the FIFO ordering invariant (FR5): bytes queued
/// before the snapshot land before it on the wire, bytes queued after land
/// after. The `merge_consecutive_chunks` step is `kind`-aware, so the
/// snapshot frame is never folded into adjacent PTY chunks.
///
/// A narrow race window remains: the reader takes `shadow_parser.lock()`,
/// applies bytes, releases the lock, and *then* enqueues the chunk onto
/// `pane_output_tx`. If this handler runs between the reader's lock release
/// and its enqueue, we can end up with `[snapshot, reader_chunk]` in the
/// channel — duplicating that chunk's effect over the snapshot. In practice
/// the gap is ~µs and dominated by absolute-positioned ANSI (which is
/// idempotent), and the snapshot's leading `\x1b[H\x1b[2J` provides a
/// recovery point, so the observable drift is minimal. Absolute ordering is
/// *not* guaranteed; callers that need it must use a different mechanism
/// (e.g. a reader-side snapshot-request barrier).
///
/// Main/alt snapshot split: the reply payload is composed by
/// `build_shadow_parser_snapshot`, which funnels through
/// `build_snapshot_bytes`. For main-buffer panes the daemon vt100 screen
/// dump is omitted from the reply and the client reconstructs the visible
/// viewport from scrollback alone; for alt-screen panes the dump is
/// included so the TUI surface is restored. See `build_snapshot_bytes` for
/// the rationale.
pub(super) async fn handle_request_pane_snapshot(
    msg: &MuxMessage,
    active_session_id: u32,
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    deferred_output: &mut DeferredOutputQueue,
) -> Result<(), bool> {
    let pane_id = msg.pane_id;

    // Resolve the shadow parser AND the scrollback buffer together so the
    // on-demand snapshot mirrors the reattach construction (FR1): the daemon
    // already holds the pane's scrollback for the pane's lifetime, so the
    // history-bearing snapshot adds no new daemon state — only bytes the
    // on-demand reply previously omitted.
    //
    // Authorization: the snapshot (screen + scrollback) is served ONLY for
    // panes that belong to the connection's currently-attached session. A
    // `pane_id` resolving to a different session is refused so a client cannot
    // read another session's terminal history by guessing pane ids — the reply
    // now carries scrollback, which commonly holds secrets / commands / file
    // contents (this also closes the pre-existing screen-only exposure).
    let resolved: Option<(SharedShadowParser, SharedScrollback)> = {
        let mgr = session_manager.lock().await;
        match mgr.find_pane(pane_id) {
            None => {
                log::warn!("RequestPaneSnapshot: pane {} not found; ignoring", pane_id);
                None
            }
            Some((sid, _)) if sid != active_session_id => {
                log::warn!(
                    "RequestPaneSnapshot: pane {} owned by session {} but requester is attached to {}; refusing",
                    pane_id,
                    sid,
                    active_session_id
                );
                None
            }
            Some((sid, wid)) => mgr
                .get_session(sid)
                .and_then(|s| s.windows.get(&wid))
                .and_then(|w| w.panes.get(&pane_id))
                .map(|p| (p.shadow_parser.clone(), p.scrollback.clone())),
        }
    };

    let Some((shadow_parser, scrollback)) = resolved else {
        return Ok(());
    };

    // Read the pane's scrollback WITHOUT clearing (the buffer lives for the
    // lifetime of the pane; an empty buffer yields a valid clear + shadow
    // snapshot). The client's segment-driven replay rebuilds history from it
    // (task0004 round-4 rework D1').
    //
    // INVARIANT (FR3 guard-rail): the scrollback lock is held ONLY for the
    // `read_segments` copy. The owned `Vec`s are returned out of this scope
    // so the guard is provably dropped at the closing brace — before
    // snapshot assembly, logging, and the channel send below. This is a
    // copy-only critical section: the O(n) copy is unavoidable, but the
    // lock must never span assembly/log/send. Keep the copy inside this
    // block when refactoring.
    let (scrollback_data, scrollback_segments): (Vec<u8>, Vec<(usize, u16, u16)>) = {
        let guard = scrollback.lock().unwrap();
        guard.read_segments()
        // guard dropped here, at scope end, before any assembly/log/send.
    };
    let (snapshot, snapshot_segments) =
        build_shadow_parser_snapshot(&shadow_parser, &scrollback_data, &scrollback_segments);
    let encoded_snapshot = encode_snapshot_segments(&snapshot, &snapshot_segments);
    // Promoted from debug -> warn so release builds (which drop debug/info)
    // capture the snapshot-reply path during recovery investigations. The
    // call is rare (only on WASM recovery / window-switch reattach), so the
    // log volume is bounded. The size now includes scrollback (NFR2: the
    // payload scales like the reattach path), so this line doubles as the
    // transfer-size diagnostic for the larger payload.
    log::warn!(
        "RequestPaneSnapshot: pane {} -> {}B (scrollback {}B, {} segments)",
        pane_id,
        encoded_snapshot.len(),
        scrollback_data.len(),
        snapshot_segments.len()
    );

    // D6'' (task0005 rework, review round-4 finding `1d4a0c96821da0ef`):
    // enforce the SAME frame-size policy `send_reattach_data` already
    // applies (`mux_ipc::protocol::fits_single_snapshot_frame`) before
    // enqueueing — this path previously sent unconditionally, so an
    // oversized snapshot would reach the connection drain
    // (`mux::ipc::connection`), fail the codec's single-frame encode, and
    // tear the whole connection down. Practically unreachable today (a
    // pane's ring is capped at `DEFAULT_SCROLLBACK_CAPACITY` = 2 MiB plus a
    // bounded shadow-parser screen dump — see `REATTACH_CHUNK_SIZE`'s doc
    // in `mux::ipc::reattach`), but the check now costs nothing and closes
    // the gap between this producer and the reattach path.
    if !mux_ipc::protocol::fits_single_snapshot_frame(encoded_snapshot.len()) {
        log::warn!(
            "RequestPaneSnapshot: pane {} snapshot {}B exceeds the single-frame \
             limit ({}B); refusing rather than risk a codec encode failure \
             tearing down the connection",
            pane_id,
            encoded_snapshot.len(),
            mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD
        );
        return Ok(());
    }

    // Send as a snapshot-tagged chunk so the drain encodes it as
    // `MessageType::Snapshot` (routing to the client's off-thread replay
    // path) while still interleaving correctly with any already-queued PTY
    // bytes for this pane via the shared `pane_output_tx` channel (FR1,
    // FR5).
    //
    // mux-window-switch-output-hang task0001 (the fix this doc block used to
    // warn was missing): this MUST NOT be a blocking
    // `pane_output_tx.send(...).await`. This handler runs INSIDE the
    // connection's own `select!` loop (via `route_message`), and that loop's
    // `pane_output_rx.recv()` arm is the ONLY thing able to free capacity on
    // this channel. A blocking send here would suspend the whole connection
    // task until that same arm ran again — which it cannot while suspended
    // here, self-deadlocking the connection (SPEC.md "Root Cause"; this was
    // exactly the pre-fix bug). `enqueue_pane_output_chunk` never blocks the
    // caller: it enqueues immediately when there is room, or pushes onto
    // `deferred_output` (task0002 rework: a connection-owned, bounded
    // `DeferredOutputQueue` — see its doc) when the channel is momentarily
    // full. That queue is flushed by `flush_deferred_output`, called from
    // the connection's own event loop the next time capacity frees, in
    // strict FIFO order relative to items already in the queue; ordering
    // relative to producers outside this connection task (the PTY reader
    // thread) is NOT structurally guaranteed — see `DeferredOutputQueue`'s
    // doc for the precise, narrowed claim (AC-2/F4/F5). A closed channel
    // (client gone) is logged and dropped there, same as before.
    crate::mux::session::pane::enqueue_pane_output_chunk(
        pane_output_tx,
        PtyOutputChunk::snapshot(pane_id, encoded_snapshot),
        deferred_output,
    );

    // SPEC FR4/FR5 (task0003 AC-5): the on-demand snapshot just enqueued had
    // agent-status OSC stripped; resync this pane's current state
    // out-of-band (window-switch counterpart of the attach-time sync).
    crate::mux::daemon::sync_agent_status_after_pane_snapshot(session_manager, pane_id).await;

    Ok(())
}

#[cfg(test)]
mod tests;
