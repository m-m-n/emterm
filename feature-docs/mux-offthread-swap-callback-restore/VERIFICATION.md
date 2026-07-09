# Verification Document: mux-offthread-swap-callback-restore

## Overview

**Feature**: mux-offthread-swap-callback-restore /
**SPEC.md**: `feature-docs/mux-offthread-swap-callback-restore/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-offthread-swap-callback-restore/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Also: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (NFR2)
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: all new TS scenarios below covered by unit tests; no
  numeric coverage threshold for this repo.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | After `apply_offthread_swap`, live core callbacks is the pre-swap instance | Recording double still receives events post-swap | Unit |
| TS-2 | Post-swap OSC 9999 (`MUX_OSC_PARAM`) sequence | Registered app-param action fires, matching a never-swapped core | Unit |
| TS-3 | Post-swap pre-mux Welcome, OSC 9999 form, via outer-stream path | Reaches `apply_mux_message` (attach bootstrap) | Unit |
| TS-4 | Post-swap pre-mux Welcome, APC form | Also reaches `apply_mux_message` | Unit |
| TS-5 | Post-swap callback-driven OSC (e.g. title change) | Transplanted callbacks invoked | Unit |
| TS-6 | Synchronous replay (< 64 KB) unchanged | Callbacks kept with no new code path; existing sync tests pass | Unit (regression) |
| TS-7 | Existing off-thread replay tests (mark backfill, 2nd-pass restore, supersede) + 2nd-pass restore preserves wiring | All pass; post-restore callbacks/OSC intact | Unit (regression) |

## Code Quality Verification

- Format: none (project has no enforced format command in workflow.yaml)
- Static analysis: none beyond `cargo check`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FRs implemented and tested (TS-1…TS-7) | Test run above |
| SC-2 | Full `--lib` test suite passes | Test command above |
| SC-3 | CLI-only build compiles | `--no-default-features` check above |
| SC-4 | MT-1 / MT-2 handed to user for later verification | Noted in verify report |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-5 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3, TS-4 |
| FR4 | task0001 | TS-5 |
| NFR1 | task0001 | TS-6, TS-7 |
| NFR2 | task0001 | SC-3 |

## E2E Testing

No E2E framework in this repo — omitted.

## Manual Testing (E2E Not Possible)

User-performed after merge (release build + real mux session required):

- [ ] MT-1: Window with ≥ 64 KB snapshot: switch to it → detach → attach
  succeeds without hang, no GUI restart needed.
- [ ] MT-2: After displaying such a window, `emterm markdown <file>` inside
  mux launches the viewer (viewer-launch-loss resolution).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 7 | 7 | 0 | 0 |
| Success criteria | 4 | 3 | 0 | 1 |
| Manual | 2 | 0 | 0 | 2 |
