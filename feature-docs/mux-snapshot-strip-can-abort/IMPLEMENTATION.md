# Implementation Plan: mux-snapshot-strip-can-abort

## Overview
The shared mux strip pass ends a Kitty APC / DCS body at the first ESC in the body. `ESC \` completes it. ESC followed by any other byte aborts it, and an aborted Kitty APC / SIXEL DCS body is removed up to the aborting ESC. The feature is one task (task0001). This document holds only the feature-wide decisions and conventions around it.

## Technology Stack
- **Language**: Rust, crate `src-tauri`. The strip module is part of the always-built CLI + mux code, outside the `gui` feature gate.
- **Test oracle**: the workspace crate term_core. The existing strip and write-filter tests already use its replay views and the end-state oracle (`client_written_state`).
- **New dependencies**: none. `project.license` is MIT and nothing is added, so there is no license record to make.

## Layer Structure
- `src-tauri/src/mux/scrollback_filter.rs`: the shared strip module (snapshot path and write path). It never depends on the IPC layer. That rule already exists and does not change.
- `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`: the scrollback write filter (IPC layer). It calls the strip module. Its boundary scan, cut closures and closing constants keep their behavior (NFR2). Only its doc comments change.
- Allowed dependency direction: IPC layer to strip module, never the reverse. The strip keeps its own body end scan and does not reuse the write filter's string scan.

## Shared Components
None across tasks: the feature has a single task. The strip pass that both the write path and the snapshot path use stays one shared pass (FR6). Its contract is in tasks/task0001.md.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | - | - | - |

## Conventions
- Cargo runs from the project root (the worktree root inside a task worktree), with the quick-check target directory and the manifest path, as `.claude/rules/core-build-location.md` states. Unit tests live under `--lib`. VERIFICATION.md lists the exact commands.
- The formatter is never run over the whole crate. Only files in the task's file set are formatted.
- Existing test names are kept. This feature changes expectations, not names. `.claude/rules/test-docs-records.md` applies only to renames, so no predecessor `test-docs/*/taskNNNN.tests.yaml` record changes.
- A test that checks term_core replay or the end-state oracle uses payloads that neither answer nor place an image (the Kitty `a=d` payload and the SIXEL `q#0;2;0;0;0` payload, the predecessor convention).

## Cross-task Design Decisions

### D1: One task
Decision: the strip change, its tests and the comment updates are one task.
Rationale: every new or updated test (the strip-level tables, the write-filter round trip, the closure-test update, the escape-closure test update) passes only with the strip change in place. A split would leave a test-only task red in its own worktree, because tasks run in parallel with no ordering. The acceptance list therefore runs past the usual size, and the plan accepts that.
Affected tasks: task0001.

### D2: The change reaches the write path as well (SPEC A-2)
Decision: the write path and the snapshot path stay the single shared strip pass, so live write-path output for an aborted Kitty APC / SIXEL DCS body changes too.
Rationale: FR6 requires identical output. The write filter's written-state tracking follows the bytes the strip writes, so the identity and state-agreement tests pin consistency.
Affected tasks: task0001.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Existing tests beyond FR7's two named tests pin the old aborted-body behavior | Medium | Low | One known case is in task0001's scope: part (d) of `escape_carry_the_escape_closure_has_no_effect_in_term_core`. Any other case is updated to FR1-FR4 under its current name and reported as a plan deviation. |
| The predecessor's doc-comment contract test fails after the comment rewrite | Medium | Low | task0001 keeps that test's required phrases and avoids its stale phrases (task plan, AC-9) |
| Wall-clock budget tests fail under machine load | Medium | Low | New timed checks reuse bounds already used in the same test files. A budget failure is re-run alone before it counts as a regression. |
| `tabs.rs` replay tests are nondeterministic when run in parallel | Medium | Low | Not touched by this feature. They are re-run with a single test thread before a failure counts. |

## Open Questions
- None blocking. FR7 names two tests that state the old behavior, and planning found a third, part (d) of the escape-closure test in `escape_state_carry.rs`. The plan includes it under FR7's intent.
