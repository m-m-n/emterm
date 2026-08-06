# Verification Document: mux-connection-input-freeze

## Overview
**Feature**: mux-connection-input-freeze /
**SPEC.md**: `feature-docs/mux-connection-input-freeze/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-connection-input-freeze/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both (the second gates the
  CLI-only feature split; task0002's shared `forward_loop` change must
  stay platform/feature-neutral in compilation).

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  - If `tabs.rs` replay tests flake in parallel, re-run as:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_throughput`
- Coverage target: every Acceptance Criterion in task0001 and task0002 has
  at least one passing test; no new failures relative to the pre-feature
  baseline (known baseline flakes/failures documented at verify time are
  not new failures).
- Bounded-delay quantification (AC-3 of SPEC.md): tests assert within a
  named 5-second timeout constant (IMPLEMENTATION.md Convention 5).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Connection task's output side saturated (small/full socket, client not reading); an inbound client message arrives | Message is processed within the named 5 s bounded timeout — `framed.next()` polling never stops while drain output is pending (FR1, FR5; SPEC AC-3) | Unit/Integration (`--lib`, connection-level harness) |
| TS2 | Same-pane snapshot → PTY-output traffic through the reworked drain path, including the deferred-output retry / held-remainder path | Client observes same-pane frames in channel FIFO order (FR3) | Unit/Integration (`--lib`) |
| TS3 | Bridge daemon→stdout progress stalls (blocked injected sink) | Socket-drain direction keeps reading until the bounded channel fills; stdin→daemon keeps forwarding; order preserved on release (FR2) | Unit/Integration (`--lib`, Unix-gated where PTY/pipe APIs are needed) |
| TS4 | Full `--lib` suite + `mux_throughput` integration test | No new failures; no throughput regression (FR3, FR4, NFR4 — includes existing reap/ack/hot-upgrade-adjacent coverage) | Regression |
| TS5 | Manual repro: `seq 1 10000000` in one mux window; switch to another window; type continuously | Rendering of the seq window does not freeze the connection; destination window accepts continuous input without waiting for seq to finish (FR1, FR2; SPEC AC-1, AC-2) | Manual |
| TS6 | Diff inspection: protocol stability | No changes under `crates/mux_ipc/`, `src-tauri/src/mux/ipc/codec.rs`, or any wire message shape (NFR1) | Inspection |
| TS7 | Diff inspection: scope boundaries | Diff touches neither GUI-side `event_tx` sizing (NFR2) nor `bridge_main_loop_windows` (NFR3) | Inspection |
| TS8 | Rework (task0003): outbound path saturated with a held remainder (client not reading); sustained notification stream arrives (single-forward and a forced Lagged resync) | Held-remainder frame count never exceeds its named bound (no per-notification growth); once the client resumes reading, the drain arm resumes and new PTY output reaches the client within the named 5 s timeout (FR4, FR1) | Unit/Integration (`--lib`, connection-level harness) |
| TS9 | Rework (task0003): reply / reattach frames (`route_message` reply, PaneCreated, same-pane SnapshotRestore) admitted while a PtyOutput remainder is held | Client observes every held remainder frame before the later reply/reattach frames — no overtaking; same-pane SnapshotRestore never precedes older held PtyOutput (FR3) | Unit/Integration (`--lib`, connection-level harness) |
| TS10 | Rework (task0003): client evicted (kick) while a remainder is held | A client that resumes reading receives the held frames followed by Detached, in FIFO order, within the named teardown budget; a never-reading client still completes teardown within the bounded budget (FR3) | Unit/Integration (`--lib`, connection-level harness) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - Note: the crate has known pre-existing rustfmt drift on the baseline;
    only drift introduced by this feature's changes counts as a failure.
    Never run crate-wide write-mode formatting.
- Static analysis: covered by the build commands (`cargo check`, both
  feature variants).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR5 implemented and tested | task0001 AC-1..AC-6, task0002 AC-1..AC-6, and task0003 AC-1..AC-7 pass |
| SC-2 | All test scenarios pass | TS1-TS4 and TS8-TS10 automated pass; TS5 manual pass; TS6-TS7 inspection pass |
| SC-3 | SPEC AC-1/AC-2 (live repro) | TS5 (manual — no E2E infrastructure exists) |
| SC-4 | SPEC AC-3 (bounded input polling pinned by test) | TS1 |
| SC-5 | NFR1-NFR4 satisfied | TS6, TS7, TS4 (reap/ack regressions), task-level ACs (task0001 AC-4/AC-5, task0002 AC-6) |
| SC-6 | Code review completed | review phase `residual_critical_high == 0` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0003 | TS1 (automated), TS5 (manual), TS8 (rework: drain-arm resumption) |
| FR2 | task0002 | TS3 (automated), TS5 (manual) |
| FR3 | task0001, task0002, task0003 | TS2, TS4, TS9, TS10 |
| FR4 | task0001, task0002, task0003 | TS4, TS8 (+ boundedness ACs: task0001 AC-3, task0002 AC-4, task0003 AC-1) |
| FR5 | task0001 | TS1 |
| NFR1 | task0001, task0002, task0003 | TS6 |
| NFR2 | (scope boundary — no implementing task by design) | TS7 |
| NFR3 | task0002 (boundary enforced by its scope/AC-6) | TS7 |
| NFR4 | task0001, task0003 | TS4 (+ task0001 AC-4/AC-5; task0003 leaves reap/ack contracts untouched) |

## E2E Testing

No E2E framework exists in this repository (no automated E2E run command).
SPEC AC-1/AC-2 are covered manually (TS5 below).

## Manual Testing (E2E Not Possible)

- [ ] TS5: With a release build the user launches themselves (release
      builds are user-initiated; not part of automated verification):
      1. Attach a mux session with at least two windows on one bridge
         connection.
      2. In window A run `seq 1 10000000`.
      3. While it is producing output, switch to window B and type
         continuously.
      4. Confirm: input in window B is accepted continuously without
         waiting for seq to finish (SPEC AC-1); window A does not drag
         window B's input down (SPEC AC-2); switching back to window A
         shows it progressing/finished rather than frozen.

## Performance / Security Verification

- Throughput regression guard: `mux_throughput` integration test (TS4)
  must pass with no regression relative to the pre-feature baseline.
- Memory boundedness (FR4): every queue introduced by either task is
  bounded by a named constant (task0001 AC-3, task0002 AC-4); reviewed
  against IMPLEMENTATION.md Convention 2.
- Security: not applicable (no auth/input-validation/protocol surface
  change; NFR1 pins the protocol).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional | FR1, FR2, FR5 | TS1, TS3, TS8 | - | TS5 |
| Ordering / backpressure | FR3, FR4 | TS2, TS4, TS8, TS9, TS10 | - | - |
| Non-functional | NFR1, NFR2, NFR3, NFR4 | TS4 | - | TS6, TS7 (inspection) |
| Build / quality | build, format | check ×2, fmt --check | - | - |
