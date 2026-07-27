# Implementation Plan: mux pane child process reaping

## Overview

Retain the shell child-process handle from PTY spawn through to `MuxPane`,
and reap it — off the daemon's async runtime, with a bounded grace period and
a kill escalation — on every pane-teardown path. This is a single-task
feature: all task-level design (module placement, reap mechanism, timing
values, error flow) lives in `tasks/task0001.md`.

## Technology Stack

- **Language**: Rust — existing `src-tauri` crate, mux subsystem.
- **Key library**: `portable-pty 0.8.1` (already a dependency) — provides the
  child-process handle contract this feature drives: non-blocking exit poll
  (`try_wait`), blocking reap (`wait`), and the kill capability of its
  killer supertrait.

**License record**: no new dependency is introduced. `portable-pty 0.8.1` is
MIT-licensed and already in the dependency tree; project license is MIT — no
conflict, nothing to resolve.

## Layer Structure

- `mux::ipc::pty_spawn` (spawn layer) — produces the child handle at spawn
  time and hands it to the session layer. Depends on `mux::session`.
- `mux::session::pane` (pane lifecycle) — owns the handle for the pane's
  lifetime; hands it off at teardown. Depends on the reaper module below.
- `mux::session::child_reaper` (NEW, leaf module) — the reap procedure and
  its background-handoff entry. Depends only on the portable-pty child
  contract and the logging facade; no dependency back into pane/session
  state, so it is unit-testable without a PTY or a pane (SPEC FR6).

The mux subsystem is built in every feature configuration (no `gui` gate in
`src-tauri/src/lib.rs`), so the new module needs no feature gating and the
`--no-default-features` (CLI-only) build is unaffected (NFR5).

## Shared Components

None — single-task feature; there is no cross-task component use.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| — | — | — | — |

## Conventions

- **Logging**: reap failures are logged at `warn` or above (release builds
  persist `warn`+, NFR4). Log lines carry the pane ID and the error kind
  only — never user data (SPEC Security Considerations).
- **Error policy**: a reap failure is terminal for the reap procedure only.
  It never propagates to the teardown caller, never fails the teardown
  operation, and never panics (NFR3).

## Cross-task Design Decisions

None — single task. See `tasks/task0001.md` for all design decisions.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Wedged shell keeps a reaper thread alive | Low | Low | Bounded grace period, then kill + wait; the kill makes the final wait return promptly |
| Regression test cannot open a PTY in the test environment | Medium | Low | Unix-gated test that skips cleanly when PTY creation fails (SPEC A4) |
| Double reap from concurrent teardown paths | Medium | High | Removing the handle from the pane is the single gate: the second caller finds nothing and starts no reap (FR3) |
| Daemon exits before a reaper thread finishes | Low | None | Orphaned child re-parents to init, which reaps it (SPEC edge case); no leak survives daemon exit |

## Open Questions

None. SPEC Assumptions A1–A5 are resolved by this plan (A1: per-exit
detached OS thread; A2: grace period 500 ms / poll interval 50 ms — see
`tasks/task0001.md`; A3–A5 adopted as specified).
