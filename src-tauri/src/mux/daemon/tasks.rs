//! Daemon-side background tasks: pane title updates, desktop
//! notification relay, agent-status ingestion / broadcast, pane-exit
//! reaping, and the graceful shutdown sweep.

use super::*;
/// Apply a title change to the SessionManager with diff detection.
///
/// Returns `true` when `window.name` was updated and a broadcast was sent;
/// `false` when the pane was not found or the title was unchanged.
pub(super) async fn apply_title_change(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    new_title: String,
) -> bool {
    let mut mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        log::warn!("apply_title_change: pane {} not found", pane_id);
        return false;
    };
    let unchanged = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .map(|w| w.name == new_title)
        .unwrap_or(false);
    if unchanged {
        return false;
    }
    log::info!(
        "Title change: pane {} -> window {} -> '{}'",
        pane_id,
        wid,
        new_title
    );
    mgr.rename_window(sid, wid, new_title.clone());
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    let rename_payload = RenameWindowMsg { name: new_title };
    let msg = MuxMessage::control(MessageType::RenameWindow, wid, &rename_payload);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!("apply_title_change: no active subscribers: {}", e);
    }
    true
}

/// Run the daemon-level title update task.
///
/// Exits when all senders are dropped (daemon shutdown).
pub(super) async fn run_title_update_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut title_rx: mpsc::Receiver<(u32, String)>,
) {
    log::info!("Title update task started");
    while let Some((pane_id, new_title)) = title_rx.recv().await {
        apply_title_change(&session_manager, pane_id, new_title).await;
    }
    log::info!("Title update task exiting");
}

/// Broadcast a Detached-pane OSC 9 notification to connected GUI clients.
///
/// The notification is sent via the SessionManager `notify_tx` broadcast; the
/// per-connection select! loop forwards it to its GUI client. If no GUI client
/// is currently subscribed the broadcast simply has no receivers (the
/// notification is fire-and-forget; FR5 keeps it out of any replay buffer, so
/// nothing replays it later). The GUI fires the OS notification (NFR3).
async fn relay_notification(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    message: String,
) {
    let notify_tx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().clone()
    };
    let payload = NotifyMsg { message };
    let msg = MuxMessage::control(MessageType::Notify, pane_id, &payload);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!("relay_notification: no active subscribers: {}", e);
    }
}

/// Run the daemon-level notification relay task.
///
/// Consumes `(pane_id, message)` from Detached pane reader threads and
/// broadcasts each as a `Notify` control message to GUI clients. Exits when
/// all senders are dropped (daemon shutdown).
pub(super) async fn run_notification_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut notification_rx: mpsc::Receiver<(u32, String)>,
) {
    log::info!("Notification relay task started");
    while let Some((pane_id, message)) = notification_rx.recv().await {
        relay_notification(&session_manager, pane_id, message).await;
    }
    log::info!("Notification relay task exiting");
}

/// Map the core (build-agnostic) `AgentState` to the `mux_ipc` wire mirror
/// enum. Two distinct types by design: `mux_ipc` must not depend on the
/// binary crate (task0002 IMPLEMENTATION.md), so this conversion (and its
/// inverse, [`from_wire_state`]) is the only place the two ever meet.
/// Widened beyond this module (`pub(in crate::mux)`) so the agent API
/// handlers (task0004, `mux::ipc::handlers`) share the same conversion
/// rather than re-deriving it.
pub(in crate::mux) fn to_wire_state(
    state: crate::agent_status::AgentState,
) -> crate::mux::ipc::protocol::AgentState {
    use crate::agent_status::AgentState as Core;
    use crate::mux::ipc::protocol::AgentState as Wire;
    match state {
        Core::Idle => Wire::Idle,
        Core::Working => Wire::Working,
        Core::Blocked => Wire::Blocked,
        Core::Done => Wire::Done,
    }
}

/// Inverse of [`to_wire_state`]: map the `mux_ipc` wire `AgentState` to the
/// core (build-agnostic) enum. Used by the agent API's `WaitAgentState`
/// handler to match a request's wire `states` set against pane state held
/// in the core type.
pub(in crate::mux) fn from_wire_state(
    state: crate::mux::ipc::protocol::AgentState,
) -> crate::agent_status::AgentState {
    use crate::agent_status::AgentState as Core;
    use crate::mux::ipc::protocol::AgentState as Wire;
    match state {
        Wire::Idle => Core::Idle,
        Wire::Working => Core::Working,
        Wire::Blocked => Core::Blocked,
        Wire::Done => Core::Done,
    }
}

/// Re-evaluate `pane`'s registered `WaitAgentState` waiters (task0004 "Wait
/// implementation", level-triggered, no polling) and build the
/// `AgentStatusUpdate` (`replay_derived: false`) message for its CURRENT
/// state at `revision`. Caller still holds `mgr`'s lock (needed for
/// `public_pane_id`) and is responsible for dropping it and sending the
/// returned message afterward.
///
/// Shared by [`apply_agent_status_report`] (explicit OSC 777 Set/Clear) and
/// [`apply_live_osc133_mark`] (task0003, SPEC FR1/FR2 — the inferred clear a
/// live OSC 133 `D`→`A` transition produces) so both ways a pane's
/// agent-status revision can change go through IDENTICAL waiter
/// re-evaluation / broadcast-payload logic — no parallel logic (FR2).
fn build_agent_status_update_message(
    mgr: &SessionManager,
    pane: &MuxPane,
    pane_id: u32,
    revision: u64,
) -> MuxMessage {
    reevaluate_agent_waiters(pane);
    let (state, name) = {
        let status = pane.agent_status.lock().unwrap();
        (status.state, status.name.clone())
    };
    let public_pane_id = mgr.public_pane_id(pane_id);
    let payload = AgentStatusUpdateMsg {
        pane_id,
        public_pane_id,
        state: state.map(to_wire_state),
        name,
        revision,
        replay_derived: false,
    };
    MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &payload)
}

/// Apply one raw agent-status OSC report to its pane and broadcast the
/// result (SPEC FR3 / FR5, task0003 AC-1/AC-2/AC-4).
///
/// Validates `raw_payload` via [`crate::agent_status::parse`]; a rejected
/// (`None`) parse leaves ALL state untouched and broadcasts nothing (AC-2).
/// An accepted event is applied to the pane (revision increments) and
/// exactly one `AgentStatusUpdate` (`replay_derived: false`) is broadcast
/// with the pane's current public ID (AC-4).
pub(super) async fn apply_agent_status_report(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    raw_payload: String,
) {
    let Some(event) = crate::agent_status::parse(&raw_payload) else {
        // Rejected sequence: no state change, no broadcast (AC-2).
        return;
    };

    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        log::warn!("apply_agent_status_report: pane {} not found", pane_id);
        return;
    };
    let Some(pane) = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id))
    else {
        return;
    };

    let revision = pane.apply_agent_status_event(event);
    let msg = build_agent_status_update_message(&mgr, pane, pane_id, revision);
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);

    if let Err(e) = notify_tx.send(msg) {
        log::debug!("apply_agent_status_report: no active subscribers: {}", e);
    }
}

/// Apply one live OSC 133 mark to its pane's inferred-clear latch and — only
/// if the mark completes an armed `D`→`A` transition — broadcast the
/// resulting inferred clear (task0003, SPEC FR1/FR2/FR3/FR4/FR5).
///
/// Delegates the actual latch update and, on firing, the clear application
/// to [`crate::mux::session::pane::MuxPane::record_live_osc133_mark`] — this
/// function's only job is finding the pane and, when a clear DID fire,
/// broadcasting it through the exact same
/// [`build_agent_status_update_message`] logic [`apply_agent_status_report`]
/// uses, so mux panes get identical downstream effects (revision increment,
/// waiter re-evaluation, `AgentStatusUpdate` push) regardless of which path
/// produced the clear. A mark that produces no clear (AC-2: `A` with no
/// preceding `D`; AC-3: disarmed after an explicit `Clear`) broadcasts
/// nothing and leaves state untouched.
pub(super) async fn apply_live_osc133_mark(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    kind: PromptMarkKind,
) {
    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        log::debug!("apply_live_osc133_mark: pane {} not found", pane_id);
        return;
    };
    let Some(pane) = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id))
    else {
        return;
    };

    let Some(revision) = pane.record_live_osc133_mark(kind) else {
        // No inferred clear fired: no state change, no broadcast.
        return;
    };
    let msg = build_agent_status_update_message(&mgr, pane, pane_id, revision);
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);

    if let Err(e) = notify_tx.send(msg) {
        log::debug!("apply_live_osc133_mark: no active subscribers: {}", e);
    }
}

/// Run the daemon-level agent-status task.
///
/// Consumes `(pane_id, item)` from every pane's reader thread (regardless of
/// attach state, SPEC FR3) and dispatches each [`AgentStatusFeedItem`] to
/// [`apply_agent_status_report`] (an OSC 777 report) or
/// [`apply_live_osc133_mark`] (task0003, a live OSC 133 mark) IN RECEIVE
/// ORDER — a single sequential `while let` loop over one channel, never two
/// independently-scheduled queues, is what gives SPEC FR4 its ordering
/// guarantee. Exits when all senders are dropped (daemon shutdown).
pub(super) async fn run_agent_status_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut agent_status_rx: mpsc::Receiver<(u32, AgentStatusFeedItem)>,
) {
    log::info!("Agent-status task started");
    while let Some((pane_id, item)) = agent_status_rx.recv().await {
        match item {
            AgentStatusFeedItem::Report(raw_payload) => {
                apply_agent_status_report(&session_manager, pane_id, raw_payload).await;
            }
            AgentStatusFeedItem::Osc133Mark(kind) => {
                apply_live_osc133_mark(&session_manager, pane_id, kind).await;
            }
        }
    }
    log::info!("Agent-status task exiting");
}

/// Broadcast one `AgentStatusUpdate` (`replay_derived: true`) per pane in
/// `session_id` whose GUI-visible state may need replacement after a
/// snapshot (SPEC FR4/FR5, task0003 AC-5, task0013 AC-1/AC-2/AC-3). Called
/// after a client receives a snapshot (attach / window switch) so state —
/// stripped from the replayed bytes — is resynced out-of-band.
///
/// Emits for every pane with `revision > 0`, i.e. every pane that has ever
/// had an accepted report, REGARDLESS of whether its current `state` is
/// `Some` or `None`: revision starts at 0 and only increments on an
/// accepted report (set, clear, or same-state re-report — see
/// `AgentStatus`), so `revision > 0` is exactly "this pane's GUI-visible
/// state may be stale" without an extra flag. This covers a pane that was
/// cleared while the GUI was detached (state is `None` here, but the GUI
/// may still show a stale badge) — the message carries `state: None` in
/// that case so the GUI clears it. Panes that have never reported
/// (`revision == 0`) produce no message, since the GUI has no stale state
/// to clear for them.
pub(in crate::mux) async fn sync_agent_status_after_snapshot(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
) {
    let mgr = session_manager.lock().await;
    let Some(session) = mgr.get_session(session_id) else {
        return;
    };
    let mut updates = Vec::new();
    for (_wid, pane) in session.panes_iter() {
        let status = pane.agent_status.lock().unwrap();
        if status.revision == 0 {
            continue;
        }
        updates.push(AgentStatusUpdateMsg {
            pane_id: pane.id,
            public_pane_id: mgr.public_pane_id(pane.id),
            state: status.state.map(to_wire_state),
            name: status.name.clone(),
            revision: status.revision,
            replay_derived: true,
        });
    }
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    for update in updates {
        let pane_id = update.pane_id;
        let msg = MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &update);
        if let Err(e) = notify_tx.send(msg) {
            log::debug!(
                "sync_agent_status_after_snapshot: no active subscribers: {}",
                e
            );
        }
    }
}

/// Single-pane counterpart of [`sync_agent_status_after_snapshot`] (SPEC
/// FR4/FR5, task0003 AC-5, task0013 AC-1/AC-2/AC-3): broadcasts one
/// `AgentStatusUpdate` (`replay_derived: true`) for `pane_id` if it has
/// `revision > 0` (ever had an accepted report — see the doc comment on
/// `sync_agent_status_after_snapshot` for why this covers cleared state
/// too). Used after an on-demand per-pane snapshot (`RequestPaneSnapshot`,
/// the same-session window-switch path) rather than a full session attach.
pub(in crate::mux) async fn sync_agent_status_after_pane_snapshot(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: PaneId,
) {
    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        return;
    };
    let Some(pane) = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id))
    else {
        return;
    };
    let status = pane.agent_status.lock().unwrap();
    if status.revision == 0 {
        return;
    }
    let update = AgentStatusUpdateMsg {
        pane_id,
        public_pane_id: mgr.public_pane_id(pane_id),
        state: status.state.map(to_wire_state),
        name: status.name.clone(),
        revision: status.revision,
        replay_derived: true,
    };
    drop(status);
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &update);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!(
            "sync_agent_status_after_pane_snapshot: no active subscribers: {}",
            e
        );
    }
}

/// Run the daemon-level pane-exit reap task.
///
/// Consumes a bare `PaneId` from each per-pane reader thread that observed
/// PTY EOF (sent regardless of attach state, FR1) and reaps the pane via
/// `handle_destroy_pane`, making "PTY death -> reap" the single authority
/// independent of attach state (FR2). Because reap is keyed on `pane_id` and
/// ignores the pane's `output_target`, this covers the detached path and the
/// connection-reset race (FR6) uniformly, and is a safe no-op when the pane
/// was already reaped via the Connected empty-chunk path (FR4). When the
/// reaped pane is the last one, `handle_destroy_pane` fires
/// `shutdown_tx.send(true)` (FR5). Exits when all senders are dropped (daemon
/// shutdown).
pub(super) async fn run_pane_exit_task(
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    mut pane_exit_rx: mpsc::Receiver<PaneId>,
) {
    log::info!("Pane-exit reap task started");
    while let Some(pane_id) = pane_exit_rx.recv().await {
        handle_destroy_pane(pane_id, &session_manager, &shutdown_tx).await;
    }
    log::info!("Pane-exit reap task exiting");
}

/// Close all PTYs in all sessions for graceful daemon shutdown.
pub(super) async fn graceful_shutdown(session_manager: &Arc<Mutex<SessionManager>>) {
    let mut mgr = session_manager.lock().await;
    let mut pane_count = 0u32;
    let session_ids: Vec<u32> = mgr.sessions_iter().map(|s| s.id).collect();
    for session_id in session_ids {
        if let Some(session) = mgr.get_session_mut(session_id) {
            for window in session.windows.values_mut() {
                for pane in window.panes.values_mut() {
                    if !pane.exited {
                        pane.mark_exited();
                        pane_count += 1;
                    }
                }
            }
        }
    }
    log::info!("Graceful shutdown: closed {} PTY(s)", pane_count);
}
