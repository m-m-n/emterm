//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, IntoRawFd, RawFd};

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

/// Get the socket path for the mux daemon.
///
/// Linux: `$XDG_RUNTIME_DIR/emterm/mux-default.sock`
///   fallback: `~/.local/run/emterm/mux-default.sock`
/// Windows: `%LOCALAPPDATA%\emterm\mux-default.sock`
pub fn socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(runtime_dir)
                .join("emterm")
                .join("mux-default.sock")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".local")
                .join("run")
                .join("emterm")
                .join("mux-default.sock")
        } else {
            PathBuf::from("/tmp")
                .join("emterm")
                .join("mux-default.sock")
        }
    }
    #[cfg(windows)]
    {
        let local_app_data =
            std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
        PathBuf::from(local_app_data)
            .join("emterm")
            .join("mux-default.sock")
    }
}

/// Get the Named Pipe name for the mux daemon (Windows).
///
/// Includes the current username to isolate pipes per user,
/// preventing cross-user access on shared machines.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    format!(r"\\.\pipe\emterm-mux-{}", user)
}

/// Open a mux log file for appending, refusing symlinks on Unix.
///
/// All three mux log files (`mux-daemon.log`, `mux-bridge.log`, `mux-client.log`)
/// live in the same user-writable runtime directory. A pre-placed symlink on
/// that path would otherwise redirect appended log lines into an arbitrary
/// user-writable file. `O_NOFOLLOW` + `mode(0o600)` closes that at open time
/// on Unix. Windows falls back to the default open, which has a residual
/// same-user reparse-point TOCTOU; the directory is per-user (`%LOCALAPPDATA%`)
/// so the threat surface is limited to a locally compromised user account.
pub fn open_mux_log_append(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Check if a daemon is already running by attempting to connect to the socket.
#[cfg(unix)]
pub fn is_daemon_running(path: &std::path::Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Check if a daemon is already running by attempting to open the Named Pipe.
#[cfg(windows)]
pub fn is_daemon_running(_path: &std::path::Path) -> bool {
    use std::fs::OpenOptions;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name())
        .is_ok()
}

/// Remove stale socket file if daemon is not running.
pub fn cleanup_stale_socket(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() && !is_daemon_running(path) {
        log::info!("Removing stale socket: {:?}", path);
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Ensure the mux daemon is running, spawning it if necessary.
///
/// If the socket file does not exist, spawns the daemon as a background
/// process and waits for it to become ready with exponential backoff.
/// Returns the socket path on success.
pub fn ensure_daemon_running() -> Result<PathBuf, String> {
    let sock_path = socket_path();

    // Clean up stale socket (daemon died but socket file remains)
    cleanup_stale_socket(&sock_path)
        .map_err(|e| format!("Failed to clean up stale socket: {}", e))?;

    let mut daemon_running = if cfg!(unix) {
        sock_path.exists()
    } else {
        is_daemon_running(&sock_path)
    };

    if daemon_running {
        // Strategy B (task0010 rework): a presence check alone cannot tell
        // an old-protocol daemon from a compatible one — every mux client
        // would fail against a long-lived v1 daemon after an eMterm
        // upgrade. Probe the real protocol version and, on the adjacent
        // older version, shut the legacy daemon down automatically so a
        // compatible one can start in its place.
        match recover_from_legacy_daemon(&sock_path)? {
            LegacyRecovery::Compatible => {}
            LegacyRecovery::Recovered => daemon_running = false,
        }
    }

    if !daemon_running {
        spawn_daemon(&sock_path)?;
    }

    Ok(sock_path)
}

/// Create the socket's parent directory with restricted permissions, spawn
/// the daemon as a detached background process, and wait for it to become
/// ready with exponential backoff.
///
/// Extracted out of [`ensure_daemon_running`] (task0001) so the `emterm mux
/// attach` path can respawn a daemon after a legacy-daemon recovery
/// shutdown, without duplicating the spawn/readiness logic.
///
/// Precondition: no compatible daemon currently owns `sock_path` (the
/// caller is responsible for stale-socket cleanup, the presence check, and
/// the recovery probe, as [`ensure_daemon_running`] does). Postcondition: a
/// daemon answers on `sock_path`, or this returns an error string identical
/// to the pre-extraction failure messages ("Failed to spawn daemon: …",
/// "Failed to start mux daemon").
pub(in crate::mux) fn spawn_daemon(sock_path: &Path) -> Result<(), String> {
    // Ensure parent directory exists with restricted permissions
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create socket directory: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let exe = crate::self_exec::self_exe_path()
        .map_err(|e| format!("Failed to get executable path: {}", e))?;

    let log_path = sock_path.with_file_name("mux-daemon.log");
    let log_file = open_mux_log_append(&log_path).or_else(|_| {
        let fallback = if cfg!(windows) {
            std::env::temp_dir().join("emterm-mux-daemon.log")
        } else {
            std::path::PathBuf::from("/tmp/emterm-mux-daemon.log")
        };
        open_mux_log_append(&fallback)
    });

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["mux", "--daemon"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    // Fall back to discarding daemon stderr if no log file could be opened
    // (symlink refusal, permission denied, disk full). Prior code panicked
    // via `.unwrap()` on the fallback, which aborted daemon startup.
    match log_file {
        Ok(f) => {
            cmd.stderr(std::process::Stdio::from(f));
        }
        Err(e) => {
            eprintln!(
                "Daemon log unavailable ({}): {} (daemon stderr discarded)",
                log_path.display(),
                e
            );
            cmd.stderr(std::process::Stdio::null());
        }
    }

    // Detach daemon into its own session so it survives parent terminal exit
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe per POSIX
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    cmd.spawn().map_err(|e| {
        crate::self_exec::note_spawn_failure();
        format!("Failed to spawn daemon: {}", e)
    })?;

    // Wait for daemon to start with exponential backoff
    let mut started = false;
    for i in 0..50 {
        if is_daemon_running(sock_path) {
            started = true;
            break;
        }
        let delay = std::cmp::min(10 * (1 << i.min(4)), 100);
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    if !started {
        return Err("Failed to start mux daemon".to_string());
    }

    Ok(())
}

// ============================================================================
// task0010 rework: safe PROTOCOL_VERSION upgrade path (strategy B)
//
// A version bump alone left a running v1 daemon stranded after an eMterm
// upgrade: `ensure_daemon_running` only checked socket presence, and even
// `mux kill` couldn't recover it (the old server rejects a v2 Hello before
// ever reading Shutdown). The helpers below open a short handshake to
// probe the real protocol version and, on the adjacent older version
// (`PREVIOUS_PROTOCOL_VERSION`), send a version-tolerant Shutdown so the
// legacy daemon exits and a compatible one can take its place.
// ============================================================================

/// Outcome of [`recover_from_legacy_daemon`]'s handshake probe.
///
/// `pub(in crate::mux)` (task0001): the `emterm mux attach` path
/// (`mux::cli::execute_attach`) needs to branch on this outcome the same
/// way [`ensure_daemon_running`] does, without exposing it outside the mux
/// module.
#[derive(Debug)]
pub(in crate::mux) enum LegacyRecovery {
    /// The running daemon already accepted a [`PROTOCOL_VERSION`] Hello —
    /// nothing to recover.
    Compatible,
    /// A daemon speaking [`PREVIOUS_PROTOCOL_VERSION`] was found and asked
    /// to exit; the caller should now spawn a fresh daemon.
    Recovered,
}

/// Connect to the daemon's control channel with read/write timeouts, ready
/// for a [`handshake_with_version`] call. Unix: the `AF_UNIX` socket at
/// `sock_path`. Windows: the daemon's Named Pipe (`sock_path` is unused
/// there, matching [`is_daemon_running`]'s existing `_path` convention).
///
/// `pub(in crate::mux)` (task0005): widened so `mux::cli::execute_upgrade`
/// can drive the same connect/handshake sequence for the standalone
/// `upgrade` subcommand, without duplicating it.
#[cfg(unix)]
pub(in crate::mux) fn connect_daemon(
    sock_path: &Path,
) -> std::io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(sock_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

#[cfg(windows)]
pub(in crate::mux) fn connect_daemon(_sock_path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name())
}

/// Send a `Hello` advertising `protocol_version` and read back the decoded
/// `Welcome`. Generic over the platform stream type returned by
/// [`connect_daemon`] (`UnixStream` / Windows `File`), both `Read + Write`.
/// Never panics on a malformed reply — any framing/decode problem surfaces
/// as an `io::Error` so callers can produce a short user-facing message
/// instead of an opaque bincode error (AC-3).
///
/// `pub(in crate::mux)` (task0005): widened alongside [`connect_daemon`] for
/// `mux::cli::execute_upgrade`.
pub(in crate::mux) fn handshake_with_version<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    protocol_version: u32,
) -> std::io::Result<WelcomeMsg> {
    let hello = HelloMsg {
        client_type: ClientType::Cli,
        protocol_version,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "daemon frame exceeds the maximum frame length",
        ));
    }
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;
    let welcome_msg = MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid daemon frame")
    })?;
    welcome_msg.decode_payload::<WelcomeMsg>().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid Welcome payload from daemon",
        )
    })
}

/// Send a bare `Shutdown` control message. `Shutdown`'s wire shape (message
/// type only, empty payload) has never changed, which is what lets a v2
/// client ask an adjacent-version daemon to exit once the Hello handshake
/// has admitted the connection.
fn send_shutdown<S: std::io::Write>(stream: &mut S) -> std::io::Result<()> {
    let msg = MuxMessage {
        msg_type: MessageType::Shutdown,
        pane_id: 0,
        payload: Vec::new(),
    };
    let body = msg.to_frame_body();
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// Send a bare `Upgrade` control message (task0005 Design "upgrade
/// subcommand" / "Recovery path"): requests that the daemon replace itself
/// in place with the currently-installed binary. Mirrors `Shutdown`'s wire
/// shape exactly (type byte, zero pane id, empty payload) — a daemon built
/// before this feature does not recognise [`MessageType::Upgrade`] and
/// discards the frame through the existing unknown-type path (D7), which is
/// why the AC-3/AC-6 timeout route is the expected outcome against those.
///
/// `pub(in crate::mux)`: shared by [`recover_from_legacy_daemon`]'s
/// upgrade-first attempt and `mux::cli::execute_upgrade`.
pub(in crate::mux) fn send_upgrade<S: std::io::Write>(stream: &mut S) -> std::io::Result<()> {
    let msg = MuxMessage {
        msg_type: MessageType::Upgrade,
        pane_id: 0,
        payload: Vec::new(),
    };
    let body = msg.to_frame_body();
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// What the daemon said in reply to a just-sent [`MessageType::Upgrade`]
/// request, read from the SAME connection (task0009 rework, AC-10: finding
/// 07f6dbc60e84d54f -- `mux upgrade` used to drop the connection immediately
/// after sending the request and never observed the daemon's own `Error`
/// reply, so a REFUSED upgrade was reported as success).
#[derive(Debug)]
pub(in crate::mux) enum UpgradeResponse {
    /// The daemon reported the reason it refused (FR13).
    Rejected(String),
    /// No explicit rejection was observed: the connection closed (accepted
    /// connections are dropped once preparation succeeds, IMPLEMENTATION.md
    /// D2) or the read timed out (bounded by the caller's own read timeout,
    /// e.g. [`connect_daemon`]'s 5s) without yielding a full frame. Either
    /// way, this is NOT itself proof of success -- the caller still polls
    /// for reachability afterward (AC-10: "reports success only after
    /// observing evidence that the replacement actually happened").
    ProceededOrUnknown,
}

/// Read exactly one response to a just-sent `Upgrade` request from `stream`
/// (task0005/task0009). Never panics on a malformed/absent reply -- any
/// framing problem, a `WouldBlock`/timeout, or a clean disconnect all
/// resolve to [`UpgradeResponse::ProceededOrUnknown`] rather than erroring,
/// since D2 means an accepted-and-proceeding connection is simply dropped
/// with no further reply.
pub(in crate::mux) fn read_upgrade_response<S: std::io::Read>(stream: &mut S) -> UpgradeResponse {
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        return UpgradeResponse::ProceededOrUnknown;
    }
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len == 0 || frame_len > MAX_FRAME_LENGTH {
        return UpgradeResponse::ProceededOrUnknown;
    }
    let mut frame_buf = vec![0u8; frame_len];
    if stream.read_exact(&mut frame_buf).is_err() {
        return UpgradeResponse::ProceededOrUnknown;
    }
    let Some(frame) = MuxMessage::from_frame_body(&frame_buf) else {
        return UpgradeResponse::ProceededOrUnknown;
    };
    if frame.msg_type != MessageType::Error {
        return UpgradeResponse::ProceededOrUnknown;
    }
    match frame.decode_payload::<ErrorMsg>() {
        Some(err) => UpgradeResponse::Rejected(err.message),
        None => UpgradeResponse::ProceededOrUnknown,
    }
}

/// Poll interval / bound for [`wait_for_daemon_reachable_at_current_version`]
/// (task0005): an in-place upgrade (execve onto an already-listening socket)
/// is expected to complete far faster than the cold-start respawn
/// [`wait_for_daemon_exit`] bounds at 5s, so a shorter ~2s bound is used —
/// generous for the actual replacement, short enough to keep the AC-3/AC-6
/// timeout tests fast.
const UPGRADE_REACHABLE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const UPGRADE_REACHABLE_POLL_MAX_ATTEMPTS: u32 = 40;
/// Overall wall-clock budget for [`wait_for_daemon_reachable_at_current_version`].
/// Each attempt's `handshake_with_version` can block for up to ~5s on a
/// per-read timeout if a peer accepts the connection but withholds its
/// Welcome frame, so the attempt-count cap alone (40 attempts) does not
/// bound total elapsed time (~200s worst case). This deadline caps the
/// whole loop regardless of how many per-attempt timeouts are consumed.
const UPGRADE_REACHABLE_POLL_DEADLINE: Duration = Duration::from_secs(20);

/// Poll `sock_path` (bounded, see [`UPGRADE_REACHABLE_POLL_MAX_ATTEMPTS`])
/// until a daemon there completes a Hello handshake at [`PROTOCOL_VERSION`],
/// as expected after sending an [`MessageType::Upgrade`] request (task0005
/// AC-2/AC-3, Recovery path AC-6/AC-7). Returns `true` once reachable,
/// `false` on timeout — never hangs indefinitely.
///
/// `pub(in crate::mux)`: shared by [`recover_from_legacy_daemon`] and
/// `mux::cli::execute_upgrade`.
pub(in crate::mux) fn wait_for_daemon_reachable_at_current_version(sock_path: &Path) -> bool {
    let deadline = std::time::Instant::now() + UPGRADE_REACHABLE_POLL_DEADLINE;
    for _ in 0..UPGRADE_REACHABLE_POLL_MAX_ATTEMPTS {
        if std::time::Instant::now() >= deadline {
            break;
        }
        if let Ok(mut stream) = connect_daemon(sock_path)
            && let Ok(WelcomeMsg::Accepted { .. }) =
                handshake_with_version(&mut stream, PROTOCOL_VERSION)
        {
            return true;
        }
        std::thread::sleep(UPGRADE_REACHABLE_POLL_INTERVAL);
    }
    false
}

/// Poll until the daemon at `sock_path` is no longer reachable (bounded to
/// ~5s), then remove any leftover socket/marker file. Used after sending a
/// `Shutdown` to a legacy daemon so the caller can safely spawn a
/// replacement without racing the exiting process for the socket.
fn wait_for_daemon_exit(sock_path: &Path) -> Result<(), String> {
    for _ in 0..50 {
        if !is_daemon_running(sock_path) {
            let _ = std::fs::remove_file(sock_path);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(
        "The legacy mux daemon did not exit after a shutdown request within 5 \
         seconds. Stop it manually (e.g. `pkill -f 'emterm mux --daemon'`) and \
         retry."
            .to_string(),
    )
}

/// Probe the daemon already occupying `sock_path` and recover automatically
/// when it is running the adjacent older protocol version (AC-1, task0010
/// rework — see IMPLEMENTATION.md "Old GUI × new daemon pairing"), and, on
/// Unix, trigger the existing hot-upgrade path when a same-protocol daemon's
/// binary was replaced (mux-daemon-binary-update-detect task0002, D5).
///
/// Performs a real handshake first; only on a version mismatch does it
/// retry with [`PREVIOUS_PROTOCOL_VERSION`] (which the legacy daemon
/// accepts) and send a `Shutdown` there.
///
/// Returns `Ok(LegacyRecovery::Compatible)` when the running daemon already
/// speaks [`PROTOCOL_VERSION`] (nothing to do, or a binary-update trigger
/// ran and concluded), `Ok(LegacyRecovery::Recovered)` once a legacy daemon
/// has been asked to exit and has released the socket, or `Err` with a
/// short, human-readable message (never a bincode/decode error, per AC-3)
/// when recovery could not complete.
///
/// `pub(in crate::mux)` (task0001): widened so `mux::cli::execute_attach`
/// can run the same probe before deciding whether to respawn.
#[cfg(unix)]
pub(in crate::mux) fn recover_from_legacy_daemon(
    sock_path: &Path,
) -> Result<LegacyRecovery, String> {
    recover_from_legacy_daemon_with(sock_path, identity::check, |line: &str| {
        eprintln!("{line}");
    })
}

/// Non-Unix build of [`recover_from_legacy_daemon`]: the binary-update
/// detection trigger (D5) is Unix-only because the `identity` module it
/// depends on is Unix-only (IMPLEMENTATION.md Conventions, "every new item
/// is Unix-only"), so this preserves the pre-task0002 behavior verbatim —
/// zero behavior change (NFR2).
#[cfg(not(unix))]
pub(in crate::mux) fn recover_from_legacy_daemon(
    sock_path: &Path,
) -> Result<LegacyRecovery, String> {
    let mut probe = connect_daemon(sock_path)
        .map_err(|e| format!("Could not connect to the existing mux daemon: {e}"))?;
    match handshake_with_version(&mut probe, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => Ok(LegacyRecovery::Compatible),
        Ok(WelcomeMsg::Rejected { reason }) => {
            drop(probe); // the daemon already closed its side after rejecting
            let reported = parse_rejected_server_version(&reason)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            log::warn!(
                "mux daemon at {:?} reports protocol version {} (this build is {}); \
                 attempting automatic recovery",
                sock_path,
                reported,
                PROTOCOL_VERSION
            );

            let mut legacy = connect_daemon(sock_path).map_err(|e| {
                format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but it became unreachable while recovering: {e}"
                )
            })?;
            match handshake_with_version(&mut legacy, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    // task0005 Recovery path: a plain shutdown kills every
                    // pane, so ask the legacy daemon to upgrade itself in
                    // place first. Only fall back to shutdown-then-respawn
                    // if it never becomes reachable at the current protocol
                    // version (AC-6/AC-7). A daemon built before this
                    // feature silently discards the Upgrade frame (D7), so
                    // that timeout is the expected route for those, not an
                    // error.
                    let upgraded = match send_upgrade(&mut legacy) {
                        Ok(()) => {
                            drop(legacy);
                            wait_for_daemon_reachable_at_current_version(sock_path)
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to send an upgrade request to the protocol \
                                 version {reported} daemon: {e}; falling back to \
                                 shutdown"
                            );
                            drop(legacy);
                            false
                        }
                    };

                    if upgraded {
                        log::info!(
                            "Legacy daemon (protocol version {reported}) upgraded in \
                             place; a compatible daemon is now reachable",
                        );
                        return Ok(LegacyRecovery::Compatible);
                    }
                    log::warn!(
                        "Legacy daemon (protocol version {reported}) did not become \
                         reachable at the current protocol version after an upgrade \
                         request; falling back to shutdown"
                    );

                    // Fallback: existing shutdown-then-respawn path. The
                    // upgrade attempt above already dropped the connection
                    // (or never sent), so reconnect for the Shutdown.
                    let mut legacy_for_shutdown = connect_daemon(sock_path).map_err(|e| {
                        format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but it became unreachable while falling back \
                             to shutdown: {e}"
                        )
                    })?;
                    match handshake_with_version(&mut legacy_for_shutdown, PREVIOUS_PROTOCOL_VERSION)
                    {
                        Ok(WelcomeMsg::Accepted { .. }) => {
                            send_shutdown(&mut legacy_for_shutdown).map_err(|e| {
                                format!(
                                    "Detected an incompatible mux daemon (protocol version \
                                     {reported}) but failed to send its shutdown request: {e}"
                                )
                            })?;
                            drop(legacy_for_shutdown);
                            wait_for_daemon_exit(sock_path)?;
                            log::info!(
                                "Recovered mux socket from a protocol version {} daemon; a \
                                 compatible daemon can now start",
                                reported
                            );
                            Ok(LegacyRecovery::Recovered)
                        }
                        Ok(WelcomeMsg::Rejected {
                            reason: retry_reason,
                        }) => Err(format!(
                            "The running mux daemon (protocol version {reported}) could not \
                             be recovered automatically: {retry_reason}. Stop it manually \
                             (e.g. `pkill -f 'emterm mux --daemon'`) and retry."
                        )),
                        Err(e) => Err(format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but failed to negotiate a compatible shutdown \
                             after the upgrade attempt: {e}"
                        )),
                    }
                }
                Ok(WelcomeMsg::Rejected {
                    reason: retry_reason,
                }) => Err(format!(
                    "The running mux daemon (protocol version {reported}) could not \
                     be recovered automatically: {retry_reason}. Stop it manually \
                     (e.g. `pkill -f 'emterm mux --daemon'`) and retry."
                )),
                Err(e) => Err(format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but failed to negotiate a compatible shutdown: {e}"
                )),
            }
        }
        Err(e) => Err(format!(
            "Failed to communicate with the existing mux daemon: {e}"
        )),
    }
}

/// Unix, parameterized variant of [`recover_from_legacy_daemon`]
/// (mux-daemon-binary-update-detect task0002, D5/D6): injects the identity-
/// verdict provider and a user-message sink so unit tests can drive every
/// verdict and assert emitted lines without a real identity file or a real
/// terminal. [`recover_from_legacy_daemon`] is the production entry point,
/// wired to [`identity::check`] and standard error.
///
/// Numbered flow (D5) for the Compatible arm: delegated to
/// [`trigger_binary_update_if_detected`]. The legacy arm (version mismatch)
/// is otherwise unchanged from the pre-task0002 behavior, plus the pinned
/// FR5 warning (D6) at the single point it commits to the shutdown-then-
/// respawn fallback.
#[cfg(unix)]
pub(in crate::mux) fn recover_from_legacy_daemon_with(
    sock_path: &Path,
    identity_check: impl Fn(&Path) -> identity::Verdict,
    mut message: impl FnMut(&str),
) -> Result<LegacyRecovery, String> {
    let mut probe = connect_daemon(sock_path)
        .map_err(|e| format!("Could not connect to the existing mux daemon: {e}"))?;
    match handshake_with_version(&mut probe, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => {
            trigger_binary_update_if_detected(probe, sock_path, identity_check, message)
        }
        Ok(WelcomeMsg::Rejected { reason }) => {
            drop(probe); // the daemon already closed its side after rejecting
            let reported = parse_rejected_server_version(&reason)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            log::warn!(
                "mux daemon at {:?} reports protocol version {} (this build is {}); \
                 attempting automatic recovery",
                sock_path,
                reported,
                PROTOCOL_VERSION
            );

            let mut legacy = connect_daemon(sock_path).map_err(|e| {
                format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but it became unreachable while recovering: {e}"
                )
            })?;
            match handshake_with_version(&mut legacy, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    // task0005 Recovery path: a plain shutdown kills every
                    // pane, so ask the legacy daemon to upgrade itself in
                    // place first. Only fall back to shutdown-then-respawn
                    // if it never becomes reachable at the current protocol
                    // version (AC-6/AC-7). A daemon built before this
                    // feature silently discards the Upgrade frame (D7), so
                    // that timeout is the expected route for those, not an
                    // error.
                    let upgraded = match send_upgrade(&mut legacy) {
                        Ok(()) => {
                            drop(legacy);
                            wait_for_daemon_reachable_at_current_version(sock_path)
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to send an upgrade request to the protocol \
                                 version {reported} daemon: {e}; falling back to \
                                 shutdown"
                            );
                            drop(legacy);
                            false
                        }
                    };

                    if upgraded {
                        log::info!(
                            "Legacy daemon (protocol version {reported}) upgraded in \
                             place; a compatible daemon is now reachable",
                        );
                        return Ok(LegacyRecovery::Compatible);
                    }
                    log::warn!(
                        "Legacy daemon (protocol version {reported}) did not become \
                         reachable at the current protocol version after an upgrade \
                         request; falling back to shutdown"
                    );
                    // mux-daemon-binary-update-detect task0002 D6: warn the
                    // user before the fallback destroys panes. Single point
                    // both the ignored-upgrade-timeout route and the
                    // failed-send route converge on (both set
                    // `upgraded = false` above).
                    const FR5_WARNING: &str = "The running mux daemon predates in-place upgrade support; panes cannot be preserved and will be recreated.";
                    message(FR5_WARNING);
                    log::warn!("{FR5_WARNING}");

                    // Fallback: existing shutdown-then-respawn path. The
                    // upgrade attempt above already dropped the connection
                    // (or never sent), so reconnect for the Shutdown.
                    let mut legacy_for_shutdown = connect_daemon(sock_path).map_err(|e| {
                        format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but it became unreachable while falling back \
                             to shutdown: {e}"
                        )
                    })?;
                    match handshake_with_version(&mut legacy_for_shutdown, PREVIOUS_PROTOCOL_VERSION)
                    {
                        Ok(WelcomeMsg::Accepted { .. }) => {
                            send_shutdown(&mut legacy_for_shutdown).map_err(|e| {
                                format!(
                                    "Detected an incompatible mux daemon (protocol version \
                                     {reported}) but failed to send its shutdown request: {e}"
                                )
                            })?;
                            drop(legacy_for_shutdown);
                            wait_for_daemon_exit(sock_path)?;
                            log::info!(
                                "Recovered mux socket from a protocol version {} daemon; a \
                                 compatible daemon can now start",
                                reported
                            );
                            Ok(LegacyRecovery::Recovered)
                        }
                        Ok(WelcomeMsg::Rejected {
                            reason: retry_reason,
                        }) => Err(format!(
                            "The running mux daemon (protocol version {reported}) could not \
                             be recovered automatically: {retry_reason}. Stop it manually \
                             (e.g. `pkill -f 'emterm mux --daemon'`) and retry."
                        )),
                        Err(e) => Err(format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but failed to negotiate a compatible shutdown \
                             after the upgrade attempt: {e}"
                        )),
                    }
                }
                Ok(WelcomeMsg::Rejected {
                    reason: retry_reason,
                }) => Err(format!(
                    "The running mux daemon (protocol version {reported}) could not \
                     be recovered automatically: {retry_reason}. Stop it manually \
                     (e.g. `pkill -f 'emterm mux --daemon'`) and retry."
                )),
                Err(e) => Err(format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but failed to negotiate a compatible shutdown: {e}"
                )),
            }
        }
        Err(e) => Err(format!(
            "Failed to communicate with the existing mux daemon: {e}"
        )),
    }
}

/// D5's Compatible-arm binary-update trigger: consults `identity_check`
/// against `sock_path` and, only on [`identity::Verdict::Updated`], fires
/// the existing hot-upgrade path on the already-handshaked `probe`
/// connection (no second connection, no payload — the client never
/// transmits a path, NFR3). Always resolves to `Ok(LegacyRecovery::Compatible)`:
/// a detection or upgrade failure here never becomes an attach / mux-start
/// failure (D5's "fires at most once, never converts a failure").
#[cfg(unix)]
fn trigger_binary_update_if_detected<S: std::io::Read + std::io::Write>(
    mut probe: S,
    sock_path: &Path,
    identity_check: impl Fn(&Path) -> identity::Verdict,
    mut message: impl FnMut(&str),
) -> Result<LegacyRecovery, String> {
    match identity_check(sock_path) {
        identity::Verdict::Unchanged | identity::Verdict::Undecidable => {
            Ok(LegacyRecovery::Compatible)
        }
        identity::Verdict::Updated(_clean_target) => {
            if let Err(e) = send_upgrade(&mut probe) {
                log::warn!("Failed to send an automatic binary-update upgrade request: {e}");
                return Ok(LegacyRecovery::Compatible);
            }
            let response = read_upgrade_response(&mut probe);
            drop(probe);
            match response {
                UpgradeResponse::Rejected(reason) => {
                    let line = format!(
                        "Warning: mux daemon declined the automatic in-place upgrade after detecting a binary update: {reason}"
                    );
                    message(&line);
                    log::warn!("{line}");
                    Ok(LegacyRecovery::Compatible)
                }
                UpgradeResponse::ProceededOrUnknown => {
                    if wait_for_daemon_reachable_at_current_version(sock_path) {
                        const NOTICE: &str =
                            "Mux daemon upgraded in place to the newly installed binary";
                        message(NOTICE);
                        log::warn!("{NOTICE}");
                    } else {
                        const TIMEOUT_WARNING: &str = "Warning: timed out waiting for the mux daemon to become reachable after an automatic binary-update upgrade; continuing with the existing daemon";
                        message(TIMEOUT_WARNING);
                        log::warn!("{TIMEOUT_WARNING}");
                    }
                    Ok(LegacyRecovery::Compatible)
                }
            }
        }
    }
}

/// Result of [`shutdown_daemon_any_version`], used by `emterm mux kill`
/// (AC-2/AC-3, task0010 rework).
#[derive(Debug)]
pub enum ShutdownOutcome {
    /// A Shutdown request was accepted by the daemon. Carries a short
    /// user-facing status line (e.g. noting when a legacy protocol version
    /// was detected and handled automatically).
    ShutDown(String),
    /// The daemon was unreachable outright (process already gone); the
    /// stale socket/marker file was removed. Mirrors the pre-task0010
    /// `execute_kill` fallback behavior.
    StaleSocketRemoved(String),
}

/// Shut down whatever daemon is occupying `sock_path`, regardless of
/// protocol version (AC-2). Tries [`PROTOCOL_VERSION`] first; on a version
/// mismatch it retries with [`PREVIOUS_PROTOCOL_VERSION`] so an adjacent
/// legacy daemon accepts the connection and can be asked to exit. Every
/// failure path returns a short explanatory message — never an opaque
/// bincode/decode error (AC-3).
pub fn shutdown_daemon_any_version(sock_path: &Path) -> Result<ShutdownOutcome, String> {
    let mut stream = match connect_daemon(sock_path) {
        Ok(s) => s,
        Err(_) => {
            let _ = std::fs::remove_file(sock_path);
            return Ok(ShutdownOutcome::StaleSocketRemoved(
                "Mux daemon not reachable (stale socket removed)".to_string(),
            ));
        }
    };

    match handshake_with_version(&mut stream, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => {
            send_shutdown(&mut stream)
                .map_err(|e| format!("Failed to send shutdown request: {e}"))?;
            Ok(ShutdownOutcome::ShutDown(
                "Mux daemon shutting down".to_string(),
            ))
        }
        Ok(WelcomeMsg::Rejected { reason }) => {
            drop(stream);
            let reported = parse_rejected_server_version(&reason)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let mut legacy = connect_daemon(sock_path).map_err(|e| {
                format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but it became unreachable while retrying: {e}"
                )
            })?;
            match handshake_with_version(&mut legacy, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    send_shutdown(&mut legacy).map_err(|e| {
                        format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but failed to send its shutdown request: {e}"
                        )
                    })?;
                    Ok(ShutdownOutcome::ShutDown(format!(
                        "Detected a mux daemon on an older protocol version ({reported}); \
                         sent a compatible shutdown request. Run `emterm mux` to start \
                         the current version."
                    )))
                }
                Ok(WelcomeMsg::Rejected {
                    reason: retry_reason,
                }) => Err(format!(
                    "The running mux daemon (protocol version {reported}) could not be \
                     shut down automatically: {retry_reason}. Stop it manually (e.g. \
                     `pkill -f 'emterm mux --daemon'`) and retry."
                )),
                Err(e) => Err(format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but failed to negotiate a compatible shutdown: {e}"
                )),
            }
        }
        Err(e) => Err(format!("Failed to communicate with the mux daemon: {e}")),
    }
}

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
fn probe_candidate_handoff_range(
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
fn real_snapshot(
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
const UPGRADE_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

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
async fn prepare_upgrade(
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
fn start_from_handoff(
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
fn startup(
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
                "mux hot-upgrade is not supported on this platform".to_string(),
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

/// Apply a title change to the SessionManager with diff detection.
///
/// Returns `true` when `window.name` was updated and a broadcast was sent;
/// `false` when the pane was not found or the title was unchanged.
async fn apply_title_change(
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
async fn run_title_update_task(
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
async fn run_notification_task(
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
async fn apply_agent_status_report(
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
async fn apply_live_osc133_mark(
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
async fn run_agent_status_task(
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
async fn run_pane_exit_task(
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
async fn graceful_shutdown(session_manager: &Arc<Mutex<SessionManager>>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::path::PathBuf;

    #[test]
    fn test_socket_path_not_empty() {
        let path = socket_path();
        assert!(!path.as_os_str().is_empty());
        assert!(path.to_str().unwrap().contains("emterm"));
        assert!(path.to_str().unwrap().contains("mux-default.sock"));
    }

    #[test]
    fn test_socket_path_contains_directory() {
        let path = socket_path();
        assert!(path.parent().is_some());
    }

    #[test]
    fn test_cleanup_stale_nonexistent() {
        let path = PathBuf::from("/tmp/emterm-test-nonexistent.sock");
        assert!(cleanup_stale_socket(&path).is_ok());
    }

    // ---- task0010 rework: legacy-daemon recovery (strategy B) ----
    //
    // A fake v1 daemon is a bare `UnixListener` thread rather than a real
    // spawned process, per the task's Test Notes ("Simulate a v1 server ...
    // by manually crafting the handshake bytes"). It speaks the exact wire
    // shapes (`HelloMsg`/`WelcomeMsg`/`Shutdown`) the real daemon does, with
    // a hardcoded `server_version` and no session/PTY machinery.

    // task0005 rework: derived from `PREVIOUS_PROTOCOL_VERSION` rather than
    // hardcoded to `1`. `recover_from_legacy_daemon`'s retry handshake uses
    // `PREVIOUS_PROTOCOL_VERSION` (exactly one version behind whatever
    // `PROTOCOL_VERSION` currently is) — a fixed literal here silently
    // stopped matching that retry the moment `PROTOCOL_VERSION` moved past
    // 2, at which point the fake daemon's `else` branch below rejects the
    // retry, `recover_from_legacy_daemon` gives up (returns `Err`), and the
    // fake daemon's `accept()` loop is left waiting forever for a THIRD
    // connection that will never arrive — a `server.join()` in a test below
    // then hangs indefinitely. Tying this constant to
    // `PREVIOUS_PROTOCOL_VERSION` keeps the fixture "one version behind
    // current" through any future bump.
    #[cfg(unix)]
    const FAKE_LEGACY_VERSION: u32 = PREVIOUS_PROTOCOL_VERSION;

    #[cfg(unix)]
    fn read_frame<S: std::io::Read>(stream: &mut S) -> MuxMessage {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).expect("read frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        stream.read_exact(&mut frame_buf).expect("read frame body");
        MuxMessage::from_frame_body(&frame_buf).expect("valid frame")
    }

    #[cfg(unix)]
    fn write_welcome<S: std::io::Write>(stream: &mut S, welcome: &WelcomeMsg) {
        let msg = MuxMessage::control(MessageType::Welcome, 0, welcome);
        let body = msg.to_frame_body();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .expect("write frame length");
        stream.write_all(&body).expect("write frame body");
        stream.flush().expect("flush");
    }

    /// Spawn a thread that behaves like a single-instance legacy (v1) mux
    /// daemon on `sock_path`: rejects any Hello whose `protocol_version`
    /// isn't [`FAKE_LEGACY_VERSION`] with the exact reason text the real
    /// daemon produces, accepts a matching Hello, then per-frame: an
    /// `Upgrade` request is silently ignored (task0005 Recovery path — a
    /// daemon predating that feature discards it via the unknown-type path,
    /// D7) and the loop keeps accepting; `Shutdown` removes the socket file
    /// and exits — mirroring the real daemon's Shutdown ->
    /// `graceful_shutdown` -> `remove_file` sequence.
    #[cfg(unix)]
    fn spawn_fake_legacy_daemon(sock_path: std::path::PathBuf) -> std::thread::JoinHandle<()> {
        use std::os::unix::net::UnixListener;

        let listener = UnixListener::bind(&sock_path).expect("bind fake daemon socket");
        std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let hello_frame = read_frame(&mut stream);
                assert_eq!(hello_frame.msg_type, MessageType::Hello);
                let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");

                if hello.protocol_version != FAKE_LEGACY_VERSION {
                    let reject = WelcomeMsg::Rejected {
                        reason: format!(
                            "Protocol version mismatch: client={}, server={}",
                            hello.protocol_version, FAKE_LEGACY_VERSION
                        ),
                    };
                    write_welcome(&mut stream, &reject);
                    // Connection closes here (stream dropped) — the real
                    // daemon's handshake path returns immediately after
                    // sending Rejected too.
                    continue;
                }

                let accept = WelcomeMsg::Accepted {
                    server_version: FAKE_LEGACY_VERSION,
                    sessions: Vec::<crate::mux::ipc::protocol::SessionInfo>::new(),
                };
                write_welcome(&mut stream, &accept);

                let frame = read_frame(&mut stream);
                match frame.msg_type {
                    MessageType::Upgrade => continue,
                    MessageType::Shutdown => {
                        // Simulate process exit: release the socket like the
                        // real daemon's shutdown path does.
                        let _ = std::fs::remove_file(&sock_path);
                        break;
                    }
                    other => panic!("unexpected frame after legacy Accepted: {other:?}"),
                }
            }
        })
    }

    /// AC-1: a v2 client recovers from encountering a running v1 daemon —
    /// `recover_from_legacy_daemon` detects the mismatch, sends a
    /// version-tolerant Shutdown, waits for the legacy daemon to release
    /// the socket, and reports `Recovered`.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_shuts_down_v1_and_reports_recovered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("legacy.sock");
        let server = spawn_fake_legacy_daemon(sock_path.clone());

        let result = recover_from_legacy_daemon(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Recovered) => {}
            other => panic!("expected Ok(Recovered), got {other:?}"),
        }
        assert!(
            !sock_path.exists(),
            "legacy daemon's socket file must be removed after recovery"
        );
    }

    /// AC-4: a compatible (current-version) daemon is left untouched —
    /// `recover_from_legacy_daemon` performs exactly one Hello/Welcome
    /// round trip and reports `Compatible` without sending Shutdown.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_is_noop_against_a_compatible_daemon() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("compatible.sock");
        let listener = UnixListener::bind(&sock_path).expect("bind fake v2 daemon socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let hello_frame = read_frame(&mut stream);
            let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
            assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
            let accept = WelcomeMsg::Accepted {
                server_version: PROTOCOL_VERSION,
                sessions: Vec::<crate::mux::ipc::protocol::SessionInfo>::new(),
            };
            write_welcome(&mut stream, &accept);
        });

        let result = recover_from_legacy_daemon(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert!(sock_path.exists(), "a compatible daemon is left untouched");
    }

    // ---- mux-daemon-binary-update-detect task0002: Compatible-arm
    // binary-update trigger (D5, AC-1..AC-5) ----
    //
    // These tests call the parameterized `recover_from_legacy_daemon_with`
    // directly, injecting the identity verdict and a message-sink closure,
    // per the task plan's Testability note (the established
    // `resolve_attach_socket_with` / `execute_upgrade_at` injection
    // pattern) -- production `identity::check` always reports Undecidable
    // (this task's stand-in), so these verdicts cannot be driven through
    // real identity files.

    /// Configures how [`spawn_fake_current_daemon`] reacts to an `Upgrade`
    /// frame arriving after its (current-protocol) handshake.
    #[cfg(unix)]
    enum UpgradeBehavior {
        /// Reply with an `Error` frame carrying this reason (AC-4).
        Reject(String),
        /// Drop the connection without replying, and never accept another
        /// one -- the reachability wait must time out (AC-5).
        Silent,
        /// Drop the connection without replying, then keep accepting and
        /// answering Hello at [`PROTOCOL_VERSION`] as if the replacement had
        /// completed, so the reachability wait succeeds (AC-1).
        BecomeUpgraded,
    }

    /// Write a bare `Error` control frame carrying `message` (mirrors
    /// [`write_welcome`]'s shape for the fake-daemon fixtures).
    #[cfg(unix)]
    fn write_error<S: std::io::Write>(stream: &mut S, message: &str) {
        let msg = MuxMessage::control(
            MessageType::Error,
            0,
            &ErrorMsg {
                message: message.to_string(),
            },
        );
        let body = msg.to_frame_body();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .expect("write frame length");
        stream.write_all(&body).expect("write frame body");
        stream.flush().expect("flush");
    }

    /// Stand-in CURRENT-protocol ([`PROTOCOL_VERSION`]) daemon for the
    /// binary-update trigger tests (AC-1..AC-5): accepts the
    /// [`PROTOCOL_VERSION`] Hello unconditionally, and when `behavior` is
    /// `Some`, expects an `Upgrade` frame after the handshake and reacts per
    /// [`UpgradeBehavior`]; when `None`, no `Upgrade` is expected and the
    /// thread exits after the first handshake (AC-2/AC-3, where the trigger
    /// must not fire at all). Returns the join handle plus a counter of how
    /// many `Upgrade` frames were observed, so tests can assert "exactly
    /// one" (AC-1) or "none" (AC-2/AC-3).
    #[cfg(unix)]
    fn spawn_fake_current_daemon(
        sock_path: std::path::PathBuf,
        behavior: Option<UpgradeBehavior>,
    ) -> (
        std::thread::JoinHandle<()>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::os::unix::net::UnixListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let listener =
            UnixListener::bind(&sock_path).expect("bind fake current-protocol daemon socket");
        let upgrade_frames_seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&upgrade_frames_seen);
        let upgraded = Arc::new(AtomicBool::new(false));

        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let hello_frame = read_frame(&mut stream);
                assert_eq!(hello_frame.msg_type, MessageType::Hello);
                let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
                assert_eq!(hello.protocol_version, PROTOCOL_VERSION);

                let accept = WelcomeMsg::Accepted {
                    server_version: PROTOCOL_VERSION,
                    sessions: Vec::<crate::mux::ipc::protocol::SessionInfo>::new(),
                };
                write_welcome(&mut stream, &accept);

                if upgraded.load(Ordering::SeqCst) {
                    // Post-"upgrade" connection: prove reachability and stop.
                    break;
                }

                let Some(behavior) = behavior.as_ref() else {
                    // No Upgrade expected -- one round trip and done.
                    break;
                };

                let frame = read_frame(&mut stream);
                assert_eq!(
                    frame.msg_type,
                    MessageType::Upgrade,
                    "expected an Upgrade frame after the handshake"
                );
                counter.fetch_add(1, Ordering::SeqCst);

                match behavior {
                    UpgradeBehavior::Reject(reason) => {
                        write_error(&mut stream, reason);
                        break;
                    }
                    UpgradeBehavior::Silent => {
                        drop(stream);
                        break;
                    }
                    UpgradeBehavior::BecomeUpgraded => {
                        upgraded.store(true, Ordering::SeqCst);
                        drop(stream);
                    }
                }
            }
        });
        (handle, upgrade_frames_seen)
    }

    /// AC-1: against a current-protocol fake daemon, with an injected
    /// `Updated` verdict, the probe sends exactly one `Upgrade` frame and --
    /// once the daemon is reachable again -- emits exactly the pinned
    /// notice line once, and returns success.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_fires_upgrade_and_reports_notice_when_reachable_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-fire.sock");
        let (server, upgrade_frames_seen) =
            spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::BecomeUpgraded));

        let mut messages: Vec<String> = Vec::new();
        let result = recover_from_legacy_daemon_with(
            &sock_path,
            |_sock_path: &Path| identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
            |line: &str| messages.push(line.to_string()),
        );

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "expected exactly one Upgrade frame"
        );
        assert_eq!(
            messages,
            vec!["Mux daemon upgraded in place to the newly installed binary".to_string()],
            "expected exactly the pinned notice line, emitted once"
        );
    }

    /// AC-2: with an injected `Unchanged` verdict, no `Upgrade` frame is
    /// sent, no notice is emitted, and the daemon is left untouched -- byte-
    /// identical to the pre-feature Compatible path.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_unchanged_verdict_does_not_fire() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-unchanged.sock");
        let (server, upgrade_frames_seen) = spawn_fake_current_daemon(sock_path.clone(), None);

        let mut messages: Vec<String> = Vec::new();
        let result = recover_from_legacy_daemon_with(
            &sock_path,
            |_sock_path: &Path| identity::Verdict::Unchanged,
            |line: &str| messages.push(line.to_string()),
        );

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no Upgrade frame must be sent for an Unchanged verdict"
        );
        assert!(messages.is_empty(), "no message expected: {messages:?}");
        assert!(sock_path.exists(), "a compatible daemon is left untouched");
    }

    /// AC-3: an injected `Undecidable` verdict behaves exactly like AC-2's
    /// `Unchanged` verdict -- no fire, no notice.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_undecidable_verdict_does_not_fire() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-undecidable.sock");
        let (server, upgrade_frames_seen) = spawn_fake_current_daemon(sock_path.clone(), None);

        let mut messages: Vec<String> = Vec::new();
        let result = recover_from_legacy_daemon_with(
            &sock_path,
            |_sock_path: &Path| identity::Verdict::Undecidable,
            |line: &str| messages.push(line.to_string()),
        );

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no Upgrade frame must be sent for an Undecidable verdict"
        );
        assert!(messages.is_empty(), "no message expected: {messages:?}");
        assert!(sock_path.exists(), "a compatible daemon is left untouched");
    }

    /// AC-4: when the daemon replies with a rejection to the fired
    /// `Upgrade`, the emitted warning contains the daemon's reason, and the
    /// probe still returns success.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_upgrade_rejected_warns_and_still_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-rejected.sock");
        let (server, upgrade_frames_seen) = spawn_fake_current_daemon(
            sock_path.clone(),
            Some(UpgradeBehavior::Reject("no recorded identity".to_string())),
        );

        let mut messages: Vec<String> = Vec::new();
        let result = recover_from_legacy_daemon_with(
            &sock_path,
            |_sock_path: &Path| identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
            |line: &str| messages.push(line.to_string()),
        );

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(messages.len(), 1, "expected exactly one message: {messages:?}");
        assert!(
            messages[0].contains("no recorded identity"),
            "expected the warning to contain the daemon's reason: {:?}",
            messages[0]
        );
    }

    /// AC-5: when the fired `Upgrade` is followed by a reachability
    /// timeout, a warning is emitted and the probe still returns success.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_upgrade_timeout_warns_and_still_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-timeout.sock");
        let (server, upgrade_frames_seen) =
            spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::Silent));

        let mut messages: Vec<String> = Vec::new();
        let result = recover_from_legacy_daemon_with(
            &sock_path,
            |_sock_path: &Path| identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
            |line: &str| messages.push(line.to_string()),
        );

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(messages.len(), 1, "expected exactly one message: {messages:?}");
        assert!(
            messages[0].to_lowercase().contains("timed out") || messages[0].to_lowercase().contains("timeout"),
            "expected a timeout warning: {:?}",
            messages[0]
        );
    }

    /// AC-7 (partial, unit-testable slice): the production entry point
    /// [`recover_from_legacy_daemon`] wires the real [`identity::check`]
    /// (this task's stand-in, always `Undecidable`), so against a current-
    /// protocol daemon it behaves exactly like the AC-2/AC-3 no-fire path
    /// with zero test-only wiring -- proving the production call site
    /// actually consumes the pinned check API rather than leaving it
    /// unwired.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_production_entry_point_does_not_fire_against_the_stand_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("update-detect-production-entry.sock");
        let (server, upgrade_frames_seen) = spawn_fake_current_daemon(sock_path.clone(), None);

        let result = recover_from_legacy_daemon(&sock_path);

        server.join().expect("fake current-protocol daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert_eq!(
            upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the stand-in identity::check always reports Undecidable, so the \
             production entry point must not fire"
        );
    }

    /// AC-2: `emterm mux kill`'s underlying helper succeeds against a v1
    /// daemon. AC-3: the resulting message is plain, human-readable text —
    /// never an opaque bincode/decode error.
    #[cfg(unix)]
    #[test]
    fn shutdown_daemon_any_version_succeeds_against_v1_daemon() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("legacy.sock");
        let server = spawn_fake_legacy_daemon(sock_path.clone());

        let result = shutdown_daemon_any_version(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(ShutdownOutcome::ShutDown(msg)) => {
                assert!(
                    msg.is_ascii(),
                    "expected a plain-text status message, got {msg:?}"
                );
                assert!(
                    msg.to_lowercase().contains("protocol version"),
                    "expected the message to explain the protocol mismatch, got {msg:?}"
                );
            }
            other => panic!("expected Ok(ShutDown(_)), got {other:?}"),
        }
    }

    /// `shutdown_daemon_any_version` falls back to stale-file cleanup when
    /// the daemon is unreachable outright (process already gone), mirroring
    /// the pre-task0010 `execute_kill` behavior.
    #[cfg(unix)]
    #[test]
    fn shutdown_daemon_any_version_removes_stale_socket_when_unreachable() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No listener bound at this path: connect() fails immediately.
        let sock_path = dir.path().join("nothing-here.sock");

        let result = shutdown_daemon_any_version(&sock_path);
        match result {
            Ok(ShutdownOutcome::StaleSocketRemoved(msg)) => {
                assert!(msg.contains("not reachable"));
            }
            other => panic!("expected Ok(StaleSocketRemoved(_)), got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_marks_all_panes_exited() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::sync::mpsc;

        fn make_test_pane(id: u32) -> MuxPane {
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            MuxPane::new_test(id, 80, 24, target)
        }

        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        // Set up two sessions with panes
        {
            let mut m = mgr.lock().await;
            let s1 = m.create_session("s1".to_string());
            let w1 = m.create_window(s1, "w1".to_string()).unwrap();
            let session = m.get_session_mut(s1).unwrap();
            session
                .windows
                .get_mut(&w1)
                .unwrap()
                .add_pane(make_test_pane(10));
            session
                .windows
                .get_mut(&w1)
                .unwrap()
                .add_pane(make_test_pane(11));

            let s2 = m.create_session("s2".to_string());
            let w2 = m.create_window(s2, "w2".to_string()).unwrap();
            let session2 = m.get_session_mut(s2).unwrap();
            session2
                .windows
                .get_mut(&w2)
                .unwrap()
                .add_pane(make_test_pane(20));
        }

        graceful_shutdown(&mgr).await;

        // Verify all panes are marked exited
        let m = mgr.lock().await;
        for session in m.sessions_iter() {
            for window in session.windows.values() {
                for pane in window.panes.values() {
                    assert!(pane.exited, "pane {} should be exited", pane.id);
                }
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_skips_already_exited() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::sync::mpsc;

        fn make_test_pane(id: u32) -> MuxPane {
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            MuxPane::new_test(id, 80, 24, target)
        }

        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        {
            let mut m = mgr.lock().await;
            let s1 = m.create_session("s1".to_string());
            let w1 = m.create_window(s1, "w1".to_string()).unwrap();
            let session = m.get_session_mut(s1).unwrap();
            let window = session.windows.get_mut(&w1).unwrap();
            window.add_pane(make_test_pane(10));
            window.add_pane(make_test_pane(11));
            // Mark one pane as already exited
            window.panes.get_mut(&10).unwrap().mark_exited();
        }

        // Should not panic; should handle already-exited panes gracefully
        graceful_shutdown(&mgr).await;

        let m = mgr.lock().await;
        let session = m.sessions_iter().next().unwrap();
        let window = session.windows.values().next().unwrap();
        assert!(window.panes.get(&10).unwrap().exited);
        assert!(window.panes.get(&11).unwrap().exited);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_empty_manager() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        // Should not panic on empty manager
        graceful_shutdown(&mgr).await;
    }

    /// Helpers for title-update tests. Returns the pane plus the `Sender`
    /// installed into its `output_target`, so tests can pass the matching
    /// `Sender` to identity-scoped `detach_session_panes`.
    fn make_title_test_pane(
        id: u32,
    ) -> (
        crate::mux::session::pane::MuxPane,
        mpsc::Sender<crate::mux::session::pane::PtyOutputChunk>,
    ) {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget =
            StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
        (MuxPane::new_test(id, 80, 24, target), tx)
    }

    async fn setup_single_pane_manager() -> (
        Arc<Mutex<SessionManager>>,
        u32,
        u32,
        u32,
        mpsc::Sender<crate::mux::session::pane::PtyOutputChunk>,
    ) {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        let pane_id = 42;
        let (pane, pane_tx) = make_title_test_pane(pane_id);
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        drop(m);
        (mgr, sid, wid, pane_id, pane_tx)
    }

    #[tokio::test]
    async fn test_apply_title_change_updates_window_and_broadcasts() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let changed = apply_title_change(&mgr, pane_id, "hello".to_string()).await;
        assert!(changed, "first title change should return true");

        let m = mgr.lock().await;
        let name = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .name
            .clone();
        assert_eq!(name, "hello");
        drop(m);

        let msg = notify_rx.recv().await.unwrap();
        assert_eq!(msg.msg_type, MessageType::RenameWindow);
        assert_eq!(msg.pane_id, wid);
        let payload: RenameWindowMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.name, "hello");
    }

    #[tokio::test]
    async fn test_apply_title_change_same_title_skips_broadcast() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let _ = apply_title_change(&mgr, pane_id, "hello".to_string()).await;

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        let changed = apply_title_change(&mgr, pane_id, "hello".to_string()).await;
        assert!(!changed, "same title should return false");

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            timeout.is_err(),
            "no broadcast should be sent for unchanged title"
        );
    }

    #[tokio::test]
    async fn test_apply_title_change_unknown_pane_no_change() {
        let (mgr, sid, wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let changed = apply_title_change(&mgr, 9999, "bogus".to_string()).await;
        assert!(!changed);

        let m = mgr.lock().await;
        assert_eq!(
            m.get_session(sid).unwrap().windows.get(&wid).unwrap().name,
            "shell"
        );
        drop(m);

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "no broadcast for unknown pane");
    }

    #[tokio::test]
    async fn test_title_update_task_applies_messages_from_channel() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let (tx, rx) = mpsc::channel::<(u32, String)>(8);
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let mgr_clone = mgr.clone();
        let task = tokio::spawn(run_title_update_task(mgr_clone, rx));

        tx.send((pane_id, "first".to_string())).await.unwrap();
        tx.send((pane_id, "first".to_string())).await.unwrap();
        tx.send((pane_id, "second".to_string())).await.unwrap();

        // Expect two broadcasts: "first" and "second"
        let msg1 = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let p1: RenameWindowMsg = msg1.decode_payload().unwrap();
        assert_eq!(p1.name, "first");
        let msg2 = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let p2: RenameWindowMsg = msg2.decode_payload().unwrap();
        assert_eq!(p2.name, "second");

        // Drop sender so task exits.
        drop(tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;

        let m = mgr.lock().await;
        assert_eq!(
            m.get_session(sid).unwrap().windows.get(&wid).unwrap().name,
            "second"
        );
    }

    /// TS-10: after a detach (output_target switched to Detached, title_sender
    /// preserved), a title change still propagates to window.name through the
    /// daemon-level title task. The subsequent Welcome snapshot observes the
    /// updated name.
    #[tokio::test]
    async fn test_detached_pane_title_change_updates_window_name() {
        use crate::mux::ipc::reattach::detach_session_panes;

        let (mgr, sid, wid, pane_id, pane_tx) = setup_single_pane_manager().await;
        let (tx, rx) = mpsc::channel::<(u32, String)>(8);

        // Attach the daemon-level tx to the pane (simulating CLI-created pane).
        {
            let m = mgr.lock().await;
            let session = m.get_session(sid).unwrap();
            let pane = session
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            *pane.title_sender.lock().unwrap() = Some(tx.clone());
        }

        // Simulate GUI disconnect: pass the pane's matching tx so the
        // identity-scoped detach_session_panes actually flips output_target
        // to Detached. The assertion below verifies title_sender is preserved
        // through this state transition.
        detach_session_panes(&mgr, sid, &pane_tx).await;
        {
            let m = mgr.lock().await;
            let session = m.get_session(sid).unwrap();
            let pane = session
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            assert!(
                pane.title_sender.lock().unwrap().is_some(),
                "detach must preserve title_sender"
            );
        }

        // Launch the daemon-level title task and send a title through the
        // pane-side sender to simulate an OSC update while detached.
        let task = tokio::spawn(run_title_update_task(mgr.clone(), rx));
        tx.send((pane_id, "detached-title".to_string()))
            .await
            .unwrap();
        drop(tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;

        // The next Welcome would observe this new name via session_list().
        let m = mgr.lock().await;
        let list = m.session_list();
        let window = list[0].windows.iter().find(|w| w.id == wid).unwrap();
        assert_eq!(window.name, "detached-title");
    }

    /// Build a pane whose `output_target` is `Detached(NetworkDetach)` with a
    /// system origin (`owner = None`), matching the state a pane is left in by
    /// `detach_session_panes` during the connection-reset race (FR6).
    #[cfg(unix)]
    fn make_detached_test_pane(id: u32) -> crate::mux::session::pane::MuxPane {
        use crate::mux::session::pane::{
            DetachReason, MuxPane, PaneOutputTarget, SharedOutputTarget,
        };
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let target: SharedOutputTarget = StdArc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        MuxPane::new_test(id, 80, 24, target)
    }

    /// Build a Connected test pane (the default attached state).
    #[cfg(unix)]
    fn make_connected_test_pane(id: u32) -> crate::mux::session::pane::MuxPane {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget =
            StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        MuxPane::new_test(id, 80, 24, target)
    }

    /// TS-1: detached last-pane reap drives shutdown. One session / window /
    /// pane fed to the reap task; the pane is removed, the session is gone,
    /// the manager is empty, and the watch channel observes `true`.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_task_last_pane_reap_fires_shutdown() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 42u32;
        let (sid, wid) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_detached_test_pane(pane_id));
            (sid, wid)
        };

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let (exit_tx, exit_rx) = mpsc::channel::<u32>(PANE_EXIT_CHANNEL_CAPACITY);
        let task = tokio::spawn(run_pane_exit_task(
            mgr.clone(),
            shutdown_tx.clone(),
            exit_rx,
        ));

        exit_tx.send(pane_id).await.unwrap();
        // Wait for the reap task to fire the shutdown signal.
        shutdown_rx.changed().await.unwrap();
        assert!(*shutdown_rx.borrow(), "shutdown signal must be true");

        let m = mgr.lock().await;
        assert!(m.is_empty(), "manager must be empty after last pane reaped");
        assert!(m.get_session(sid).is_none(), "session must be removed");
        assert!(
            m.get_session(sid)
                .and_then(|s| s.windows.get(&wid))
                .is_none(),
            "window must be removed"
        );
        drop(m);

        drop(exit_tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
    }

    /// TS-2: detached non-last pane reap. Two panes in distinct windows; reap
    /// one and assert only it is removed and the shutdown signal does NOT fire.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_task_non_last_pane_reap_keeps_daemon_alive() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (sid, wid1, wid2) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid1 = m.create_window(sid, "w1".to_string()).unwrap();
            let wid2 = m.create_window(sid, "w2".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid1)
                .unwrap()
                .add_pane(make_detached_test_pane(1));
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid2)
                .unwrap()
                .add_pane(make_detached_test_pane(2));
            (sid, wid1, wid2)
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        handle_destroy_pane(1, &mgr, &shutdown_tx).await;

        // Pane 1 removed, its (now-empty) window removed; pane 2 / its window
        // intact; the session survives; shutdown NOT fired.
        let m = mgr.lock().await;
        assert!(!m.is_empty(), "session must survive a non-last reap");
        let session = m.get_session(sid).expect("session must remain");
        assert!(
            session.windows.get(&wid1).is_none(),
            "emptied window must be pruned"
        );
        let window2 = session.windows.get(&wid2).expect("window 2 must remain");
        assert!(window2.panes.contains_key(&2), "pane 2 must remain");
        drop(m);

        assert!(
            !*shutdown_rx.borrow(),
            "shutdown signal must not fire while a pane remains"
        );
    }

    /// TS-3: connection-reset race (FR6). A pane switched to
    /// `Detached(NetworkDetach)` (as `detach_session_panes` does) must still be
    /// reaped regardless of its `output_target`.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_reap_removes_network_detached_pane() {
        use crate::mux::session::pane::PaneOutputTarget;

        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 7u32;
        let sid = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_detached_test_pane(pane_id));
            // Confirm the precondition: the pane is Detached, not Connected.
            let pane = m
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            assert!(matches!(
                *pane.output_target.lock().unwrap(),
                PaneOutputTarget::Detached { .. }
            ));
            sid
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;

        let m = mgr.lock().await;
        assert!(
            m.get_session(sid).is_none(),
            "the detached pane's session must be reaped despite Detached target"
        );
        assert!(
            m.is_empty(),
            "manager must be empty after reaping last pane"
        );
        drop(m);
        // Last pane gone -> shutdown fires.
        assert!(
            *shutdown_rx.borrow(),
            "shutdown must fire on last pane reap"
        );
    }

    /// TS-4: idempotent reap (FR4). Reaping the same pane id twice — and also
    /// a pane that was already torn down via the Connected empty-chunk path —
    /// is a safe no-op: no panic, and the shutdown signal is not re-fired.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_reap_is_idempotent() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 5u32;
        {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_connected_test_pane(pane_id));
        }

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        // First reap removes the pane and fires shutdown (last pane).
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;
        assert!(shutdown_rx.has_changed().unwrap());
        let _ = shutdown_rx.changed().await;
        assert!(*shutdown_rx.borrow());

        // Second reap of the same (already-removed) pane is a safe no-op:
        // no panic, and the watch channel observes no further change.
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;
        assert!(
            !shutdown_rx.has_changed().unwrap(),
            "double reap must not re-fire the shutdown signal"
        );

        let m = mgr.lock().await;
        assert!(m.is_empty());
    }

    /// TS-11: notify_tx subscription taken before Welcome construction must
    /// capture any RenameWindow emitted between snapshot build and message-
    /// loop entry. Emulate the race: subscribe first, then broadcast, then
    /// verify the subscriber receives the event (i.e. no gap).
    #[tokio::test]
    async fn test_subscribe_before_welcome_catches_rename() {
        let (mgr, _sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        // Phase A: subscribe BEFORE any broadcast (simulates the reordered
        // sequence in handle_connection).
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        // Phase B: build Welcome snapshot (mimics the Welcome frame build).
        let _snapshot_len = { mgr.lock().await.session_list().len() };

        // Phase C: broadcast arrives between snapshot and loop-entry.
        apply_title_change(&mgr, pane_id, "raced-title".to_string()).await;

        // Phase D: subscriber must see it (the race is closed).
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("notify_rx should receive RenameWindow")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::RenameWindow);
        assert_eq!(msg.pane_id, wid);
        let payload: RenameWindowMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.name, "raced-title");
    }

    // ── apply_agent_status_report / sync_agent_status_after_snapshot ─────
    // (SPEC FR3/FR4/FR5, task0003 AC-1/AC-2/AC-4/AC-5)

    /// AC-4: an accepted report updates the pane and broadcasts exactly one
    /// `AgentStatusUpdate` with `replay_derived = false` and the pane's
    /// current public ID.
    #[tokio::test]
    async fn test_apply_agent_status_report_accepted_broadcasts_update() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=working;name=claude".to_string(),
        )
        .await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive AgentStatusUpdate")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        assert_eq!(msg.pane_id, pane_id);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        let expected_public_id = { mgr.lock().await.public_pane_id(pane_id) };
        assert_eq!(payload.public_pane_id, expected_public_id);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Working)
        );
        assert_eq!(payload.name.as_deref(), Some("claude"));
        assert_eq!(payload.revision, 1);
        assert!(!payload.replay_derived);

        // No further message pending (exactly one broadcast).
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(none.is_err(), "exactly one AgentStatusUpdate expected");
    }

    /// AC-2: a rejected sequence leaves state and revision untouched and
    /// broadcasts nothing.
    #[tokio::test]
    async fn test_apply_agent_status_report_rejected_no_broadcast_no_mutation() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=bogus".to_string(),
        )
        .await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "rejected report must not broadcast");

        let m = mgr.lock().await;
        let pane = m
            .get_session(m.find_pane(pane_id).unwrap().0)
            .and_then(|s| s.windows.values().next())
            .and_then(|w| w.panes.get(&pane_id))
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 0);
    }

    /// AC-2: same-state re-report is accepted (revision increments) and
    /// broadcasts again.
    #[tokio::test]
    async fn test_apply_agent_status_report_same_state_re_report_broadcasts_again() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=idle".to_string(),
        )
        .await;
        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=idle".to_string(),
        )
        .await;

        let msg1 = notify_rx.recv().await.unwrap();
        let p1: AgentStatusUpdateMsg = msg1.decode_payload().unwrap();
        let msg2 = notify_rx.recv().await.unwrap();
        let p2: AgentStatusUpdateMsg = msg2.decode_payload().unwrap();
        assert_eq!(p1.revision, 1);
        assert_eq!(p2.revision, 2);
    }

    /// AC-4: an unknown pane_id is a no-op (no broadcast, no panic).
    #[tokio::test]
    async fn test_apply_agent_status_report_unknown_pane_no_broadcast() {
        let (mgr, _sid, _wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            9999,
            "emterm;agent-status;v=1;state=working".to_string(),
        )
        .await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "unknown pane must not broadcast");
    }

    // ── apply_live_osc133_mark (task0003, SPEC FR1/FR2/FR3/FR4) ──────────
    // Integration tests exercising the actual daemon path: an accepted OSC
    // 777 report via `apply_agent_status_report`, then live OSC 133 marks
    // via `apply_live_osc133_mark` — the same path the daemon's PTY reader
    // wiring (`mux::ipc::pty_spawn::pty_reader_loop`) drives in production.

    /// AC-1: `Set` (OSC 777) followed by live OSC 133 `D` then `A` results
    /// in `pane.agent_status` becoming `None`, the pane's revision
    /// incrementing, a registered `WaitAgentState` waiter being
    /// re-evaluated (the SAME `reevaluate_agent_waiters` call the explicit
    /// report path uses — proven here via a stale (closed-receiver) waiter
    /// getting swept, mirroring
    /// `reevaluate_agent_waiters_discards_waiter_with_closed_receiver` in
    /// `mux::ipc::handlers` — a waiter cannot itself match a `None` state,
    /// `check_wait_immediate` short-circuits on it), and an
    /// `AgentStatusUpdate(state: None)` being produced for the GUI.
    #[tokio::test]
    async fn test_apply_live_osc133_mark_set_then_d_then_a_fires_inferred_clear() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=working;name=claude".to_string(),
        )
        .await;

        // Register a waiter with an already-closed receiver so its removal
        // during the inferred-clear's `reevaluate_agent_waiters` call is
        // the observable proof that call happened (see doc comment above).
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            let (tx, rx) = oneshot::channel();
            pane.agent_waiters.lock().unwrap().push(
                crate::mux::session::pane::AgentWaiter {
                    states: vec![crate::agent_status::AgentState::Idle],
                    after_revision: None,
                    responder: Some(tx),
                },
            );
            drop(rx); // simulate a disconnected waiting client
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        // Live `D`: command ended, but no clear yet (still armed only).
        apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::CommandEnd).await;
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(none.is_err(), "a lone D must not broadcast a clear");

        // Live `A`: completes the D→A transition, fires the inferred clear.
        apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::PromptStart).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive AgentStatusUpdate")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.state, None);
        assert_eq!(payload.revision, 2);
        assert!(!payload.replay_derived);

        let m = mgr.lock().await;
        let pane = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pane_id)
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 2);
        assert!(
            pane.agent_waiters.lock().unwrap().is_empty(),
            "the closed-receiver waiter must be swept by reevaluate_agent_waiters"
        );
    }

    /// AC-2: `Set` followed only by live OSC 133 `A` (no `D`) leaves
    /// `pane.agent_status` unchanged — no clear, no broadcast, no revision
    /// bump.
    #[tokio::test]
    async fn test_apply_live_osc133_mark_a_without_prior_d_leaves_state_unchanged() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=blocked".to_string(),
        )
        .await;

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::PromptStart).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "an A with no prior D must not broadcast");

        let m = mgr.lock().await;
        let pane = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pane_id)
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(
            status.state,
            Some(crate::agent_status::AgentState::Blocked)
        );
        assert_eq!(status.revision, 1, "revision must not bump");
    }

    /// AC-3: an explicit `Clear` followed by live `D`/`A` does not produce
    /// a second/duplicate clear application or a second revision
    /// increment — the latch is already disarmed by the explicit `Clear`.
    #[tokio::test]
    async fn test_apply_live_osc133_mark_after_explicit_clear_no_duplicate_clear() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=done".to_string(),
        )
        .await;
        apply_agent_status_report(&mgr, pane_id, "emterm;agent-status;clear".to_string()).await;

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::CommandEnd).await;
        apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::PromptStart).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            timeout.is_err(),
            "D/A after an explicit Clear must not broadcast a second clear"
        );

        let m = mgr.lock().await;
        let pane = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pane_id)
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(
            status.revision, 2,
            "revision must stay at the explicit Clear's value (Set=1, Clear=2)"
        );
    }

    /// AC-6 (NFR3 regression guard): a pane whose shell never emits OSC 133
    /// (no `apply_live_osc133_mark` call ever happens) behaves exactly as
    /// before this feature — an unresolved `Set` with no explicit `Clear`
    /// leaves the icon (state) showing indefinitely.
    #[tokio::test]
    async fn test_pane_without_osc133_marks_keeps_set_state_indefinitely() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=working;name=claude".to_string(),
        )
        .await;

        let m = mgr.lock().await;
        let pane = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pane_id)
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(
            status.state,
            Some(crate::agent_status::AgentState::Working)
        );
        assert_eq!(status.revision, 1);
    }

    /// AC-5: after a snapshot, each stateful pane produces one
    /// `AgentStatusUpdate` with `replay_derived = true`; a stateless pane
    /// produces none.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_only_stateful_panes() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (sid, stateful_id, stateless_id) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            let (stateful_pane, _tx1) = make_title_test_pane(1);
            let (stateless_pane, _tx2) = make_title_test_pane(2);
            stateful_pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Blocked,
                name: Some("agent".to_string()),
            });
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(stateful_pane);
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(stateless_pane);
            (sid, 1u32, 2u32)
        };

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_snapshot(&mgr, sid).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive one AgentStatusUpdate")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, stateful_id);
        assert!(payload.replay_derived);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Blocked)
        );

        // Nothing further: the stateless pane produces no message.
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            none.is_err(),
            "stateless pane {} must not produce a message",
            stateless_id
        );
    }

    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_unknown_session_no_panic() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        // Should not panic on an unknown session id.
        sync_agent_status_after_snapshot(&mgr, 9999).await;
    }

    /// task0013 AC-1 (rework, review round 1 `replay_clear_lost`): a pane
    /// that transitioned blocked -> cleared (revision now 2, state now
    /// None) while the GUI was detached must still produce a
    /// replay-derived `AgentStatusUpdate` with `state: None` on reattach,
    /// so the stale badge/summary from before the clear is replaced.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_cleared_pane_emits_state_none() {
        let (mgr, sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Blocked,
                name: Some("agent".to_string()),
            });
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Clear);
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_snapshot(&mgr, sid).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive a replay-derived AgentStatusUpdate for the cleared pane")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(payload.state, None, "cleared pane must sync as state=None");
        assert_eq!(payload.name, None);
        assert_eq!(payload.revision, 2);
    }

    /// task0013 AC-2: a pane that has never reported any state (revision
    /// still 0) must not produce a sync message on reattach — no
    /// unnecessary state=None update for a pane that never had state.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_never_reported_pane_no_broadcast() {
        let (mgr, sid, _wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        sync_agent_status_after_snapshot(&mgr, sid).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            timeout.is_err(),
            "a pane that never reported must not produce a sync message"
        );
    }

    /// AC-5 (per-pane / window-switch counterpart): a stateful pane
    /// produces one `AgentStatusUpdate` with `replay_derived = true`.
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_stateful_pane_broadcasts() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Done,
                name: None,
            });
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive AgentStatusUpdate")
            .unwrap();
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Done)
        );
    }

    /// AC-5: a stateless (never-reported, revision == 0) pane produces no
    /// message (task0013 AC-2).
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_stateless_pane_no_broadcast() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "stateless pane must not broadcast");
    }

    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_unknown_pane_no_panic() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        sync_agent_status_after_pane_snapshot(&mgr, 9999).await;
    }

    /// task0013 AC-1 (per-pane / window-switch counterpart): a pane that
    /// transitioned blocked -> cleared while the GUI was detached must
    /// still produce a replay-derived `AgentStatusUpdate` with
    /// `state: None` on the per-pane snapshot sync path.
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_cleared_pane_emits_state_none() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Working,
                name: Some("agent".to_string()),
            });
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Clear);
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive a replay-derived AgentStatusUpdate for the cleared pane")
            .unwrap();
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(payload.state, None, "cleared pane must sync as state=None");
        assert_eq!(payload.revision, 2);
    }

    // ---- mux-daemon-hot-upgrade task0009 (rework): upgrade preparation,
    // now wired to the REAL snapshot (AC-1..AC-7) ----
    //
    // `prepare_upgrade` is parameterized over the probe and snapshot
    // operations (Test Notes: "a substitutable probe") so every branch is
    // testable without a real candidate binary; production always passes
    // `probe_candidate_handoff_range` / `real_snapshot` (exercised directly
    // by the dedicated tests further below).

    // task0004 (agent-exit-after-icon, SPEC FR6): the handoff schema
    // version was bumped from 1 to 2 to carry the new inferred-clear
    // latch fields (`crates/mux_ipc/src/handoff.rs`). This stand-in
    // reports the CURRENT schema range so it stays a valid "the candidate
    // is compatible" probe regardless of future version bumps, instead of
    // hardcoding a version number that has no bearing on what any of
    // these tests are actually exercising.
    #[cfg(unix)]
    fn ok_probe(_candidate: &Path) -> Result<std::ops::RangeInclusive<u32>, String> {
        Ok(mux_ipc::handoff::SUPPORTED_HANDOFF_SCHEMA_VERSIONS)
    }

    #[cfg(unix)]
    fn incompatible_probe(_candidate: &Path) -> Result<std::ops::RangeInclusive<u32>, String> {
        Ok(99..=100)
    }

    #[cfg(unix)]
    fn ok_snapshot(
        manager: &SessionManager,
        _listen_fd: RawFd,
        _socket_path: &Path,
    ) -> Result<mux_ipc::handoff::HandoffDocument, String> {
        Ok(mux_ipc::handoff::HandoffDocument {
            schema_version: mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            incarnation: manager.incarnation().to_string(),
            listen_fd: 0,
            next_session_id: manager.next_session_id_counter(),
            next_pane_id: manager.next_pane_id_counter(),
            sessions: Vec::new(),
        })
    }

    #[cfg(unix)]
    fn failing_snapshot(
        _manager: &SessionManager,
        _listen_fd: RawFd,
        _socket_path: &Path,
    ) -> Result<mux_ipc::handoff::HandoffDocument, String> {
        Err("disk full".to_string())
    }

    #[cfg(unix)]
    fn no_ack_slot() -> SharedUpgradeAckSlot {
        Arc::new(StdMutex::new(None))
    }

    /// AC-1: an upgrade request runs the upgrade branch, not the shutdown
    /// branch -- observed here as "no pane is marked exited" and (by
    /// construction: `prepare_upgrade`'s source never references
    /// `graceful_shutdown` or `MuxPane::mark_exited`) "the pane-killing
    /// shutdown helper is not invoked". Uses `setup_single_pane_manager`'s
    /// live (non-exited) pane -- exactly the case round 1's placeholder
    /// unconditionally refused.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_does_not_mark_any_pane_exited() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let ack_slot = no_ack_slot();

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            dir.path(),
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            ok_snapshot,
        )
        .await;
        assert!(
            result.is_ok(),
            "AC-1: an upgrade with a live pane must proceed, not be refused: {result:?}"
        );

        let m = mgr.lock().await;
        let pane = m
            .get_session(sid)
            .and_then(|s| s.windows.get(&wid))
            .and_then(|w| w.panes.get(&pane_id))
            .unwrap();
        assert!(!pane.exited, "AC-1: upgrade preparation must not exit panes");
    }

    /// AC-1: with a REAL PTY-backed live pane (not the output-target-only
    /// `make_title_test_pane` fixture), `prepare_upgrade` wired to the real
    /// `crate::mux::upgrade::snapshot` produces a handoff document recording
    /// that pane's real master descriptor, id and the manager's incarnation
    /// token -- proving this is genuinely connected, not a placeholder.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_with_a_real_live_pane_records_its_descriptor_and_incarnation() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex2};

        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();

        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (pane_id, incarnation) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            let pane_id = m.alloc_pane_id();
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex2::new(PaneOutputTarget::Connected(tx)));
            let pane = MuxPane::new(pane_id, 80, 24, target, writer, pair.master, None);
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
            (pane_id, m.incarnation().to_string())
        };

        let dir = tempfile::tempdir().unwrap();
        // `real_snapshot` derives the handoff path via
        // `crate::mux::upgrade::handoff_file_path`, which replaces the
        // LAST component of its argument -- so this must be a socket-FILE
        // path (like production's `sock_path`), not a bare directory.
        let sock_path = dir.path().join("mux-default.sock");
        let ack_slot = no_ack_slot();

        let outcome = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            &sock_path,
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            real_snapshot,
        )
        .await
        .expect("AC-1: an upgrade with a live pane must proceed, not be refused");

        let doc =
            crate::mux::upgrade::read_and_remove_handoff_file(&outcome.handoff_document_path)
                .expect("the real snapshot must have written a decodable handoff document");
        assert_eq!(doc.incarnation, incarnation, "AC-1: incarnation token recorded");
        assert_eq!(doc.sessions.len(), 1);
        let recorded_pane = &doc.sessions[0].windows[0].panes[0];
        assert_eq!(recorded_pane.id, pane_id);
        assert!(
            recorded_pane.master_fd.is_some(),
            "AC-1: the live pane's real master descriptor must be recorded"
        );
    }

    /// AC-2: the upgrade branch does not remove the socket file.
    /// `prepare_upgrade` never receives or references a socket path at all
    /// (only `listen_fd`, a bare integer, and `socket_path` for the handoff
    /// file's own naming); this test pins that a file representing the
    /// socket, sitting right next to the handoff directory, survives
    /// untouched.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_does_not_remove_socket_file() {
        let (mgr, ..) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("mux-default.sock");
        std::fs::write(&sock_path, b"pretend socket").unwrap();
        let ack_slot = no_ack_slot();

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            dir.path(),
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            ok_snapshot,
        )
        .await;
        assert!(result.is_ok());
        assert!(
            sock_path.exists(),
            "AC-2: the socket file must never be removed on the upgrade path"
        );
    }

    /// AC-3: an incompatible schema range aborts the upgrade, leaves no
    /// handoff file, and (since `prepare_upgrade` simply returns `Err`
    /// rather than panicking or exiting) the accept loop that called it is
    /// free to continue its `select!` unchanged.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_aborts_on_incompatible_schema_range() {
        let (mgr, ..) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let ack_slot = no_ack_slot();

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            dir.path(),
            1,
            &ack_slot,
            incompatible_probe,
            ok_snapshot,
        )
        .await;
        assert!(result.is_err());
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(
            entries.is_empty(),
            "AC-3: no handoff file may remain after an incompatible-schema abort"
        );
    }

    /// AC-4: a snapshot failure aborts the upgrade, reports the reason
    /// (via the `Err` the requesting connection forwards to the client --
    /// see `ipc::connection::upgrade_reply_to_message`'s test coverage for
    /// that half), and leaves no handoff file.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_aborts_on_snapshot_failure_and_leaves_no_handoff_file() {
        let (mgr, ..) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let ack_slot = no_ack_slot();

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            dir.path(),
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            failing_snapshot,
        )
        .await;
        match result {
            Err(reason) => assert!(reason.contains("disk full")),
            Ok(_) => panic!("expected snapshot failure to abort the upgrade"),
        }
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(
            entries.is_empty(),
            "AC-4: no handoff file may remain after a snapshot-failure abort"
        );
    }

    /// AC-5 (queueing half): on successful preparation, the `Upgrading`
    /// announcement is broadcast before the outcome is returned to the
    /// caller -- a subscriber created beforehand already has the message
    /// queued by the time `prepare_upgrade` resolves. Full observed-delivery
    /// (AC-7) is proven separately below against a REAL connected client.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_broadcasts_upgrading_before_returning_outcome() {
        let (mgr, ..) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        let ack_slot = no_ack_slot();

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            dir.path(),
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            ok_snapshot,
        )
        .await;
        assert!(result.is_ok());

        let msg = notify_rx
            .try_recv()
            .expect("Upgrading broadcast must already be queued once prepare_upgrade returns");
        assert_eq!(msg.msg_type, MessageType::Upgrading);
    }

    /// AC-7: the `Upgrading` announcement is OBSERVABLY delivered to a REAL
    /// connected GUI client's own socket before `prepare_upgrade` returns --
    /// proven by reading it back from the client side (Test Notes: "a
    /// stand-in client that reads from its socket, not a subscriber that
    /// polls the queue"), through the production connection-handling code
    /// (`ipc::connection::handle_connection`), not a hand-rolled stand-in.
    ///
    /// A second, never-drained subscription stands in for the CLI connection
    /// that issues a real `Upgrade` request (its own subscription, taken
    /// unconditionally before the CLI/GUI branch in `handle_connection`,
    /// never acks -- see `prepare_upgrade`'s doc comment): this makes
    /// `expected_acks` resolve to exactly 1 (the real GUI client), so the
    /// wait genuinely gates on that one real delivery rather than being
    /// skipped, and the read below is deterministic, not a timing race.
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_delivers_upgrading_to_a_real_connected_client_before_returning() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let dir = tempfile::tempdir().unwrap();
        // See the sibling AC-1 test above: `real_snapshot` needs a
        // socket-FILE path, not a bare directory.
        let sock_path = dir.path().join("mux-default.sock");
        let ack_slot = no_ack_slot();

        let _cli_phantom_subscription = { mgr.lock().await.notify_tx().subscribe() };

        let (server_stream, mut client_stream) = tokio::net::UnixStream::pair().unwrap();
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
        let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

        let conn_task = tokio::spawn(crate::mux::ipc::connection::handle_connection(
            server_stream,
            mgr.clone(),
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            ack_slot.clone(),
        ));

        write_frame_async(
            &mut client_stream,
            &MuxMessage::control(
                MessageType::Hello,
                0,
                &HelloMsg {
                    client_type: ClientType::Gui,
                    protocol_version: PROTOCOL_VERSION,
                },
            ),
        )
        .await;
        let welcome = read_frame_async(&mut client_stream).await;
        assert_eq!(welcome.msg_type, MessageType::Welcome);

        let result = prepare_upgrade(
            &mgr,
            0,
            Path::new("/bin/true"),
            vec!["mux".to_string(), "--daemon".to_string()],
            &sock_path,
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            real_snapshot,
        )
        .await;
        assert!(result.is_ok(), "preparation should succeed: {result:?}");

        // AC-7: read directly off the client's own socket AFTER
        // `prepare_upgrade` already returned. A short timeout here is only a
        // safety net against a genuine hang -- the frame must already be
        // sitting in the socket buffer by construction (the ack wait above
        // blocked on exactly this write completing).
        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            read_frame_async(&mut client_stream),
        )
        .await
        .expect("the Upgrading frame must already be readable off the client socket");
        assert_eq!(frame.msg_type, MessageType::Upgrading);

        conn_task.abort();
    }

    /// AC-6: the returned run outcome carries the target binary path,
    /// argument vector, environment addition (naming [`HANDOFF_ENV_VAR`]),
    /// and handoff file path (derived from `crate::mux::upgrade::handoff_file_path`,
    /// the single owner of that naming).
    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_upgrade_outcome_carries_target_args_env_and_handoff_path() {
        let (mgr, ..) = setup_single_pane_manager().await;
        let dir = tempfile::tempdir().unwrap();
        let candidate = Path::new("/bin/true");
        let args = vec!["mux".to_string(), "--daemon".to_string()];
        let ack_slot = no_ack_slot();

        let outcome = prepare_upgrade(
            &mgr,
            0,
            candidate,
            args.clone(),
            dir.path(),
            mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            &ack_slot,
            ok_probe,
            ok_snapshot,
        )
        .await
        .expect("preparation should succeed");

        assert_eq!(outcome.target, candidate);
        assert_eq!(outcome.args, args);
        assert_eq!(outcome.env_addition.0, HANDOFF_ENV_VAR);
        assert_eq!(
            PathBuf::from(&outcome.env_addition.1),
            outcome.handoff_document_path
        );
        assert_eq!(
            outcome.handoff_document_path,
            crate::mux::upgrade::handoff_file_path(dir.path()),
            "AC-9 (single ownership): the handoff path must come from \
             crate::mux::upgrade's single naming authority"
        );
    }

    // ---- mux-daemon-hot-upgrade task0009 (rework): handoff-mode startup,
    // now wired to the REAL restore (AC-5, AC-7..AC-9) ----

    /// Bind a real Unix listener at `path` and hand back its raw fd, taking
    /// ownership (mirrors what a real snapshot step would do after clearing
    /// close-on-exec: the descriptor crosses into `start_from_handoff`
    /// exactly once).
    #[cfg(unix)]
    fn bind_listener_raw_fd(path: &Path) -> RawFd {
        let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
        listener.into_raw_fd()
    }

    /// Build a minimal but real `HandoffDocument` recording `listen_fd` and
    /// an empty session list, and write it (bincode-encoded) to `path`.
    #[cfg(unix)]
    fn write_test_handoff_document(path: &Path, listen_fd: RawFd) {
        let doc = mux_ipc::handoff::HandoffDocument {
            schema_version: mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
            incarnation: "deadbeef".to_string(),
            listen_fd: listen_fd as i32,
            next_session_id: 1,
            next_pane_id: 1,
            sessions: Vec::new(),
        };
        let bytes = mux_ipc::handoff::encode_handoff_document(&doc);
        std::fs::write(path, bytes).unwrap();
    }

    /// A `TitleChangeSender` / `NotificationSender` / `AgentStatusReportSender`
    /// / `SharedPaneExitSender` quadruple, matching what `run_daemon` creates
    /// before calling `startup` (task0009 rework: restore needs them to
    /// re-wire a restored pane's reader thread).
    #[cfg(unix)]
    fn test_daemon_channels() -> (
        TitleChangeSender,
        NotificationSender,
        AgentStatusReportSender,
        SharedPaneExitSender,
    ) {
        let (title_tx, _title_rx) = mpsc::channel(16);
        let (notification_tx, _notification_rx) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
        (title_tx, notification_tx, agent_status_tx, pane_exit_sender)
    }

    /// AC-5/AC-7/AC-8: with a valid handoff document recorded over a real
    /// listener, handoff start skips bind entirely, adopts the SAME listener
    /// (proven by accepting a real connection through the returned handle),
    /// removes the handoff file, and restores a real `SessionManager` (via
    /// `crate::mux::upgrade::restore`) whose incarnation matches the
    /// document's.
    #[cfg(unix)]
    #[tokio::test]
    async fn start_from_handoff_adopts_listener_and_restores_the_real_session_manager() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("adopted.sock");
        let handoff_path = dir.path().join("handoff.state");
        let fd = bind_listener_raw_fd(&sock_path);
        write_test_handoff_document(&handoff_path, fd);
        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_daemon_channels();

        let (listener, manager, counts) = start_from_handoff(
            &handoff_path,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        )
        .expect("handoff start should succeed");
        assert_eq!(manager.incarnation(), "deadbeef");
        assert_eq!(counts.pane_count, 0);
        assert_eq!(counts.descriptor_count, 1, "the listener always counts as one descriptor");
        assert!(
            !handoff_path.exists(),
            "AC-7/AC-8: the handoff file must be removed"
        );

        // Prove real adoption: a client connecting to the ORIGINAL socket
        // path is accepted through the returned (adopted, not freshly
        // bound) listener handle.
        let accept = tokio::spawn(async move { listener.accept().await });
        let _client = tokio::net::UnixStream::connect(&sock_path)
            .await
            .expect("adopted listener must still accept connections at the original path");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(2), accept)
            .await
            .expect("accept must complete promptly")
            .expect("accept task must not panic");
        assert!(accepted.is_ok());
    }

    /// A missing/unreadable handoff file fails `start_from_handoff` outright
    /// (there is no listener to adopt), so the caller can fall back to a
    /// fresh bind.
    #[cfg(unix)]
    #[tokio::test]
    async fn start_from_handoff_missing_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("does-not-exist.state");
        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_daemon_channels();

        let result = start_from_handoff(
            &handoff_path,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );
        assert!(result.is_err());
    }

    /// AC-6: adopting a recorded listen descriptor that is not a live
    /// listening Unix socket fails start_from_handoff outright (the caller
    /// falls back to a fresh bind) rather than adopting a wild descriptor.
    #[cfg(unix)]
    #[tokio::test]
    async fn start_from_handoff_rejects_a_non_listening_recorded_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff-bogus-listener.state");
        // An ordinary regular file's fd is not a socket at all.
        let file = tempfile::tempfile().unwrap();
        write_test_handoff_document(&handoff_path, file.as_raw_fd());
        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_daemon_channels();

        let result = start_from_handoff(
            &handoff_path,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );
        assert!(
            result.is_err(),
            "AC-6: a non-socket recorded listen descriptor must not adopt"
        );
    }

    /// Serializes the two tests below that mutate the process-wide
    /// [`HANDOFF_ENV_VAR`] (task0011 rework). `cargo test`'s default
    /// parallel execution runs every `#[test]` fn on its own thread with no
    /// isolation between them, and an environment variable is shared by
    /// every thread in the process — so without this lock, one test's
    /// `set_var` can land between another, CONCURRENTLY running test's own
    /// `var_os`/`remove_var` and its `startup()` call. That other test then
    /// reads back the WRONG (this test's) handoff path and adopts THIS
    /// test's listen descriptor too: two independent `UnixListener`s now
    /// believe they own the same descriptor number, and whichever drops
    /// first closes it out from under the other — aborting the whole
    /// process ("IO Safety violation: owned file descriptor already
    /// closed") once the survivor's own drop later finds its descriptor
    /// already gone. Reproduced and root-caused via `strace -f`, correlating
    /// the fatal `fcntl(fd, F_GETFD) = -1 EBADF` against two threads
    /// racing to open the identical (shared, not merely
    /// coincidentally-alike) handoff document path.
    static HANDOFF_ENV_VAR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// AC-8: with the handoff environment variable absent, `startup`
    /// behaves exactly as today -- binds the socket, reports no handoff
    /// counts.
    #[cfg(unix)]
    #[tokio::test]
    async fn startup_without_handoff_env_var_binds_fresh() {
        // Held for the whole test (see `HANDOFF_ENV_VAR_TEST_LOCK`) so this
        // test's env var window never overlaps
        // `startup_with_handoff_env_var_adopts_listener_and_clears_env_var`'s.
        let _env_guard = HANDOFF_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: env mutation is process-wide; saved/restored around this
        // test (matches the existing project convention, e.g.
        // `render::font::user_dir`'s tests).
        let prev = std::env::var_os(HANDOFF_ENV_VAR);
        unsafe {
            std::env::remove_var(HANDOFF_ENV_VAR);
        }

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("fresh.sock");
        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_daemon_channels();
        let result = startup(
            &sock_path,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );

        unsafe {
            match prev {
                Some(v) => std::env::set_var(HANDOFF_ENV_VAR, v),
                None => std::env::remove_var(HANDOFF_ENV_VAR),
            }
        }

        let (_listener, _manager, counts) = result.expect("normal startup must succeed");
        assert!(counts.is_none(), "AC-8: no handoff logging on normal start");
        assert!(sock_path.exists());
    }

    /// AC-7/AC-9: with the handoff environment variable set to a valid
    /// document, `startup` skips bind, adopts the recorded listener,
    /// reports handoff counts, and -- critically -- clears the environment
    /// variable before returning, so a pane child spawned afterward by the
    /// caller never inherits it.
    ///
    /// "Skips bind" is proven positively, not just by absence of an error:
    /// `sock_path` already has a real listener bound at it (the fixture
    /// setup below) before `startup` runs, so a `UnixListener::bind` at the
    /// same path would fail with "address in use". `startup` succeeding
    /// AND the returned listener still accepting connections at that same
    /// path together show it adopted the existing listener rather than
    /// attempting (and silently swallowing a failure on) a fresh bind.
    #[cfg(unix)]
    #[tokio::test]
    async fn startup_with_handoff_env_var_adopts_listener_and_clears_env_var() {
        // Held for the whole test (see `HANDOFF_ENV_VAR_TEST_LOCK`) so this
        // test's env var window never overlaps
        // `startup_without_handoff_env_var_binds_fresh`'s.
        let _env_guard = HANDOFF_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("handoff-adopted.sock");
        let handoff_path = dir.path().join("handoff3.state");
        let fd = bind_listener_raw_fd(&sock_path);
        write_test_handoff_document(&handoff_path, fd);
        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_daemon_channels();

        let prev = std::env::var_os(HANDOFF_ENV_VAR);
        // SAFETY: env mutation is process-wide; saved/restored around this
        // test (matches the existing project convention).
        unsafe {
            std::env::set_var(HANDOFF_ENV_VAR, &handoff_path);
        }

        let result = startup(
            &sock_path,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );

        // AC-9: cleared regardless of what the caller does next -- checked
        // BEFORE restoring `prev`, so a leftover value would fail this
        // assertion rather than being silently masked by the restore below.
        let cleared = std::env::var(HANDOFF_ENV_VAR).is_err();

        unsafe {
            match prev {
                Some(v) => std::env::set_var(HANDOFF_ENV_VAR, v),
                None => std::env::remove_var(HANDOFF_ENV_VAR),
            }
        }

        let (listener, _manager, counts) = result.expect("handoff startup must succeed");
        assert!(counts.is_some(), "AC-7: handoff start must report counts");
        assert!(!handoff_path.exists());
        assert!(cleared, "AC-9: handoff env var must be cleared");

        // Positive proof of adoption (see doc comment): the returned
        // listener still accepts a connection at the ORIGINAL bind path.
        let accept = tokio::spawn(async move { listener.accept().await });
        let _client = tokio::net::UnixStream::connect(&sock_path)
            .await
            .expect("adopted listener must still accept connections at the original path");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(2), accept)
            .await
            .expect("accept must complete promptly")
            .expect("accept task must not panic");
        assert!(accepted.is_ok());
    }

    // ---- small async frame helpers for AC-7's real-connection test ----

    #[cfg(unix)]
    async fn write_frame_async<S: tokio::io::AsyncWrite + Unpin>(stream: &mut S, msg: &MuxMessage) {
        use tokio::io::AsyncWriteExt;
        let body = msg.to_frame_body();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .await
            .expect("write frame length");
        stream.write_all(&body).await.expect("write frame body");
        stream.flush().await.expect("flush");
    }

    #[cfg(unix)]
    async fn read_frame_async<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> MuxMessage {
        use tokio::io::AsyncReadExt;
        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .await
            .expect("read frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        stream
            .read_exact(&mut frame_buf)
            .await
            .expect("read frame body");
        MuxMessage::from_frame_body(&frame_buf).expect("valid frame")
    }
}
