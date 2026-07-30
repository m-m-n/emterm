# Verification Document: mux-tab-switch-replay-latency

## Overview

**Feature**: mux-tab-switch-replay-latency
**SPEC.md**: `feature-docs/mux-tab-switch-replay-latency/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-tab-switch-replay-latency/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: every Acceptance Criterion across task0001-task0003 has
  a corresponding automated test (no percentage target — this is a
  bug-fix feature with a fixed enumerable AC set)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Switch to a pane whose scrollback matches the reproduced marker-cluster shape (2 MiB, ~31 segments, dense tail resize markers) | Replay completes in the tens-of-ms order, not the ~800-1000 ms non-bypass order | Unit/Bench |
| TS-2 | Ordinary switch (small `k`, dominating suffix) | Latency unregressed from the 1.57 ms-class baseline | Unit/Bench |
| TS-3 | Bypass equivalence for TS-1's shape | Viewport/cursor match a full non-bypass replay of the same payload; `scrollback_populated` semantics unchanged | Unit |
| TS-4 | Existing `snapshot_replay_bench_2mib_seq` bench | Stays green (unmodified assertion) | Unit/Bench |
| TS-5 | Startup/reattach `visible_row_count` 0→1 (and oscillating) transition | `Tab::resize`'s group-wide `Resize` broadcast does not fire once per transient transition; fires at most once for the final settled size; every pane ends up at the correct final size | Unit |
| TS-6 | Grid resize races an in-flight pane switch | Final replay for the new target is not defeated into a full non-bypass drain by target-dims mismatch (tens-of-ms class, not the ~21 ms raced-today class, when the shape would otherwise support bypass at the new target) | Unit |
| TS-7 | Two `Snapshot`/`SnapshotRestore` frames for the same pane arrive in immediate succession | Only one decode+replay actually completes; the discarded one's work does not run | Unit |
| TS-8 | NFR1 regression guard: a genuinely large, content-heavy prefix paired with a small qualifying suffix | Split still does not engage for it, or the 2nd-pass worker is not invoked twice for that prefix (no double non-bypass cost) | Unit/Bench |

## Code Quality Verification

- Format: (no `format_command` configured for this project — skip)
- Static analysis: build verification above (`cargo check`) is the
  project's only configured static gate

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR8 implemented and tested | TS-1 through TS-8 all pass |
| SC-2 | All test scenarios pass | Test Verification command above, exit 0 |
| SC-3 | Performance goals met (tens-of-ms for the reproduced shape; no regression for ordinary switch) | TS-1, TS-2 |
| SC-4 | `snapshot_replay_bench_2mib_seq` stays green | TS-4 |
| SC-5 | Code review completed | review phase (workflow.yaml `review` step) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-1 |
| FR3 | task0001 | TS-2 |
| FR4 | task0001 | TS-3 |
| FR5 | task0001 | TS-4 |
| FR6 | task0002 | TS-5 |
| FR7 | task0003 | TS-6 |
| FR8 | task0003 | TS-7 |
| NFR1 | task0001 | TS-8 |

## E2E Testing

No E2E infrastructure exists in this repository (Phase 0.5 scan found
none). Not applicable.

## Manual Testing (E2E Not Possible)

- [ ] MT-1: On a real machine, restart the eMterm client, reattach to a
  mux session with at least one pane carrying substantial scrollback, and
  switch to that pane — confirm the display appears without a
  multi-second delay (subjective confirmation of the fix's real-world
  effect; the automated TS-1/TS-5/TS-6/TS-7 tests verify the underlying
  mechanisms in isolation, but the full end-to-end startup → attach →
  switch sequence on a live daemon is not automatable without live GUI +
  daemon orchestration).

## Performance / Security Verification

- FR1/TS-1: replay latency for the reproduced shape — tens-of-ms order
  (see task0001 AC-1's ceiling).
- FR3/TS-2: ordinary switch latency — no regression from the 1.57 ms
  baseline.
- NFR1/TS-8: no reintroduced double non-bypass replay cost for a
  genuinely large prefix.
- Security: not applicable (internal performance fix, no new external
  input surface).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | Yes | - | - |
| Test scenarios | 8 (TS-1–TS-8) | Yes | - | - |
| Success criteria | 5 (SC-1–SC-5) | Yes (SC-1–SC-4) | - | SC-5 via review phase |
| Manual | 1 (MT-1) | - | - | Yes |
