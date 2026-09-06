# Verification Document: ac6-red-reason-change-set

## Overview

**Feature**: ac6-red-reason-change-set
**SPEC.md**: `feature-docs/ac6-red-reason-change-set/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ac6-red-reason-change-set/IMPLEMENTATION.md`

The verified artifact is one machine-readable YAML record,
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`. Verification is a
YAML parse plus text and diff assertions; no compiled or bundled code is
exercised (SPEC assumption A-6, IMPLEMENTATION.md decision D-4).

Throughout this document, "the record" means
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`, and "SPEC AC-n"
refers to the Success Criteria of SPEC.md — distinct from the acceptance
criteria numbered inside `tasks/task0001.md`.

## Build Verification

The feature changes no compiled or bundled input, so no build output can
change. The project build commands are run as an unchanged-baseline regression
check only; they are not an acceptance gate (IMPLEMENTATION.md D-4).

| Component | Command | Expected |
|---|---|---|
| rust | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0; result identical to the pre-change baseline |
| typescript | `bun run typecheck` | exit code 0; result identical to the pre-change baseline |

A failure here that also reproduces on the base commit is a pre-existing
condition, not a finding against this feature. Record it as such rather than
attributing it to the change set.

## Test Verification

| Component | Command | Role |
|---|---|---|
| rust | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | regression baseline only (not an acceptance gate) |
| typescript | `bun test` | regression baseline only (not an acceptance gate) |

**Coverage target**: not applicable. The change set adds no executable line, so
line coverage is not a meaningful measure for this feature. The meaningful
measure is requirement coverage, tabulated below: every FR and NFR maps to at
least one scenario.

The feature's own verification is the scenario table that follows. Each
scenario is executed as an ad-hoc command; no test file is committed
(IMPLEMENTATION.md D-3).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Safe-load the record with a YAML parser (`python3` + PyYAML) and inspect the loaded mapping. | Load succeeds. The `acceptance_tests` key set equals {AC-1, AC-2, AC-3, AC-4, AC-5, AC-6, AC-7}; the top-level key set equals {`task_id`, `baseline_failures`, `final_failures`, `acceptance_tests`, `notes`}; `AC-6.red_confirmed` is false. | Unit |
| TS-2 | Fixed-string check on the loaded AC-6 `red_reason` for the clause asserting that the whole change set is that one YAML file, and for any other change-set file-count or sole-entry assertion. | Occurrence count is 0 after the edit (it is 1 before the edit — a red→green criterion). No count or sole-entry assertion in any wording. | Unit |
| TS-3 | Substring check on the loaded AC-6 `red_reason` for the "no Rust" and "no TypeScript" claims and for both record paths (`test-docs/stale-test-name-refs/task0001.tests.yaml` and `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`). | All four present. | Unit |
| TS-4 | Substring check on the loaded AC-6 `red_reason` for `feature-docs/` named as an expected carve-out. | Present after the edit (count is 0 before the edit — a red→green criterion). | Unit |
| TS-5 | Load the committed pre-edit version of the record (from git history) and the post-edit version; compare every `acceptance_tests` entry other than AC-6, plus `task_id`, `baseline_failures` and `final_failures`. Also inspect `git diff` for the file. | Every compared value is equal. The diff's hunks are confined to the AC-6 entry's `red_reason`. | Integration |
| TS-6 | Raw-text check (not a parsed check) on the file: the AC-6 `red_reason` still uses the `>-` folded block-scalar indicator; the top-level key order in the raw file is unchanged; indentation is 2-space and the line-wrap width matches the surrounding entries. | All hold. The diff reads as a same-style revision, not a reformat. | Unit |
| TS-7 | `git status --porcelain` and `git diff --stat` over this feature's change set. | No listed path ends in `.rs`, `.ts` or `.css`, and no listed path is under `src-tauri/`, `crates/` or `scripts/`. Listed paths are contained in: the record, `feature-docs/ac6-red-reason-change-set/**`, `test-docs/ac6-red-reason-change-set/**`. Invariant guard — no observable pre-state. | Integration |
| TS-8 | Read the rewritten AC-6 `red_reason` and judge it: the preserved evidence is intact (`red_confirmed: false`, the invariant-guard framing, the clean pre-state observation, the two-hunk `git diff` observation, the untouched-keys list), and every assertion the text makes is re-checkable from the record plus the ac7 task's own commit, with no dependency on a pre-edit working state. | Both hold. | Manual (human judgment) |

## Code Quality Verification

- **Format**: not applicable to the change set. `cargo fmt --check` and
  `bunx biome format .` cover Rust and TypeScript/web sources, none of which
  this feature touches. Run them only to confirm the baseline is unchanged.
- **Static analysis**: the applicable check is the YAML parse in TS-1 plus the
  formatting-fidelity check in TS-6.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SPEC AC-1 | The AC-6 `red_reason` no longer contains the single-file phrase and asserts no change-set file count. | TS-2 |
| SPEC AC-2 | The AC-6 `red_reason` states the change set is YAML documentation only, naming both records, and contains no Rust and no TypeScript file. | TS-3 |
| SPEC AC-3 | The AC-6 `red_reason` mentions `feature-docs/` workflow-generated artifacts as an expected carve-out. | TS-4 |
| SPEC AC-4 | The record still parses and still has exactly the seven `acceptance_tests` entries with `AC-6.red_confirmed` false. | TS-1 |
| SPEC AC-5 | `git diff` for the file shows hunks confined to the AC-6 `red_reason`; every other entry and top-level key is untouched. | TS-5 |
| SPEC AC-6 | This feature's change set contains no Rust and no TypeScript file. | TS-7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-3 |
| FR2 | task0001 | TS-4 |
| FR3 | task0001 | TS-2 |
| FR4 | task0001 | TS-8 |
| FR5 | task0001 | TS-5 |
| NFR1 | task0001 | TS-1 |
| NFR2 | task0001 | TS-6 |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-8 |

Every requirement maps to at least one task and at least one scenario; there is
no uncovered requirement.

## E2E Testing

The project has no E2E framework and no E2E run command
(`e2e_test_command` is empty for both components). Nothing in this feature is
E2E-testable: there is no runtime surface to drive.

## Manual Testing (E2E Not Possible)

- [ ] TS-8: read the rewritten AC-6 `red_reason` end to end and confirm the
      preserved evidence is intact and that every assertion it makes is
      re-checkable after the fact from the record plus the ac7 task's own
      commit. This is a judgment about text, so it has no mechanical form —
      record the judgment and its basis.
- [ ] Confirm the replacement clause did not substitute a different over-claim:
      an exhaustive "the change set is exactly these files" enumeration is
      still a count claim and still fails SPEC AC-1, even though it would pass
      a naive fixed-string check for the original phrase.

No mockup comparison item applies: the design step is skipped for this feature
and there is no DESIGN.md or mockup to compare against.

## Performance / Security Verification

Not applicable. The feature changes no runtime behaviour, and the change set is
a prose revision inside a documentation record — no authentication,
authorization, input-handling or data-protection surface is involved. SPEC.md
declares both sections not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Record shape / parse (TS-1) | 1 | 1 | 0 | 0 |
| Rewritten-clause content (TS-2, TS-3, TS-4) | 3 | 3 | 0 | 0 |
| Preservation / locality (TS-5, TS-6) | 2 | 2 | 0 | 0 |
| Change-set invariant (TS-7) | 1 | 1 | 0 | 0 |
| After-the-fact verifiability (TS-8) | 1 | 0 | 0 | 1 |
| Regression baseline (build/test commands) | 4 | 4 | 0 | 0 |
| **Total** | **12** | **11** | **0** | **1** |

"Automated" here means executable as a command with a mechanical pass/fail
result, not that a committed test file exists — by decision D-3 none does.
