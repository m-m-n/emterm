# Feature: mux-tab-switch-replay-latency

## Overview

mux tab switching to a pane with accumulated scrollback takes seconds to
display because the snapshot replay bypass-split gate breaks down when
resize markers accumulate near the tail of the scrollback. This feature
restores bypass-equivalent replay latency (tens of ms) for that shape and
removes the upstream cause of marker accumulation.

## Objectives

- Restore bypass-equivalent replay latency for panes whose scrollback has
  accumulated multiple resize markers near the tail.
- Stop the GUI from reshaping all mux panes before the status bar's visible
  row count has settled, which is the upstream cause of marker accumulation.
- Prevent bypass from dropping when a grid resize races with an in-flight
  pane switch, and prevent duplicate snapshot fetches for consecutive
  switches to the same pane.

## User Stories

### US1: Fast switch to a heavy pane
As a mux user, I want switching to a pane with a large accumulated
scrollback to be fast, so that I am not blocked for seconds after
reattaching to the daemon.

**Acceptance Criteria:**
- [ ] Switching to a pane matching the reproduced shape (2 MiB scrollback,
  31 segments, `k=27`, `split_at=2,096,654`, `suffix_len=7,395`) completes
  in the tens-of-ms order, matching bypass-enabled latency for the same
  payload size.
- [ ] Ordinary switch latency (currently 1.57 ms) does not regress.

### US2: Correct behavior during startup/reattach resize
As a mux user, I want the GUI to avoid resizing all panes while the status
bar row count is still settling at startup, so that resize markers do not
accumulate in scrollback in the first place.

**Acceptance Criteria:**
- [ ] The initial `visible_row_count` 0 → 1 transition does not send
  `MessageType::Resize` to all panes in the mux group.

### US3: Bypass resilience during concurrent resize / repeated switch
As a mux user, I want a pane switch in progress to keep using the bypass
path even if a grid resize happens concurrently, and I want repeated
switches to the same pane to not refetch the snapshot, so that transient
timing does not degrade responsiveness.

**Acceptance Criteria:**
- [ ] A grid resize occurring while a switch is in flight does not cause
  target-dims mismatch to defeat the bypass split (the ~2.2x latency gap
  observed between a resize-during-switch case (21.0 ms) and a
  resize-free case (9.5 ms) is closed).
- [ ] Consecutive switches to the same pane do not trigger duplicate
  snapshot replay/rebuild. (Scope decision, review round 1 / task0006:
  decode and daemon fetch may still happen twice for two frames arriving
  close together — only the off-thread replay build is deduplicated. See
  FR8.)

## Technical Requirements

### Functional Requirements

- **FR1:** A pane whose scrollback matches the reproduced shape ("2 MiB
  scrollback + resize-marker cluster near the tail with dims different
  from target + small suffix") replays in the tens-of-ms order, on par
  with bypass-enabled replay for the same payload size.
- **FR2:** A bench/regression test reproducing this shape is added and
  asserts a latency ceiling.
- **FR3:** Ordinary switch latency (currently 1.57 ms) does not regress.
- **FR4:** Bypass equivalence is preserved — viewport and cursor state
  after replay match the non-bypass path, and the meaning of
  `scrollback_populated` is unchanged.
- **FR5:** The existing bench guard `snapshot_replay_bench_2mib_seq`
  (`crates/term_core/src/bench.rs:169`) stays green.
- **FR6:** The GUI does not reshape all panes in the mux group before the
  status bar's visible row count has settled (the initial
  `visible_row_count` 0 → 1 transition must not broadcast
  `MessageType::Resize` to all panes).
- **FR7:** A grid resize that races with an in-flight pane switch does not
  cause the bypass split to be defeated by target-dims mismatch.
- **FR8:** Consecutive switches to the same pane do not replay/rebuild the
  snapshot twice (scope decision, review round 1 / task0006: decode and
  daemon fetch are NOT deduplicated by this feature — only the off-thread
  replay build is; see `ac3_duplicate_same_pane_snapshot_coalesces_before_spawning_again`
  and `t6_ac6_...` in `src-tauri/src/tabs.rs`).

### Non-Functional Requirements

- **NFR1 - Performance:** The fix for FR1/FR7 must not reintroduce the
  double non-bypass replay cost that `BYPASS_PREFIX_MAX_BYTES` (64 KiB) and
  `suffix_len >= split_at` were added (in round-7/round-8 review) to
  prevent — i.e. simply loosening those two gates without also making
  prefix-side replay cheap is out of bounds.

## Implementation Approach

### Root Cause (confirmed by measurement, see References)

1. **Marker accumulation (upstream):** `panel_height_logical =
   ROW_HEIGHT(22.0) × visible_row_count`. On startup/reattach, the status
   bar's `visible_row_count` transitions 0 → 1, changing `bot_inset` and
   therefore the grid by 2 rows (`floor` difference at `cell_h=19.0`).
   This single resize is broadcast via `MessageType::Resize` to all panes
   in the mux group (`src-tauri/src/tabs.rs:3397`), and
   `ScrollbackRingBuffer::attribute_write`
   (`src-tauri/src/mux/scrollback_buffer.rs:524`) appends a resize marker
   to every affected pane's scrollback whenever recorded dims differ from
   the previous write. Repeated settling produced 24 markers within an 11
   KB tail window in the measured case.

2. **Bypass split gate breakdown (downstream, `crates/term_core/src/terminal_core.rs:1143`):**
   ```rust
   let bypass_split = bypass
       && k > 0
       && k <= BYPASS_PREFIX_MAX_SEGMENTS      // 24
       && suffix_len >= BYPASS_SUFFIX_MIN_BYTES // 4096
       && split_at <= BYPASS_PREFIX_MAX_BYTES   // 64 KiB
       && suffix_len >= split_at;
   let bypass_engaged = bypass_split || (bypass && k == 0);
   ```
   With markers clustered near the tail, `k` (from
   `stable_target_suffix_start`, `crates/term_core/src/terminal_core.rs:1798`)
   grows past 24, `split_at` grows to nearly the full payload length, and
   `suffix_len < split_at` — all three gates break simultaneously, forcing
   a full non-bypass replay of the entire payload (measured 782.8–977.6 ms
   for 2.1 MB, vs. 9.3–9.5 ms for the same pane when bypass engages).

3. **Resize-during-switch bypass drop:** when a grid resize lands between
   snapshot decode and replay, the `target` used at decode time and at
   replay time can differ. If the tail marker's dims match the decode-time
   target but not the replay-time target, `k` becomes the full segment
   count (no matching run), `split_at` becomes the full payload length,
   and `suffix_len` becomes 0 — defeating bypass independently of marker
   accumulation. Measured: 21.0 ms (resize raced) vs. 9.5 ms (no race) for
   the same ~50 KB pane.

4. **Duplicate snapshot fetch:** a switch to the same pane has been
   observed to trigger two decodes ~1 ms apart (e.g. `segs=9` then
   `segs=10`), with the first replay result discarded.

### Constraint on the fix shape

`BYPASS_PREFIX_MAX_BYTES` and `suffix_len >= split_at` exist specifically
to prevent a large `split_at` (large prefix) from engaging split when the
2nd-pass worker (`build_scrollback_only_from_snapshot`) would then pay the
same non-bypass cost again for the prefix. A correct fix for FR1 must
either avoid the double-cost scenario while allowing the gates to loosen,
or take a different approach (e.g. treating dense marker clusters
differently from ordinary prefix growth) — not just relax the numeric
thresholds.

### Affected Components

- `crates/term_core/src/terminal_core.rs` — bypass split gate
  (`stable_target_suffix_start`, `bypass_split`/`bypass_engaged` computation)
- `src-tauri/src/mux/scrollback_buffer.rs:524` — resize marker recording
  (`ScrollbackRingBuffer::attribute_write`)
- `src-tauri/src/tabs.rs:3397` — GUI grid resize → all-pane
  `MessageType::Resize` broadcast
- Pane-switch snapshot fetch path (dedup for FR8; target-dims capture
  timing for FR7)

## Test Scenarios

### Unit Tests
- [ ] Test: bypass split gate with a marker cluster near the tail
  (k > 24, large split_at, small suffix) — after the fix, bypass engages
  or replay otherwise completes within the latency ceiling.
- [ ] Test: bypass split gate with an ordinary marker position (existing
  behavior) — unchanged, still engages as before.

### Integration Tests
- [ ] Test: pane switch with a resize landing between decode and replay —
  target-dims mismatch does not defeat bypass.
- [ ] Test: two consecutive switches to the same pane within a short
  window — snapshot is fetched/decoded once.

### E2E Tests
**Existing E2E tests**: None detected in this repository.
**Run command**: Not applicable.
- [ ] N/A — no existing E2E suite to regress.

### Edge Cases
- [ ] Marker count exactly at `BYPASS_PREFIX_MAX_SEGMENTS` (24) boundary.
- [ ] `split_at` exactly at `BYPASS_PREFIX_MAX_BYTES` (64 KiB) boundary.
- [ ] `suffix_len` exactly at `BYPASS_SUFFIX_MIN_BYTES` (4096) boundary.

### Performance Tests
- [ ] Bench: reproduce the measured shape (2 MiB scrollback, 31 segments,
  `k=27`, `split_at=2,096,654`, `suffix_len=7,395`) and assert a
  tens-of-ms latency ceiling (FR2).
- [ ] Bench: `snapshot_replay_bench_2mib_seq` remains green (FR5).
- [ ] Bench/measurement: ordinary switch latency does not regress from the
  1.57 ms baseline (FR3).

## Security Considerations

Not applicable — internal performance fix, no new external inputs, no
authN/authZ or data-protection surface touched.

## Performance Optimization

### Performance Goals
- Marker-cluster shape (FR1): tens-of-ms order, matching bypass-enabled
  replay for the same payload size (measured bypass baseline: 9.3–9.5 ms
  for a ~50 KB pane; non-bypass baseline for the 2.1 MB shape before the
  fix: 782.8–977.6 ms).
- Ordinary switch (FR3): no regression from the 1.57 ms baseline.

### Optimization Strategies
- See "Constraint on the fix shape" above — the approach must not
  reintroduce the double 2nd-pass replay cost that the existing
  thresholds guard against.

## Success Criteria

- [ ] FR1–FR8 implemented and tested.
- [ ] All test scenarios pass.
- [ ] Performance goals above are met.
- [ ] `snapshot_replay_bench_2mib_seq` stays green.
- [ ] Code review is completed.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — all requirements resolved from the source task page; no `tbd`
requirements.

## References

- Notion task page: https://www.notion.so/3ac3509ec8ee81578318cd552d238518
- Investigation report (referenced by the source task, present in the main
  working tree's gitignored `tmp/`): `tmp/tab-switch-latency-investigation-2026-07-30.md`
- `crates/term_core/src/bench.rs:169` — existing bench guard
  `snapshot_replay_bench_2mib_seq`
- `crates/term_core/src/terminal_core.rs:1143` — bypass split gate
- `crates/term_core/src/terminal_core.rs:1798` — `stable_target_suffix_start`
- `src-tauri/src/mux/scrollback_buffer.rs:524` — resize marker recording
- `src-tauri/src/tabs.rs:3397` — all-pane resize broadcast
