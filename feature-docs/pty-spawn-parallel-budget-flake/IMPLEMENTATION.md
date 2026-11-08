# Implementation Plan: pty-spawn-parallel-budget-flake

## Overview

Make the verdict of five `mux::ipc::pty_spawn` tests independent of machine load:
three budget tests judge their production calls by thread CPU time through the
existing `CpuMeter`, two reattach tests arm the second P2 stop before the first
release, and a source-scan guard detects a return to either old form. Only test
code under `src-tauri/src/mux/ipc/pty_spawn/` (`tests.rs` and `tests/`) changes.

## Technology Stack

- **Language / Framework**: Rust, libtest (`cargo test --lib`), existing tokio test
  harness of the reattach tests.
- **Key internal components**: `CpuMeter` and `BudgetVerdict` in
  `src-tauri/src/mux/ipc/pty_spawn/tests/thread_cpu_time.rs` (existing, unchanged);
  the shared `BUDGET` (10 s) in `post_strip_cut_csi.rs` (existing, unchanged).
- **New dependencies**: none. No license record is needed.

## Layer Structure

All three layers are test code; production code is not touched (NFR3).

1. **Target tests** — the five tests named in SPEC.md FR1-FR5, in their existing
   files. They depend on the measurement layer (FR1-FR3) or on the existing P2
   pause hook of the output capture (FR4/FR5).
2. **Measurement layer** — the existing `CpuMeter` module. Used as is; not edited.
3. **Guard** — a new test module that reads the source text of the target test
   files, embedded at compile time, and judges it. It calls no target test and no
   production code. Nothing depends on the guard.

Allowed dependency directions: target tests → measurement layer; guard → source
text of the target test files only.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `CpuMeter` (existing, unchanged) | Accumulates the thread CPU time of measured calls and judges the total | Pre: created and used on one thread (it panics when measuring on another thread). Post: measuring a call runs it exactly once, adds the calling thread's CPU time spent in it to the total and returns its result unchanged; the total reads the accumulated time; judging against a budget yields a verdict that is within exactly when the total is strictly below the budget, and whose text shows both values | task0001 |
| `BUDGET` (existing, unchanged) | The shared 10 s budget of the budget tests | Imported where needed, never redefined or changed | task0001 |
| Guard token contract | The literal forms the guard requires or forbids in the five target test bodies | See Conventions C2. The target test bodies and the guard are written against the same list | task0001 |

## Conventions

### C1. Measurement convention (FR1-FR3, NFR1)

- Each converted test creates one `CpuMeter` in its own body and judges it once at
  the end of the body against `BUDGET`.
- A measured region is one call whose work is production write-filter or overflow
  calls plus the collection of their output. Building inputs, building expected
  values, assertions and term_core oracle replays stay outside every measured
  region (SPEC.md A4).
- Each converted test asserts that the verdict is within the budget and that the
  measured total is greater than zero. Assertion messages show the verdict (total
  and budget).
- No new wall-clock budget is added anywhere (NFR1).

### C2. Guard token contract (FR6)

- A budget test body (FR1-FR3) contains `CpuMeter::new`, `.measure(` and
  `.judge(BUDGET)`, and contains neither `Instant::now` nor `.elapsed()`.
- An arm-order test body (FR4/FR5) contains at least two `p2.arm()`, and its first
  `release_tx.send(` comes after its second `p2.arm()`.
- The guard ignores whole-line `//` comments inside a body: a comment may describe
  the order or the old form in words without affecting the verdict.
- A body is found by the exact text `fn` + space + the test name + `(`, and ends at
  the end of that function; text of any other function is never part of it.

### C3. No intentional CPU load (NFR2)

Implementation and verification use single ordinary test runs only: no repeated
runs, no run loops, no stress tools, no core-count parallel runs, and no attempt to
reproduce the parallel failure under load.

## Cross-task Design Decisions

The feature has one task. The decisions below are recorded here because SPEC.md A7
delegates the measuring form to create-plan, and because they bind several test
files and the guard together.

### D1. One task

- Decision: FR1-FR6 are one task (task0001).
- Rationale: the guard's real-source check passes only once all five target tests
  are converted. Tasks run fully in parallel with no ordering, so a separate guard
  task would stay red in its own worktree until the others merged. The whole change
  fits one implementer session.
- Affected tasks: task0001.

### D2. Measured-region granularity: one helper call (SPEC.md A7)

- Decision: FR1 and FR2 measure each call of their existing helpers (`feed_pieces`,
  `run_pieces` with the oracle check off as today, `emitted_through`) from the test
  body; the helpers' signatures and their other callers stay as they are. FR3's
  shared helper `check_a_flushed_open_body_is_closed_at_the_next_removal` contains
  the oracle replay, so it gains one caller-supplied runner through which it
  executes its two production calls (the overflow setup and the next-call feed);
  the DCS / APC test supplies a runner that measures, and the OSC sibling supplies a
  runner that executes the call directly, keeping its own wall-clock budget and its
  behavior.
- Rationale: `.measure(` then appears in each test body, where the guard can see
  it; the many small production calls of byte-at-a-time feeding are measured as one
  region, so the total stays well above the coarse thread-time tick of Windows and
  the clock reads add little; the oracle replay of FR3 stays unmeasured.
- Affected tasks: task0001.

### D3. Guard input: source embedded at compile time

- Decision: the guard reads the four target files as text embedded at compile time,
  with no runtime file access and no path resolution. CR LF and LF line ends are
  judged alike.
- Rationale: identical behavior on Linux and Windows (NFR4), including a checkout
  with CR LF line ends.
- Affected tasks: task0001.

### D4. Arm placement in the reattach tests

- Decision: the second `output_capture.p2.arm()` moves to immediately before the
  first `release_tx.send`, after the first reattach (FR4) or the first on-demand
  snapshot request (FR5), while the reader is still paused at the first P2. This is
  the pattern of `tests.rs:1893-1894`. The 5 s `recv_timeout` values stay.
- Rationale: arming while the reader is blocked in `PauseHook::hit` affects only the
  next hit (SPEC.md A5); nothing else in the sequence moves.
- Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| On Windows the thread CPU time advances in coarse ticks, so very short measured regions can each read zero and the "total > 0" assertion could fail | Low | Medium | D2: regions are whole helper calls over inputs of hundreds of KiB, far above one tick |
| Another `--lib` test that is sensitive to parallel runs fails in the single ordinary run | Medium | Low | VERIFICATION.md judges AC-4 by the six named tests and records other failures by name; no rerun (C3) |
| The guard breaks on harmless formatting changes | Low | Low | C2 uses tokens that formatting keeps intact, finds bodies by exact name, bounds them to one function and ignores comment lines |
| The reattach tests still use a 5 s wall-clock `recv_timeout` | Low | Low | Kept by SPEC.md FR4/FR5; with the arm before the release the reader always stops at the second P2, so the timeout only covers the reader's progress over one chunk |

## Open Questions

- None.
