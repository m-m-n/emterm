//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` -- Start/attach to default session (long-running bridge)
//! - `emterm mux --daemon` -- Run as daemon process (internal)
//! - `emterm mux attach [session]` -- Attach to existing session (long-running bridge)
//! - `emterm mux ls` -- List sessions
//! - `emterm mux kill [session]` -- Kill a session
//! - `emterm mux new [name]` -- Create a new session
//!
//! `run` is the single entry point invoked from `main.rs`; it owns all
//! subcommand-argument parsing for the mux CLI so the binary entry point
//! stays a one-liner.

use serde::{Deserialize, Serialize};

use super::bridge::run_bridge;
use super::daemon;
use super::ipc::protocol::*;
// `tmux_import` writes to GUI-only `settings_store`, so it is reachable only
// in the GUI build. The mux-only deb (`emterm-mux`) is intended for
// headless SSH hosts where `settings.json` is hand-managed; auto-importing
// `~/.tmux.conf` into it is meaningless there.
#[cfg(feature = "gui")]
use super::tmux_import::import_tmux_conf_if_needed;

mod admin;
pub use admin::*;

mod agent_api;
pub use agent_api::*;

mod connect;
pub use connect::*;

mod pane_cmd;
pub use pane_cmd::*;

/// Usage summary printed for an unknown `mux` subcommand (AC-1, task0005:
/// `upgrade` / `probe-handoff` registered here alongside the dispatch
/// table).
const MUX_USAGE: &str = "Available: ls / kill [session] / attach [session] / new-window / script / \
     switch-window <index> / send-keys / read / send / wait / upgrade / \
     probe-handoff / clear-logs / (no subcommand = start session)";

/// Dispatch `emterm mux …` subcommands. `args` is the slice that follows
/// the literal `mux` token (so `args[0]` is the next positional, e.g.
/// `attach` / `--daemon` / `ls`). Returns the desired process exit code.
///
/// All argument-parsing logic for the mux CLI lives here, mirroring the
/// `emterm::cli::run` pattern used for `markdown` / `json` / `yaml` /
/// `image`. Adding a new mux subcommand means editing this file only;
/// `main.rs` just forwards.
pub fn run(args: &[String]) -> i32 {
    // `--daemon` is recognised both as a bare flag and as a "subcommand"
    // alongside the positional ones, matching the legacy clap surface.
    let mut daemon_mode = false;
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if a == "--daemon" {
            daemon_mode = true;
        } else {
            positional.push(a.as_str());
        }
    }

    if daemon_mode {
        return match execute_daemon() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Daemon error: {e}");
                1
            }
        };
    }

    let sub = positional.first().copied();
    let rest: Vec<&str> = positional.iter().copied().skip(1).collect();

    let result: Result<(), Box<dyn std::error::Error>> = match sub {
        Some("ls") => execute_ls(),
        Some("kill") => execute_kill(rest.first().copied()),
        Some("attach") => execute_attach(rest.first().copied()),
        Some("script") => execute_script(),
        Some("clear-logs") => execute_clear_logs(),
        Some("new-window") => {
            let mut name: Option<&str> = None;
            let mut command: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-n" | "--name" => name = iter.next(),
                    "-c" | "--command" => command = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `new-window`: {other}");
                        return 2;
                    }
                }
            }
            execute_new_window(name, command)
        }
        Some("switch-window") => {
            let Some(arg) = rest.first().copied() else {
                eprintln!("Error: `switch-window` requires a 0-based window index");
                return 2;
            };
            let idx: u32 = match arg.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("Error: window index must be a non-negative integer (got {arg:?})");
                    return 2;
                }
            };
            execute_switch_window(idx)
        }
        Some("send-keys") => {
            let mut target: Option<u32> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-t" | "--target" => {
                        let Some(v) = iter.next() else {
                            eprintln!("Error: `--target` requires a window index");
                            return 2;
                        };
                        match v.parse() {
                            Ok(n) => target = Some(n),
                            Err(_) => {
                                eprintln!(
                                    "Error: target must be a non-negative integer (got {v:?})"
                                );
                                return 2;
                            }
                        }
                    }
                    other => {
                        eprintln!("Error: unknown argument to `send-keys`: {other}");
                        return 2;
                    }
                }
            }
            execute_send_keys(target)
        }
        Some("read") => {
            let mut pane: Option<&str> = None;
            let mut lines: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--lines" => lines = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `read`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `read` requires --pane <id|current>");
                return 2;
            };
            return execute_mux_read(pane, lines);
        }
        Some("send") => {
            let mut pane: Option<&str> = None;
            let mut text: Option<&str> = None;
            let mut use_stdin = false;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--text" => text = iter.next(),
                    "--stdin" => use_stdin = true,
                    other => {
                        eprintln!("Error: unknown argument to `send`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `send` requires --pane <id|current>");
                return 2;
            };
            if text.is_some() == use_stdin {
                eprintln!("Error: `send` requires exactly one of --text <s> or --stdin");
                return 2;
            }
            return execute_mux_send(pane, text, use_stdin);
        }
        Some("wait") => {
            let mut pane: Option<&str> = None;
            let mut state: Option<&str> = None;
            let mut timeout: Option<&str> = None;
            let mut after: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--state" => state = iter.next(),
                    "--timeout" => timeout = iter.next(),
                    "--after" => after = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `wait`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `wait` requires --pane <id|current>");
                return 2;
            };
            let Some(state) = state else {
                eprintln!("Error: `wait` requires --state <set> (comma-separated)");
                return 2;
            };
            return execute_mux_wait(pane, state, timeout, after);
        }
        Some("upgrade") => {
            return execute_upgrade();
        }
        Some("probe-handoff") => {
            return execute_probe_handoff();
        }
        Some(other) => {
            eprintln!("Error: unknown `mux` subcommand: {other}");
            eprintln!("{MUX_USAGE}");
            return 2;
        }
        None => execute_mux(),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

/// Check for nesting (EMTERM_MUX=1).
fn check_nesting() -> Result<(), String> {
    if std::env::var("EMTERM_MUX").is_ok() {
        Err("Cannot nest mux sessions (EMTERM_MUX is set)".to_string())
    } else {
        Ok(())
    }
}

/// Initialize env_logger with a component label prefix (e.g. "[DAEMON]", "[BRIDGE]").
fn init_mux_logger(component: &'static str) {
    use std::io::Write;

    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(move |buf, record| {
            writeln!(
                buf,
                "{} {}{} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f%:z"),
                record.level(),
                component,
                record.args()
            )
        })
        .init();
}

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

// ============================================================================
// Agent-facing API: `emterm mux read|send|wait` (task0004)
//
// Exit codes per IMPLEMENTATION.md "Conventions": 0 success; 2 usage/invalid
// input; 3 wait timeout (dedicated); 4 unknown pane / pane gone; 5
// not_mux_pane; 1 all other errors (connection failure etc.).
// ============================================================================

#[cfg(test)]
mod tests;
