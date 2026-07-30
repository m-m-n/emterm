# Verification Document: Remove the pane-ID copy button from the mux sidebar

## Overview

**Feature**: mux-sidebar-remove-pane-id-copy /
**SPEC.md**: `feature-docs/mux-sidebar-remove-pane-id-copy/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-sidebar-remove-pane-id-copy/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings (specifically no
  unused constant / function / field / parameter warnings)

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: not tracked numerically in this project; every acceptance
  criterion that is test-translatable has at least one test

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Draw the sidebar with an entry whose pane has a known public pane id | No copy affordance is painted and no interaction region other than the row is registered | Unit |
| TS-2 | Click at the row's right edge (the former icon region's center) | The outcome reports that row's window switch | Unit |
| TS-3 | Inspect the sidebar's per-frame outcome value | It exposes only the window-switch result | Unit |
| TS-4 | Evaluate the frame-event "any event fired" predicate over a default value and over each remaining field | False for default, true for each remaining field; no clipboard field exists | Unit |
| TS-5 | Draw rows with and without an agent-status badge, in both placements | Number position, badge position, and name origin are unchanged from before the removal | Unit |
| TS-6 | Draw an empty entry list, and draw at the minimum panel width | No rows and no placeholder; the name ellipsizes against the row's right padding | Unit |
| TS-7 | Run the whole library test suite | Passes; only the copy-affordance tests are absent | Integration |
| TS-8 | Run the build command | Exit code 0 with no new warnings | Integration |
| TS-9 | Inspect the surviving public-pane-id accessor, its tests, and the host clipboard helper | All present; the accessor's tests still pass | Integration |

## Code Quality Verification

- Format: not run as a workflow command — `rustfmt` is not installed on this
  host (SPEC.md Assumption A6). Style follows the surrounding code.
- Static analysis: the build command's warning output is the check
  (`cargo check`).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | No copy affordance renders in a sidebar pane row | TS-1, plus manual check MV-1 |
| SC-2 | No copy hit target exists; the row is one switch target | TS-2, TS-3 |
| SC-3 | The sidebar-originated clipboard channel is gone end to end | TS-3, TS-4, TS-9 |
| SC-4 | No dead code remains from the removal | TS-8 |
| SC-5 | Everything else the sidebar draws is unchanged | TS-5, TS-6, plus manual check MV-2 |
| SC-6 | Unrelated state and helpers are preserved | TS-9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, MV-1 |
| FR2 | task0001 | TS-2, TS-3 |
| FR3 | task0001 | TS-3, TS-4 |
| FR4 | task0001 | TS-8 |
| NFR1 | task0001 | TS-5, TS-6, MV-2 |
| NFR2 | task0001 | TS-7 |
| NFR3 | task0001 | TS-8 |
| NFR4 | task0001 | TS-9 |
| NFR5 | task0001 | TS-10 |

TS-10: run the CLI-only feature check
(`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`)
and confirm exit code 0 — all touched code is behind the `gui` feature, so
this must be unaffected.

## E2E Testing

No E2E framework is wired for the native terminal UI in this project; the
sidebar is exercised through the module's headless unit tests instead.

## Manual Testing (E2E Not Possible)

These require a human looking at a running GUI build and are NOT part of the
automated verify run.

- [ ] MV-1: With an agent running in a mux pane, open the sidebar (both the
      persistent and the overlay variant) and confirm the pane row shows only
      number, badge and name — no icon at the right edge, and no hover
      affordance there.
- [ ] MV-2: Confirm the agent-status badge still appears and the row's
      active / hover appearance is unchanged; clicking near the row's right
      edge switches to that pane.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 10 | 10 | 0 | 0 |
| Success criteria | 6 | 6 | 0 | 2 (MV-1, MV-2 as extra confirmation) |
