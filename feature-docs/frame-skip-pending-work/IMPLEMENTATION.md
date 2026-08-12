# Implementation Plan: frame-skip-pending-work

## Overview

Add toast-creation-preceding pending work (undrained SFTP channel events, the
restart-required flag) to the frame-skip gate through one new non-consuming
App predicate, so an idle window paints the frame that creates the first
toast. Closes em-review finding `b5f2cce1822ab271` (2026-08-11, deferred).

## Technology Stack

- **Rust (existing native GUI runtime)** — winit event loop, egui, wgpu; no
  new architectural element is introduced.
- **crossbeam-channel** — existing project dependency; its receiver half
  provides the non-destructive, lock-free emptiness check the new predicate
  relies on.
- **New external dependencies: none.** No license entries to record; the
  project license (MIT) is unaffected.

## Layer Structure

Unchanged. The change stays inside the existing native GUI runtime path:

| Layer | Modules | Role in this feature |
|-------|---------|----------------------|
| State / predicates | `self_exec` (process-global restart flag), `app` (App-level pending-work predicate) | Own the pending-work signals and expose non-consuming reads |
| Scheduling | `window_host` (skip gate, redraw pacing, wait deadline) | Read the App predicate in the frame-scheduling decisions; never mutate App state in the decision path |

Dependency direction stays `window_host` → `app` → (`self_exec`, `sftp`).

## Shared Components

None — this feature is a single task; component contracts live in
`tasks/task0001.md`.

## Conventions

- Tests follow the project's existing placement (`self_exec` in-module tests;
  App tests under `app/tests/`) and `<subject>_<scenario>_<expected>` naming.
- The test suite runs single-threaded (project test command passes
  `--test-threads=1`). Tests that mutate process-global state must still
  restore clear state before returning, so no test depends on execution
  order.
- Test-only seams are `cfg(test)`-gated and must not alter the production
  channel layout or flag semantics.

## Cross-task Design Decisions

### D1 — FR5 resolved: `next_toast_deadline()` gates on the new predicate

**Decision**: `App::next_toast_deadline()` moves from the toast-only
predicate (`toast_pending`) to the new pending-work predicate
(`frame_work_pending`). FR5's workflow status changes tbd → ok with this
rationale.

**Rationale** (grounded in the current code):

1. The event loop's toast-driven redraw request (`about_to_wait`) is
   rate-limited to the `TOAST_POLL_MS` cadence via `last_toast_redraw`. A
   wake for newly arrived pending work that lands inside that window is
   consumed by the turn WITHOUT a redraw request.
2. If the wait deadline still keyed off visible toasts only, a fully idle
   window (no blink / bell / sidebar deadline armed) would then arm no timer
   and fall back to event-wait. The restart flag wakes the loop exactly
   once, so that one-shot pending work could stall until an unrelated event
   — the same self-lock class this feature closes, reduced to a
   poll-cadence race window.
3. Gating the deadline on the new predicate guarantees re-entry within the
   poll interval whenever ANY pending work exists; on that re-entry the
   rate limit has expired and the redraw fires.
4. Cost: the poll timer additionally arms only while pre-toast pending work
   exists — a transient state resolved by the next painted frame — so
   fully-idle behavior is unchanged and NFR3 is preserved.
5. It keeps all three consumers named in the finding (skip veto, redraw
   pacing, wait deadline) on a single predicate — no divided semantics.

**Affected task**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Non-consuming peek accidentally changes `restart_required()` consume semantics (toast arms twice or never) | Low | High | Peek is read-only by contract; TS-4 / TS-5 assert peek-then-consume ordering explicitly (NFR1) |
| Predicate evaluation is accidentally destructive (drains a channel or swaps the flag) | Low | High | "Consumes nothing" is part of the predicate contract; TS-2 / TS-3 / TS-4 assert the queued event / raised flag is still observable after evaluation |
| Global-flag tests interfere across the suite | Medium | Medium | Single-threaded test run + restore-to-clear discipline (Conventions) |
| Keeping frames flowing on pre-toast work regresses idle CPU | Low | Medium | The pre-toast state is transient (drained by the next painted frame); manual idle observation in TS-7 |

## Open Questions

- None.
