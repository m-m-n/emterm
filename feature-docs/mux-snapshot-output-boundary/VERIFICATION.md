# Verification Document: mux-snapshot-output-boundary

## Overview
**Feature**: mux-snapshot-output-boundary / **SPEC.md**: `feature-docs/mux-snapshot-output-boundary/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-snapshot-output-boundary/IMPLEMENTATION.md`

Run every command from the repository root (the integration worktree root during verify). Do not `cd` into `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors (CLI + mux build, NFR5)

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every test named for TS-1 to TS-12 below exists and passes.
- Coverage target: not measured (the project has no coverage tooling). Instead, each TS-n below must map to at least one named test.
- Known flakiness unrelated to this feature: the `tabs.rs` replay tests can fail nondeterministically when run in parallel, and the `tmux_sockets` discover test fails rarely under parallel runs. If one of these alone fails, re-run it with a single test thread before treating the run as failed. New tests added by this feature must pass under the default parallel runner.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Visible reattach. A Detached pane's reader is paused right after the shadow and ring update for a chunk containing an insert-line sequence. Visible reattach data is collected, then the reader resumes. | The new destination never receives that chunk as `PtyOutput`. Feeding the snapshot plus every later delivered byte into `term_core` equals feeding the raw stream once. The test fails on the pre-change code. | Integration (in-crate) |
| TS-2 | Visibility resume. A `Detached{HiddenByVisibility}` pane is paused the same way. `resume_pane_with_permit` runs, then the reader resumes. | The chunk does not appear on the channel after the snapshot. `term_core` reference equality holds. The test fails on the pre-change code. | Integration (in-crate) |
| TS-3 | On-demand snapshot. A Connected pane is paused the same way. The on-demand snapshot build and enqueue run exactly as the handler does, then the reader resumes. | The channel order is [snapshot], never [snapshot, same chunk]. `term_core` reference equality holds. The test fails on the pre-change code. | Integration (in-crate) |
| TS-4 | Capture consistency. A snapshot is attempted while the reader sits between its shadow update and its ring write. Separately, a resize runs concurrently with snapshots. | The captured ring and shadow dump correspond to the same last sequence number; no chunk is only in the shadow. The ring's latest dimension marker equals the captured shadow parser's dimensions. | Integration (in-crate) |
| TS-5 | Backpressure. A small-capacity channel is filled. The reader is parked (a) waiting for a slot, and (b) between observing Full and starting to wait. An on-demand snapshot is taken, then the channel is drained. | The waiting chunk never arrives after the snapshot. | Integration (in-crate) |
| TS-6 | No over-suppression. New chunks and EOF follow a snapshot. Separately, a snapshot is rejected for size, and a deferred snapshot is evicted. | Every post-boundary chunk and the EOF empty chunk are delivered. With no snapshot delivered, the in-flight chunk is delivered too. | Integration (in-crate) |
| TS-7 | OSC 9 in a suppressed chunk (TS-3 conditions). The OSC 9 is complete in one case and split across the suppressed chunk and the next chunk in another. A later Detached period follows. | Complete case: exactly one notification reaches the notification channel, and the destination does not receive the chunk. Split case: no double fire. A later Detached output does not produce a stitched notification. | Integration (in-crate) |
| TS-8 | Side effects of a suppressed chunk (TS-3 conditions). The chunk contains an OSC 2 title, an OSC 777 agent-status report, an OSC 133 mark and an OSC 7 cwd. | Title sender, agent-status sender and pane cwd show the same results as for an unsuppressed chunk. | Integration (in-crate) |
| TS-9 | Terminal queries in a suppressed chunk (TS-1 and TS-3 conditions). The chunk contains a cursor-position query and a primary device-attributes query. The alternate-screen color-query variant is also covered. | On the destination channel, each query arrives exactly once, after the snapshot and before the next reader chunk, in the original order. Queries remaining in the snapshot bytes are not re-delivered. | Integration (in-crate) |
| TS-10 | Boundary-spanning sequences (TS-1 conditions). The suppressed chunk ends with (a) a cut CSI, (b) a cut UTF-8 character, or (c) the start of a rich-content candidate the write filter holds as pending. The continuation is in the next chunk. | `term_core` fed with the snapshot plus later delivered bytes equals the raw-stream reference. No continuation bytes and no replacement character are displayed. | Integration (in-crate) |
| TS-11 | Sender binding. A boundary is recorded for destination A. Destination B takes the pane over via reattach. | Forwarding to B is never suppressed by A's boundary, and no chunk above any boundary is missing on any destination. | Integration (in-crate) |
| TS-12 | Lock order. The reader streams continuously while resize and all four snapshot paths run repeatedly from other threads and tasks. | Finishes within the test timeout (no deadlock). The existing EOF G1 regression test still passes. | Integration (in-crate, stress) |
| TS-13 | Snapshot byte shape. The existing byte-shape tests in `reattach/tests.rs`, `pane/tests.rs`, and the apt / ring-wrap tests in `pty_spawn/tests.rs` run without modification. | All pass. `git diff` of those tests shows only added tests, with no edits to existing assertions or inputs. | Existing tests |
| TS-14 | Build configurations. The test command and the `--no-default-features` check both run. The diff is reviewed for platform-specific APIs. | Both commands exit 0. No platform-specific API is added. | Automated + review |
| TS-15 | FR7 comments. Review the comments named in FR7 and any other doc comment that states the old ordering. | None contradicts the new guarantee. | Review |
| TS-16 | Lock-order and lock-scope audit. | Every acquisition follows `output_target` → capture → {ring, shadow} or `output_target` → boundary. Capture and boundary are never held together. Nothing waits for channel capacity under these locks. No assembly, encoding or probe replay is added inside an `output_target` section. | Review |
| TS-17 | Reader normal-path audit. | For an unsuppressed chunk, the added work is one sequence increment, one comparison and uncontended exclusion acquisitions. Query extraction and tail handling run only on suppressed chunks. | Review |

## Code Quality Verification
- Format: no project-wide format command (`format_command` is empty). Do not run a crate-wide format; the rustfmt hook formats edited files. The diff must not contain formatting-only changes to files outside the feature's `files`.
- Static analysis: the `--no-default-features` check above.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage table below; every TS-n has a passing test or a completed review item |
| SC-2 | All test scenarios pass | Test Verification command, TS-1 to TS-12 |
| SC-3 | NFR1 to NFR5 are met | TS-13, TS-14, TS-16, TS-17, and TS-12 |
| SC-4 | Security requirements are met | Performance / Security Verification below |
| SC-5 | FR7 comments do not contradict the new guarantee | TS-15 |
| SC-6 | Code review completed | review phase status in workflow.yaml |

SPEC acceptance criteria map to these scenarios:

| SPEC AC | Scenarios |
|---------|-----------|
| AC-1 | TS-1, TS-2, TS-3 |
| AC-2 | TS-4 |
| AC-3 | TS-5 |
| AC-4 | TS-6 |
| AC-5 | TS-7 |
| AC-6 | TS-13 |
| AC-7 | TS-14 |
| AC-8 | TS-15 |
| AC-9 | TS-8 |
| AC-10 | TS-9 |
| AC-11 | TS-10 |
| AC-12 | TS-11 |
| AC-13 | TS-12 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-12 |
| FR3 | task0001 | TS-1, TS-2, TS-3 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-6 |
| FR6 | task0001 | TS-7 |
| FR7 | task0001 | TS-15 |
| FR8 | task0001 | TS-8 |
| FR9 | task0001 | TS-9 |
| FR10 | task0001 | TS-10 |
| FR11 | task0001 | TS-5, TS-11 |
| NFR1 | task0001 | TS-13 |
| NFR2 | task0001 | TS-12, TS-16 |
| NFR3 | task0001 | TS-16 |
| NFR4 | task0001 | TS-17 |
| NFR5 | task0001 | TS-14 |

## E2E Testing
Not applicable: the project has no E2E suite for the mux daemon (`resolved_input_paths.e2e` is empty).

## Manual Testing (E2E Not Possible)
These need a release build and a restarted mux daemon; they are judged by eye.
- [ ] MT-1: Keep a pane producing continuous output that uses insert-line and cursor-movement sequences (for example a full-screen TUI or a package-manager progress display). Switch tabs and windows repeatedly. No transient corruption appears after a switch: no duplicated lines, no misplaced cursor.
- [ ] MT-2: Detach and re-attach while the pane keeps producing output. The restored screen matches the continuing output with no duplicated region.
- [ ] MT-3: Minimize and restore the GUI window (visibility resume) while output continues. No duplicated region appears after the restore.
- [ ] MT-4 (optional): Repeat MT-1 on Windows.

## Performance / Security Verification (if applicable)
- NFR3: TS-16. No work is added inside `output_target` sections on tokio workers beyond the O(1) boundary record and the capture exclusion around reads already done there.
- NFR4: TS-17. The reader's unsuppressed-chunk cost is limited to a counter and a comparison.
- Security (SPEC): the authorization check in `handle_request_pane_snapshot` is unchanged. The existing handler authorization tests pass unmodified, and a diff review confirms it.
- Security (SPEC): replacement bytes (FR9, FR10) go only to the destination that received the snapshot. Covered by TS-9 and TS-11, which assert the payload appears only on that destination's channel.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Concurrency tests (TS-1 to TS-5, TS-12) | 6 | 6 | 0 | 0 |
| Suppression scope and side effects (TS-6 to TS-11) | 6 | 6 | 0 | 0 |
| Regression and configuration (TS-13, TS-14) | 2 | 2 | 0 | 0 |
| Review audits (TS-15 to TS-17) | 3 | 0 | 0 | 3 |
| Manual runtime checks (MT-1 to MT-4) | 4 | 0 | 0 | 4 |
