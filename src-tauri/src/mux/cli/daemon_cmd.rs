//! The `--daemon` subcommand: daemon runtime entry and the hot-upgrade
//! replacement decision it acts on.

use super::init_mux_logger;
use crate::mux::daemon;

/// Bound on [`execute_daemon`]'s runtime shutdown (task0009 rework, Design
/// "Announcement delivery"): `prepare_upgrade` already waits for connected
/// GUI clients to acknowledge the `Upgrading` write before `run_daemon`
/// returns, so this is a defense-in-depth bound for any OTHER still-running
/// task (an unrelated connection mid-write, a background broadcast task) —
/// generous enough for a socket write to complete, short enough to keep the
/// replacement prompt.
const DAEMON_RUNTIME_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// Execute the `emterm mux --daemon` command (runs the daemon).
///
/// Inspects the daemon's run outcome (task0005, IMPLEMENTATION.md D1 /
/// "Performing the replacement"): normal termination keeps today's
/// behaviour; an upgrade request performs the process replacement, but only
/// after the async runtime has been fully shut down — replacing the process
/// image while its worker threads are alive is undefined behaviour.
pub fn execute_daemon() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger for daemon process (Tauri's logger is not available here).
    // Daemon stderr is redirected to mux-daemon.log by the spawning process.
    init_mux_logger("[DAEMON]");

    let rt = tokio::runtime::Runtime::new()?;
    let outcome = rt.block_on(daemon::run_daemon())?;

    // D1: blocks this thread until every worker thread has exited (or the
    // grace period elapses), which must happen before any process
    // replacement below. `shutdown_timeout` (rather than a bare `drop`,
    // which tears the runtime down with a zero-duration bound) gives any
    // still-running connection task a bounded chance to finish flushing a
    // write already in flight, instead of forcibly cutting it off the
    // instant this function proceeds.
    rt.shutdown_timeout(DAEMON_RUNTIME_SHUTDOWN_GRACE);

    match outcome {
        daemon::DaemonRunOutcome::Terminated => Ok(()),
        daemon::DaemonRunOutcome::UpgradeRequested(req) => {
            #[cfg(unix)]
            {
                perform_upgrade_replacement(req);
            }
            #[cfg(not(unix))]
            {
                // Unreachable in practice: `run_daemon` never constructs
                // this variant on a non-Unix build (upgrade is a Unix-only
                // feature, IMPLEMENTATION.md Conventions).
                let _ = req;
            }
            Ok(())
        }
    }
}

/// Replace this process's image with the upgrade target (IMPLEMENTATION.md
/// D1 / "Performing the replacement"). Called only from [`execute_daemon`]
/// after the async runtime has been fully shut down. `exec` only returns on
/// failure — the process image is otherwise gone and this function does not
/// return in the success case.
///
/// On failure, logs at error level and re-enters service in this same
/// process rather than exiting silently (IMPLEMENTATION.md "Error policy",
/// D1; SPEC.md A14), via [`daemon::run_daemon_in_handoff_mode`] (task0004's
/// entry point, callable for a document this process just wrote itself —
/// task0004's design, "Replacement failure recovery"). If THAT re-entered
/// run also requests an upgrade (e.g. the operator retries immediately),
/// the loop below tries the exec again with the new request rather than
/// stopping at one attempt.
///
/// DEVIATION (task0004): this function's body — the retry loop calling
/// `run_daemon_in_handoff_mode` — was added by task0004 on top of
/// task0005's already-merged `exec` attempt (this function's doc comment,
/// written by task0005, explicitly flagged the gap: "Full in-process
/// re-entry ... is not yet wired here"). `cli.rs` is outside task0004's
/// file scope; this is the minimal edit needed to close that flagged gap
/// now that both tasks are present.
///
/// task0004 (NFR3, "Candidate validation" call site 2): re-validates the
/// candidate immediately before EVERY exec attempt (the recorded-path
/// resolution the accept loop validated is necessarily stale relative to
/// exec time — the on-disk candidate could have been altered in the
/// intervening window), via [`decide_replacement`]. A refusal here skips
/// the exec attempt entirely and takes the SAME re-entry path an exec
/// failure takes today, so the daemon keeps serving.
#[cfg(unix)]
fn perform_upgrade_replacement(mut req: daemon::UpgradeRequest) {
    use std::os::unix::process::CommandExt;

    loop {
        match decide_replacement(crate::mux::identity::validate_candidate_path(
            &req.target,
            crate::mux::identity::effective_uid(),
        )) {
            ReplacementDecision::Attempt => {
                let err = std::process::Command::new(&req.target)
                    .args(&req.args)
                    .env(&req.env_addition.0, &req.env_addition.1)
                    .exec();

                log::error!(
                    "Failed to exec upgrade target {:?}: {err} (handoff document at {:?} was \
                     not consumed); re-entering service in this process",
                    req.target,
                    req.handoff_document_path
                );
            }
            ReplacementDecision::Reenter { reason } => {
                log::error!(
                    "mux upgrade replacement refused for target {:?}: {reason} (handoff \
                     document at {:?} was not consumed); re-entering service without \
                     attempting exec",
                    req.target,
                    req.handoff_document_path
                );
            }
        }

        match daemon::run_daemon_in_handoff_mode(&req.handoff_document_path) {
            Ok(daemon::DaemonRunOutcome::Terminated) => return,
            Ok(daemon::DaemonRunOutcome::UpgradeRequested(next_req)) => {
                req = next_req;
                // loop: attempt the exec again against the new request
            }
            Err(e) => {
                log::error!("mux daemon re-entry after failed exec also failed: {e} (giving up)");
                return;
            }
        }
    }
}

/// Decision for [`perform_upgrade_replacement`]'s per-attempt validation
/// gate (AC-3): parameterized on the validation OUTCOME (not performing the
/// validation itself), so it is table-tested across accepted/refused rows
/// without a real exec.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
pub(in crate::mux::cli) enum ReplacementDecision {
    /// Validation passed: attempt the exec.
    Attempt,
    /// Validation failed: skip the exec attempt and re-enter service
    /// through the existing handoff-mode path. Carries the refusal reason
    /// for the caller's error log.
    Reenter { reason: String },
}

#[cfg(unix)]
pub(in crate::mux::cli) fn decide_replacement(
    validation: Result<(), String>,
) -> ReplacementDecision {
    match validation {
        Ok(()) => ReplacementDecision::Attempt,
        Err(reason) => ReplacementDecision::Reenter { reason },
    }
}
