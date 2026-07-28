# Implementation Plan: Empty-Preedit Key Passthrough

## Overview

Replace the single composition flag in the winit IME bridge with two
independent states, and gate key suppression on the state that matches each
platform's meaning of the winit IME events.

## Technology Stack

- **Language**: Rust (existing `emterm` crate, `gui` feature)
- **Key libraries**: winit `=0.31.0-beta.2` — the source of the IME events being
  reinterpreted. No new dependency is introduced, so there is no license
  constraint to evaluate against the project's MIT license.

## Layer Structure

Unchanged. The IME layer keeps its existing three-part shape:

| Layer | Responsibility | Touched by this feature |
|-------|----------------|-------------------------|
| Platform bridge | Translate winit IME events into the neutral event queue and answer the key-suppression question | Yes — state fields and the suppression predicate |
| Backend seam (`ImeBackend` trait) | The only contract between the app and the IME client | No |
| App routes (`pump_ime`, preedit overlay, PTY write) | Consume the neutral events | No |

Dependency direction is unchanged: the app depends on the trait, the bridge
implements it, and nothing in the bridge reaches back into the app.

## Shared Components

This feature decomposes into a single task, so there is no cross-task component
use and no contract that two tasks must independently implement against. The
existing `ImeBackend` trait shape (`dispatch_key_event`, `pump`,
`notify_cursor_rect`, `notify_focus`, `name`, `on_winit_ime`) is a fixed
external constraint, not a shared component being built here — it must not
change.

## Conventions

- **Naming**: the two new states are named for what they observe, not for what
  they gate. One names the emptiness of the last preedit; the other names the
  IME lifecycle being open. Neither may be named "composing" — the ambiguity of
  that word across platforms is the root cause being fixed.
- **Comments**: every claim about winit behavior that justifies a branch cites
  the specific winit sub-crate and the event it maps from. The existing comment
  asserting that Wayland sends an empty preedit for cursor-only updates is
  factually wrong for the pinned winit version and must be removed rather than
  softened.
- **Platform branching**: the platform split lives in exactly one place — the
  suppression predicate. The event-handling arms stay platform-neutral so that
  the state table is identical on every target.

## Cross-task Design Decisions

### D1: Two-level state instead of the minimal single-flag fix

The state model separates "the last preedit was non-empty" from "the IME
lifecycle is open" because the two carry different information on different
platforms, and collapsing them is what produced the bug.

Affected: the single task.

### D2: The suppression predicate is platform-conditional

On Windows the lifecycle events delimit exactly one composition, so the
lifecycle state is the correct gate and an empty preedit inside a live
composition must still suppress keys. On every other target the lifecycle
events fire for the whole focus duration (Wayland) or for the whole time an
input context is allowed (X11), so gating on them would swallow ordinary
direct input; the preedit-emptiness state is the correct gate there.

The rationale and the winit source locations backing it are recorded in
SPEC.md under "Rationale for the platform split". The implementation carries a
condensed form of the same rationale in code comments (FR9).

Affected: the single task.

### D3: No behavior change to the event queue

The mapping from winit IME events to the neutral event queue is unchanged,
including pushing an empty preedit event so the overlay clears. Only the
internal state updates and the suppression predicate change. This keeps the
overlay rendering and the PTY commit path out of the blast radius.

Affected: the single task.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The Windows gate change cannot be exercised on the Linux host | Certain | Medium | Cover it with target-conditional unit tests that compile and run on Windows, verify the Windows target still compiles via the cross-check, and leave a manual verification item for a human on a Windows host |
| A target-conditional predicate leaves one branch untested in CI | Medium | Medium | Write the tests so that both branches are asserted, each under its own target condition, rather than asserting only the host's branch |
| An existing test silently changes meaning under the new model | Medium | Low | Re-read every existing test in the module and state, per test, whether its expected result is unchanged; adjust only those whose premise no longer holds |

## Open Questions

- [ ] Windows behavior (FR7) rests on the source-level mapping of the winit
      lifecycle events to the IMM32 composition messages, not on observation.
      It is recorded as `status: assumed` in workflow.yaml and gated by a
      manual verification item.
