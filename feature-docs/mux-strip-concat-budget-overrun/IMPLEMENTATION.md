# Implementation Plan: mux-strip-concat-budget-overrun

## Overview
Bring `strip_concat_alternations_and_chains_finish_within_the_budget` (the target
test) within its unchanged 10 s thread CPU-time budget on a debug build. A
measured cost breakdown (FR1) decides whether the test's own workload shrinks or
a non-linear production path is fixed (FR2). A scaling-ratio guard on the real
strip and write-filter paths detects a later non-linear regression (FR3).

## Technology Stack
- **Language**: Rust, the `src-tauri` crate's `--lib` unit tests under
  `mux::ipc::pty_spawn::tests`.
- **Measurement**: the existing thread CPU-time meter `CpuMeter`
  (`src-tauri/src/mux/ipc/pty_spawn/tests/thread_cpu_time.rs`), on the Linux
  per-thread clock and the Windows thread-times query.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure
| Layer | Responsibility | Members |
|---|---|---|
| Test | Builds inputs and expected outputs, drives the measured calls, asserts the outputs, judges the measurements | The target test (`strip_concat_query.rs`); the new scaling-ratio test module |
| Measurement | Thread CPU-time accounting and its judgments | `CpuMeter` and its budget verdict (existing); the scaling-ratio judgment (new) |
| Production (measured) | Strips PTY output before it reaches the scrollback ring | The strip pass and its entries (`scrollback_filter.rs`); `ScrollbackWriteFilter` (`write_filter.rs`) |

Allowed dependency directions: Test → Measurement, Test → Production.
Measurement depends on nothing this feature adds. Production never depends on
test code.

## Shared Components
| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| `CpuMeter` (existing, unchanged) | Accumulates the thread CPU time of the calls passed through it | Measuring a call runs it on the owner thread, adds the CPU time the calling thread spent in it and hands back its result unchanged; work outside measured calls is not counted; the total is judged against a budget as within exactly when it is strictly below it; a call from another thread panics | task0001, task0002 |
| Scaling-ratio judgment (new, next to `CpuMeter`, built by task0002) | Judges two thread CPU times measured at input sizes N and 4N | Pre: the smaller time is the measurement at N, the larger the measurement at 4N, both on the same thread. Post: linear exactly when the smaller time is above zero and larger / smaller is strictly below 8; a ratio of exactly 8, a ratio above 8 and a zero smaller time are not linear. The verdict exposes both times and the ratio for a message. No clock reading, no platform-specific code | task0002 (implements and calls it). task0001 applies the same rule (D1) to its FR1 numbers by hand and does not call it |
| `TARGETS` / `Target` (existing, `strip_concat_query.rs`, unchanged) | The 8 strip targets: name, bytes, re-emitted C0 bytes, held flag | Read-only; the definitions stay as they are (A7) | task0001, task0002 |
| `SCROLLBACK_FILTER_PENDING_CAP` (existing, `write_filter.rs`, 512 KiB, unchanged) | The write filter's pending cap; a one-call input longer than it takes the overflow flush | Read-only | task0001, task0002 |
| Measured production entries (existing): `strip_pty_output_for_scrollback_write`, `strip_replayable_rich_content`, `ScrollbackWriteFilter::feed` | Strip PTY output for the ring and the snapshot | Signatures unchanged; output byte-identical for every input (NFR3); internals change only in task0001's non-linear branch (D2) | task0001 (measures; may fix), task0002 (measures) |

## Conventions
- **Measurement**: every budget and every ratio is thread CPU time measured
  through `CpuMeter` on the test thread. Inputs, expected outputs and
  comparisons are built outside the measured calls. No wall-clock reading and
  no wall-clock budget is added (NFR1).
- **No intentional CPU load (NFR2)**: investigation and verification run
  individual tests, or the `mux::ipc::pty_spawn::tests` module, with
  `--test-threads=1`. No load loop, no parallel load run, no stress tool, and
  no repeated run beyond one run per measured size.
- **Commands**: cargo runs from the project root with
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`;
  the unit tests are selected through `--lib`.
- **Frozen shared definitions**: `TARGETS`, `Target`, `BUDGET` (10 s, also used
  by three other tests of `strip_concat_query.rs`) and the existing behavior of
  `CpuMeter` are not changed (A7, A9).
- **Test names**: no existing test is renamed, so no predecessor test-docs
  record needs a rename update.
- **Placement**: a new test helper lives in the test tree next to `CpuMeter`
  (A9).
- **Comments**: doc comments name the feature, the task and the FR IDs, as the
  neighbouring test modules do. They hold no absolute path and no URL.

## Cross-task Design Decisions

### D1: One linearity criterion for FR1 and FR3
**Decision**: a measured path is linear when its thread CPU time at 4N divided
by its thread CPU time at N is strictly below 8, the geometric mean of the
linear ratio 4 and the quadratic ratio 16. A ratio of 8 or more is non-linear.
Each smaller measurement is sized to about 100 ms or more of thread CPU time on
a debug build, so the Windows clock resolution does not dominate the ratio.

**Rationale**: SPEC FR3 and A6 fix the threshold and the sizing. Using the same
rule for FR1 makes the FR2 branch and the regression guard agree on what
linear means.

**Affected tasks**: task0001 (the FR1 verdict, from its own measurements),
task0002 (the judgment component, its tests and the real-path guard).

### D2: The measured production entries keep their contract
**Decision**: no task changes the signature or the output bytes of the measured
production entries. Only task0001, and only when its FR1 verdict is
non-linear, changes their internals. task0002 tests them as they are on its
base: when its real-path ratio reaches 8 there, it reports that as a failure
instead of changing production code, the threshold or the input.

**Rationale**: the tasks run in parallel; the guard must exercise the real
paths without depending on whether task0001's change has merged.

**Affected tasks**: task0001, task0002.

### D3: File ownership
**Decision**: no file belongs to both tasks. task0001 owns
`strip_concat_query.rs` and, in its non-linear branch only, `scrollback_filter.rs`
and `write_filter.rs`. task0002 owns `thread_cpu_time.rs`, the new
scaling-ratio test module and that module's registration in
`pty_spawn/tests.rs`. task0002 imports `TARGETS` / `Target` from
`strip_concat_query.rs` read-only.

**Rationale**: keeps the parallel merges conflict-free.

**Affected tasks**: task0001, task0002.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| FR1 finds a non-linear path, so task0001 changes the strip or the write filter at the PTY-output trust boundary | Low (every body, terminator and boundary scan in these paths stops at the next ESC) | High | THREAT-MODEL.md TM-1: byte-identical output; every existing strip and write-filter test unchanged and passing |
| Non-linear branch: after the fix, the unchanged workload still exceeds 5 s | Low | Medium | task0001 stops and reports the conflict between FR2's non-linear branch and SPEC AC-2 instead of shrinking the workload; re-planning decides |
| task0002's guard runs against production code that is still non-linear | Low | Medium | D2: task0002 reports it; the guard is re-run after task0001's fix merges |
| Shrinking the workload drops the overflow-flush coverage | Medium | Medium | Every one-call input is sized above the cap and the test asserts it (task0001 AC-4) |
| The Windows clock resolution (about 15.6 ms) or the machine's speed distorts a ratio | Low | Medium | D1 sizing; the margin between the linear ratio 4 and the threshold 8 |
| The 5 s target is a development-machine reading, not an asserted bound | Medium | Low | The test asserts the unchanged 10 s `BUDGET`; the post-change total is recorded in the FR1 record and re-read through TS-1 |

## Open Questions
- [ ] SPEC AC-4 asks for the FR1 record in a feature document. Implementers never
      write under `feature-docs/`, and the planner runs no measurement, so the
      FR1 record is kept in the target test's doc comment (task0001) and TS-6
      reviews it there.
- [ ] SPEC AC-2's 5 s is not asserted by any test. It is read from the target
      test's verdict line (TS-1) and from the post-change total in the FR1
      record.
