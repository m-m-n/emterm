# Feature: restored-pane-echo-deadline-start-delay

## Overview

The two deadline tests of the test-only helper `read_echo_until_marker`
(`src-tauri/src/mux/session/pane/tests.rs`, module `restored_echo_wait`) depend
on the helper reader thread being scheduled within `SHORT_DEADLINE` (200 ms).
This feature makes their collected-bytes assertions hold regardless of when the
reader thread starts. It also adds a deterministic start-delay regression test
and a deterministic check of the deadline failure message. It is a test-only
change.

## Objectives

- Stop the two deadline tests of the test-only helper `read_echo_until_marker`
  (`src-tauri/src/mux/session/pane/tests.rs`, module `restored_echo_wait`) from
  depending on the helper reader thread being scheduled within `SHORT_DEADLINE`
  (200 ms). Today they can fail intermittently under CPU contention, which
  would add new intermittent failures to full `--lib` results.
- Keep checking the deadline behavior (reason `DeadlinePassed`, the absolute
  deadline, the bound on elapsed time) and the failure message (collected
  bytes, deadline wording) in a form that does not depend on scheduling.
- Add a test that detects the regression deterministically by injecting a
  helper-thread start delay. It uses no CPU load.

## Root Cause

`read_echo_until_marker` computes `deadline_at = Instant::now() + deadline`
before it calls `std::thread::spawn` for the reader thread (`tests.rs` lines
3312-3314).

- `read_echo_until_marker_reports_the_deadline_with_the_collected_bytes`
  asserts `collected == b"restored-"` (`assert_eq` on the whole `MissingEcho`,
  lines 3617-3624) and `text.contains("\"restored-\"")` (line 3631).
- `read_echo_until_marker_deadline_is_absolute_while_non_marker_bytes_trickle_in`
  asserts `collected.starts_with(b"restored-")` (lines 3655-3659) and
  `text.contains("\"restored-")` (line 3666).

If the spawned thread's first read happens more than 200 ms after
`deadline_at` is computed, `recv_timeout` returns `Timeout` with `collected`
empty. The reason assertion still holds, but the collected-bytes assertions
fail.

## Technical Requirements

### Functional Requirements

- **FR1: Deadline-test assertions hold regardless of reader-thread start time.**
  The withheld-data deadline test and the trickle deadline test pass whether
  or not the helper reader thread delivered its first chunk before the
  deadline. Each test still asserts:
  - (a) the outcome is Missing with reason
    `MissingReason::DeadlinePassed(SHORT_DEADLINE)`;
  - (b) elapsed time < `GENEROUS_BOUND`;
  - (c) the collected bytes match the scripted script. For the withheld-data
    test that means empty or exactly `b"restored-"`. For the trickle test it
    means empty, or starting with `b"restored-"` and followed only by `.`
    bytes;
  - (d) the `describe_missing_echo` text contains `deadline`, `200ms`, and the
    rendering of the bytes actually collected.
- **FR2: Deterministic start-delay regression test.**
  A test injects a reader-thread start delay deterministically. Its scripted
  reader's first read is gated and returns nothing until the test releases it,
  which happens only after `read_echo_until_marker` has returned at
  `SHORT_DEADLINE`. The test then runs the same deadline checks as FR1 against
  that delayed-start reader and expects `DeadlinePassed(SHORT_DEADLINE)` with
  empty collected bytes. If the deadline checks again require the prefix to
  have been collected (`collected == b"restored-"` /
  `starts_with(b"restored-")` unconditionally), this test fails. It uses only
  sleeps or flag waits, never CPU load.
- **FR3: Deterministic check that collected bytes appear in the deadline
  failure message.**
  `describe_missing_echo` is checked with a constructed
  `MissingEcho { reason: DeadlinePassed(SHORT_DEADLINE), collected: b"restored-" }`.
  The text contains `"restored-"`, `deadline` and `200ms`. The deadline tests
  used to give this coverage. Now it does not depend on thread scheduling.

### Non-Functional Requirements

- **NFR1: No intentional CPU load.**
  No test, acceptance criterion, verification step or test scenario puts
  intentional load on the CPU: no busy-loop threads or spin loops, no
  `stress` / `stress-ng`, and no one-process-per-core or core-count parallel
  runs. This holds even though reproduction step 1 of the task asks for it.
  The reader-thread scheduling delay is reproduced deterministically instead
  (FR2). The completion item "the reproduction no longer triggers the
  symptom" is verified by that deterministic test.
- **NFR2: Test-only change.**
  Only `src-tauri/src/mux/session/pane/tests.rs` changes under `src-tauri/`.
  Production code, `Cargo.toml` and `Cargo.lock` stay unchanged. No new
  dependencies: std only.
- **NFR3: Bounded, non-hanging tests.**
  Every new or changed test ends in bounded time. The injected start delay is
  released by the test or ends by `SCRIPTED_READER_SAFETY_LIMIT` (6 s), so a
  broken helper fails an assertion instead of hanging the suite.
- **NFR4: Platform and feature gating kept.**
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  compiles. The `restored_echo_wait` module stays platform-neutral (std only).

## Acceptance Criteria

- [ ] **AC-1 (FR1):** `read_echo_until_marker_reports_the_deadline_with_the_collected_bytes`
  asserts reason `DeadlinePassed(SHORT_DEADLINE)`, elapsed < `GENEROUS_BOUND`,
  collected empty or exactly `b"restored-"`, and failure text with `deadline`,
  `200ms` and the bytes actually collected. It no longer requires the prefix
  to have arrived before the deadline.
- [ ] **AC-2 (FR1):** `read_echo_until_marker_deadline_is_absolute_while_non_marker_bytes_trickle_in`
  asserts reason `DeadlinePassed(SHORT_DEADLINE)`, elapsed < `GENEROUS_BOUND`,
  collected empty or starting with `b"restored-"` followed only by `.` bytes,
  and failure text with the bytes actually collected. It no longer requires
  the prefix to have arrived before the deadline.
- [ ] **AC-3 (FR2, NFR1, NFR3):** A test whose scripted reader's first read is
  held until after `read_echo_until_marker` returns passes the same deadline
  checks with `DeadlinePassed(SHORT_DEADLINE)` and empty collected bytes. It
  fails (red) when run against the current strict assertions
  (`collected == b"restored-"` / unconditional `starts_with(b"restored-")`).
- [ ] **AC-4 (FR3):** A deterministic test of `describe_missing_echo` with
  `DeadlinePassed(SHORT_DEADLINE)` and collected `b"restored-"` checks for
  `"restored-"`, `deadline` and `200ms` in the text.
- [ ] **AC-5 (NFR1, NFR2, NFR4):** The full `--lib` suite
  (`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`)
  passes with every `restored_echo_wait` test passing. `cargo check` compiles.
  The diff under `src-tauri/` touches only
  `src-tauri/src/mux/session/pane/tests.rs`. No verification step loads the
  CPU on purpose.

## Implementation Approach

- The fix lives in the tests and their scripted reader. The body and
  semantics of `read_echo_until_marker` stay unchanged (A2).
- `SHORT_DEADLINE` stays 200 ms (A1).
- How the start delay is injected (a new gate before the first chunk on
  `ScriptedEchoReader`, or a separate gated reader) and whether the deadline
  checks are factored into a shared helper are chosen at planning (A5). The
  FR2 test uses the same deadline checks as the FR1 tests.

### Existing test infrastructure

Rust built-in `cargo test` with inline `#[cfg(test)]` modules. The target
module `restored_echo_wait` in `src-tauri/src/mux/session/pane/tests.rs` has
`ScriptedEchoReader` with `AfterChunks::{EndOfData, Error, Withhold, Trickle}`,
`scripted_reader()`, `SHORT_DEADLINE` = 200 ms, `GENEROUS_BOUND` = 3 s, and
`SCRIPTED_READER_SAFETY_LIMIT` = 6 s.

### Reference impact

| Symbol | Change | Affected paths | Note |
|---|---|---|---|
| `mux::session::pane::tests::restored_echo_wait::read_echo_until_marker_reports_the_deadline_with_the_collected_bytes` | Body changes; rename not planned (A4) | `src-tauri/src/mux/session/pane/tests.rs`, `test-docs/restored-pane-read-flake-fix/task0001.tests.yaml` | Listed under AC-4 (line 42) of the predecessor test record. Update it only if the test is renamed. The predecessor SPEC.md does not mention this test name. |
| `mux::session::pane::tests::restored_echo_wait::read_echo_until_marker_deadline_is_absolute_while_non_marker_bytes_trickle_in` | Body changes; rename not planned (A4) | `src-tauri/src/mux/session/pane/tests.rs`, `test-docs/restored-pane-read-flake-fix/task0001.tests.yaml` | Listed under AC-4 (line 43) of the predecessor test record. Update it only if the test is renamed. |
| `ScriptedEchoReader` / `scripted_reader` / `AfterChunks` (`restored_echo_wait`) | May be extended with a start-delay gate (A5); no deletion or rename planned | `src-tauri/src/mux/session/pane/tests.rs` | Used by the 7 scripted-reader tests in the same module (lines 3483-3692). No reference in the predecessor SPEC.md or test-docs. |
| `SHORT_DEADLINE` | Unchanged (A1) | `src-tauri/src/mux/session/pane/tests.rs` | Referenced only in `tests.rs`. The `200ms` text assertion depends on its value. |

### Commands

- Check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Unit tests: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/restored-pane-echo-deadline-start-delay/**`
- `test-docs/restored-pane-echo-deadline-start-delay/**`

`feature-docs/restored-pane-echo-deadline-start-delay/**` covers
`REQUIREMENTS.md`, `SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`,
`phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These
are generated and owned by the phase documents and by
`references/phase-state.md`; this section cites them and restates none of
their rules.

`test-docs/restored-pane-echo-deadline-start-delay/**` covers
`test-docs/restored-pane-echo-deadline-start-delay/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/restored-pane-echo-deadline-start-delay/` directory at all; the
declared `test-docs/restored-pane-echo-deadline-start-delay/**` entry is
still correct in that case — a declared path that never materializes is not
a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1 (AC-1):** Run the withheld-data deadline test normally. It passes
  with collected `b"restored-"`.
- [ ] **TS-2 (AC-2):** Run the trickle deadline test normally. It passes with
  collected starting with `b"restored-"`.
- [ ] **TS-3 (AC-3):** A delayed-start scripted reader (first read gated until
  released after the call returns) gives `DeadlinePassed(SHORT_DEADLINE)`
  with empty collected bytes and passes the shared deadline checks. Red check:
  the same test fails against the strict pre-change assertions.
- [ ] **TS-4 (AC-4):** `describe_missing_echo` with `DeadlinePassed(200ms)` and
  collected `restored-`: the text contains `"restored-"`, `deadline`,
  `200ms`.
- [ ] **TS-5 (AC-5):** Full `--lib` run and `cargo check` pass. Inspect the diff
  to confirm only `src-tauri/src/mux/session/pane/tests.rs` changed under
  `src-tauri/`. Unit-test run command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.

### E2E Tests

**Existing E2E tests**: None (`test/README.md`: no `docker-compose.e2e.yml`,
no `e2e-tests/`).
**Run command**: Not detected

### Edge Cases

- [ ] The reader thread starts after the deadline: collected is empty, the
  reason is still `DeadlinePassed`, and the failure text shows 0 bytes (TS-3).
- [ ] The reader thread starts just before the deadline in the trickle test:
  collected is `restored-` plus zero or more `.` bytes (TS-2).
- [ ] In the withheld-data test, the 64-byte read buffer hands out `restored-`
  as one chunk, so collected is either empty or exactly `restored-` and never
  a partial prefix.
- [ ] The injected start delay is released by the test after the call
  returns. If release never happens, `SCRIPTED_READER_SAFETY_LIMIT` (6 s,
  measured from reader construction) ends the reader, so the suite does not
  hang.
- [ ] Main-thread wake-up delay is covered only by the existing generous upper
  bound (`GENEROUS_BOUND` = 3 s). No lower bound on elapsed time is asserted.

### Load / Stress Tests

None. No scenario puts intentional load on the CPU (NFR1, A3).

## Assumptions

- **A1:** Of the two remedies the task offers (relax the collected-bytes
  assertion, or raise `SHORT_DEADLINE`), this feature relaxes the assertion.
  Raising `SHORT_DEADLINE` only moves the scheduling threshold and cannot be
  checked deterministically for any delay. `SHORT_DEADLINE` stays 200 ms, and
  the `200ms` text check stays.
- **A2:** The body and semantics of `read_echo_until_marker` stay unchanged,
  including the absolute deadline measured from the start of the call
  (predecessor FR2). Moving the deadline after `thread::spawn` would not
  remove the dependence, because `spawn` returns before the thread runs. The
  fix lives in the tests and their scripted reader.
- **A3:** The task's reproduction step 1 (busy-loop threads on every logical
  CPU) is not carried out. Per the user's no-intentional-CPU-load rule, the
  completion item "the reproduction no longer triggers the symptom" is
  verified by the deterministic start-delay test (FR2 / AC-3).
- **A4:** The two existing test names stay unchanged, so the test-docs-records
  rename rule does not apply and
  `test-docs/restored-pane-read-flake-fix/task0001.tests.yaml` (AC-4 lines
  42-43) needs no update. If planning renames either test, that record's
  `acceptance_tests[].tests` entries must be updated to the new names in this
  feature, per `.claude/rules/test-docs-records.md`.
- **A5:** Planning chooses how the start delay is injected: a new gate before
  the first chunk on `ScriptedEchoReader`, or a separate gated reader.
  Planning also chooses whether the deadline checks are factored into a
  shared helper. The only requirement is that the FR2 test uses the same
  deadline checks as the FR1 tests.
- **A6:** The predecessor feature's prose
  (`feature-docs/restored-pane-read-flake-fix/SPEC.md`, including its
  busy-loop AC-4 / TS-4 / A4) stays as written.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

None. No requirement has `status: tbd`.

## References

- Predecessor feature SPEC: `feature-docs/restored-pane-read-flake-fix/SPEC.md`
- Predecessor test record: `test-docs/restored-pane-read-flake-fix/task0001.tests.yaml`
- Test-docs record rule: `.claude/rules/test-docs-records.md`
