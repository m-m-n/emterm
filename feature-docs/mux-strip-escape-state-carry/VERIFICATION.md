# Verification Document: mux-strip-escape-state-carry

## Overview
**Feature**: mux-strip-escape-state-carry / **SPEC.md**: `feature-docs/mux-strip-escape-state-carry/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-strip-escape-state-carry/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-strip-escape-state-carry/THREAT-MODEL.md`

All commands run from the integration worktree root (the project root of the checkout), never
from `src-tauri/` (`.claude/rules/core-build-location.md`). Test identifiers R1-R12 are the
ones in IMPLEMENTATION.md, "Regression test identifiers".

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  (workflow.yaml `project.components.src-tauri.build_command`)
- Expected: exit code 0, no errors, no new warnings in the files the tasks changed.
- The CLI-only check of NFR5 (TS-10) is not a `project.components` command; it is listed
  under Manual Testing.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  (workflow.yaml `project.components.src-tauri.test_command`)
- Expected: exit code 0, 0 failed; each of R1-R12 appears in the output as `ok` (this is the
  name-resolution check of IMPLEMENTATION.md D5).
- Coverage target: not measured (no coverage tooling is configured in `project.components`).
- Known unrelated flakiness: the `tabs.rs` replay tests (nondeterministic in parallel) and the
  `tmux_sockets` discover tests (rare). Re-run the same command once before reporting such a
  failure; it is not caused by this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | The reproduction through the write filter for each held target and the CSI query: call 1 `ESC` + target without a cut, call 2 `[6` with a trailing cut, then a later `n` (R4) | The emitted bytes are `ESC[6` + DEL; term_core replaying them and `n` gives no CPR and displays `n` | Unit |
| TS-2 | Split-invariance corpus of R1 extended with `ESC ESC` + each held target, + `[6`, + `(`; every split position and byte-at-a-time feeding, with and without a trailing cut (R1) | Emitted bytes, pending, awaiting flag and full carried state equal the single-call result; after each cut-free call the carried state equals the term_core end-state oracle's | Unit |
| TS-3 | A cut right after `ESC` + each removed construct: in-call cut, three cuts at one position, fallback closing, second fallback (R5); replay with `[6n`, `n`, `c` after the cut (R6); Escape-closure properties against term_core (R7) | The ring ends in `ESC` + the Escape closure (CAN); a second fallback writes nothing; at most one closure per cut; replay equals the raw-stream reference with no CPR, DA or RIS; CAN returns term_core to ground with no effect and never forms ST | Unit |
| TS-4 | A splice-made designator wait (`ESC` + target + `(` / `)`) and the case where the boundary scan awaits a designator but the written stream ends in ground (`ESC` + launch + `( ESC (`) (R8) | The first writes exactly one designator ESC at a cut and the carried Designator does not change the next call's strip output; the second writes nothing at a cut | Unit |
| TS-5 | An OSC held at the 512 KiB cap is flushed by a call ending in `ESC` + a complete removed construct, with and without a cut (R9) | With a cut: the stripped run + one Escape closure; without: the stripped run, Escape carried, and a later fallback writes the Escape closure once | Unit |
| TS-6 | The stateful-strip table returns Escape / Designator end states (`ESC[6 ESC`, `ESC ESC` + removed construct, `ESC[6 ESC(`, ...) (R2); output identity with the existing strip for every allowed start state and flag (R3) | Rows hold and are confirmed by term_core; output is byte-identical to the designator form | Unit |
| TS-7 | Production reader with the visibility-restore harness: `ESC` + removed construct + screen switch, then `[6n` / `c` in a later read (R10) | Client responses, screen and cursor equal the raw-stream reference | Integration |
| TS-8 | Long input alternating `ESC` + a removed construct, below and above the cap, in one call, two calls and byte at a time, with and without a cut (R11) | Finishes within 10 s, no panic, equal output for every feeding | Performance |
| TS-9 | Decision record content (R12) and the predecessor's record test | `a879a02de382209f` row with verdict, reason and R1-R11; residuals section; behavior-changing tests; predecessor records named as not modified; the predecessor's record test still passes | Record |
| TS-10 | CLI-only build check (`--no-default-features`) | Exit code 0, no errors | Build (manual) |
| TS-11 | Regression suite: the full `--lib` suite (SPEC AC-8) | All tests pass; no existing test renamed; no existing expectation changed in meaning except the two IMPLEMENTATION.md D4 rows | Unit (suite) |

## Code Quality Verification
- Format: none configured (`format_command` is empty).
- Static analysis: none configured; the build command's warnings for the changed files are
  reviewed (no new warnings).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Reproduction: ring `ESC[6` + DEL, no CPR, `n` displayed, for each held target and the CSI query | TS-1 (R4) |
| AC-2 | Split invariance with the extended corpus, with and without a trailing cut; carried state equals term_core's | TS-2 (R1) |
| AC-3 | Cut right after `ESC` + each removed construct ends in `ESC` + the FR4 closure; second fallback writes nothing; replay with `[6n`, `n`, `c` equals the raw-stream baseline | TS-3 (R5, R6, R7) |
| AC-4 | Splice-made designator wait closes with exactly one designator ESC; carried Designator does not change strip output | TS-4 (R8) |
| AC-5 | Overflow flush ending in `ESC` + a removed construct: Escape closure once with a cut, Escape carried without | TS-5 (R9) |
| AC-6 | The added regression tests fail before the fix and pass after it | task0001 test-docs record (`red_confirmed` per AC), TS-1 to TS-7 passing |
| AC-7 | Decision record content; predecessor DECISIONS.md and reviews/round1.yaml unchanged | TS-9 (R12) and the manual diff check below |
| AC-8 | Existing tests pass unchanged; changed expectations listed in the record; test-docs updated on rename | TS-11, TS-9 (behavior-changing section) |
| AC-9 | `--lib` test and `--no-default-features` check pass; long alternating input within budget, no panic | TS-11, TS-10, TS-8 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-5, TS-6 |
| FR2 | task0001 | TS-2, TS-4, TS-6 |
| FR3 | task0001 | TS-1, TS-3, TS-4, TS-5, TS-7 |
| FR4 | task0001 | TS-3 |
| FR5 | task0001 | TS-1, TS-2, TS-3 |
| FR6 | task0002 | TS-9 |
| NFR1 | task0001 | TS-6, TS-11 |
| NFR2 | task0001 | TS-8 |
| NFR3 | task0001 | TS-1, TS-3, TS-7 |
| NFR4 | task0001 | TS-8 |
| NFR5 | task0001 | TS-10, TS-11 |

## E2E Testing
None: the project has no E2E framework for this area (`e2e_test_command` is empty).

## Manual Testing (E2E Not Possible)
- [ ] TS-10: from the project root, run the CLI-only check
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`;
      expect exit code 0 and no errors. It is not an approved `project.components` command, so
      it runs only by hand or after approval; task0001's test-docs record also records its run.
- [ ] TS-9, predecessor records: the integrated diff (`workflow.implement.base_commit` to the
      integration branch) contains no change under `feature-docs/mux-cut-csi-post-strip-closure/`
      or `test-docs/mux-cut-csi-post-strip-closure/`.

## Performance / Security Verification
- NFR2: no byte-scanning pass is added to the reader's normal path. The existing one-pass
  test (`post_strip_state_form_is_one_linear_pass`) and the predecessor budget tests stay
  green, and task0001's test-docs record states the inspection result.
- NFR4: TS-8 (R11) finishes within 10 s without a panic; the 512 KiB cap and the overflow
  flush through the strip are unchanged (TS-5).
- TM-1: the full written end state is carried and every cut writes at most one closure
  chosen from it (Escape closure CAN, DEL, designator ESC, or nothing) — checked by R4 (no
  CPR after the reproduction), R5, R8 and R9 (exact closure bytes on every cut path), R6 and
  R10 (replay equals the raw-stream reference for `[6n`, `n` and `c`; no CPR, DA or RIS),
  and R7 (CAN returns term_core to ground with no effect and never forms ST).
- TM-2: the end state is O(1) state inside the single strip pass, nothing is held for CSI or
  Escape bytes, and the 512 KiB cap stays — checked by R11 and the existing budget tests
  (`post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` and the
  round4 budget tests) staying green.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (build command, TS-10) | 2 | 1 | 0 | 1 |
| Unit / filter level (TS-1 to TS-6) | 6 | 6 | 0 | 0 |
| Integration (TS-7) | 1 | 1 | 0 | 0 |
| Performance (TS-8) | 1 | 1 | 0 | 0 |
| Record (TS-9 test, predecessor diff check) | 2 | 1 | 0 | 1 |
| Regression suite (TS-11) | 1 | 1 | 0 | 0 |
| Security (TM-1, TM-2) | 2 | 2 | 0 | 0 |
| **Total** | **15** | **13** | **0** | **2** |
