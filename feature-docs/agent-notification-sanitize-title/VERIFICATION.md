# Verification Document: agent-notification-sanitize-title

## Overview

**Feature**: agent-notification-sanitize-title
**SPEC.md**: `feature-docs/agent-notification-sanitize-title/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/agent-notification-sanitize-title/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no numeric project-wide target is defined; all four test
  scenarios below must be covered.
- Known flake: the `tabs.rs` replay tests can fail non-deterministically
  under parallel execution. On a failure there, re-run the same command with
  `-- --test-threads=1` appended before judging TS4.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Tab title containing a CSI sequence (e.g. ESC [ ... m) passed to `agent_notification_body` | Returned body contains no escape / CSI bytes | Unit |
| TS2 | Tab title containing C0 control characters | No control character survives into the body | Unit |
| TS3 | Normal tab title | Embedded into the body as before (no regression) | Unit |
| TS4 | Whole existing `--lib` suite | Stays green (single-thread re-run on `tabs.rs` replay flake) | Regression |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- Static analysis: none configured beyond the compiler (build verification
  above)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | `agent_notification_body` passes the title through the existing `sanitize_title` internally (the choke point closing both call sites) | Code review of `src-tauri/src/notifications.rs` + TS1 / TS2 passing |
| SC2 | A unit test pinning that CSI / control-character titles do not survive into the body exists in `notifications::tests` | TS1 / TS2 present in the inline test module and passing |
| SC3 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes | Test verification command above (TS4) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2, TS3 |
| FR2 | task0001 | TS1, TS2 |
| NFR1 | task0001 | TS1; code review confirms no new sanitizer implementation and `sanitize_title` unmodified |
| NFR2 | task0001 | TS3, TS4 |

## Manual Testing (E2E Not Possible)

None required. Every scenario is a pure-function unit test; the change does
not alter notification dispatch, rendering, UI, or design tokens (the design
step was skipped), so no human-judgment check is needed. The project defines
no E2E test command, so there is no E2E section.

## Performance / Security Verification

- Security (finding 7dd413bdd9289905): untrusted OSC 0/2-derived tab titles
  are sanitized at the `agent_notification_body` choke point before reaching
  `notify_rust::Notification::body` / D-Bus — verified by TS1 and TS2.
- Performance: no requirement; the sanitizer's existing raw-input cap already
  bounds work on pathological titles (pre-existing behavior, unchanged).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests (TS1–TS3) | 3 | 3 | 0 | 0 |
| Regression (TS4) | 1 | 1 | 0 | 0 |
| Format | 1 | 1 | 0 | 0 |
| Success criteria (SC1–SC3) | 3 | 3 | 0 | 0 |
