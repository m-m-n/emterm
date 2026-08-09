//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, IntoRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

// mux-daemon-binary-update-detect task0002 (D5): the binary-update identity
// check, consumed only by the Unix-only recovery-probe trigger below.
#[cfg(unix)]
use crate::mux::identity;

use super::ipc::connection::handle_connection;
use super::ipc::handlers::{handle_destroy_pane, reevaluate_agent_waiters};
use super::ipc::protocol::{
    AgentStatusUpdateMsg, ClientType, ErrorMsg, HelloMsg, MAX_FRAME_LENGTH, MessageType,
    MuxMessage, NotifyMsg, PREVIOUS_PROTOCOL_VERSION, PROTOCOL_VERSION, RenameWindowMsg,
    WelcomeMsg, parse_rejected_server_version,
};
use super::session::manager::SessionManager;
use super::session::pane::{
    AgentStatusFeedItem, AgentStatusReportSender, MuxPane, NotificationSender, PaneExitSender,
    PaneId, SharedPaneExitSender, TitleChangeSender,
};
use crate::prompts::PromptMarkKind;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

mod connect;
mod control_client;
#[cfg(unix)]
mod handoff;
mod tasks;

pub use connect::*;
pub use control_client::*;
#[cfg(unix)]
pub use handoff::*;
pub(in crate::mux) use tasks::*;

/// Daemon-level title channel capacity.
const TITLE_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level notification channel capacity (OSC 9 desktop notifications
/// detected on Detached panes).
const NOTIFICATION_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level agent-status report channel capacity (SPEC FR3): raw OSC
/// payload strings forwarded from every pane's reader thread, regardless of
/// attach state.
const AGENT_STATUS_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level pane-exit channel capacity. Reader threads enqueue a bare
/// `PaneId` here on PTY EOF; the reap task drains it. EOF is one-shot per
/// pane so the channel never sustains high throughput.
const PANE_EXIT_CHANNEL_CAPACITY: usize = 64;

/// Upgrade-signal channel capacity: bounded to 1. Only one upgrade
/// preparation is ever in flight -- the accept loop leaves its `select!` to
/// run it synchronously before resuming -- so a second concurrent request
/// simply waits for room rather than needing an unbounded queue.
const UPGRADE_SIGNAL_CHANNEL_CAPACITY: usize = 1;

#[cfg(unix)]
use tokio::net::UnixListener;

/// Environment variable naming the handoff state file's absolute path
/// (IMPLEMENTATION.md Shared Components, "Handoff environment contract" --
/// task0004-owned). Its presence at daemon start selects handoff startup:
/// the normal socket bind is skipped and the recorded listener is adopted
/// instead (AC-7). Its absence selects normal startup (AC-8). Cleared with
/// `std::env::remove_var` before any pane child is spawned, so a restored
/// pane's shell never inherits it (AC-9).
pub const HANDOFF_ENV_VAR: &str = "EMTERM_MUX_HANDOFF_FILE";

/// A client's request (via `MessageType::Upgrade`, handled in
/// `ipc::connection::handle_cli_client`) to perform an in-place upgrade.
/// Sent to the accept loop, which alone leaves its `select!` to perform the
/// (fallible) preparation steps -- signalled through this dedicated channel
/// rather than the existing `shutdown_tx` watch channel, so the two paths
/// can never be confused (task0004's design, "Request handling"). Named
/// `UpgradeSignal` (not `UpgradeRequest`) to avoid colliding with
/// [`UpgradeRequest`] (task0005's run-outcome payload type, already merged).
///
/// `reply` carries the outcome of *preparation* back to the SPECIFIC
/// requesting connection: `Ok(())` once the upgrade announcement has been
/// broadcast and the daemon is about to return its "upgrade requested" run
/// outcome (the connection is then simply dropped once the process is
/// replaced -- IMPLEMENTATION.md D2 -- so no further reply follows), or
/// `Err(reason)` when preparation aborted and the daemon continues serving
/// unchanged (AC-3, AC-4).
pub struct UpgradeSignal {
    pub reply: oneshot::Sender<Result<(), String>>,
}

/// Per-connection sender half of the upgrade-signal channel, cloned into
/// every spawned connection task exactly like the other daemon-level
/// senders (`title_tx`, `notification_tx`, ...).
pub type UpgradeSignalSender = mpsc::Sender<UpgradeSignal>;

/// Sender half of the per-upgrade "Upgrading write observed" acknowledgement
/// (Design "Announcement delivery" — queueing the broadcast on `notify_tx`
/// is not delivery). A connection task sends on this exactly once, right
/// after it has successfully written an `Upgrading` frame to its own socket.
pub type UpgradeAckSender = mpsc::Sender<()>;

/// Shared slot `prepare_upgrade` installs a fresh [`UpgradeAckSender`] into
/// immediately before broadcasting `Upgrading`, and clears once it is done
/// waiting for acknowledgements (bounded, AC-7). `None` outside an upgrade
/// attempt, so every other broadcast message (`SwitchWindow`,
/// `RenameWindow`, ...) never touches it — `ipc::connection`'s GUI message
/// loop only checks it right after writing an `Upgrading` frame.
pub type SharedUpgradeAckSlot = Arc<StdMutex<Option<UpgradeAckSender>>>;

/// Everything the synchronous caller (`mux::cli::execute_daemon`) needs to
/// replace this process's image in place (IMPLEMENTATION.md D1, Shared
/// Components "Daemon run outcome").
///
/// task0004 owns wiring the accept-loop branch that actually constructs
/// [`DaemonRunOutcome::UpgradeRequested`] (after receiving `Upgrade` and
/// snapshotting the session tree, task0003); task0005 owns consuming it in
/// `execute_daemon` and is defined here ahead of that merge so no task
/// leaves a placeholder for the other (D9). `run_daemon` returns
/// `Terminated` unconditionally today.
#[derive(Debug, Clone)]
pub struct UpgradeRequest {
    /// Absolute path of the target binary to replace this process with.
    pub target: PathBuf,
    /// Argument vector for the replacement (mirrors this process's own,
    /// per IMPLEMENTATION.md D1).
    pub args: Vec<String>,
    /// Single environment variable addition carrying the handoff document
    /// path (Shared Components "Handoff environment contract", task0004).
    pub env_addition: (String, String),
    /// Absolute path of the handoff document just written, so the caller
    /// can re-enter service over it if the replacement itself fails.
    pub handoff_document_path: PathBuf,
}

/// Outcome of the async daemon entry point ([`run_daemon`]), consumed by the
/// synchronous caller (`mux::cli::execute_daemon`) only after the async
/// runtime has been fully shut down (IMPLEMENTATION.md D1: replacing the
/// process image while runtime worker threads are alive is undefined
/// behaviour).
#[derive(Debug)]
pub enum DaemonRunOutcome {
    /// The daemon exited normally — today's behaviour, unchanged.
    Terminated,
    /// An `Upgrade` request was accepted and a handoff document has been
    /// written; the caller must perform the process replacement described
    /// by the carried [`UpgradeRequest`].
    UpgradeRequested(UpgradeRequest),
}

/// Run the mux daemon.
///
/// This is the main entry point for `emterm mux --daemon`.
/// It blocks until all sessions end, SIGTERM is received, or an upgrade is
/// requested and prepared (in which case the caller must perform the
/// replacement -- IMPLEMENTATION.md D1).
#[cfg(unix)]
pub async fn run_daemon() -> anyhow::Result<DaemonRunOutcome> {
    let sock_path = socket_path();

    // Ensure parent directory exists with restricted permissions
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    // Daemon-level channels are created BEFORE `startup()` (task0009 rework:
    // a handoff-mode start's `restore` re-wires each restored live pane's
    // reader thread through these SAME senders, exactly like a freshly
    // spawned pane's -- so they must already exist when `startup()` runs).
    //
    // Daemon-level title channel: lives as long as the daemon so every pane
    // (GUI-created or CLI-created) can propagate OSC title changes to the
    // session manager even when no GUI client is attached.
    let (title_tx, title_rx): (TitleChangeSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(TITLE_CHANNEL_CAPACITY);

    // Daemon-level notification channel: pane reader threads forward OSC 9
    // desktop notifications detected on Detached output here; the task
    // broadcasts them to connected GUI clients via notify_tx (FR2, NFR3).
    let (notification_tx, notification_rx): (NotificationSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(NOTIFICATION_CHANNEL_CAPACITY);

    // Daemon-level agent-status channel (SPEC FR3): pane reader threads
    // forward raw agent-status OSC payload strings here REGARDLESS of
    // attach state (unlike notifications, which only scan while Detached) —
    // the daemon owns per-pane agent-status state unconditionally.
    let (agent_status_tx, agent_status_rx): (
        AgentStatusReportSender,
        mpsc::Receiver<(u32, AgentStatusFeedItem)>,
    ) = mpsc::channel(AGENT_STATUS_CHANNEL_CAPACITY);

    // Shutdown signal: sent by handle_destroy_pane/handle_destroy_window when all sessions empty
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Daemon-level pane-exit channel: pane reader threads enqueue their pane_id
    // here on PTY EOF (regardless of attach state); the reap task reaps each via
    // handle_destroy_pane, making "PTY death -> reap" the single authority (FR1,
    // FR2, FR7). The SharedPaneExitSender is fixed at pane creation and never
    // swapped on detach, so a detached pane can still notify on EOF (M1).
    let (pane_exit_tx, pane_exit_rx): (PaneExitSender, mpsc::Receiver<PaneId>) =
        mpsc::channel(PANE_EXIT_CHANNEL_CAPACITY);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(Some(pane_exit_tx)));

    let (listener, restored_manager, _handoff_counts) = startup(
        &sock_path,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    )?;

    // task0001 (mux-daemon-binary-update-detect, Design D4): record-or-
    // invalidate the daemon's own start-binary identity on EVERY pass
    // through this function -- fresh bind, post-execve handoff start, and
    // failed-exec re-entry (`run_daemon_in_handoff_mode`) all funnel through
    // `run_daemon` itself, so placing this call here, right after `startup()`
    // has returned successfully, covers all three routes uniformly while
    // ensuring the identity file is only published once this process has
    // confirmed it owns the listening socket -- a competing process that
    // loses the bind race never gets this far and so never overwrites the
    // active daemon's identity sidecar (cluster1-identity-write-timing).
    // Kept in-process (not re-read from disk) so the upgrade-signal branch
    // below resolves its exec candidate from exactly the value that was
    // persisted, never from a fresh resolution (D3).
    let recorded_identity = crate::mux::identity::record_or_invalidate(
        std::env::current_exe().ok().as_deref(),
        &sock_path,
    );

    // task0004 (NFR3): the daemon's own effective uid, computed once and
    // reused by every candidate-validation call in the upgrade-signal branch
    // below (`admit_upgrade_candidate`'s injected `validate` closure).
    let daemon_uid = crate::mux::identity::effective_uid();

    let session_manager = Arc::new(Mutex::new(restored_manager));

    tokio::spawn(run_title_update_task(session_manager.clone(), title_rx));
    tokio::spawn(run_notification_task(
        session_manager.clone(),
        notification_rx,
    ));
    tokio::spawn(run_agent_status_task(
        session_manager.clone(),
        agent_status_rx,
    ));
    tokio::spawn(run_pane_exit_task(
        session_manager.clone(),
        shutdown_tx.clone(),
        pane_exit_rx,
    ));

    // Upgrade-signal channel: `ipc::connection::handle_cli_client` signals
    // here on `MessageType::Upgrade` (task0004's design, "Request
    // handling") rather than `shutdown_tx`, so the two paths cannot be
    // confused. The original sender stays owned by this function (never
    // moved away, only cloned per connection), so `upgrade_rx.recv()` never
    // observes `None` while the loop below runs.
    let (upgrade_tx, mut upgrade_rx): (UpgradeSignalSender, mpsc::Receiver<UpgradeSignal>) =
        mpsc::channel(UPGRADE_SIGNAL_CHANNEL_CAPACITY);

    // AC-7: slot `prepare_upgrade` installs a fresh ack sender into right
    // before broadcasting `Upgrading`; `None` the rest of the time.
    let upgrade_ack_slot: SharedUpgradeAckSlot = Arc::new(StdMutex::new(None));

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    #[cfg(unix)]
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    // Set once the upgrade branch below completes preparation successfully;
    // breaks the loop and skips graceful_shutdown / socket removal entirely
    // (AC-1, AC-2, IMPLEMENTATION.md D4: the listen socket stays open and on
    // disk so mid-upgrade connections queue in the kernel backlog).
    let mut pending_upgrade: Option<UpgradeRequest> = None;

    // task0004 (NFR1, Design "Repeat-refusal suppression"): the most
    // recently refused candidate's (device, inode) plus the reason it
    // produced. In-memory only, run-loop-scoped alongside `pending_upgrade`
    // -- a daemon restart naturally clears it.
    let mut last_refused_candidate: Option<RefusedCandidate> = None;

    loop {
        #[cfg(unix)]
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        tokio::spawn(handle_connection(stream, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), agent_status_tx.clone(), pane_exit_sender.clone(), upgrade_tx.clone(), upgrade_ack_slot.clone()));
                    }
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                    }
                }
            }
            Some(signal) = upgrade_rx.recv() => {
                let UpgradeSignal { reply } = signal;
                // task0001 (Design D3): the exec candidate comes exclusively
                // from the identity recorded at this process's own startup
                // -- never from a fresh executable-path resolution, which
                // resolves to a "(deleted)" path after a rename-replacement
                // and would re-launch the SAME old image. No recorded
                // identity means the capture at startup failed; refuse
                // rather than fall back to fresh resolution (NFR3).
                let candidate = match crate::mux::identity::resolve_upgrade_candidate(
                    recorded_identity.as_ref(),
                ) {
                    Ok(p) => p,
                    Err(msg) => {
                        log::warn!("mux upgrade aborted: {}", msg);
                        let _ = reply.send(Err(msg));
                        continue;
                    }
                };

                // task0004 (NFR1/NFR3): fast-reject a repeat refusal of the
                // SAME (device, inode) without spawning a probe, and refuse
                // a candidate whose current on-disk state is not
                // owner-controlled BEFORE the handoff schema probe ever
                // runs (Design "Candidate validation" / "Repeat-refusal
                // suppression").
                let candidate_id = match admit_upgrade_candidate(
                    &candidate,
                    &mut last_refused_candidate,
                    crate::mux::identity::capture_dev_ino,
                    |c| crate::mux::identity::validate_candidate_path(c, daemon_uid),
                ) {
                    UpgradeAdmission::Blocked(reason) => {
                        if reason.starts_with(UPGRADE_SUPPRESSED_MARKER) {
                            log::warn!(
                                "mux upgrade: suppressing repeat refusal for candidate {:?}: {}",
                                candidate,
                                reason
                            );
                        } else {
                            log::warn!("mux upgrade refused: {}", reason);
                        }
                        let _ = reply.send(Err(reason));
                        continue;
                    }
                    UpgradeAdmission::Admitted { candidate_id } => candidate_id,
                };

                // The probe's own deadline (bounding the subprocess it
                // spawns, so a hung candidate is actually killed) mirrors
                // `prepare_upgrade`'s outer `UPGRADE_PROBE_TIMEOUT` join
                // timeout; computed here, immediately before the call, so
                // the two stay effectively in sync.
                let probe_deadline = std::time::Instant::now() + UPGRADE_PROBE_TIMEOUT;
                match prepare_upgrade(
                    &session_manager,
                    listener.as_raw_fd(),
                    &candidate,
                    vec!["mux".to_string(), "--daemon".to_string()],
                    &sock_path,
                    mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
                    &upgrade_ack_slot,
                    move |c: &Path| probe_candidate_handoff_range(c, probe_deadline),
                    real_snapshot,
                )
                .await
                {
                    Ok(outcome) => {
                        let _ = reply.send(Ok(()));
                        pending_upgrade = Some(outcome);
                    }
                    Err(reason) => {
                        log::warn!("mux upgrade aborted: {}", reason);
                        // task0004 (Design "Recording"): a probe spawn
                        // failure, probe timeout, or schema-range gate
                        // rejection is recorded exactly like a validation
                        // failure, so a repeat of the SAME candidate is
                        // suppressed on the next signal too.
                        record_post_probe_refusal(
                            &mut last_refused_candidate,
                            candidate_id,
                            &reason,
                        );
                        let _ = reply.send(Err(reason));
                    }
                }
                if pending_upgrade.is_some() {
                    break;
                }
            }
            _ = sigterm.recv() => {
                log::info!("SIGTERM received, shutting down");
                break;
            }
            _ = sigint.recv() => {
                log::info!("SIGINT received, shutting down");
                break;
            }
            _ = sighup.recv() => {
                log::info!("SIGHUP received, ignoring (daemon continues)");
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("All sessions empty, auto-shutting down");
                    break;
                }
            }
        }
    }

    if let Some(outcome) = pending_upgrade {
        log::warn!(
            "mux daemon exiting to perform upgrade: target={:?} handoff={:?}",
            outcome.target,
            outcome.handoff_document_path
        );

        // Design "Descriptor lifetime" (critical rework findings
        // dd7a4526fea67d1d / 5106b22111395091): every descriptor recorded
        // in the handoff document must still be open at the moment this
        // function returns. This whole async fn's locals are about to be
        // dropped when it returns below -- ordinary Rust `Drop` semantics
        // would otherwise close every one of them right here, which is
        // exactly the bug this task exists to fix. Deliberately transfer
        // ownership out of anything that would run that `Drop`:
        //
        // - the listener: extract its raw fd and leak it (a bare `RawFd` is
        //   a plain integer with no `Drop` impl) rather than let `listener`
        //   (a `tokio::net::UnixListener`) drop at the end of this function.
        if let Ok(std_listener) = listener.into_std() {
            let _ = std_listener.into_raw_fd();
        }
        // - every pane's master descriptor, and everything else the session
        //   tree owns: `SessionManager` (and `MuxPane`'s master field) have
        //   no public API to extract descriptors individually (session/pane
        //   internals are outside this task's file scope), so leak the
        //   whole tree instead by permanently inflating its `Arc` refcount
        //   by one. This guarantees the LAST clone's drop (one of the
        //   background tasks spawned above) can never bring the count to
        //   zero, so `SessionManager`'s own drop (which would otherwise
        //   close every pane's master via `portable_pty`'s Drop impl) never
        //   runs. Safe here specifically because this process is moments
        //   away from either being replaced (`execve`) -- at which point no
        //   Rust destructor runs for anything, leaked or not -- or, on a
        //   failed replacement, re-entering service over the SAME document
        //   (`run_daemon_in_handoff_mode`), which builds a brand new
        //   `SessionManager` from scratch and never looks at this one again.
        std::mem::forget(session_manager.clone());

        return Ok(DaemonRunOutcome::UpgradeRequested(outcome));
    }

    // Graceful shutdown: close all PTYs so shell processes terminate
    graceful_shutdown(&session_manager).await;

    // Cleanup socket file
    let _ = std::fs::remove_file(&sock_path);
    log::info!("Daemon shutdown complete");

    Ok(DaemonRunOutcome::Terminated)
}

/// Run the mux daemon on Windows using Named Pipes.
///
/// Listens on `\\.\pipe\emterm-mux-default`, accepts client connections,
/// and manages PTY sessions. Auto-exits when all sessions end or Ctrl+C.
#[cfg(windows)]
pub async fn run_daemon() -> anyhow::Result<DaemonRunOutcome> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name_str = pipe_name();

    // Write marker file so socket_path().exists() works for other checks
    let sock_path = socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&sock_path, pipe_name_str.as_bytes())?;

    log::info!("Mux daemon listening on {}", pipe_name_str);

    let session_manager = Arc::new(Mutex::new(SessionManager::new()));

    // Daemon-level title channel: lives as long as the daemon so every pane
    // (GUI-created or CLI-created) can propagate OSC title changes to the
    // session manager even when no GUI client is attached.
    let (title_tx, title_rx): (TitleChangeSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(TITLE_CHANNEL_CAPACITY);
    tokio::spawn(run_title_update_task(session_manager.clone(), title_rx));

    // Daemon-level notification channel: pane reader threads forward OSC 9
    // desktop notifications detected on Detached output here; the task
    // broadcasts them to connected GUI clients via notify_tx (FR2, NFR3).
    let (notification_tx, notification_rx): (NotificationSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(NOTIFICATION_CHANNEL_CAPACITY);
    tokio::spawn(run_notification_task(
        session_manager.clone(),
        notification_rx,
    ));

    // Daemon-level agent-status channel (SPEC FR3, same wiring as the Unix
    // run loop): pane reader threads forward raw agent-status OSC payload
    // strings here regardless of attach state.
    let (agent_status_tx, agent_status_rx): (
        AgentStatusReportSender,
        mpsc::Receiver<(u32, AgentStatusFeedItem)>,
    ) = mpsc::channel(AGENT_STATUS_CHANNEL_CAPACITY);
    tokio::spawn(run_agent_status_task(
        session_manager.clone(),
        agent_status_rx,
    ));

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Daemon-level pane-exit channel (same wiring as the Unix run loop, FR7):
    // reader threads enqueue their pane_id on PTY EOF; the reap task reaps each
    // via handle_destroy_pane regardless of attach state (FR1, FR2). The
    // SharedPaneExitSender is fixed at pane creation and never swapped (M1).
    let (pane_exit_tx, pane_exit_rx): (PaneExitSender, mpsc::Receiver<PaneId>) =
        mpsc::channel(PANE_EXIT_CHANNEL_CAPACITY);
    tokio::spawn(run_pane_exit_task(
        session_manager.clone(),
        shutdown_tx.clone(),
        pane_exit_rx,
    ));
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(Some(pane_exit_tx)));

    // Upgrade-signal channel (parity with the Unix run loop's type, NFR4):
    // in-place hot-upgrade is Unix-only, so every request here is simply
    // answered "unsupported" -- no accept-loop upgrade branch exists on
    // this platform.
    let (upgrade_tx, mut upgrade_rx): (UpgradeSignalSender, mpsc::Receiver<UpgradeSignal>) =
        mpsc::channel(UPGRADE_SIGNAL_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        while let Some(signal) = upgrade_rx.recv().await {
            let _ = signal.reply.send(Err(
                "mux hot-upgrade is not supported on this platform".to_string()
            ));
        }
    });

    // Parity with the Unix run loop's parameter list (NFR4): never actually
    // populated on this platform (no accept-loop upgrade branch exists
    // here), but `handle_connection` is shared across both platforms.
    let upgrade_ack_slot: SharedUpgradeAckSlot = Arc::new(StdMutex::new(None));

    // First iteration claims exclusive pipe ownership to prevent hijacking
    let mut is_first_instance = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(is_first_instance)
            .reject_remote_clients(true)
            .create(&pipe_name_str)?;
        is_first_instance = false;

        tokio::select! {
            result = server.connect() => {
                match result {
                    Ok(()) => {
                        tokio::spawn(handle_connection(server, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), agent_status_tx.clone(), pane_exit_sender.clone(), upgrade_tx.clone(), upgrade_ack_slot.clone()));
                    }
                    Err(e) => {
                        log::error!("Pipe accept error: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("Ctrl+C received, shutting down");
                break;
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("All sessions empty, auto-shutting down");
                    break;
                }
            }
        }
    }

    graceful_shutdown(&session_manager).await;

    // Cleanup marker file
    let _ = std::fs::remove_file(&sock_path);
    log::info!("Daemon shutdown complete");

    Ok(DaemonRunOutcome::Terminated)
}

/// Run the mux daemon (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub async fn run_daemon() -> anyhow::Result<DaemonRunOutcome> {
    anyhow::bail!("Mux daemon is not supported on this platform.");
}

#[cfg(test)]
mod tests;
