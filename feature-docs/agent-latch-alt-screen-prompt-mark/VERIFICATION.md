# Verification Document: agent-latch-alt-screen-prompt-mark

## Overview
**Feature**: agent-latch-alt-screen-prompt-mark / **SPEC.md**: `feature-docs/agent-latch-alt-screen-prompt-mark/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/agent-latch-alt-screen-prompt-mark/IMPLEMENTATION.md`

## Build Verification
- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only feature set): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both
- Run from the integration worktree root (no `cd`).

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: every test passes. Known non-deterministic failures under parallel
  load (tabs replay tests, tmux_sockets discover tests) are judged against the
  task record's `baseline_failures`; any failure outside that set fails
  verification.
- Coverage target: not measured by the project; every scenario below maps to
  named tests.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Plain tab, one `process_combined` call: enter alternate screen (`?1049`), alt OSC 133 D, alt OSC 133 A, leave, OSC 777 Set, main OSC 133 D, main OSC 133 A | Pending latch inputs [Set, Mark(CommandEnd), Mark(PromptStart)]; applying them with the Set event leaves the tab's status cleared | Unit (pipeline end-to-end) |
| TS-2 | TS-1 with the switch done through `?1047`, and through `?47` | Same as TS-1 | Unit (pipeline end-to-end) |
| TS-3 | One call: Set, main D, `?1049h`, alt A, `?1049l`, Clear, main A | [Set, Mark(CommandEnd), Clear, Mark(PromptStart)] | Unit (pipeline end-to-end) |
| TS-4 | One call: Set, main D, `?1049h`, alt A, `?1049l`, Set, main A | [Set, Mark(CommandEnd), Set, Mark(PromptStart)] | Unit (pipeline end-to-end) |
| TS-5 | Callback level: OSC 133 A / B / C / D through the screen-aware notification with alternate screen active, with main screen active, and through the screen-less notification | Alternate screen: no latch-feed candidate; main screen and screen-less: exactly one candidate of that kind | Unit |
| TS-6 | Same call with main A before alternate-screen marks; several alternate-screen marks before the main marks | Only main-screen marks reach the latch inputs, in order | Unit (pipeline end-to-end) |
| TS-7 | Full `--lib` suite, including existing latch / osc7501 / program_status_feed / callback tests | All pass | Integration |

## Code Quality Verification
- Format: no format command declared (`project.components.src-tauri.format_command` is empty).
- Static analysis: the two `cargo check` runs under Build Verification.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | AC-1 bytes in one call give [Set, Mark(CommandEnd), Mark(PromptStart)] and clear the tab's status | TS-1 |
| AC-2 | `?1047` / `?47` give the AC-1 result | TS-2 |
| AC-3 | Clear in between gives [Set, Mark(CommandEnd), Clear, Mark(PromptStart)] | TS-3 |
| AC-4 | Second Set in between gives [Set, Mark(CommandEnd), Set, Mark(PromptStart)] | TS-4 |
| AC-5 | Existing latch / osc7501 / program_status_feed / screen-aware callback tests pass; the renamed test's record is updated per `.claude/rules/test-docs-records.md` | TS-7; inspect `test-docs/osc7501-alt-screen-prompt-mark/task0001.tests.yaml` AC-3 (new name, supersede note naming this feature's SPEC FR1, `red_reason` unchanged) and confirm each listed name appears in the `--lib` run output |
| AC-6 | `--lib` tests, default `cargo check` and `--no-default-features` `cargo check` pass | Build Verification; TS-7 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6 |
| FR2 | task0001 | TS-1, TS-3, TS-4, TS-6 |
| FR3 | task0001 | TS-5, TS-7 |
| NFR1 | task0001 | TS-7; Performance / Security Verification below |
| NFR2 | task0001 | TS-7 (mux latch-feed discard tests, osc7501 tests, screen-aware callback tests for non-OSC-133 actions) |

## Manual Testing (E2E Not Possible)
- None. The reproduction from the goal (alternate-screen A, leave, report,
  main-screen A in one output unit) runs through the real parse path in TS-1
  and TS-2.

## Performance / Security Verification (if applicable)
- NFR1: reconciling one pump's latch feed is a single pass over the feed, with
  no blocking and no I/O — TS-7 passes, and the integrated diff of
  `reconcile_latch_feed` and the OSC 133 callback branch adds no loop over
  earlier entries, no lock wait and no I/O.
- Security: THREAT-MODEL.md verdict is `no-applicable-threat`; no TM-n items.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 7 | 7 | 0 | 0 |
| Success criteria | 6 | 6 | 0 | 0 |
| Requirements coverage | 5 | 5 | 0 | 0 |
| Performance / Security | 1 | 1 | 0 | 0 |
