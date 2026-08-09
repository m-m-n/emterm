//! Hot-upgrade preparation and handoff-mode startup on the daemon side:
//! candidate probing, session-tree snapshot assembly, the announce / ack
//! round, and starting from a handoff document written by the old daemon.

use super::*;
// ============================================================================
// mux-daemon-hot-upgrade task0009 (rework): upgrade preparation and
// handoff-mode startup, wired to the REAL implementation.
//
// Round 1 shipped this section with `snapshot`/`restore` PLACEHOLDERS: the
// real session-tree snapshot/restore (`crate::mux::upgrade`) existed but was
// never called from here, so every upgrade with a live pane was refused and
// every handoff start discarded the session tree. This section now calls
// `crate::mux::upgrade::{snapshot, restore, adopt_listener,
// read_and_remove_handoff_file, handoff_file_path}` directly — `upgrade.rs`
// is the single owner of the handoff file's path/creation/read/removal and
// of descriptor adoption; nothing here duplicates that.
// ============================================================================

/// Ask `candidate` (the binary about to replace this process) which handoff
/// schema versions it can restore, by running its `probe-handoff`
/// subcommand (task0005's contract, real: `mux::cli::execute_probe_handoff`
/// prints `"<min> <max>"` and exits 0). Any spawn failure, non-zero exit, or
/// unparsable output means "incompatible" (IMPLEMENTATION.md D3) -- there is
/// no partial-trust fallback.
///
/// `deadline` bounds how long this call polls the spawned subprocess for:
/// unlike wrapping this whole function in `tokio::time::timeout` (which only
/// stops AWAITING it and leaves the blocking-pool thread and the child
/// process itself running forever), this function owns the child directly
/// (`Command::spawn`, not `Command::output`) and actively `kill()`s it if
/// `deadline` passes before it exits, so a hung candidate binary is
/// terminated rather than merely abandoned.
#[cfg(unix)]
pub(super) fn probe_candidate_handoff_range(
    candidate: &Path,
    deadline: std::time::Instant,
) -> Result<std::ops::RangeInclusive<u32>, String> {
    let mut child = std::process::Command::new(candidate)
        .args(["mux", "probe-handoff"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run handoff probe on {candidate:?}: {e}"))?;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "handoff probe on {candidate:?} timed out and was killed"
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!(
                    "failed to wait on handoff probe subprocess for {candidate:?}: {e}"
                ));
            }
        }
    };

    let mut stdout_buf = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        use std::io::Read;
        let _ = out.read_to_end(&mut stdout_buf);
    }
    if !status.success() {
        return Err(format!(
            "handoff probe on {candidate:?} exited with {:?}",
            status.code()
        ));
    }
    parse_schema_range(String::from_utf8_lossy(&stdout_buf).trim())
}

/// Parse a `"<min> <max>"` schema-range line (the handoff probe's output
/// shape, `mux::cli::handoff_schema_range_line`).
#[cfg(unix)]
fn parse_schema_range(text: &str) -> Result<std::ops::RangeInclusive<u32>, String> {
    let mut parts = text.split_whitespace();
    let min: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("unparsable handoff probe output: {text:?}"))?;
    let max: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("unparsable handoff probe output: {text:?}"))?;
    if parts.next().is_some() {
        return Err(format!("unparsable handoff probe output: {text:?}"));
    }
    Ok(min..=max)
}

/// Counts reported by a completed snapshot or restore, used for the
/// handoff-start log line (FR11).
#[cfg(unix)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandoffCounts {
    pub pane_count: u32,
    pub descriptor_count: u32,
}

/// Derive [`HandoffCounts`] from a (produced or decoded) handoff document:
/// the total pane count across every session/window, and the descriptor
/// count (the listen descriptor, always present, plus every live pane's
/// master descriptor).
#[cfg(unix)]
fn handoff_counts_of(document: &mux_ipc::handoff::HandoffDocument) -> HandoffCounts {
    let mut pane_count: u32 = 0;
    let mut live_descriptors: u32 = 0;
    for session in &document.sessions {
        for window in &session.windows {
            for pane in &window.panes {
                pane_count += 1;
                if pane.master_fd.is_some() {
                    live_descriptors += 1;
                }
            }
        }
    }
    HandoffCounts {
        pane_count,
        descriptor_count: live_descriptors + 1, // +1: the listen descriptor
    }
}

/// Adapter matching [`prepare_upgrade`]'s substitutable `snapshot` parameter
/// shape to the real `crate::mux::upgrade::snapshot` contract (task0003).
#[cfg(unix)]
pub(super) fn real_snapshot(
    manager: &SessionManager,
    listen_fd: RawFd,
    socket_path: &Path,
) -> Result<mux_ipc::handoff::HandoffDocument, String> {
    crate::mux::upgrade::snapshot(manager, listen_fd, socket_path).map_err(|e| e.to_string())
}

/// Bound on how long [`prepare_upgrade`] waits for connected GUI clients to
/// acknowledge that their copy of the `Upgrading` announcement was actually
/// written to their socket (Design "Announcement delivery": queueing on
/// `notify_tx` alone is not delivery). Generous relative to an in-process
/// broadcast wakeup + one socket write, bounded so one slow/stuck client
/// never blocks an upgrade indefinitely.
#[cfg(unix)]
const UPGRADE_ANNOUNCE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// Bound on how long [`prepare_upgrade`] waits for the candidate binary's
/// `probe-handoff` subprocess (`probe_candidate_handoff_range`) to complete.
/// That probe is a real subprocess spawn/exec on the candidate binary,
/// which can hang indefinitely (e.g. stuck resolving a shared library) --
/// without this bound, a hung probe would stall this async fn's caller,
/// `run_daemon`'s `tokio::select!` loop, freezing the daemon's entire
/// accept/dispatch loop. A hang resolves to the existing upgrade-refusal
/// reply path, same as any other probe failure.
///
/// MUST stay strictly below the client's upgrade-response read timeout
/// (`connect_daemon`'s `set_read_timeout`, currently 5s): if a refusal
/// reply took longer to arrive than the client is willing to wait on that
/// read, the client's read times out first and falls into
/// `UpgradeResponse::ProceededOrUnknown`, then `wait_for_daemon_reachable_at_current_version`
/// trivially succeeds against the same still-running old daemon -- a
/// refused upgrade gets misreported as a successful in-place replacement
/// (the AC-10 same-daemon-reachability trap documented in cli.rs).
#[cfg(unix)]
pub(super) const UPGRADE_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Perform upgrade preparation (design steps 1-3): probe compatibility,
/// snapshot the live session tree to the handoff file next to `socket_path`
/// (owned end-to-end by `crate::mux::upgrade`), wait -- bounded -- for
/// connected GUI clients to acknowledge the `Upgrading` announcement was
/// written to their sockets, then return the run outcome (AC-5/AC-7:
/// announcement observably delivered before the outcome is returned).
///
/// Parameterized over the probe and snapshot operations (Test Notes: "a
/// substitutable probe") so every branch is testable without a real
/// candidate binary or real session-tree internals; production always
/// passes [`probe_candidate_handoff_range`] / [`real_snapshot`]. Never
/// removes the socket file and never calls [`graceful_shutdown`] or marks
/// any pane exited (AC-1, AC-2) -- both stay exclusively on the normal
/// shutdown path in [`run_daemon`], never invoked from here.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(super) async fn prepare_upgrade(
    session_manager: &Arc<Mutex<SessionManager>>,
    listen_fd: RawFd,
    candidate: &Path,
    args: Vec<String>,
    socket_path: &Path,
    current_schema_version: u32,
    upgrade_ack_slot: &SharedUpgradeAckSlot,
    probe: impl Fn(&Path) -> Result<std::ops::RangeInclusive<u32>, String> + Send + 'static,
    snapshot: impl FnOnce(
        &SessionManager,
        RawFd,
        &Path,
    ) -> Result<mux_ipc::handoff::HandoffDocument, String>,
) -> Result<UpgradeRequest, String> {
    // The real `probe` (`probe_candidate_handoff_range`) runs a synchronous,
    // untimed subprocess spawn/wait on the candidate binary. This async fn
    // is awaited directly inside `run_daemon`'s `tokio::select!` loop, so
    // calling it in-line here would block that loop's executor thread on
    // whatever the candidate binary does at startup. Run it on a blocking
    // thread and bound the wait, so a hung candidate degrades to the
    // existing upgrade-refusal path instead of stalling the daemon.
    let candidate_owned = candidate.to_path_buf();
    let range = match tokio::time::timeout(
        UPGRADE_PROBE_TIMEOUT,
        tokio::task::spawn_blocking(move || probe(&candidate_owned)),
    )
    .await
    {
        Ok(Ok(Ok(range))) => range,
        Ok(Ok(Err(e))) => return Err(e),
        Ok(Err(join_err)) => {
            return Err(format!("handoff probe task failed to run: {join_err}"));
        }
        Err(_) => {
            return Err(format!(
                "handoff probe on {candidate:?} timed out after {:?}",
                UPGRADE_PROBE_TIMEOUT
            ));
        }
    };
    if !range.contains(&current_schema_version) {
        return Err(format!(
            "candidate binary {candidate:?} supports handoff schema {}-{}, this daemon needs {}",
            range.start(),
            range.end(),
            current_schema_version
        ));
    }

    let mut document = {
        let mgr = session_manager.lock().await;
        snapshot(&mgr, listen_fd, socket_path)?
    };
    let counts = handoff_counts_of(&document);
    log::warn!(
        "mux upgrade: snapshot prepared ({} pane(s), {} descriptor(s))",
        counts.pane_count,
        counts.descriptor_count
    );

    // AC-7: establish the ack synchronization point BEFORE broadcasting, so
    // no connection can process the message before this function starts
    // waiting for its acknowledgement.
    let notify_tx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().clone()
    };
    // The CLI connection that issued THIS request is itself subscribed to
    // `notify_tx` (subscription happens unconditionally before the CLI/GUI
    // branch in `ipc::connection::handle_connection`) but is blocked
    // awaiting this very function's reply, so it never drains/acks its own
    // subscription -- exactly one guaranteed non-acking subscriber always
    // exists. Expect an ack from everyone else.
    let expected_acks = notify_tx.receiver_count().saturating_sub(1);
    let (ack_tx, mut ack_rx) = mpsc::channel::<()>(expected_acks.max(1));
    *upgrade_ack_slot.lock().unwrap() = Some(ack_tx);

    let msg = MuxMessage {
        msg_type: MessageType::Upgrading,
        pane_id: 0,
        payload: Vec::new(),
    };
    if let Err(e) = notify_tx.send(msg) {
        log::debug!(
            "prepare_upgrade: no active subscribers for Upgrading broadcast: {}",
            e
        );
    }

    let mut acked = 0usize;
    let deadline = tokio::time::Instant::now() + UPGRADE_ANNOUNCE_ACK_TIMEOUT;
    while acked < expected_acks {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log::warn!(
                "mux upgrade: timed out waiting for {} of {} Upgrading acknowledgement(s); \
                 proceeding anyway",
                expected_acks - acked,
                expected_acks
            );
            break;
        }
        match tokio::time::timeout(remaining, ack_rx.recv()).await {
            Ok(Some(())) => acked += 1,
            Ok(None) => break,
            Err(_) => {
                log::warn!(
                    "mux upgrade: timed out waiting for {} of {} Upgrading acknowledgement(s); \
                     proceeding anyway",
                    expected_acks - acked,
                    expected_acks
                );
                break;
            }
        }
    }
    *upgrade_ack_slot.lock().unwrap() = None;

    // task0006 (review rework, finding 2e6f18b4dc0a7593): `document` above
    // was captured before the client-acknowledgement wait, which is the
    // dominant (multi-second) part of the window between `snapshot` and
    // this process's eventual `exec` -- pane reader threads and the
    // daemon's agent-status task (this function's own caller's sibling
    // task, still running on this runtime) keep applying live agent-status
    // reports and OSC 133 marks in that window. Re-read each still-live
    // pane's CURRENT state now, as late as possible before returning, and
    // patch it into the ALREADY-WRITTEN handoff file -- see
    // `crate::mux::upgrade::refresh_live_agent_state`'s doc comment for
    // exactly what this narrows and what residual window remains.
    {
        let mgr = session_manager.lock().await;
        crate::mux::upgrade::refresh_live_agent_state(&mut document, &mgr);
    }
    if let Err(e) = crate::mux::upgrade::rewrite_handoff_file(&document, socket_path) {
        log::warn!(
            "mux upgrade: failed to refresh agent-status/latch state in the handoff file \
             before exec: {e}"
        );
    }

    let handoff_document_path = crate::mux::upgrade::handoff_file_path(socket_path);
    Ok(UpgradeRequest {
        target: candidate.to_path_buf(),
        args,
        env_addition: (
            HANDOFF_ENV_VAR.to_string(),
            handoff_document_path.to_string_lossy().to_string(),
        ),
        handoff_document_path,
    })
}

/// Handoff-mode startup (AC-7, AC-8, AC-9): read, decode and remove the
/// handoff file (`crate::mux::upgrade::read_and_remove_handoff_file`, single
/// owner of that file's whole lifetime), validate and adopt its recorded
/// listen descriptor (`crate::mux::upgrade::adopt_listener`, AC-6: refuses
/// and takes no ownership if the descriptor is not a live listening
/// `AF_UNIX` socket), and restore the session tree
/// (`crate::mux::upgrade::restore`, AC-5: incarnation/counters/tree restored
/// verbatim; individual pane adoption failures degrade that pane to exited
/// rather than failing the whole restore). Only a failed read/decode or a
/// failed listener adoption fails this function outright (the caller falls
/// back to a fresh bind in that case).
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(super) fn start_from_handoff(
    handoff_path: &Path,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> Result<(UnixListener, SessionManager, HandoffCounts), String> {
    let doc = crate::mux::upgrade::read_and_remove_handoff_file(handoff_path)
        .map_err(|e| e.to_string())?;

    let std_listener = crate::mux::upgrade::adopt_listener(doc.listen_fd as RawFd)
        .map_err(|e| format!("failed to adopt inherited listener: {e}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| format!("failed to prepare adopted listener as non-blocking: {e}"))?;
    let listener = UnixListener::from_std(std_listener)
        .map_err(|e| format!("failed to adopt inherited listener into the async runtime: {e}"))?;

    let counts = handoff_counts_of(&doc);
    let manager = crate::mux::upgrade::restore(
        &doc,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
    );

    Ok((listener, manager, counts))
}

/// Clean up any stale socket and bind a fresh listener at `sock_path`,
/// restricting its permissions to owner-only. Factored out of [`startup`]
/// so both the normal-startup and handoff-start-failure-fallback paths
/// share one implementation.
#[cfg(unix)]
fn bind_fresh_listener(sock_path: &Path) -> anyhow::Result<UnixListener> {
    cleanup_stale_socket(sock_path)?;
    let listener = UnixListener::bind(sock_path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o700))?;
    Ok(listener)
}

/// Decide and perform daemon startup: handoff-mode (the handoff environment
/// variable is present, AC-7) or normal bind (absent, AC-8). Extracted out
/// of [`run_daemon`] so it is unit-testable without spinning up the full
/// accept loop. The env var is cleared unconditionally before returning,
/// regardless of which branch ran, so a pane child spawned afterwards never
/// inherits it (AC-9). `title_tx` / `notification_tx` / `agent_status_tx` /
/// `pane_exit_sender` are the daemon's own lifetime channels a restored
/// pane's reader thread is re-wired to (`crate::mux::upgrade::restore`) --
/// [`run_daemon`] therefore creates them BEFORE calling this function.
#[cfg(unix)]
pub(super) fn startup(
    sock_path: &Path,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> anyhow::Result<(UnixListener, SessionManager, Option<HandoffCounts>)> {
    match std::env::var(HANDOFF_ENV_VAR) {
        Ok(path_str) if !path_str.is_empty() => {
            let handoff_path = PathBuf::from(&path_str);
            let result = start_from_handoff(
                &handoff_path,
                title_tx,
                notification_tx,
                agent_status_tx,
                pane_exit_sender,
            );
            // SAFETY: env mutation is process-wide; cleared unconditionally
            // here, before the caller can spawn any pane child (AC-9).
            unsafe {
                std::env::remove_var(HANDOFF_ENV_VAR);
            }
            match result {
                Ok((listener, manager, counts)) => {
                    log::warn!(
                        "mux daemon HANDOFF START: {} pane(s), {} descriptor(s) adopted",
                        counts.pane_count,
                        counts.descriptor_count
                    );
                    Ok((listener, manager, Some(counts)))
                }
                Err(e) => {
                    log::error!(
                        "mux daemon handoff start failed: {e} - falling back to a fresh bind"
                    );
                    Ok((bind_fresh_listener(sock_path)?, SessionManager::new(), None))
                }
            }
        }
        _ => {
            let listener = bind_fresh_listener(sock_path)?;
            log::info!("Mux daemon listening on {:?}", sock_path);
            Ok((listener, SessionManager::new(), None))
        }
    }
}

/// Re-enter service in this same process after the upgrade replacement
/// (`execve`) itself failed (IMPLEMENTATION.md D1, SPEC.md A14). Sets the
/// handoff environment variable to the document the failed attempt just
/// wrote, then runs the daemon lifecycle again from a fresh async runtime
/// (the previous one was already fully shut down before the replacement was
/// attempted, per D1). Reuses [`run_daemon`]'s own handoff-mode startup path
/// unchanged -- this is the "callable for a document the current process
/// itself produced, not only for one produced by a predecessor" entry point
/// task0004's design calls for.
///
/// Called from `mux::cli::perform_upgrade_replacement` (task0005) on a
/// failed `exec`; not exercised in this task's own tests (no process
/// replacement is involved here), but the handoff-mode startup path it
/// reuses is covered by `startup_with_handoff_env_var_adopts_listener_and_clears_env_var`.
#[cfg(unix)]
pub fn run_daemon_in_handoff_mode(
    handoff_document_path: &Path,
) -> anyhow::Result<DaemonRunOutcome> {
    // SAFETY: env mutation is process-wide; this process fully owns its
    // environment again (the exec that would have replaced it just
    // failed), and `run_daemon`'s own `startup()` clears this var before
    // returning, so there is no window where a spawned pane child could
    // inherit it.
    unsafe {
        std::env::set_var(HANDOFF_ENV_VAR, handoff_document_path);
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_daemon())
}
