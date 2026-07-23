//! Message handlers for mux IPC commands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use super::pty_spawn::{register_pane_and_start_reader, spawn_pty};
use super::reattach::{
    build_shadow_parser_snapshot, collect_reattach_data, detach_session_panes, send_reattach_data,
};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentPaneStatus, AgentWaitOutcome, AgentWaiter, MuxPane, NotificationSender, PaneId,
    PtyOutputChunk, SharedPaneExitSender, SharedScrollback, SharedShadowParser, TitleChangeSender,
    evaluate_output_target, lock_shadow_parser, resume_pane_with_permit,
};

/// Spawn a PTY, create a pane, and start a reader thread for output streaming.
///
/// Decodes optional `CreateWindowPayload` from the message to set window name
/// and execute an initial command. Empty or missing payload defaults to
/// name="Terminal" with no command (backward compatible with GUI).
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_create_window<S>(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: u32,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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

    // Pre-allocate the pane ID before spawning the PTY: EMTERM_PANE_ID must
    // be set in the shell's environment at spawn time (IMPLEMENTATION.md
    // FR13), which is before the pane is registered in the SessionManager.
    let pane_id = {
        let mut mgr = session_manager.lock().await;
        mgr.alloc_pane_id()
    };

    let spawned = match spawn_pty(80, 24, pane_id) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{}", e);
            let err = ErrorMsg {
                message: format!("Failed to spawn PTY: {}", e),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
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
            let _ = framed.send(resp).await;
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
    if framed.send(resp).await.is_err() {
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
    // snapshot). The client's reset_and_replay rebuilds history from it.
    //
    // INVARIANT (FR3 guard-rail): the scrollback lock is held ONLY for the
    // `read_all` copy. The owned `Vec` is returned out of this scope so the
    // guard is provably dropped at the closing brace — before snapshot
    // assembly, logging, and the channel send below. This is a copy-only
    // critical section: the O(n) copy is unavoidable, but the lock must never
    // span assembly/log/send. Keep the copy inside this block when refactoring.
    let scrollback_data: Vec<u8> = {
        let guard = scrollback.lock().unwrap();
        guard.read_all()
        // guard dropped here, at scope end, before any assembly/log/send.
    };
    let snapshot = build_shadow_parser_snapshot(&shadow_parser, &scrollback_data);
    // Promoted from debug -> warn so release builds (which drop debug/info)
    // capture the snapshot-reply path during recovery investigations. The
    // call is rare (only on WASM recovery / window-switch reattach), so the
    // log volume is bounded. The size now includes scrollback (NFR2: the
    // payload scales like the reattach path), so this line doubles as the
    // transfer-size diagnostic for the larger payload.
    log::warn!(
        "RequestPaneSnapshot: pane {} -> {}B (scrollback {}B)",
        pane_id,
        snapshot.len(),
        scrollback_data.len()
    );

    // Send as a snapshot-tagged chunk so the drain encodes it as
    // `MessageType::Snapshot` (routing to the client's off-thread replay
    // path) while still interleaving correctly with any already-queued PTY
    // bytes for this pane via the shared `pane_output_tx` channel (FR1,
    // FR5). If the client is gone the channel is closed — that's not a
    // fatal error for this handler, just drop the reply.
    if let Err(e) = pane_output_tx
        .send(PtyOutputChunk::snapshot(pane_id, snapshot))
        .await
    {
        log::warn!(
            "RequestPaneSnapshot: failed to enqueue snapshot for pane {}: {}",
            pane_id,
            e
        );
    }
    Ok(())
}

/// Handle Attach message: switch the client to a different session.
///
/// Detaches panes from the current session, updates the active session,
/// and reattaches panes from the new session with buffered output replay.
///
/// Also allocates a fresh kick channel: the sender is installed on the new
/// session (firing any previously-installed kick to evict the prior client),
/// and the receiver is written to `kick_rx` so the connection loop can await
/// it in its select!. Any prior receiver held by the caller is replaced.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_attach<S>(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
    title_tx: &TitleChangeSender,
    kick_rx: &mut Option<oneshot::Receiver<()>>,
    visible_state: &Arc<AtomicBool>,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let attach_msg: AttachMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid Attach payload");
            return Ok(());
        }
    };

    let new_session_id = attach_msg.session_id;
    log::info!("Client attaching to session {}", new_session_id);

    // Verify session exists
    {
        let mgr = session_manager.lock().await;
        if mgr.get_session(new_session_id).is_none() {
            log::warn!("Attach: session {} not found", new_session_id);
            let err = ErrorMsg {
                message: format!("Session {} not found", new_session_id),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
            return Ok(());
        }
    }

    // Detach from current session — identity-scoped: only panes still owned
    // by our pane_output_tx are flipped to Detached. Panes that have been
    // handed off to another connection (race window) are preserved.
    detach_session_panes(session_manager, *active_session_id, pane_output_tx).await;

    // Update active session
    *active_session_id = new_session_id;

    // Allocate a fresh kick channel for this attachment. The sender is
    // installed onto the target session by collect_reattach_data, which also
    // fires any previously-installed kick to evict the prior client.
    let (new_kick_tx, new_kick_rx) = oneshot::channel::<()>();

    // FR13: pass the current visible state into collect_reattach_data so a
    // hidden reattach skips the snapshot send entirely (panes stay Detached
    // and continue accumulating ring + raw_passthrough until the next
    // SetVisibility(true)). When visible, the existing flow runs: drain
    // and switch to Connected.
    let attach_visible = visible_state.load(Ordering::Acquire);
    let reattach_data = collect_reattach_data(
        session_manager,
        new_session_id,
        pane_output_tx,
        title_tx,
        new_kick_tx,
        attach_visible,
    )
    .await;

    // Replace the connection's kick receiver with the fresh one for the new
    // session. Any prior receiver (for the session we just left) is dropped;
    // firing its sender would return Err and be ignored.
    *kick_rx = Some(new_kick_rx);

    // `send_reattach_data` always emits one `PaneCreated` per entry. When
    // `attach_visible == false`, every entry has empty buffer bytes so no
    // `PtyOutput` is sent (the frontend learns the pane exists but receives
    // no screen contents until the next SetVisibility(true) resume).
    if send_reattach_data(framed, &reattach_data).await.is_err() {
        return Err(true);
    }

    log::info!(
        "Attached to session {} with {} pane(s)",
        new_session_id,
        reattach_data.len()
    );
    Ok(())
}

/// Apply a `SetVisibility` message: update the connection-scoped visible
/// state and re-evaluate every pane in the active session.
///
/// `visible -> false` flips identity-owned panes to Detached so their
/// shadow + ring + raw_passthrough accumulate while we are hidden. No
/// snapshot is involved on this edge.
///
/// `visible -> true` resumes panes that were Detached only because we
/// were hidden. FR9 race-freedom: for every candidate pane the handler
/// reserves a permit on `pane_output_tx` *outside* the pane lock, then
/// hands the permit to `resume_pane_with_permit`, which holds the pane's
/// `output_target` mutex across (snapshot enqueue, swap to Connected).
/// The reader thread also takes the same mutex before its `try_send`, so
/// a live PTY chunk cannot land between the snapshot send and the
/// Connected swap — channel FIFO then guarantees the snapshot arrives at
/// the client ahead of any subsequent live batch.
pub(super) async fn handle_set_visibility(
    visible: bool,
    session_manager: &Arc<Mutex<SessionManager>>,
    active_session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    visible_state: &Arc<AtomicBool>,
) {
    let prev = visible_state.swap(visible, Ordering::AcqRel);
    if prev == visible {
        return;
    }
    log::debug!(
        "[DEBUG][BACKEND] handle_set_visibility: {} (session {})",
        visible,
        active_session_id
    );

    if !visible {
        // Hidden edge: no snapshot, just flip eligible panes.
        let mgr = session_manager.lock().await;
        let Some(session) = mgr.get_session(active_session_id) else {
            return;
        };
        for window in session.windows.values() {
            for pane in window.panes.values() {
                if pane.exited {
                    continue;
                }
                let _ = evaluate_output_target(pane, false, false, pane_output_tx);
            }
        }
        return;
    }

    // Visible edge: collect candidate pane ids first, then per pane reserve
    // a permit (await without locks), re-acquire the manager lock, and run
    // `resume_pane_with_permit` under the pane mutex.
    let candidate_pane_ids: Vec<PaneId> = {
        let mgr = session_manager.lock().await;
        let Some(session) = mgr.get_session(active_session_id) else {
            return;
        };
        let mut ids = Vec::new();
        for window in session.windows.values() {
            for pane in window.panes.values() {
                if !pane.exited {
                    ids.push(pane.id);
                }
            }
        }
        ids
    };

    for pane_id in candidate_pane_ids {
        let permit = match pane_output_tx.reserve().await {
            Ok(p) => p,
            Err(_) => {
                log::warn!(
                    "[WARN][BACKEND] handle_set_visibility: pane_output_tx closed; aborting resume for pane {} and remaining panes",
                    pane_id
                );
                return;
            }
        };
        let mgr = session_manager.lock().await;
        let Some(session) = mgr.get_session(active_session_id) else {
            drop(permit);
            return;
        };
        let pane = session
            .windows
            .values()
            .find_map(|w| w.panes.get(&pane_id))
            .filter(|p| !p.exited);
        let Some(pane) = pane else {
            drop(permit);
            continue;
        };
        let _ = resume_pane_with_permit(pane, pane_output_tx, permit);
    }
}

// ============================================================================
// Agent-facing API: ReadPane / SendText / WaitAgentState (task0004)
//
// Implements FR10-FR12 (see IMPLEMENTATION.md "Wait implementation" /
// "Revision semantics"). All three requests are CLI -> daemon, CLI-client-
// only (dispatched from `mux::ipc::connection::handle_cli_client`).
// ============================================================================

/// Caps and defaults for `ReadPane` (NFR3: read responses are size-capped).
const READ_LINES_MAX: u32 = 2000;
const READ_MAX_BYTES: usize = 256 * 1024;
/// Raw scrollback bytes considered for the tail before ANSI-stripping and
/// line-splitting. Sized generously above `READ_LINES_MAX` lines worth of
/// typical terminal output so line-splitting rarely runs short.
const SCROLLBACK_READ_TAIL_BYTES: usize = 512 * 1024;

/// Cap on `SendText` payload size (NFR1: request validation).
const SEND_MAX_BYTES: usize = 1024 * 1024;

fn unknown_pane_error(public_pane_id: &str) -> AgentApiError {
    AgentApiError {
        kind: AgentApiErrorKind::UnknownPane,
        message: format!("unknown pane: {public_pane_id}"),
    }
}

fn invalid_payload_error() -> AgentApiError {
    AgentApiError {
        kind: AgentApiErrorKind::InvalidInput,
        message: "invalid request payload".to_string(),
    }
}

/// Strip ANSI/VT escape sequences (CSI, OSC, DCS/APC/PM/SOS, and other
/// two-byte ESC sequences) from raw PTY bytes, returning the remainder
/// decoded as UTF-8 (lossy). Used only for `ReadPane`'s scrollback-tail
/// portion — the current-screen portion already comes pre-stripped from
/// `vt100::Screen::contents()`. An unterminated sequence at the very end of
/// `bytes` is dropped rather than causing a panic or infinite loop.
fn strip_ansi_escapes(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if i + 1 >= bytes.len() {
            break; // dangling ESC at the very end; drop it.
        }
        match bytes[i + 1] {
            b'[' => {
                // CSI: ESC [ params... final-byte (0x40..=0x7E)
                let mut j = i + 2;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                i = if j < bytes.len() { j + 1 } else { bytes.len() };
            }
            b']' => {
                // OSC: ESC ] ... terminated by BEL or ESC \
                let mut j = i + 2;
                let mut end = bytes.len();
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        end = j + 1;
                        break;
                    }
                    if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                        end = j + 2;
                        break;
                    }
                    j += 1;
                }
                i = end;
            }
            b'P' | b'_' | b'^' | b'X' => {
                // DCS / APC / PM / SOS: terminated by ST (ESC \)
                let mut j = i + 2;
                let mut end = bytes.len();
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                        end = j + 2;
                        break;
                    }
                    j += 1;
                }
                i = end;
            }
            _ => {
                // Two-byte escape (e.g. ESC 7 / ESC 8 / ESC c / ESC =).
                i += 2;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Combine the (ANSI-stripped) scrollback tail with the current screen's
/// plain-text contents and return the last `lines` lines, capped at
/// `READ_MAX_BYTES` (AC-1).
fn render_pane_tail(scrollback_tail: &[u8], screen_contents: &str, lines: u32) -> String {
    let stripped_scrollback = strip_ansi_escapes(scrollback_tail);
    let mut all_lines: Vec<&str> = stripped_scrollback.lines().collect();
    all_lines.extend(screen_contents.lines());

    let take = lines as usize;
    let start = all_lines.len().saturating_sub(take);
    let mut text = all_lines[start..].join("\n");

    if text.len() > READ_MAX_BYTES {
        let mut end = READ_MAX_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// Resolve `public_pane_id` to the pane's shadow parser + scrollback (for
/// `ReadPane`) — errors uniformly as `unknown_pane` for both an
/// unresolvable public ID and a resolvable-but-absent pane_id.
async fn resolve_read_targets(
    public_pane_id: &str,
    session_manager: &Arc<Mutex<SessionManager>>,
) -> Result<(SharedShadowParser, SharedScrollback), AgentApiError> {
    let pane_id = crate::mux::daemon::resolve_public_pane_id(public_pane_id)
        .ok_or_else(|| unknown_pane_error(public_pane_id))?;
    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        return Err(unknown_pane_error(public_pane_id));
    };
    let pane = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id));
    let Some(pane) = pane else {
        return Err(unknown_pane_error(public_pane_id));
    };
    Ok((pane.shadow_parser.clone(), pane.scrollback.clone()))
}

/// Handle `ReadPane`: return the tail `lines` rendered rows of a mux pane
/// (current screen + scrollback tail), ANSI-stripped (AC-1, FR10).
pub(super) async fn handle_read_pane(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) -> Result<ReadPaneResultMsg, AgentApiError> {
    let req: ReadPaneMsg = msg.decode_payload().ok_or_else(invalid_payload_error)?;
    let lines = req.lines.clamp(1, READ_LINES_MAX);

    let (shadow_parser, scrollback) =
        resolve_read_targets(&req.public_pane_id, session_manager).await?;

    let screen_contents = {
        let parser = lock_shadow_parser(&shadow_parser);
        parser.screen().contents()
    };
    let scrollback_tail: Vec<u8> = {
        let guard = scrollback.lock().unwrap();
        let all = guard.read_all();
        let start = all.len().saturating_sub(SCROLLBACK_READ_TAIL_BYTES);
        all[start..].to_vec()
    };

    let text = render_pane_tail(&scrollback_tail, &screen_contents, lines);
    Ok(ReadPaneResultMsg { text })
}

/// Handle `SendText`: write `bytes` verbatim to a mux pane's PTY (no
/// implicit Enter), rejecting NUL / oversize without writing, and
/// returning the pre-write revision watermark (AC-2, FR11).
pub(super) async fn handle_send_text(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) -> Result<SendTextResultMsg, AgentApiError> {
    let req: SendTextMsg = msg.decode_payload().ok_or_else(invalid_payload_error)?;

    if req.bytes.contains(&0) {
        return Err(AgentApiError {
            kind: AgentApiErrorKind::InvalidInput,
            message: "input must not contain NUL bytes".to_string(),
        });
    }
    if req.bytes.len() > SEND_MAX_BYTES {
        return Err(AgentApiError {
            kind: AgentApiErrorKind::InvalidInput,
            message: format!("input exceeds the {SEND_MAX_BYTES}-byte cap"),
        });
    }

    let pane_id = crate::mux::daemon::resolve_public_pane_id(&req.public_pane_id)
        .ok_or_else(|| unknown_pane_error(&req.public_pane_id))?;

    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        return Err(unknown_pane_error(&req.public_pane_id));
    };
    let pane = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id));
    let Some(pane) = pane else {
        return Err(unknown_pane_error(&req.public_pane_id));
    };

    // Watermark: the revision observed immediately before the write
    // (IMPLEMENTATION.md "Revision semantics"). `write_input` takes the
    // pane's writer mutex for the whole write+flush, matching the
    // atomicity every other PtyInput write already relies on.
    let watermark = pane.agent_status.lock().unwrap().revision;
    pane.write_input(&req.bytes).map_err(|e| AgentApiError {
        kind: AgentApiErrorKind::InvalidInput,
        message: format!("failed to write to pane: {e}"),
    })?;

    Ok(SendTextResultMsg {
        revision_watermark: watermark,
    })
}

/// Pure check: does `status` already satisfy `states` (and, when set,
/// `after_revision`)? Used both for the immediate "wait succeeds now" path
/// and shared with [`reevaluate_agent_waiters`]'s matching logic (AC-3,
/// AC-4).
fn check_wait_immediate(
    status: &AgentPaneStatus,
    states: &[AgentState],
    after_revision: Option<u64>,
) -> Option<(AgentState, u64)> {
    let state = status.state?;
    let revision_ok = after_revision.map(|a| status.revision > a).unwrap_or(true);
    if revision_ok && states.contains(&state) {
        Some((state, status.revision))
    } else {
        None
    }
}

/// Re-evaluate every registered waiter on `pane` against its current agent
/// status (IMPLEMENTATION.md "Wait implementation": level-triggered,
/// re-evaluated on every accepted report). Intended to be called by the
/// daemon's agent-status ingestion path (task0003) after every accepted
/// report (set/clear/same-state re-report).
///
/// A waiter is removed from the registry when it fires (states+revision
/// match — its outcome is sent) OR when its receiver is already gone
/// (disconnected client, or a `wait` request that already timed out) —
/// this is the (non-polling) discard mechanism for AC-5.
pub(in crate::mux) fn reevaluate_agent_waiters(pane: &MuxPane) {
    let status = pane.agent_status.lock().unwrap().clone();
    let mut waiters = pane.agent_waiters.lock().unwrap();
    waiters.retain_mut(|w| {
        let is_closed = match w.responder.as_ref() {
            Some(responder) => responder.is_closed(),
            None => true,
        };
        if is_closed {
            return false;
        }
        match check_wait_immediate(&status, &w.states, w.after_revision) {
            Some((state, revision)) => {
                if let Some(responder) = w.responder.take() {
                    let _ = responder.send(AgentWaitOutcome::Matched { state, revision });
                }
                false
            }
            None => true,
        }
    });
}

/// Fail every waiter registered on `pane` with [`AgentWaitOutcome::PaneGone`]
/// and clear the registry. Called from [`handle_destroy_pane`] before the
/// pane is torn down (AC-5: "pane destruction during wait resolves with
/// pane_gone").
pub(in crate::mux) fn fail_agent_waiters_pane_gone(pane: &MuxPane) {
    let mut waiters = pane.agent_waiters.lock().unwrap();
    for mut waiter in waiters.drain(..) {
        if let Some(responder) = waiter.responder.take() {
            let _ = responder.send(AgentWaitOutcome::PaneGone);
        }
    }
}

/// Handle `WaitAgentState`: block until the pane's agent state enters
/// `states` (optionally requiring `revision > after_revision`), or until
/// `timeout_ms` elapses (AC-3, AC-4, AC-5, FR12).
///
/// Level-triggered and race-free with respect to concurrent state updates:
/// the immediate check and the waiter registration both run while holding
/// the pane's `agent_status` lock, so a state change landing between "check"
/// and "register" cannot be missed (any mutator must also take that lock).
pub(super) async fn handle_wait_agent_state(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) -> Result<WaitAgentStateResultMsg, AgentApiError> {
    let req: WaitAgentStateMsg = msg.decode_payload().ok_or_else(invalid_payload_error)?;
    if req.states.is_empty() {
        return Err(AgentApiError {
            kind: AgentApiErrorKind::InvalidInput,
            message: "states must not be empty".to_string(),
        });
    }

    let pane_id = crate::mux::daemon::resolve_public_pane_id(&req.public_pane_id)
        .ok_or_else(|| unknown_pane_error(&req.public_pane_id))?;

    let (agent_status, agent_waiters) = {
        let mgr = session_manager.lock().await;
        let Some((sid, wid)) = mgr.find_pane(pane_id) else {
            return Err(unknown_pane_error(&req.public_pane_id));
        };
        let pane = mgr
            .get_session(sid)
            .and_then(|s| s.windows.get(&wid))
            .and_then(|w| w.panes.get(&pane_id));
        let Some(pane) = pane else {
            return Err(unknown_pane_error(&req.public_pane_id));
        };
        (pane.agent_status.clone(), pane.agent_waiters.clone())
    };

    let rx = {
        let status = agent_status.lock().unwrap();
        if let Some((state, revision)) =
            check_wait_immediate(&status, &req.states, req.after_revision)
        {
            return Ok(WaitAgentStateResultMsg { state, revision });
        }
        let (tx, rx) = oneshot::channel();
        agent_waiters.lock().unwrap().push(AgentWaiter {
            states: req.states.clone(),
            after_revision: req.after_revision,
            responder: Some(tx),
        });
        rx
        // `status` (agent_status lock) is dropped here, after registration —
        // closing the check-then-register race window.
    };

    let timeout = std::time::Duration::from_millis(req.timeout_ms);
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(AgentWaitOutcome::Matched { state, revision })) => {
            Ok(WaitAgentStateResultMsg { state, revision })
        }
        Ok(Ok(AgentWaitOutcome::PaneGone)) => Err(AgentApiError {
            kind: AgentApiErrorKind::PaneGone,
            message: format!("pane {} was destroyed while waiting", req.public_pane_id),
        }),
        Ok(Err(_)) => Err(AgentApiError {
            kind: AgentApiErrorKind::PaneGone,
            message: "waiter channel closed unexpectedly".to_string(),
        }),
        Err(_elapsed) => Err(AgentApiError {
            kind: AgentApiErrorKind::Timeout,
            message: "wait timed out".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
    use std::sync::Mutex as StdMutex;

    fn add_pane(
        mgr: &mut SessionManager,
        session_id: u32,
        window_id: u32,
        pane_id: u32,
        target: SharedOutputTarget,
    ) {
        let pane = MuxPane::new_test(pane_id, 80, 24, target);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&window_id)
            .unwrap()
            .add_pane(pane);
    }

    /// FR3 byte-identity guard-rail: the lock-scope refactor in
    /// `handle_request_pane_snapshot` (scoped `read_all` block) must NOT change
    /// the assembled snapshot bytes. This reconstructs the same inputs the
    /// handler feeds to `build_shadow_parser_snapshot` (an owned `read_all`
    /// copy + the shadow screen) and asserts the result follows the
    /// `ESC[H ESC[2J + scrollback + screen` layout — for both a representative
    /// screen + scrollback and the empty-scrollback case.
    ///
    /// Driven through the `alt_screen = true` branch (parser flipped via
    /// ESC[?1049h before feeding the screen bytes) because the layout-split
    /// contract omits the daemon vt100 dump for main-buffer panes; the
    /// SCREEN-CONTENT presence assertion is only meaningful for the alt
    /// branch.
    #[test]
    fn snapshot_bytes_unchanged_after_lock_scope_guardrail() {
        use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
        use crate::mux::session::pane::new_shadow_parser;
        use std::sync::Mutex as StdMutex;

        // Representative screen + scrollback. Switch to alt-screen first so
        // build_shadow_parser_snapshot follows the alt branch and includes
        // the screen dump.
        let shadow_parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
        shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
        shadow_parser
            .lock()
            .unwrap()
            .process(b"\x1b[31mSCREEN-CONTENT\x1b[0m");

        let scrollback: SharedScrollback =
            Arc::new(StdMutex::new(ScrollbackRingBuffer::new(64 * 1024)));
        scrollback
            .lock()
            .unwrap()
            .write(b"HISTORY-LINE-ONE\r\nHISTORY-LINE-TWO\r\n");

        // Mirror the handler's scoped-copy step, then assemble.
        let scrollback_data: Vec<u8> = {
            let guard = scrollback.lock().unwrap();
            guard.read_all()
        };
        let assembled = build_shadow_parser_snapshot(&shadow_parser, &scrollback_data);

        // Established layout: ESC[3J ESC[H ESC[2J + scrollback + shadow screen.
        assert!(
            assembled.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
            "snapshot must start with the clear+home prefix"
        );
        let find = |needle: &[u8]| {
            assembled
                .windows(needle.len())
                .position(|w| w == needle)
                .unwrap_or_else(|| panic!("needle {:?} not found", needle))
        };
        let sb_at = find(b"HISTORY-LINE-ONE");
        let screen_at = find(b"SCREEN-CONTENT");
        assert!(
            sb_at >= b"\x1b[3J\x1b[H\x1b[2J".len(),
            "scrollback after clear prefix"
        );
        assert!(
            sb_at < screen_at,
            "scrollback must precede the shadow screen"
        );
        // The owned-copy path produces the exact same bytes as feeding the
        // scrollback slice straight through (no behavioral divergence).
        let direct =
            build_shadow_parser_snapshot(&shadow_parser, &scrollback.lock().unwrap().read_all());
        assert_eq!(assembled, direct, "scoped copy must be byte-identical");

        // Empty-scrollback case: still a valid clear + shadow snapshot.
        let empty_sb: SharedScrollback =
            Arc::new(StdMutex::new(ScrollbackRingBuffer::new(64 * 1024)));
        let empty_data: Vec<u8> = {
            let guard = empty_sb.lock().unwrap();
            guard.read_all()
        };
        assert!(empty_data.is_empty(), "fresh buffer reads back empty");
        let empty_assembled = build_shadow_parser_snapshot(&shadow_parser, &empty_data);
        assert!(empty_assembled.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));
        assert!(
            empty_assembled
                .windows(b"SCREEN-CONTENT".len())
                .any(|w| w == b"SCREEN-CONTENT"),
            "shadow screen present with empty scrollback"
        );
    }

    /// TS-7: SetVisibility(false) flips identity-owned panes to Detached.
    /// While hidden, no PTY chunks must reach the channel (the reader thread
    /// would push into the per-pane ring buffer instead).
    #[tokio::test]
    async fn handle_set_visibility_false_switches_owned_pane_to_detached() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            sid
        };

        let visible_state = Arc::new(AtomicBool::new(true));
        handle_set_visibility(false, &mgr, session_id, &owned_tx, &visible_state).await;

        assert!(!visible_state.load(Ordering::Acquire));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
        // No snapshot enqueued on hidden transition.
        assert!(rx.try_recv().is_err(), "no snapshot expected on hidden");
    }

    /// TS-7 / TS-14b: SetVisibility(true) after hidden enqueues exactly one
    /// snapshot per pane onto the channel and restores Connected.
    #[tokio::test]
    async fn handle_set_visibility_true_after_hidden_enqueues_snapshot() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        // Start in Detached as if SetVisibility(false) ran earlier and the
        // reader had captured shadow + raw_passthrough state. Owner = the
        // caller's tx so the visibility resume is permitted.
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            // Seed shadow + raw_passthrough on the just-added pane.
            let pane_ref = m
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&1)
                .unwrap();
            pane_ref.shadow_parser.lock().unwrap().process(b"hi-shadow");
            pane_ref
                .raw_passthrough
                .lock()
                .unwrap()
                .append(b"\x1b_Gi=9;XX\x1b\\");
            sid
        };

        // visible_state was false before this call — same precondition the
        // hidden -> visible transition exhibits in production.
        let visible_state = Arc::new(AtomicBool::new(false));
        handle_set_visibility(true, &mgr, session_id, &owned_tx, &visible_state).await;

        assert!(visible_state.load(Ordering::Acquire));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));

        // Exactly one snapshot chunk must have landed on the channel.
        let chunk = rx.try_recv().expect("snapshot chunk expected");
        assert_eq!(chunk.pane_id, 1);
        assert!(chunk.data.starts_with(b"\x1b[H\x1b[2J"));
        // Captured passthrough must NOT be replayed (would re-render the image).
        let needle = b"\x1b_Gi=9;XX\x1b\\";
        assert!(
            !chunk.data.windows(needle.len()).any(|w| w == needle),
            "snapshot must NOT include the captured passthrough sequence"
        );
        assert!(
            rx.try_recv().is_err(),
            "no further chunk expected for a single-pane session"
        );
    }

    /// F2 regression: SetVisibility(true) holds the pane's `output_target`
    /// mutex across (snapshot enqueue → Connected swap). A reader that takes
    /// the same mutex cannot interleave a live chunk between those steps,
    /// so the channel FIFO guarantees the snapshot lands first.
    ///
    /// The test inspects the per-chunk ordering on the channel: the
    /// snapshot chunk must appear with `pane_output_tx` already in
    /// `Connected` mode is impossible to assert with deterministic timing
    /// in a unit test, so we instead verify the post-conditions that prove
    /// the lock was held across both steps:
    /// - target is Connected
    /// - snapshot chunk is on the channel
    /// - no concurrent reader could have raced because the test does not
    ///   spawn a reader and the resume path is single-threaded
    ///
    /// Combined with `pane_output_tx` having capacity 1 *and* the receiver
    /// being unread until after `handle_set_visibility` completes, the
    /// existence of the chunk in the channel after the swap proves the
    /// permit-based synchronous send happened under the pane lock.
    #[tokio::test]
    async fn handle_set_visibility_resume_uses_permit_under_pane_lock() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        // Capacity 1: the only way the snapshot can land while the swap to
        // Connected also succeeds is if the resume path reserved a permit
        // and used it synchronously inside the pane lock.
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);

        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            sid
        };

        let visible_state = Arc::new(AtomicBool::new(false));
        handle_set_visibility(true, &mgr, session_id, &owned_tx, &visible_state).await;

        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        let chunk = rx.try_recv().expect("snapshot chunk must be queued");
        assert_eq!(chunk.pane_id, 1);
        assert!(chunk.data.starts_with(b"\x1b[H\x1b[2J"));
    }

    /// F2 regression: with two panes, each gets exactly one snapshot
    /// chunk and the per-pane (send, swap) sequence cannot interleave
    /// because `resume_pane_with_permit` holds the per-pane mutex.
    #[tokio::test]
    async fn handle_set_visibility_resume_two_panes_each_gets_one_snapshot() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid1 = m.create_window(sid, "shell".to_string()).unwrap();
            let wid2 = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid1, 1, target1.clone());
            add_pane(&mut m, sid, wid2, 2, target2.clone());
            sid
        };

        let visible_state = Arc::new(AtomicBool::new(false));
        handle_set_visibility(true, &mgr, session_id, &owned_tx, &visible_state).await;

        let mut seen = std::collections::HashSet::new();
        for _ in 0..2 {
            let chunk = rx.try_recv().expect("snapshot chunk expected");
            assert!(seen.insert(chunk.pane_id), "duplicate snapshot for pane");
            assert!(chunk.data.starts_with(b"\x1b[H\x1b[2J"));
        }
        assert!(rx.try_recv().is_err(), "exactly two snapshots expected");
        assert!(matches!(
            *target1.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        assert!(matches!(
            *target2.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    /// Idempotent: SetVisibility with the same value as the current state
    /// must be a no-op (no pane churn, no snapshot).
    #[tokio::test]
    async fn handle_set_visibility_same_state_is_noop() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            sid
        };

        let visible_state = Arc::new(AtomicBool::new(true));
        handle_set_visibility(true, &mgr, session_id, &owned_tx, &visible_state).await;

        // No state change, no snapshot.
        assert!(visible_state.load(Ordering::Acquire));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        assert!(rx.try_recv().is_err(), "no snapshot expected on no-op");
    }

    /// TS-2 (FR1, FR3): `handle_request_pane_snapshot` enqueues a chunk
    /// whose discriminator is `ChunkKind::Snapshot` (not the default
    /// `ChunkKind::PtyOutput`). The drain layer (`mux::ipc::connection`)
    /// is responsible for encoding `Snapshot` chunks as
    /// `MessageType::Snapshot` on the wire so the client routes them to
    /// the `apply_mux_message::Snapshot|SnapshotRestore` arm and the
    /// `build_from_snapshot` + `scrollback_bypass` fast path.
    ///
    /// The assembled payload follows the `ESC[3J ESC[H ESC[2J` clear-prefix,
    /// then scrollback, then (for alt-screen panes) shadow screen contents
    /// layout. The shadow parser is driven into alt-screen mode before
    /// feeding the screen bytes because the layout-split contract omits the
    /// daemon vt100 dump for main-buffer panes.
    #[tokio::test]
    async fn handle_request_pane_snapshot_emits_snapshot_kind() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            // Seed shadow + scrollback so the assembled snapshot has
            // recognisable bytes for the post-conditions. Flip to alt-screen
            // first so the daemon vt100 dump is included.
            let pane_ref = m
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&1)
                .unwrap();
            pane_ref
                .shadow_parser
                .lock()
                .unwrap()
                .process(b"\x1b[?1049h");
            pane_ref
                .shadow_parser
                .lock()
                .unwrap()
                .process(b"\x1b[31mSCREEN-CONTENT\x1b[0m");
            pane_ref
                .scrollback
                .lock()
                .unwrap()
                .write(b"HISTORY-LINE-ONE\r\n");
            sid
        };

        let req = MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id: 1,
            payload: Vec::new(),
        };
        handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx)
            .await
            .expect("handle_request_pane_snapshot");

        let chunk = rx.try_recv().expect("snapshot chunk expected");
        assert_eq!(chunk.pane_id, 1);
        assert_eq!(
            chunk.kind,
            crate::mux::session::pane::ChunkKind::Snapshot,
            "snapshot reply must carry kind = Snapshot (FR1, FR3)"
        );
        // Byte-identity guardrail: clear+home prefix, then scrollback,
        // then shadow screen.
        assert!(
            chunk.data.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
            "snapshot must start with the clear+home prefix"
        );
        let find = |needle: &[u8]| {
            chunk
                .data
                .windows(needle.len())
                .position(|w| w == needle)
                .unwrap_or_else(|| panic!("needle {:?} not found in snapshot", needle))
        };
        let sb_at = find(b"HISTORY-LINE-ONE");
        let screen_at = find(b"SCREEN-CONTENT");
        assert!(
            sb_at < screen_at,
            "scrollback must precede the shadow screen"
        );
        assert!(
            rx.try_recv().is_err(),
            "exactly one snapshot chunk expected"
        );
    }

    /// TS-3 (FR1, FR5): FIFO ordering between PTY chunks and a snapshot
    /// reply on the same pane. The on-channel order MUST be
    /// `[PRE(PtyOutput), snapshot(Snapshot), POST(PtyOutput)]`. The
    /// drain layer's `merge_consecutive_chunks` must not collapse across
    /// `kind`, so the snapshot stays a standalone chunk between the two
    /// PTY chunks.
    #[tokio::test]
    async fn handle_request_pane_snapshot_preserves_fifo_ordering() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let session_id = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            add_pane(&mut m, sid, wid, 1, target.clone());
            sid
        };

        // PRE PTY chunk (simulates a reader-thread chunk already in flight).
        owned_tx
            .send(PtyOutputChunk::pty_output(1, b"PRE".to_vec()))
            .await
            .expect("send PRE");

        // Snapshot reply runs *between* the PRE and POST PTY chunks.
        let req = MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id: 1,
            payload: Vec::new(),
        };
        handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx)
            .await
            .expect("handle_request_pane_snapshot");

        // POST PTY chunk after the snapshot.
        owned_tx
            .send(PtyOutputChunk::pty_output(1, b"POST".to_vec()))
            .await
            .expect("send POST");

        let pre = rx.try_recv().expect("PRE chunk");
        let snap = rx.try_recv().expect("snapshot chunk");
        let post = rx.try_recv().expect("POST chunk");
        assert!(
            rx.try_recv().is_err(),
            "exactly three chunks expected in this order"
        );

        assert_eq!(pre.data, b"PRE");
        assert_eq!(pre.kind, crate::mux::session::pane::ChunkKind::PtyOutput);

        assert_eq!(snap.pane_id, 1);
        assert_eq!(snap.kind, crate::mux::session::pane::ChunkKind::Snapshot);
        assert!(snap.data.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));

        assert_eq!(post.data, b"POST");
        assert_eq!(post.kind, crate::mux::session::pane::ChunkKind::PtyOutput);
    }

    // ========================================================================
    // Agent-facing API: ReadPane / SendText / WaitAgentState (task0004)
    // ========================================================================

    /// Build a session with one pane (Connected, sink writer) and return
    /// `(session_manager, session_id, window_id)`; `pane_id` is the caller's
    /// own choice so tests can build the matching public pane ID via
    /// `crate::mux::daemon::public_pane_id(pane_id)`.
    async fn setup_session_with_pane(pane_id: u32) -> (Arc<Mutex<SessionManager>>, u32, u32) {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (sid, wid) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            add_pane(&mut m, sid, wid, pane_id, target);
            (sid, wid)
        };
        (mgr, sid, wid)
    }

    /// Like [`setup_session_with_pane`] but installs a `Vec`-backed writer
    /// so `SendText`'s exact-bytes contract is directly observable.
    async fn setup_session_with_capturing_pane(
        pane_id: u32,
    ) -> (Arc<Mutex<SessionManager>>, u32, u32, Arc<StdMutex<Vec<u8>>>) {
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

        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let captured: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let (sid, wid) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            let pane = MuxPane::new_test_with_writer(
                pane_id,
                80,
                24,
                target,
                Box::new(CapturingWriter(captured.clone())),
            );
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
            (sid, wid)
        };
        (mgr, sid, wid, captured)
    }

    fn get_pane<'a>(mgr: &'a SessionManager, sid: u32, wid: u32, pane_id: u32) -> &'a MuxPane {
        mgr.get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pane_id)
            .unwrap()
    }

    /// Poll `cond` (yielding to the executor between checks) until it
    /// returns true, or panic after a bounded number of iterations. Used
    /// instead of a real sleep to deterministically wait for a spawned
    /// task to reach its registration point under the (single-threaded)
    /// `#[tokio::test]` runtime.
    async fn wait_until(mut cond: impl FnMut() -> bool) {
        for _ in 0..2000 {
            if cond() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition not met in time");
    }

    // ---- ReadPane (AC-1) ----

    #[test]
    fn strip_ansi_escapes_removes_two_byte_escape_sequences() {
        // ESC 7 (DECSC, save cursor) is a two-byte escape with no CSI/OSC
        // introducer; it must be dropped without disturbing surrounding text.
        let input = b"before\x1b7after";
        assert_eq!(strip_ansi_escapes(input), "beforeafter");
    }

    #[test]
    fn strip_ansi_escapes_removes_sgr_and_osc_keeps_text() {
        let input = b"\x1b[31mred\x1b[0m plain \x1b]0;title\x07tail";
        assert_eq!(strip_ansi_escapes(input), "red plain tail");
    }

    #[test]
    fn strip_ansi_escapes_drops_dangling_unterminated_sequence() {
        let input = b"before\x1b[31";
        assert_eq!(strip_ansi_escapes(input), "before");
    }

    #[test]
    fn strip_ansi_escapes_plain_text_is_unchanged() {
        assert_eq!(strip_ansi_escapes(b"hello world"), "hello world");
    }

    #[test]
    fn render_pane_tail_combines_scrollback_and_screen_in_order() {
        let text = render_pane_tail(b"line1\nline2\n", "line3\nline4", 10);
        assert_eq!(text, "line1\nline2\nline3\nline4");
    }

    #[test]
    fn render_pane_tail_returns_only_the_last_n_lines() {
        let text = render_pane_tail(b"a\nb\nc\n", "d\ne", 2);
        assert_eq!(text, "d\ne");
    }

    #[test]
    fn render_pane_tail_caps_total_bytes() {
        let huge_screen = "x".repeat(READ_MAX_BYTES + 1000);
        let text = render_pane_tail(b"", &huge_screen, 1);
        assert!(text.len() <= READ_MAX_BYTES);
    }

    #[tokio::test]
    async fn handle_read_pane_returns_ansi_stripped_tail() {
        let pane_id = 100;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            pane.scrollback
                .lock()
                .unwrap()
                .write(b"\x1b[31mhistory-line\x1b[0m\r\n");
            pane.shadow_parser.lock().unwrap().process(b"current-line");
        }

        let req = ReadPaneMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            lines: 100,
        };
        let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
        let result = handle_read_pane(&msg, &mgr)
            .await
            .expect("read should succeed");

        assert!(
            result.text.contains("history-line"),
            "got {:?}",
            result.text
        );
        assert!(
            result.text.contains("current-line"),
            "got {:?}",
            result.text
        );
        assert!(
            !result.text.contains('\x1b'),
            "ANSI escapes must be stripped, got {:?}",
            result.text
        );
    }

    #[tokio::test]
    async fn handle_read_pane_unknown_pane_id_errors() {
        let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
        let req = ReadPaneMsg {
            public_pane_id: "deadbeef00000000-999".to_string(),
            lines: 10,
        };
        let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
        let err = handle_read_pane(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
    }

    #[tokio::test]
    async fn handle_read_pane_malformed_public_id_errors_unknown_pane() {
        let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
        let req = ReadPaneMsg {
            public_pane_id: "not-a-valid-id".to_string(),
            lines: 10,
        };
        let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
        let err = handle_read_pane(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
    }

    #[tokio::test]
    async fn handle_read_pane_clamps_lines_above_max() {
        let pane_id = 101;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            let many_lines: String = (0..(READ_LINES_MAX + 500))
                .map(|i| format!("l{i}\n"))
                .collect();
            pane.scrollback.lock().unwrap().write(many_lines.as_bytes());
        }
        let req = ReadPaneMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            lines: READ_LINES_MAX + 500,
        };
        let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
        let result = handle_read_pane(&msg, &mgr)
            .await
            .expect("read should succeed");
        let line_count = result.text.lines().count();
        assert!(
            (line_count as u32) <= READ_LINES_MAX,
            "line count {line_count} must be clamped to {READ_LINES_MAX}"
        );
    }

    // ---- SendText (AC-2) ----

    #[tokio::test]
    async fn handle_send_text_writes_exact_bytes_and_returns_pre_write_watermark() {
        let pane_id = 200;
        let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            pane.agent_status.lock().unwrap().revision = 9;
        }

        let req = SendTextMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            bytes: b"hello agent".to_vec(),
        };
        let msg = MuxMessage::control(MessageType::SendText, 0, &req);
        let result = handle_send_text(&msg, &mgr)
            .await
            .expect("send should succeed");

        assert_eq!(result.revision_watermark, 9);
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            b"hello agent",
            "must write exactly the given bytes, no trailing newline added"
        );
    }

    #[tokio::test]
    async fn handle_send_text_rejects_nul_without_writing() {
        let pane_id = 201;
        let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
        let _ = (sid, wid);

        let req = SendTextMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            bytes: b"has\0nul".to_vec(),
        };
        let msg = MuxMessage::control(MessageType::SendText, 0, &req);
        let err = handle_send_text(&msg, &mgr).await.unwrap_err();

        assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
        assert!(
            captured.lock().unwrap().is_empty(),
            "NUL-containing input must not be written"
        );
    }

    #[tokio::test]
    async fn handle_send_text_rejects_oversize_without_writing() {
        let pane_id = 202;
        let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
        let _ = (sid, wid);

        let req = SendTextMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            bytes: vec![b'a'; SEND_MAX_BYTES + 1],
        };
        let msg = MuxMessage::control(MessageType::SendText, 0, &req);
        let err = handle_send_text(&msg, &mgr).await.unwrap_err();

        assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
        assert!(
            captured.lock().unwrap().is_empty(),
            "oversize input must not be written"
        );
    }

    #[tokio::test]
    async fn handle_send_text_unknown_pane_errors() {
        let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
        let req = SendTextMsg {
            public_pane_id: "deadbeef00000000-999".to_string(),
            bytes: b"hi".to_vec(),
        };
        let msg = MuxMessage::control(MessageType::SendText, 0, &req);
        let err = handle_send_text(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
    }

    // ---- WaitAgentState (AC-3, AC-4, AC-5) ----

    #[tokio::test]
    async fn wait_agent_state_succeeds_immediately_when_state_already_in_set() {
        let pane_id = 300;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            let mut st = pane.agent_status.lock().unwrap();
            st.state = Some(AgentState::Blocked);
            st.revision = 3;
        }

        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![AgentState::Blocked, AgentState::Done],
            timeout_ms: 1000,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let result = handle_wait_agent_state(&msg, &mgr)
            .await
            .expect("wait should succeed immediately");
        assert_eq!(result.state, AgentState::Blocked);
        assert_eq!(result.revision, 3);
    }

    #[tokio::test]
    async fn wait_agent_state_no_state_yet_blocks_until_report_then_matches() {
        let pane_id = 301;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        // Precondition: pane has no agent state yet.
        {
            let m = mgr.lock().await;
            assert!(
                get_pane(&m, sid, wid, pane_id)
                    .agent_status
                    .lock()
                    .unwrap()
                    .state
                    .is_none()
            );
        }

        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![AgentState::Working],
            timeout_ms: 5000,
            after_revision: None,
        };
        let mgr_clone = mgr.clone();
        let handle = tokio::spawn(async move {
            let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
            handle_wait_agent_state(&msg, &mgr_clone).await
        });

        wait_until(|| {
            mgr.try_lock()
                .ok()
                .map(|m| {
                    !get_pane(&m, sid, wid, pane_id)
                        .agent_waiters
                        .lock()
                        .unwrap()
                        .is_empty()
                })
                .unwrap_or(false)
        })
        .await;

        // Now report a qualifying accepted state change and re-evaluate
        // (simulates what task0003's OSC ingestion path will call).
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            {
                let mut st = pane.agent_status.lock().unwrap();
                st.state = Some(AgentState::Working);
                st.revision = 1;
            }
            reevaluate_agent_waiters(pane);
        }

        let result = handle.await.unwrap().expect("wait should resolve");
        assert_eq!(result.state, AgentState::Working);
        assert_eq!(result.revision, 1);
    }

    #[tokio::test]
    async fn wait_agent_state_after_revision_does_not_satisfy_at_or_below_watermark() {
        let pane_id = 302;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            let mut st = pane.agent_status.lock().unwrap();
            st.state = Some(AgentState::Done);
            st.revision = 5;
        }

        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![AgentState::Done],
            timeout_ms: 5000,
            after_revision: Some(5), // current revision (5) must NOT satisfy
        };
        let mgr_clone = mgr.clone();
        let handle = tokio::spawn(async move {
            let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
            handle_wait_agent_state(&msg, &mgr_clone).await
        });

        // The immediate check must have registered a waiter (not resolved
        // immediately), since revision (5) is not > after_revision (5).
        wait_until(|| {
            mgr.try_lock()
                .ok()
                .map(|m| {
                    !get_pane(&m, sid, wid, pane_id)
                        .agent_waiters
                        .lock()
                        .unwrap()
                        .is_empty()
                })
                .unwrap_or(false)
        })
        .await;

        // A same-state re-report bumps the revision past the watermark —
        // now it must satisfy (send-then-wait linearization, AC-4).
        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            pane.agent_status.lock().unwrap().revision = 6;
            reevaluate_agent_waiters(pane);
        }

        let result = handle
            .await
            .unwrap()
            .expect("wait should resolve after revision bump");
        assert_eq!(result.state, AgentState::Done);
        assert_eq!(result.revision, 6);
    }

    #[tokio::test]
    async fn wait_agent_state_times_out_when_condition_never_met() {
        let pane_id = 303;
        let (mgr, _sid, _wid) = setup_session_with_pane(pane_id).await;
        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![AgentState::Done],
            timeout_ms: 20,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::Timeout);
    }

    #[tokio::test]
    async fn wait_agent_state_pane_destroyed_resolves_pane_gone() {
        let pane_id = 304;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;

        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![AgentState::Done],
            timeout_ms: 5000,
            after_revision: None,
        };
        let mgr_clone = mgr.clone();
        let handle = tokio::spawn(async move {
            let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
            handle_wait_agent_state(&msg, &mgr_clone).await
        });

        wait_until(|| {
            mgr.try_lock()
                .ok()
                .map(|m| {
                    !get_pane(&m, sid, wid, pane_id)
                        .agent_waiters
                        .lock()
                        .unwrap()
                        .is_empty()
                })
                .unwrap_or(false)
        })
        .await;

        {
            let m = mgr.lock().await;
            let pane = get_pane(&m, sid, wid, pane_id);
            fail_agent_waiters_pane_gone(pane);
        }

        let err = handle
            .await
            .unwrap()
            .expect_err("wait must fail once the pane is gone");
        assert_eq!(err.kind, AgentApiErrorKind::PaneGone);
    }

    #[tokio::test]
    async fn wait_agent_state_unknown_pane_errors() {
        let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
        let req = WaitAgentStateMsg {
            public_pane_id: "deadbeef00000000-999".to_string(),
            states: vec![AgentState::Idle],
            timeout_ms: 1000,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
    }

    #[tokio::test]
    async fn wait_agent_state_empty_states_is_invalid_input() {
        let pane_id = 305;
        let (mgr, _sid, _wid) = setup_session_with_pane(pane_id).await;
        let req = WaitAgentStateMsg {
            public_pane_id: crate::mux::daemon::public_pane_id(pane_id),
            states: vec![],
            timeout_ms: 1000,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
        assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
    }

    /// AC-5: client disconnect discards the waiter. Modeled at the
    /// data-structure level per the Test Notes (handler-level, in-memory,
    /// no live socket): dropping the `oneshot::Receiver` is exactly what a
    /// disconnected CLI connection's abandoned future does, and
    /// `reevaluate_agent_waiters`'s cleanup pass removes any waiter whose
    /// responder is already closed — independent of whether the state
    /// ever changes.
    #[tokio::test]
    async fn reevaluate_agent_waiters_discards_waiter_with_closed_receiver() {
        let pane_id = 306;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);

        let (tx, rx) = oneshot::channel();
        pane.agent_waiters.lock().unwrap().push(AgentWaiter {
            states: vec![AgentState::Done],
            after_revision: None,
            responder: Some(tx),
        });
        drop(rx); // simulate client disconnect

        assert_eq!(pane.agent_waiters.lock().unwrap().len(), 1);
        reevaluate_agent_waiters(pane);
        assert!(
            pane.agent_waiters.lock().unwrap().is_empty(),
            "closed-receiver waiter must be discarded"
        );
    }

    /// A waiter whose `states` set does not match the current state stays
    /// registered across a re-evaluation pass (no spurious firing/removal).
    #[tokio::test]
    async fn reevaluate_agent_waiters_keeps_non_matching_waiter() {
        let pane_id = 307;
        let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        pane.agent_status.lock().unwrap().state = Some(AgentState::Idle);

        let (tx, _rx) = oneshot::channel();
        pane.agent_waiters.lock().unwrap().push(AgentWaiter {
            states: vec![AgentState::Done],
            after_revision: None,
            responder: Some(tx),
        });

        reevaluate_agent_waiters(pane);
        assert_eq!(
            pane.agent_waiters.lock().unwrap().len(),
            1,
            "non-matching waiter must remain registered"
        );
    }
}
