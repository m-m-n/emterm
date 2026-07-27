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

- Command (emterm lib): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command (term_core lib — round-9 rework, D6'''''', review round-8 finding
  `ba7953c458050780`: this feature added 21 `#[test]`s directly to
  `crates/term_core/src/terminal_core.rs`, none of which the `emterm lib`
  command above ever runs, since `term_core` is a separate workspace
  package): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Command (web): `bun test`
- Command (NFR1 latency benches, round-6 rework, cap raised round-9,
  hard latency assertion restored round-10 (D3, AC-6) — not part of the
  default `--lib` run — release-mode timing, AC-1/AC-3):
  ```sh
  CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    --manifest-path crates/term_core/Cargo.toml --lib \
    -- --nocapture --include-ignored \
    ordinary_switch_bench_950kib_matches_segment_free_cost \
    segment_bounded_replay_bench_950kib
  ```
  `segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`
  now asserts (not merely reports) that the storm-path replay at the
  daemon cap stays within a stated small multiple of the segment-free
  baseline — see the NFR1 Measured Latency section below.

### Known pre-existing failures (out of scope, AC-9)

Running the `emterm lib` command above single-threaded produces **7
failures**, confirmed to be the exact same 7 named tests present on base
commit `a8d4bed2df5e2f41f524bd0c1ff59fb07f76f025` (this task's own
starting point) — they are not caused by, and not fixed by, this task:

- `tabs::tests::ts7_offthread_swap_then_restored_scrollback_matches_reference`
- `tabs::tests::ts8_new_offthread_switch_supersedes_in_flight_restore`
- `tabs::tests::ts9_concurrent_live_drain_trims_rebuilt_tail_no_duplicates`
- `tabs::tests::ts10_resize_cancels_pending_restore_without_respawn`
- `tabs::tests::ts11_restore_worker_panic_returns_failed_and_clears_state`
- `tabs::tests::ts13_offthread_swap_installs_pending_scrollback_restore`
- `tabs::tests::welcome_without_windows_leaves_group_none`

All other tests pass: 2737 passed / 7 failed (above) / 3 ignored for the
`emterm lib` command; 753 passed / 0 failed / 11 ignored for the
`term_core lib` command (round-10 rework, task0010, adds 9 direct
equivalence tests — `reflow::tests::same_width_fast_path_matches_reference_*`
and `..._falls_back_to_reference_...` — comparing the new same-width
resize path against the pre-round-10 reference implementation, on top of
round-9's 744). "All pass" is therefore not a literal claim this run
supports for the `emterm lib` gate — the 7 above are the sole, documented
exception.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Replay of a resize-interleaved apt-style recording into a fixed-size core | No row mixes content from two distinct logical lines | Unit |
| TS-2 | Replay of a resize-interleaved TUI-style (cursor-addressed redraw) recording | No cross-line content mixing | Unit |
| TS-3 | Replay of a resize-free recording | Grid identical to pre-fix behavior | Unit |
| TS-4 | Marker-bearing recording through full pipeline (write filter → snapshot → replay) | Marker bytes never appear as visible cells | Unit |
| TS-5 (round-9 rework, D6'''''', review round-8 finding `ba7953c458050780`) | Existing Rust `emterm lib` suite (single-threaded) + `term_core lib` suite + `bun test` | All pass, apart from the 7 pre-existing failures documented above (reproduced on base commit, out of scope) | Unit/Integration |
| TS-6 | CLI-only feature check (`--no-default-features`) | Compiles clean | Build |
| TS-7 (round-8 rework, AC-1/AC-2, review round-7 finding `01f91fe698ceb287`; extended round-9, AC-1, review round-8 finding `6082de4e619d7f51`) | Resize sequence longer than `MAX_DIM_MARKERS`, driven through the real ring / snapshot assembly / replay, PARAMETERIZED over eviction counts of zero (round-9 addition), one, several, and many. Round 7's version pinned eviction to exactly one, so it could not reach — and its `capped == full` assertion made the mixed-row comparison vacuously true even where it could reach — the shapes where round 7's fix (attribute the whole gap to the LAST evicted entry's dims) actually broke down: measured at 1/3/13 mixed rows against full attribution's 0, worse than "no segments at all" in the worst case. Round 8 stops attributing the gap at all once 2+ entries are evicted (`ScrollbackRingBuffer::read_segments` leaves it unattributed; `TerminalCore::replay_segments` replays it at the caller's target dims instead of dropping it). Round 9 raises `MAX_DIM_MARKERS` to 62 (the wire ceiling) and adds the `eviction_count = 0` case: a storm whose total recorded markers land AT the new cap needs no eviction at all, so its capped segment list is IDENTICAL (not merely "no worse than") to full uncapped attribution — this is the concrete shape AC-1's "resize storm of any length up to the wire ceiling" describes. At every eviction count the test asserts the capped replay is never worse than replaying with no segments at all (the "worse than not fixing" catch), and for zero/one eviction it additionally asserts exact structural + mixed-row equality with full uncapped attribution. **Residual** (documented in the test, not hidden, and NOT closed by the round-9 cap raise): for 2+ evictions — now reachable only by a storm recording MORE than 62 distinct dimensions with no intervening switch — this implementer's own measurement (re-run at the new cap) found the SAME construction where the gap spans phase 0's own two dimension regimes still mixes 1-12 rows against full attribution's 0; raising the cap only raises how long a storm must run before it needs any eviction at all, it does not make the 2+-eviction fallback itself any smarter. This is an accepted, measured precision loss mirroring `MAX_DIM_MARKERS`'s own documented trade-off, not a round-9 regression. | Unit |
| TS-7b (round-9 rework, AC-1, review round-8 finding `6082de4e619d7f51`) | Round-8's own cap sweep, re-measured directly (not extrapolated) against the raised cap: fixed-length storms of 26/32/52 total recorded markers, replayed at cap 62 | Zero mixed rows for all three — none reaches the new cap, so no eviction occurs and the capped replay matches full uncapped attribution exactly (see "FR2 cap-sweep re-measurement" below for the full table across eviction counts, not just these three totals) | Unit (re-measured via a generalization of TS-7's own harness, `run_resize_storm_cap_eviction_case`) |
| TS-8 (round-6, AC-5) | `build_from_snapshot`'s prefix/suffix split for the "ordinary switch" shape (small differing head + bulk tail at target) | Grid/cursor identical to the fully synchronous reference | Unit |
| TS-9 (round-6, AC-6/AC-9; revised round-8, AC-3/AC-6, review round-7 finding `01f91fe698ceb287`; revised round-9, AC-2/AC-7, review round-8 findings `6082de4e619d7f51`/`45033eaafbdf8e25`) | Malformed segment offsets (non-monotonic / past content length) and an over-budget segment dimension product, PLUS (round 8) a non-zero LEADING offset and the largest segment list the daemon can actually produce (structural check), PLUS (round 9) the same largest-shape check driven through the REAL producer path (ring → `read_segments` → `build_snapshot_bytes` → encode → decode), and a direct check that realistically large terminal dimensions (400×900, 700×700, 1000×500) still fit `PRODUCER_SEGMENT_CELL_BUDGET` unclamped. Round 8 flips the non-zero-leading-offset case from rejected to ACCEPTED: it is now the normal shape `ScrollbackRingBuffer::read_segments` produces when 2+ `dim_markers` entries are cap-evicted (TS-7), and `TerminalCore::replay_segments` was fixed in lockstep to replay that leading span at the caller's target dims instead of dropping it. Round 9 raises `MAX_DAEMON_SNAPSHOT_SEGMENTS` from 26 to 64 (`MAX_DIM_MARKERS` + head + screen) alongside the cap, and raises `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` from 8x to 32x `MAX_SEGMENT_CELLS` so the derived `PRODUCER_SEGMENT_CELL_BUDGET` (500,000) stays ABOVE the pre-round-9 value (307,692) rather than shrinking to 125,000 underneath a real large display — the round-8 finding `45033eaafbdf8e25`'s concern (a tautological structural-only test cannot catch this constant drifting from real producer output) is closed by the new real-path test. | Decoder rejects non-monotonic / past-content-length offsets and over-budget dimensions as `Malformed`; accepts a non-zero leading offset and the daemon's largest producible segment list (structurally AND via the real producer path); real large terminal dimensions round-trip unclamped | Unit |
| TS-10 (round-6, AC-7) | A malformed structured snapshot frame reaching `Tab::apply_mux_message` | Frame is skipped (logged), current display unchanged — not blanked | Unit |
| TS-11 (round-6, AC-10/AC-11) | Oversize snapshot on the visibility-resume path; partial-drain reattribution in the scrollback write filter | Pane stays Detached (no doomed frame enqueued); retained tail attributed to the read that produced it | Unit |
| TS-12 (round-9 rework, AC-5, review round-8 finding `1e7e069001cf22dc`) | Every client-side dimension decision point: `Tab::spawn_shell`'s initial core/PTY, `Tab::resize`, and `App::set_grid_size`'s own grid record, each driven with an out-of-domain (`u16::MAX`) request | All three clamp to the IDENTICAL `clamp_dims_to_wire_domain` output; `App::cell_size` never disagrees with the core it drives | Unit |
| TS-13 (round-9 rework, AC-6, review round-8 finding `7be271b2ead1bf07`) | `MuxPane::new` construction when the corrective PTY resize (after a clamp) fails | Pane records the PTY's ACTUAL size (queried via `get_size()`), not the clamped values it never reached | Unit |

### FR2 cap-sweep re-measurement (round-9 rework, AC-1, review round-8 finding `6082de4e619d7f51`)

Round-8's own reviewer measured a cap sweep (216 shapes: row pairs × replay
targets × eviction counts) and reported, for a fixed shape family, "cap 24
→ 3/3/3 mixed rows / cap 32 → 0/0/3 / cap 48 → 0/0/3 / cap 62 → 0/0/0" for
three fixed-length storms (26/32/52 total recorded markers). This
implementer re-measured directly (not trusting the reported numbers
blind) using a generalization of TS-7's own harness
(`run_resize_storm_cap_eviction_case`, `src-tauri/src/mux/ipc/pty_spawn.rs`)
at the new cap (62), sweeping `eviction_count` (total recorded markers =
`MAX_DIM_MARKERS` + `eviction_count`) rather than fixed totals, which
generalizes the reviewer's three-point sweep into a continuous curve:

| eviction_count | total markers | no-segments mixed | full-attribution mixed | capped mixed |
|---|---|---|---|---|
| 0 | 62 | 30 | 0 | **0** |
| 1 | 63 | 3 | 0 | **0** |
| 2 | 64 | 31 | 0 | 1 |
| 3 | 65 | 3 | 0 | 3 |
| 4 | 66 | 32 | 0 | 1 |
| 8 | 70 | 34 | 0 | 3 |
| 12 | 74 | 36 | 0 | 5 |
| 20 | 82 | 40 | 0 | 9 |
| 28 | 90 | 43 | 0 | 12 |
| 40 | 102 | 46 | 0 | 15 |
| 60 | 122 | 46 | 0 | 15 |

**This confirms the reviewer's result in the regime they actually tested**:
every storm whose total recorded markers is at or under the new cap (62) —
`eviction_count` 0 or 1 — matches full uncapped attribution EXACTLY (0
mixed rows), reproducing their "cap 62 → 0/0/0" for storms of 26/32/52
total markers (all ≤ 62). It does **not** reproduce a stronger claim the
prose could be misread as ("cap 62 eliminates the residual for ANY storm
length"): once `eviction_count` reaches 2, the same bounded-but-nonzero
divergence documented before this task (`MAX_DIM_MARKERS`'s own doc, TS-7)
persists at the new cap exactly as it did at the old one — raising the cap
only raises how many distinct dimensions a storm can record before it
needs any eviction at all (from 24 to 62), not the outcome once eviction
genuinely happens. AC-1's own wording ("a resize storm of any length **up
to the wire ceiling**") already scopes to the regime this table confirms;
this note exists so a future reader does not read AC-1 as a universal
claim it never made.

### NFR1 Measured Latency (round-6 rework, AC-1/AC-2/AC-4/AC-13; re-measured round-9 at the raised cap, AC-3; round-10 rework closes the storm-path cost, task0010 AC-1/AC-3/AC-4/AC-6)

Release build, `ordinary_switch_bench_950kib_matches_segment_free_cost` +
`segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`
(0.95 MiB payload, 100x30 grid, 10 000-line scrollback default — the
round-4/round-5 measurement methodology), re-measured after round-10's
fix (D1: `TerminalCore::resize_same_width` no longer re-wraps retained
scrollback for a same-width resize — see `crates/term_core/src/reflow.rs`
and that bench's own doc comment):

| Shape | Latency |
|-------|---------|
| Segment-free (baseline) | ~8-9 ms |
| Ordinary switch (small differing head + bulk tail at target — the shape a real spawn-size head marker + settled GUI grid produces) | ~1.4-2.2 ms, cap-independent (unaffected by round-10 — the prefix/suffix split, not this bench's storm path, is what makes this shape fast) |
| Single segment differing, no stable tail (contrived worst case; the split cannot help) | ~160-170 ms (round-9: ~270-300 ms — improved incidentally because this segment's dims differ only in ROWS, the same-width shape round-10 makes cheap) |
| Resize storm, 15 segments (quarter of the new `MAX_DIM_MARKERS`=62) | ~18-20 ms (round-9: ~234-237 ms) |
| Resize storm, 24 segments | ~17-18 ms (round-9: ~323-330 ms) |
| Resize storm, 32 segments | ~160-170 ms (round-9: ~2.49-2.52 **seconds**) |
| Resize storm, 48 segments | ~160-170 ms (round-9: ~3.65-3.69 **seconds**) |
| Resize storm, 62 segments (at the daemon cap) | ~160-170 ms (round-9: ~4.5-4.7 **seconds**) |

**D1 closes the NFR1 question round-9 left open.** The storm-path cost
that scaled to seconds is gone: every row above stays within roughly
20x of the segment-free baseline instead of climbing to 500-600x. This
implementer verified the mechanism directly during development (not just
the aggregate timing): `resize_same_width`'s own inputs — the number of
rows moved between viewport and scrollback per call — stay bounded by
`|rows_a - rows_b|` (6, in this bench's fixture) for EVERY segment
transition regardless of how much scrollback has accumulated or how many
segments the storm contains, confirming the fix removes the dependency on
accumulated content, not merely the specific numbers above.

The storm rows do NOT flatten to a single constant value (24/15 segments
measure ~10x cheaper than 32/48/62) — this residual variation is NOT
resize cost: it traces to how much real scrollback-compression work
(`cell_to_slim` on rows that genuinely scroll off — an ordinary,
pre-existing PTY-output cost this task does not touch) a given storm
shape happens to trigger, which this implementer confirmed varies with
segment count independent of the resize path (the per-resize row-movement
count stays 0-6 in every case measured, in both the cheap and the not-as-
cheap segment counts). AC-1's "within a stated small multiple of the
segment-free baseline" is satisfied by the worst measured row (~160-170 ms
against ~8-9 ms, roughly 20x) — `segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`
asserts this directly as a hard bound (`t_cap < t_baseline*60 + 200ms`,
comfortably above the ~160-170 ms measured and comfortably below the
pre-D1 multi-second cost), restoring the automated latency gate round-9
downgraded to informational (AC-6). Confirmed to fail without D1:
reverting `resize_same_width` to call the pre-round-10 reference
implementation unconditionally reproduces the exact round-9 numbers above
(re-measured by this implementer, not merely cited) and fails the
restored bound by roughly 7x.

AC-4 is judged on the "ordinary switch" row: it is not measurably slower
than segment-free (well under the bench's 3x + 20ms bound), unaffected by
round-10 (that shape was already fast via the prefix/suffix split, an
unrelated mechanism this task does not change) — confirmed by direct
re-measurement, not assumption.

The round-8/round-9 concern this section previously carried — "the
verify phase should treat this as an open NFR1 question" — is resolved:
the round-8 reviewer's suggestion (b) (changing
`enforce_dim_marker_cap`'s victim-selection strategy) is no longer the
only path to an acceptable NFR1 outcome at this cap, because the cap's
own latency trade-off (`MAX_DIM_MARKERS`'s doc, `scrollback_buffer.rs`)
no longer applies now that the storm-path resize cost is flat.

**Scope of the fix, stated explicitly (not folded silently into the
numbers above):** D1's cheap path applies to a SAME-WIDTH resize only —
`cols` unchanged, only `rows` differs — which is what `dim_markers`
records for this ring (a `(cols, rows)` pair) and what every fixture in
this codebase's storm/cap-sweep tests actually exercises (a height-only
window drag; `crates/term_core/src/bench.rs`'s storm payload and
`pty_spawn.rs`'s `run_resize_storm_cap_eviction_case` both hold `cols`
fixed). A resize that ALSO changes `cols` still goes through the
pre-existing full content-preserving reflow (`resize_full_reflow`,
unchanged by this task) and still costs what it did before — this task
did not attempt the harder general case (re-wrapping deferred across a
mix of past COLUMN widths, not just row counts), because it is not the
shape any measured benchmark, cap-sweep test, or MT-3's own wording
(a vertical/height drag) exercises. If a real corner-drag resize storm
that also varies width turns out to matter, it is not covered by this
measurement and would need its own investigation.

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
| FR2 | task0001, task0006, task0009, task0010 | TS-1, TS-2, TS-7, TS-7b, FR2 cap-sweep re-measurement above; re-confirmed unaffected by task0010's resize-path change (`pty_spawn::tests::resize_storm_beyond_marker_cap_replays_no_worse_than_full_attribution` and the other cap-sweep/fingerprint tests in that module re-run green — task0010 does not touch `scrollback_buffer.rs`'s marker logic, only `term_core`'s resize implementation) |
| FR3 | task0001, task0006, task0009, task0010 | TS-1, TS-2, TS-3, TS-4, TS-7, TS-9, TS-10, TS-11, TS-12, TS-13; task0010 adds 9 direct equivalence tests (`crates/term_core/src/reflow.rs`, `same_width_fast_path_matches_reference_*`) comparing the new same-width resize path against the pre-round-10 reference on grid, cursor, scrollback content/wrapped-flags, eviction total, and intern-table state across grow/shrink/zero-capacity/mid-viewport-cursor/fallback shapes |
| NFR1 | task0001, task0006, task0009, task0010 | MT-2 (manual latency feel check) + NFR1 Measured Latency table above — **task0010 closes the open NFR1 question round-9 left**: the storm-path resize cost that scaled to seconds is now flat (~160-170 ms worst case), and the automated latency bound is a hard assertion again (AC-6) |
| NFR2 | task0001, task0009 | TS-6 (re-confirmed green after this task's changes) |
| NFR3 | task0001, task0009 | TS-5 (updated round-9 to include `term_core lib` and state the 7 pre-existing failures) |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: On-device — run Claude Code in mux, repeat window/tab switches
      (including detach → attach); no line-content mixing appears
- [ ] MT-2: On-device — window switch / reattach latency feels unchanged
      from before the fix
- [ ] MT-3 (task0005 rework, review round-4 finding `6c650908ea8e95e9`;
      **round-9 note, AC-3**, superseded by **round-10, task0010 AC-8**:
      round-9 flagged that this exact shape — dozens of grid-size changes
      in quick succession — measured 2.5-4.7 SECONDS at 32-62 recorded
      dimensions, and warned this on-device check might observe a
      multi-second stall. Round-10's `TerminalCore::resize_same_width` fix
      (D1) closes that: the same shape now measures ~160-170 ms (see the
      NFR1 Measured Latency table above), so this check is not expected to
      observe a multi-second stall any longer):
      On-device — drag a mux window's edge to resize it repeatedly (a
      continuous drag, not discrete resizes), producing dozens of grid-size
      changes in quick succession against a pane with substantial
      scrollback (e.g. a long-running `seq`/`glances`/log-tailing pane),
      then immediately switch away to another window and back. Record
      whether the switch stalls and for how long, and confirm the restored
      content is correct (no cross-phase-mixed rows) regardless.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Tests | 14 (TS-1..TS-13, including TS-7b) | 14 | 0 | 0 |
| Manual | 3 (MT-1, MT-2, MT-3) | 0 | 0 | 3 |
