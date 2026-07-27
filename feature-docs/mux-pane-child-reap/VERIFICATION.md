# Verification Document: mux pane child process reaping

## Overview

**Feature**: mux-pane-child-reap
**SPEC.md**: `feature-docs/mux-pane-child-reap/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-pane-child-reap/IMPLEMENTATION.md`

## Build Verification

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors.
- CLI-only build (NFR5, TS-13):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  — expected exit code 0 (this variant is not in the approved-command set;
  the verify phase may need a one-off approval for it).

## Test Verification

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Expected: all tests pass. Note (project memory): unit tests live under
  `--lib`; the new tests are parallel-safe (PID-keyed assertions), so no
  test-thread restriction applies to them.
- Coverage: no numeric coverage gate is configured for this project;
  coverage is scenario-based per the table below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `mark_exited` on a pane with no child (test constructor) | No reap started, no panic; existing postconditions hold | Unit |
| TS-2 | `mark_exited` called twice on the same pane | Second call finds no child handle; at most one reap handoff | Unit |
| TS-3 | `mark_exited` clears writer/master and sets exited | Existing behavior preserved (`test_mark_exited_clears_writer_and_master` keeps passing) | Unit |
| TS-4 | Reap procedure on a promptly exiting child | Child reaped within the grace period; no kill sent | Unit |
| TS-5 | Reap procedure on a child that outlives the grace period | Kill sent after the grace period, then reaped; procedure returns within a deadline | Unit |
| TS-6 | Blocking reap fails (e.g. already reaped elsewhere) | Procedure returns without panic; failure logged at warn or above | Unit |
| TS-7 | `spawn_pty` retains the child handle; pane stores it | Handle present from spawn through pane construction | Integration |
| TS-8 | Spawn via `spawn_pty`, tear down via `mark_exited` | Spawned PID leaves the process table or is non-zombie within a deadline | Integration |
| TS-9 | FR9 regression: open/close panes N times | No test-owned child remains in state Z afterwards | Integration |
| TS-10 | `mark_exited` on a pane with a still-running child | Returns without waiting for the child's exit (bounded, generous margin) | Unit |
| TS-11 | Kill fails (child already gone) during escalation | Error ignored; procedure proceeds to the blocking reap and returns | Unit |
| TS-12 | No external-PID kill path exists (NFR2) | Only daemon-owned spawn handles are killed/waited; no API accepts an outside PID | Manual (inspection) |
| TS-13 | CLI-only build unaffected (NFR5) | `--no-default-features` check exits 0 | Build |

TS-8/TS-9 are Unix-gated and skip cleanly when the environment cannot open
a PTY (SPEC A4) — a skip is recorded as such, not silently counted as pass.

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: none configured beyond compiler warnings.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | Traceability table below; all mapped tests pass |
| SC-2 | All test scenarios pass | Test command above; TS-1..TS-11, TS-13 green |
| SC-3 | `cargo test` passes on the default feature set | Test command above |
| SC-4 | `--no-default-features` check passes | TS-13 |
| SC-5 | Reap failures visible at warn level in release logs | TS-6 + inspection: every reap-failure log site uses warn or above |
| SC-6 | No pane-teardown path bypasses the reap | TS-8/TS-9 + inspection of the four call sites (handlers.rs `handle_destroy_pane` / `handle_destroy_window`, daemon.rs `graceful_shutdown`, `run_pane_exit_task` → `handle_destroy_pane`) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-7 |
| FR2 | task0001 | TS-1, TS-7 |
| FR3 | task0001 | TS-2, TS-3 |
| FR4 | task0001 | TS-10 |
| FR5 | task0001 | TS-4, TS-5 |
| FR6 | task0001 | TS-4, TS-5, TS-6 (these run against the standalone procedure without a PTY or a pane — the testability FR6 requires) |
| FR7 | task0001 | TS-6, TS-11 |
| FR8 | task0001 | TS-8 (plus SC-6 inspection) |
| FR9 | task0001 | TS-9 |
| NFR1 | task0001 | TS-10 |
| NFR2 | task0001 | TS-12 |
| NFR3 | task0001 | TS-1, TS-6 |
| NFR4 | task0001 | TS-6 |
| NFR5 | task0001 | TS-13 (Linux); MT-3 (Windows cross-build, optional) |

## E2E Testing

Not applicable — no E2E framework exists in this repository
(workflow.yaml `e2e_test_command` is empty; SPEC "E2E Tests": none
detected).

## Manual Testing (E2E Not Possible)

- [ ] TS-12: code inspection — no code path accepts an externally supplied
      PID for kill/wait; the reaper's only inputs are daemon-owned spawn
      handles plus timing values (NFR2).
- [ ] MT-1: real-daemon soak — run the mux daemon, open and close several
      panes through each path (shell `exit`, close pane, close window,
      daemon shutdown), then check
      `ps --ppid <daemon_pid> -o pid,stat,comm`: no entry in state `Z`
      (REQUIREMENTS KPI).
- [ ] MT-2: wedged-shell force close — in a pane, run a foreground process
      that ignores hangup; close the pane; the pane closes immediately and
      the process disappears from the process table within ~1 s
      (grace 500 ms + kill).
- [ ] MT-3 (optional): Windows cross-build compiles (`make win-build`) —
      NFR5's Windows half; skip if the toolchain is unavailable.

## Performance / Security Verification

- NFR1 (performance): TS-10 — `mark_exited` returns without waiting on the
  child; reaping runs on a detached thread, so other panes' I/O latency is
  unaffected by design.
- NFR2 (security): TS-12 (manual inspection).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit | TS-1..TS-6, TS-10, TS-11 | 8 | 0 | 0 |
| Integration | TS-7..TS-9 | 3 | 0 | 0 |
| Build | TS-13 (+ default build/format checks) | 1 | 0 | 0 |
| Manual | TS-12, MT-1..MT-3 | 0 | 0 | 4 |
