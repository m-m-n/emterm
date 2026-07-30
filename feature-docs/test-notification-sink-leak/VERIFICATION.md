# Verification Document: Stop the unit test suite from emitting real desktop notifications

## Overview

**Feature**: test-notification-sink-leak /
**SPEC.md**: `feature-docs/test-notification-sink-leak/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/test-notification-sink-leak/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: not applicable (no new production code; this feature changes a
  test's setup only)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run the target test `pump_all_applies_daemon_agent_status_update_to_model` | Passes; the mux-pane-42 model entry has state "blocked" and revision 7 | Unit |
| TS-2 | Run the whole library unit test suite | All tests pass; no regression in the existing agent-notification test group | Integration |
| TS-3 | Inspect the target test's app construction | The app comes from the capturing-sink helper; the production sink is not used | Unit (inspection) |
| TS-4 | Scan the test module for other apps built with the plain constructor that push a blocked/done agent transition through the pump | Findings reported; none expected per the bug report | Inspection |
| TS-5 | Inspect the feature diff | Only the test module of `src-tauri/src/app.rs` and `feature-docs/` are touched | Inspection |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: not configured for this project beyond `cargo check`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 implemented | TS-1 + TS-3 |
| SC-2 | All unit tests pass | TS-2 |
| SC-3 | Format check passes | `cargo fmt … --check` exits 0 |
| SC-4 | No desktop notification during a test run | Manual check MC-1 |
| SC-5 | Diff scope limited to the test module and feature-docs | TS-5 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3 |
| NFR1 | task0001 | TS-2, TS-4, MC-1 |
| NFR2 | task0001 | TS-5 |

## E2E Testing

No E2E framework exists in this repository (`e2e_test_command` is empty in
workflow.yaml). Nothing to run.

## Manual Testing (E2E Not Possible)

- [ ] MC-1: On a desktop session with a live notification service, run the unit
      test suite and confirm no notification appears. Automated verification
      cannot observe the absence of an OS-level popup, and the verify phase may
      run without a session bus — so this item is for the human evaluator.

## Performance / Security Verification

Not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 5 | 2 | 0 | 3 (inspection) |
| Code quality | 1 | 1 | 0 | 0 |
| Success criteria | 5 | 4 | 0 | 1 |
