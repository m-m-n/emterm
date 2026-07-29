//! End-to-end integration test for the mux daemon hot-upgrade feature
//! (feature-docs/mux-daemon-hot-upgrade).
//!
//! Proves, against a REAL daemon process and a REAL shell running inside it,
//! the single claim the whole feature exists to make: an in-place upgrade
//! (`execve` handing the listen socket and every PTY master descriptor to a
//! freshly exec'd process) never kills a pane's shell and never loses state
//! a client can observe.
//!
//! ## Structure
//!
//! Follows `tests/mux_throughput.rs`'s established pattern: an isolated
//! `XDG_RUNTIME_DIR` per scenario so no scenario ever touches a real user
//! daemon, the daemon spawned as a real child process, a bounded readiness
//! poll on its Unix socket, raw `MuxMessage` frame helpers speaking the mux
//! wire protocol directly (no CLI subcommand involved — that surface is
//! task0005's own test responsibility), and an RAII cleanup guard that kills
//! the daemon and removes the isolated directory even when an assertion
//! panics.
//!
//! ## Acceptance criteria mapping (task0008.md)
//!
//! - AC-1 / AC-2: [`hot_upgrade_preserves_shell_pid_and_marker_file`]
//! - AC-3: [`hot_upgrade_logs_handoff_start_with_pane_count`]
//! - AC-4: [`hot_upgrade_succeeds_with_zero_panes`]
//! - AC-5: [`hot_upgrade_aborts_on_incompatible_schema_probe`]
//! - AC-6 (isolation + cleanup, including on failure) and AC-7 (every wait
//!   bounded, naming the stuck step) are cross-cutting properties of the
//!   shared helpers below ([`DaemonGuard`]'s `Drop`, and every polling loop
//!   taking an explicit timeout and panicking with the step name) rather
//!   than criteria a single scenario asserts on its own.
//! - AC-8: documented in `test/README.md`.
//!
//! ## Expected to fail until sibling tasks land
//!
//! At the time this file was written, task0003 (snapshot/restore),
//! task0004 (daemon upgrade branch + handoff startup) and task0005 (CLI
//! surface + the replacement itself) are still in progress on their own
//! branches. Every scenario below is written against the CONTRACTS those
//! tasks are building to (IMPLEMENTATION.md / SPEC.md), not against
//! already-landed behaviour, so every scenario is expected to fail — by
//! timing out at a clearly-named bounded wait — until the integration
//! branch carries that behaviour. This is intentional per task0008.md's
//! Test Notes ("do not weaken or delete a scenario to make it pass") and
//! must not be treated as a defect in this file.

#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use mux_ipc::protocol::{
    AttachMsg, ClientType, CreateWindowPayload, ErrorMsg, HelloMsg, MAX_FRAME_LENGTH, MessageType,
    MuxMessage, PROTOCOL_VERSION, WelcomeMsg,
};

// ---------------------------------------------------------------------------
// Bounded timeouts (AC-7): every wait below is one of these, never a bare
// sleep. Values are generous multiples of SPEC.md NFR3's "a few seconds at
// most" so a slow (but working) CI host never false-fails, while a truly
// stuck daemon still fails in well under a minute per step.
// ---------------------------------------------------------------------------

const SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PANE_CREATE_TIMEOUT: Duration = Duration::from_secs(5);
const SHELL_ROUNDTRIP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for the daemon to broadcast `Upgrading` (FR2) or close
/// the connection after receiving `Upgrade`, before concluding the request
/// was silently ignored.
const UPGRADE_SIGNAL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for a response to an `Upgrade` request that must be
/// REJECTED (FR13: "reported to the requesting client").
const UPGRADE_REJECTION_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to wait for the daemon to become reachable again (new
/// process, same socket) after a successful upgrade.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const LOG_READ_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Cleanup (AC-6)
// ---------------------------------------------------------------------------

/// RAII guard that kills the spawned daemon and removes the isolated
/// runtime directory on drop, so a panicking assertion never leaks a daemon
/// process or its temporary directory. Rust's default test harness unwinds
/// (not aborts) on a panicking assertion, so this `Drop` impl runs even when
/// a scenario fails partway through.
struct DaemonGuard {
    child: Child,
    runtime_dir: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

// ---------------------------------------------------------------------------
// Isolated daemon spawn
// ---------------------------------------------------------------------------

/// A fresh, collision-free isolated runtime directory for one scenario.
fn unique_runtime_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "emterm-mux-hotupgrade-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos()
    ))
}

/// The daemon socket path for a given `XDG_RUNTIME_DIR`, mirroring
/// `daemon::socket_path()` (`$XDG_RUNTIME_DIR/emterm/mux-default.sock`).
fn socket_path_for(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("emterm").join("mux-default.sock")
}

/// The daemon log path for a given `XDG_RUNTIME_DIR`, mirroring
/// `daemon::spawn_daemon`'s `sock_path.with_file_name("mux-daemon.log")`.
fn log_path_for(runtime_dir: &Path) -> PathBuf {
    socket_path_for(runtime_dir).with_file_name("mux-daemon.log")
}

/// Spawn `<exe> mux --daemon` against an isolated `XDG_RUNTIME_DIR`, with
/// stderr captured to the runtime dir's `mux-daemon.log` (so a scenario can
/// inspect handoff-startup logging — AC-3) and `SHELL` forced to `/bin/sh`
/// (dash: no per-user rc file, so the pane's shell is ready to receive input
/// immediately — see `tests/mux_throughput.rs`'s identical rationale; a
/// heavier interactive shell is not guaranteed ready within the daemon's
/// initial-command delay).
fn spawn_isolated_daemon(exe: &Path, runtime_dir: &Path) -> Child {
    let mux_dir = runtime_dir.join("emterm");
    std::fs::create_dir_all(&mux_dir).expect("create isolated mux runtime dir");
    {
        let mut perms = std::fs::metadata(&mux_dir)
            .expect("stat isolated mux runtime dir")
            .permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&mux_dir, perms).expect("restrict mux runtime dir permissions");
    }

    let log_path = log_path_for(runtime_dir);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("open isolated daemon log file");

    Command::new(exe)
        .args(["mux", "--daemon"])
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("SHELL", "/bin/sh")
        // Avoid the nesting guard / stray inherited mux env confusing the
        // freshly spawned daemon (mirrors tests/mux_throughput.rs).
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .spawn()
        .expect("spawn emterm mux --daemon")
}

/// Copy the built `emterm` test binary to a private, freely-mutable path
/// inside the scenario's own isolated runtime dir. Used only by the
/// incompatible-schema scenario, which must safely REPLACE the file its
/// daemon's own executable path resolves to — mutating the shared
/// `CARGO_BIN_EXE_emterm` artifact directly would corrupt every other
/// scenario (and any other test binary) reusing it.
fn copy_daemon_binary(dest: &Path) {
    std::fs::copy(env!("CARGO_BIN_EXE_emterm"), dest)
        .expect("copy the built emterm binary to a private, freely-mutable path");
    let mut perms = std::fs::metadata(dest)
        .expect("stat copied daemon binary")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(dest, perms).expect("mark copied daemon binary executable");
}

/// Atomically replace the file at `path` (which a running daemon's own
/// executable-path resolution points at) with a script that cannot serve as
/// a valid handoff candidate: it exits non-zero on any invocation.
///
/// This mirrors exactly how a package manager replaces an installed binary
/// (write new content under a temp name, then `rename(2)` over the old
/// name) rather than truncating the file in place, which the kernel refuses
/// (`ETXTBSY`) while a process is still executing it. After a `rename(2)`
/// replacement the daemon process already running from the OLD inode keeps
/// running completely unaffected (POSIX unlink/rename-while-open
/// semantics); only a FRESH resolution of `path` (which is exactly what the
/// daemon's schema probe / replacement-exec step performs, per SPEC.md
/// Security Considerations: "executes `self_exec::self_exe_path()` only")
/// observes the new, broken content. This is a black-box way to force the
/// pre-`execve` compatibility probe (IMPLEMENTATION.md D3) to reject the
/// candidate without needing to know its exact wire format, which is owned
/// by task0004/task0005 and not fixed at the time this file was written.
fn poison_binary_at(path: &Path) {
    let tmp_path = path.with_file_name(format!(
        "{}.replacement-tmp",
        path.file_name()
            .expect("poisoned path has a file name")
            .to_string_lossy()
    ));
    std::fs::write(&tmp_path, "#!/bin/sh\nexit 1\n")
        .expect("write replacement candidate binary");
    let mut perms = std::fs::metadata(&tmp_path)
        .expect("stat replacement candidate binary")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmp_path, perms)
        .expect("mark replacement candidate binary executable");
    std::fs::rename(&tmp_path, path).expect("atomically replace the daemon's on-disk binary");
}

// ---------------------------------------------------------------------------
// Raw frame helpers (mirrors tests/mux_throughput.rs)
// ---------------------------------------------------------------------------

fn write_frame(stream: &mut UnixStream, msg: &MuxMessage) -> std::io::Result<()> {
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()
}

fn read_frame(stream: &mut UnixStream) -> std::io::Result<MuxMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len == 0 || frame_len > MAX_FRAME_LENGTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid frame length {frame_len}"),
        ));
    }
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;
    MuxMessage::from_frame_body(&frame_buf)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame body"))
}

/// A frame read that timed out (would-block) rather than errored for real.
fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// The peer closed the connection (expected once `execve` drops accepted
/// connections per IMPLEMENTATION.md D2).
fn is_disconnect(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
    )
}

// ---------------------------------------------------------------------------
// Protocol-level steps
// ---------------------------------------------------------------------------

fn wait_for_socket(sock_path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !sock_path.exists() {
        if Instant::now() > deadline {
            panic!("daemon socket {sock_path:?} did not appear within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn connect(sock_path: &Path, timeout: Duration) -> UnixStream {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(sock_path) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("failed to connect to daemon socket {sock_path:?}: {e}"),
        }
    }
}

/// Send `Hello` and read back `Welcome`, panicking (naming the step) if the
/// handshake does not complete within [`HANDSHAKE_TIMEOUT`].
fn handshake(stream: &mut UnixStream) -> WelcomeMsg {
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .expect("set handshake read timeout");
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    write_frame(
        stream,
        &MuxMessage::control(MessageType::Hello, 0, &hello),
    )
    .expect("send Hello");
    let frame = read_frame(stream).expect("read Welcome frame within handshake timeout");
    assert_eq!(
        frame.msg_type,
        MessageType::Welcome,
        "expected Welcome, got {:?}",
        frame.msg_type
    );
    frame
        .decode_payload::<WelcomeMsg>()
        .expect("decode Welcome payload")
}

/// Total pane count across every session in a `WelcomeMsg::Accepted`.
fn total_pane_count(welcome: &WelcomeMsg) -> u32 {
    match welcome {
        WelcomeMsg::Accepted { sessions, .. } => sessions.iter().map(|s| s.pane_count).sum(),
        WelcomeMsg::Rejected { reason } => panic!("expected an accepted handshake, got: {reason}"),
    }
}

/// The id of the first (auto-created default) session in a
/// `WelcomeMsg::Accepted`.
fn first_session_id(welcome: &WelcomeMsg) -> u32 {
    match welcome {
        WelcomeMsg::Accepted { sessions, .. } => sessions
            .first()
            .unwrap_or_else(|| panic!("expected at least one session in Welcome, got none"))
            .id,
        WelcomeMsg::Rejected { reason } => panic!("expected an accepted handshake, got: {reason}"),
    }
}

/// Re-attach `stream` to `session_id` (mirrors the real bridge reconnect
/// flow, FR12) and wait, bounded by `timeout`, for the `PaneCreated` frame
/// the attach path (`ipc::reattach::send_reattach_data`) emits for
/// `pane_id`. Required before this (freshly (re)connected) connection will
/// receive `PtyOutput` for a pre-existing pane again — a bare Hello/Welcome
/// alone does not subscribe a connection to any pane's output.
fn attach_and_await_pane(stream: &mut UnixStream, session_id: u32, pane_id: u32, timeout: Duration) {
    write_frame(
        stream,
        &MuxMessage::control(MessageType::Attach, 0, &AttachMsg { session_id }),
    )
    .expect("send Attach");

    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set attach read timeout");
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            panic!(
                "timed out after {timeout:?} waiting for PaneCreated for pane {pane_id} after \
                 Attach to session {session_id}"
            );
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::PaneCreated && msg.pane_id == pane_id => {
                return;
            }
            Ok(msg) if msg.msg_type == MessageType::Error => {
                let err: Option<ErrorMsg> = msg.decode_payload();
                panic!("daemon returned Error attaching to session {session_id}: {err:?}");
            }
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => panic!("socket read error waiting for PaneCreated after Attach: {e}"),
        }
    }
}

/// Create a window with a bare interactive shell (no initial command — this
/// test drives the pane with its own `PtyInput` frames instead) and return
/// the new pane's id, bounded by [`PANE_CREATE_TIMEOUT`].
fn create_shell_pane(stream: &mut UnixStream) -> u32 {
    let payload = CreateWindowPayload {
        name: Some("hotupgrade".to_string()),
        command: None,
    };
    write_frame(
        stream,
        &MuxMessage::control(MessageType::CreateWindow, 0, &payload),
    )
    .expect("send CreateWindow");

    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set pane-create read timeout");
    let deadline = Instant::now() + PANE_CREATE_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            panic!("timed out after {PANE_CREATE_TIMEOUT:?} waiting for PaneCreated");
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::PaneCreated => return msg.pane_id,
            Ok(msg) if msg.msg_type == MessageType::Error => {
                let err: Option<ErrorMsg> = msg.decode_payload();
                panic!("daemon returned Error creating window: {err:?}");
            }
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => panic!("socket read error waiting for PaneCreated: {e}"),
        }
    }
}

/// Type `line` (plus a trailing newline, i.e. Enter) into `pane_id`.
fn send_pane_line(stream: &mut UnixStream, pane_id: u32, line: &str) {
    let mut data = line.as_bytes().to_vec();
    data.push(b'\n');
    write_frame(stream, &MuxMessage::pty_input(pane_id, data)).expect("send PtyInput");
}

/// Read `PtyOutput` frames tagged with `pane_id` until `needle` appears in
/// the accumulated text, bounded by `timeout`. Panics naming `step_name` on
/// timeout, on the pane exiting first, or on a daemon `Error` frame — never
/// hangs (AC-7).
fn read_pane_until(
    stream: &mut UnixStream,
    pane_id: u32,
    needle: &str,
    step_name: &str,
    timeout: Duration,
) -> String {
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set pane read timeout");
    let mut collected = String::new();
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            panic!(
                "{step_name}: timed out after {timeout:?} waiting for {needle:?} in pane {pane_id} \
                 output (collected so far: {collected:?})"
            );
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::PtyOutput && msg.pane_id == pane_id => {
                collected.push_str(&String::from_utf8_lossy(&msg.payload));
                if collected.contains(needle) {
                    return collected;
                }
            }
            Ok(msg) if msg.msg_type == MessageType::PtyExited && msg.pane_id == pane_id => {
                panic!(
                    "{step_name}: pane {pane_id} exited before {needle:?} appeared \
                     (collected: {collected:?})"
                );
            }
            Ok(msg) if msg.msg_type == MessageType::Error => {
                let err: Option<ErrorMsg> = msg.decode_payload();
                panic!("{step_name}: daemon returned Error while waiting for {needle:?}: {err:?}");
            }
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => panic!("{step_name}: socket read error: {e}"),
        }
    }
}

/// Extract the first run of ASCII digits that immediately follows an
/// occurrence of `prefix` in `haystack`, skipping occurrences with no
/// digits right after them.
///
/// This is needed because the PTY line discipline echoes typed input back
/// verbatim (see `tests/mux_throughput.rs`'s identical observation): the
/// command text this file types contains `prefix` followed by the LITERAL,
/// unexpanded `printf` format specifier (no digits), and only the shell's
/// REAL evaluated output line has `prefix` followed by actual PID digits.
/// Skipping non-digit occurrences instead of matching the first occurrence
/// blindly is what makes this robust to that echo.
fn extract_pid_after(haystack: &str, prefix: &str) -> u32 {
    let mut search_from = 0usize;
    while let Some(rel_idx) = haystack[search_from..].find(prefix) {
        let idx = search_from + rel_idx;
        let rest = &haystack[idx + prefix.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            return digits.parse().expect("validated ASCII digits");
        }
        search_from = idx + prefix.len();
    }
    panic!("no {prefix:?} followed by digits found in {haystack:?}");
}

/// Send a bare `Upgrade` request (empty payload, pane id zero — mirrors
/// `Shutdown`'s wire shape exactly, per IMPLEMENTATION.md) and wait, bounded
/// by `timeout`, for either the `Upgrading` broadcast (FR2) or the
/// connection closing (also an acceptable sign the daemon acted, since
/// accepted connections are dropped across the replacement per D2).
///
/// Panics naming the step on timeout: a daemon that silently discards
/// `Upgrade` (today's behaviour before task0004/task0005 land — the
/// message type decodes fine since task0001 merged it into `mux_ipc`, but
/// nothing in the currently-merged connection layer acts on it yet) must
/// fail this wait rather than let the rest of a scenario pass vacuously.
fn send_upgrade_and_await_signal(stream: &mut UnixStream, timeout: Duration) {
    write_frame(
        stream,
        &MuxMessage {
            msg_type: MessageType::Upgrade,
            pane_id: 0,
            payload: Vec::new(),
        },
    )
    .expect("send Upgrade");

    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set upgrade-signal read timeout");
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            panic!(
                "timed out after {timeout:?} waiting for the daemon to react to Upgrade \
                 (no Upgrading broadcast and the connection never closed)"
            );
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::Upgrading => return,
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) if is_disconnect(&e) => return,
            Err(e) => panic!("unexpected socket error waiting for Upgrading: {e}"),
        }
    }
}

/// Wait, bounded by `timeout`, for an `Error` frame on `stream` after an
/// `Upgrade` request that is expected to be REJECTED before ever reaching
/// `execve` (FR13: aborted upgrades are "reported to the requesting
/// client", the same established channel `handle_create_window` /
/// `handle_attach` already use for a failure reported over the requesting
/// connection). Returns `false` (never panics) on timeout, or if the
/// daemon instead broadcasts `Upgrading` (meaning it proceeded rather than
/// aborted — not this scenario), so the caller can assert with a message
/// naming exactly what happened.
///
/// Deliberately does NOT treat every OTHER frame type as evidence: this
/// connection has a live pane attached and an active session, either of
/// which can emit completely unsolicited traffic with no relation to the
/// `Upgrade` request at all (observed in practice: a queued `StatusUpdate`
/// left over from pane creation, drained only once this function's next
/// read call reached it). Only `Error` is treated as a positive signal;
/// everything else (including `Upgrading`'s absence) is ignored and the
/// wait continues.
fn await_upgrade_rejection(stream: &mut UnixStream, timeout: Duration) -> bool {
    write_frame(
        stream,
        &MuxMessage {
            msg_type: MessageType::Upgrade,
            pane_id: 0,
            payload: Vec::new(),
        },
    )
    .expect("send Upgrade");

    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set upgrade-rejection read timeout");
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            return false;
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::Upgrading => return false,
            Ok(msg) if msg.msg_type == MessageType::Error => return true,
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(_) => return false,
        }
    }
}

/// Poll (bounded by `timeout`) until a fresh connection to `sock_path`
/// completes the handshake again, returning that connection and its
/// `Welcome` payload. Used after a successful upgrade, when the OLD
/// connection has been dropped per D2 and a NEW one must be opened.
fn await_daemon_reachable_again(sock_path: &Path, timeout: Duration) -> (UnixStream, WelcomeMsg) {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            panic!(
                "daemon at {sock_path:?} did not become reachable again within {timeout:?} \
                 after the upgrade"
            );
        }
        if let Ok(mut s) = UnixStream::connect(sock_path) {
            s.set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set post-upgrade handshake read timeout");
            let hello = HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            };
            if write_frame(
                &mut s,
                &MuxMessage::control(MessageType::Hello, 0, &hello),
            )
            .is_ok()
            {
                if let Ok(frame) = read_frame(&mut s) {
                    if frame.msg_type == MessageType::Welcome {
                        if let Some(welcome) = frame.decode_payload::<WelcomeMsg>() {
                            return (s, welcome);
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Read a file's contents, retrying (bounded by `timeout`) until it is
/// non-empty. Used for the daemon log, which a freshly exec'd process may
/// take a brief moment to flush after its socket starts answering again.
fn read_file_with_retry(path: &Path, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.is_empty() {
                return content;
            }
        }
        if Instant::now() > deadline {
            return std::fs::read_to_string(path).unwrap_or_default();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// AC-1 / AC-2 (main scenario): a shell's own PID survives the upgrade
/// unchanged, and a file it created beforehand is still observable
/// afterward FROM THAT SAME SHELL (not merely present on the host
/// filesystem — the shell itself is asked to check, and the answer is read
/// back through the pane's output, per task0008.md's Design section).
#[test]
fn hot_upgrade_preserves_shell_pid_and_marker_file() {
    let runtime_dir = unique_runtime_dir("main");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_emterm"));
    let child = spawn_isolated_daemon(&exe, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);
    let mut stream = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let _welcome = handshake(&mut stream);

    let pane_id = create_shell_pane(&mut stream);

    let marker_path = runtime_dir.join("hotupgrade-marker");
    send_pane_line(
        &mut stream,
        pane_id,
        &format!(
            "printf 'EMTERM_HOTUPG_PID:%s\\n' \"$$\"; touch '{}'; printf '%s%s\\n' EMTERM_HOTUPG _BEFOREDONE",
            marker_path.display()
        ),
    );
    let before = read_pane_until(
        &mut stream,
        pane_id,
        "EMTERM_HOTUPG_BEFOREDONE",
        "pre-upgrade shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_before = extract_pid_after(&before, "EMTERM_HOTUPG_PID:");

    send_upgrade_and_await_signal(&mut stream, UPGRADE_SIGNAL_TIMEOUT);

    let (mut stream2, welcome2) = await_daemon_reachable_again(&sock_path, RECONNECT_TIMEOUT);
    let session_id = first_session_id(&welcome2);
    attach_and_await_pane(&mut stream2, session_id, pane_id, RECONNECT_TIMEOUT);
    send_pane_line(
        &mut stream2,
        pane_id,
        &format!(
            "printf 'EMTERM_HOTUPG_PID:%s\\n' \"$$\"; if [ -f '{}' ]; then printf '%s%s\\n' EMTERM_HOTUPG _MARKERPRESENT; else printf '%s%s\\n' EMTERM_HOTUPG _MARKERABSENT; fi; printf '%s%s\\n' EMTERM_HOTUPG _AFTERDONE",
            marker_path.display()
        ),
    );
    let after = read_pane_until(
        &mut stream2,
        pane_id,
        "EMTERM_HOTUPG_AFTERDONE",
        "post-upgrade shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_after = extract_pid_after(&after, "EMTERM_HOTUPG_PID:");

    assert_eq!(
        pid_before, pid_after,
        "AC-1: the pane's shell PID must be unchanged across the hot upgrade \
         (before={pid_before}, after={pid_after})"
    );
    assert!(
        after.contains("EMTERM_HOTUPG_MARKERPRESENT"),
        "AC-2: a file created before the upgrade must still be observable from the same shell \
         afterward; got: {after:?}"
    );
}

/// AC-3: after a successful upgrade, the daemon's log contains a
/// handoff-start entry distinguishable from a normal start, including the
/// number of panes adopted (FR11).
#[test]
fn hot_upgrade_logs_handoff_start_with_pane_count() {
    let runtime_dir = unique_runtime_dir("handoff-log");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_emterm"));
    let child = spawn_isolated_daemon(&exe, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    let log_path = log_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);
    let mut stream = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let _welcome = handshake(&mut stream);

    // Exactly one pane, so the expected adopted-pane count is unambiguous.
    let _pane_id = create_shell_pane(&mut stream);

    send_upgrade_and_await_signal(&mut stream, UPGRADE_SIGNAL_TIMEOUT);
    let (_stream2, _welcome2) = await_daemon_reachable_again(&sock_path, RECONNECT_TIMEOUT);

    let log_text = read_file_with_retry(&log_path, LOG_READ_TIMEOUT);
    let lower = log_text.to_ascii_lowercase();
    let handoff_idx = lower.find("handoff").unwrap_or_else(|| {
        panic!(
            "AC-3: expected a log line distinguishing handoff startup (containing \"handoff\", \
             per IMPLEMENTATION.md's naming convention); log contents: {log_text:?}"
        )
    });

    // Evidence of the adopted pane count is expected at or after the
    // handoff-start mention (never before it, since a normal-start log
    // cannot itself contain a "handoff" mention to begin with).
    let suffix = &log_text[handoff_idx..];
    let suffix_lower = &lower[handoff_idx..];
    assert!(
        suffix_lower.contains("pane"),
        "AC-3: expected the handoff-start log entry to mention pane counts; log tail: {suffix:?}"
    );
    let mentions_one_pane = suffix
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| tok == "1");
    assert!(
        mentions_one_pane,
        "AC-3: expected the handoff-start log entry to include the adopted pane count (1 pane \
         was created before the upgrade); log tail: {suffix:?}"
    );
}

/// AC-4: upgrading a daemon with zero panes succeeds, and the daemon still
/// answers a handshake afterwards.
#[test]
fn hot_upgrade_succeeds_with_zero_panes() {
    let runtime_dir = unique_runtime_dir("zero-panes");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_emterm"));
    let child = spawn_isolated_daemon(&exe, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);
    let mut stream = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let welcome = handshake(&mut stream);
    assert_eq!(
        total_pane_count(&welcome),
        0,
        "AC-4 setup: expected zero panes before the upgrade (no window was created)"
    );

    send_upgrade_and_await_signal(&mut stream, UPGRADE_SIGNAL_TIMEOUT);

    let (_stream2, welcome2) = await_daemon_reachable_again(&sock_path, RECONNECT_TIMEOUT);
    match welcome2 {
        WelcomeMsg::Accepted { .. } => {}
        WelcomeMsg::Rejected { reason } => {
            panic!("AC-4: daemon rejected handshake after a zero-pane upgrade: {reason}")
        }
    }
}

/// AC-5: an upgrade whose candidate binary cannot answer a valid handoff
/// schema probe aborts, and the original daemon keeps serving with its
/// pane still live (never killed — NFR2).
#[test]
fn hot_upgrade_aborts_on_incompatible_schema_probe() {
    let runtime_dir = unique_runtime_dir("incompatible-schema");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    // A private copy this scenario can safely mutate (see `poison_binary_at`
    // for why this simulates an incompatible candidate binary).
    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);
    let mut stream = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let _welcome = handshake(&mut stream);

    let pane_id = create_shell_pane(&mut stream);
    send_pane_line(
        &mut stream,
        pane_id,
        "printf 'EMTERM_HOTUPG_PID:%s\\n' \"$$\"; printf '%s%s\\n' EMTERM_HOTUPG _BEFOREDONE",
    );
    let before = read_pane_until(
        &mut stream,
        pane_id,
        "EMTERM_HOTUPG_BEFOREDONE",
        "pre-upgrade shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_before = extract_pid_after(&before, "EMTERM_HOTUPG_PID:");

    poison_binary_at(&daemon_bin_path);

    let rejected = await_upgrade_rejection(&mut stream, UPGRADE_REJECTION_TIMEOUT);
    assert!(
        rejected,
        "AC-5: expected the daemon to report the rejected upgrade back over the requesting \
         connection (FR13) instead of staying silent or proceeding with Upgrading"
    );

    let (mut stream2, welcome2) = await_daemon_reachable_again(&sock_path, RECONNECT_TIMEOUT);
    let pane_count = total_pane_count(&welcome2);
    assert!(
        pane_count >= 1,
        "AC-5: expected the pane created before the aborted upgrade to still be counted, got {pane_count}"
    );
    let session_id = first_session_id(&welcome2);
    attach_and_await_pane(&mut stream2, session_id, pane_id, RECONNECT_TIMEOUT);

    send_pane_line(
        &mut stream2,
        pane_id,
        "printf 'EMTERM_HOTUPG_PID:%s\\n' \"$$\"; printf '%s%s\\n' EMTERM_HOTUPG _AFTERDONE",
    );
    let after = read_pane_until(
        &mut stream2,
        pane_id,
        "EMTERM_HOTUPG_AFTERDONE",
        "post-abort shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_after = extract_pid_after(&after, "EMTERM_HOTUPG_PID:");
    assert_eq!(
        pid_before, pid_after,
        "AC-5 / NFR2: the pane's shell must not be killed by an aborted upgrade \
         (pid before={pid_before}, after={pid_after})"
    );
}
