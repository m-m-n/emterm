# Verification Document: capability-query-failure-warn

## Overview

**Feature**: capability-query-failure-warn / **SPEC.md**: `feature-docs/capability-query-failure-warn/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/capability-query-failure-warn/IMPLEMENTATION.md`

Run every command from the worktree root. Do not `cd` into `src-tauri/`.

## Build Verification

- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: both exit with code 0, with no errors and no new warnings in `src-tauri/src/callbacks.rs`.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured, so no percentage target
  applies. Every scenario below must pass, either automated or by review.
- If an unrelated test (the tabs replay tests or the tmux_sockets discovery
  test) fails, re-run it alone before recording it as a failure. These tests
  are known to fail intermittently under parallel execution.

### Test Scenarios from SPEC.md

TS-1 to TS-8 correspond to SPEC TS1 to TS8. TS-9 to TS-13 are review checks
derived from SPEC requirements that have no SPEC test scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Warn decision helper: previous state none, result failure "e1" | Warn; next state "e1" (AC1) | Unit |
| TS-2 | Warn decision helper: previous state "e1", result failure "e1" | No warn; next state "e1" (AC2) | Unit |
| TS-3 | Warn decision helper: previous state "e1", result failure "e2" | Warn; next state "e2" (AC3) | Unit |
| TS-4 | Warn decision helper: previous state "e1", result success, then failure "e1" chained through the returned state | The success returns no warn and next state none; the failure then returns warn (AC4) | Unit |
| TS-5 | Worker with an injected fake that always fails with "e1": a notification with a metacharacter, one without, one with | Exactly 2 queries and 3 sends in order. The 1st and 3rd sends are escaped exactly as today; the 2nd is unchanged. No log capture is used (AC5, AC7). | Unit (worker) |
| TS-6 | The existing worker_injection_points, capability_query_skip_gate and body_markup_escape tests | All pass with unchanged assertions. Only the worker fake's error type differs (AC7). | Unit (regression) |
| TS-7 | The site that builds the warn record | It uses only the marker constant and the error text, and references no title, body, escaped value or redacted rendering (AC6) | Review |
| TS-8 | `cargo test --lib` and `cargo check --no-default-features` (commands above) | Both succeed (AC8, AC9) | Integration |
| TS-9 | Doc comments of the unix notify_worker and at its capability-query gate call | They describe the warn on query failure and the transition-only rule (FR5) | Review |
| TS-10 | Log levels introduced by the diff | The new record is warn level. The diff adds no debug-level or info-level record. (NFR2) | Review |
| TS-11 | Previous-failure state and caching | The state is one worker-local optional text value, with no static, global, thread-local or sink-shared storage. It is never read by an escape function, and no capability result is kept between notifications. (FR3, NFR3) | Review |
| TS-12 | Scope of the diff | Changes stay inside unix-only code in `src-tauri/src/callbacks.rs` and its test file. The Windows notify_worker and spawn_notify_worker are unchanged, and `src-tauri/Cargo.toml` and the lock file are unchanged. (NFR4, NFR5) | Review |
| TS-13 | Redaction order in the unix notify_worker | The redacted rendering is still built from the queue-received title and body, before the escape gate (NFR7) | Review |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`. Run it in
  check mode only; do not rewrite the whole crate.
- Static analysis: none is configured in workflow.yaml.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Every row of the coverage table below has a passing scenario |
| SC-2 | All test scenarios pass | TS-1 to TS-13 |
| SC-3 | Security requirements are satisfied | TS-5, TS-6 (fail-closed escaping), TS-7 (no notification text in the record), TS-13 (redaction order) |
| SC-4 | Documentation is complete | TS-9 |
| SC-5 | Code review is completed | Review phase record in workflow.yaml |
| SC-6 | AC1 to AC9 are all satisfied | AC1→TS-1, AC2→TS-2, AC3→TS-3, AC4→TS-4, AC5→TS-5, AC6→TS-7, AC7→TS-5 and TS-6, AC8 and AC9→TS-8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-5 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5 |
| FR3 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-11 |
| FR4 | task0001 | TS-7 |
| FR5 | task0001 | TS-9 |
| NFR1 | task0001 | TS-5, TS-6 |
| NFR2 | task0001 | TS-10 |
| NFR3 | task0001 | TS-5, TS-11 |
| NFR4 | task0001 | TS-8, TS-12 |
| NFR5 | task0001 | TS-5, TS-12 |
| NFR6 | task0001 | TS-5, TS-6 |
| NFR7 | task0001 | TS-13 |

## E2E Testing

Not applicable: the project has no E2E command (`e2e_test_command` is empty).

## Manual Testing (E2E Not Possible)

- [ ] MT-1 (optional; the user runs it on a release build on Linux): check
  that a capability-query failure shows up in emterm.log
  (`~/.local/share/net.laser5.app.emterm/logs/emterm.log`).
  1. Start eMterm with a session bus address that cannot be reached, for
     example `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent`.
  2. In a shell, emit these three OSC 9 notifications in order:
     `printf '\033]9;probe;a<b 1\007'`, `printf '\033]9;probe;plain 2\007'`,
     `printf '\033]9;probe;a<b 3\007'`. The texts differ because identical
     pairs within 1 s are suppressed as duplicates.
  3. Expected: exactly one line containing
     `LOG_NOTIFY_CAPABILITY_QUERY_FAILED: `, followed by the error text.
     The notification failures still produce their existing
     `notify-rust failed:` lines. No marker line contains `probe`, `a<b` or
     `plain`.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 commands | 2 | 0 | 0 |
| Unit / regression | TS-1 to TS-6 | 6 | 0 | 0 |
| Integration | TS-8 | 1 | 0 | 0 |
| Review | TS-7, TS-9 to TS-13 | 0 | 0 | 6 (code review) |
| Format | 1 command | 1 | 0 | 0 |
| Manual runtime check | MT-1 (optional) | 0 | 0 | 1 |
