# Verification Document: ac7-red-reason-scope

## Overview

**Feature**: ac7-red-reason-scope
**SPEC.md**: `feature-docs/ac7-red-reason-scope/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ac7-red-reason-scope/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. The single task's own
acceptance criteria live in `feature-docs/ac7-red-reason-scope/tasks/task0001.md`.

The feature changes one folded block scalar in one YAML evidence record. No Rust and no
TypeScript source is touched and no rebuild is required (NFR5); the build and test commands
below are therefore run as no-regression checks only, and none of them exercises the edited
record's content (A-6).

## Build Verification

| Component | Command | Expected |
|---|---|---|
| main (Rust) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| cli_only (Rust) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors |
| typescript | `bun run typecheck` | exit code 0, no errors |

## Test Verification

| Component | Command | Expected |
|---|---|---|
| main (Rust) | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | pass count matching the baseline, 0 failures |
| typescript | `bun test` | pass count matching the baseline, 0 failures |
| cli_only | (no test command declared) | — |

- Coverage target: not applicable — the feature adds no product code. The verification value
  of this feature is carried entirely by the record-level scenarios below.
- A pass-count difference against the baseline is a regression signal, not a result of this
  change: nothing in the change set participates in either suite.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Load `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` with PyYAML before and after the change and assert on the parsed AC-7 `red_reason` string (never on raw line content — the folded scalar re-wraps) | Before: the string carries the single-member claim that `git status --porcelain` lists only `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`. After: it names both `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` and `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` as change-set members, identifies the second as the workflow-generated per-task test record tied to `test-docs/{feature}/**`, and no longer carries that claim or any equivalent single-member assertion about the whole change set | Unit |
| TS-2 | Assert the preserved substance of the same parsed scalar, and the AC-7 verdict fields | The parsed string still contains the no-Rust / no-TypeScript finding, the invariant-guard classification, the empty-pre-state statement and the zero-contiguous-occurrence identifier-search result; the AC-7 `red_confirmed` parses as the boolean false both before and after; the AC-7 `tests` value is unchanged | Unit |
| TS-3 | Inspect shape and format: the raw introducer of the AC-7 `red_reason`, the raw top-level key order, the parsed `acceptance_tests` entry count, and the character set of the rewritten scalar | The scalar is still introduced by `red_reason: >-` at its original indentation; the top-level key order is unchanged; `acceptance_tests` has exactly seven entries (AC-1..AC-7); the rewritten scalar is ASCII/English throughout | Unit |
| TS-4 | Diff the edited file against the base revision and compare the untouched regions | Every hunk lies inside the AC-7 `red_reason` scalar; `task_id`, `baseline_failures`, `final_failures`, the AC-1 through AC-6 entries and the trailing `notes` block are byte-identical to the base revision | Integration |
| TS-5 | Enumerate this feature's own task change set from git and inspect the text this feature produced about it | The change set contains no Rust path and no TypeScript path; its expected membership is stated as the two paths `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and `test-docs/ac7-red-reason-scope/task0001.tests.yaml` — the same multi-path shape the corrected text uses; and no criterion, scenario or record text this feature produced (the generated `test-docs/ac7-red-reason-scope/task0001.tests.yaml` above all) scopes the whole change set to a single file | Integration |
| TS-6 | (planner-added, closes the NFR4 gap) Compare the path list named in the rewritten scalar against the path list actually observed from the described task's commit, and check every remaining claim in the scalar against a recorded observation | The named path list equals the observed path list; no claim in the rewritten scalar asserts a state that was not inspected; if the observation diverged from the two paths FR1 names, the divergence appears as a reported plan deviation and the text follows the observation | Integration |

TS-6 is not in SPEC.md's own scenario list: NFR4 had no verifying scenario there, and this
document adds one rather than leaving the requirement uncovered.

## Code Quality Verification

- Format: `bunx biome format .` — expected to report no changes. The Rust components declare
  no format command; the project does not enforce crate-wide formatting.
- Static analysis: none configured for this change set. The record's own quality gate is the
  YAML load in TS-3.
- Record-text quality (this feature's own convention, IMPLEMENTATION.md Conventions): English,
  folded `>-` preserved, no claim beyond an observation, no whole-change-set statement
  narrowed to one member.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The parsed AC-7 `red_reason` names both change-set paths | TS-1 |
| AC-2 | The same scalar attributes the second path to the workflow-generated per-task record and to `test-docs/{feature}/**` | TS-1 |
| AC-3 | The same scalar still states no Rust file and no TypeScript file | TS-2 |
| AC-4 | The same scalar makes no single-member assertion about the whole change set | TS-1 |
| AC-5 | Invariant-guard classification, empty-pre-state statement and zero-occurrence result remain; `red_confirmed` still false; `tests` unchanged | TS-2 |
| AC-6 | Every diff hunk is inside the AC-7 `red_reason` scalar; the rest is byte-identical | TS-4 |
| AC-7 | PyYAML load with the seven-entry `acceptance_tests` mapping, unchanged raw top-level key order, `red_reason: >-` at the same indentation, English | TS-3 |
| AC-8 | This feature's own change set has no Rust and no TypeScript path, and is stated as the two expected paths, never as a single file | TS-5 |
| — | FR1..FR6 and NFR1..NFR6 implemented and holding | The coverage table below |
| — | TS-1 through TS-5 pass (TS-6 added by this document) | This document's scenario runs |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-1 |
| FR3 | task0001 | TS-2 |
| FR4 | task0001 | TS-1 |
| FR5 | task0001 | TS-2 |
| FR6 | task0001 | TS-4 |
| NFR1 | task0001 | TS-3 |
| NFR2 | task0001 | TS-3 |
| NFR3 | task0001 | TS-3 |
| NFR4 | task0001 | TS-6 (evidence grounding), TS-1 (the removed unobserved claim) |
| NFR5 | task0001 | TS-5, plus the no-regression build/test runs above |
| NFR6 | task0001 | TS-5 |

## E2E Testing

Not applicable. No E2E framework is configured for this project (`e2e_test_command` is empty
for every component in workflow.yaml), and the change set contains no executable surface.

## Manual Testing (E2E Not Possible)

- [ ] Read the rewritten AC-7 `red_reason` end to end and confirm it reads as a text
      correction rather than a re-judgement: the verdict, its classification and the recorded
      inspection are the same finding as before, stated about the change set that was
      actually observed (BO-2, BO-3).
- [ ] Confirm the rewritten scalar's tone, terminology and wrap style match the file's other
      entries, so the record still reads as one document (NFR2, NFR3).
- [ ] Confirm that the implementer either observed the two expected paths in the described
      task's commit, or reported a divergence — and that the text matches whichever was the
      case (A-3, NFR4).
- [ ] Confirm whether the implementer raised the `notes` block's single-file wording as a
      plan deviation (A-5), and record the decision. `notes` itself stays byte-identical
      either way.
- No mockup comparison item: the design step was skipped for this feature, so there are no
  mockups and no visual surface to compare against.

## Performance / Security Verification

Not applicable. No code path, no input handling and no data surface of the product is touched
(NFR5); the change is a documentation-text correction inside one YAML record.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (no-regression) | 3 | 3 | 0 | 0 |
| Test (no-regression) | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1..TS-6) | 6 | 6 | 0 | 0 |
| Code quality | 1 | 1 | 0 | 0 |
| Manual judgement | 4 | 0 | 0 | 4 |
| **Total** | **16** | **12** | **0** | **4** |
