# Verification Document: asset-manifest-scanner-hardening

## Overview

**Feature**: asset-manifest-scanner-hardening
**SPEC.md**: `feature-docs/asset-manifest-scanner-hardening/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/asset-manifest-scanner-hardening/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/asset-manifest-scanner-hardening/tasks/task0001.md`.

## Build Verification

- Command:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings attributable to this change.

## Test Verification

- Command:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`
- Expected: exit code 0. The pre-existing tests of the `asset_manifest` target
  pass unmodified, and the tests added for TS1–TS5 pass.
- Focused runner used by the scenarios below:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test asset_manifest`
- Coverage target: no coverage tooling is configured for this project. Coverage
  is asserted structurally instead, through the requirement → scenario mapping
  in this document.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Path-exclusion predicate is given a path whose final component begins with the build-output prefix (a name merely starting with it, and a name equal to it) | Not excluded in both cases | Unit |
| TS2 | Path-exclusion predicate is given the four build-output paths already pinned by the existing exclusion test, plus an ordinary source path | The four stay excluded; the ordinary path stays in scope | Unit |
| TS3 | Temporary tree contains a font embedding in a source file whose name begins with the build-output prefix, and the same embedding inside a build-output directory | The first is found by the scan; the second yields nothing | Integration |
| TS4 | Extractor is given text shaped like the build script's generated-code site: a call with a non-literal argument, followed by an unrelated string literal | Empty set; the unrelated literal is never reached | Unit |
| TS5 | Extractor is given a call with a non-literal argument immediately followed by a call embedding a real font path | Exactly one name, the real font file name | Unit |
| TS6 | The existing repository-facing test runs unmodified against the real tree | The extracted set is still exactly the seven documented font names | Integration |
| TS7 | The whole `asset_manifest` test target is run | Green: pre-existing tests plus the added tests | Integration |

## Code Quality Verification

- Format: no format command is configured in `workflow.yaml`
  (`project.components.main.format_command` is empty). The change follows the
  surrounding file's existing formatting; no crate-wide reformatting is run.
- Static analysis: no separate static-analysis command is configured; the
  compiler diagnostics from the build command above are the check.
- Structural review (NFR1): the changed analysis functions take values in and
  return values out, with no filesystem access and no dependence on process
  state. Filesystem access stays confined to the repository-binding section at
  the end of the file.
- Cost review (NFR4): the argument scan is a single forward pass over the text,
  with no re-examination of text already passed.
- Convention review (NFR6): added tests use `<subject>_<scenario>_<expected>`
  naming and the file's existing construction style (synthetic values for pure
  predicates, a temporary directory tree for the walk-level scenario).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | An undeclared font embedded in a source file whose name begins with the build-output prefix makes the declaration guard fail | TS3 (hermetic form); optionally the live reproduction in Manual Testing |
| AC2 | Directory-component exclusion preserved; a final-component file name beginning with the prefix is not excluded | TS1 + TS2 |
| AC3 | Text shaped like the build script's generated-code site yields the empty set and no pickup of the following literal | TS4 |
| AC4 | A real embedding following a non-literal-argument call is still extracted, exactly once | TS5 |
| AC5 | The unmodified tree still yields exactly the seven documented fonts | TS6 |
| AC6 | The focused `asset_manifest` target is green with the pre-existing tests unmodified | TS7 + diff inspection of the pre-existing tests |
| AC7 | No dependency diff | Manifest inspection (Manual Testing below) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2, TS3 |
| FR2 | task0001 | TS4, TS5 |
| FR3 | task0001 | TS6 |
| FR4 | task0001 | No dedicated scenario — the parenthesis form is the only form exercised; verified by code review that no bracket / brace / nested-macro handling was added, and by TS6 showing the extraction result unchanged |
| FR5 | task0001 | TS4, TS5 |
| FR6 | task0001 | TS7 (with TS1–TS5 as the added recurrence detectors) |
| NFR1 | task0001 | Code Quality Verification — structural review |
| NFR2 | task0001 | Manual Testing — manifest inspection |
| NFR3 | task0001 | TS2, TS3 |
| NFR4 | task0001 | Code Quality Verification — cost review |
| NFR5 | task0001 | Manual Testing — change-set inspection |
| NFR6 | task0001 | Code Quality Verification — convention review |

## E2E Testing

No E2E framework is configured for this project
(`project.components.main.e2e_test_command` is empty), and this change has no
runtime surface reachable by an end-to-end run. Nothing to verify here.

## Manual Testing (E2E Not Possible)

- [ ] **Change-set inspection (NFR5, AC7)**: confirm the diff of this feature
      touches only `src-tauri/tests/asset_manifest.rs` among source files, and
      that `src-tauri/Cargo.toml` has no dependency or dev-dependency change.
- [ ] **Pre-existing-test integrity (AC6)**: confirm the diff contains no
      modification to any test that existed before this feature — added tests
      only.
- [ ] **Live reproduction check (AC1, optional)**: temporarily place a source
      file whose name begins with the build-output prefix under `src-tauri/`,
      containing an embedding of an undeclared font, run the focused test
      target, and confirm the declaration guard now fails. Remove the temporary
      file afterwards and re-run the target to confirm it is green again. TS3
      covers the same behaviour hermetically; run this only when a
      working-tree-level confirmation is wanted.

## Performance / Security Verification

- NFR4 (performance): the scan cost stays roughly linear in source-text length.
  No measurement threshold is defined — the check is the structural one under
  Code Quality Verification (single forward pass, no backtracking).
- Security: this feature adds no runtime code path, reads no untrusted input at
  runtime, and changes no dependency. Nothing to verify.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios (TS1–TS7) | 7 | 7 | 0 | 0 |
| Code quality / structural reviews | 5 | 2 | 0 | 3 |
| Success criteria (AC1–AC7) | 7 | 5 | 0 | 2 |
| Manual items | 3 | 0 | 0 | 3 |
