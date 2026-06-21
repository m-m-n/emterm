//! Real-process throughput benchmark for the mux output pipeline.
//!
//! This integration test spawns a real `emterm mux --daemon` child process,
//! connects to its Unix socket exactly like the GUI bridge does, asks the
//! daemon to run `seq 1 N` in a PTY, and measures the **wall-clock
//! throughput** of the full output path:
//!
//!   daemon PTY read + shadow parse
//!     -> mpsc channel (capacity `PTY_CHANNEL_CAPACITY`)
//!     -> connection handler drain (`DRAIN_BATCH_LIMIT`) + merge
//!     -> Unix socket frame (`[u32 BE len][frame body]`)
//!     -> this test's socket read + frame decode
//!     -> `term_core::TerminalCore::process_pty_data_fully` (the client-side
//!        re-parse the GUI performs on every chunk)
//!
//! The client-side `TerminalCore` re-parse is included on purpose: the GUI's
//! per-chunk parsing cost is part of the end-to-end throughput the user feels,
//! so a benchmark that drops it would over-report. (Base64/APC encode-decode
//! is NOT included: the bridge subprocess performs it on stdin/stdout, but the
//! daemon socket itself carries raw `MuxMessage` frames, which is the layer
//! this test connects to.)
//!
//! ## Running
//!
//! The test is `#[ignore]`d by default because it spawns a real process, runs
//! for seconds, and is therefore unsuitable for the default CI test run. It is
//! also only meaningful in a **release** build — a debug `term_core` parse is
//! several times slower and does not reflect the throughput the shipped binary
//! delivers.
//!
//! ```sh
//! CARGO_TARGET_DIR=src-tauri/target \
//!   cargo test --release --test mux_throughput \
//!   --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
//! ```
//!
//! `--nocapture` is required to see the measured numbers (they are printed via
//! `eprintln!`).
//!
//! ## Why `#[ignore]` + release-only
//!
//! - Spawning a real daemon, racing on socket creation, and draining a live
//!   PTY makes this inherently flakier than a pure unit test; keeping it out of
//!   the default run avoids destabilizing CI.
//! - Only a release build reflects the real-world pipeline throughput; a debug
//!   build's slow parse would report misleadingly low numbers.

// The mux daemon and its socket transport are Unix-only (Windows uses a Named
// Pipe with a different client API). Gate the whole test file so the
// CLI-only / non-Unix builds still compile cleanly.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use mux_ipc::protocol::{
    ClientType, CreateWindowPayload, HelloMsg, MAX_FRAME_LENGTH, MessageType, MuxMessage,
    PROTOCOL_VERSION, WelcomeMsg,
};
use term_core::TerminalCore;

/// Default upper bound for the `seq` generator. 1_000_000 keeps the default run
/// fast (a few seconds in release) while still being large enough to saturate
/// the pipeline and expose backpressure. Override with `EMTERM_THROUGHPUT_N`
/// (e.g. `EMTERM_THROUGHPUT_N=10000000`) to reproduce the pathological slow
/// case (mux ~2% CPU / minutes) described in the issue.
const DEFAULT_SEQ_UPPER_BOUND: u64 = 1_000_000;

/// Resolve the `seq` upper bound from `EMTERM_THROUGHPUT_N`, falling back to
/// [`DEFAULT_SEQ_UPPER_BOUND`].
fn seq_upper_bound() -> u64 {
    std::env::var("EMTERM_THROUGHPUT_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SEQ_UPPER_BOUND)
}

/// Sentinel printed by the shell after `seq` finishes. Detecting it in the
/// received byte stream is a deterministic completion signal (independent of
/// PTY EOF timing, which only fires when the shell itself exits).
///
/// The PTY runs an interactive shell that **echoes the command line back**, so
/// the marker must NOT appear verbatim in the command we send — otherwise the
/// echoed command matches the marker before `seq` even starts. We therefore
/// assemble the marker from two fragments: the command prints their
/// concatenation (`MARKER`), while the command text only ever contains the
/// fragments as separate `printf` arguments (never the joined string).
const MARKER_HEAD: &str = "EMTERM_THROUGHPUT";
const MARKER_TAIL: &str = "_DONE";

/// The fully-joined marker the shell's output contains (but the command text
/// does not).
fn done_marker() -> String {
    format!("{MARKER_HEAD}{MARKER_TAIL}")
}

/// Overall receive-loop timeout. If the marker never arrives (handshake wrong,
/// pane never created, pipeline wedged) the test fails loudly instead of
/// hanging the whole suite.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long to wait for the daemon to create its socket after spawn.
const SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// RAII guard that kills the spawned daemon and removes the isolated runtime
/// directory on drop, so a panicking assertion never leaks a daemon process.
struct DaemonGuard {
    child: Child,
    runtime_dir: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        // Kill the daemon process. It was spawned with its own session
        // (`setsid` via the binary's own daemon spawn path is NOT used here —
        // we spawn it directly), so a direct kill is sufficient.
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Best-effort cleanup of the isolated runtime dir (socket + logs).
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

/// Compute the daemon socket path for a given `XDG_RUNTIME_DIR`, mirroring
/// `daemon::socket_path()` (`$XDG_RUNTIME_DIR/emterm/mux-default.sock`).
fn socket_path_for(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("emterm").join("mux-default.sock")
}

/// Write a length-prefixed `MuxMessage` frame: `[u32 BE len][frame body]`.
fn write_frame(stream: &mut UnixStream, msg: &MuxMessage) -> std::io::Result<()> {
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()
}

/// Read exactly one length-prefixed frame and decode it into a `MuxMessage`.
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

#[test]
#[ignore = "spawns a real daemon process; run with --release --ignored --nocapture"]
fn mux_output_pipeline_throughput() {
    // 1. Isolated runtime dir so we never collide with the user's real daemon.
    let runtime_dir = std::env::temp_dir().join(format!(
        "emterm-mux-throughput-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&runtime_dir).expect("create isolated runtime dir");

    // 2. The binary under test is the freshly built `emterm` (default features
    // = gui on, so the mux daemon path is present).
    let exe = env!("CARGO_BIN_EXE_emterm");

    // 3. Spawn `emterm mux --daemon` with the isolated XDG_RUNTIME_DIR.
    //
    // SHELL is forced to `/bin/sh` (dash): the daemon's `spawn_pty` launches
    // `$SHELL` for each pane, and a heavy interactive shell (zsh/bash with a
    // p10k-style rc) is NOT ready within the daemon's 50ms command-send delay
    // — the typed command lands mid-`.zshrc` and is dropped by the line editor,
    // so `seq` never runs. `/bin/sh` has no per-user rc and is ready
    // immediately, making the workload deterministic. The throughput we measure
    // is the OUTPUT pipeline, which is shell-independent.
    let child = Command::new(exe)
        .args(["mux", "--daemon"])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env("SHELL", "/bin/sh")
        // Avoid the nesting guard refusing to run if the host shell already
        // has EMTERM_MUX set (the daemon itself doesn't check it, but a child
        // PTY would inherit it — clear it so `seq` runs in a clean shell).
        .env_remove("EMTERM_MUX")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn emterm mux --daemon");

    let _guard = DaemonGuard {
        child,
        runtime_dir: runtime_dir.clone(),
    };

    // 4. Wait for the socket to appear (daemon startup is async).
    let sock_path = socket_path_for(&runtime_dir);
    let socket_deadline = Instant::now() + SOCKET_WAIT_TIMEOUT;
    while !sock_path.exists() {
        if Instant::now() > socket_deadline {
            panic!(
                "daemon socket {:?} did not appear within {:?}",
                sock_path, SOCKET_WAIT_TIMEOUT
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // 5. Connect and perform the GUI handshake: Hello -> Welcome.
    let mut stream = {
        // The socket file can exist a hair before the listener accepts; retry
        // connect briefly.
        let connect_deadline = Instant::now() + SOCKET_WAIT_TIMEOUT;
        loop {
            match UnixStream::connect(&sock_path) {
                Ok(s) => break s,
                Err(_) if Instant::now() < connect_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("failed to connect to daemon socket {:?}: {}", sock_path, e),
            }
        }
    };

    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    write_frame(
        &mut stream,
        &MuxMessage::control(MessageType::Hello, 0, &hello),
    )
    .expect("send Hello");

    // Read Welcome with a bounded timeout (the daemon auto-creates the default
    // session and responds immediately).
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout for handshake");
    let welcome = read_frame(&mut stream).expect("read Welcome frame");
    assert_eq!(
        welcome.msg_type,
        MessageType::Welcome,
        "expected Welcome, got {:?}",
        welcome.msg_type
    );
    let welcome_payload: WelcomeMsg = welcome.decode_payload().expect("decode Welcome payload");
    match welcome_payload {
        WelcomeMsg::Accepted { .. } => {}
        WelcomeMsg::Rejected { reason } => panic!("daemon rejected handshake: {reason}"),
    }

    // 6. Ask the daemon to create a window whose PTY runs `seq` and then prints
    // the completion sentinel. CreateWindow with pane_id=0 targets the
    // connection's active (auto-created "default") session; the new pane's
    // reader thread streams PtyOutput back over THIS connection.
    // Note: `%s%s` joins the two fragments in the OUTPUT, but the command
    // TEXT (which the interactive shell echoes back) only contains the
    // fragments as separate arguments, so the echo never matches the marker.
    let seq_upper_bound = seq_upper_bound();
    let command =
        format!("seq 1 {seq_upper_bound}; printf '\\n%s%s\\n' {MARKER_HEAD} {MARKER_TAIL}");
    let payload = CreateWindowPayload {
        name: Some("throughput".to_string()),
        command: Some(command),
    };
    write_frame(
        &mut stream,
        &MuxMessage::control(MessageType::CreateWindow, 0, &payload),
    )
    .expect("send CreateWindow");

    // 7. Receive PtyOutput frames until the DONE marker appears.
    //
    // Switch to a short per-read timeout so the overall RECEIVE_TIMEOUT is
    // enforced even if the pipeline wedges mid-stream. The first chunk may take
    // a moment (shell startup + 50ms command-send delay in the daemon), so the
    // per-read timeout is generous; the wall-clock guard below is authoritative.
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout for receive loop");

    let mut client_core = TerminalCore::new(80, 24, 1000);

    // Rolling tail to detect the marker even when it straddles a frame
    // boundary. Keep slightly more than the marker length.
    let marker = done_marker();
    let marker_bytes = marker.as_bytes();
    let mut tail: Vec<u8> = Vec::with_capacity(marker_bytes.len() * 2);

    let mut total_bytes: u64 = 0;
    let mut frame_count: u64 = 0;
    let mut first_byte_at: Option<Instant> = None;

    let start = Instant::now();
    let deadline = start + RECEIVE_TIMEOUT;

    'recv: loop {
        if Instant::now() > deadline {
            panic!(
                "timed out after {:?} without seeing '{}' (received {} bytes in {} frames)",
                RECEIVE_TIMEOUT, marker, total_bytes, frame_count
            );
        }

        let msg = match read_frame(&mut stream) {
            Ok(m) => m,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No data this interval; loop to re-check the wall-clock guard.
                continue;
            }
            Err(e) => panic!("socket read error mid-stream: {e}"),
        };

        match msg.msg_type {
            MessageType::PtyOutput => {
                if first_byte_at.is_none() && !msg.payload.is_empty() {
                    first_byte_at = Some(Instant::now());
                }
                frame_count += 1;
                total_bytes += msg.payload.len() as u64;

                if std::env::var_os("EMTERM_THROUGHPUT_DEBUG").is_some() {
                    eprintln!(
                        "[frame {} pane={} {}B] {:?}",
                        frame_count,
                        msg.pane_id,
                        msg.payload.len(),
                        String::from_utf8_lossy(&msg.payload)
                    );
                }

                // Client-side re-parse: this is the GUI's per-chunk cost.
                let _responses = client_core.process_pty_data_fully(&msg.payload);

                // Marker detection across frame boundaries.
                tail.extend_from_slice(&msg.payload);
                if tail.windows(marker_bytes.len()).any(|w| w == marker_bytes) {
                    break 'recv;
                }
                // Trim the tail so it never grows unbounded.
                if tail.len() > marker_bytes.len() * 2 {
                    let keep = marker_bytes.len() * 2;
                    let drop_from = tail.len() - keep;
                    tail.drain(..drop_from);
                }
            }
            MessageType::PaneCreated => {
                // Acknowledgement that the window/pane was created — informational.
            }
            MessageType::PtyExited => {
                // The shell exited before we saw the marker. The marker is
                // printed before the shell would normally exit, so this would
                // indicate the command failed to run; surface it.
                panic!(
                    "pane exited before DONE marker (received {} bytes in {} frames)",
                    total_bytes, frame_count
                );
            }
            MessageType::Error => {
                panic!("daemon returned Error frame during throughput run");
            }
            // Status bar / rename / notify frames are pipeline noise here.
            _ => {}
        }
    }

    let elapsed = start.elapsed();
    let time_to_first_byte = first_byte_at.map(|t| t.duration_since(start));

    let mib = total_bytes as f64 / (1024.0 * 1024.0);
    let secs = elapsed.as_secs_f64();
    let mib_per_sec = if secs > 0.0 {
        mib / secs
    } else {
        f64::INFINITY
    };

    eprintln!("=== mux output pipeline throughput ===");
    eprintln!("  seq upper bound : {seq_upper_bound}");
    eprintln!("  total received  : {total_bytes} bytes ({mib:.2} MiB)");
    eprintln!("  frames received : {frame_count}");
    eprintln!("  wall-clock      : {:.3} s", secs);
    if let Some(ttfb) = time_to_first_byte {
        eprintln!("  time-to-first   : {:.3} s", ttfb.as_secs_f64());
    }
    eprintln!("  throughput      : {mib_per_sec:.2} MiB/s");
    eprintln!("======================================");

    // Sanity: we must have actually received the bulk of `seq` output. `seq 1
    // N` emits roughly sum-of-digit-lengths bytes; for N = 1_000_000 that is
    // ~6.9 MB. Assert a conservative lower bound so a silently-truncated run
    // (e.g. marker matched against startup noise) fails.
    assert!(
        total_bytes > 100_000,
        "implausibly small received total ({total_bytes} bytes) — pipeline likely broke"
    );
}
