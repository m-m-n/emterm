//! Attach / visibility handlers and the deferred-output flush path for
//! reattached or newly-visible panes.

use super::*;
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
pub(in crate::mux::ipc) async fn handle_attach(
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
pub(in crate::mux::ipc) async fn handle_set_visibility(
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
pub(in crate::mux::ipc) async fn flush_deferred_output(
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
pub(in crate::mux::ipc) async fn apply_fair_permit_to_front_deferred_item(
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
