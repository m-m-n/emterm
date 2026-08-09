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

/// Handle Attach message: switch the client to a different session.
///
/// Detaches panes from the current session, updates the active session,
/// and reattaches panes from the new session with buffered output replay.
///
/// Also allocates a fresh kick channel: the sender is installed on the new
/// session (firing any previously-installed kick to evict the prior client),
/// and the receiver is written to `kick_rx` so the connection loop can await
/// it in its select!. Any prior receiver held by the caller is replaced.
///
/// Only called from the GUI loop (`route_message`) — never from the
/// CLI-client path — so `admission` (task0001, task0003 rework) is the
/// outbound admission component directly, with no need for the
/// [`ReplySink`] abstraction `handle_create_window` uses to serve both
/// paths. Every send below (the error reply, and every frame
/// `send_reattach_data` emits) is an ORDERED BLOCKING admission
/// (`OutboundAdmission::admit_blocking`): it drains any remainder already
/// held by an earlier producer FIRST, then admits its own frame(s) — so a
/// reattach's `PaneCreated`/`SnapshotRestore` can never overtake older
/// held `PtyOutput` (FR3, the worst case Design "Problem" names).
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_attach(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    admission: &mut OutboundAdmission,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
    title_tx: &TitleChangeSender,
    kick_rx: &mut Option<oneshot::Receiver<()>>,
    visible_state: &Arc<AtomicBool>,
) -> Result<(), bool> {
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
            let _ = admission.admit_blocking(vec![resp]).await;
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
    // snapshot frame is sent (the frontend learns the pane exists but
    // receives no screen contents until the next SetVisibility(true) resume).
    if send_reattach_data(admission, &reattach_data).await.is_err() {
        return Err(true);
    }

    // SPEC FR4/FR5 (task0003 AC-5): the snapshot bytes just delivered had
    // agent-status OSC stripped (scrollback_filter); resync current state
    // out-of-band, one `AgentStatusUpdate` (`replay_derived: true`) per
    // stateful pane in the session.
    crate::mux::daemon::sync_agent_status_after_snapshot(session_manager, new_session_id).await;

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
///
/// mux-window-switch-output-hang task0001/task0002: reserving a permit is
/// `.reserve().await` — exactly the same self-blockable shape as
/// `handle_request_pane_snapshot`'s old blocking send, and reachable from
/// the SAME connection task via `route_message`'s `SetVisibility` arm. The
/// per-pane loop below therefore tries `try_reserve()` first (non-blocking,
/// the common case when the channel has room, runs the resume inline
/// exactly as before) and, only when the channel is momentarily full,
/// defers THAT pane's resume attempt onto the connection-owned
/// `deferred_output` queue instead (task0002 rework: no longer an
/// independently spawned task — see `DeferredOutputQueue`'s doc for why, and
/// `flush_deferred_output` below for the retry, which re-validates
/// `visible_state` fresh at flush time so a pane hidden again in the interim
/// is never resumed incorrectly, AC-1/F1/F2/F3).
pub(super) async fn handle_set_visibility(
    visible: bool,
    session_manager: &Arc<Mutex<SessionManager>>,
    active_session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    visible_state: &Arc<AtomicBool>,
    deferred_output: &mut DeferredOutputQueue,
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
        match pane_output_tx.try_reserve() {
            Ok(permit) => {
                let _ = resolve_pane_and_resume(
                    session_manager,
                    active_session_id,
                    pane_id,
                    pane_output_tx,
                    AnyPermit::Borrowed(permit),
                )
                .await;
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                // mux-window-switch-output-hang task0001/task0002: the
                // channel is momentarily full — do NOT `reserve().await`
                // here (this task's own drain arm is the only thing that
                // can free capacity, and it cannot run while this task is
                // suspended waiting for a permit, the exact self-deadlock
                // class this feature fixes). Defer this pane's resume onto
                // the connection-owned queue instead; the connection's own
                // select! loop keeps running in the meantime, and
                // `flush_deferred_output` retries it (re-validating
                // `visible_state` and re-resolving the pane fresh) the next
                // time capacity frees.
                log::warn!(
                    "[WARN][BACKEND] handle_set_visibility: pane_output_tx full; \
                     deferring visibility-resume for pane {} to the connection's \
                     own deferred queue",
                    pane_id
                );
                deferred_output.defer_visibility_resume(pane_id);
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                log::warn!(
                    "[WARN][BACKEND] handle_set_visibility: pane_output_tx closed; \
                     aborting resume for pane {} and remaining panes",
                    pane_id
                );
                return;
            }
        }
    }
}

/// Resolve `pane_id` in the CURRENT session and, if it is still eligible
/// (session still exists, pane still exists, not exited), hand it `permit`
/// via `resume_pane_with_permit`. Shared by `handle_set_visibility`'s
/// immediate fast path (channel had room) and `flush_deferred_output`'s
/// retry of a deferred visibility-resume — architecture medium finding
/// handlers.rs:786 (task0002 review round 1): this reserve -> resolve ->
/// resume block used to be duplicated verbatim across those two call sites
/// (once inline, once inside the old spawned task).
///
/// On any early-out (session gone, pane gone, pane exited), `permit` is
/// dropped so its reserved channel slot is released back to the channel
/// rather than leaked.
///
/// `permit` is an [`AnyPermit`] (task0003 rework, AC-3/G2) so this same
/// resolve/resume logic also serves the fair `reserve_owned()`-based
/// starvation-avoidance path in `apply_fair_permit_to_front_deferred_item`
/// below, not just the two `try_reserve`-based callers.
async fn resolve_pane_and_resume(
    session_manager: &Arc<Mutex<SessionManager>>,
    active_session_id: u32,
    pane_id: PaneId,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    permit: AnyPermit<'_>,
) -> ResumeOutcome {
    let mgr = session_manager.lock().await;
    let Some(session) = mgr.get_session(active_session_id) else {
        drop(permit);
        return ResumeOutcome::NoChange;
    };
    let pane = session
        .windows
        .values()
        .find_map(|w| w.panes.get(&pane_id))
        .filter(|p| !p.exited);
    let Some(pane) = pane else {
        drop(permit);
        return ResumeOutcome::NoChange;
    };
    resume_pane_with_permit(pane, pane_output_tx, permit)
}

/// Retry every item in `deferred_output` against `pane_output_tx`, stopping
/// at the first one that still can't be sent (preserves FIFO order — see
/// `DeferredOutputQueue`'s doc). Called by the connection's own event loop
/// (`mux::ipc::connection::handle_connection`) immediately after it drains
/// `pane_output_rx` — the only place capacity on that channel is ever freed
/// — and, defensively, at the top of `route_message` so a newly-arrived
/// client message also gives the queue a chance to progress even if PTY
/// output has gone quiet in the meantime.
///
/// `Chunk` items are retried verbatim via `try_send`. `VisibilityResume`
/// items are re-validated against `visible_state` FIRST (AC-1/F1/F2/F3: a
/// pane hidden again since the resume was deferred must not be resumed to
/// `Connected`), then attempted via `try_reserve` + `resolve_pane_and_resume`
/// — which re-resolves the pane from `session_manager` fresh, so a pane that
/// exited or moved sessions in the interim is simply skipped, exactly like
/// the immediate path above.
///
/// A `Closed` channel drops the ENTIRE remaining backlog (every future send
/// would fail identically, so there is nothing to gain by retaining it).
pub(super) async fn flush_deferred_output(
    deferred_output: &mut DeferredOutputQueue,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    session_manager: &Arc<Mutex<SessionManager>>,
    active_session_id: u32,
    visible_state: &Arc<AtomicBool>,
) {
    while let Some(item) = deferred_output.pop_front() {
        match item {
            DeferredOutputItem::Chunk(chunk) => match pane_output_tx.try_send(chunk) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(chunk)) => {
                    deferred_output.requeue_front(DeferredOutputItem::Chunk(chunk));
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(chunk)) => {
                    log::warn!(
                        "flush_deferred_output: pane_output_tx closed; dropping deferred \
                         chunk for pane {} and the rest of the backlog",
                        chunk.pane_id
                    );
                    deferred_output.clear();
                    return;
                }
            },
            DeferredOutputItem::VisibilityResume(pane_id) => {
                if !visible_state.load(Ordering::Acquire) {
                    // Pane was hidden again since this resume was deferred
                    // (AC-1/F1/F2/F3) — drop it. A later visible=true edge
                    // re-scans every non-exited pane from scratch, so this
                    // is a stale resume, not a missed one.
                    continue;
                }
                match pane_output_tx.try_reserve() {
                    Ok(permit) => {
                        let _ = resolve_pane_and_resume(
                            session_manager,
                            active_session_id,
                            pane_id,
                            pane_output_tx,
                            AnyPermit::Borrowed(permit),
                        )
                        .await;
                    }
                    Err(mpsc::error::TrySendError::Full(())) => {
                        deferred_output
                            .requeue_front(DeferredOutputItem::VisibilityResume(pane_id));
                        break;
                    }
                    Err(mpsc::error::TrySendError::Closed(())) => {
                        log::warn!(
                            "flush_deferred_output: pane_output_tx closed; dropping deferred \
                             visibility-resume for pane {} and the rest of the backlog",
                            pane_id
                        );
                        deferred_output.clear();
                        return;
                    }
                }
            }
        }
    }
}

/// Apply `permit` — obtained FAIRLY via `pane_output_tx.clone().reserve_owned()`
/// polled as its own `select!` arm in `mux::ipc::connection::handle_connection`
/// (AC-3/G2, mux-window-switch-output-hang task0003 rework) — to the FRONT
/// item of `deferred_output`.
///
/// Unlike `flush_deferred_output`'s `try_send`/`try_reserve` retries (which
/// can lose a freed permit to a parked PTY reader thread indefinitely, since
/// neither joins tokio's semaphore waiter queue — see `DeferredOutputQueue`'s
/// doc), `permit` here already holds a slot reserved through that SAME
/// queue, so applying it to a `Chunk` item always succeeds, and applying it
/// to a `VisibilityResume` item only ever gives the slot back (dropping
/// `permit`) when the item itself is stale (pane hidden again) or no longer
/// eligible (session/pane gone) — never because the channel was full.
///
/// A no-op (dropping `permit` without using it) when `deferred_output` is
/// already empty by the time this runs — e.g. another path (the ordinary
/// `try_send`-based flush) already drained it between the reservation being
/// armed and it resolving.
pub(super) async fn apply_fair_permit_to_front_deferred_item(
    deferred_output: &mut DeferredOutputQueue,
    permit: mpsc::OwnedPermit<PtyOutputChunk>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    session_manager: &Arc<Mutex<SessionManager>>,
    active_session_id: u32,
    visible_state: &Arc<AtomicBool>,
) {
    let Some(item) = deferred_output.pop_front() else {
        drop(permit);
        return;
    };
    match item {
        DeferredOutputItem::Chunk(chunk) => {
            let _ = permit.send(chunk);
        }
        DeferredOutputItem::VisibilityResume(pane_id) => {
            if !visible_state.load(Ordering::Acquire) {
                // Stale — pane was hidden again since this resume was
                // deferred (AC-1/F1/F2/F3). Drop the permit, releasing its
                // reserved slot back to the channel.
                drop(permit);
                return;
            }
            let _ = resolve_pane_and_resume(
                session_manager,
                active_session_id,
                pane_id,
                pane_output_tx,
                AnyPermit::Owned(permit),
            )
            .await;
        }
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
pub(super) const READ_LINES_MAX: u32 = 2000;
pub(super) const READ_MAX_BYTES: usize = 256 * 1024;
/// Raw scrollback bytes considered for the tail before VT100 rendering
/// (see `render_scrollback_rows`). Sized generously above `READ_LINES_MAX`
/// lines worth of typical terminal output so the rendered tail rarely
/// runs short.
const SCROLLBACK_READ_TAIL_BYTES: usize = 512 * 1024;

/// Cap on `SendText` payload size (NFR1: request validation).
pub(super) const SEND_MAX_BYTES: usize = 1024 * 1024;

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

/// Resolve a public-facing pane ID (opaque `"{incarnation}-{pane_id}"`
/// string minted by [`SessionManager::public_pane_id`]) to the internal
/// wire [`PaneId`]. Malformed input or a stale (previous-daemon-
/// incarnation) ID both resolve to `None` — every caller maps that
/// uniformly to `unknown_pane` per IMPLEMENTATION.md's shared error
/// contract ("Requests targeting an unknown ID (including a stale
/// incarnation) -> unknown_pane").
fn resolve_public_pane_id(mgr: &SessionManager, public_pane_id: &str) -> Option<PaneId> {
    let parsed = PublicPaneId::parse(public_pane_id).ok()?;
    if parsed.incarnation != mgr.incarnation() {
        return None;
    }
    Some(parsed.pane_id)
}

/// Look up a pane by its internal wire [`PaneId`] across every session.
fn find_pane<'a>(mgr: &'a SessionManager, pane_id: PaneId) -> Option<&'a MuxPane> {
    let (sid, wid) = mgr.find_pane(pane_id)?;
    mgr.get_session(sid)?.windows.get(&wid)?.panes.get(&pane_id)
}

/// Resolve `public_pane_id` all the way to its `MuxPane`, uniformly mapping
/// every failure mode (malformed ID, stale incarnation, resolvable-but-
/// absent pane) to `unknown_pane` (IMPLEMENTATION.md shared error contract;
/// `not_mux_pane` is the CLI-facing name for the same wire error, task0004
/// task plan "Design").
fn resolve_pane<'a>(
    mgr: &'a SessionManager,
    public_pane_id: &str,
) -> Result<&'a MuxPane, AgentApiError> {
    let pane_id = resolve_public_pane_id(mgr, public_pane_id)
        .ok_or_else(|| unknown_pane_error(public_pane_id))?;
    find_pane(mgr, pane_id).ok_or_else(|| unknown_pane_error(public_pane_id))
}

/// Render raw scrollback PTY bytes into plain-text rows by feeding them
/// through a scratch VT100 parser (task0011 REWORK, FR10 / AC-1): CR-based
/// overwrites, cursor movement, and erasure are honored exactly as a real
/// terminal would render them. The previous implementation only stripped
/// ANSI escape bytes and split on `\n`, which does not reproduce those
/// effects (an embedded `\r` was left as a literal character rather than
/// resetting the column, so an overwritten progress-bar line rendered as
/// concatenated garbage instead of its final state).
///
/// The scratch grid is sized `lines + 1` rows (not exactly `lines`): with
/// exactly `lines` rows, the terminal's own scroll-on-overflow would
/// discard one real content row to make room for the blank line the
/// cursor lands on after a trailing `\r\n` — the common case, since PTY
/// output almost always ends with a newline. The `+1` margin absorbs that
/// artifact (trimmed back off below); beyond `lines + 1` rows of history
/// the scratch parser's own terminal semantics naturally scroll older
/// content off the top, which is exactly the "last N rendered rows" tail
/// behavior FR10 asks for, without an unbounded intermediate buffer.
///
/// Only the row the cursor currently sits on is dropped, and only when it
/// is genuinely empty (the trailing-newline artifact) — real blank lines
/// earlier in the content are preserved.
fn render_scrollback_rows(scrollback_tail: &[u8], lines: u32, cols: u16) -> Vec<String> {
    let rows = lines
        .clamp(1, READ_LINES_MAX)
        .saturating_add(1)
        .min(u32::from(u16::MAX)) as u16;
    let cols = cols.max(1);
    let mut scratch = vt100::Parser::new(rows, cols, 0);
    scratch.process(scrollback_tail);
    let screen = scratch.screen();
    let (cursor_row, _cursor_col) = screen.cursor_position();
    let mut rendered: Vec<String> = screen
        .rows(0, cols)
        .take(usize::from(cursor_row) + 1)
        .collect();
    if matches!(rendered.last(), Some(r) if r.is_empty()) {
        rendered.pop();
    }
    rendered
}

/// Combine the rendered scrollback tail (AC-1) with the current screen's
/// plain-text contents and return the last `lines` lines, capped at
/// `READ_MAX_BYTES` by retaining the UTF-8-safe NEWEST suffix (AC-2) — the
/// previous implementation truncated from the end, keeping the oldest
/// prefix and dropping the newest output, which is backwards for a "tail"
/// read.
pub(super) fn render_pane_tail(
    scrollback_tail: &[u8],
    screen_contents: &str,
    lines: u32,
    cols: u16,
) -> String {
    let rendered_scrollback = render_scrollback_rows(scrollback_tail, lines, cols);
    let mut all_lines: Vec<&str> = rendered_scrollback.iter().map(String::as_str).collect();
    all_lines.extend(screen_contents.lines());

    let take = lines as usize;
    let start = all_lines.len().saturating_sub(take);
    let mut text = all_lines[start..].join("\n");

    if text.len() > READ_MAX_BYTES {
        let drop = text.len() - READ_MAX_BYTES;
        let mut cut = drop;
        while cut < text.len() && !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = text[cut..].to_string();
    }
    text
}

/// Handle `ReadPane`: return the tail `lines` RENDERED rows of a mux pane
/// (current screen + rendered scrollback tail), plain text with no
/// formatting/escape bytes (AC-1, FR10).
pub(super) async fn handle_read_pane(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) -> Result<ReadPaneResultMsg, AgentApiError> {
    let req: ReadPaneMsg = msg.decode_payload().ok_or_else(invalid_payload_error)?;
    let lines = req.lines.clamp(1, READ_LINES_MAX);

    let (shadow_parser, scrollback, cols) = {
        let mgr = session_manager.lock().await;
        let pane = resolve_pane(&mgr, &req.public_pane_id)?;
        (
            pane.shadow_parser.clone(),
            pane.scrollback.clone(),
            pane.cols,
        )
    };

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

    let text = render_pane_tail(&scrollback_tail, &screen_contents, lines, cols);
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

    // Lock hygiene (task0011 REWORK): resolve the pane and clone the two
    // handles the write needs — the PTY writer and the `agent_status` Arc
    // for the watermark — then release the manager lock BEFORE performing
    // the (potentially slow) synchronous PTY write. A stalled/non-
    // consuming child on THIS pane must not stall ReadPane /
    // WaitAgentState / pane lifecycle operations on every OTHER pane,
    // which all also need this lock (AC-3).
    let (writer_handle, agent_status) = {
        let mgr = session_manager.lock().await;
        let pane = resolve_pane(&mgr, &req.public_pane_id)?;
        let writer_handle = pane.writer_handle().ok_or_else(|| AgentApiError {
            kind: AgentApiErrorKind::PaneGone,
            message: format!("pane {} has no active writer", req.public_pane_id),
        })?;
        (writer_handle, pane.agent_status.clone())
    };
    // `mgr` (the manager MutexGuard) is dropped here, at scope end.

    // Watermark: the revision observed immediately before the write
    // (IMPLEMENTATION.md "Revision semantics"). `agent_status` is a
    // separate `std::sync::Mutex` from the manager lock, so reading it
    // after the manager lock is released observes the same value it would
    // have under the old lock-held-throughout scheme — only the manager
    // lock's scope has changed, not what this read synchronizes with.
    let watermark = agent_status.lock().unwrap().revision;

    // Perform the write on the blocking thread pool rather than inline in
    // this async fn: `write_all` + `flush` are synchronous and can block
    // on a non-consuming child, which must not stall the Tokio worker
    // thread driving other tasks. Atomicity per request (AC-5) is
    // preserved because `writer_handle` still points at the pane's single
    // `std::sync::Mutex`-guarded writer (see `write_via_writer_handle`) —
    // concurrent sends to the SAME pane serialize on that mutex exactly as
    // they did when the write ran inline under the manager lock.
    let bytes = req.bytes;
    tokio::task::spawn_blocking(move || {
        crate::mux::session::pane::write_via_writer_handle(&writer_handle, &bytes)
    })
    .await
    .map_err(|e| AgentApiError {
        kind: AgentApiErrorKind::InvalidInput,
        message: format!("write task failed: {e}"),
    })?
    .map_err(|e| AgentApiError {
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
    status: &AgentStatus,
    states: &[CoreAgentState],
    after_revision: Option<u64>,
) -> Option<(CoreAgentState, u64)> {
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
/// re-evaluated on every accepted report). Called by the daemon's
/// agent-status ingestion path (`mux::daemon::apply_agent_status_report`)
/// after every accepted report (set/clear/same-state re-report).
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
    let core_states: Vec<CoreAgentState> =
        req.states.iter().copied().map(from_wire_state).collect();

    let (agent_status, agent_waiters) = {
        let mgr = session_manager.lock().await;
        let pane = resolve_pane(&mgr, &req.public_pane_id)?;
        (pane.agent_status.clone(), pane.agent_waiters.clone())
    };

    let rx = {
        let status = agent_status.lock().unwrap();
        if let Some((state, revision)) =
            check_wait_immediate(&status, &core_states, req.after_revision)
        {
            return Ok(WaitAgentStateResultMsg {
                state: to_wire_state(state),
                revision,
            });
        }
        let (tx, rx) = oneshot::channel();
        agent_waiters.lock().unwrap().push(AgentWaiter {
            states: core_states,
            after_revision: req.after_revision,
            responder: Some(tx),
        });
        rx
        // `status` (agent_status lock) is dropped here, after registration —
        // closing the check-then-register race window.
    };

    let timeout = std::time::Duration::from_millis(req.timeout_ms);
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(AgentWaitOutcome::Matched { state, revision })) => Ok(WaitAgentStateResultMsg {
            state: to_wire_state(state),
            revision,
        }),
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
mod tests;
