# Verification Document: ac8-red-reason-self-search

## Overview

**Feature**: ac8-red-reason-self-search /
**SPEC.md**: `feature-docs/ac8-red-reason-self-search/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/ac8-red-reason-self-search/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/ac8-red-reason-self-search/tasks/task0001.md`.

The feature changes one folded YAML scalar in a documentation evidence record
under `test-docs/`. The project's build and test commands are therefore run as a
no-regression guard, not as the primary acceptance evidence; acceptance rests on
the scripted parse/assert check and on git inspection (SPEC A5).

## Build Verification

Run from the project root.

- Rust component: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- TypeScript component: `bun run build:viewer && bun run build:settings`
- Expected: exit code 0, no errors, and no difference from the pre-change
  baseline — the change touches no build input.

## Test Verification

- Rust component: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- TypeScript component: `bun run typecheck && bun test`
- Expected: results identical to the baseline the predecessor task recorded.
- Coverage target: not applicable. No automated suite covers `test-docs/`
  evidence records, and the change adds no executable path, so no coverage
  figure moves.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Scripted parse/assert check over `test-docs/ac7-red-reason-scope/task0001.tests.yaml`: load the file, assert the top-level key order (`task_id`, `baseline_failures`, `final_failures`, `acceptance_tests`, `notes`) and the eight-entry `acceptance_tests` mapping, then assert on the **loaded** AC-8 `red_reason` string — both required paths named, the exactly-two-paths statement present, the no-Rust / no-TypeScript wording present, the no-observable-pre-state framing present, no self-search report present; AC-8's `tests` still the empty list and `red_confirmed` still boolean `false`; the scalar still a folded block scalar at the same indentation as the file's other folded scalars, with no trailing whitespace | All assertions pass. Every needle used by the check lives in the check script, never inside the scalar being searched | Unit (scripted) |
| TS2 | Re-run the check the rewritten scalar describes: enumerate the predecessor task's commit change set against its merge base (`git diff --stat <merge-base>..HEAD`) and compare against the recorded wording | The enumeration yields exactly the two named paths and contains no Rust file and no TypeScript file, matching the scalar verbatim in substance | Integration |
| TS3 | `git diff` / `git diff --stat` inspection of the changed file, plus a region-wise byte comparison of the untouched regions against the base revision | Exactly one changed file, exactly one hunk, confined to the AC-8 `red_reason`. `task_id`, `baseline_failures`, `final_failures`, the AC-1..AC-7 entries, AC-8's `tests` and `red_confirmed`, and the `notes` block are byte-identical — including the AC-4 `red_reason` occurrences of `lists only` at lines 43 and 47 | Integration |
| TS4 | Regression guard: run the project's TypeScript commands (`bun run typecheck && bun test`) and the Rust `--lib` suite | Results match the baseline the predecessor task recorded; neither exercises the edited YAML content | Integration |

## Code Quality Verification

- Format: no format command is configured for either component in
  `workflow.yaml` (`project.components.*.format_command` is empty), and the
  changed file is YAML rather than Rust or TypeScript. Style is verified instead
  by TS1's scalar-form assertions and by the manual style read-through below.
- Static analysis: `bun run typecheck` (part of the TypeScript test command
  above) and `cargo check`. Neither analyses the changed file; both are run as a
  regression guard.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| CRIT-1 | All functional requirements FR1–FR7 are implemented and verified | Functional Requirements Coverage table below |
| CRIT-2 | All non-functional requirements NFR1–NFR4 are satisfied | Functional Requirements Coverage table below |
| CRIT-3 | All test scenarios TS1–TS4 pass | Test Scenarios table above; all four executed and recorded |
| CRIT-4 | All SPEC acceptance criteria AC-1 – AC-7 hold | TS1 covers SPEC AC-1, AC-2, AC-3, AC-5, AC-7; TS2 covers AC-4; TS3 covers AC-6 |
| CRIT-5 | Code review is completed | Review phase output for this feature |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 — the loaded AC-8 `red_reason` reports no outcome of searching the record's own text and quotes no needle for such a search |
| FR2 | task0001 | TS2 — the property the replacement clause states is re-derived from the change set and matches; TS1 confirms no claim beyond it was introduced |
| FR3 | task0001 | TS1 — both record paths named and the exactly-two-paths statement present in the loaded scalar |
| FR4 | task0001 | TS1 — the no-Rust / no-TypeScript wording present in the loaded scalar |
| FR5 | task0001 | TS1 — the no-observable-pre-state framing present; AC-8's `red_confirmed` still `false` and `tests` still the empty list |
| FR6 | task0001 | TS3 — one hunk confined to the AC-8 `red_reason`; every other region byte-identical to the base revision, including `lists only` at lines 43 and 47 |
| FR7 | task0001 | TS1 — parser load succeeds, top-level key order and the eight AC entries hold, folded block scalar introducer and indentation unchanged, text still English |
| NFR1 | task0001 | TS4 — project test commands match the baseline; TS3 confirms only one file under `test-docs/` changed |
| NFR2 | task0001 | TS1 — scalar form, indentation, comparable line width, no trailing whitespace; plus the manual style read-through below |
| NFR3 | task0001 | TS2 — re-executing the check the scalar describes reproduces the recorded outcome; no remaining clause is contradicted by re-running it |
| NFR4 | task0001 | TS1 — a plain parser load yields the same key structure consumers already rely on |

## E2E Testing

Not applicable. `test/README.md` documents Rust `cargo test` and `bun test` as
the project's only test frameworks and records that no E2E infrastructure
exists; the feature adds no user-facing path an E2E test could exercise.

## Manual Testing (E2E Not Possible)

- [ ] MT-1: Read the rewritten AC-8 `red_reason` end to end and confirm it
      contains no self-referential claim of any kind — no statement about what
      does or does not appear in the record's own text, in any phrasing. This is
      the defect class under repair, and a mechanical needle list cannot close it
      completely (SPEC EC1).
- [ ] MT-2: Confirm every remaining sentence of the scalar is supported by the
      change set or by the no-observable-pre-state framing, and that the rewrite
      introduced no new claim the record cannot support.
- [ ] MT-3: Read this feature's own generated record
      `test-docs/ac8-red-reason-self-search/task0001.tests.yaml` and confirm its
      statement about this task's own change set (which again touches two paths)
      is a verified property rather than a search result (SPEC EC2).
- [ ] MT-4: Compare the rewritten scalar side by side with the file's other
      `red_reason` scalars and confirm the style matches — folded block scalar,
      English prose, comparable line width, no trailing whitespace (NFR2).

No mockup comparison item applies: the design step is `skipped` for this feature
and no visual artifact exists.

## Performance / Security Verification

Not applicable. The change is confined to a documentation evidence record; there
is no input handling, no privilege boundary, and no network or filesystem
surface (SPEC SC1). The SPEC states no performance requirement.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS1–TS4) | 4 | 4 | 0 | 0 |
| Success criteria (CRIT-1–CRIT-5) | 5 | 4 | 0 | 1 |
| Requirements coverage (FR1–FR7, NFR1–NFR4) | 11 | 11 | 0 | 0 |
| Manual checks (MT-1–MT-4) | 4 | 0 | 0 | 4 |
