# Verification Document: mux attach legacy daemon recovery

## Overview

**Feature**: mux-attach-legacy-recovery /
**SPEC.md**: `feature-docs/mux-attach-legacy-recovery/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-attach-legacy-recovery/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Note: per test/README.md, the tabs.rs replay tests can flake in parallel;
  re-run with `-- --test-threads=1` when they do (approved variant exists).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Fake legacy (v1) daemon on the socket → attach pre-bridge helper | Legacy daemon shut down, new daemon spawned, handshake accepted | Integration (in `--lib` test module) |
| TS-2 | Fake compatible daemon on the socket → attach pre-bridge helper | Success without any spawn; fake daemon still owns the socket | Integration (in `--lib` test module) |
| TS-3 | No socket present → attach pre-bridge helper | Existing "No mux sessions to attach to" error, unchanged | Unit |
| TS-4 | Existing daemon/mux test suite after the spawn extraction | All pre-existing tests pass unchanged | Regression (`--lib`) |

## Code Quality Verification

- Format: none configured in workflow.yaml (project PostToolUse hook
  formats on write).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Attach recovers from a legacy daemon | TS-1 passes |
| SC-2 | Compatible-daemon attach unchanged | TS-2 passes |
| SC-3 | Daemon-absent attach error unchanged | TS-3 passes |
| SC-4 | No regression in other mux entry points | TS-4 passes |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-4 (behavior parity after extraction) |
| FR2 | task0001 | TS-1, TS-2, TS-3 |
| FR3 | task0001 | TS-1, TS-2, TS-3 (the tests themselves) |
| NFR1 | task0001 | TS-4 + build check (cfg gates compile) |

## E2E Testing

No E2E infrastructure in this project (test/README.md).

## Manual Testing (E2E Not Possible)

- [ ] MT-1: On a machine with a long-lived daemon started by an older
      binary, run `emterm mux attach` after upgrading eMterm and confirm
      the session reattaches without manually killing the daemon.
      (Requires a real binary upgrade; deferred to the user.)

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 4 | 4 | 0 | 0 |
| Manual | 1 | 0 | 0 | 1 |
