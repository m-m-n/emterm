# Implementation Plan: mux-upgrade-exited-pane-revision

## Overview

The mux hot-upgrade refresh re-reads an exited pane's agent state, agent name,
revision and OSC 7501 records together, under one agent-status lock
acquisition, and carries them to the successor process. The feature is a
single task (task0001): the fix, its regression tests, the comment updates and
the predecessor test-record updates. The fix design lives in
`tasks/task0001.md`.

## Technology Stack

- **Language / Framework**: Rust, existing `src-tauri` crate (emterm package).
  The new tests are `--lib` unit tests in the existing upgrade test module.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Role in this feature | Dependency direction |
|-------|----------|----------------------|----------------------|
| Upgrade module | `src-tauri/src/mux/upgrade.rs` | Builds, refreshes and restores the handoff document; the fix is here | Reads pane state from the session layer |
| Session pane state | `src-tauri/src/mux/session/pane/` | Owns each pane's agent status behind a per-pane lock | Read-only for this feature; never depends on the upgrade module |
| Daemon | `src-tauri/src/mux/daemon/` | Calls the refresh before exec; resyncs agent status to clients after a snapshot | Read-only for this feature, apart from one comment at the refresh call site |

## Shared Components

None. The feature has one task, so no task builds a component that another
task uses.

## Conventions

- Code comments and test names are in English and follow the module's
  existing style. Test names describe the behavior they pin.
- When a test is renamed, the test records in `test-docs/` follow
  `.claude/rules/test-docs-records.md`.
- No format command is configured (`format_command` is empty). Formatting
  changes stay inside the task's files.

## Cross-task Design Decisions

None. The feature has one task.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Changing how the refresh reads agent status also changes the live-pane path | Low | Medium (FR3 regression) | The existing live-pane and not-in-manager refresh tests must pass with their bodies unmodified (task0001 AC-5) |
| Predecessor test records still list the old test name after the rename | Medium | Low (records drift from the tests) | task0001 AC-7 requires the record update and the test-listing check |

## Open Questions

None.
