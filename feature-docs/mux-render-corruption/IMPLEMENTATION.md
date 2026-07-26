# Implementation Plan: mux Render Corruption Fix

## Overview

Single-task fix for the intermittent post-window-switch line-content mixing in
mux: confirm it shares the proven root cause of the replay coordinate-drift
bug (resize-interleaved scrollback replayed into a fixed-size core), then fix
that mechanism with regression tests.

## Technology Stack

- **Language**: Rust (existing crates only — no new dependencies planned;
  if the implementer needs a new dependency it must be MIT-compatible and
  reported as a deviation)
- **Key components**: `crates/term_core` (grid + replay), `src-tauri/src/mux`
  (daemon scrollback / snapshot assembly / GUI replay path)

## Layer Structure

Existing layers, unchanged:

1. **daemon layer** (`src-tauri/src/mux/` daemon side) — owns scrollback
   recording and snapshot assembly; authoritative for what bytes are replayed
2. **transport** (`crates/mux_ipc`, bridge) — opaque byte delivery; not
   expected to change semantically
3. **GUI replay layer** (`src-tauri/src/tabs.rs`, `src-tauri/src/app.rs`,
   `src-tauri/src/mux/ipc/handlers.rs`) — applies snapshot bytes to a fresh
   `term_core` grid
4. **core** (`crates/term_core`) — ANSI interpretation and grid state

## Cross-task Design Decisions

### D1: Fix direction — record/replay coordinate-system agreement

The proven mechanism (report `tmp/apt-progress-bar-regression-2026-07-09.md`,
PROBE D) is: bytes emitted for different terminal row counts coexist serially
in scrollback, and replay feeds them into a core fixed at the current size.
The chosen fix direction is **candidate A (resize markers)**: the daemon
records a marker in the scrollback stream at each pane resize, and the replay
path interprets the marker by resizing the replay core before continuing, so
the coordinate system during replay always matches the one the bytes were
produced for. Candidates B (flatten scrollback on resize — loses styling) and
C (structured snapshot transport — large rework) are explicitly out of scope.

If the investigation disproves the shared-root-cause hypothesis for the
reported Claude Code symptom, the implementer fixes the actually-identified
cause instead and reports the divergence (plan deviation), rather than
implementing markers nobody needs.

### D2: Marker encoding constraint

The marker travels in-band in the recorded byte stream, so it must be a
sequence that (a) cannot collide with output a real application can emit,
(b) is stripped or consumed before reaching any non-marker-aware consumer,
and (c) survives the existing scrollback write filter and snapshot stripper
untouched. The concrete encoding is the implementer's choice within these
constraints.

## Conventions

- Diagnostic logging added during investigation uses `warn`+ (release logs
  drop lower levels); temporary probes are removed before completion
- Replay-related tests run single-threaded (`--test-threads=1`) per project
  testing notes

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Reported symptom has a different root cause | medium | medium | Investigation gate before fixing; deviation report path defined in D1 |
| Marker leaks into visible output or viewer OSC handling | low | high | Regression tests replay marker-bearing scrollback and assert grid equality with non-marker recordings when no resize occurred |
| Old daemon + new GUI (or vice versa) during rollout | medium | low | Marker unknown to old consumers must degrade to current behavior (no worse than today); noted in acceptance criteria |
| Replay latency regression from mid-replay resizes | low | medium | NFR1 manual check; resize events are rare relative to output volume |

## Open Questions

- [ ] Whether the mux-external (non-mux) path shows the same symptom is
      unverified by the user — out of scope unless investigation shows the
      defect lives in shared code
