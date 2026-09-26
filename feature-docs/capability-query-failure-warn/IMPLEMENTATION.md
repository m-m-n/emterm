# Implementation Plan: capability-query-failure-warn

## Overview

Add a transition-only, warn-level emterm.log record to the unix notification
worker (`notify_worker` in `src-tauri/src/callbacks.rs`) when the notify-rust
capability query fails. The fail-closed escape decision and every existing
notification output stay unchanged. The feature is one task (task0001).

## Technology Stack

- **Language / Framework**: Rust, existing `src-tauri` crate. The `callbacks`
  module is compiled only with the `gui` feature.
- **Key libraries**: notify-rust (existing — capability query and send, both
  unchanged), log (existing — warn level).
- **New dependencies**: none (NFR5). No license entry is needed;
  `project.license` (MIT) is unaffected.

## Layer Structure

Every change lives in the `callbacks` module, inside its unix-only
notification-worker section.

| Layer | Element | Change |
|-------|---------|--------|
| Worker loop (the only I/O layer) | unix `notify_worker` | Owns the worker-local previous-failure state, binds the query result once per notification, emits the warn record |
| Pure decision | Capability-query warn decision helper (new) | New; performs no I/O |
| Escape gate | `escape_for_send_on_demand`, `escape_for_send`, `body_markup_absence_confirmed`, `escape_body_markup` | Unchanged (NFR1) |
| Constants | Log marker constants section | One new marker constant |

Allowed dependency directions:

- The worker loop depends on the decision helper, the escape gate and the
  marker constant.
- The decision helper depends on nothing else in the module: not on the
  escape gate, not on the redaction renderer, not on any notification text.
- The escape gate never depends on the decision helper or on its state.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Capability-query warn decision helper (new, unix-only, module-private) | Decide whether a capability-query outcome is a warn-worthy transition, and compute the next state | Inputs: the previous failure text (optional text, borrowed) and the current query result (borrowed; a capability list on success, an error on failure, where the error type has a Display representation). Outputs: whether to warn (boolean) and the next state (optional owned text). Precondition: none (total over all inputs). Postcondition: follows the transition table below. Pure: no I/O, no logging, no static / global / thread-local state, deterministic. | task0001 |
| Capability-query failure marker constant (new, unix-only) | Fixed text that identifies the warn record in emterm.log | Named `LOG_NOTIFY_CAPABILITY_QUERY_FAILED`. Its value equals its own name, the same shape as the existing `LOG_*` marker constants. Documented like the existing ones. | task0001 |
| Worker previous-failure state | Remember the previous failure's error text for the transition decision | One optional text value, local to one run of the unix `notify_worker` loop. Initial value: none. Updated only from the helper's next state, and only for a notification whose capability query was actually invoked. Never read by, or passed to, any escape function (NFR3). | task0001 |

### Transition table (decision helper postcondition)

| Previous state | Current result | Warn | Next state |
|----------------|----------------|------|------------|
| none | success (any list) | no | none |
| none | failure with error text E | yes | E |
| P | success (any list) | no | none |
| P | failure with error text E, E equal to P | no | P |
| P | failure with error text E, E different from P | yes | E |

"Error text" is the Display representation of the error value.

Invariant: whenever the helper returns "warn", the next state is present and
holds exactly the current error text. That text is the only variable content
of the warn record.

### Warn record format

One warn-level record: the marker constant, then a colon and a single space,
then the error text. This matches the shape of the existing marker-prefixed
records (queue saturation, rate limit, OSC 52 denial).

## Conventions

- **Log level**: this feature adds warn-level records only. It adds no
  debug-level or info-level records (NFR2). The existing dispatch-success,
  dispatch-failure and queue-saturation records keep their text and level.
- **Log content**: the warn record never carries text derived from a
  notification: not the raw title or body, not the escaped values, not the
  redacted rendering (FR4).
- **Platform gating**: every new item (helper, marker constant, state, new
  tests) sits under the same unix-only gating as the existing escape helpers.
  The Windows `notify_worker` and `spawn_notify_worker` stay untouched (NFR4).
- **Error typing**: the fetch error type parameter of the unix
  `notify_worker` gains a Display requirement, like its send error type
  parameter already has. The production error type already satisfies it.
- **Tests**: no log capture. The warn decision is verified through the
  helper's return value (NFR5, SPEC A6). Existing assertions stay unchanged;
  only a fake's error type may change (NFR6).

## Cross-task Design Decisions

### D1: One task

The helper, the worker wiring and their tests live in the same two files.
Splitting them into parallel tasks would need placeholder wiring and would
produce same-file conflicts, so the feature is one task. Affected: task0001.

### D2: Bind the query result once through the gate's deferred supplier

The worker hands `escape_for_send_on_demand` a deferred supplier. When
invoked, the supplier:

1. performs the capability query once and binds the result,
2. runs the warn decision and, if needed, the warn emission on that bound
   result,
3. yields the same bound result to the gate.

Rationale:

- `escape_for_send_on_demand` and `escape_for_send` stay unchanged (NFR1).
- The metacharacter gate stays defined in one place. For a notification
  without a metacharacter, the supplier is never invoked, so no query runs
  and the state does not change (FR1, FR2).
- "At most once per notification" still holds by the gate's consume-once
  supplier parameter.

Rejected alternative: repeating the metacharacter check inside the worker.
It would duplicate the gate logic and leave `escape_for_send_on_demand`
unused by production code.

Affected: task0001.

### D3: The state is optional text, compared by Display text

Previous failures are compared by their error text, never by error value
(SPEC A4, FR3). Recovery from a failure (a success after a failure) resets
the state to none without logging (FR2, SPEC A2). Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The wiring change alters what the escape gate receives (a different or repeated query result), weakening fail-closed escaping | Low | High | D2 passes the identical bound result. TS-5 and TS-6 pin query counts and escape outputs. The security review perspective covers the gate. |
| The warn record includes notification-derived text | Low | High | The record is built only from the marker constant and the helper's next-state text. Checked by review TS-7. |
| The new Display requirement breaks the existing worker test fake, whose error type is the unit type | Certain | Low | Switch the fake's error type to a text type. Assertions stay unchanged (NFR6). |
| An error whose Display text changes on every call defeats the de-duplication, so every failure warns | Low | Low | Accepted. SPEC defines transitions by error text. Warn records stay limited to notifications that contain a metacharacter. |

## Open Questions

- None.
