# Verification Document: focus-loss-drag-termination

## Overview

**Feature**: focus-loss-drag-termination
**SPEC.md**: `feature-docs/focus-loss-drag-termination/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/focus-loss-drag-termination/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/focus-loss-drag-termination/tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings attributable to this feature
- Additional build check required by NFR4 / TS-6 (CLI-only feature gate):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0. This command is NOT one of the four commands declared
  in workflow.yaml `project.components.main`; it comes from the SPEC (NFR4) and
  from the project's own CLI-only check rule, and may need a separate approval
  before it can run.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0, every test passing. Single-threaded because some
  existing replay tests in this crate are non-deterministic under parallelism.
- Coverage target: no coverage tool is configured in this project, so no
  percentage threshold applies. The coverage criterion for this feature is
  scenario-based instead: TS-1 through TS-5 each have at least one passing test,
  and no previously passing test regresses.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Truth table of the new pure termination predicate over every combination of (drag in flight, left held) | Returns "terminate" only for in-flight AND not-held; bare unit test, no window constructed | Unit |
| TS-2 | The structural source-text assertion over the event-loop module pins the new composed form | Needle set replaced by literals of the rewritten arm covering the predicate call and the held-button read, each unsatisfiable by the pre-change composition; the fold-click negative assertion still holds | Unit (structural) |
| TS-3 | A no-owner left release terminates a drag preserved across a focus steal | With no recorded owner and drag-in-flight true, the release takes the local completion arm; with drag-in-flight false it stays a no-op. Existing test passes unchanged | Unit |
| TS-4 | The three focus-loss state shapes driven through the extracted predicate | In-flight and held → preserve; in-flight and not held → terminate; not in flight → preserve regardless of the held value | Unit |
| TS-5 | The clear routine still empties both the gesture-ownership and the held-button records | Both existing clear-routine tests pass unchanged, and a decision made from post-clear records reports nothing and takes no local arm | Unit |
| TS-6 | CLI-only feature gate intact | The no-default-features build check exits 0 | Build check |
| TS-7 | Manual reproduction of the focus steal against the release binary | Held-button case: the selection keeps extending after focus returns and PRIMARY is unchanged until release. Button-up case: behaviour is as before the change | Manual |

## Code Quality Verification

- Format: `make fmt-check` — expected exit code 0
- Static analysis: no separate lint command is declared in workflow.yaml
  `project.components.main`; the compiler's own diagnostics from the two build
  checks above are the static-analysis gate for this feature.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements FR1-FR8 are implemented and tested | Functional Requirements Coverage table below; FR6 is documentation-only and was satisfied at create-spec |
| SC-2 | All non-functional requirements NFR1-NFR6 are satisfied | Coverage table below plus the build, test and format commands |
| SC-3 | All acceptance criteria AC-1..AC-10 of the SPEC are satisfied | task0001's acceptance criteria discharge them; AC-8 is discharged by SPEC.md's Supersession section |
| SC-4 | All test scenarios TS-1..TS-7 pass | Test run, build checks and the manual scenario below |
| SC-5 | The supersession of the prior feature's FR3 / AC-3 / state-table row is recorded in prose with refreshed line references | Read SPEC.md's Supersession section; no code change verifies this |
| SC-6 | The CLI-only build still compiles | TS-6 build check |
| SC-7 | The decision layer remains window-free and the new predicate remains pure | Inspect every new or changed signature in the pointer-routing and decision layers for windowing, GPU, PTY or terminal-mode types; TS-1 passing without a window is the mechanical half |
| SC-8 | Documentation is complete | SPEC.md, IMPLEMENTATION.md, this document and the task plan are present and mutually consistent |
| SC-9 | Code review is completed | The review phase's record for this feature |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-4 (predicate decision for both in-flight shapes), TS-2 (arm calls it in the gated form), TS-7 (manual) |
| FR2 | task0001 | TS-4, TS-2 (the held-button read is pinned as a source literal ahead of the clear routine) |
| FR3 | task0001 | TS-4 (preserve on held), TS-3 (the preserved drag's terminator), TS-7 (manual) |
| FR4 | task0001 | TS-1 (truth table from a bare unit test, no window) |
| FR5 | task0001 | TS-2 |
| FR6 | — (no implementing task) | SC-5: satisfied at create-spec by SPEC.md's Supersession section; the prior feature's document is outside this feature's declared change set. Recorded as a coverage gap, not planned work |
| FR7 | task0001 | TS-5, plus the full test run showing no mouse-report byte, target-tab or wheel-matrix change |
| FR8 | task0001 | TS-3 (the release-path fallback is the single terminator), plus review that no second terminator was added |
| NFR1 | task0001 | TS-1 (drivable with no window) and signature inspection per SC-7 |
| NFR2 | task0001 | TS-1 plus review that the predicate mutates nothing and performs no I/O |
| NFR3 | task0001 | TS-1 (bare unit test in the inline module, subject-scenario-expected naming, no new framework) and `make fmt-check` |
| NFR4 | task0001 | TS-6 |
| NFR5 | task0001 | TS-5 and the full test run (no new PTY bytes, wheel-report notch clamp untouched) |
| NFR6 | task0001 | No scenario: a design-level statement (one boolean read per focus-loss event). Verified by review of the diff — recorded as a coverage gap |

## Manual Testing (E2E Not Possible)

No E2E automation exists in this project (SPEC assumption A6), and the
focus-loss arm cannot be driven end to end from the test layer because it needs
a real window. The end-to-end confirmation is therefore user-executed against a
release build.

- [ ] TS-7a: Start a left drag over the grid and keep the button down. Cause the
      window to lose focus (raise another window, or let terminal output open a
      child viewer window). Return focus and keep moving the pointer with the
      button still down — the selection must still be extending, and PRIMARY must
      be unchanged until the button is released. On release, the full selection
      is published.
- [ ] TS-7b: Start and finish a left drag normally, then cause focus loss with
      the button up — behaviour is exactly as before the change.
- [ ] TS-7c: Cause focus loss with no drag in progress — nothing is published and
      the current selection state is untouched.

## Performance / Security Verification

- NFR6 (performance): the diff adds exactly one boolean read per focus-loss
  event and no per-frame or per-motion work. Verified by reading the diff — no
  benchmark is run and none exists in this project.
- NFR5 (security): no additional bytes are emitted to the PTY, the wheel-report
  notch clamp and its maximum-notch constant are untouched, and no network
  surface, persisted data or WebView content is involved. Verified by the full
  test run plus review of the diff's scope.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (default check, CLI-only check) | 2 | 0 | 0 |
| Test scenarios | 7 (TS-1..TS-7) | 6 (TS-1..TS-6) | 0 | 1 (TS-7) |
| Code quality | 1 (format check) | 1 | 0 | 0 |
| Success criteria | 9 (SC-1..SC-9) | 5 | 0 | 4 (SC-3, SC-5, SC-7, SC-8 include document or diff review) |
| Requirements coverage | 14 (FR1-FR8, NFR1-NFR6) | 12 with at least one scenario | 0 | 2 gaps (FR6, NFR6) verified by review |
