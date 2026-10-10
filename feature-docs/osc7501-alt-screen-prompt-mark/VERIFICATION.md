# Verification Document: osc7501-alt-screen-prompt-mark

## Overview

**Feature**: osc7501-alt-screen-prompt-mark / **SPEC.md**:
`feature-docs/osc7501-alt-screen-prompt-mark/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/osc7501-alt-screen-prompt-mark/IMPLEMENTATION.md`

All commands run from the project root.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, which also compiles `term_core`):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: no coverage tool is configured; every Acceptance Criterion
  of task0001 maps to at least one passing test in
  `test-docs/osc7501-alt-screen-prompt-mark/task0001.tests.yaml`

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Feed ESC `[?1049h`, OSC `133;A`, ESC `[?1049l`, OSC `7501;id=job:state=working`, OSC `133;A` to a plain tab in one `process_combined` call (FR1, FR2) | Record table empty; pending summary changes `[Some(working), None]` | Unit |
| TS-2 | Same sequence with the alternate screen switched by `?1047`, and by `?47` (FR1) | Same result as TS-1 for both | Unit |
| TS-3 | Main-screen A, `?1049h`, alternate-screen A, `?1049l`, working report, main-screen A in one call (FR1, FR2) | Record table empty | Unit |
| TS-4 | `?1049h`, two alternate-screen A marks, `?1049l`, working report, main-screen A in one call (FR1) | Record table empty | Unit |
| TS-5 | Full `src-tauri` `--lib` suite, including the existing osc7501_* and program_status_feed tests and the agent-status latch tests, names unchanged (FR3, NFR1, NFR2) | All pass | Integration |
| TS-6 | Full `term_core` `--lib` suite, including the tests that the screen-aware OSC notification reports the alternate-screen state for `?1049` / `?1047` / `?47` and that callbacks implementing only the existing notification see an unchanged sequence (FR1, FR3) | All pass | Unit |

## Code Quality Verification

- Format: no format command is configured in `workflow.yaml`
- Static analysis: no command is configured in `workflow.yaml`; the build
  checks above must report no errors

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| C-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below: every FR maps to task0001 and to at least one passing scenario |
| C-2 | All test scenarios pass | TS-1 to TS-6 pass |
| C-3 | SPEC AC-1 to AC-5 are satisfied | AC-1 by TS-1; AC-2 by TS-2; AC-3 by TS-3; AC-4 by TS-5 (existing test names unchanged); AC-5 by TS-5 and the first Build Verification command |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-6 |
| FR2 | task0001 | TS-1, TS-3 |
| FR3 | task0001 | TS-5, TS-6 |
| NFR1 | task0001 | TS-5; review check under Performance / Security Verification |
| NFR2 | task0001 | TS-5 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: In a release build, open a plain tab running a shell that emits no
  OSC 133 marks of its own, and print the TS-1 byte sequence in one write. The
  tab's program status does not stay at working afterwards.

## Performance / Security Verification (if applicable)

- NFR1: review the diff — the OSC dispatch path and the 7501 feed path add no
  loop, no allocation, no additional lock acquisition and no I/O; the feed is
  still applied in one pass proportional to its length.
- THREAT-MODEL.md verdict is `no-applicable-threat`; no mitigation item applies.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 6 | 6 | 0 | 0 |
| Success criteria | 3 | 3 | 0 | 0 |
| Manual testing | 1 | 0 | 0 | 1 |
| Performance / Security | 1 | 0 | 0 | 1 |
