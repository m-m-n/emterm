# Verification Document: mux-cut-csi-post-strip-closure

## Overview
**Feature**: mux-cut-csi-post-strip-closure / **SPEC.md**: `feature-docs/mux-cut-csi-post-strip-closure/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-cut-csi-post-strip-closure/IMPLEMENTATION.md`

Every command runs from the repository root (the integration worktree root);
never change into `src-tauri/`. Test identifiers R1-R9 are the full test paths
listed in IMPLEMENTATION.md "Regression test identifiers".

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (NFR5, CLI-only build): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both commands.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0.
- Coverage target: not configured (no coverage tool in workflow.yaml).
- Test listing (TS-6 name resolution): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`. Every identifier R1-R9 appears as a `<name>: test` line, and so does every test path that DECISIONS.md cites.
- Known flaky tests outside this feature: the `tabs.rs` replay tests (non-deterministic under parallel execution, stable with `--test-threads=1`) and the `tmux_sockets` discover tests (rare fork-window race). Rerun such a failure alone before treating it as a regression.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | CLOSING_CASES gains `ESC[6` + OSC 777 launch, `ESC[6` + Kitty APC and `ESC[6` + `ESC[6n` (kept `ESC[6`, inside true); end cut, three cuts, byte by byte and fallback closing (R1); edge cases EC-2, EC-3, EC-4, EC-6 (R6) | Output is `ESC[6` + one DEL and the state is cleared on every path except the CSI-query row fed byte by byte, which writes its fed bytes with no DEL (IMPLEMENTATION.md D2); EC-2 and EC-3 write one DEL, EC-4 writes none, no cut writes both the designator ESC and a DEL | Unit |
| TS-2 | term_core raw-stream comparison: three forms × switch pairs × continuations including `n`, same-call cut and fallback closing (R2) | Rows, cursor and responses provoked after the cut equal the raw-stream reference; no cursor-position report; `n` is displayed as text | Unit |
| TS-3 | Inputs mixing open CSIs and held strip targets: every split position and byte-by-byte feeding; carried state after `ESC[6` + launch; EC-1 (R3) | Output, pending, awaiting designator and CSI state equal the one-call result; the CSI state after `ESC[6` + launch, and after EC-1, is the parameter state | Unit |
| TS-4 | OSC held at the 512 KiB cap, flushed in a run ending in `ESC[6` + a complete strip target, followed by a cut (R4) | One DEL follows the flushed run; a later `n` yields no cursor-position report on replay | Unit |
| TS-5 | Production reader with `run_visibility_restore_at`: `ESC[6` + strip target + switch pair, then `n` in a later read (R5) | Client responses, screen and cursor equal the raw-stream reference (for the CSI-query form, apart from the reference's own answer to the stripped query) | Integration |
| TS-6 | Decision record content (R8), cited-name resolution (test listing) and predecessor records untouched | R8 passes; every cited path resolves in the listing; the feature's diff contains neither predecessor file | Unit |
| TS-7 | Long input alternating open CSIs and strip targets, one call, two calls and byte by byte, with and without cuts (R7) | Finishes within the 10-second budget, no panic | Performance |
| TS-8 | `--no-default-features` cargo check | Exit code 0 | Build |
| TS-9 | (Planner-added for NFR1 / SPEC AC-7) Output identity of the state-reporting strip form against the existing write-path strip (R9), and the whole lib suite | R9 passes; every existing test passes with no expectation changed | Unit |

## Code Quality Verification
- Format: not configured in workflow.yaml (`format_command` is empty). Do not run a crate-wide formatter.
- Static analysis: no tool configured; the build commands report no new warnings in the changed files.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | For the three forms, a ring cut in the same call or closed by the fallback, then `n`, replayed through term_core, gives no CPR and shows `n` as text | R2 and R5 pass |
| AC-2 | The three CLOSING_CASES rows behave as expected on every existing path | R1 passes |
| AC-3 | After a cut-free call, `csi_phase()` is the post-strip state (`Some(Param)` after `ESC[6` + launch); output and state are split-invariant | R3 passes |
| AC-4 | After an overflow flush, a cut writes one DEL when the stripped output ends in an open CSI | R4 passes |
| AC-5 | The added regression tests fail on the pre-change code and pass after | task0001's test-docs record shows red confirmed for R1's new rows, R2, R3, R4 and R5 |
| AC-6 | The decision record holds the verdict, rationale and regression tests for `4c0ad9058a983648` and the FR6 residuals; the predecessor `reviews/round1.yaml` and `DECISIONS.md` are unchanged | R8 passes; `git diff --name-only` from the feature base to HEAD lists neither `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml` nor `feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md` |
| AC-7 | Existing tests pass unchanged; intentionally changed expectations are listed in the record; renames follow `.claude/rules/test-docs-records.md` | Lib suite passes; the diff of existing test files only adds (R1 rows and its row-specific expectation, new module declarations); the record's behavior-changing tests section matches the diff |
| AC-8 | The lib test command and the `--no-default-features` check pass | Build Verification and Test Verification commands |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-5 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-5 |
| FR3 | task0001 | TS-4 |
| FR4 | task0001 | TS-1, TS-2 |
| FR5 | task0002 | TS-6 |
| FR6 | task0001, task0002 | TS-6, TS-9 |
| NFR1 | task0001 | TS-9 |
| NFR2 | task0001 | TS-7, plus the inspection in Performance / Security Verification |
| NFR3 | task0001 | TS-2, TS-5 |
| NFR4 | task0001 | TS-7 |
| NFR5 | task0001 | TS-8 |

## E2E Testing
No E2E framework is configured (`e2e_test_command` is empty); no E2E scenario applies.

## Manual Testing (E2E Not Possible)
- [ ] The reproduction steps of the goal in workflow.yaml, on a release binary the user builds and runs in a mux session: a pane writes `ESC[6` followed by a complete strip target, a snapshot (tab switch or reattach) takes a cut right after it, then the pane writes `n`. No cursor-position report (`ESC[row;colR`) reaches the pane and `n` is displayed. TS-5 covers the same production reader path automatically; this check runs only when the user chooses to.

## Performance / Security Verification
- NFR2: inspect the task0001 diff of the write filter: no new loop over the fed or written bytes; the post-strip CSI state comes from the strip's existing pass (IMPLEMENTATION.md D1). R7 passes within its budget.
- NFR4: R7 passes; the existing pending-cap and overflow-flush tests in `round3_write_path`, `round4_cut_csi` and `round4_chain` pass unchanged.
- TM-1: the cut closing and the carried state follow the strip-applied output on the in-call cut, the empty-segment and fallback closing, and the overflow flush — checked by R1, R2, R3, R4, R5 and R6 passing; in each, the trailing `n` provokes no cursor-position report and is displayed as text.
- TM-2: O(1) state inside the strip's existing pass, pending cap and strip-filtered overflow flush unchanged, no panic — checked by R7 and the existing cap and overflow tests passing, together with the NFR2 inspection above.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-9) | 9 | 9 | 0 | 0 |
| Success criteria (AC-1 to AC-8) | 8 | 8 | 0 | 0 |
| Performance / Security (NFR2, NFR4, TM-1, TM-2) | 4 | 3 | 0 | 1 |
| Manual reproduction | 1 | 0 | 0 | 1 |
