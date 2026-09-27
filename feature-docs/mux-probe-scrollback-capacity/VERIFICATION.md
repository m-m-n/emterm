# Verification Document: mux-probe-scrollback-capacity

## Overview

**Feature**: mux-probe-scrollback-capacity / **SPEC.md**: `feature-docs/mux-probe-scrollback-capacity/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-probe-scrollback-capacity/IMPLEMENTATION.md`

This document covers the integrated verification run by the verify phase. Each
task plan carries its own task-level acceptance criteria.

## Build Verification

- Command (GUI crate, default features): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (mux_ipc crate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/mux_ipc/Cargo.toml`
- Expected: every command exits 0 with no errors.

## Test Verification

- Command (GUI crate, default features): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command (CLI-only build): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --test-threads=1`
- Command (mux_ipc crate): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/mux_ipc/Cargo.toml --lib`
- Coverage target: not measured, because workflow.yaml configures no coverage command. Coverage is judged by the scenarios below: every one must be present and passing.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Wrapped ring plus one grow shape (rows-only grow segment, rows+cols grow segment, or current_dims larger than the last segment) and client capacity C in {0, a small non-zero value}. Build at probe capacity C, replay on a client core of capacity C through both reset-and-replay-segments and build-from-snapshot, then feed a CR LF and a marker | Cursor row/col and the marker's row equal those of an oracle of capacity C that replays the delegated pre-dump payload plus the same continuation; at least one case fails when the probe is fixed at 10,000 (red recorded) | Unit |
| TS-2 | Probe-capacity resolution for unreported, 0, 50, 10,000, 10,001 and u32::MAX | Resolves to 10,000, 0, 50, 10,000, 10,000 and 10,000; above the cap the dump block is still appended; the probe never builds a scratch terminal deeper than 10,000 lines | Unit |
| TS-3 | Byte identity at probe capacity 10,000 for the existing wrapped fixtures, for both wrap-aware builders | Payload and segments equal golden output captured before the change; existing dump-block probe-equality, wrap-restore and apt-progress-bar-related snapshot tests pass, changed only by the capacity argument | Unit |
| TS-4 | Protocol: the new type through `from_u8`, `from_frame_body`, `from_apc` and the EMUX plaintext path with a 4-byte payload; capacity-payload decode for 4-byte and non-4-byte inputs; the first unassigned byte through the daemon codec | The new type and its payload round-trip; the payload decodes only from exactly 4 bytes; every existing discriminant and PROTOCOL_VERSION 3 are unchanged; an unknown-type frame is dropped without closing the stream | Unit |
| TS-5 | GUI tab: first accepted Welcome, with panes and without panes, for a tab core of capacity 0 and one of a small non-default value | The capacity message is the first control frame, its value equals the tab core's scrollback capacity, and Attach, then Resize and RequestPaneSnapshot (or CreateWindow) follow; a duplicate Welcome sends no second capacity message | Unit |
| TS-6 | Bridge: forwarding and storing the capacity message; upgrade reconnect | The capacity message is forwarded unchanged and the latest one is stored, the way Attach is captured; after a reconnect the stored capacity frame is written before the stored Attach | Unit |
| TS-7 | Daemon: the per-connection value reaches the visible reattach, the on-demand RequestPaneSnapshot, the visibility resume (immediate, and deferred through both flush paths) and the evaluate-output-target resume branch; a malformed payload; two connections with different reports | Every site builds with the requesting connection's resolved capacity; a malformed payload leaves the value unchanged, gets no reply and keeps the connection open; the two connections' snapshots are built at their own values | Unit / Integration |
| TS-8 | Manual repro on a release build with scrollback_lines 0 and with a small value: fill a pane past a ring wrap, grow the window mid-output, switch tab or reattach, then continue output | Continued output lands on the expected row | Manual |
| TS-9 | Inspection of the cap constant's documentation in the dump-block module and of SPEC.md's Known Limits section | Both state the above-10,000 residual mismatch and the mixed-version legacy fallback, and state that the cap does not change lock-hold time | Manual (inspection) |
| TS-10 | Build and test matrix: GUI-crate lib tests, CLI-only check and CLI-only lib tests, mux_ipc crate tests | Every command in Build Verification and Test Verification exits 0 | Automated (build) |
| TS-11 | Inspection of every diagnostic added by the feature | Every one that must reach release logs is at warn or higher, and none logs payload bytes | Manual (inspection) |
| TS-12 | Inspection of the integrated diff against the implement base commit | No formatting-only edits to files outside the tasks' declared files; no crate-wide reformat | Manual (inspection) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`. Check mode only; it modifies nothing. A difference reported in a file that no task of this feature touched is pre-existing drift and not a failure of this feature (NFR5).
- Static analysis: workflow.yaml configures no static-analysis command.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | AC-1: with a small or 0 capacity and a grow-resize inside the payload, a wrapped-ring pane restores through tab switch, reattach or visibility resume, and continued output lands on the same row as a replay at the client's own capacity | TS-1, TS-7 (automated), TS-8 (manual) |
| SC-2 | AC-2: the new MessageType round-trips through the frame, APC/OSC and EMUX plaintext encodings; discriminants and PROTOCOL_VERSION 3 are unchanged | TS-4 |
| SC-3 | AC-3: on the first accepted Welcome the capacity message precedes Attach, CreateWindow and RequestPaneSnapshot, and equals the core's scrollback capacity | TS-5 |
| SC-4 | AC-4: the bridge forwards the message and, on the Unix upgrade reconnect, sends the stored capacity before the re-sent Attach | TS-6 |
| SC-5 | AC-5: the resolution table, the malformed-payload handling, and independent per-connection values | TS-2, TS-7 |
| SC-6 | AC-6: all four assembly sites and the deferred visibility resume use the requesting connection's capacity | TS-7 |
| SC-7 | AC-7: byte identity at 10,000; existing wrap-restore, dump-block and apt-related snapshot tests pass | TS-3 |
| SC-8 | AC-8: an old daemon drops the new message and the connection keeps working | TS-4 (unknown-type codec drop) |
| SC-9 | AC-9: known limits documented next to the cap and in SPEC | TS-9 |
| SC-10 | AC-10: default-feature lib tests, mux_ipc tests and the CLI-only check pass | TS-10 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS-4, TS-7 |
| FR2 | task0001 | TS-5, TS-8 |
| FR3 | task0001 | TS-6 |
| FR4 | task0002 | TS-2, TS-7 |
| FR5 | task0002 | TS-1, TS-2, TS-7, TS-8 |
| FR6 | task0002 | TS-2 |
| FR7 | task0002 | TS-3 |
| FR8 | task0002 | TS-9 |
| FR9 | task0002 | TS-1 |
| NFR1 | task0001, task0002 | TS-2, TS-4 |
| NFR2 | task0001, task0002 | TS-2, TS-4, TS-7 |
| NFR3 | task0001, task0002 | TS-10 |
| NFR4 | task0001, task0002 | TS-11 |
| NFR5 | task0001, task0002 | TS-12 |

## E2E Testing

No E2E framework is configured: workflow.yaml's e2e_test_command is empty for every
component. The end-to-end repro is the manual scenario TS-8.

## Manual Testing (E2E Not Possible)

- [ ] TS-8: On a release build, set scrollback_lines to 0 and then to a small value. For each setting, fill a mux pane past a ring wrap, grow the window mid-output, switch tab or reattach, then continue output. The output must land on the expected row.
- [ ] TS-9: Read the cap constant's documentation in the dump-block module and SPEC.md's Known Limits section. Both must state the above-10,000 limit, the mixed-version limit, and the lock-hold note.
- [ ] TS-11: Review every diagnostic the feature added. Each must be at warn or higher where it needs to reach release logs, and none may log payload bytes.
- [ ] TS-12: Review the integrated diff. It must contain no formatting-only edits outside the tasks' declared files.

## Performance / Security Verification

- NFR2: the probe's scratch-terminal depth is at most 10,000 lines for every reported value, and the payload is validated before any capacity-driven allocation. Checked by TS-2 and TS-4.
- TM-1: probe capacity is resolved as min(reported, 10,000), with unreported mapped to 10,000, and the probe enforces the cap at its own allocation point; above the cap the dump block is still appended. Checked by the TS-2 automated tests (resolution table, the probe never exceeding the cap, the above-cap snapshot equal to the 10,000 output).
- TM-2: the capacity payload decodes only from exactly 4 bytes, and a malformed payload is ignored with the stored value unchanged, no reply and the connection left open. Checked by TS-4 (decoder exactness) and TS-7 (router behaviour on a malformed payload).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (3 commands) | 3 | 3 | 0 | 0 |
| Test scenarios TS-1 to TS-7 | 7 | 7 | 0 | 0 |
| Build and test matrix TS-10 | 1 | 1 | 0 | 0 |
| Manual and inspection (TS-8, TS-9, TS-11, TS-12) | 4 | 0 | 0 | 4 |
| Code quality (format check) | 1 | 1 | 0 | 0 |
| Security mitigations (TM-1, TM-2) | 2 | 2 | 0 | 0 |
| **Total** | 18 | 14 | 0 | 4 |
