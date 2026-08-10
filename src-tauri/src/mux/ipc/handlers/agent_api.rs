//! Agent-facing API handlers: ReadPane / SendText / WaitAgentState,
//! their size caps, pane resolution, and agent-state waiter management.

use std::sync::Arc;

use mux_ipc::protocol::{
    AgentApiError, AgentApiErrorKind, MuxMessage, PublicPaneId, ReadPaneMsg, ReadPaneResultMsg,
    SendTextMsg, SendTextResultMsg, WaitAgentStateMsg, WaitAgentStateResultMsg,
};
use tokio::sync::{Mutex, oneshot};

use crate::agent_status::AgentState as CoreAgentState;
use crate::mux::daemon::{from_wire_state, to_wire_state};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatus, AgentWaitOutcome, AgentWaiter, MuxPane, PaneId, lock_shadow_parser,
};

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
pub(in crate::mux::ipc) async fn handle_read_pane(
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
pub(in crate::mux::ipc) async fn handle_send_text(
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
pub(in crate::mux::ipc) async fn handle_wait_agent_state(
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
