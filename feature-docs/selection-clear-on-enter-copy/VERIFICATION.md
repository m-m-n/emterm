# Verification Document: selection-clear-on-enter-copy

## Overview

**Feature**: selection-clear-on-enter-copy
**SPEC.md**: `feature-docs/selection-clear-on-enter-copy/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/selection-clear-on-enter-copy/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Per-task
completion is governed by the Acceptance Criteria in
`feature-docs/selection-clear-on-enter-copy/tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- Second command (feature-gate check, NFR4):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

Run both from the project root. Do not let cargo fall back to the
workspace-default target directory.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every pre-existing test still passes and none has been
  modified or removed.
- Coverage target: no numeric coverage gate is configured for this project.
  Coverage is asserted instead by the requirement-to-test mapping below — every
  FR and NFR must have at least one verifying scenario.
- Tests must live in the library target; the binary target contains none, so
  scenarios placed there would silently not run.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Truth table of the clear-decision predicate in `window_host/input_translate.rs` | True only when forwarded and the key is the named Enter key; false when forwarded with a non-Enter key; false when not forwarded | Unit (pure) |
| TS2 | The Enter flag across all four `shift_enter_behavior` modes, independent of which form the Shift+Enter rewrite produces | The Enter flag is true in all four modes, so the predicate answers true in all four | Unit (pure) |
| TS3 | The application-state clear helper called with both `selection` and `pending_selection_anchor` set | Both fields end unset | Unit (state transition) |
| TS4 | The same helper called with no selection present | State unchanged; nothing passed to the clipboard side | Unit (state transition) |
| TS5 | Source-text scan of the two call sites: the Enter-conditioned clear inside the forwarded branch of `event_loop.rs`, and the clear immediately after the clipboard write inside the selection-present branch of `key_routing.rs` | Both placements hold as asserted | Unit (source scan) |
| TS6 | Source-text scan that the IME-consume path and a modifier key pressed alone cannot reach the clear site | The early return and the no-bytes translation still stand | Unit (source scan) |
| TS7 | Dirty rows of the frame after the selection goes from set to unset | The rows the old highlight occupied are reported dirty, through the existing union of current and previous selection; no full-redraw flag is introduced | Unit (state transition) |
| TS8 | Source-text scan that the PRIMARY-set path on pointer release and the middle-click paste path are untouched | Both paths unchanged | Unit (source scan) |
| TS9 | Manual, release build: make a selection, then press Enter / Shift+Enter / the copy chord, then type a printable key | The highlight disappears on all three triggers and survives printable-key input | Manual |
| TS10 | The six existing clear conditions (left-button press, tab switch, column-count-changing resize, alt-screen enter/exit, frame reset, scrollback eviction) after the change | Their call sites are unmodified and the full library test suite covering them passes unchanged | Unit (regression) |
| TS11 | The CLI-only feature gate after the change | The no-default-features check exits 0 | Build check |

TS1–TS9 are the SPEC.md scenarios verbatim. TS10 and TS11 are added at
create-plan to close the verification gap on NFR1 and NFR4, which SPEC.md left
without a scenario.

## Code Quality Verification

- Format: no format command is configured for this project
  (`project.components.main.format_command` is empty in `workflow.yaml`), so no
  format gate runs for this feature. Match the surrounding style of each file
  touched; do not reformat files beyond the change.
- Static analysis: none configured. The build verification above is the
  compile-level gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SUC1 | All functional requirements FR1–FR6 are implemented and tested | The Functional Requirements Coverage table below has a non-empty task and verification cell for each |
| SUC2 | All test scenarios TS1–TS9 pass | TS1–TS8 via the test command; TS9 by the manual checklist |
| SUC3 | NFR5 holds — the render-skip optimizations are not regressed | TS7: the clear relies on the existing dirty-row union and adds no redraw request or full-redraw flag |
| SUC4 | SC1 holds — nothing is written to clipboard or PRIMARY content | TS8 plus the review of the diff: the only clipboard interaction is the pre-existing copy write |
| SUC5 | Documentation is complete | REQUIREMENTS.md, SPEC.md, IMPLEMENTATION.md, VERIFICATION.md and task0001.md all present |
| SUC6 | Code review is completed | Review phase reaches zero residual critical/high findings |
| SUC7 | AC1–AC11 of SPEC.md are satisfied | Each maps into TS1–TS9 as listed in SPEC.md's test scenarios |
| SUC8 | NFR4 holds — the CLI-only build still succeeds | TS11 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS5, TS9 |
| FR2 | task0001 | TS1, TS2, TS9 |
| FR3 | task0001 | TS5, TS9 |
| FR4 | task0001 | TS4 |
| FR5 | task0001 | TS3, TS5 |
| FR6 | task0001 | TS7 |
| NFR1 | task0001 | TS10 |
| NFR2 | task0001 | TS6 |
| NFR3 | task0001 | TS8 |
| NFR4 | task0001 | TS11 |
| NFR5 | task0001 | TS7 |
| NFR6 | task0001 | TS1 |

Every requirement has at least one implementing task and at least one verifying
scenario; there is no uncovered requirement.

## E2E Testing

The project has no E2E framework (`test/README.md`: "E2E Tests: None at the
moment") and `project.components.main.e2e_test_command` is empty, so no
automated E2E run applies to this feature. The two call sites are additionally
not reachable from an automated test at all — the Enter site lives inside the
winit event loop and the copy site's handler requires a real window — which is
why their placement is pinned by the source-text scans TS5, TS6 and TS8 and
their end-to-end behaviour is confirmed by the manual scenario below.

## Manual Testing (E2E Not Possible)

Run only on the user's explicit instruction, on a release build.

- [ ] TS9-a: Drag-select some output, then press Enter — the highlight
      disappears in the same frame the view snaps to the live tail.
- [ ] TS9-b: Drag-select, then press Shift+Enter — the highlight disappears.
      Repeat under each `shift_enter_behavior` setting.
- [ ] TS9-c: Drag-select, then press the copy chord (default Ctrl+Shift+C) —
      the highlight disappears and the clipboard holds the selected text.
- [ ] TS9-d: Press the copy chord with nothing selected — nothing happens and
      the clipboard is unchanged.
- [ ] TS9-e: Drag-select, then type printable characters and press cursor keys —
      the highlight survives.
- [ ] TS9-f: With a Japanese IME, drag-select, then commit a composition with
      Enter — the highlight survives the commit.
- [ ] TS9-g: Drag-select, release, then middle-click — the PRIMARY paste still
      works and inserts the previously selected text.

No mockup comparison applies: the design step was skipped because the feature
adds no UI surface, layout, or design token.

## Performance / Security Verification

- NFR5 (no render-skip regression): the diff introduces no redraw request and
  no full-redraw flag; the clear relies on the existing dirty-row union.
  Verified by TS7 and confirmed in review of the diff.
- SC1 (clipboard / PRIMARY): the change writes nothing to the clipboard or to
  PRIMARY beyond the copy that already existed, and does not touch the
  bracketed-paste sanitization path. Verified by TS8 and confirmed in review of
  the diff.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 11 | 10 | 0 | 1 |
| Success criteria | 8 | 5 | 0 | 3 |
| Requirements coverage | 12 | 12 | 0 | 0 |

The three manual success criteria are SUC2 (its TS9 half), SUC5 and SUC6.
