# Verification Document: mux-write-filter-overflow-open-string-cut

## Overview

**Feature**: mux-write-filter-overflow-open-string-cut / **SPEC.md**: `feature-docs/mux-write-filter-overflow-open-string-cut/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-write-filter-overflow-open-string-cut/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-write-filter-overflow-open-string-cut/THREAT-MODEL.md`

All commands run from the project root (integration worktree root), without `cd`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only build (NFR4): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured; every task0001 Acceptance Criterion maps to at least one scenario below.

### Test Scenarios from SPEC.md

TS-1 to TS-8 come from SPEC.md; TS-9 to TS-11 are added by the plan (task0001 AC-1, AC-8, AC-9).

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Production reader (run_reader_without_owner): reads grow an `ESC ]0;` OSC past the cap, the crossing read ends in `ESC`, the next read is a 47 / 1047 / 1049 `h` ... `l` pair followed by main-buffer plain text (SPEC AC-1; task0001 AC-7) | The ring holds the string-body closure once; term_core replaying the ring displays the text; view (rows, cursor, responses) equals the raw-stream reference | Integration |
| TS-2 | Held-`ESC` open-body ending (OSC held at the cap, call 1 = body bytes + `ESC`) closed by the reader's fallback closing and by a cut at fed 0, per 47 / 1047 / 1049 pair (SPEC AC-2, AC-3; task0001 AC-2) | Exactly one `ESC` held with the open OSC body state; the closing writes exactly `ESC` + CAN; `pending` empty, state Ground, no designator awaited; a second fallback closing writes nothing; the replay equals view_after_a_cut | Unit |
| TS-3 | Overflow run ending in plain OSC body bytes, then a cut at fed 0 of the next call, the fallback closing, and a cut in the same call (SPEC AC-4, AC-5; task0001 AC-3) | Each closing is `ESC` + CAN; the same-call case writes the strip of the run followed by `ESC` + CAN, holds nothing, leaves Ground; replays equal view_after_a_cut | Unit |
| TS-4 | DCS / APC bodies (non-strip-target DCS and APC, Kitty APC, SIXEL DCS) left open past the cap, including a BEL written inside the body (SPEC AC-6; task0001 AC-4) | Open ST-terminated body state; a cut writes `ESC` + CAN; BEL keeps the body; replays equal view_after_a_cut | Unit |
| TS-5 | State carry across calls after an overflow leaves an open OSC body (SPEC AC-7; task0001 AC-5) | Plain bytes keep the body (cut writes `ESC` + CAN); BEL and `ESC \` return to Ground (cut writes nothing); `ESC [` leaves the CSI state (cut writes DEL) | Unit |
| TS-6 | Updated predecessor expectations: overflow_lone_esc.rs `endings()` entry 'inside an open string body'; round4_designator_cut.rs open-OSC case moved (SPEC AC-8; task0001 AC-6) | The entry expects the open OSC body state and `ESC` + CAN; every overflow_lone_esc test passes, including the continuation-removal and `ESC \` tests for that ending; the round4 test keeps its name and its two ground-ending cases; no test renamed | Unit |
| TS-7 | Full `--lib` suite and the `--no-default-features` cargo check (SPEC AC-9; task0001 AC-9) | Both exit 0; no existing expectation changed beyond TS-6 (any other change is reported per task0001's rule for an unforeseen failing test) | Suite / Build |
| TS-8 | Read-through of the doc comments FR7 lists, the written-state field doc and the cut-branch comments (SPEC AC-10; task0001 AC-10) | They state the open-body states and the `ESC` + CAN closure; none states that a string body counts as ground or that a cut writes at most one of two closures | Manual (review) |
| TS-9 | State-reporting strip with the body states (task0001 AC-1) | Body states reported per the SPEC transition table; written bytes identical whatever state is carried in; each new row confirmed by the extended end-state oracle; existing oracle reference streams keep their classification | Unit |
| TS-10 | String-body closure properties (task0001 AC-8) | The closure is `ESC`, CAN; a stream ending in an open OSC / DCS / APC body plus the closure ends in Ground per the oracle and probe text replays as after a switch pair, with no response; neither strip reads the closure as a strip-target start or as ST; a strip target right after it is still removed | Unit |
| TS-11 | Diff inspection of the integrated change (task0001 AC-9) | Cap value, overflow warning log, live forwarding, snapshot-time strip and reader cut derivation unchanged (NFR5); added state O(1), strip still one pass, overflow branch strips each flushed run once (NFR1) | Manual (review) |

## Code Quality Verification

- Format: none configured (`format_command` is empty in workflow.yaml); no crate-wide formatter run.
- Static analysis: none configured.
- Test-docs records: every test name in `test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` resolves in the `--lib` test listing, per the Resolution check of `.claude/rules/test-docs-records.md`; no predecessor record under `test-docs/` is modified (no test is renamed).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-10 pass |
| SC-2 | All test scenarios pass | TS-1 to TS-11 |
| SC-3 | Performance meets specified goals (NFR1) | TS-11 (structure); the existing time-budget guards in the overflow tests pass within TS-7 |
| SC-4 | Documentation is complete (FR7) | TS-8 |
| SC-5 | Code review is completed | Review phase record (`reviews/roundN.yaml`) with no residual critical / high finding |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-4, TS-5, TS-9 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR3 | task0001 | TS-3, TS-5 |
| FR4 | task0001 | TS-2, TS-6 |
| FR5 | task0001 | TS-2, TS-3, TS-10 |
| FR6 | task0001 | TS-1, TS-2, TS-3, TS-6 |
| FR7 | task0001 | TS-8 |
| NFR1 | task0001 | TS-7, TS-11 |
| NFR2 | task0001 | TS-7, TS-9 |
| NFR3 | task0001 | TS-6, TS-7 |
| NFR4 | task0001 | TS-7 |
| NFR5 | task0001 | TS-7, TS-11 |

## Manual Testing (E2E Not Possible)

No E2E framework is configured. TS-1 reproduces the issue through the production reader automatically; the item below is for a human with a release build.

- [ ] Real-pane reproduction (user-run; needs a release build and a restarted mux daemon): in a mux pane, print `ESC ]0;` plus more than 512 KiB of body ending in `ESC`, then switch to the alternate screen and back, then print plain text; detach and reattach (or switch windows). The plain text is displayed after the snapshot replay.

## Performance / Security Verification

- NFR1: the added state is O(1), the strip stays one pass and the overflow branch strips each flushed run once — checked by TS-11; the existing time-budget assertions of the overflow tests still pass (TS-7).
- TM-1: every cut path writes `ESC` + CAN when the written stream ends inside an open OSC / DCS / APC body, so output after the cut is not absorbed into the string on replay — checked by TS-1, TS-2, TS-3 and TS-4 (replay equals the raw-stream reference; the text after the cut is displayed).
- TM-2: the overflow hold decision is unchanged and the closure is neither ST nor a strip-target opener — checked by TS-6 (every strip-target continuation after the open-body ending is still removed and the totals equal the reference feeding) and TS-10.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (check, CLI-only check) | 2 | 0 | 0 |
| Test scenarios | 11 (TS-1 to TS-11) | 9 (TS-1 to TS-7, TS-9, TS-10) | 0 | 2 (TS-8, TS-11) |
| Code quality | 1 (test-docs record resolution) | 1 | 0 | 0 |
| Success criteria | 5 (SC-1 to SC-5) | 2 (SC-1, SC-2) | 0 | 3 (SC-3, SC-4, SC-5) |
| Performance / Security | 3 (NFR1, TM-1, TM-2) | 2 (TM-1, TM-2) | 0 | 1 (NFR1) |
| Manual testing | 1 (real-pane reproduction) | 0 | 0 | 1 |
