# Feature: restored-pane-read-flake-fix

## Overview

`mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`
fails intermittently when the first PTY echo read returns only a prefix of the
echo. This feature replaces the test's single read with a bounded
read-until-marker loop, puts the collected bytes in the failure message, adds a
deterministic regression test, and rewords the comment that calls the read
deterministic. Requirements: `REQUIREMENTS.md`.

## Objectives

- Stop the target test from failing intermittently when the first PTY echo read
  returns only a prefix (13.5 % of unmodified full `--lib` runs, 18.7 % alone
  under full CPU load), so that full `--lib` results can be read again.
- When the target test does fail, its message shows the bytes it read.

## User Stories

### US1: Readable full `--lib` results
As a developer running the full `--lib` suite, I want the target test to pass
when the echo arrives in pieces, so that full `--lib` results can be read again.

**Acceptance Criteria:**
- [ ] AC-1 (FR1, FR2): The target test passes when the echo arrives in several
  chunks whose first chunk is shorter than the 14-byte marker.
- [ ] AC-3 (FR4): A test exists that deterministically produces a prefix-only
  first read and passes only with the read-until-marker logic.
- [ ] AC-4 (FR1, FR2, NFR4): Running the original reproduction (target test
  alone, 150 runs, while busy-loop threads occupy every logical CPU; 28 of 150
  failed before the fix) gives 0 failures of the target test.
- [ ] AC-5 (FR1, NFR1): The full `--lib` suite
  (`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`)
  runs with the target test passing.

### US2: Bytes in the failure message
As a developer reading a failure of the target test, I want the message to show
the bytes the test read.

**Acceptance Criteria:**
- [ ] AC-2 (FR3): When the marker does not arrive (deadline, EOF, or read error),
  the failure message contains the collected bytes.

### US3: Accurate comment and unchanged build surface

**Acceptance Criteria:**
- [ ] AC-6 (FR5): The comment in `src-tauri/src/mux/upgrade/tests.rs` near the
  `after-restore` write no longer says the pane-level test proves the read side
  deterministically.
- [ ] AC-7 (NFR2, NFR3): `Cargo.toml` / `Cargo.lock` are unchanged and the
  `--no-default-features` check compiles.

## Technical Requirements

### Functional Requirements

- **FR1:** Read until the marker arrives or a deadline passes. After writing
  `restored-write\n` through the restored pane, the test keeps reading from the
  cloned master reader and appends each chunk to a buffer. It stops when the
  buffer contains the marker `restored-write` or when a bounded deadline passes.
  A first read that returns only a prefix of the echo (1-13 bytes), or an echo
  that arrives in more than two pieces, does not fail the test as long as the
  marker is complete before the deadline.
- **FR2:** Bounded wait on a blocking reader. The wait for the echo has a
  deadline even though the cloned master reader is a blocking fd with no
  timeout. The deadline is enforced with `poll` on the master fd or with a helper
  thread that does the reads and hands chunks back through a channel with a
  receive timeout. The test never blocks without a bound.
- **FR3:** Show the bytes read when the test fails. When the deadline passes
  without the marker, when the reader returns `Ok(0)` (EOF), or when the read
  returns an error, the test fails. Its message contains every byte collected so
  far, in a readable escaped form such as `String::from_utf8_lossy(..)` with
  `{:?}`, and keeps the meaning of the current message (bytes written through the
  adopted master's writer must be readable back through the adopted master,
  echoed by the PTY line discipline).
- **FR4:** Test that catches the bug coming back. A test makes a prefix-only
  first read happen deterministically and proves that the read-back logic from
  FR1/FR3 still finds the marker. One way: feed it a scripted reader that returns
  `rest`, then `ored-write\r\n`. The test fails against the old shape (one read,
  then assert).
- **FR5:** Correct the comment that calls the read deterministic. The comment in
  `src-tauri/src/mux/upgrade/tests.rs` (lines 295-299) says
  `from_restored_pane_can_write_and_read_through_its_adopted_master` proves the
  read side "deterministically". It is reworded so it no longer claims a
  deterministic single read and instead matches the bounded read-until-marker
  check.

### Non-Functional Requirements

- **NFR1 - Test-only change:** Only test code under `src-tauri/src/`
  (`#[cfg(test)]` modules and test files) and test comments change. Production
  code paths (`MuxPane::from_restored`, `write_input`, `pane/handles.rs`) stay
  unchanged.
- **NFR2 - No new dependencies:** The fix uses dependencies already available to
  the crate's tests: std, `libc` (unix target dependency), and
  `crossbeam-channel`. `Cargo.toml` and `Cargo.lock` stay unchanged.
- **NFR3 - Platform gating kept:** The target test stays `#[cfg(unix)]`. Any
  `poll`/fd code is unix-gated, so the Windows target still compiles.
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  still compiles.
- **NFR4 - Bounded duration:** The deadline is a few seconds (for example 5 s, as
  in `pty/mod.rs` `pty_round_trip_echo_hello` and the reproduction program's loop
  mode). A passing run ends as soon as the marker is found, without waiting for
  the deadline.

## Implementation Approach

### Architecture

Test-only change. No production module, API, or data schema changes (NFR1).

### Data Flow

```
restored pane write_input("restored-write\n")
  → PTY line discipline echo ("restored-write\r\n", possibly in several chunks)
  → cloned master reader (blocking fd)
  → bounded read loop (poll on the master fd, or helper thread + channel receive timeout)
  → buffer: marker found → pass
            deadline / Ok(0) / read error → fail with the collected bytes
```

### API Design

Not applicable.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `MuxPane::from_restored` and `write_input`: exercised by the target test,
  unchanged (NFR1).
- `open_test_pty_pair` in `src-tauri/src/mux/session/pane/tests.rs`: the target
  test's PTY pair helper.
- `src-tauri/src/pty/mod.rs`: `ScriptedReader` and the accumulate-until-deadline
  read in `pty_round_trip_echo_hello` are pattern references.

**External Dependencies:**
- std, `libc` (unix target dependency), `crossbeam-channel`: already available;
  no new dependency (NFR2).

### File Structure

```
src-tauri/src/mux/
├── session/pane/tests.rs   # target test body (FR1-FR3), regression test (FR4)
└── upgrade/tests.rs        # comment rewording (FR5)
```

### Assumptions

- **A1:** The test function name
  `from_restored_pane_can_write_and_read_through_its_adopted_master` stays
  unchanged, so the test-docs-records rename rule does not apply and the
  `test-docs/` records that list it need no update.
- **A2:** FR4's regression test may be a new unit test of a small test-only
  read-until-marker helper, driven by a scripted `Read` implementation (the
  `ScriptedReader` style in `src-tauri/src/pty/mod.rs` is a precedent). It sits
  next to the target test in `src-tauri/src/mux/session/pane/tests.rs`. The name
  and exact placement are left to planning.
- **A3:** Choosing between `poll` on the master fd and a helper thread plus
  channel timeout for the deadline (FR2) is left to planning. Both meet the
  requirement.
- **A4:** In AC-4, "every logical CPU" means one busy-loop thread per logical CPU
  on the verifying machine (24 in the investigation). The busy-loop load is
  test-time tooling run outside the repository and is not committed.
- **A5:** AC-5 counts only the target test. Other known intermittent `--lib`
  failures (`mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor`,
  `mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`)
  and the `inherited_pty.rs` single-read tests (investigation follow-up
  candidates 2 and 3) are out of scope.
- **A6:** `feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md`
  is a predecessor feature's record. It stays as written, including its quotation
  of the "deterministic" comment and its line numbers.
- **A7:** The PTY keeps the kernel's default termios (ICANON, ECHO, ONLCR on), so
  the expected echo is `restored-write\r\n` and the 14-byte marker
  `restored-write` is a valid success condition. This is a preserved fact from
  the investigation.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every task's
`files` entries in `workflow.yaml` (`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/restored-pane-read-flake-fix/**`
- `test-docs/restored-pane-read-flake-fix/**`

`feature-docs/restored-pane-read-flake-fix/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/restored-pane-read-flake-fix/**` covers
`test-docs/restored-pane-read-flake-fix/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/restored-pane-read-flake-fix/` directory at all; the declared
`test-docs/restored-pane-read-flake-fix/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1, AC-3): A scripted reader returns a strict prefix of
  `restored-write\r\n`, then the rest (and a variant that splits into three or
  more pieces). The read-back logic returns success with the marker found.
- [ ] TS-2 (AC-2): A scripted reader that never completes the marker (returns a
  prefix, then EOF / no more data): the read-back logic reports failure, and the
  failure text contains the collected bytes.
- [ ] TS-3 (AC-1): `from_restored_pane_can_write_and_read_through_its_adopted_master`
  passes against a real PTY pair with the new read-until-marker logic.

### Integration Tests
- [ ] TS-5 (AC-5, AC-7): Full `--lib` run with the target test passing;
  `--no-default-features` cargo check compiles; `git diff` shows no Cargo
  manifest/lock changes.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] First read returns 1-13 bytes of the echo: the test keeps reading and
  passes once the marker is complete (TS-1).
- [ ] Echo arrives in three or more pieces: the test passes once the marker is
  complete (TS-1).
- [ ] Reader returns `Ok(0)` before the marker: the test fails with the collected
  bytes (TS-2).
- [ ] Deadline passes or the read returns an error before the marker: the test
  fails with the collected bytes (FR3).

### Performance Tests
- [ ] TS-4 (AC-4): Run the target test alone 150 times (`--exact`) while
  busy-loop threads occupy every logical CPU. Expect 0 failures.

### Inspection
- [ ] TS-6 (AC-6): Inspect the reworded comment in
  `src-tauri/src/mux/upgrade/tests.rs`.

## Security Considerations

Not applicable.

## Error Handling

| Condition | Result |
|-----------|--------|
| Deadline passes without the marker | Test fails; message contains every byte collected so far |
| Reader returns `Ok(0)` (EOF) | Test fails; message contains every byte collected so far |
| Read returns an error | Test fails; message contains every byte collected so far |

The failure message keeps the meaning of the current message: bytes written
through the adopted master's writer must be readable back through the adopted
master, echoed by the PTY line discipline (FR3).

## Performance Optimization

- The deadline is a few seconds (for example 5 s). A passing run ends as soon as
  the marker is found, without waiting for the deadline (NFR4).

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 through AC-7 are met
- [ ] Code review is completed

## Open Questions

None.

## References

- Requirements: `feature-docs/restored-pane-read-flake-fix/REQUIREMENTS.md`
- Predecessor investigation: `feature-docs/restored-pane-read-flake-investigation/INVESTIGATION.md`
