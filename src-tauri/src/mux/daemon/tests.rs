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

// ---- mux-daemon-binary-update-detect task0005: positive replacement
// proof before the upgraded notice (client side) ----
//
// These tests drive the trigger's post-reachability identity re-check
// via a stateful scripted provider, since the ordinary constant-verdict
// closure used by the task0002 tests above cannot distinguish the
// pre-fire call from the post-fire re-check call.

/// Stateful identity-check stand-in: returns `verdicts` in order, one
/// per call, repeating the last scripted verdict for any call beyond
/// the sequence. Exposes the total call count so tests can assert
/// "invoked exactly once" (no re-check, task0005 AC-4) vs "invoked
/// twice" (pre-fire then post-fire, task0005 AC-1..AC-3). `Cell` (not
/// `Mutex`/atomics) is sufficient: the trigger and its identity-check
/// closure both run synchronously on the test's own thread.
#[cfg(unix)]
struct ScriptedIdentityCheck {
    verdicts: Vec<identity::Verdict>,
    calls: std::cell::Cell<usize>,
}

#[cfg(unix)]
impl ScriptedIdentityCheck {
    fn new(verdicts: Vec<identity::Verdict>) -> Self {
        assert!(
            !verdicts.is_empty(),
            "at least one scripted verdict is required"
        );
        Self {
            verdicts,
            calls: std::cell::Cell::new(0),
        }
    }

    fn check(&self, _sock_path: &Path) -> identity::Verdict {
        let idx = self.calls.get();
        self.calls.set(idx + 1);
        self.verdicts
            .get(idx)
            .cloned()
            .unwrap_or_else(|| self.verdicts.last().cloned().expect("non-empty verdicts"))
    }

    fn call_count(&self) -> usize {
        self.calls.get()
    }
}

/// AC-1: with the provider scripted `Updated` (pre-fire) then
/// `Unchanged` (post-fire), and a fake daemon that stays reachable, the
/// trigger sends exactly one `Upgrade` frame and -- once the
/// post-reachability re-check confirms `Unchanged` -- emits exactly the
/// pinned notice line once, and returns success (the shipped TS-8
/// behavior is preserved).
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_fires_upgrade_and_reports_notice_when_reachable_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-fire.sock");
    let (server, upgrade_frames_seen) =
        spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::BecomeUpgraded));

    let provider = ScriptedIdentityCheck::new(vec![
        identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
        identity::Verdict::Unchanged,
    ]);
    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |sock_path: &Path| provider.check(sock_path),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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
    assert_eq!(
        provider.call_count(),
        2,
        "expected the identity-check provider to be consulted twice: once for the \
         firing decision, once for the post-reachability re-check"
    );
}

/// task0005 AC-2: with the provider scripted `Updated` (pre-fire) then
/// `Updated` again (post-fire -- the replacement did not take), no
/// replacement notice is emitted; exactly one pinned unconfirmed-
/// replacement warning line is emitted instead; the trigger still
/// returns success.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_unconfirmed_replacement_warns_instead_of_notice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-unconfirmed.sock");
    let (server, upgrade_frames_seen) =
        spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::BecomeUpgraded));

    let provider = ScriptedIdentityCheck::new(vec![
        identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
        identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
    ]);
    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |sock_path: &Path| provider.check(sock_path),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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
        vec![
            "Warning: mux daemon is reachable but the binary replacement could not be \
             confirmed; continuing with the existing daemon"
                .to_string()
        ],
        "expected exactly the pinned unconfirmed-replacement warning, no success notice"
    );
    assert_eq!(
        provider.call_count(),
        2,
        "expected pre-fire and post-fire calls"
    );
}

/// task0005 AC-3: with the provider scripted `Updated` then
/// `Undecidable`, behavior equals AC-2's -- undecidable evidence is
/// never claimed as success.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_undecidable_post_check_warns_instead_of_notice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-undecidable-postcheck.sock");
    let (server, upgrade_frames_seen) =
        spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::BecomeUpgraded));

    let provider = ScriptedIdentityCheck::new(vec![
        identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
        identity::Verdict::Undecidable,
    ]);
    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |sock_path: &Path| provider.check(sock_path),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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
        vec![
            "Warning: mux daemon is reachable but the binary replacement could not be \
             confirmed; continuing with the existing daemon"
                .to_string()
        ],
        "an Undecidable post-fire verdict must never be claimed as success"
    );
    assert_eq!(
        provider.call_count(),
        2,
        "expected pre-fire and post-fire calls"
    );
}

/// task0005 AC-4: on a reachability timeout, the identity-check provider
/// is invoked exactly once (pre-fire only -- no post-reachability
/// re-check is performed), the existing timeout warning is emitted, and
/// the trigger still returns success.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_timeout_does_not_re_check_identity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-timeout-no-recheck.sock");
    let (server, upgrade_frames_seen) =
        spawn_fake_current_daemon(sock_path.clone(), Some(UpgradeBehavior::Silent));

    let provider = ScriptedIdentityCheck::new(vec![identity::Verdict::Updated(PathBuf::from(
        "/fake/new-emterm",
    ))]);
    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |sock_path: &Path| provider.check(sock_path),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

    match result {
        Ok(LegacyRecovery::Compatible) => {}
        other => panic!("expected Ok(Compatible), got {other:?}"),
    }
    assert_eq!(
        upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one message: {messages:?}"
    );
    assert!(
        messages[0].to_lowercase().contains("timed out")
            || messages[0].to_lowercase().contains("timeout"),
        "expected a timeout warning: {:?}",
        messages[0]
    );
    assert_eq!(
        provider.call_count(),
        1,
        "a reachability timeout must not trigger a post-reachability re-check"
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

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

    match result {
        Ok(LegacyRecovery::Compatible) => {}
        other => panic!("expected Ok(Compatible), got {other:?}"),
    }
    assert_eq!(
        upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one message: {messages:?}"
    );
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

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

    match result {
        Ok(LegacyRecovery::Compatible) => {}
        other => panic!("expected Ok(Compatible), got {other:?}"),
    }
    assert_eq!(
        upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one message: {messages:?}"
    );
    assert!(
        messages[0].to_lowercase().contains("timed out")
            || messages[0].to_lowercase().contains("timeout"),
        "expected a timeout warning: {:?}",
        messages[0]
    );
}

// ---- task0004 AC-6: trigger-side warning suppression for a
// marker-prefixed rejection ----

/// AC-6: a marker-prefixed rejection (a repeat refusal the daemon
/// already suppressed) emits NO user-facing line and still returns
/// success.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_suppressed_rejection_emits_no_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-suppressed.sock");
    let (server, upgrade_frames_seen) = spawn_fake_current_daemon(
        sock_path.clone(),
        Some(UpgradeBehavior::Reject(format!(
            "{UPGRADE_SUPPRESSED_MARKER}world-writable candidate (install a new binary or \
             restart the daemon to re-enable the attempt)"
        ))),
    );

    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |_sock_path: &Path| identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

    match result {
        Ok(LegacyRecovery::Compatible) => {}
        other => panic!("expected Ok(Compatible), got {other:?}"),
    }
    assert_eq!(
        upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(
        messages.is_empty(),
        "AC-6: a marker-prefixed rejection must emit no user-facing line, got: {messages:?}"
    );
}

/// AC-6: a rejection WITHOUT the marker still emits the existing
/// visible warning -- unchanged from before this task.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_unmarked_rejection_still_warns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("update-detect-unmarked.sock");
    let (server, upgrade_frames_seen) = spawn_fake_current_daemon(
        sock_path.clone(),
        Some(UpgradeBehavior::Reject(
            "candidate binary supports handoff schema 1-1, this daemon needs 2".to_string(),
        )),
    );

    let mut messages: Vec<String> = Vec::new();
    let result = recover_from_legacy_daemon_with(
        &sock_path,
        |_sock_path: &Path| identity::Verdict::Updated(PathBuf::from("/fake/new-emterm")),
        |line: &str| messages.push(line.to_string()),
    );

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

    match result {
        Ok(LegacyRecovery::Compatible) => {}
        other => panic!("expected Ok(Compatible), got {other:?}"),
    }
    assert_eq!(
        upgrade_frames_seen.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one message: {messages:?}"
    );
    assert!(
        messages[0].contains("declined the automatic in-place upgrade"),
        "AC-6: an unmarked rejection must still emit the existing visible warning: {:?}",
        messages[0]
    );
}

// ---- task0004 AC-2 (unit half) / AC-4 / AC-5: `admit_upgrade_candidate`
// sequencing -- the run-loop `select!` body is not unit-testable whole
// (Test Notes), so these exercise the extracted "suppress -> validate"
// helper directly, plus the REAL `prepare_upgrade` (already
// parameterized over its probe) with an injected counting probe to
// prove the "before any probe subprocess is spawned" / "exactly one
// spawn across two signals" claims end to end. ----

/// AC-2 (unit half): a validation-failing candidate is `Blocked` with
/// the validator's own reason, recorded for later suppression, and (by
/// construction -- `admit_upgrade_candidate` never calls a probe itself)
/// this happens without ever reaching one.
#[cfg(unix)]
#[test]
fn admit_upgrade_candidate_blocks_on_validation_failure_and_records_it() {
    let mut last_refused: Option<RefusedCandidate> = None;
    let candidate = Path::new("/fake/candidate");

    let outcome = admit_upgrade_candidate(
        candidate,
        &mut last_refused,
        |_c| Some((7, 42)),
        |_c| Err("world-writable candidate".to_string()),
    );

    match outcome {
        UpgradeAdmission::Blocked(reason) => assert_eq!(reason, "world-writable candidate"),
        other => panic!("expected Blocked, got {other:?}"),
    }
    assert_eq!(
        last_refused,
        Some((
            (7, 42),
            "world-writable candidate".to_string(),
            RefusalStage::Validation
        )),
        "AC-2: a validation refusal must be recorded for suppression"
    );
}

/// AC-4: after a refused attempt (here, a real schema-probe rejection
/// via `prepare_upgrade` with an injected COUNTING probe), a second
/// signal for a candidate with the SAME (device, inode) is rejected
/// WITHOUT spawning the probe again -- the counter stays at exactly one
/// across both signals -- and the reply reason starts with the pinned
/// `upgrade-suppressed: ` marker.
#[cfg(unix)]
#[tokio::test]
async fn admit_upgrade_candidate_suppresses_repeat_refusal_after_a_real_probe_rejection() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    // A permissive ambient umask (e.g. `002`) would otherwise leave a
    // freshly created tempdir group-writable, making "the parent
    // directory is conforming" environment-dependent -- harden
    // explicitly rather than relying on umask.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
        .expect("harden tempdir to a conforming mode");
    let candidate = tempfile::NamedTempFile::new_in(dir.path()).expect("candidate file");
    let daemon_uid = crate::mux::identity::effective_uid();
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let ack_slot = no_ack_slot();
    let probe_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut last_refused: Option<RefusedCandidate> = None;

    // First signal: validation passes (an owner-only regular file with
    // a conforming parent), so admission proceeds to the probe.
    let candidate_id = match admit_upgrade_candidate(
        candidate.path(),
        &mut last_refused,
        crate::mux::identity::capture_dev_ino,
        |c| crate::mux::identity::validate_candidate_path(c, daemon_uid),
    ) {
        UpgradeAdmission::Admitted { candidate_id } => candidate_id,
        UpgradeAdmission::Blocked(reason) => {
            panic!("expected the first signal to be admitted, got Blocked({reason})")
        }
    };

    let counter = Arc::clone(&probe_calls);
    let counting_probe = move |_c: &Path| -> Result<std::ops::RangeInclusive<u32>, String> {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err("simulated schema mismatch".to_string())
    };
    let result = prepare_upgrade(
        &mgr,
        -1,
        candidate.path(),
        vec!["mux".to_string(), "--daemon".to_string()],
        dir.path(),
        1,
        &ack_slot,
        counting_probe,
        |_mgr: &SessionManager,
         _fd: RawFd,
         _path: &Path|
         -> Result<mux_ipc::handoff::HandoffDocument, String> {
            panic!("snapshot must not run when the probe rejects")
        },
    )
    .await;
    let reason = result.expect_err("the counting probe always rejects");
    record_post_probe_refusal(&mut last_refused, candidate_id, &reason);
    assert_eq!(probe_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Second signal: SAME candidate (same device, inode) -> suppressed
    // WITHOUT spawning the probe again.
    match admit_upgrade_candidate(
        candidate.path(),
        &mut last_refused,
        crate::mux::identity::capture_dev_ino,
        |c| crate::mux::identity::validate_candidate_path(c, daemon_uid),
    ) {
        UpgradeAdmission::Blocked(reason2) => {
            assert!(
                reason2.starts_with(UPGRADE_SUPPRESSED_MARKER),
                "AC-4: a suppressed rejection must start with the pinned marker: {reason2:?}"
            );
            assert!(
                reason2.contains("simulated schema mismatch"),
                "AC-4: the suppressed reason must carry the original refusal reason: \
                 {reason2:?}"
            );
        }
        UpgradeAdmission::Admitted { .. } => panic!("AC-4: the repeat must be suppressed"),
    }
    assert_eq!(
        probe_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "AC-4: the probe must be spawned exactly once across both signals"
    );
}

/// AC-5: after a refusal, a candidate whose (device, inode) DIFFERS
/// clears the suppression state and is validated and probed normally
/// (not suppressed).
#[cfg(unix)]
#[tokio::test]
async fn admit_upgrade_candidate_clears_suppression_for_a_differing_candidate_and_probes_normally()
{
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    // A permissive ambient umask (e.g. `002`) would otherwise leave a
    // freshly created tempdir group-writable, making "the parent
    // directory is conforming" (needed for `new_candidate` below to be
    // admitted) environment-dependent -- harden explicitly rather than
    // relying on umask.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
        .expect("harden tempdir to a conforming mode");
    let daemon_uid = crate::mux::identity::effective_uid();

    // First candidate: world-writable -> refused by VALIDATION (not the
    // probe), recording its (device, inode).
    let refused_candidate = tempfile::NamedTempFile::new_in(dir.path()).expect("file");
    std::fs::set_permissions(
        refused_candidate.path(),
        std::fs::Permissions::from_mode(0o646),
    )
    .expect("loosen permissions");

    let mut last_refused: Option<RefusedCandidate> = None;
    match admit_upgrade_candidate(
        refused_candidate.path(),
        &mut last_refused,
        crate::mux::identity::capture_dev_ino,
        |c| crate::mux::identity::validate_candidate_path(c, daemon_uid),
    ) {
        UpgradeAdmission::Blocked(_) => {}
        UpgradeAdmission::Admitted { .. } => {
            panic!("a world-writable candidate must be refused by validation")
        }
    }
    assert!(
        last_refused.is_some(),
        "the validation refusal must be recorded"
    );

    // Second signal: a DIFFERENT candidate (different device/inode),
    // conforming permissions -- must clear the suppression state and be
    // validated + probed normally.
    let new_candidate = tempfile::NamedTempFile::new_in(dir.path()).expect("file");
    match admit_upgrade_candidate(
        new_candidate.path(),
        &mut last_refused,
        crate::mux::identity::capture_dev_ino,
        |c| crate::mux::identity::validate_candidate_path(c, daemon_uid),
    ) {
        UpgradeAdmission::Admitted { .. } => {}
        UpgradeAdmission::Blocked(reason) => {
            panic!("AC-5: a differing candidate must not inherit suppression: {reason}")
        }
    }
    assert!(
        last_refused.is_none(),
        "AC-5: the suppression state must be cleared for a differing candidate"
    );

    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let ack_slot = no_ack_slot();
    let probe_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&probe_calls);
    let counting_probe = move |_c: &Path| -> Result<std::ops::RangeInclusive<u32>, String> {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(mux_ipc::handoff::SUPPORTED_HANDOFF_SCHEMA_VERSIONS)
    };
    let result = prepare_upgrade(
        &mgr,
        0,
        new_candidate.path(),
        vec!["mux".to_string(), "--daemon".to_string()],
        dir.path(),
        mux_ipc::handoff::HANDOFF_SCHEMA_VERSION,
        &ack_slot,
        counting_probe,
        ok_snapshot,
    )
    .await;
    assert!(
        result.is_ok(),
        "AC-5: the differing candidate must probe normally: {result:?}"
    );
    assert_eq!(probe_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// AC-7: this task adds no non-Unix-gated item -- a Windows / CLI-only
// build must compile with zero behavior change (NFR2). Asserted here by
// construction (every new item introduced by task0004 in this file and
// in `identity.rs` is `#[cfg(unix)]`); the actual cross-build
// verification is `cargo check --no-default-features` (project
// convention, not a unit test).

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

    server
        .join()
        .expect("fake current-protocol daemon thread panicked");

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
    use crate::mux::session::pane::{DetachReason, MuxPane, PaneOutputTarget, SharedOutputTarget};
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
    let target: SharedOutputTarget = StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
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
    let none = tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
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
        pane.agent_waiters
            .lock()
            .unwrap()
            .push(crate::mux::session::pane::AgentWaiter {
                states: vec![crate::agent_status::AgentState::Idle],
                after_revision: None,
                responder: Some(tx),
            });
        drop(rx); // simulate a disconnected waiting client
    }

    let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

    // Live `D`: command ended, but no clear yet (still armed only).
    apply_live_osc133_mark(&mgr, pane_id, PromptMarkKind::CommandEnd).await;
    let none = tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
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
    assert_eq!(status.state, Some(crate::agent_status::AgentState::Blocked));
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
    assert_eq!(status.state, Some(crate::agent_status::AgentState::Working));
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
    let none = tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
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
    assert!(
        !pane.exited,
        "AC-1: upgrade preparation must not exit panes"
    );
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

    let doc = crate::mux::upgrade::read_and_remove_handoff_file(&outcome.handoff_document_path)
        .expect("the real snapshot must have written a decodable handoff document");
    assert_eq!(
        doc.incarnation, incarnation,
        "AC-1: incarnation token recorded"
    );
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
    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_daemon_channels();

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
    assert_eq!(
        counts.descriptor_count, 1,
        "the listener always counts as one descriptor"
    );
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
    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_daemon_channels();

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
    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_daemon_channels();

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
    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_daemon_channels();
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
    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_daemon_channels();

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
