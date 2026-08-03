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
///
/// task0004 (mux-daemon-binary-update-detect, NFR3): creates the directory
/// now and hardens its permission bits to `0o755` explicitly, rather than
/// leaving creation to whichever caller's own `create_dir_all` happens to
/// run first (harmless -- `create_dir_all` is idempotent, so every existing
/// call site's own `create_dir_all(&runtime_dir)` still succeeds unchanged).
/// A daemon candidate binary's PARENT directory is one of NFR3's own
/// validation targets (no group/world write); relying on the invoking
/// process's ambient umask to produce a conforming mode is exactly the kind
/// of environmental nondeterminism the project's test discipline avoids -- a
/// permissive umask (e.g. `002`) would otherwise make this shared isolated
/// directory group-writable, and every scenario that replaces the daemon
/// binary directly inside it (several pre-existing scenarios above) would
/// then be refused by the validation gate this task adds, even though
/// nothing about the SCENARIO itself is wrong.
fn unique_runtime_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "emterm-mux-hotupgrade-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create isolated runtime dir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
        .expect("harden isolated runtime dir to a conforming (non-group/world-writable) mode");
    dir
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

/// Place a copy of the built `emterm` test binary at `dest`, a private path
/// inside the scenario's own isolated runtime dir, so DaemonGuard::drop
/// reclaims it along with the rest of that scenario's runtime dir -- no
/// process-lifetime-scoped shared path, no leak.
///
/// Prefers `std::fs::hard_link` over a full copy to avoid paying for a
/// (debug-build) multi-hundred-MB copy into tmpfs per scenario -- a
/// scenario that later rename(2)-replaces the file at `dest` (see
/// `poison_binary_at` / `replace_binary_with_valid_copy`) only unlinks the
/// hard-linked directory entry, leaving the original `CARGO_BIN_EXE_emterm`
/// inode (and any other scenario's own hard link to it) untouched. Falls
/// back to `std::fs::copy` only if the hard link fails (e.g. `dest` is on a
/// different filesystem than the cargo build output).
fn copy_daemon_binary(dest: &Path) {
    let source = env!("CARGO_BIN_EXE_emterm");
    if std::fs::hard_link(source, dest).is_err() {
        std::fs::copy(source, dest)
            .expect("copy the built emterm binary to a private, freely-mutable path");
    }
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
    // task0004 (NFR3): spawn from a private copy inside this scenario's own
    // isolated runtime dir rather than the shared `CARGO_BIN_EXE_emterm` path
    // directly -- that shared cargo build output directory's permission bits
    // are outside this test's control (e.g. an ambient umask of `002` leaves
    // it group-writable), which would make the daemon's own self-upgrade
    // candidate fail NFR3's parent-directory rule for reasons having nothing
    // to do with this scenario. This scenario never mutates the binary it
    // spawns from, but still uses its own copy (hard-linked where possible,
    // see `copy_daemon_binary`) so cleanup is handled by `DaemonGuard::drop`
    // like every other scenario, with no process-lifetime-scoped shared path.
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
    // task0004 (NFR3): see the identical rationale in
    // `hot_upgrade_preserves_shell_pid_and_marker_file` above; this scenario
    // also never mutates the binary it spawns from, but still uses its own
    // private copy in its own isolated runtime dir.
    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);
    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
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
    // task0004 (NFR3): see the identical rationale in
    // `hot_upgrade_preserves_shell_pid_and_marker_file` above; this scenario
    // also never mutates the binary it spawns from, but still uses its own
    // private copy in its own isolated runtime dir.
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

// =============================================================================
// task0003 extension (feature-docs/mux-daemon-binary-update-detect)
// =============================================================================
//
// Everything above this point is unchanged, pre-existing coverage for the
// mux-daemon-hot-upgrade feature (task0008.md / AC-1..AC-8). The scenarios
// below prove a DIFFERENT, later feature: that a rename-replaced daemon
// binary is itself DETECTED and triggers the SAME hot-upgrade machinery,
// via BOTH CLI entry points (`emterm mux attach` and the
// `ensure_daemon_running` probe behind `emterm mux` / `emterm mux script`),
// driven by an identity file (`mux-identity.txt`) the daemon records at
// startup (task0001's `mux::identity` module) and a trigger wired into the
// shared recovery probe's Compatible arm (task0002, IMPLEMENTATION.md
// D1/D5).
//
// ## Acceptance criteria mapping (task0003.md, mux-daemon-binary-update-detect)
//
// - AC-1: [`hot_upgrade_fires_on_binary_replacement_via_attach`] (TS-3)
// - AC-2: [`hot_upgrade_no_churn_when_binary_unchanged`] (TS-4)
// - AC-3: [`hot_upgrade_fires_on_binary_replacement_via_mux_start`] (TS-5)
// - AC-4: [`hot_upgrade_no_misfire_when_identity_file_absent`] (TS-7)
// - AC-5 (every new wait bounded and naming the stuck step; isolated
//   runtime dir + RAII cleanup that also terminates any spawned
//   attach/script child on failure) is a cross-cutting property of the
//   helpers below, exactly like the pre-existing file's own AC-6/AC-7 note
//   at the top of this file.
// - AC-6 (pre-existing scenarios textually untouched) is satisfied by this
//   entire section being purely additive.
//
// ## Expected to fail until task0001/task0002 land
//
// TS-3 and TS-5 are written against the IMPLEMENTATION.md contract for a
// mechanism (`mux::identity`, the Compatible-arm trigger) that does not
// exist yet on this branch at the time this section was written: they are
// expected to fail by timing out waiting for the pinned notice line, per
// this feature's own task0003.md Design section ("do not weaken or delete
// a scenario to make it pass"). TS-4 and TS-7 are absence assertions --
// they may legitimately already pass before the sibling tasks land (there
// is nothing yet that COULD misfire); their value is as a regression guard
// once the identity check exists, not as a red-before-merge proof.

/// How long to wait for the pinned replacement notice line (FR2/D5,
/// IMPLEMENTATION.md Shared Components) to appear on an attaching/starting
/// client's standard error after a binary replacement -- generous enough to
/// cover probe + `Upgrade` round-trip + `execve` + reachability recheck
/// (mirrors [`RECONNECT_TIMEOUT`]'s budget) plus margin for the client's
/// own post-reachability print.
const NOTICE_LINE_TIMEOUT: Duration = Duration::from_secs(25);
/// Bounded observation window for a no-churn scenario (TS-4/TS-7): long
/// enough for a real (incorrect) fire to have shown itself -- the
/// underlying identity check is a single small-file read plus one stat
/// (NFR1), so any genuine fire announces itself within, at most, a couple
/// of seconds -- but short enough to keep the suite fast.
const NO_CHURN_OBSERVATION_WINDOW: Duration = Duration::from_secs(8);
/// How long to wait for the `mux script` vehicle (TS-5) to exit -- it does
/// no bridging, so exit after the probe completes should be near-instant.
const SCRIPT_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// The pinned replacement notice line (FR2/D5, IMPLEMENTATION.md Shared
/// Components: "Replacement notice line"), printed to standard error by
/// whichever client's probe triggered a successful in-place binary-update
/// upgrade.
const UPGRADE_NOTICE_LINE: &str = "Mux daemon upgraded in place to the newly installed binary";

/// RAII guard that kills a spawned attach/script vehicle child (this
/// section's own subprocesses) on drop -- kept independent of
/// [`DaemonGuard`] so the pre-existing struct and its four scenarios stay
/// textually untouched (AC-6).
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The daemon's recorded-identity file path for a given `XDG_RUNTIME_DIR`
/// (IMPLEMENTATION.md Shared Components: the sibling file named
/// `mux-identity.txt` in the socket's own directory). Consumed here only to
/// DELETE it (TS-7) -- everything else about its format/writer is
/// task0001's own module, out of this file's black-box scope.
fn identity_path_for(runtime_dir: &Path) -> PathBuf {
    socket_path_for(runtime_dir).with_file_name("mux-identity.txt")
}

/// Atomically replace the file at `path` (which a running daemon's own
/// executable-path resolution points at) with a FRESH, VALID copy of the
/// built `emterm` test binary -- the "compatible candidate" counterpart of
/// [`poison_binary_at`]. Uses the identical write-to-a-temp-name-then-
/// `rename(2)` technique (mirrors a package manager's real install), so the
/// currently running daemon (holding the OLD, now-unlinked inode) keeps
/// running completely unaffected while a FRESH resolution of `path` (the
/// daemon's own upgrade-exec step) observes the new content -- see
/// `poison_binary_at`'s doc comment for the full rename-while-open
/// semantics this relies on.
fn replace_binary_with_valid_copy(path: &Path) {
    let tmp_path = path.with_file_name(format!(
        "{}.replacement-tmp",
        path.file_name()
            .expect("replacement path has a file name")
            .to_string_lossy()
    ));
    std::fs::copy(env!("CARGO_BIN_EXE_emterm"), &tmp_path)
        .expect("copy the built emterm binary to a private, freely-mutable path");
    let mut perms = std::fs::metadata(&tmp_path)
        .expect("stat replacement candidate binary")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmp_path, perms)
        .expect("mark replacement candidate binary executable");
    std::fs::rename(&tmp_path, path).expect("atomically replace the daemon's on-disk binary");
}

/// Whether `pid`'s own executable-path resolution currently reports the
/// kernel's dangling-inode "(deleted)" suffix (Linux `/proc/<pid>/exe`,
/// after a `rename(2)` replacement leaves a still-running process mapped to
/// an unlinked inode -- see `self_exec.rs`'s module doc for the same
/// mechanism from the GUI side). Panics if the link cannot be read at all,
/// since a live process's own `/proc/<pid>/exe` must always be readable by
/// its own user; a read failure here means the test's assumption about
/// which process is being inspected is already wrong.
fn exe_link_reports_deleted(pid: u32) -> bool {
    let target = std::fs::read_link(format!("/proc/{pid}/exe"))
        .unwrap_or_else(|e| panic!("failed to read /proc/{pid}/exe: {e}"));
    target.to_string_lossy().ends_with(" (deleted)")
}

/// Spawn a background thread that reads `stderr` to completion into a
/// shared, growable buffer the caller can poll without blocking --
/// `std::process::ChildStderr` has no read-timeout API (unlike the raw
/// socket helpers above), so a bounded wait for a line on it must offload
/// the blocking read to its own thread.
fn spawn_stderr_collector(
    mut stderr: std::process::ChildStderr,
) -> std::sync::Arc<std::sync::Mutex<String>> {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let buf_writer = std::sync::Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&chunk[..n]).into_owned();
                    buf_writer
                        .lock()
                        .expect("stderr collector lock")
                        .push_str(&text);
                }
                Err(_) => break,
            }
        }
    });
    buf
}

/// Wait, bounded by `timeout`, for `needle` to appear in a stderr buffer
/// being filled by [`spawn_stderr_collector`]. Panics naming `step_name` on
/// timeout.
fn wait_for_stderr_needle(
    buf: &std::sync::Arc<std::sync::Mutex<String>>,
    needle: &str,
    step_name: &str,
    timeout: Duration,
) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let text = buf.lock().expect("stderr collector lock");
            if text.contains(needle) {
                return text.clone();
            }
        }
        if Instant::now() > deadline {
            let text = buf.lock().expect("stderr collector lock").clone();
            panic!(
                "{step_name}: timed out after {timeout:?} waiting for {needle:?} on the child's \
                 standard error (collected so far: {text:?})"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Poll (bounded by `timeout`) until `child` has exited, returning its
/// `ExitStatus`. Panics naming `step_name` if it is still running once the
/// deadline passes -- never calls the blocking `Child::wait` directly, so a
/// stuck vehicle process cannot hang the test suite (AC-5/AC-7).
fn wait_for_child_exit(
    child: &mut Child,
    step_name: &str,
    timeout: Duration,
) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("poll child exit status") {
            return status;
        }
        if Instant::now() > deadline {
            panic!("{step_name}: child process did not exit within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Watch `stream` (a raw connection already registered as a GUI client) for
/// an [`MessageType::Upgrading`] broadcast for `window`, panicking naming
/// `step_name` immediately if it arrives, or if the connection closes early
/// (also a sign the daemon proceeded with a replacement -- accepted
/// connections are dropped across one, IMPLEMENTATION.md D2). A plain
/// timeout with neither is the expected PASSING outcome for a no-churn
/// scenario (TS-4/TS-7): nothing should have happened at all.
fn assert_no_misfire_signal(stream: &mut UnixStream, step_name: &str, window: Duration) {
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set no-misfire observation read timeout");
    let deadline = Instant::now() + window;
    loop {
        if Instant::now() > deadline {
            return;
        }
        match read_frame(stream) {
            Ok(msg) if msg.msg_type == MessageType::Upgrading => panic!(
                "{step_name}: unexpected Upgrading broadcast observed (nothing should have \
                 triggered an upgrade in this scenario)"
            ),
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) if is_disconnect(&e) => panic!(
                "{step_name}: observer connection closed unexpectedly (a sign the daemon \
                 proceeded with a replacement it should not have): {e}"
            ),
            Err(e) => panic!("{step_name}: unexpected socket error during observation: {e}"),
        }
    }
}

/// Assert the daemon log at `log_path` contains no handoff-start marker --
/// the absence counterpart to the pre-existing
/// `hot_upgrade_logs_handoff_start_with_pane_count` scenario's positive
/// check of the same marker.
fn assert_log_has_no_handoff_marker(log_path: &Path, step_name: &str) {
    let log_text = std::fs::read_to_string(log_path).unwrap_or_default();
    assert!(
        !log_text.to_ascii_lowercase().contains("handoff"),
        "{step_name}: expected no handoff-start log entry; log contents: {log_text:?}"
    );
}

/// AC-1 (task0003 TS-3, this feature's main-line scenario): a rename-
/// replaced daemon binary triggers an in-place upgrade via the `emterm mux
/// attach` entry point. Asserts all four outcomes AC-1 names: the upgrade
/// fired (proven by the notice line and the handoff-start log marker), the
/// pane's shell PID is unchanged and alive, the daemon is running the NEW
/// image (its executable-path resolution no longer reports the kernel's
/// dangling-inode "(deleted)" marker -- D3/AC-6), and the pinned
/// replacement notice line (FR2/D5) appears on the attaching client's
/// standard error.
///
/// Expected to fail (time out waiting for the notice line) until
/// task0001/task0002 land on the integration branch -- see this section's
/// header note above. Do not weaken this wait to pass early.
#[test]
fn hot_upgrade_fires_on_binary_replacement_via_attach() {
    let runtime_dir = unique_runtime_dir("attach-replace");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let daemon_pid = child.id();
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    let log_path = log_path_for(&runtime_dir);
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
        "TS-3 pre-replacement shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_before = extract_pid_after(&before, "EMTERM_HOTUPG_PID:");

    assert!(
        !exe_link_reports_deleted(daemon_pid),
        "test setup sanity: the daemon's own executable link must resolve cleanly before the \
         replacement"
    );
    replace_binary_with_valid_copy(&daemon_bin_path);
    assert!(
        exe_link_reports_deleted(daemon_pid),
        "test setup sanity: after a rename(2) replacement the daemon's own executable-path \
         resolution must report the kernel's dangling-inode \"(deleted)\" marker until it \
         re-execs"
    );

    let mut attach_child = Command::new(&daemon_bin_path)
        .args(["mux", "attach"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn `<new binary> mux attach`");
    let attach_stderr = attach_child
        .stderr
        .take()
        .expect("attach child stderr was piped");
    let stderr_buf = spawn_stderr_collector(attach_stderr);
    let _attach_guard = ChildGuard(attach_child);

    let notice = wait_for_stderr_needle(
        &stderr_buf,
        UPGRADE_NOTICE_LINE,
        "TS-3: waiting for the replacement notice line on the attach client's stderr",
        NOTICE_LINE_TIMEOUT,
    );
    assert!(
        notice.contains(UPGRADE_NOTICE_LINE),
        "AC-1: expected the pinned notice line on the attach client's standard error; got: \
         {notice:?}"
    );

    assert!(
        !exe_link_reports_deleted(daemon_pid),
        "AC-1: expected the daemon's executable-path resolution to no longer report \"(deleted)\" \
         after a successful in-place upgrade (the new image must be running)"
    );

    let log_text = read_file_with_retry(&log_path, LOG_READ_TIMEOUT);
    assert!(
        log_text.to_ascii_lowercase().contains("handoff"),
        "AC-1: expected the daemon log to contain a handoff-start marker after the upgrade; log \
         contents: {log_text:?}"
    );

    let (mut stream2, welcome2) = await_daemon_reachable_again(&sock_path, RECONNECT_TIMEOUT);
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
        "TS-3 post-replacement shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_after = extract_pid_after(&after, "EMTERM_HOTUPG_PID:");
    assert_eq!(
        pid_before, pid_after,
        "AC-1: the pane's shell PID must be unchanged and alive across the binary-replacement \
         upgrade (before={pid_before}, after={pid_after})"
    );
}

/// AC-2 (task0003 TS-4): an UNCHANGED on-disk binary never fires an
/// upgrade. A raw-frame observer connection, registered as a GUI client
/// BEFORE the vehicle runs (so it would see an `Upgrading` broadcast if one
/// fired, regardless of which client's probe caused it -- the daemon
/// broadcasts to every connected GUI client, not just the requester),
/// watches for a misfire while `mux script` drives the exact
/// `ensure_daemon_running` probe the real `emterm mux` / `emterm mux
/// script` entry points use (Test Notes: preferred over `attach` here
/// since this scenario has no need for the long-running bridge).
///
/// This is an absence assertion (FR3/SPEC AC-4): it may legitimately
/// already pass before task0001/task0002 land (nothing yet exists that
/// COULD misfire) -- see this section's header note above.
#[test]
fn hot_upgrade_no_churn_when_binary_unchanged() {
    let runtime_dir = unique_runtime_dir("no-churn");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    // task0004 (NFR3): see the identical rationale in
    // `hot_upgrade_preserves_shell_pid_and_marker_file` above; this scenario
    // never mutates the binary it spawns from, but still uses its own
    // private copy in its own isolated runtime dir.
    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    let log_path = log_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);

    let mut observer = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let _welcome = handshake(&mut observer);

    let vehicle = Command::new(&daemon_bin_path)
        .args(["mux", "script"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn `mux script` vehicle");
    let mut vehicle_guard = ChildGuard(vehicle);
    let status = wait_for_child_exit(
        &mut vehicle_guard.0,
        "TS-4 no-churn: waiting for the `mux script` vehicle to exit",
        SCRIPT_EXIT_TIMEOUT,
    );
    assert!(
        status.success(),
        "AC-2 setup: expected the `mux script` vehicle to exit successfully against an \
         unchanged, reachable daemon (exit status: {status:?})"
    );

    assert_no_misfire_signal(&mut observer, "TS-4 no-churn", NO_CHURN_OBSERVATION_WINDOW);
    assert_log_has_no_handoff_marker(&log_path, "TS-4 no-churn");
}

/// AC-3 (task0003 TS-5): the `ensure_daemon_running` probe -- the vehicle
/// behind `emterm mux` / `emterm mux script`, sharing the SAME Compatible-
/// arm trigger as the attach path (IMPLEMENTATION.md D1) -- fires the
/// identical binary-replacement upgrade, including the pinned notice line,
/// proving the detection is not attach-specific.
///
/// Expected to fail (time out waiting for the notice line) until
/// task0001/task0002 land on the integration branch -- see this section's
/// header note above. Do not weaken this wait to pass early.
#[test]
fn hot_upgrade_fires_on_binary_replacement_via_mux_start() {
    let runtime_dir = unique_runtime_dir("mux-start-replace");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let daemon_pid = child.id();
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    let log_path = log_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);

    assert!(
        !exe_link_reports_deleted(daemon_pid),
        "test setup sanity: the daemon's own executable link must resolve cleanly before the \
         replacement"
    );
    replace_binary_with_valid_copy(&daemon_bin_path);
    assert!(
        exe_link_reports_deleted(daemon_pid),
        "test setup sanity: after a rename(2) replacement the daemon's own executable-path \
         resolution must report the kernel's dangling-inode \"(deleted)\" marker until it \
         re-execs"
    );

    let mut vehicle_child = Command::new(&daemon_bin_path)
        .args(["mux", "script"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn `<new binary> mux script`");
    let vehicle_stderr = vehicle_child
        .stderr
        .take()
        .expect("vehicle stderr was piped");
    let stderr_buf = spawn_stderr_collector(vehicle_stderr);
    let mut vehicle_guard = ChildGuard(vehicle_child);

    let notice = wait_for_stderr_needle(
        &stderr_buf,
        UPGRADE_NOTICE_LINE,
        "TS-5: waiting for the replacement notice line on the `mux script` vehicle's stderr",
        NOTICE_LINE_TIMEOUT,
    );
    assert!(
        notice.contains(UPGRADE_NOTICE_LINE),
        "AC-3: expected the pinned notice line on the `mux script` vehicle's standard error; \
         got: {notice:?}"
    );

    let status = wait_for_child_exit(
        &mut vehicle_guard.0,
        "TS-5: waiting for the `mux script` vehicle to exit after the upgrade fired",
        SCRIPT_EXIT_TIMEOUT,
    );
    assert!(
        status.success(),
        "AC-3: expected the `mux script` vehicle to exit successfully after the upgrade fired \
         (exit status: {status:?})"
    );

    assert!(
        !exe_link_reports_deleted(daemon_pid),
        "AC-3: expected the daemon's executable-path resolution to no longer report \"(deleted)\" \
         after a successful in-place upgrade fired via the mux-start path"
    );

    let log_text = read_file_with_retry(&log_path, LOG_READ_TIMEOUT);
    assert!(
        log_text.to_ascii_lowercase().contains("handoff"),
        "AC-3: expected the daemon log to contain a handoff-start marker after the mux-start-\
         triggered upgrade; log contents: {log_text:?}"
    );
}

/// AC-4 (task0003 TS-7, FR7): a daemon generation that predates this
/// feature (no identity file was ever recorded -- simulated here by
/// deleting it after the daemon is up) must never misfire an upgrade even
/// when the on-disk binary IS replaced, and `emterm mux attach` must still
/// succeed (no identity failure may ever make attach fail).
///
/// This is an absence assertion (like TS-4): it may legitimately already
/// pass before task0001/task0002 land -- see this section's header note
/// above.
#[test]
fn hot_upgrade_no_misfire_when_identity_file_absent() {
    let runtime_dir = unique_runtime_dir("no-identity");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    let sock_path = socket_path_for(&runtime_dir);
    let log_path = log_path_for(&runtime_dir);
    wait_for_socket(&sock_path, SOCKET_WAIT_TIMEOUT);

    // Observer: registered as a GUI client BEFORE the identity file is
    // removed / the binary is replaced, so it would see an `Upgrading`
    // broadcast if one fired regardless of which client's probe caused it.
    let mut observer = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let _welcome = handshake(&mut observer);

    // Simulate a pre-feature daemon generation: no identity file was ever
    // recorded. Removal is a no-op (not an error) if the file does not
    // exist yet -- e.g. because task0001 has not landed on this branch.
    let _ = std::fs::remove_file(identity_path_for(&runtime_dir));

    replace_binary_with_valid_copy(&daemon_bin_path);

    let attach_child = Command::new(&daemon_bin_path)
        .args(["mux", "attach"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn `emterm mux attach` with no identity file present");
    let mut attach_guard = ChildGuard(attach_child);

    assert_no_misfire_signal(
        &mut observer,
        "TS-7 no-identity",
        NO_CHURN_OBSERVATION_WINDOW,
    );
    assert_log_has_no_handoff_marker(&log_path, "TS-7 no-identity");

    // "attach still works" (FR7): the attach process must not have bailed
    // out early with an identity-related error. Its exit code would be
    // non-zero ONLY on that resolution-failure path (`execute_attach`
    // returns `Err`, printed via `Error: ...` and a non-zero exit); the
    // success path always exits 0, whether still running past this check
    // or already shut down via its own stdin (`/dev/null`) reaching EOF --
    // so a zero exit status (or still running) is the correct "worked"
    // signal here, not "still running" alone.
    if let Some(status) = attach_guard
        .0
        .try_wait()
        .expect("poll attach child exit status")
    {
        assert!(
            status.success(),
            "FR7: `emterm mux attach` must not fail when no identity file is present (exit \
             status: {status:?})"
        );
    }

    let mut fresh = connect(&sock_path, SOCKET_WAIT_TIMEOUT);
    let welcome = handshake(&mut fresh);
    match welcome {
        WelcomeMsg::Accepted { .. } => {}
        WelcomeMsg::Rejected { reason } => panic!(
            "TS-7: expected attach to still work with no identity file present, but a fresh \
             handshake was rejected: {reason}"
        ),
    }
}

// =============================================================================
// task0004 extension (feature-docs/mux-daemon-binary-update-detect)
// =============================================================================
//
// The scenario below proves this task's own live half: a candidate binary at
// the daemon's recorded identity path that fails NFR3's ownership/writability
// gate is refused BEFORE any handoff-schema probe subprocess is spawned, the
// daemon keeps serving the pre-existing pane untouched, and the refusal is
// visible on the attaching client's standard error (AC-2, task0004.md TS-11).

/// How long to wait for the validation-refusal warning line on the attach
/// client's standard error -- generous enough to cover the identity check,
/// the validation gate itself, and the Upgrade round-trip, mirroring
/// [`NOTICE_LINE_TIMEOUT`]'s budget for the success-path counterpart.
const REFUSAL_WARNING_TIMEOUT: Duration = Duration::from_secs(25);

/// AC-2 (TS-11 integration half, NFR3/FR6): after a rename(2) replacement,
/// loosening the NEW candidate's permission bits to group/world-writable
/// must make the daemon refuse the automatic in-place upgrade -- visibly, on
/// the attaching client's standard error -- instead of spawning the
/// handoff-schema probe (observed here as: no successful-upgrade notice, and
/// the daemon's own executable-path resolution still reporting the dangling
/// "(deleted)" marker of the OLD image, since no `execve` ever happened) and
/// without harming the pre-existing pane (its shell PID survives, read back
/// through the SAME still-open connection -- no replacement, no restart).
#[test]
fn hot_upgrade_refuses_group_world_writable_candidate_via_attach() {
    let runtime_dir = unique_runtime_dir("writable-candidate");
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    let daemon_bin_path = runtime_dir.join("emterm-under-test");
    copy_daemon_binary(&daemon_bin_path);

    let child = spawn_isolated_daemon(&daemon_bin_path, &runtime_dir);
    let daemon_pid = child.id();
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
        "TS-11 pre-replacement shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_before = extract_pid_after(&before, "EMTERM_HOTUPG_PID:");

    replace_binary_with_valid_copy(&daemon_bin_path);
    // NFR3: loosen the freshly replaced candidate's permission bits to
    // group+world-writable -- the exact condition this task's validation
    // gate must refuse, on top of an otherwise valid binary.
    let mut perms = std::fs::metadata(&daemon_bin_path)
        .expect("stat replaced candidate binary")
        .permissions();
    perms.set_mode(0o777);
    std::fs::set_permissions(&daemon_bin_path, perms)
        .expect("loosen replaced candidate binary to group/world-writable");
    assert!(
        exe_link_reports_deleted(daemon_pid),
        "test setup sanity: after a rename(2) replacement the daemon's own executable-path \
         resolution must report the kernel's dangling-inode \"(deleted)\" marker"
    );

    let mut attach_child = Command::new(&daemon_bin_path)
        .args(["mux", "attach"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn `<new binary> mux attach`");
    let attach_stderr = attach_child
        .stderr
        .take()
        .expect("attach child stderr was piped");
    let stderr_buf = spawn_stderr_collector(attach_stderr);
    let _attach_guard = ChildGuard(attach_child);

    let warning = wait_for_stderr_needle(
        &stderr_buf,
        "declined the automatic in-place upgrade",
        "TS-11: waiting for the validation-refusal warning on the attach client's stderr",
        REFUSAL_WARNING_TIMEOUT,
    );
    assert!(
        warning.contains("write"),
        "AC-2: expected the refusal reason to name the group/world-write rule; got: {warning:?}"
    );
    assert!(
        !warning.contains(UPGRADE_NOTICE_LINE),
        "AC-2: a refused upgrade must never also print the success notice; got: {warning:?}"
    );

    assert!(
        exe_link_reports_deleted(daemon_pid),
        "AC-2/NFR3: a refused upgrade must never exec -- the daemon must still be running the \
         OLD image"
    );

    send_pane_line(
        &mut stream,
        pane_id,
        "printf 'EMTERM_HOTUPG_PID:%s\\n' \"$$\"; printf '%s%s\\n' EMTERM_HOTUPG _AFTERDONE",
    );
    let after = read_pane_until(
        &mut stream,
        pane_id,
        "EMTERM_HOTUPG_AFTERDONE",
        "TS-11 post-refusal shell round-trip",
        SHELL_ROUNDTRIP_TIMEOUT,
    );
    let pid_after = extract_pid_after(&after, "EMTERM_HOTUPG_PID:");
    assert_eq!(
        pid_before, pid_after,
        "AC-2/FR6: the pane's shell must survive a refused upgrade untouched (pid before=\
         {pid_before}, after={pid_after})"
    );
}
