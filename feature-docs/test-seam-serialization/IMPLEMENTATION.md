# Implementation Plan: test-seam-serialization

## Overview

Keep every test that touches process-global state through a `#[cfg(test)]`
seam deterministic under the default parallel `cargo test --lib`. The
`RESTART_REQUIRED` serialization (`self_exec::RestartFlagTestGuard`) already
exists at the base and is preserved; the SFTP test seams are routed through
the module's wake helpers, and the module's structural send-site check is
widened to cover them. The feature is planned as a single task (task0001).

## Technology Stack

- **Language**: Rust, edition 2024 — crate `emterm` under `src-tauri/`.
- **Test harness**: Rust built-in test harness at its default (parallel)
  thread count. `--test-threads=1` is never part of an acceptance run.
- **New dependencies**: none (NFR2). `serial_test` is not added, so no new
  license enters the project (`project.license: MIT` is unaffected).

## Layer Structure

No production layer changes (NFR1). The surfaces this feature touches or
depends on:

| Surface | Location | Role in this feature |
|---------|----------|----------------------|
| Restart-flag test seam | `src-tauri/src/self_exec.rs` (`RESTART_FLAG_TEST_LOCK`, `RestartFlagTestGuard`) | Preserved as-is (FR1–FR3); reused by any test FR7 finds |
| SFTP send helpers | `src-tauri/src/sftp/service.rs` (`send_progress`, `send_result`) | The only permitted send path onto the progress / result channels; unchanged |
| SFTP test seams | `src-tauri/src/sftp/service.rs` (`#[cfg(test)] impl SftpService`) | Rerouted through the helpers (FR4) and re-documented (FR5) |
| Structural send-site check | `src-tauri/src/sftp/service.rs` (`mod tests`) | Scan region widened to include the seams (FR6) |
| Seam callers | `src-tauri/src/app/tests/timing.rs` | Unchanged; must stay green (TS-1, TS-3) |

Dependency direction: test code may depend on test seams, send helpers and
the guard. Production code never depends on a `#[cfg(test)]` item.

## Shared Components

Existing components whose contracts this feature relies on. They are
restated here because every later task (including review / verify rework)
must keep them intact.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `self_exec::RestartFlagTestGuard` | Exclusive span over `RESTART_REQUIRED` for one test | Pre: none. Post (acquire): the flag reads `false` and the holder has exclusive access until the guard ends. Post (span end, including an unwinding panic): the flag is `false`. A panic inside one span never prevents a later acquisition. | task0001 |
| `sftp::service::send_progress` / `send_result` | Deliver one event on the progress / result channel and request an event-loop wake | Pre: the sender of the matching channel. Post: the event is enqueued (a send failure is ignored, as today) and a wake is requested; the wake is a no-op in the test binary (SPEC A-2). | task0001 |
| `SftpService::test_push_progress_event` / `test_push_result_event` | Test-only seams that place one synthetic event on a channel | Name, crate visibility and `#[cfg(test)]` gating unchanged. Post: exactly one event carrying the FR4 field values arrives on the matching channel, sent through the matching helper. | task0001 (existing callers: `app/tests/timing.rs`) |

## Conventions

- **Change boundary (NFR1)**: code changes are confined to `#[cfg(test)]`
  items and comments. Production function bodies, signatures and `cfg`
  gating stay byte-identical.
- **Restart-flag rule (FR1)**: a test that raises, clears, consumes or
  observes `RESTART_REQUIRED` — directly, through `restart_pending` /
  `restart_required`, through `App::frame_work_pending` /
  `next_toast_deadline` / `pump_toasts` / `pump_restart_toast`, or through
  any function whose call chain reaches them — holds a
  `RestartFlagTestGuard` from before its first touch until after its last
  observation. Flag restoration is the guard's job; a manual reset call is
  never the mechanism.
- **Send-site rule (FR4, FR6)**: every send onto the SFTP progress / result
  channels in `sftp/service.rs`, test seams included, goes through
  `send_progress` / `send_result`. Only the `mod tests` body is exempt from
  the structural check, because it spells the forbidden shape as data.
- **Formatting (NFR4)**: format only the files this feature modifies. Never
  run crate-wide `cargo fmt`; seven untouched files carry pre-existing
  drift.
- **Command location**: cargo commands run from the worktree root with an
  explicit `CARGO_TARGET_DIR` and `--manifest-path` (project rule
  `core-build-location.md`).

## Cross-task Design Decisions

### D1: One task

The whole change sits in one module's test seams plus a repository audit.
Splitting the FR7 audit into its own task would produce a task whose diff
may be empty, with no parallelism gain. Affected: task0001.

### D2: Serialization stays on the existing Mutex + RAII guard

The guard is already in place at the base (SPEC A-3) and satisfies NFR2.
Any flag-touching test found by the FR7 audit adopts the same guard; no
second serialization mechanism is introduced. Affected: task0001.

### D3: Scan region of the structural check ends at the `mod tests` item

The current check cuts the module text at the first `#[cfg(test)]`
attribute, which is the seam block's own attribute — so the seams sat
outside enforcement. The region boundary becomes the start of the
`mod tests` item, so the seam block and any future `#[cfg(test)]` item
placed before `mod tests` are scanned. A boundary that cannot be located
makes the check fail loudly instead of silently scanning a wrong region.
Affected: task0001.

### D4: The red case is a standing automated test

"A seam reverted to a bare send makes the check fail" (SPEC AC-5) is kept
as a permanent regression test that feeds the check's detection logic a
synthetic module text, in addition to the one-time red observed when the
widened scan first runs against the current bare seams. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The FR7 audit finds a flag-touching test in a file outside task0001's `files` | Unknown at plan time (the planner does not search the tree) | Actual change set exceeds the declared `files` | SPEC.md File Structure pre-declares that such a file joins the change set; the implementer modifies it, reports it as a plan deviation with its path, and the orchestrator reconciles `files` |
| The scan-region boundary matches prose (e.g. a doc comment mentioning `mod tests`) before the real item | Low | Region cut early; seams silently escape the check | Anchor the boundary to the item declaration; the red-case test proves a seam-block violation is detected |
| Known unrelated parallel flakes (e.g. `tabs.rs` replay test, `test/README.md:40`) fail in the full parallel run | Medium | AC-1 reporting ambiguity | Classify per SPEC A-1: rerun and record; never report such a run as an overall pass (NFR5 keeps fixing them out of scope) |
| The crate-wide format command in `workflow.yaml` fails on pre-existing drift | High | False quality failure | Format verification is limited to the modified files (VERIFICATION.md) |
| The forbidden-shape match is a single-line literal; a line-wrapped bare send would evade it | Low | Missed violation | Pre-existing limitation, not widened by this feature; out of scope |

## Open Questions

- [ ] The FR7 audit's resulting file set cannot be predicted at plan time
      (see Risk Assessment, first row).
