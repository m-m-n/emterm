//! Session-management subcommands: `ls` / `kill` / `upgrade` /
//! `probe-handoff`.

use super::*;

/// Execute the `emterm mux ls` command.
#[cfg(unix)]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    let (_stream, sessions) = cli_handshake()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    for session in &sessions {
        println!(
            "{}: {} ({} windows, {} panes)",
            session.id, session.name, session.window_count, session.pane_count
        );
    }

    Ok(())
}

/// Execute the `emterm mux ls` command (Windows).
#[cfg(windows)]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    let (_stream, sessions) = cli_handshake()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    for session in &sessions {
        println!(
            "{}: {} ({} windows, {} panes)",
            session.id, session.name, session.window_count, session.pane_count
        );
    }

    Ok(())
}

/// Execute the `emterm mux ls` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

/// Execute the `emterm mux kill` command.
///
/// Sends a Shutdown message to the daemon, regardless of the protocol
/// version it happens to be running (AC-2, task0010 rework): a presence
/// check alone can't tell a long-lived adjacent-version daemon from a
/// compatible one, and the old server rejects a v2 Hello before ever
/// reading Shutdown — `daemon::shutdown_daemon_any_version` retries with
/// the previous protocol version so that legacy daemon can still be asked
/// to exit. Falls back to stale socket/marker-file removal if the daemon is
/// unreachable outright.
#[cfg(any(unix, windows))]
pub fn execute_kill(session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    if session.is_some() {
        eprintln!(
            "Killing specific sessions is not yet supported. Use 'emterm mux kill' to kill the daemon."
        );
        return Ok(());
    }

    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    match daemon::shutdown_daemon_any_version(&sock_path) {
        Ok(daemon::ShutdownOutcome::ShutDown(msg))
        | Ok(daemon::ShutdownOutcome::StaleSocketRemoved(msg)) => {
            println!("{msg}");
            Ok(())
        }
        Err(msg) => Err(msg.into()),
    }
}

/// Execute the `emterm mux kill` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

// ============================================================================
// `emterm mux upgrade` (task0005): ask a running daemon to replace itself in
// place with the currently-installed binary.
// ============================================================================

/// Execute `emterm mux upgrade` against the daemon at `sock_path` (task0005
/// AC-1..AC-4). Numbered flow mirrors the task plan's Design section:
///
/// 1. Fail clearly (without creating a socket or spawning a daemon) when no
///    daemon is reachable (AC-4).
/// 2. Connect and handshake, tolerating a daemon one protocol version behind
///    — the same tolerance [`daemon::recover_from_legacy_daemon`] uses — so
///    a mismatched daemon can still be asked to upgrade.
/// 3. Send the `Upgrade` request.
/// 4. Poll (bounded) until a daemon speaking the current protocol version is
///    reachable again; report success or timeout (AC-2/AC-3).
///
/// Split out of [`execute_upgrade`] so tests can point it at an isolated
/// stand-in daemon's socket instead of the real per-user
/// `daemon::socket_path()` (mirrors [`resolve_attach_socket_with`]'s
/// existing test-injection shape).
#[cfg(unix)]
fn execute_upgrade_at(sock_path: &std::path::Path) -> i32 {
    if !daemon::is_daemon_running(sock_path) {
        eprintln!("No mux daemon running (nothing to upgrade)");
        return 1;
    }

    let mut stream = match daemon::connect_daemon(sock_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: could not connect to the mux daemon: {e}");
            return 1;
        }
    };

    match daemon::handshake_with_version(&mut stream, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => {}
        Ok(WelcomeMsg::Rejected { .. }) => {
            drop(stream);
            let mut retry = match daemon::connect_daemon(sock_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: could not connect to the mux daemon: {e}");
                    return 1;
                }
            };
            match daemon::handshake_with_version(&mut retry, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    stream = retry;
                }
                Ok(WelcomeMsg::Rejected { reason }) => {
                    eprintln!("Error: mux daemon rejected the handshake: {reason}");
                    return 1;
                }
                Err(e) => {
                    eprintln!("Error: failed to negotiate with the mux daemon: {e}");
                    return 1;
                }
            }
        }
        Err(e) => {
            eprintln!("Error: failed to communicate with the mux daemon: {e}");
            return 1;
        }
    }

    if let Err(e) = daemon::send_upgrade(&mut stream) {
        eprintln!("Error: failed to send the upgrade request: {e}");
        return 1;
    }

    // AC-10 (task0009 rework, finding 07f6dbc60e84d54f): read the daemon's
    // own reply BEFORE treating this as accepted. An `Error` frame (FR13)
    // means the daemon refused; report the reason and exit non-zero rather
    // than falling through to the reachability poll, which would otherwise
    // trivially "succeed" against the SAME still-running daemon that just
    // refused (nothing happened, but the command would have reported
    // success). Anything else -- a clean disconnect (the daemon dropped this
    // connection per IMPLEMENTATION.md D2) or a read timeout while
    // preparation is still in flight -- proceeds to the poll below, which is
    // the actual evidence of a completed replacement.
    match daemon::read_upgrade_response(&mut stream) {
        daemon::UpgradeResponse::Rejected(reason) => {
            eprintln!("Error: mux daemon refused the upgrade: {reason}");
            return 1;
        }
        daemon::UpgradeResponse::ProceededOrUnknown => {}
    }
    drop(stream);

    if daemon::wait_for_daemon_reachable_at_current_version(sock_path) {
        println!("Mux daemon upgraded in place");
        0
    } else {
        eprintln!(
            "Timed out waiting for the mux daemon to become reachable after the upgrade \
             request"
        );
        1
    }
}

/// Execute the `emterm mux upgrade` command.
#[cfg(unix)]
pub fn execute_upgrade() -> i32 {
    execute_upgrade_at(&daemon::socket_path())
}

/// Execute the `emterm mux upgrade` command (unsupported platform):
/// in-place upgrade is a Unix-only feature (execve-based process
/// replacement, IMPLEMENTATION.md Conventions) — report unsupported and
/// leave today's behaviour untouched (AC-8), rather than a partial
/// Windows-side implementation.
#[cfg(not(unix))]
pub fn execute_upgrade() -> i32 {
    eprintln!("In-place upgrade is not supported on this platform");
    1
}

// ============================================================================
// `emterm mux probe-handoff` (task0005): print the inclusive range of
// handoff schema versions this binary can restore. Answer side of
// IMPLEMENTATION.md D3 — the asking side (a running daemon invoking this as
// a subprocess against a candidate binary) lives in task0004.
// ============================================================================

/// The line [`execute_probe_handoff`] prints: two whitespace-separated
/// unsigned integers, `<min> <max>`, the inclusive range of handoff schema
/// versions this binary can restore. Factored out for testability (AC-5).
///
/// task0009 rework (AC-9, finding 32bb6e465ac0fbb4 / a50509ac760abb59 /
/// d6b2bb34403b44f9): derived from `mux_ipc::handoff::
/// SUPPORTED_HANDOFF_SCHEMA_VERSIONS`, the single source of truth
/// `crate::mux::upgrade::read_and_remove_handoff_file` actually decodes
/// against — this used to be a local literal that could silently drift from
/// it.
pub(in crate::mux::cli) fn handoff_schema_range_line() -> String {
    let range = mux_ipc::handoff::SUPPORTED_HANDOFF_SCHEMA_VERSIONS;
    format!("{} {}", range.start(), range.end())
}

/// Execute `emterm mux probe-handoff` (task0005 AC-5). Never touches the
/// daemon socket or any daemon state — a pure, static self-description used
/// by a running daemon (task0004) to decide whether a candidate binary is
/// safe to hand off to (IMPLEMENTATION.md D3).
pub fn execute_probe_handoff() -> i32 {
    println!("{}", handoff_schema_range_line());
    0
}

/// Format a byte count as a human-readable size.
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

/// Open an existing regular file for truncation, refusing symlinks.
///
/// Uses `symlink_metadata` to reject symlinked targets before opening, and on
/// Unix additionally sets `O_NOFOLLOW` to close the TOCTOU window between the
/// stat and the open. `create(false)` prevents re-creating a file that was
/// deleted between the check and the open (which would otherwise leave behind
/// a zero-byte file with default umask permissions).
fn truncate_regular_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let meta = std::fs::symlink_metadata(path)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to truncate symlink",
        ));
    }
    if !ft.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).truncate(true).create(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    opts.open(path)
}

/// Execute the `emterm mux clear-logs` command.
///
/// Truncates `mux-daemon.log`, `mux-bridge.log`, and `mux-client.log` in-place.
/// Truncation (not removal) keeps open file descriptors held by the running
/// daemon/bridge valid — new log lines continue writing from offset 0.
///
/// Returns `Err` if any targeted file could not be truncated, so callers
/// (including the CLI dispatcher in `main.rs`) propagate a non-zero exit
/// status. Partial-failure details are still printed to stderr per file.
pub fn execute_clear_logs() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    let log_dir = sock_path
        .parent()
        .ok_or("Failed to resolve mux log directory")?;

    let files = ["mux-daemon.log", "mux-bridge.log", "mux-client.log"];
    let mut cleared = 0u32;
    let mut total_bytes = 0u64;
    let mut failed = 0u32;

    for name in &files {
        let path = log_dir.join(name);
        // Use symlink_metadata so a dangling/victim symlink is treated as
        // "refuse to touch" rather than being silently skipped via exists().
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("Failed to stat {}: {}", path.display(), e);
                failed += 1;
                continue;
            }
        };
        let size = meta.len();
        match truncate_regular_file(&path) {
            Ok(_) => {
                println!("Cleared: {} ({})", path.display(), format_size(size));
                cleared += 1;
                total_bytes += size;
            }
            Err(e) => {
                eprintln!("Failed to clear {}: {}", path.display(), e);
                failed += 1;
            }
        }
    }

    if cleared == 0 && failed == 0 {
        println!("No mux log files to clear in {}", log_dir.display());
    } else {
        println!(
            "Cleared {} file(s), freed {}",
            cleared,
            format_size(total_bytes)
        );
    }

    if failed > 0 {
        Err(format!("Failed to clear {} file(s)", failed).into())
    } else {
        Ok(())
    }
}
