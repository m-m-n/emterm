# Verification Document: osc52-clipboard-write-drain

## Overview
**Feature**: osc52-clipboard-write-drain / **SPEC.md**: `feature-docs/osc52-clipboard-write-drain/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/osc52-clipboard-write-drain/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/osc52-clipboard-write-drain/THREAT-MODEL.md`

Run every command from the project root.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, TS-7): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured for this project. Every acceptance criterion in `tasks/task0001.md` and `tasks/task0002.md` maps to at least one test, recorded in `test-docs/osc52-clipboard-write-drain/`.
- The `tabs` replay tests are order-sensitive under parallel runs. A replay test that fails only in the full parallel run is rerun alone with `--test-threads=1` before it counts as a failure. Do not reproduce timing by loading the CPU.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Table-driven test of the target-to-destination mapping over `c`, `p`, `cp`, `pc`, `''`, `s`, `0`, `7`, `q`, `s0` (plus `x`, A3), with PRIMARY available and with PRIMARY unavailable | Each target maps to the destinations of SPEC AC-2. With PRIMARY unavailable every target maps to CLIPBOARD only, and a both-target to a single CLIPBOARD | Unit |
| TS-2 | Drain and apply against a recording sink (per-destination write / clear) and a failing / clipboard-less sink | Entries apply in arrival order and each destination keeps the last write addressed to it, with at most one operation per destination per pump. Clears reach the clear operation and are not dropped as empty writes. Both tab queues are empty after the pump. Reads are discarded with no PTY write. A failing or clipboard-less sink causes no panic, and its failure log line holds neither the text nor its base64. An empty-selection copy still writes nothing (existing behaviour). | Unit |
| TS-3 | A mux-attached tab gets a coalesced buffer with an active-pane `PtyOutput` frame carrying an OSC 52 write, and a post-`Detached` tail carrying another, in one pump | Both writes are in that pump's batch. The test fails when the drain is removed or moved ahead of the mux frame-apply loop | Integration (tab / App unit) |
| TS-4 | Pre-mux outer parse: an OSC 52 write in plain shell output. Added cases: a background tab, and live output queued during an off-thread switch and applied on the swap | Every write is in the batch of the pump that parsed it | Integration (tab / App unit) |
| TS-5 | Snapshot / SnapshotRestore whose content holds an OSC 52 write or clear, on the synchronous path, the off-thread path, the live-queue overflow fallback and the worker-disconnect fallback | No queue entry comes from snapshot content. Entries queued before the replay, and entries from live output applied after it, are kept | Integration (tab unit) |
| TS-6 | Existing callbacks OSC 52 tests run without edits (`osc_52_write_denied_when_over_quota`, `osc_52_query_denied_when_read_disabled`, `osc_52_write_default_allows_within_quota`, `osc_52_clear_pushes_empty_write`, `parse_osc52_*`) | All pass | Unit |
| TS-7 | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | Succeeds | Build |

## Code Quality Verification
- Format: `workflow.yaml` configures no format command. Formatting is applied per file by the project's editor hook. Do not run a crate-wide format.
- Static analysis: none configured.
- Code review checks (manual):
  - [ ] NFR2: the pump-to-sink step is called only from `about_to_wait` in `src-tauri/src/window_host/event_loop.rs`, right after `pump_all`. No other code calls the OSC 52 sink or arboard for OSC 52.
  - [ ] NFR3: every log line the diff adds or changes carries only a target / destination, a length, a count or an error description. None interpolates the decoded text or the base64 payload.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage table below; test command passes |
| SC-2 | All test scenarios (TS-1 to TS-7) pass | Test and build commands above |
| SC-3 | NFR1 queue bound holds | TS-2, TS-3, TS-4 (queues empty after every pump) |
| SC-4 | NFR3 log requirement holds | TS-2 (formatter assertion) and the NFR3 code review check |
| SC-5 | Documents are complete | IMPLEMENTATION.md, VERIFICATION.md, THREAT-MODEL.md, task plans and test records exist |
| SC-6 | Code review is complete | Review phase result |
| SC-7 | MT-1 pastes `hello` | Manual Testing MT-1 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, MT-1 |
| FR2 | task0001 | TS-2, MT-2 |
| FR3 | task0001 | TS-1, TS-2, MT-1, MT-2 |
| FR4 | task0001 | TS-2 |
| FR5 | task0001 | TS-3, TS-4 |
| FR6 | task0001 | TS-3, TS-4 |
| FR7 | task0002 | TS-5 |
| FR8 | task0001 | TS-2 |
| FR9 | task0001 | TS-6 |
| NFR1 | task0001 | TS-2 |
| NFR2 | task0001 | Code review check (no automated test) |
| NFR3 | task0001 | TS-2, code review check |
| NFR4 | task0001 | TS-7 |
| NFR5 | task0001 | TS-2, the existing `window_host` tests in the test command |
| NFR6 | task0001 | TS-1, MT-3 |

## Manual Testing (E2E Not Possible)
- [ ] MT-1 (user, release build): over SSH (or where wl-copy / xclip is absent), run the SPEC reproduction `printf` for `hello`, then paste from CLIPBOARD. The result is `hello`. Optional: drag-select in a remote Claude Code full-screen view and paste locally (SPEC AC-1).
- [ ] MT-2 (user, release build, Linux): emit an OSC 52 write with target `p` and paste with the middle button; the text appears. Then emit an OSC 52 clear with target `c` (empty payload) and paste from CLIPBOARD; no text is pasted (FR2, FR3).
- [ ] MT-3 (user, optional): the Windows cross-build from `.claude/rules/core-build-location.md` succeeds (NFR6). The user runs it; it is a release build.

## Performance / Security Verification
- NFR1: after every pump, `pending_clipboard_writes` and `pending_clipboard_reads` are empty on every tab — checked by TS-2, TS-3 and TS-4.
- TM-1: existing OSC 52 acceptance gates stay in front of the queue — checked by TS-6 and by task0001 AC-7 (over-quota, invalid base64 and non-UTF-8 writes produce no batch entry).
- TM-2: OSC 52 reads are discarded with no clipboard read and no PTY response — checked by TS-2 (task0001 AC-4: read queue empty, no PTY bytes written for the query).
- TM-3: failure log lines hold no clipboard text or base64 — checked by TS-2 (task0001 AC-3 formatter assertion) and the NFR3 code review check.
- TM-4: queues drained every pump and at most one clipboard operation per destination per pump — checked by TS-2 (task0001 AC-2, AC-4).
- TM-5: snapshot replay content adds no queue entry — checked by TS-5 (task0002 AC-1, AC-3, AC-4).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (cargo check, TS-7) | 2 | 0 | 0 |
| Unit tests | 3 (TS-1, TS-2, TS-6) | 3 | 0 | 0 |
| Integration tests | 3 (TS-3, TS-4, TS-5) | 3 | 0 | 0 |
| Performance / Security | 6 (NFR1, TM-1, TM-2, TM-3, TM-4, TM-5) | 6 | 0 | 0 |
| Code review checks | 2 (NFR2, NFR3) | 0 | 0 | 2 |
| Manual testing | 3 (MT-1, MT-2, MT-3) | 0 | 0 | 3 |
| **Total** | **19** | **14** | **0** | **5** |
