//! Performance test scaffolding for Phase 4-F (SDD `mux-tabs-windows-ime`).
//!
//! Two scenarios from the SPEC's performance acceptance criteria:
//!
//! - **TS-perf-1** — snapshot apply latency. Time how long
//!   [`term_core::TerminalCore::reset_and_replay`] takes to absorb a ~1 MiB
//!   snapshot payload (the realistic upper bound for a full pane redraw the
//!   daemon sends on `Snapshot`).
//! - **TS-perf-2** — prefix-to-send round trip latency. Time the path from
//!   a prefix chord follow-up key reaching [`super::prefix::Latch::observe`]
//!   to the resulting [`mux_ipc::protocol::MuxMessage`] being framed and
//!   handed to the mock daemon's [`super::mock::ChannelTransport`].
//!
//! Both tests are `#[ignore]` by default. They are not gating today — Phase
//! 4-F treats them as scaffolding, not as a CI threshold. The point is to
//! prove the harness compiles and gives reproducible numbers when run on a
//! dev host (`cargo test -p emterm-native-poc -- --ignored snapshot_apply_`
//! or `prefix_round_trip_`).
//!
//! Recorded measurements live as comments inside each test. Update them in
//! place when the host run produces new numbers; do not gate CI on them.

#![cfg(test)]

use std::time::Instant;

use term_core::TerminalCore;

use super::mock;
use super::prefix::{Latch, PrefixAction, PrefixChord};
use super::wire;

use egui::{Key, Modifiers};
use mux_ipc::protocol::{MessageType, MuxMessage};

/// TS-perf-1 — snapshot apply latency for a ~1 MiB payload.
///
/// The payload is synthesised in two halves so the parser exercises a mix of
/// printable bytes and CSI cursor-position commands (the realistic shape of a
/// captured `Snapshot` from a busy pane). The grid is sized 200×60 (≈ tmux
/// default) with 10000 lines of scrollback to match `Settings::default()`.
///
/// **Recorded measurements**
///
/// - Docker (debug build, in-container): ~1.84 s for the 1 MiB payload.
///   This is informational only — the SPEC threshold targets a release
///   build on a real host where the parser runs ~50-100× faster.
/// - dev host (release): pending
/// - acceptance target: < 50 ms (per SPEC TS-perf-1)
#[test]
#[ignore = "perf scaffolding — run explicitly with --ignored"]
fn snapshot_apply_1mib_under_50ms() {
    let mut payload: Vec<u8> = Vec::with_capacity(1024 * 1024);
    // First half: a long stream of printable ASCII to drive the fast path.
    while payload.len() < 512 * 1024 {
        payload.extend_from_slice(b"the quick brown fox jumps over the lazy dog 0123456789\n");
    }
    // Second half: interleave CSI cursor-position + SGR sequences so the
    // state machine has to do real work, not just a printable burst.
    while payload.len() < 1024 * 1024 {
        payload.extend_from_slice(b"\x1b[2;3H\x1b[31;1mERROR\x1b[0m line content\n");
    }
    // Truncate to exactly 1 MiB so the measurement is reproducible.
    payload.truncate(1024 * 1024);

    let mut core = TerminalCore::new(200, 60, 10_000);

    let started = Instant::now();
    core.reset_and_replay(&payload);
    let elapsed = started.elapsed();

    eprintln!(
        "TS-perf-1: reset_and_replay({} bytes) = {:?}",
        payload.len(),
        elapsed
    );
    // We do not assert a hard threshold here — Phase 4-F treats this as
    // scaffolding only. Once a baseline is measured on the dev host, add an
    // assert!(elapsed < Duration::from_millis(50)) and reclassify the test
    // as non-`#[ignore]`.
}

/// TS-perf-2 — prefix chord follow-up → wire-encoded frame round trip.
///
/// Drives one armed prefix chord (`Ctrl+B`) followed by `n` (next window),
/// constructs the resulting `PtyInput` (encoded as a control frame the
/// daemon would receive), and measures the wall-clock time from the
/// follow-up keystroke to the frame appearing in the mock transport's
/// `recorded_frames()`.
///
/// **Recorded measurements**
///
/// - Docker (debug build, in-container): ~59 µs for the follow-up → wire path.
///   Well under the 5 ms threshold even in debug — the path is pure
///   in-process state-machine + bincode encode, no I/O on the hot path.
/// - dev host (release): pending
/// - acceptance target: < 5 ms (per SPEC TS-perf-2 — interactive feel)
#[test]
#[ignore = "perf scaffolding — run explicitly with --ignored"]
fn prefix_round_trip_under_5ms() {
    let chord = PrefixChord::default();
    let mut latch = Latch::new(chord, std::time::Duration::from_secs(3));
    let (transport, server) = mock::pair();

    // Arm the latch (Ctrl+B). This is the steady-state "prefix pressed".
    let arm_now = Instant::now();
    let _ = latch.observe(Modifiers::CTRL, Key::B, arm_now);
    assert!(latch.is_armed(), "latch should arm on Ctrl+B");

    let started = Instant::now();
    // Follow-up: `n` → NextWindow.
    let action = latch.observe(Modifiers::NONE, Key::N, started);
    assert_eq!(action, PrefixAction::NextWindow);

    // Map the action to a wire message. In production this is the App layer's
    // job; for the perf measurement we synthesise the same call site here.
    let msg = MuxMessage::pty_input(0, b"\x1b[5;1~".to_vec());

    let mut buf = Vec::with_capacity(64);
    wire::encode_into(&mut buf, &msg).expect("encode");
    use super::client::Transport;
    transport.write_all(&buf).expect("write");

    let elapsed = started.elapsed();

    let recorded = server.recorded_frames();
    assert_eq!(recorded.len(), 1, "exactly one frame recorded");
    assert_eq!(recorded[0].msg_type, MessageType::PtyInput);

    eprintln!("TS-perf-2: prefix follow-up → wire = {elapsed:?}");
    // No hard threshold yet — scaffolding only. See TS-perf-1 comment for
    // the activation plan once dev-host numbers exist.
}
