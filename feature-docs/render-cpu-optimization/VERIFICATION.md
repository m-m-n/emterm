# Verification Document: GUI Render CPU Optimization

## Overview

**Feature**: render-cpu-optimization /
**SPEC.md**: `feature-docs/render-cpu-optimization/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/render-cpu-optimization/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Note: `tabs.rs` replay tests are non-deterministic in parallel; re-run with
  `-- --test-threads=1` if they flake.
- Coverage target: every SPEC acceptance-relevant CPU-side behavior covered
  by a unit test (no numeric % target; wgpu-device paths excluded by design).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Dirty-set properties: unchanged frame / cursor move / blink on-off / PTY output | empty set on unchanged frame; old+new rows on move; cursor row only on phase flips (blink on); output rows only | Unit |
| TS-2 | Cursor overlay geometry: normal / wide / emoji / empty cell; scrollback; fold rows; screen edges | correct rects; suppressed when scrolled back; no broken rect | Unit |
| TS-3 | Draw-skip decision pure function | Some(0) dirty AND status bar unchanged → skip; other combinations → draw | Unit |
| TS-4 | Row-cache equivalence under mutations (writes, scroll, selection, resize) | cache-built instance sequence identical to full rebuild | Unit |
| TS-5 | Invalidation trigger matrix (selection / hover / search / scroll / resize / font-theme / focus) | each single mutation invalidates expected rows; equivalence holds next frame | Unit |
| TS-6 | Persistent-buffer growth policy | monotone capacity, always sufficient, geometric growth | Unit |
| TS-7 | `EMTERM_RENDER_PERF=1` counters | frames-drawn and rows-rebuilt logged at warn level; no side effects when unset | Unit + manual log check |
| TS-8 | Idle / output performance measurement | idle 10 s frame count ≈ blink flips; rebuilt rows ≈ output lines during `seq` flood; idle wakeups ≪ 155/s (blink cadence at most, none with blink off); before/after CPU% reduced (`/proc` sampling, report §6) | Manual (procedure below) |
| TS-9 | Visual pass: cursor 3 shapes × blink on/off; cursor over CJK/emoji; selection / search / hover; TUI (vim, Claude Code) ghosting; mux tab switch | no visual regressions | Manual |
| TS-10 | Known limitation check | TUI row-rewrite highlight residue not worsened vs. before | Manual |
| TS-11 | CLI-only build gate | `--no-default-features` check compiles | Automated (build) |

## Code Quality Verification

- Format: per-file formatting via the session PostToolUse hook (no crate-wide
  `cargo fmt` — project convention).
- Static analysis: none beyond `cargo check` (project has no clippy gate).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR6 implemented and unit-tested | TS-1 … TS-7 pass |
| SC-2 | CPU reduction confirmed (idle + output) | TS-8 before/after sampling |
| SC-3 | CLI build intact | TS-11 |
| SC-4 | No visual regressions | TS-9, TS-10 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-1, TS-3 |
| FR2 | task0001 | TS-2, TS-9 |
| FR3 | task0003 | TS-4, TS-5 |
| FR4 | task0003 | TS-6 |
| FR5 | task0004 | TS-8 |
| FR6 | task0002, task0003 | TS-7 |
| NFR1 | task0002, task0003, task0004 | TS-8 |
| NFR2 | task0001 | TS-9, TS-10 |
| NFR3 | (plan phase — `research/benchmark-validation.md`) | recorded findings reviewed |
| NFR4 | (constraint on all tasks) | TS-11 |

## E2E Testing

No automated E2E framework in this project; the scenarios below are manual.

## Manual Testing (E2E Not Possible)

Performed at final verify against a release-style run of the binary:

- [ ] TS-8 measurement procedure (from investigation report §6):
  1. Launch emterm, note PID.
  2. Idle CPU: sample `/proc/<PID>/stat` utime+stime over 5 s twice; compare
     with pre-fix baseline (10–15%).
  3. Idle wakeups: `grep voluntary_ctxt /proc/<PID>/status` deltas — expect
     far below the pre-fix ~155/s (blink cadence at most; near zero with
     blink disabled).
  4. Output CPU: run a `seq` flood; sample again; compare with ~80% baseline.
  5. `EMTERM_RENDER_PERF=1` run: idle 10 s → frames ≈ blink flips; flood →
     rebuilt rows on the order of output lines.
- [ ] TS-9 visual matrix: cursor {block, underline, bar} × blink {on, off};
  cursor on CJK and emoji cells; selection, search highlight, link hover;
  vim and Claude Code sessions for ghosting; mux tab switch redraw.
- [ ] TS-10: reproduce the known TUI highlight-residue scenario; confirm not
  worsened.
- [ ] IME: composition preedit still displays and commits correctly (wakeup
  path sanity for FR5).

## Performance / Security Verification

- Performance: TS-8 (pass criterion = reduction vs. pre-fix baseline; no
  absolute target per REQUIREMENTS §7).
- Security: no new inputs, no new privilege surface — not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit scenarios | TS-1…TS-7 | 7 | 0 | log check in TS-7 |
| Performance | TS-8 | 0 | 0 | 1 |
| Visual | TS-9, TS-10 | 0 | 0 | 2 |
| Build gate | TS-11 | 1 | 0 | 0 |
