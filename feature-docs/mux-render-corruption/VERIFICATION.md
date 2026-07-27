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
| TS-7 (round-8 rework, AC-1/AC-2, review round-7 finding `01f91fe698ceb287`) | Resize sequence longer than `MAX_DIM_MARKERS`, driven through the real ring / snapshot assembly / replay, PARAMETERIZED over eviction counts of one, several, and many (25/28/52 total markers → 1/4/28 evictions). Round 7's version pinned eviction to exactly one, so it could not reach — and its `capped == full` assertion made the mixed-row comparison vacuously true even where it could reach — the shapes where round 7's fix (attribute the whole gap to the LAST evicted entry's dims) actually broke down: measured at 1/3/13 mixed rows against full attribution's 0, worse than "no segments at all" in the worst case. Round 8 stops attributing the gap at all once 2+ entries are evicted (`ScrollbackRingBuffer::read_segments` leaves it unattributed; `TerminalCore::replay_segments` replays it at the caller's target dims instead of dropping it). At every eviction count the test asserts the capped replay is never worse than replaying with no segments at all (the "worse than not fixing" catch), and for exactly one eviction (the case `MAX_DIM_MARKERS`'s own doc guarantees precision for) it additionally asserts exact structural + mixed-row equality with full uncapped attribution. **Residual** (documented in the test, not hidden): for 2+ evictions this implementer's own measurement found a construction where the gap spans phase 0's own two dimension regimes — no single gap-dims choice (neither round 7's "last evicted" nor round 8's "unattributed") eliminates all mixing there (round 8 measured 1/13 mixed rows vs full attribution's 0, matching — not improving on — round 7's own numbers for that construction); the discriminator against reverting round 8 for 2+ evictions is structural (no head segment synthesized), not a universal "no more than full attribution" bound. This is an accepted, measured precision loss mirroring `MAX_DIM_MARKERS`'s own documented trade-off, not a round-8 regression. | Unit |
| TS-8 (round-6, AC-5) | `build_from_snapshot`'s prefix/suffix split for the "ordinary switch" shape (small differing head + bulk tail at target) | Grid/cursor identical to the fully synchronous reference | Unit |
| TS-9 (round-6, AC-6/AC-9; revised round-8, AC-3/AC-6, review round-7 finding `01f91fe698ceb287`) | Malformed segment offsets (non-monotonic / past content length) and an over-budget segment dimension product, PLUS (round 8) a non-zero LEADING offset and the largest segment list the daemon can actually produce. Round 8 flips the non-zero-leading-offset case from rejected to ACCEPTED: it is now the normal shape `ScrollbackRingBuffer::read_segments` produces when 2+ `dim_markers` entries are cap-evicted (TS-7), and `TerminalCore::replay_segments` was fixed in lockstep to replay that leading span at the caller's target dims instead of dropping it — the round-6 rationale for rejecting it ("would make replay_segments silently drop content") no longer holds. The producer-side per-segment clamp is also now derived from the decoder's CUMULATIVE budget (not the per-segment one alone), so the daemon's own largest possible segment list (`MAX_DIM_MARKERS` + head + screen = 26 segments) round-trips cleanly instead of being rejected as `Malformed`. | Decoder rejects non-monotonic / past-content-length offsets and over-budget dimensions as `Malformed`; accepts a non-zero leading offset and the daemon's largest producible segment list | Unit |
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
AC-3 requires.

Round-6 rework (review round-6 finding `004fe3021b5d0c15`): SPEC.md's NFR1
states no numeric threshold ("must not noticeably degrade... latency"),
so the resize-storm rows above (~240-350 ms) are presented here as
measurements against that qualitative bar, not against an invented
"1-second ceiling" — the "noticeably degrade" judgement for this shape is
left to the verify phase / MT-3 rather than resolved here by rewriting the
criterion.

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
