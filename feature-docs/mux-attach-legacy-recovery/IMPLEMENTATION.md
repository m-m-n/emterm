# Implementation Plan: mux attach legacy daemon recovery

## Overview

Insert the existing legacy-daemon recovery step (Strategy B) into the
`emterm mux attach` path, extracting the daemon-spawn logic of
`ensure_daemon_running` into a reusable function so the attach path can
respawn the daemon after a recovery shutdown.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate, mux module). No new
  dependencies — license constraint (MIT) is unaffected.

## Layer Structure

Unchanged. All work stays inside the existing mux module:
`src-tauri/src/mux/daemon.rs` (daemon lifecycle) and
`src-tauri/src/mux/cli.rs` (command entry points). `cli.rs` may depend on
`daemon.rs` (existing direction); no new layers.

## Shared Components

Single-task feature — no cross-task contracts required. The table below
records the internal contracts for reference:

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| daemon spawn function (extracted from ensure_daemon_running) | Create socket parent dir with restricted permissions, spawn detached daemon process, wait for readiness with backoff | Pre: no compatible daemon owns the socket. Post: daemon answering on the socket, or an error string identical to today's failure messages | task0001 |
| legacy recovery probe (existing, visibility widened) | Probe protocol version; shut down adjacent-older daemon | Unchanged behavior; becomes callable from the cli module | task0001 |

## Conventions

- Error strings and log messages currently emitted by
  `ensure_daemon_running` / `recover_from_legacy_daemon` are preserved
  verbatim (users and tests depend on them).
- Platform-specific branches (`cfg(unix)` / `cfg(windows)`) move with the
  extracted code unmodified.

## Cross-task Design Decisions

None (single task).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Behavior drift in `ensure_daemon_running` after extraction | Low | High (every mux entry point uses it) | Extraction is move-only; existing daemon tests must keep passing (TS-4) |
| Attach semantics change (auto-starting a daemon when none existed) | Low | Medium | Socket-existence check stays first; spawn happens only on `Recovered` |
| Test flakiness from real daemon spawns in tests | Medium | Low | Reuse the existing fake-daemon test patterns; follow test/README.md (`--lib`, serialized reruns if needed) |

## Open Questions

- [ ] None.
