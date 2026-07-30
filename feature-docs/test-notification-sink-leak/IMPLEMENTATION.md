# Implementation Plan: Stop the unit test suite from emitting real desktop notifications

## Overview

A single-task feature: one unit test in `src-tauri/src/app.rs` is switched from
the production notification sink to the existing capturing test sink, so the
test suite stops sending real desktop notifications over D-Bus.

## Technology Stack

- **Language**: Rust (the `emterm` crate under `src-tauri/`)
- **Test runner**: cargo test (library unit tests, `--lib`)
- **Key components involved**: the notification sink abstraction in
  `src-tauri/src/callbacks.rs` (production sink vs. capturing test sink) and the
  agent-status transition drain in `App::pump_all` (`src-tauri/src/app.rs`)
- **New dependencies**: none — therefore no license check is required against
  `project.license: MIT`

## Layer Structure

Unchanged. The edit lives entirely in the test layer:

| Layer | Touched | Note |
|-------|---------|------|
| Production (`App`, `pump_all`, notification wiring, sink implementations) | No | NFR2 forbids it |
| Test support (`#[cfg(test)] mod tests` in `src-tauri/src/app.rs`) | Yes | the only edit site |

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Capturing-sink test helper (`app_with_test_sink`, existing, in `app.rs`'s test module) | Build an `App` whose notification sink captures instead of sending | Pre: both notification settings default to enabled (the helper asserts this). Post: returns the app plus a handle to the capturing sink; nothing the app sends reaches the OS | task0001 |

No new shared component is introduced, so there is no cross-task contract to
pin — the feature has a single task.

## Conventions

- Reuse the existing helper rather than constructing a sink inline, so all
  notification-touching tests in this module keep one setup path.
- Leave the test's existing assertions and its doc comment intent (AC-2: a
  daemon agent-status update reaches the model through the pump) untouched.

## Cross-task Design Decisions

### D1: Minimal fix over structural guard

The leak could also be closed structurally, by having the app's test-time
constructor default to a non-sending sink. That is deliberately NOT done here:
the task scope (SPEC.md Assumption A1) is the named test only. Recorded so a
reviewer does not read the narrow fix as an oversight.

**Affected tasks**: task0001.

### D2: The "only one test leaks" claim is verified, not assumed

The bug report states this is the sole missed substitution. The verification
step re-checks it by scanning the test module for other apps built with the
production sink that push a blocked/done agent transition through the pump.
Findings are reported, not fixed under this feature (SPEC.md Assumption A2).

**Affected tasks**: task0001 (its Test Notes carry the scan), verify phase.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The helper's setting assertions fail under future default changes, breaking the test for an unrelated reason | Low | Low | The helper is already used by the whole task0009 test group, so any such break surfaces there first, not only here |
| Another test leaks a notification and the fix looks incomplete | Low | Medium | D2's scan; anything found becomes a separate task |

## Open Questions

None.
