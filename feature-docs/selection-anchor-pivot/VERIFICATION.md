# Verification Document: selection-anchor-pivot

## Overview

**Feature**: selection-anchor-pivot /
**SPEC.md**: `feature-docs/selection-anchor-pivot/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/selection-anchor-pivot/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no numeric target (project does not measure coverage);
  every Acceptance Criterion of task0001 maps to at least one test

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Word mode: two consecutive extends onto a word on the row above | Range = [upper word start .. origin word end] | Unit |
| TS-2 | Word mode: extend away, then back inside the origin word | Range = exactly the origin word | Unit |
| TS-3 | Word mode: extend above, then below past the origin word | Range = [origin word start .. lower word end] | Unit |
| TS-4 | Word mode: extend to an earlier word on the same row | Range = [pointer word start .. origin word end] | Unit |
| TS-5 | Line mode: repeated extends up / down / back to origin row | Full rows, origin row always included; origin row only when back | Unit |
| TS-6 | Eviction compensation with an active word/line selection | Origin shifted with endpoints; fully evicted selection dropped | Unit |
| TS-7 | Existing selection suite (character mode, single-extend, resolve) | All pass unmodified | Unit |

## Code Quality Verification

- Format: not configured for this component (format_command empty)
- Static analysis: covered by the build verification command

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR3 implemented and tested | TS-1..TS-6 pass |
| SC-2 | NFR1 regression suite passes | TS-7: full `--lib` test run green |
| SC-3 | Code review completed | review phase record (reviews/) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR2 | task0001 | TS-5 |
| FR3 | task0001 | TS-6 |
| NFR1 | task0001 | TS-7 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: In a running terminal, double-click a word, drag to the line
  above; the selection spans from the word under the pointer to the origin
  word, and the copied text matches the highlight.
- [ ] MT-2: Triple-click a line, drag up and then back down; the origin line
  stays selected throughout, and returning to the origin row leaves only
  that row selected.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests | 7 (TS-1..TS-7) | 7 | 0 | 0 |
| Manual scenarios | 2 (MT-1, MT-2) | 0 | 0 | 2 |
