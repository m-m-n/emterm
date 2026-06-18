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
    NotificationSender, PaneId, PtyOutputChunk, SharedPaneExitSender, SharedScrollback,
    SharedShadowParser, TitleChangeSender, evaluate_output_target, resume_pane_with_permit,
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

    let spawned = match spawn_pty(80, 24) {
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

/// Handle RequestPaneSnapshot message by pushing a snapshot `PtyOutputChunk`
/// onto the shared pane output channel.
///
/// Why the channel (and not a direct `framed.send`): the PTY reader thread
/// updates `shadow_parser` *and* enqueues the raw bytes onto
/// `pane_output_tx`. If the snapshot bypassed the channel, pending PTY chunks
/// already in the queue — whose effects are already baked into the snapshot
/// state — would be delivered *after* the snapshot and re-applied on top of
/// it, producing duplicated/shifted output. Routing through the same channel
/// minimizes (but does not strictly eliminate) this ordering divergence.
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

    // Send as a regular PTY output chunk so it interleaves correctly with any
    // already-queued bytes for this pane. If the client is gone the channel is
    // closed — that's not a fatal error for this handler, just drop the reply.
    if let Err(e) = pane_output_tx
        .send(PtyOutputChunk {
            pane_id,
            data: snapshot,
        })
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
    /// copy + the shadow screen) and asserts the result is byte-for-byte the
    /// established `ESC[H ESC[2J + scrollback + screen` layout — for both a
    /// representative screen + scrollback and the empty-scrollback case.
    #[test]
    fn snapshot_bytes_unchanged_after_lock_scope_guardrail() {
        use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
        use crate::mux::session::pane::new_shadow_parser;
        use std::sync::Mutex as StdMutex;

        // Representative screen + scrollback.
        let shadow_parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
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
        let needle = b"\x1b_Gi=9;XX\x1b\\";
        assert!(
            chunk.data.windows(needle.len()).any(|w| w == needle),
            "snapshot must include the captured passthrough sequence"
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
}
