# Verification Document: mouse-report-held-callsite-test

## Overview

**Feature**: mouse-report-held-callsite-test
**SPEC.md**: `feature-docs/mouse-report-held-callsite-test/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mouse-report-held-callsite-test/IMPLEMENTATION.md`

This document covers the integrated verification of the feature. Task-level
acceptance criteria live in `tasks/task0001.md`.

## Build Verification

| Component | Command | Expected |
|---|---|---|
| `main` | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| `cli_feature_gate` | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors — proves the new tests, which sit under the GUI-gated `window_host` module, do not leak into the CLI-only build (NFR7) |

Run both from the repository root.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; the six new `window_host::tests` entries below appear
  in the run and pass; no pre-existing test changes its result.
- Coverage target: not applicable — the project has no coverage tooling
  configured, and this feature adds tests only (no production lines to cover).
  The coverage question this feature answers is mutation coverage of two call
  sites, discharged by TS-5 rather than by a line-coverage percentage.
- Known caveat: a red outside `window_host::tests` should be re-run with a
  single test thread before being attributed to this feature — a small number
  of pre-existing tests in this suite are parallelism sensitive. The new tests
  themselves require no such flag (NFR4).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `run_button_decision_passes_live_held_value_to_the_held_aware_apply_entry_point` — extract the button-path body from the embedded source and decompose the held-aware apply call's arguments | Exactly one such call; 4 arguments; the 4th is exactly the three-token held sequence whose leading identifier is one of the function's own parameters | Unit |
| TS-2 | `handle_mouse_wheel_passes_live_held_value_to_the_wheel_report_step` — same for the wheel-path body and the wheel report step call | Exactly one such call; 5 arguments; the 5th is the same three-token held sequence | Unit |
| TS-3 | `button_and_wheel_bodies_never_call_the_held_unaware_apply_entry_point` — whole-token search of both extracted bodies | The held-unaware apply path is absent from both bodies; the held-aware name is demonstrated not to trigger the check | Unit |
| TS-4 | `body_extractor_tolerates_benign_edits_and_ignores_comments_and_string_literals` — in-memory inputs covering comments, literals and benign edits | Benign edits stay accepted; a "correct call" written inside a comment or a string literal does not satisfy the judgment | Unit |
| TS-5 | `argument_scanner_rejects_defaulted_and_deleted_held_arguments` — in-memory rewrites of the **real extracted bodies** for both call sites (IMPLEMENTATION.md D6.1) | For each mutation (defaulted argument, deleted argument, reverted to the held-unaware path, receiver identifier shadowed by a same-named local) the unmutated body passes, the rewrite touched exactly one site, and the mutated body is rejected | Unit |
| TS-6 | `held_argument_scan_is_scoped_to_the_two_named_bodies_only` — the motion path's call against the extracted bodies | The motion-path call is absent from both extracted bodies | Unit |
| TS-M1 | `manual: none required` — change-set inspection; no runtime behaviour changes, no E2E harness exists | The change set is contained in the declared paths and touches no production behaviour | Manual |

## Code Quality Verification

- Format: no format command is configured for either component in
  `workflow.yaml`; follow the repository's Rust formatting settings as the
  surrounding file already does.
- Static analysis: no separate lint command is configured; the two `cargo check`
  runs above are the static gate.
- Inspection items:
  - New test names follow `<subject>_<scenario>_<expected>` and match the TS
    names in this document character for character (NFR3).
  - No test constructs or names a windowing, GPU, PTY or terminal-core runtime
    type (NFR1).
  - No new dependency appears in any manifest (NFR2); the feature's dependency
    licence ledger is empty, so `project.license` (`MIT`) is untouched.
  - Source-scan assertions exist only for the two call connections FR1/FR2/FR3
    name, and for nothing else (NFR5's scoped exception).
  - The names the scan couples to are limited to the closed set in
    `IMPLEMENTATION.md` Conventions, and the host-side identifier is derived
    from the extracted signature rather than hardcoded (NFR8).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The library test run passes including the new tests | Run the Test Verification command; confirm exit 0 and that TS-1 … TS-6 appear in the run |
| AC-2 | A test fails if the button path's 4th argument becomes a default value | TS-5's defaulted-argument input for the button call site is rejected |
| AC-3 | A test fails if that argument is deleted, or the call reverts to the held-unaware path | TS-5's deleted-argument, reverted-call and shadowed-receiver inputs are rejected; TS-3 forbids the held-unaware path in the body |
| AC-4 | A test fails on any of the same three mutations at the wheel call site | TS-5's three wheel-side inputs are rejected; TS-2 and TS-3 cover the file-backed side |
| AC-5 | Benign edits keep the tests green | TS-4's benign-edit inputs stay accepted; TS-6 confirms the scan does not drift into the motion path |
| AC-6 | The scanner excludes correct calls written inside comments and string literals | TS-4's comment-hidden and literal-hidden inputs do not satisfy the judgment |
| AC-7 | The feature's diff is additions to `src-tauri/src/window_host/tests.rs` only, with no production behaviour touched | `git diff --name-only` and `git diff --stat` against the base revision: the only `src-tauri/src/` path listed is `window_host/tests.rs`, and `pointer_routing.rs` / `mouse_report.rs` / `event_loop.rs` are absent |
| AC-8 | Existing tests, including `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`, pass unmodified | `git diff` shows only insertions in `tests.rs` (no modified or deleted lines in existing tests); the named test passes in the Test Verification run |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-M1 |
| FR5 | task0001 | TS-1, TS-2, TS-6 |
| FR6 | task0001 | TS-4 |
| FR7 | task0001 | TS-4, TS-5 |
| FR8 | task0001 | AC-8 (diff inspection plus the named test in the suite run); no dedicated TS in the SPEC's traceability table |
| NFR1 | task0001 | AC-1 plus the Code Quality inspection item on runtime types |
| NFR2 | task0001 | AC-1 plus the Code Quality inspection item on manifests |
| NFR3 | task0001 | Code Quality inspection item on test naming |
| NFR4 | task0001 | AC-1; the suite run needs no single-thread flag for the new tests |
| NFR5 | task0001 | AC-7 plus the Code Quality inspection item on the exception's scope |
| NFR6 | task0001 | AC-1; no `#[ignore]` gate present, run time unchanged in practice |
| NFR7 | task0001 | The `cli_feature_gate` build command |
| NFR8 | task0001 | TS-6 plus the Code Quality inspection item on the closed name set |

## E2E Testing

No E2E framework exists in this project and none is introduced. The feature
changes no runtime behaviour, so there is no end-to-end path to exercise.

## Manual Testing (E2E Not Possible)

- [ ] TS-M1: Change-set verification. Compare against the base revision with
      `git diff --name-only` and `git diff --stat`, and confirm the only
      `src-tauri/` entry is `src-tauri/src/window_host/tests.rs`, that its diff
      contains insertions only, and that no production file appears.
- [ ] Confirm the design step was correctly skipped: this feature renders
      nothing and touches no design token, so no mockup comparison applies.

## Performance / Security Verification

Not applicable. The feature adds compile-time-embedded test scanning with
negligible run cost (NFR6) and introduces no authentication, authorization,
input-validation or data-protection surface.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 7 | 6 | 0 | 1 |
| Success criteria | 8 | 6 | 0 | 2 |
| Code quality inspection | 5 | 0 | 0 | 5 |
