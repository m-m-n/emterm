# Verification Document: wheel-report-tests-yaml-name-drift

## Overview

**Feature**: wheel-report-tests-yaml-name-drift
**SPEC.md**: `feature-docs/wheel-report-tests-yaml-name-drift/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/wheel-report-tests-yaml-name-drift/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `tasks/task0001.md`.

> **Three `AC-n` namespaces overlap in this feature.** The `AC-1`..`AC-6` used
> in this document are the feature's own success criteria from SPEC.md. The
> repaired file `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`
> has its own `AC-1`..`AC-10`, and `tasks/task0001.md` has its own `AC-1`..`AC-7`.
> References to the other two always name their file.

## Build Verification

| Component | Command | Expected |
|---|---|---|
| main | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors — unchanged from the base, since no Rust source is touched |
| cli_only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors — unchanged from the base |
| typescript | `bun run typecheck` | exit code 0 — unchanged from the base; no TypeScript is touched |

This feature changes documentation only, so all three are regression guards
rather than verifications of new behavior. A failure in any of them means
something outside the declared change set was modified.

## Test Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Regression-comparison form (order-sensitive suites): append `-- --test-threads=1`
- Command (typescript): `bun test`
- Coverage target: not configured in `workflow.yaml`, and no new code is
  introduced, so coverage is unchanged by construction. No threshold applies.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Each of the 13 distinct test names cited across AC-1..AC-10 of the repaired `task0001.tests.yaml` is looked up in `src-tauri/src/window_host/tests.rs` and its defining line recorded | Every one of the 13 names has a recorded definition file and line; none is unresolvable | Manual evidence (no automation added) |
| TS-2 | The `main` test command filtered by the repaired AC-7 name is run | Non-zero test count reported (the pre-change name reports `0 tests run`) | Integration (existing suite, name-filtered) |
| TS-3 | The repaired file is searched for the old test name and for `last_tracking_active` | No match for either | Manual evidence (text search of one file) |
| TS-4 | The diff of the repaired file against the base is reviewed | Every hunk falls inside the four enumerated locations; no `red_confirmed` line and no AC-9 line appears | Manual evidence (`git diff` review) |
| TS-5 | The `--lib` suite is re-run as a regression guard | Same pass/fail/ignored counts as the base; confirms the change is documentation-only | Integration |

Commands for the automatable scenarios:

- TS-2: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
- TS-5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

## Code Quality Verification

- Format (main): no `format_command` is configured for the Rust components in
  `workflow.yaml`, and the project does not enforce crate-wide formatting. No
  Rust source is touched by this feature, so nothing is formatted.
- Format (typescript): `bunx biome format .` — regression guard only; no
  TypeScript is touched.
- Static analysis: none configured. The edited artifact is a YAML evidence
  record; its quality gate is the YAML parse plus the structural comparison
  under AC-5 below.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Every test name referenced by the repaired `task0001.tests.yaml` resolves to a definition in `src-tauri/src` | TS-1 (definition file+line for each of the 13 names) and TS-2 (name-filtered run reporting a non-zero count) |
| AC-2 | The AC-7 entry reads `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`, and the old name appears nowhere in the file | TS-3 (no match for the old name) plus confirming the new name is present in the AC-7 `tests` list |
| AC-3 | `last_tracking_active` appears nowhere in the file, and the AC-6 / AC-7 `red_reason` bodies read coherently without it | TS-3 (no match) plus a read-through of both bodies checking for dangling separators, unbalanced quotes and orphaned conjunctions |
| AC-4 | Every `red_confirmed` value, every acceptance-criterion key, and all text outside the four enumerated locations are byte-identical to the pre-change file | TS-4 (diff hunks confined to the enumerated locations; no `red_confirmed` and no AC-9 line in the diff) |
| AC-5 | The repaired file parses as valid YAML with the same top-level structure (`task_id`, `baseline_failures`, `final_failures`, `acceptance_tests` AC-1..AC-10, `notes`) | YAML parse of the repaired file succeeds and its top-level key set plus its AC-1..AC-10 key set match the pre-change file |
| AC-6 | No file outside `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` is modified (beyond this feature's own workflow and evidence documents) | Change-set listing against the base revision by path; TS-5 corroborates that no code changed |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-3, plus the AC-3 read-through of the AC-6 body |
| FR3 | task0001 | TS-3, plus the AC-3 read-through of the AC-7 body |
| FR4 | task0001 | TS-3, plus the AC-3 read-through of the AC-7 closing clause |
| FR5 | task0001 | TS-4 |
| FR6 | task0001 | TS-1, TS-2 |
| NFR1 | task0001 | TS-5, plus AC-6's change-set listing |
| NFR2 | task0001 | TS-4 (diff confined to the enumerated locations preserves key order, folded scalars and wrapping style), plus AC-5's YAML parse and structure comparison |
| NFR3 | task0001 | TS-1 — the cited definitions exist only if the upstream merge this work is sequenced after is present on the base. Also checked directly as a precondition under Manual Testing below. |

## E2E Testing

Not applicable. No `e2e_test_command` is configured for any component in
`workflow.yaml`, and this feature touches no user-visible surface.

## Manual Testing (E2E Not Possible)

- [ ] Precondition (NFR3): confirm on the verification base that the repaired
      file and the successor test definition are both present — i.e. the
      upstream merge this work is sequenced after is in the base.
- [ ] TS-1: for each of the 13 distinct names cited by the repaired file,
      record the definition's file and line. AC-9's `tests` list is empty and
      contributes no name; the `wheel_report_notches_*` mention inside AC-9's
      prose is a pattern, not a citation.
- [ ] TS-3: text-search the repaired file for the old test name and for
      `last_tracking_active`; both must return no match.
- [ ] TS-4: review the diff of the repaired file and confirm every hunk falls
      inside the four enumerated locations, and that no `red_confirmed` line
      and no AC-9 line is touched.
- [ ] AC-3 read-through: read the AC-6 and AC-7 `red_reason` bodies end to end
      and confirm each reads as a coherent sentence, with balanced
      escaped-backtick and code-span delimiters after the removals.
- [ ] AC-5: parse the repaired file as YAML and compare its top-level key set
      and its AC-1..AC-10 key set against the pre-change file.

No mockup comparison applies: the design step was skipped and this feature has
no visual surface.

## Performance / Security Verification

Not applicable. The change is documentation-only: no runtime code path, no
authentication or authorization surface, no input handling, and no data
protection concern is involved. SPEC.md records both as not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios (TS-1..TS-5) | 5 | 2 (TS-2, TS-5) | 0 | 3 (TS-1, TS-3, TS-4) |
| Success criteria (AC-1..AC-6) | 6 | 0 | 0 | 6 |
| Code quality | 1 | 1 | 0 | 0 |
| Precondition / structural checks | 3 | 0 | 0 | 3 |
| **Total** | **18** | **6** | **0** | **12** |
