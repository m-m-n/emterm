use super::*;
use crate::mux::daemon;
use mux_ipc::protocol::{
    AgentApiErrorKind, AgentState, ClientType, ErrorMsg, HelloMsg, MessageType, MuxMessage,
    PREVIOUS_PROTOCOL_VERSION, PROTOCOL_VERSION, SessionInfo, WelcomeMsg,
};

#[test]
fn test_check_nesting_not_set() {
    temp_env::with_var_unset("EMTERM_MUX", || {
        assert!(check_nesting().is_ok());
    });
}

#[test]
fn test_check_nesting_set() {
    temp_env::with_var("EMTERM_MUX", Some("1"), || {
        assert!(check_nesting().is_err());
    });
}

// ---- task0004 AC-3: `perform_upgrade_replacement`'s attempt-or-reenter
// decision helper, table-tested across accepted/refused outcomes
// (the final pre-exec site cannot be end-to-end tested without a real
// exec -- this decision helper plus TS-9's unmodified regression suite
// are the planned coverage, per the task plan's Test Notes) ----

#[cfg(unix)]
#[test]
fn decide_replacement_attempts_on_accepted_validation() {
    assert_eq!(decide_replacement(Ok(())), ReplacementDecision::Attempt);
}

#[cfg(unix)]
#[test]
fn decide_replacement_reenters_on_refused_validation() {
    assert_eq!(
        decide_replacement(Err("upgrade candidate is owned by uid 2000".to_string())),
        ReplacementDecision::Reenter {
            reason: "upgrade candidate is owned by uid 2000".to_string()
        }
    );
}

// ---- send-keys target resolution tests ----

use crate::mux::ipc::protocol::WindowInfo;

fn make_test_sessions(windows: Vec<WindowInfo>, active_window_index: u32) -> Vec<SessionInfo> {
    vec![SessionInfo {
        id: 1,
        name: "test".to_string(),
        window_count: windows.len() as u32,
        pane_count: windows.len() as u32,
        active_window_index,
        windows,
    }]
}

#[test]
fn test_resolve_target_pane_active_window() {
    let sessions = make_test_sessions(
        vec![
            WindowInfo {
                id: 1,
                name: "shell".to_string(),
                active_pane_id: 10,
            },
            WindowInfo {
                id: 2,
                name: "editor".to_string(),
                active_pane_id: 20,
            },
        ],
        1, // active window index = 1
    );
    let pane_id = resolve_target_pane(&sessions, None).unwrap();
    assert_eq!(pane_id, 20); // active window's pane
}

#[test]
fn test_resolve_target_pane_explicit_index() {
    let sessions = make_test_sessions(
        vec![
            WindowInfo {
                id: 1,
                name: "shell".to_string(),
                active_pane_id: 10,
            },
            WindowInfo {
                id: 2,
                name: "editor".to_string(),
                active_pane_id: 20,
            },
        ],
        0,
    );
    let pane_id = resolve_target_pane(&sessions, Some(0)).unwrap();
    assert_eq!(pane_id, 10);
    let pane_id = resolve_target_pane(&sessions, Some(1)).unwrap();
    assert_eq!(pane_id, 20);
}

#[test]
fn test_resolve_target_pane_out_of_range() {
    let sessions = make_test_sessions(
        vec![WindowInfo {
            id: 1,
            name: "shell".to_string(),
            active_pane_id: 10,
        }],
        0,
    );
    let err = resolve_target_pane(&sessions, Some(5)).unwrap_err();
    assert!(err.to_string().contains("out of range"));
}

#[test]
fn test_resolve_target_pane_no_sessions() {
    let err = resolve_target_pane(&[], None).unwrap_err();
    assert!(err.to_string().contains("No active session"));
}

#[test]
fn test_resolve_target_pane_no_active_pane() {
    let sessions = make_test_sessions(
        vec![WindowInfo {
            id: 1,
            name: "empty".to_string(),
            active_pane_id: 0,
        }],
        0,
    );
    let err = resolve_target_pane(&sessions, None).unwrap_err();
    assert!(err.to_string().contains("No active pane"));
}

// ---- Agent API (read/send/wait): exit-code mapping and --pane current
// resolution, per Test Notes "no live daemon needed" (AC-7, AC-8) ----

#[test]
fn agent_api_error_exit_code_matches_convention() {
    assert_eq!(
        agent_api_error_exit_code(AgentApiErrorKind::InvalidInput),
        2
    );
    assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::Timeout), 3);
    assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::UnknownPane), 4);
    assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::PaneGone), 4);
    assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::NotMuxPane), 5);
}

#[test]
fn resolve_pane_arg_passes_through_explicit_id() {
    assert_eq!(
        resolve_pane_arg("abc123-7").unwrap(),
        "abc123-7".to_string()
    );
}

#[test]
fn resolve_pane_arg_current_resolves_from_env() {
    temp_env::with_var("EMTERM_PANE_ID", Some("deadbeef-3"), || {
        assert_eq!(resolve_pane_arg("current").unwrap(), "deadbeef-3");
    });
}

#[test]
fn resolve_pane_arg_current_missing_env_is_usage_error() {
    temp_env::with_var_unset("EMTERM_PANE_ID", || {
        let err = resolve_pane_arg("current").unwrap_err();
        assert!(err.contains("EMTERM_PANE_ID"));
    });
}

#[test]
fn parse_agent_states_single_and_multiple() {
    assert_eq!(parse_agent_states("done").unwrap(), vec![AgentState::Done]);
    assert_eq!(
        parse_agent_states("done,blocked").unwrap(),
        vec![AgentState::Done, AgentState::Blocked]
    );
    assert_eq!(
        parse_agent_states(" idle , working ").unwrap(),
        vec![AgentState::Idle, AgentState::Working]
    );
}

#[test]
fn parse_agent_states_rejects_unknown_state() {
    assert!(parse_agent_states("bogus").is_err());
    assert!(parse_agent_states("done,bogus").is_err());
}

#[test]
fn parse_agent_states_rejects_empty() {
    assert!(parse_agent_states("").is_err());
    assert!(parse_agent_states(",").is_err());
}

// ---- `emterm mux attach` legacy-daemon recovery (task0001) ----
//
// A fake daemon is a bare `UnixListener` thread rather than a real
// spawned process, mirroring `mux::daemon::tests`' construction style
// and socket-path isolation (task0001 Test Notes).

// task0005 rework: derived from `PREVIOUS_PROTOCOL_VERSION` rather than
// hardcoded to `1`. `recover_from_legacy_daemon`'s retry handshake uses
// `PREVIOUS_PROTOCOL_VERSION` (exactly one version behind whatever
// `PROTOCOL_VERSION` currently is) — a fixed literal here silently
// stopped matching that retry the moment `PROTOCOL_VERSION` moved past
// 2, at which point the fake daemon's `else` branch below rejects the
// retry, `recover_from_legacy_daemon` gives up (returns `Err`), and the
// fake daemon's `accept()` loop is left waiting forever for a THIRD
// connection that will never arrive — this test's `legacy.join()` below
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

/// Like [`read_frame`], but returns `None` instead of panicking on EOF
/// (task0005): `daemon::is_daemon_running`'s reachability probe opens
/// and immediately drops a bare connection before the real handshake
/// connection follows, so a one-shot stand-in daemon must be able to
/// skip past it rather than treat it as the real Hello.
#[cfg(unix)]
fn try_read_frame<S: std::io::Read>(stream: &mut S) -> Option<MuxMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).ok()?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf).ok()?;
    MuxMessage::from_frame_body(&frame_buf)
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

/// Write an `Error` control frame (task0009 rework, AC-10 test fixture):
/// mirrors [`write_welcome`]'s framing exactly, for a stand-in daemon
/// simulating a refused `Upgrade` request (FR13).
#[cfg(unix)]
fn write_error<S: std::io::Write>(stream: &mut S, err: &ErrorMsg) {
    let msg = MuxMessage::control(MessageType::Error, 0, err);
    let body = msg.to_frame_body();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .expect("write frame length");
    stream.write_all(&body).expect("write frame body");
    stream.flush().expect("flush");
}

/// Spawn a thread that behaves like a single-instance legacy (v1) mux
/// daemon on `sock_path`: rejects a mismatched Hello, accepts a
/// [`FAKE_LEGACY_VERSION`] Hello, then per-frame: an `Upgrade` request is
/// silently ignored (task0005 Recovery path — a daemon predating that
/// feature discards it via the unknown-type path, D7) and the loop keeps
/// accepting; `Shutdown` removes the socket file and exits, exactly as
/// before this feature.
#[cfg(unix)]
fn spawn_fake_legacy_daemon(sock_path: std::path::PathBuf) -> std::thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&sock_path).expect("bind fake legacy daemon socket");
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
                continue;
            }

            let accept = WelcomeMsg::Accepted {
                server_version: FAKE_LEGACY_VERSION,
                sessions: Vec::<SessionInfo>::new(),
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

/// Stand-in for a freshly-respawned current-protocol daemon: binds
/// `sock_path` synchronously (so it is ready the moment this returns),
/// then accepts exactly one Hello and replies `Accepted` on a
/// background thread. Used as the injected `spawn` step in
/// [`resolve_attach_socket_with`] tests, since a `cargo test --lib`
/// unit test binary is not the real `emterm` binary
/// [`daemon::spawn_daemon`] would spawn (task0001 Test Notes / AC-1
/// deviation).
#[cfg(unix)]
fn spawn_fake_current_daemon(sock_path: std::path::PathBuf) -> std::thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&sock_path).expect("bind fake respawned daemon socket");
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let hello_frame = read_frame(&mut stream);
        assert_eq!(hello_frame.msg_type, MessageType::Hello);
        let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);

        let accept = WelcomeMsg::Accepted {
            server_version: PROTOCOL_VERSION,
            sessions: Vec::<SessionInfo>::new(),
        };
        write_welcome(&mut stream, &accept);
        let _ = std::fs::remove_file(&sock_path);
    })
}

/// AC-1: with a fake legacy daemon listening, `resolve_attach_socket_with`
/// shuts it down (via the shared recovery probe), invokes the spawn step,
/// and a subsequent handshake against the socket is accepted.
#[cfg(unix)]
#[test]
fn resolve_attach_socket_recovers_from_legacy_daemon_and_respawns() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("legacy-attach.sock");
    let legacy = spawn_fake_legacy_daemon(sock_path.clone());

    let respawned: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let respawned_for_closure = respawned.clone();

    let result = resolve_attach_socket_with(&sock_path, move |p| {
        let handle = spawn_fake_current_daemon(p.to_path_buf());
        *respawned_for_closure.lock().unwrap() = Some(handle);
        Ok(())
    });

    legacy.join().expect("fake legacy daemon thread panicked");

    match &result {
        Ok(path) => assert_eq!(path, &sock_path),
        Err(e) => panic!("expected Ok(sock_path), got Err({e:?})"),
    }

    // A subsequent handshake on the socket is accepted (AC-1).
    let mut stream =
        std::os::unix::net::UnixStream::connect(&sock_path).expect("connect to respawned daemon");
    let hello = HelloMsg {
        client_type: ClientType::Cli,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .expect("write frame length");
    stream.write_all(&body).expect("write frame body");
    stream.flush().expect("flush");

    let welcome_frame = read_frame(&mut stream);
    assert_eq!(welcome_frame.msg_type, MessageType::Welcome);
    let welcome: WelcomeMsg = welcome_frame.decode_payload().expect("Welcome payload");
    assert!(
        matches!(welcome, WelcomeMsg::Accepted { .. }),
        "expected the respawned daemon to accept the handshake, got {welcome:?}"
    );

    if let Some(handle) = respawned.lock().unwrap().take() {
        handle
            .join()
            .expect("fake respawned daemon thread panicked");
    }
}

/// AC-2: with a fake current-protocol daemon listening,
/// `resolve_attach_socket_with` succeeds without spawning anything; the
/// fake daemon still owns the socket afterwards.
#[cfg(unix)]
#[test]
fn resolve_attach_socket_is_noop_against_a_compatible_daemon() {
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("compatible-attach.sock");
    let listener = UnixListener::bind(&sock_path).expect("bind fake v2 daemon socket");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let hello_frame = read_frame(&mut stream);
        let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        let accept = WelcomeMsg::Accepted {
            server_version: PROTOCOL_VERSION,
            sessions: Vec::<SessionInfo>::new(),
        };
        write_welcome(&mut stream, &accept);
    });

    let spawn_called = Arc::new(AtomicBool::new(false));
    let spawn_called_for_closure = spawn_called.clone();

    let result = resolve_attach_socket_with(&sock_path, move |_p| {
        spawn_called_for_closure.store(true, Ordering::SeqCst);
        Ok(())
    });

    server.join().expect("fake daemon thread panicked");

    match &result {
        Ok(path) => assert_eq!(path, &sock_path),
        Err(e) => panic!("expected Ok(sock_path), got Err({e:?})"),
    }
    assert!(
        !spawn_called.load(Ordering::SeqCst),
        "a compatible daemon must not trigger a respawn"
    );
    assert!(sock_path.exists(), "a compatible daemon is left untouched");
}

/// AC-3: with no socket present, `resolve_attach_socket_with` fails with
/// the existing "No mux sessions to attach to" message, byte-identical
/// to today's error, and never calls the spawn step.
#[test]
fn resolve_attach_socket_fails_when_no_socket_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("nonexistent.sock");

    let result = resolve_attach_socket_with(&sock_path, |_p| {
        panic!("spawn must not be called when no socket is present");
    });

    match result {
        Err(msg) => assert_eq!(
            msg,
            "No mux sessions to attach to (daemon not running)\n\
             Use 'emterm mux' to start a new session."
        ),
        Ok(_) => panic!("expected Err for a nonexistent socket"),
    }
}

// ---- `emterm mux upgrade` / `probe-handoff` / usage table (task0005) ----

/// AC-1: `upgrade` is registered in both the dispatch table and the
/// usage text; an unknown subcommand still reports usage exactly as
/// before (non-zero exit, no daemon interaction).
#[test]
fn usage_text_lists_upgrade_and_probe_handoff_subcommands() {
    assert!(MUX_USAGE.contains("upgrade"));
    assert!(MUX_USAGE.contains("probe-handoff"));
}

#[test]
fn run_reports_usage_for_unknown_subcommand() {
    assert_eq!(run(&["bogus".to_string()]), 2);
}

// ---- `emterm mux probe-handoff` (AC-5) ----

/// AC-5: prints a parsable schema version range and exits successfully.
/// Never references a socket path at all, so "without connecting to a
/// socket" holds by construction.
#[test]
fn probe_handoff_prints_parsable_range_and_succeeds() {
    let line = handoff_schema_range_line();
    let parts: Vec<u32> = line
        .split_whitespace()
        .map(|s| s.parse().expect("schema range values must be integers"))
        .collect();
    assert_eq!(
        parts.len(),
        2,
        "expected exactly `<min> <max>`, got {line:?}"
    );
    assert!(parts[0] <= parts[1], "min must not exceed max: {line:?}");
    assert_eq!(execute_probe_handoff(), 0);
}

/// AC-9 (task0009 rework, finding 32bb6e465ac0fbb4 / a50509ac760abb59 /
/// d6b2bb34403b44f9): the probe's output must be derived from —
/// therefore always equal to — `mux_ipc::handoff::
/// SUPPORTED_HANDOFF_SCHEMA_VERSIONS`, the same range
/// `crate::mux::upgrade::read_and_remove_handoff_file` checks a decoded
/// document against. A local literal here could silently drift from it;
/// this test fails the moment the two diverge.
#[test]
fn probe_handoff_range_matches_the_canonical_supported_schema_versions() {
    let range = mux_ipc::handoff::SUPPORTED_HANDOFF_SCHEMA_VERSIONS;
    assert_eq!(
        handoff_schema_range_line(),
        format!("{} {}", range.start(), range.end())
    );
}

// ---- `emterm mux upgrade` (AC-2/AC-3/AC-4) ----

/// Accept connections on `listener` until one delivers a real frame
/// (task0005): skips over bare connect-then-drop probes (e.g.
/// `daemon::is_daemon_running`) that close before writing anything,
/// which [`try_read_frame`] reports as `None`. Returns `None` only if
/// the listener itself stops accepting.
#[cfg(unix)]
fn accept_until_real_frame(
    listener: &std::os::unix::net::UnixListener,
) -> Option<(std::os::unix::net::UnixStream, MuxMessage)> {
    loop {
        let (mut stream, _) = listener.accept().ok()?;
        if let Some(frame) = try_read_frame(&mut stream) {
            return Some((stream, frame));
        }
        // Spurious probe connection (already closed) — accept the next.
    }
}

/// Stand-in daemon for [`execute_upgrade_at`] tests: handshakes at
/// [`PROTOCOL_VERSION`] and asserts it then receives an `Upgrade`
/// request. When `becomes_reachable_after_upgrade` is set, accepts one
/// further real connection and answers as a current-version daemon
/// (simulating the in-place replacement, AC-2); otherwise never accepts
/// a further real connection, so the caller's poll must time out
/// (AC-3). Tolerates the leading `daemon::is_daemon_running`
/// reachability probe via [`accept_until_real_frame`].
#[cfg(unix)]
fn spawn_fake_daemon_for_upgrade(
    sock_path: std::path::PathBuf,
    becomes_reachable_after_upgrade: bool,
) -> std::thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&sock_path).expect("bind fake daemon socket (upgrade)");
    std::thread::spawn(move || {
        let Some((mut stream, hello_frame)) = accept_until_real_frame(&listener) else {
            return;
        };
        assert_eq!(hello_frame.msg_type, MessageType::Hello);
        let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        write_welcome(
            &mut stream,
            &WelcomeMsg::Accepted {
                server_version: PROTOCOL_VERSION,
                sessions: Vec::new(),
            },
        );

        let upgrade_frame = read_frame(&mut stream);
        assert_eq!(upgrade_frame.msg_type, MessageType::Upgrade);
        drop(stream);

        if becomes_reachable_after_upgrade {
            let Some((mut stream2, hello2_frame)) = accept_until_real_frame(&listener) else {
                return;
            };
            let hello2: HelloMsg = hello2_frame.decode_payload().expect("Hello payload");
            assert_eq!(hello2.protocol_version, PROTOCOL_VERSION);
            write_welcome(
                &mut stream2,
                &WelcomeMsg::Accepted {
                    server_version: PROTOCOL_VERSION,
                    sessions: Vec::new(),
                },
            );
            let _ = std::fs::remove_file(&sock_path);
        }
        // else (AC-3): never accept a further real connection — the
        // listener is dropped when this thread returns, so subsequent
        // poll connects fail.
    })
}

/// AC-2: against a stand-in that accepts the handshake, `upgrade` sends
/// the request and reports success once the stand-in becomes reachable
/// at the current protocol version.
#[cfg(unix)]
#[test]
fn execute_upgrade_reports_success_once_daemon_reachable_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("upgrade-success.sock");
    let fake = spawn_fake_daemon_for_upgrade(sock_path.clone(), true);

    let code = execute_upgrade_at(&sock_path);

    fake.join().expect("fake daemon thread panicked");
    assert_eq!(
        code, 0,
        "expected success once the daemon is reachable again"
    );
}

/// AC-3: against a stand-in that never becomes reachable again, `upgrade`
/// reports a timeout with a non-success exit status and returns (does
/// not hang indefinitely).
#[cfg(unix)]
#[test]
fn execute_upgrade_reports_timeout_when_daemon_never_returns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("upgrade-timeout.sock");
    let fake = spawn_fake_daemon_for_upgrade(sock_path.clone(), false);

    let code = execute_upgrade_at(&sock_path);

    fake.join().expect("fake daemon thread panicked");
    assert_ne!(code, 0, "expected a non-success exit on timeout");
}

/// Stand-in daemon that REFUSES the upgrade (FR13): handshakes normally,
/// then answers the `Upgrade` request with an `Error` frame carrying
/// `reason`, exactly like the real daemon's accept-loop abort path.
#[cfg(unix)]
fn spawn_fake_daemon_that_rejects_upgrade(
    sock_path: std::path::PathBuf,
    reason: &'static str,
) -> std::thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&sock_path).expect("bind fake daemon socket (upgrade)");
    std::thread::spawn(move || {
        let Some((mut stream, hello_frame)) = accept_until_real_frame(&listener) else {
            return;
        };
        assert_eq!(hello_frame.msg_type, MessageType::Hello);
        write_welcome(
            &mut stream,
            &WelcomeMsg::Accepted {
                server_version: PROTOCOL_VERSION,
                sessions: Vec::new(),
            },
        );

        let upgrade_frame = read_frame(&mut stream);
        assert_eq!(upgrade_frame.msg_type, MessageType::Upgrade);
        write_error(
            &mut stream,
            &ErrorMsg {
                message: reason.to_string(),
            },
        );
        // The daemon keeps serving unchanged after an aborted upgrade —
        // this connection just ends here, matching the real daemon's
        // per-connection handling (the CLI process exits right after
        // reading the Error, so nothing further needs to be served).
    })
}

/// AC-10 (task0009 rework, finding 07f6dbc60e84d54f): when the daemon
/// refuses the upgrade with an `Error` reply, `mux upgrade` reports that
/// refusal (including the daemon's own reason) and exits non-zero,
/// rather than falling through to the reachability poll and reporting
/// success against the SAME still-running (unchanged) daemon.
#[cfg(unix)]
#[test]
fn execute_upgrade_reports_rejection_with_daemons_reason_and_non_zero_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("upgrade-rejected.sock");
    let fake = spawn_fake_daemon_that_rejects_upgrade(sock_path.clone(), "candidate incompatible");

    let code = execute_upgrade_at(&sock_path);

    fake.join().expect("fake daemon thread panicked");
    assert_ne!(
        code, 0,
        "AC-10: a daemon-refused upgrade must exit non-zero, not report success"
    );
}

/// AC-4: with no daemon running, `upgrade` reports that clearly without
/// creating a socket or spawning a daemon.
#[test]
fn execute_upgrade_reports_no_daemon_without_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("no-daemon.sock");

    #[cfg(unix)]
    let code = execute_upgrade_at(&sock_path);
    #[cfg(not(unix))]
    let code = execute_upgrade();

    assert_ne!(
        code, 0,
        "expected a non-success exit with no daemon running"
    );
    assert!(
        !sock_path.exists(),
        "must not create a socket when no daemon is running"
    );
}

// ---- Recovery path upgrade-first attempt (AC-6/AC-7) ----

/// Stand-in legacy daemon for the recovery-path tests: rejects a
/// [`PROTOCOL_VERSION`] Hello (so the initial compatibility probe
/// mismatches, exactly like [`spawn_fake_legacy_daemon`]), accepts a
/// [`FAKE_LEGACY_VERSION`] Hello, and then branches on the next frame:
/// `Upgrade` either flips this stand-in into answering as a
/// current-version daemon from then on (`upgrades_in_place = true`,
/// AC-7) or is silently ignored so the daemon keeps answering as legacy
/// (`upgrades_in_place = false`, AC-6); `Shutdown` removes the socket and
/// exits, exactly like today's fallback expects.
#[cfg(unix)]
fn spawn_fake_legacy_daemon_with_upgrade(
    sock_path: std::path::PathBuf,
    upgrades_in_place: bool,
) -> std::thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = UnixListener::bind(&sock_path).expect("bind fake legacy daemon socket");
    let upgraded = Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let hello_frame = read_frame(&mut stream);
            assert_eq!(hello_frame.msg_type, MessageType::Hello);
            let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");

            let is_upgraded = upgraded.load(Ordering::SeqCst);
            let is_current = is_upgraded && hello.protocol_version == PROTOCOL_VERSION;
            let is_legacy = !is_upgraded && hello.protocol_version == FAKE_LEGACY_VERSION;

            if !is_current && !is_legacy {
                let reject = WelcomeMsg::Rejected {
                    reason: format!(
                        "Protocol version mismatch: client={}, server={}",
                        hello.protocol_version,
                        if is_upgraded {
                            PROTOCOL_VERSION
                        } else {
                            FAKE_LEGACY_VERSION
                        }
                    ),
                };
                write_welcome(&mut stream, &reject);
                continue;
            }

            let accept = WelcomeMsg::Accepted {
                server_version: if is_current {
                    PROTOCOL_VERSION
                } else {
                    FAKE_LEGACY_VERSION
                },
                sessions: Vec::<SessionInfo>::new(),
            };
            write_welcome(&mut stream, &accept);

            if is_current {
                // One successful post-upgrade connection is enough to
                // prove reachability; clean up and stop (AC-7).
                let _ = std::fs::remove_file(&sock_path);
                break;
            }

            let frame = read_frame(&mut stream);
            match frame.msg_type {
                MessageType::Upgrade => {
                    if upgrades_in_place {
                        upgraded.store(true, Ordering::SeqCst);
                    }
                    // else (AC-6): ignore — drop this connection and
                    // keep accepting as a legacy daemon.
                }
                MessageType::Shutdown => {
                    let _ = std::fs::remove_file(&sock_path);
                    break;
                }
                other => panic!("unexpected frame after legacy Accepted: {other:?}"),
            }
        }
    })
}

/// AC-6: against a stand-in that ignores the upgrade request, the
/// recovery helper falls back to shutdown-then-respawn (`Recovered`)
/// only after the upgrade attempt.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_falls_back_after_ignored_upgrade() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("recovery-ignored-upgrade.sock");
    let legacy = spawn_fake_legacy_daemon_with_upgrade(sock_path.clone(), false);

    let result = daemon::recover_from_legacy_daemon(&sock_path);

    legacy.join().expect("fake legacy daemon thread panicked");

    match result {
        Ok(daemon::LegacyRecovery::Recovered) => {}
        other => panic!("expected Recovered (fallback after timeout), got {other:?}"),
    }
}

/// AC-7: against a stand-in that becomes reachable at the current
/// protocol version after the upgrade request, the recovery helper does
/// not fall back to shutdown — it reports `Compatible`.
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_treats_in_place_upgrade_as_compatible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("recovery-in-place-upgrade.sock");
    let legacy = spawn_fake_legacy_daemon_with_upgrade(sock_path.clone(), true);

    let result = daemon::recover_from_legacy_daemon(&sock_path);

    legacy.join().expect("fake legacy daemon thread panicked");

    match result {
        Ok(daemon::LegacyRecovery::Compatible) => {}
        other => panic!("expected Compatible (no fallback), got {other:?}"),
    }
}

/// AC-6 (mux-daemon-binary-update-detect task0002, D6/FR5): against a
/// legacy daemon that silently ignores the `Upgrade` frame, the pinned
/// FR5 warning line is emitted through the injected message sink before
/// the existing shutdown-then-respawn fallback commits, and the
/// fallback outcome (`Recovered`) is unchanged from today's tests —
/// reuses [`spawn_fake_legacy_daemon_with_upgrade`]'s ignored-upgrade
/// configuration (the same fixture backing
/// `recover_from_legacy_daemon_falls_back_after_ignored_upgrade`).
#[cfg(unix)]
#[test]
fn recover_from_legacy_daemon_emits_fr5_warning_before_falling_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("fr5-warning.sock");
    let legacy = spawn_fake_legacy_daemon_with_upgrade(sock_path.clone(), false);

    let mut messages: Vec<String> = Vec::new();
    let result = daemon::recover_from_legacy_daemon_with(
        &sock_path,
        |_sock_path: &std::path::Path| crate::mux::identity::Verdict::Undecidable,
        |line: &str| messages.push(line.to_string()),
    );

    legacy.join().expect("fake legacy daemon thread panicked");

    match result {
        Ok(daemon::LegacyRecovery::Recovered) => {}
        other => panic!("expected Recovered (fallback after timeout), got {other:?}"),
    }
    assert!(
        messages.iter().any(|m| {
            m == "The running mux daemon predates in-place upgrade support; panes \
                  cannot be preserved and will be recreated."
        }),
        "expected the pinned FR5 warning line, got {messages:?}"
    );
}
