// Phase 4-C: a number of APIs in this module are exercised only by the
// integration tests today and by the Phase 4-D status-bar widget (yet to
// land). We suppress dead-code warnings module-wide because individual
// `#[allow]` attributes would bury intent under noise.
#![allow(dead_code)]

//! `mux` — native-poc's client-side mux daemon protocol stack.
//!
//! Layout (Phase 4-C):
//!
//! - [`wire`] — sync length-prefix framing (4-byte big-endian length + the
//!   existing `mux_ipc::protocol::MuxMessage::to_frame_body` body). Caps the
//!   frame size at `MAX_FRAME_LENGTH` (16 MiB).
//! - [`osc777`] — parser for the `OSC 777 ; emterm ; mux ; <action> ; …`
//!   sequence the PTY emits to ask the GUI to attach / detach.
//! - [`prefix`] — pure state machine for the prefix-key latch (default
//!   `Ctrl+B`). The keybinds layer feeds it `egui::Key` + `Modifiers` events
//!   and it emits typed [`prefix::PrefixAction`] decisions.
//! - [`client`] — blocking `UnixStream` client wrapper. Runs a single RX
//!   thread that calls [`wire::read_frame`] in a loop and forwards typed
//!   [`mux_ipc::protocol::MuxMessage`] frames over an `mpsc` channel to the
//!   main thread; the send side is mutex-guarded.
//! - [`mock`] (`#[cfg(test)]` only) — in-memory daemon pair used by the
//!   integration tests so they stay deterministic and Docker-friendly.

pub mod osc777;
pub mod prefix;
pub mod wire;

// `client` is unix-only because it depends on `UnixStream`. Phase 4 targets
// Linux + Windows; the Windows port will land alongside Phase 4-E and use
// named pipes (or skip mux entirely). For now we gate the module on `unix`
// so `cargo build --workspace` stays green on Windows CI.
#[cfg(unix)]
pub mod client;

#[cfg(test)]
pub mod mock;

// Phase 4-F: perf scaffolding for TS-perf-1 (snapshot apply latency) and
// TS-perf-2 (prefix follow-up → wire round trip). `#[ignore]` by default;
// run with `cargo test -p emterm-native-poc -- --ignored`.
#[cfg(test)]
mod perf_tests;
