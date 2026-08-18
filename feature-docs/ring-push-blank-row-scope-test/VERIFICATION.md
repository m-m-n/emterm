# Verification Document: ring-push-blank-row-scope-test

## Overview

**Feature**: ring-push-blank-row-scope-test
**SPEC.md**: `feature-docs/ring-push-blank-row-scope-test/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ring-push-blank-row-scope-test/IMPLEMENTATION.md`

This document defines the INTEGRATED verification of the feature. Task-level acceptance
criteria live in `feature-docs/ring-push-blank-row-scope-test/tasks/task0001.md`.

Scenario IDs below (`TS1`–`TS4`) are the SPEC.md "Test Scenarios" IDs. REQUIREMENTS.md
section 12.1 numbers the same four scenarios `TS-1`–`TS-4`; they are the same scenarios and
`TS1` is used as the canonical form throughout this document.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors and no new warnings attributable to this feature.

Only the `term_core` component is in scope. No other component of the project has a file in
this feature's change set, so no other component's commands are run.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0; zero failures; the pass count is at least the pre-feature count
  (the feature adds assertions to an existing test rather than adding a test function, so the
  test count is expected to be unchanged).
- Coverage target: **not expressed as a percentage**. No coverage tooling is configured for
  this crate, and a line-coverage number would not move for this change — the lines under
  observation were already executed by the pre-feature test. Adequacy is judged instead by
  TS3 (the mutation check): the feature is adequately covered exactly when a whole-table
  clear turns the extended test red.
- Determinism: the suite is expected to pass without any thread-count restriction. A result
  that only reproduces with a serialized harness is a failure of this feature's determinism
  requirement, not an acceptable workaround.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Row-scoped clear observed — the extended `ring_push_blank` survivor test: populate the to-be-recycled row and a second, non-recycled survivor row with overflow-bound content, capture both absolute row keys before the scroll, then drive the full-screen scroll path with a line feed from the last row | The recycled row's overflow entries are gone and its reverse-index key is absent; the survivor row's overflow entry is still present and its reverse-index key still carries the expected column set | Unit |
| TS2 | Anti-vacuity guard — the pre-scroll assertions covering both the recycled row and the survivor row | Both rows' entries are asserted to genuinely exist in the overflow table and the reverse index before the scroll, so a fixture whose content stopped exceeding the inline cap fails loudly at the pre-assertion instead of making the survivor assertions vacuously true | Unit |
| TS3 | Mutation check — the row-scoped clear temporarily replaced by a whole-table clear inside the production module | The extended test fails on a survivor-row assertion (verbatim failure message recorded in the task's test record); the mutation is then reverted and the production file is byte-identical to its pre-feature state | Unit (transient, evidence reviewed at verification) |
| TS4 | Crate-level regression run and quality gate — the whole `term_core` lib suite under the default parallel harness, plus the component's build and format checks | Suite green with no regression in any neighbouring fixture (the untouched sibling emptiness-only test included); build clean; format check clean; no dev-dependency added to the crate manifest | Unit (suite) + tooling |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` — expected clean
  (exit code 0). Run in check mode only; never as a crate-wide rewrite.
- Static analysis: no separate static-analysis command is registered for this component. The
  build command above is the static check for this feature.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | All functional requirements FR1–FR6 are implemented; FR7 remains excluded | Requirements coverage table below; confirm the sibling emptiness-only test is untouched in the integrated diff |
| SC2 | All test scenarios TS1–TS4 pass | Test command output for TS1/TS2/TS4; the task test record for TS3 |
| SC3 | REQUIREMENTS.md section 11.1 AC-1…AC-8 are satisfied | Task acceptance criteria mapping in `tasks/task0001.md` Test Notes, plus the checks in this table |
| SC4 | The final diff touches only `crates/term_core/src/ring_buffer/tests.rs` plus the workflow-generated `feature-docs/` and `test-docs/` entries | Inspect the integrated diff: the production ring-buffer module must appear with no changed hunks, and every changed Rust line must lie inside an inline `#[cfg(test)]` module |
| SC5 | The format check is clean | Format command output |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 — the survivor row exists in the fixture, holds overflow-bound content, is distinct from the recycled row, and its absolute row key is captured before the scroll |
| FR2 | task0001 | TS1 — post-scroll assertions confirm the survivor row's overflow entry and its reverse-index column set survive |
| FR3 | task0001 | TS2 — the pre-scroll anti-vacuity block covers the survivor row as well as the recycled row |
| FR4 | task0001 | TS1 — the three pre-existing recycled-row post-assertions are still present and green; confirm by reading the diff that none was removed or weakened |
| FR5 | task0001 | TS3 — the recorded mutation evidence names the failing assertion and shows the production file restored byte-identically |
| FR6 | task0001 | TS1 — read the test body: the redundancy comment (single-site removal leaves the test green because the new bottom absolute row equals the evicted absolute row) is present and accurate |
| FR7 | — (excluded) | Not implemented by design. Verification is negative: the sibling emptiness-only test must be unchanged in the integrated diff |
| NFR1 | task0001 | TS3 — the production module is byte-identical after the mutation is reverted; reinforced by SC4's diff inspection (every changed Rust line inside an inline `#[cfg(test)]` module) |
| NFR2 | task0001 | TS4 plus diff review — inline test module, existing `test_*` name kept, fixture adjacent to the test it extends, leading explanatory comment block updated to match the test's widened claim |
| NFR3 | task0001 | TS4 — the suite builds and runs against an unchanged dependency set; the crate manifest shows no added dev-dependency in the diff |
| NFR4 | task0001 | TS4 — the suite passes under the default parallel harness, with no thread-count restriction |
| NFR5 | task0001 | TS4 — the component's format check is clean |

## E2E Testing

Not applicable. This project has no E2E infrastructure, and the `term_core` component has no
E2E command registered. Nothing about a change confined to an inline unit test of a pure
logic crate would be observable at an E2E layer.

## Manual Testing (E2E Not Possible)

The design step is `skipped` for this feature (no UI surface), so there is no visual
comparison item. The items below need human judgment and are performed by reading the
integrated diff and the task's test record — they introduce no new command.

- [ ] MT1 (FR5 / TS3): the task's test record contains the mutation evidence — the verbatim
      assertion-failure message, the name of the failing test, and the statement that the
      production module was restored byte-identically afterwards. Evidence that only says
      "confirmed" without the message is insufficient.
- [ ] MT2 (NFR1 / SC4): the integrated diff shows no changed hunk in the production
      ring-buffer module, and every changed Rust line lies inside an inline `#[cfg(test)]`
      module.
- [ ] MT3 (FR6): the redundancy comment in the test body is present, in English, and states
      both the fact (removing only one of the two clearing sites leaves the test green) and
      the reason (the two sites target the same row within a single push).
- [ ] MT4 (FR4 / FR7): the three pre-existing recycled-row assertions are intact, and the
      sibling emptiness-only test is untouched.
- [ ] MT5 (NFR2): the explanatory comment block above the test describes what the test now
      proves — row-scoped clearing, not merely eviction-time clearing.

## Performance / Security Verification (if applicable)

Not applicable. The feature adds test assertions only: no runtime behavior change, no input
handling change, no I/O, and no exposed surface. Suite runtime impact is negligible (a few
additional cells and assertions inside one existing test).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 4 | 3 (TS1, TS2, TS4) | 0 | 1 (TS3 — transient, evidence reviewed) |
| Code quality | 1 (format) | 1 | 0 | 0 |
| Success criteria | 5 | 3 (SC2, SC5, part of SC1) | 0 | 2 (SC3, SC4) |
| Requirements | 11 verified (FR1–FR6, NFR1–NFR5) + 1 excluded (FR7) | 6 | 0 | 6 |
