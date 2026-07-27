# Verification Document: mux Render Corruption Fix

## Overview

**Feature**: mux-render-corruption /
**SPEC.md**: `feature-docs/mux-render-corruption/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-render-corruption/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only gate, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (web): `bun run build:viewer`
- Expected: exit code 0, no errors

## Test Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command (web): `bun test`
- Command (NFR1 latency benches, round-6 rework; not part of the default
  `--lib` run — release-mode timing, AC-1/AC-13):
  ```sh
  CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    --manifest-path crates/term_core/Cargo.toml --lib \
    -- --nocapture --include-ignored \
    ordinary_switch_bench_950kib_matches_segment_free_cost \
    segment_bounded_replay_bench_950kib
  ```

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Replay of a resize-interleaved apt-style recording into a fixed-size core | No row mixes content from two distinct logical lines | Unit |
| TS-2 | Replay of a resize-interleaved TUI-style (cursor-addressed redraw) recording | No cross-line content mixing | Unit |
| TS-3 | Replay of a resize-free recording | Grid identical to pre-fix behavior | Unit |
| TS-4 | Marker-bearing recording through full pipeline (write filter → snapshot → replay) | Marker bytes never appear as visible cells | Unit |
| TS-5 | Existing Rust `--lib` suite (single-threaded) + `bun test` | All pass | Unit/Integration |
| TS-6 | CLI-only feature check (`--no-default-features`) | Compiles clean | Build |
| TS-7 (round-6, AC-3) | Resize sequence longer than `MAX_DIM_MARKERS`, driven through the real ring / snapshot assembly / replay | Zero cross-line mixing; segment count still bounded | Unit |
| TS-8 (round-6, AC-5) | `build_from_snapshot`'s prefix/suffix split for the "ordinary switch" shape (small differing head + bulk tail at target) | Grid/cursor identical to the fully synchronous reference | Unit |
| TS-9 (round-6, AC-6/AC-9) | Malformed segment offsets (non-zero-leading / non-monotonic / past content length) and an over-budget segment dimension product | Decoder rejects as `Malformed` before replay / allocation | Unit |
| TS-10 (round-6, AC-7) | A malformed structured snapshot frame reaching `Tab::apply_mux_message` | Frame is skipped (logged), current display unchanged — not blanked | Unit |
| TS-11 (round-6, AC-10/AC-11) | Oversize snapshot on the visibility-resume path; partial-drain reattribution in the scrollback write filter | Pane stays Detached (no doomed frame enqueued); retained tail attributed to the read that produced it | Unit |

### NFR1 Measured Latency (round-6 rework, AC-1/AC-2/AC-4/AC-13)

Release build, `ordinary_switch_bench_950kib_matches_segment_free_cost` +
`segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`
(0.95 MiB payload, 100x30 grid, 10 000-line scrollback default — the
round-4/round-5 measurement methodology):

| Shape | Latency |
|-------|---------|
| Segment-free (baseline) | ~8-10 ms |
| Ordinary switch (small differing head + bulk tail at target — the shape a real spawn-size head marker + settled GUI grid produces) | ~1.5-2.2 ms |
| Single segment differing, no stable tail (contrived worst case; the split cannot help) | ~280-300 ms |
| Resize storm, 6 segments (quarter of `MAX_DIM_MARKERS`=24) | ~240 ms |
| Resize storm, 24 segments (at the daemon cap) | ~340-350 ms |

AC-4 is judged on the "ordinary switch" row: it is not measurably slower
than segment-free (well under the bench's 3x + 20ms bound) — the round-5
regression (170-220ms for this exact shape) is closed. The resize-storm
rows are UNCHANGED in kind from round 4/5 (segment count still gates their
cost; the prefix/suffix split does not apply when no stable tail exists) —
their correctness (zero cross-line mixing), not their latency, is what
AC-3 requires, and `MAX_DIM_MARKERS` (raised from 16 to 24) keeps them
comfortably under the 1-second NFR1 ceiling.

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Root cause identified; relationship to known coordinate-drift bug documented | task0001 completion report (AC-6) names the verdict and reproducing test |
| SC-2 | Regression tests added and passing | TS-1..TS-4 exist and pass |
| SC-3 | No regression in existing tests | TS-5 |
| SC-4 | User performs final on-device verification | MT-1 / MT-2 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | SC-1 (investigation verdict with test evidence) |
| FR2 | task0001, task0006 | TS-1, TS-2, TS-7 |
| FR3 | task0001, task0006 | TS-1, TS-2, TS-3, TS-4, TS-7, TS-9, TS-10, TS-11 |
| NFR1 | task0001, task0006 | MT-2 (manual latency feel check) + NFR1 Measured Latency table above (automated) |
| NFR2 | task0001 | TS-6 |
| NFR3 | task0001 | TS-5 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: On-device — run Claude Code in mux, repeat window/tab switches
      (including detach → attach); no line-content mixing appears
- [ ] MT-2: On-device — window switch / reattach latency feels unchanged
      from before the fix
- [ ] MT-3 (task0005 rework, review round-4 finding `6c650908ea8e95e9`):
      On-device — drag a mux window's edge to resize it repeatedly (a
      continuous drag, not discrete resizes), producing dozens of grid-size
      changes in quick succession against a pane with substantial
      scrollback (e.g. a long-running `seq`/`glances`/log-tailing pane),
      then immediately switch away to another window and back. The switch
      completes without a multi-second stall and the restored content is
      correct (no cross-phase-mixed rows).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Tests | 11 (TS-1..TS-11) | 11 | 0 | 0 |
| Manual | 3 (MT-1, MT-2, MT-3) | 0 | 0 | 3 |
