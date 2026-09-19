# Verification Document: mouse-report-guard-local-arms

## Overview

**Feature**: mouse-report-guard-local-arms /
**SPEC.md**: `feature-docs/mouse-report-guard-local-arms/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mouse-report-guard-local-arms/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `tasks/task0001.md`.

## Build Verification

- Command (component `main`):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (component `cli_only`):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors. The `cli_only` check confirms the feature
  gates still compile; this feature touches only modules behind the GUI feature,
  so the CLI-only build must be unaffected.
- Run every command from the project root. Do not let cargo fall back to the
  workspace-default target directory.

## Test Verification

- Command (component `main`):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0. Every pre-existing assertion in the changed module's
  inline test module still passes, with the guarded-region sweep's strengthening
  (TS9) as the single intended change (NFR6).
- Coverage target: the project defines no numeric coverage threshold. Coverage is
  expressed as scenario coverage — TS1 through TS11 all present and passing.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Wheel × region matrix, tracking inactive, main screen (seven region inputs × both wheel directions) | Title-bar band, bottom strip, scrollbar overlay and resize hot zone each decide the scroll-scrollback local arm; tab-bar band, mux sidebar and profile-selector-visible each decide "nothing" | Unit |
| TS2 | The same matrix repeated with each tracking mode active and both encodings | Dispositions identical to TS1, and no bytes are appended for any cell | Unit |
| TS3 | Alternate screen with the alternate-scroll mode bit and setting both on, tracking inactive, over the four arm-bearing regions | The translate-to-arrow-bytes local arm; turning any one of the three conditions off returns the scroll-scrollback arm; shift changes nothing | Unit |
| TS4 | Middle press × region matrix with middle-click paste enabled | Paste-primary local arm for bottom strip, scrollbar overlay, mux sidebar and resize hot zone; "nothing" for title-bar band, tab-bar band and profile-selector-visible | Unit |
| TS5 | The same matrix with middle-click paste disabled | "Nothing" in every region | Unit |
| TS6 | Left press × region matrix, then the matching left release; right press × region matrix | Every press cell decides "nothing" with no gesture owner recorded; the release names no arm and appends no bytes, never reaching the selection-completion arm; every right-press cell decides "nothing" | Unit |
| TS7 | Motion × region matrix | "Nothing" with the default record-update bundle; a following grid-owned motion at the previously cached cell still reports | Unit |
| TS8 | Grid-ownership predicate truth table over the decomposed inputs | False when the title-bar flag alone is true, false when the tab-bar flag alone is true, and unchanged for every other single-region and multi-region combination | Unit |
| TS9 | Strengthening of the existing guarded-region sweep (region × event kind × button identity) | Each cell's decided disposition matches an expected table, in addition to the existing byte-absence assertion; reverting any arm to "nothing" fails the suite | Unit |
| TS10 | Overlap precedence | Resize hot zone with bottom strip → the local arm; mux sidebar with bottom strip → "nothing"; profile selector with any arm-bearing region → "nothing"; tab-bar band with resize hot zone → "nothing" | Unit |
| TS11 | Tab bar hidden (effective tab-bar height zero) | The tab-bar band is empty: a position just below the title-bar height is grid-owned, a position above it is the title-bar band and still decides the scroll-scrollback arm | Unit |

All scenarios live in the inline test module beside the decision units in
`src-tauri/src/window_host/mouse_report.rs`, run from bare unit tests with no
window, GPU surface or PTY constructed (NFR1).

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — read-only
  check; expected to report no diff for the changed files.
- Static analysis: the project defines no separate lint command in
  `workflow.yaml`; the compiler warnings surfaced by the build commands above
  serve as the static-analysis gate (expected: no new warnings in the changed
  files).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-A | All functional requirements FR1-FR10 are implemented and tested | The coverage table below; every FR maps to at least one passing scenario |
| SC-B | All automated scenarios TS1-TS11 pass | The test command above exits zero with all eleven scenarios present |
| SC-C | All manual scenarios MS1-MS4 are executed against a release build | The manual section below, each item checked off with its observed result |
| SC-D | The decision units remain pure | No windowing type, terminal-core type, window handle, GPU surface or PTY appears in any decision-unit signature, and every new test runs from a bare unit test — confirmed by source inspection in the review phase and by the tests compiling without any such construction |
| SC-E | No disposition variant and no local-arm variant is added | Source inspection of the two enumerations against the base revision |
| SC-F | The early-return order in the pointer-button and wheel handlers is unchanged and no guard is added or removed | Diff inspection of `pointer_routing.rs` in the review phase: the only permitted change is the region-flag population |
| SC-G | Every currently-green assertion in the inline test module still passes, with TS9's strengthening the single intended change | The full unit suite exits zero, and the diff of the test module shows no other pre-existing assertion weakened or deleted |
| SC-H | Documentation is complete | REQUIREMENTS.md, SPEC.md, IMPLEMENTATION.md, `tasks/task0001.md` and this document are present and mutually consistent |
| SC-I | Code review is completed | The review phase records no residual critical or high finding |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2, TS3, TS10, TS11, MS1 |
| FR2 | task0001 | TS1, TS10, MS3 |
| FR3 | task0001 | TS4, TS5, TS9, MS2 |
| FR4 | task0001 | TS6, TS9, MS4 |
| FR5 | task0001 | TS6 |
| FR6 | task0001 | TS7, TS9 |
| FR7 | task0001 | TS2, TS9 |
| FR8 | task0001 | TS7 |
| FR9 | task0001 | TS8, TS11 |
| FR10 | task0001 | TS1, TS4, TS10 |
| NFR1 | task0001 | TS1, TS4, TS7 — each runs from a bare unit test with nothing windowed constructed; additionally SC-D's signature inspection |
| NFR2 | task0001 | TS1, TS4, TS9 — the expected tables are expressible with the existing decision values only; additionally SC-E |
| NFR3 | task0001 | MS1, MS3, MS4 — the upstream guards still consume their own regions; additionally SC-F's diff inspection |
| NFR4 | task0001 | No automated scenario. Verified structurally by code inspection in the review phase (no allocation, no lock acquisition, no log line added per pointer event) — see Performance Verification below |
| NFR5 | task0001 | TS2, MS1 |
| NFR6 | task0001 | TS9 plus the full unit suite exiting zero; SC-G |

## E2E Testing

The project has no E2E harness (`test/README.md`: "E2E Tests: None at the
moment.") and none is introduced by this feature. `workflow.yaml` declares an
empty E2E command for both components. End-to-end coverage is the user-driven
manual section below.

## Manual Testing (E2E Not Possible)

Rationale for the split: the decision units are pure and fully unit-testable, but
the wheel and pointer-button handlers take mutable host and application state that
a test cannot construct. The wiring from a real pointer event through the early
guards to the decision call is therefore confirmable only by hand. Run all four
against a release build.

- [ ] MS1 (AC1-AC4, FR1, NFR3, NFR5) — with the scrollback filled, turn the wheel
      over the bottom status bar, over the right-edge scrollbar overlay, over the
      CSD title bar, and over each of the four resize bands. The scrollback moves
      in all seven places. Repeat with a mouse-reporting application running
      (tracking active): same result.
- [ ] MS2 (AC8, FR3) — with middle-click paste on and the primary selection holding
      known text, middle-click on the status bar. The text is pasted.
- [ ] MS3 (AC6, AC7, FR2, NFR3) — the wheel over the tab bar still scrolls the tab
      strip horizontally; the wheel over the mux sidebar (persistent and overlay)
      still scrolls the window list; with the profile selector open, the wheel
      scrolls the modal list only. In all three, the terminal scrollback does not
      move.
- [ ] MS4 (AC11, FR4, NFR3) — a left-press drag starting on the status bar or on
      the scrollbar overlay starts no terminal selection; a left press on a resize
      band still resizes the window.

The design step is skipped for this feature (no UI element, layout, design token
or visual surface is touched), so no mockup visual comparison applies.

## Performance / Security Verification

- **NFR4 (performance)**: no allocation, no lock acquisition and no log line is
  added per pointer event. Checked by inspection of the changed decision branches
  in the review phase — the region dispatch must read only booleans already
  present in the input record, and the wheel answer must come from the existing
  wheel-consumer helper rather than a re-derived matrix. No load or stress test is
  specified.
- **Security**: the resolved requirements contain no security requirement for this
  feature. The change adds no input channel and stores no data; it only changes
  which local arm the decision units name for a guard-rejected position, and FR7
  keeps the reporting prohibition intact.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 11 | 11 | 0 | 0 |
| Code quality | 1 | 1 | 0 | 0 |
| Manual scenarios | 4 | 0 | 0 | 4 |
| Success criteria | 9 | 3 | 0 | 6 |
| Requirements coverage | 16 | 15 | 0 | 1 |
