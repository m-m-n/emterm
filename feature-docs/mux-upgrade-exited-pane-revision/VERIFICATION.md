# Verification Document: mux-upgrade-exited-pane-revision

## Overview

**Feature**: mux-upgrade-exited-pane-revision / **SPEC.md**: `feature-docs/mux-upgrade-exited-pane-revision/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-upgrade-exited-pane-revision/IMPLEMENTATION.md`

Run every command from the project root (the integration worktree root).

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only build: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not measured, because no coverage tool is configured.
  Every scenario below must pass.
- Baseline: a failure already present at the task's base (the test record's
  baseline_failures) is not attributed to this feature.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | A live pane at revision 0 is snapshotted, then receives an accepted OSC 7501 error report (revision becomes 1), is marked exited, and is refreshed | The entry's agent_revision is 1. Its program_records equal the pane's current table export, including the error record. Its state and name equal the pane's current values. Every other field equals its snapshot-time value | Unit |
| TS-2 | A live pane receives an OSC 777 Set, is snapshotted, is marked exited, receives a Clear, and is refreshed (replaces the old "since exited untouched" expectation) | The entry has no agent_state and no agent_name, and its agent_revision equals the pane's current revision. Every other field equals its snapshot-time value | Unit |
| TS-3 | A pane already exited at snapshot time receives an OSC 777 event and an OSC 7501 report after the snapshot, and is refreshed | The entry's state, name, revision and records equal the pane's current values. Every other field equals its snapshot-time value | Unit |
| TS-4 | The TS-1 document is restored, and the session-wide post-snapshot resync runs while a client subscribes to the restored manager's notification channel | Exactly one AgentStatusUpdate arrives for the pane, with replay_derived true, revision 1, and a program-status summary in the error state | Unit (async) |
| TS-5 | The full --lib test command and the --no-default-features cargo check are run | Both pass. The existing live-pane refresh tests and the not-in-manager refresh test pass with their bodies unmodified | Integration |
| TS-6 | Inspect the exited-pane path of the refresh | The state, name, revision and records come from a single agent-status lock acquisition, with no second acquisition on that path | Inspection |
| TS-7 | Inspect the refresh doc comment, the exited-branch inline comment, and the daemon call-site comment | Each describes the new behavior. None says an exited pane is left untouched apart from its records | Inspection |

TS-6 and TS-7 do not come from SPEC.md. They are inspection items added here
for FR1's single-lock requirement and for FR5, which no unit test can verify.

## Code Quality Verification

- Format: none. workflow.yaml has an empty format_command. Formatting changes
  stay inside the task's files.
- Static analysis: none configured.
- Test-record resolution check (`.claude/rules/test-docs-records.md`): the
  output of `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
  lists the replacement test name as a test. That name is the one now written
  in `test-docs/agent-exit-after-icon/task0006.tests.yaml` and
  `test-docs/osc7501-program-status/task0007.tests.yaml`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-7 |
| SC-2 | All test scenarios pass | TS-5 run, plus TS-6 and TS-7 inspection |
| SC-3 | Code review is completed | Review phase result in workflow.yaml |
| SC-4 | SPEC AC-1 | TS-1 |
| SC-5 | SPEC AC-2 | TS-2, TS-3 |
| SC-6 | SPEC AC-3 | TS-1, TS-2, TS-3 |
| SC-7 | SPEC AC-4 | TS-4 |
| SC-8 | SPEC AC-5 | TS-5 |
| SC-9 | The defect no longer reproduces with the reproduction steps from the request | TS-1 |
| SC-10 | A regression test exists that applies a real report before the handoff, then restores and resyncs | TS-1, TS-4 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-6 |
| FR2 | task0001 | TS-1, TS-2, TS-3 |
| FR3 | task0001 | TS-5 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-7 |
| NFR1 | task0001 | TS-5 (TS-1 to TS-4 run under the --lib test command) |
| NFR2 | task0001 | TS-5 |

## E2E Testing

None. The project has no E2E framework for this path (SPEC.md: existing E2E
tests: none).

## Manual Testing (E2E Not Possible)

None. The defect window is a report reaching an exited pane between the
snapshot and the refresh of a hot-upgrade, and its timing cannot be reproduced
reliably by hand. TS-1 to TS-4 reproduce it deterministically.

## Performance / Security Verification (if applicable)

Not applicable. SPEC.md has no performance or security requirement, and
THREAT-MODEL.md's verdict is no-applicable-threat, so there is no TM-n item.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default and CLI-only check) | 2 | 2 | 0 | 0 |
| Unit tests (TS-1 to TS-4) | 4 | 4 | 0 | 0 |
| Full suite and CLI-only check (TS-5) | 1 | 1 | 0 | 0 |
| Inspection (TS-6, TS-7) | 2 | 0 | 0 | 2 |
| Test-record resolution check | 1 | 1 | 0 | 0 |
| Total | 10 | 8 | 0 | 2 |
