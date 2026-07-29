# Verification Document: mux daemon hot-upgrade via execve

## Overview

**Feature**: mux-daemon-hot-upgrade
**SPEC.md**: `feature-docs/mux-daemon-hot-upgrade/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-daemon-hot-upgrade/IMPLEMENTATION.md`

## Build Verification

- Command (main crate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (mux_ipc): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/mux_ipc/Cargo.toml`
- Command (CLI-only feature gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, no new warnings introduced by this feature

## Test Verification

- Command (main crate): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (mux_ipc): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/mux_ipc/Cargo.toml --lib`
- Command (integration): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`
- Coverage target: no numeric coverage gate is enforced in this repository.
  The gate is behavioural: every acceptance criterion in every task plan has
  at least one test, and the scenarios below all pass.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Upgrade request and upgrade announcement messages round-trip through the frame helpers | Type, pane id and empty payload preserved both ways | Unit |
| TS-2 | A frame with an unrecognised type byte reaches the decoder | Frame is discarded, the connection is not torn down | Unit |
| TS-3 | Handoff document encode → decode | Session tree, ID counters, incarnation token, descriptor numbers, child process ids and scrollback preserved byte-for-byte | Unit |
| TS-4 | Handoff document with an unsupported schema version is decoded | Fails with a version-specific error, nothing partially applied | Unit |
| TS-5 | Protocol version constant | Unchanged from its pre-feature value | Unit |
| TS-6 | Inherited master adapter over a real PTY pair | Read, write, resize and ownership behave as a freshly opened master; construction over a bad descriptor fails | Unit |
| TS-7 | Snapshot of a manager with live panes | Close-on-exec cleared on the listen descriptor and every live pane master | Unit |
| TS-8 | Snapshot → restore round-trip | ID counters, incarnation token, tree ordering, active selections and per-pane scrollback all equal the source; next allocated id continues the sequence | Unit |
| TS-9 | Handoff file lifecycle | Created owner-only; removed after successful read, after failed read, and after aborted snapshot | Unit |
| TS-10 | Upgrade request received by the daemon | Upgrade branch taken; pane-killing shutdown helper not invoked; no pane marked exited; socket file not removed; announcement broadcast to connected clients | Unit |
| TS-11 | Upgrade aborts (incompatible probe answer, snapshot failure) | Reason reported to the requesting client, logged at warn or above, no handoff file left, daemon keeps serving | Unit |
| TS-12 | Daemon started with the handoff environment variable set | Socket bind skipped, recorded listener adopted, session tree restored, handoff file removed, handoff-start log line with pane and descriptor counts | Unit |
| TS-13 | `emterm mux upgrade` against stand-in daemons | Sends the request and reports success when the daemon returns; reports a bounded timeout otherwise; reports clearly when no daemon runs | Unit |
| TS-14 | Legacy recovery against a stand-in that ignores the upgrade request | Upgrade attempted first, shutdown-then-respawn used only after the bounded wait; no fallback when the daemon returns | Unit |
| TS-15 | Bridge disconnect with and without a preceding announcement | With announcement: bounded, backed-off reconnect and re-attach to the same session. Without: exits as today. Window exhausted: exits with a logged reason | Unit |
| TS-16 | Process-id reaping vs handle reaping | Same grace-then-terminate behaviour and same observable pane end state; already-collected id returns promptly | Unit |
| TS-17 | Real daemon, real shell, upgrade triggered | Shell process id unchanged across the upgrade and a file it created beforehand is still observable from that same shell | Integration |
| TS-18 | Daemon log after a real upgrade | Contains the handoff-start entry including the adopted pane count | Integration |
| TS-19 | Upgrade of a daemon with zero panes | Succeeds; daemon still answers a handshake | Integration |
| TS-20 | Upgrade rejected by the schema probe | Aborted; the original daemon still answers a handshake with its pane still live | Integration |
| TS-21 | Feature-gate builds | Default-feature check and `--no-default-features` check both succeed | Build |
| TS-22 | Pane child spawned after a handoff start | Does not see the handoff environment variable | Unit |

## Code Quality Verification

- Format: enforced per-file by the project's editing hook; no crate-wide
  formatting command is run by this workflow (`format_command` is empty in
  workflow.yaml by design — a crate-wide reformat would touch unrelated files).
- Static analysis: the compiler warnings produced by the build commands above.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | Requirements coverage table below; every FR maps to ≥ 1 task and ≥ 1 scenario |
| SC-2 | All test scenarios pass | TS-1 … TS-22 |
| SC-3 | Security requirements satisfied | TS-9, TS-22 |
| SC-4 | Library tests and the CLI-only check both pass | Test and Build Verification commands |
| SC-5 | No pane's shell is killed on any upgrade path | TS-10, TS-11, TS-17, TS-20 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001, task0004 | TS-1, TS-10 |
| FR3 | task0004 | TS-10, TS-19 |
| FR4 | task0001, task0003 | TS-3, TS-8 |
| FR5 | task0003 | TS-7 |
| FR6 | task0003 | TS-9 |
| FR7 | task0004, task0005, task0008 | TS-13, TS-17 |
| FR8 | task0004 | TS-12 |
| FR9 | task0003, task0008 | TS-8, TS-17 |
| FR10 | task0002, task0003, task0008 | TS-6, TS-17 |
| FR11 | task0004, task0008 | TS-12, TS-18 |
| FR12 | task0006 | TS-15 |
| FR13 | task0004, task0005 | TS-11, TS-20 |
| FR14 | task0001, task0004, task0005 | TS-4, TS-20 |
| FR15 | task0005 | TS-13 |
| FR16 | task0005 | TS-14 |
| FR17 | task0004 | TS-10 |
| FR18 | task0003, task0007 | TS-16 |
| FR19 | task0003 | TS-9 |
| NFR1 | task0003, task0004 | TS-9, TS-22 |
| NFR2 | task0003, task0004, task0007, task0008 | TS-10, TS-17 |
| NFR3 | task0006 | TS-15 |
| NFR4 | task0002, task0005 | TS-6, TS-21 |
| NFR5 | task0004 | TS-12, TS-18 |
| NFR6 | task0001 | TS-2, TS-5 |

## E2E Testing

The project has no browser/E2E framework (`test/README.md`: none present).
The end-to-end behaviour of this feature is covered by the process-level
integration test registered as `e2e_test_command` in workflow.yaml:

- [ ] TS-17: shell survives the upgrade with its process id and files intact
- [ ] TS-18: handoff start is logged with the adopted pane count
- [ ] TS-19: zero-pane upgrade succeeds
- [ ] TS-20: schema-rejected upgrade leaves the original daemon serving

## Manual Testing (E2E Not Possible)

- [ ] Install a newly built binary over a running daemon that has a pane with
      a live ssh session, run `emterm mux upgrade`, and confirm the session
      continues and the attached client repaints without relaunching.
- [ ] Confirm the daemon log distinguishes the handoff start from a normal
      start when read by a human.

## Performance / Security Verification

- NFR3: the upgrade of a daemon with several panes at default scrollback
  capacity completes within a few seconds end to end — observed while running
  TS-17.
- NFR1: handoff file permission and removal — TS-9; environment variable not
  leaked to pane children — TS-22.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional requirements | 19 | 19 | 4 | 0 |
| Non-functional requirements | 6 | 6 | 2 | 2 |
| Test scenarios | 22 | 22 | 4 | 0 |
