//! Client-side daemon control: the ensure-running bootstrap, shutdown and
//! upgrade requests over the socket, reachability polling, legacy-daemon
//! recovery, and upgrade admission bookkeeping.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mux_ipc::protocol::{
    ErrorMsg, MAX_FRAME_LENGTH, MessageType, MuxMessage, PREVIOUS_PROTOCOL_VERSION,
    PROTOCOL_VERSION, WelcomeMsg, parse_rejected_server_version,
};

use super::connect::{
    cleanup_stale_socket, connect_daemon, handshake_with_version, is_daemon_running, socket_path,
    spawn_daemon,
};
#[cfg(unix)]
use crate::mux::identity;

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
                    match handshake_with_version(
                        &mut legacy_for_shutdown,
                        PREVIOUS_PROTOCOL_VERSION,
                    ) {
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
                    match handshake_with_version(
                        &mut legacy_for_shutdown,
                        PREVIOUS_PROTOCOL_VERSION,
                    ) {
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
                // task0004 (NFR1, "Trigger-side warning suppression"): a
                // reason carrying the pinned suppression marker (produced
                // only for a repeat refusal of the SAME candidate the daemon
                // already refused once) emits no user-facing line -- the
                // first refusal was already visible; the repeat is silent.
                // Any other reason behaves exactly as before.
                UpgradeResponse::Rejected(reason)
                    if reason.starts_with(UPGRADE_SUPPRESSED_MARKER) =>
                {
                    log::debug!(
                        "automatic binary-update upgrade request suppressed by the daemon \
                         (repeat refusal of an already-refused candidate): {reason}"
                    );
                    Ok(LegacyRecovery::Compatible)
                }
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
                        // task0005 (SPEC FR2/AC-6/AC-8): reachability alone
                        // cannot distinguish a genuinely replaced daemon
                        // from the original one that refused or ignored the
                        // upgrade and kept serving -- re-check through the
                        // SAME injected identity-check provider used for the
                        // firing decision. Only an Unchanged post-fire
                        // verdict is positive proof of replacement (the
                        // answering daemon has already re-recorded its own
                        // identity per D4's startup ordering); Updated or
                        // Undecidable means the replacement could not be
                        // confirmed, so the success notice must not be
                        // emitted.
                        match identity_check(sock_path) {
                            identity::Verdict::Unchanged => {
                                const NOTICE: &str =
                                    "Mux daemon upgraded in place to the newly installed binary";
                                message(NOTICE);
                                log::warn!("{NOTICE}");
                            }
                            identity::Verdict::Updated(_) | identity::Verdict::Undecidable => {
                                const UNCONFIRMED_WARNING: &str = "Warning: mux daemon is reachable but the binary replacement could not be confirmed; continuing with the existing daemon";
                                message(UNCONFIRMED_WARNING);
                                log::warn!("{UNCONFIRMED_WARNING}");
                            }
                        }
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

// ============================================================================
// task0004 (mux-daemon-binary-update-detect, NFR1/NFR3): upgrade-candidate
// validation call sites and repeat-refusal suppression, daemon-side
// (`run_daemon`'s upgrade-signal branch, `admit_upgrade_candidate` below) and
// the trigger-side marker consumer (`trigger_binary_update_if_detected`
// above, already updated).
// ============================================================================

/// Run-loop-scoped repeat-refusal suppression state (Design "Repeat-refusal
/// suppression", NFR1): the most recently refused candidate's `(device,
/// inode)` plus the refusal reason it produced. In-memory only -- owned by a
/// local in `run_daemon`, so a daemon restart naturally clears it.
#[cfg(unix)]
pub(super) type RefusedCandidate = ((u64, u64), String, RefusalStage);

/// Which stage of [`admit_upgrade_candidate`] produced a recorded refusal
/// (sid-validate-failure-suppression-regression fix): a `validate` failure
/// and a POST-probe failure ([`record_post_probe_refusal`]) are suppressed
/// independently. A `Validation`-stage record only suppresses a REPEATED
/// `validate` failure for the same `(device, inode)`; it is ignored once
/// that candidate passes `validate` (e.g. after an operator `chmod`-fixes a
/// world-writable candidate), so a since-fixed candidate is never
/// incorrectly blocked. A `PostProbe`-stage record only suppresses the
/// post-probe re-check, matching the pre-existing behavior.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RefusalStage {
    /// Recorded when `validate` itself rejected the candidate.
    Validation,
    /// Recorded by [`record_post_probe_refusal`]: a probe spawn failure,
    /// probe timeout, or schema-range gate rejection AFTER `validate`
    /// already passed.
    PostProbe,
}

/// Marker prefix pinning the suppressed-repeat rejection reason (Design
/// "Pinned suppression reason"): a data contract shared between this
/// module's daemon-side producer ([`suppression_reason`], used by
/// [`admit_upgrade_candidate`]) and the trigger-side consumer
/// (`trigger_binary_update_if_detected`'s rejected-reply arm, above).
#[cfg(unix)]
pub(super) const UPGRADE_SUPPRESSED_MARKER: &str = "upgrade-suppressed: ";

/// Build the pinned suppression-marker rejection reason (Design "Pinned
/// suppression reason"): the exact marker, the ORIGINAL refusal reason, and
/// the recovery hint that installing a new binary or restarting the daemon
/// re-enables the attempt.
#[cfg(unix)]
fn suppression_reason(original_reason: &str) -> String {
    format!(
        "{UPGRADE_SUPPRESSED_MARKER}{original_reason} (install a new binary or restart the \
         daemon to re-enable the attempt)"
    )
}

/// Outcome of [`admit_upgrade_candidate`] (Design "Repeat-refusal
/// suppression" + "Candidate validation"): whether the upgrade-signal branch
/// may proceed to the compatibility probe.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum UpgradeAdmission {
    /// Proceed to the compatibility probe. Carries the candidate's captured
    /// `(device, inode)` (if any), so the caller can record a POST-probe
    /// refusal (a probe spawn failure, timeout, or schema-range rejection)
    /// keyed on the SAME identity without re-stating it.
    Admitted { candidate_id: Option<(u64, u64)> },
    /// Refuse immediately (repeat-suppressed OR validation-failed) with this
    /// reply reason; no probe is spawned, no snapshot is taken.
    Blocked(String),
}

/// The upgrade-signal branch's "suppress -> validate" sequencing (Test
/// Notes: "extract the branch's 'validate -> suppress -> probe' sequencing
/// into a parameterized helper... injecting the probe function and the
/// refusal sink" -- the probe call itself stays in the caller since
/// [`prepare_upgrade`] is already independently parameterized over it; this
/// helper owns everything BEFORE that call). `capture_id` / `validate` are
/// injected so this is unit-testable without a real candidate binary or a
/// real daemon uid (AC-2 unit half, AC-4, AC-5).
///
/// Mutates `last_refused`, tagging every record with a [`RefusalStage`] so a
/// `validate` failure and a POST-probe failure ([`record_post_probe_refusal`])
/// are suppressed independently
/// (sid-validate-failure-suppression-regression): a `validate` failure
/// records a `Validation`-stage entry so a REPEAT of the exact same failure
/// for the same `(device, inode)` is suppressed (NFR1), but that record is
/// ignored once the candidate passes `validate` (e.g. an operator ran
/// `chmod` on a world-writable candidate without changing its identity) --
/// it never masks an admission. Cleared on anything other than a matching
/// repeat (Design: "If it differs, or the capture fails -> clear the
/// state").
#[cfg(unix)]
pub(super) fn admit_upgrade_candidate(
    candidate: &Path,
    last_refused: &mut Option<RefusedCandidate>,
    capture_id: impl Fn(&Path) -> Option<(u64, u64)>,
    validate: impl Fn(&Path) -> Result<(), String>,
) -> UpgradeAdmission {
    let candidate_id = capture_id(candidate);

    // sid-suppression-affects-explicit-upgrade-and-key-too-coarse: the
    // repeat-refusal suppression check now runs AFTER `validate`, not
    // before it, so a candidate that passes validation is always admitted
    // -- even when its (device, inode) still matches the last-refused
    // candidate (e.g. an operator fixed a world-writable candidate with
    // `chmod` without changing the file's identity). Suppression is thus
    // reserved for repeats of a POST-probe refusal (`record_post_probe_refusal`,
    // recorded once a candidate has already cleared `validate`), never for
    // masking a candidate that has since become valid.
    //
    // TODO: distinguish explicit `emterm mux upgrade` requests from
    // automatic trigger-detected ones (e.g. an `origin` field on
    // `UpgradeSignal`) so suppression -- which exists only to quiet a
    // repeatedly re-firing automatic trigger -- never applies to an
    // explicit user command at all; that is a wire-protocol change out of
    // scope here.
    if let Err(reason) = validate(candidate) {
        // sid-validate-failure-suppression-regression: a repeat `validate`
        // failure for the SAME (device, inode), where the last recorded
        // refusal was ALSO a `Validation`-stage one, is still suppressed
        // (NFR1) -- a permanently invalid candidate (e.g. distributed
        // world-writable) must not re-warn on every single signal. A
        // `PostProbe`-stage record never suppresses a `validate` failure
        // (it is a different failure mode), and any OTHER candidate always
        // gets a fresh `Blocked` with the raw reason.
        if let Some((last_id, last_reason, RefusalStage::Validation)) = last_refused.as_ref() {
            if candidate_id == Some(*last_id) {
                return UpgradeAdmission::Blocked(suppression_reason(last_reason));
            }
        }
        // Record this as a `Validation`-stage refusal so a REPEAT of the
        // same failure is suppressed above. Not recorded as `PostProbe`:
        // that stage's suppression check (below) is intentionally skipped
        // for `Validation` records, so a candidate that later passes
        // `validate` (e.g. after a `chmod` fix) is never incorrectly
        // blocked by a stale `Validation` refusal.
        match candidate_id {
            Some(id) => *last_refused = Some((id, reason.clone(), RefusalStage::Validation)),
            None => *last_refused = None,
        }
        return UpgradeAdmission::Blocked(reason);
    }

    // Only a `PostProbe`-stage record suppresses here: a `Validation`-stage
    // record means this SAME candidate previously failed `validate` but has
    // now passed it (its identity is unchanged, e.g. after a `chmod` fix),
    // so it must be admitted rather than masked by the stale refusal.
    if let Some((last_id, last_reason, RefusalStage::PostProbe)) = last_refused.as_ref() {
        if candidate_id == Some(*last_id) {
            return UpgradeAdmission::Blocked(suppression_reason(last_reason));
        }
    }
    *last_refused = None;

    UpgradeAdmission::Admitted { candidate_id }
}

/// Record a refusal produced AFTER [`admit_upgrade_candidate`] already
/// admitted the candidate -- a probe spawn failure, probe timeout, or
/// schema-range gate rejection from [`prepare_upgrade`] (Design
/// "Recording"). Keyed on the SAME `candidate_id` [`admit_upgrade_candidate`]
/// already captured, so this never re-stats the candidate.
#[cfg(unix)]
pub(super) fn record_post_probe_refusal(
    last_refused: &mut Option<RefusedCandidate>,
    candidate_id: Option<(u64, u64)>,
    reason: &str,
) {
    if let Some(id) = candidate_id {
        *last_refused = Some((id, reason.to_string(), RefusalStage::PostProbe));
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
