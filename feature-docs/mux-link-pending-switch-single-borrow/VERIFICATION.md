# Verification Document: mux-link Pending-Switch Single Borrow

## Overview

**Feature**: mux-link-pending-switch-single-borrow
**SPEC.md**: `feature-docs/mux-link-pending-switch-single-borrow/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-link-pending-switch-single-borrow/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `tasks/task0001.md`.

The change is behaviour-preserving by requirement, so most of the evidence is
compile-time: the four gates below (one test run, three check configurations)
are the acceptance gates fixed at create-spec (assumption A1) and all four are
blocking. Every command runs from the project root with an explicit target
directory; cargo is never run from inside the crate directory and is never
allowed to fall back to the workspace-default target directory.

## Build Verification

| Component | Command | Expected |
|---|---|---|
| main (GUI, default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit 0, no new warning |
| cli-only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit 0, no new warning |
| windows-cross | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests` | exit 0, no new warning |

"No new warning" means: no warning that was not already emitted for this crate
before the change. A pre-existing warning elsewhere in the crate is not a
failure of this feature; a warning naming the edited function is.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit 0, no test newly failing.
- Coverage target: none set. The project defines no coverage tooling, this
  feature adds no new code path, and no new test is written (assumption A2):
  the two existing regression tests below are the behavioural safety net.
- Known flake: the replay tests in this suite can fail non-deterministically
  under parallel execution. A failure that does not reproduce when the suite is
  re-run single-threaded is the known flake, not a regression; a failure that
  persists single-threaded is a regression.

### Test Scenarios from SPEC.md

SPEC.md numbers its scenarios TS1 / TS2 / TS3; they appear here as TS-1 / TS-2 /
TS-3 in the same order. TS-4 and TS-5 are added by this document to give the
maintainability and safety requirements an explicit verification item.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `ts3_live_output_queued_during_pending_switch` (existing regression, `src-tauri/src/tabs/tests/replay.rs`) — under-cap queueing while a switch is pending | Two payloads for the target pane are queued in arrival order; a payload for a different pane is dropped; no redraw is owed | Unit (existing) |
| TS-2 | `offthread_live_queue_cap_falls_back_to_sync` (existing regression, `src-tauri/src/tabs/tests/replay.rs`) — the overflow path end to end | Four large payloads stay pending; the fifth exceeds the cap; the pending switch is abandoned, the snapshot is reparsed synchronously, the queued live output is applied on top, and a redraw is owed | Unit (existing) |
| TS-3 | Outcome match is exhaustive with exactly two arms and no unreachable optional arm | All three check configurations compile the edited call site; a locally added third outcome variant breaks compilation at this site (throwaway experiment, reverted) | Compile-time (no test file changes) |
| TS-4 | Diff-scope and safety inspection of the integrated diff | The diff touches only `src-tauri/src/tabs/mux_link.rs`; no `unsafe` is added; no new clone of the payload is introduced; the live-queue entry point, its must-use return and the outcome enum are unmodified | Static (diff review) |
| TS-5 | Doc-comment inspection at the edited call site | Only the sentences describing the removed arm are rewritten; neighbouring commentary and its own requirement numbering are unchanged | Static (diff review) |

## Code Quality Verification

- Format: no format command is configured for this project
  (`workflow.yaml` `project.components.*.format_command` is empty) and the
  crate is not formatted crate-wide, so no formatting command is run. The edited
  block matches the surrounding file's existing style.
- Static analysis: no separate linter is configured. The three check
  configurations above are the static gate; their warning output is the signal.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Exactly one access to the pending-switch field in the block, through the mutable borrow accessor, with the pane id latched as a copied integer | TS-4 diff review plus TS-3 compilation (a second look-up would also make the borrow discipline visible in review) |
| AC2 | Two-armed outcome match; no optional-`None` arm, no wildcard | TS-3 |
| AC3 | The `--lib` test run passes, including both named regression tests | TS-1, TS-2 |
| AC4 | The default-features check and the `--no-default-features` check both succeed with no new warning | Build Verification rows 1 and 2 |
| AC5 | The Windows cross-check succeeds with no new warning | Build Verification row 3 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-3, TS-4 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-1, TS-3 |
| FR4 | task0001 | TS-2, TS-3 |
| FR5 | task0001 | TS-1, TS-2 |
| NFR1 | task0001 | TS-4 |
| NFR2 | task0001 | TS-2 |
| NFR3 | task0001 | TS-4 |
| NFR4 | task0001 | TS-5 |
| NFR5 | task0001 | TS-3 |

Every requirement has at least one implementing task and at least one
verification item; there is no uncovered requirement.

## E2E Testing

No E2E framework applies to this feature: the project's E2E commands are empty
in `workflow.yaml`, and the change has no user-facing surface. Omitted
deliberately, not by oversight.

## Manual Testing (E2E Not Possible)

The design step is `skipped` for this feature (no UI, no CSS, no design token),
so there is no mockup comparison item. The single item below is optional — the
automated gates already cover the requirements — and is listed only as a
real-device sanity check of the path that was restructured.

- [ ] MT-1 (optional): In a running mux session, switch panes while the
      previously focused pane is producing output, and confirm that output is
      neither lost nor duplicated after the switch completes. Evidence, if the
      run looks wrong, is `emterm.log` — never DevTools, which this app does not
      expose.

## Performance / Security Verification

- Performance: not applicable. Observable behaviour is unchanged and no
  performance target is set by SPEC.md.
- Security: memory safety only. No new `unsafe`, no new payload clone, no
  ownership restructuring (NFR3) — verified by TS-4. No authentication,
  authorization, input validation or injection surface is involved.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 5 (TS-1 … TS-5) | 3 (TS-1, TS-2, TS-3) | 0 | 2 static diff reviews (TS-4, TS-5) |
| Build / check gates | 3 | 3 | 0 | 0 |
| Success criteria | 5 (AC1 … AC5) | 4 | 0 | 1 (AC1's diff-review half) |
| Optional sanity check | 1 (MT-1) | 0 | 0 | 1 |
