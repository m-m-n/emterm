# Implementation Plan: inherited-pty-drop-fd-tiocgptn

## Overview
Replace the `st_rdev`-based closure check in the `mux::inherited_pty` test
module with a pts-number check (the TIOCGPTN query), add a deterministic
recurrence test for that check, and correct the comments and the cross-file
reference that stated the wrong premise. The feature is one task, task0001.

## Technology Stack
- **Language**: Rust, existing `src-tauri` crate; test code of the
  `mux::inherited_pty` module only.
- **Key libraries** (all already in use; no new dependency):
  - `libc` — the TIOCGPTN query and descriptor duplication; the same test
    module already issues TIOCGPTN.
  - `portable-pty` — opening real PTY pairs in tests (existing test-helper
    pattern).
  - `tempfile` — the ordinary-file case (already used by the same test
    module).
- **New dependencies and their licenses**: none (`project.license`: MIT).

## Layer Structure
Not applicable. No production layer is touched: the change lives in the test
section of one module, plus one doc comment in the `mux::upgrade` test file.

## Shared Components
None. The feature has a single task; the test helpers it introduces are
private to the `mux::inherited_pty` test module and have no consumer in any
other task.

## Conventions
Feature-wide constraints. They also bind any later rework task of this
feature.

- Scope: test-only code (the module's `cfg(test)` section) and doc comments.
  `InheritedMasterPty` and its `Drop` stay unchanged (NFR1).
- The `mux::inherited_pty` module keeps its unix-only gating (NFR4).
- Determinism: no added or changed test depends on CPU load, repeated
  execution or sleep-based timing, and no implementation or verification step
  repeats a run to show the flake is gone (NFR2).
- The test name `dropping_the_adapter_closes_the_owned_descriptor` stays
  (NFR3), so no predecessor feature's test-docs record needs a rename update
  (SPEC A2).
- Predecessor feature documents, including its investigation record, are not
  edited (SPEC A4).

## Cross-task Design Decisions

### D1: Single-task decomposition
- **Decision**: task0001 owns every change of the feature.
- **Rationale**: the recurrence test must exercise the same closure judgment
  the target test uses, and the `upgrade/tests.rs` comment fix depends on
  whether `stat_rdev` survives. Both live in one test module; splitting them
  would need a cross-task contract for a private test helper.
- **Affected tasks**: task0001.

### D2: Identity of a PTY master descriptor
- **Decision**: whether a descriptor number still refers to the original PTY
  master is decided only by the pts number the TIOCGPTN query reports,
  compared with the number recorded before the drop. `st_rdev` is not used
  for this decision anywhere in the feature (FR1).
- **Affected tasks**: task0001.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The TIOCGPTN query succeeds on a PTY slave on the build host's kernel, contradicting FR3's slave case | Low | The recurrence test fails at the slave case | task0001 keeps the case as specified; the implementer reports a plan deviation instead of weakening it |
| The `st_rdev` red check cannot be re-run after merge, because the final code keeps no `st_rdev` judgment | Medium | TS-3 cannot be re-executed at verify | The red run is recorded in the task's test record during TDD; verify checks that record (VERIFICATION.md TS-3) |
| A single-test run with `--exact` is not an approved command string in batch mode | Medium | The TS-1 run is blocked | VERIFICATION.md accepts the target test's result line from the single full `--lib` run (TS-6) |
| An unrelated race still makes the target test fail | Low | The flake persists | Not checked by repetition (NFR2); the failure message carries the fd number and both pts outcomes (FR2) for diagnosis |

## Open Questions
- None.
