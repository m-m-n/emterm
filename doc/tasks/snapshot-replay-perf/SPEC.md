# Feature: snapshot-replay-perf — fast tab-switch by bypassing scrollback compression during replay

## Overview

When the mux client receives a pane snapshot (~2 MiB) and replays it via
`TerminalCore::build_from_snapshot`, the dominant cost is **not** ANSI parsing
itself but the per-row scrollback compression performed inside
`ring_buffer::ring_push_blank` (`cell_to_slim` intern + `release_slim_row`
dec-ref). For a `seq 1 N` style 2 MiB payload over a 200×50 grid with
`scrollback_lines = 10_000`, the replay takes **~4040 ms**; rerunning the same
replay with `scrollback_lines = 0` finishes in **~51 ms** (~80× faster) because
the compression loop is skipped entirely.

This feature exploits that observation in the narrowest possible way:
**at snapshot-replay time only**, `TerminalCore` enters an internal
"replay-mode bypass" inside `ring_push_blank` that skips the per-row
SlimCell intern + scrollback-deque push/pop hot loop while keeping all
observable bookkeeping (`scrollback_evicted_total`,
`get_scrollback_length()` used by mark stamping) byte-identical to the
current implementation. Once the payload has been fully drained, the
bypass is turned off and the core is handed back to the caller with the
caller-requested `scrollback_lines` already in effect so subsequent live
PTY output accumulates scrollback normally.

The mux IPC protocol, the daemon-side snapshot path, the snapshot binary
format, and all live-input code paths are unchanged. The only observable
user-facing behavior change is: **immediately after a tab switch / reattach,
the client-side scrollback contains only the viewport rows.** New output
appended after the switch accumulates into scrollback as usual.

## Objectives

- Reduce `snapshot_replay_bench_2mib_seq` per-call wall time from ~4040 ms
  to **< 1000 ms (MUST)**, ideally < 200 ms (SHOULD) or < 100 ms (STRETCH).
- Make the regression visible in CI by adding `assert!` thresholds to the
  existing `#[ignore]`-gated benches.
- Keep all other code paths (live PTY input, daemon snapshot assembly,
  scroll/search/fold features that depend on client-side scrollback for
  pre-existing rows) byte-identical to the current implementation.

## User Stories

### US1: Switching to a heavy mux tab feels instant
As a developer using mux mode with one tab running `seq 1 10000000`, I want
the act of switching to that tab to feel sub-second, so that mux is a viable
replacement for tmux for AI-heavy workflows.

**Acceptance Criteria:**
- [ ] `snapshot_replay_bench_2mib_seq` per-call < 1000 ms.
- [ ] In a manual run of `make build` + `seq 1 10000000` in one mux tab,
      switching to that tab is visibly comparable to tmux (no multi-second
      stall).

### US2: Scrollback for newly-arriving output still works after a switch
As a mux user, I want output that arrives *after* the tab switch to still
land in client-side scrollback, so my mux experience is not silently broken
beyond the one-shot replay window.

**Acceptance Criteria:**
- [ ] After a tab switch / reattach, additional PTY output causes
      `core.scrollback_count()` to grow up to the configured
      `scrollback_lines`.
- [ ] Existing unit tests covering replay output equivalence
      (`test_build_from_snapshot_matches_reset_and_replay`,
      `test_build_from_snapshot_empty_payload`,
      `test_build_from_snapshot_is_send_across_threads`,
      `test_build_from_snapshot_cancelled_returns_none`) continue to pass.

### US3: Performance does not silently regress later
As the next maintainer, I want CI to flag a regression if either the main
replay path or any supporting hot path becomes slow again, so that I'm not
chasing the bug through git bisect six months later.

**Acceptance Criteria:**
- [ ] `snapshot_replay_bench_2mib_seq` `assert!`s its per-call < 1000 ms.
- [ ] `snapshot_replay_attribution_2mib_seq` `assert!`s the
      scrollback-disabled configuration < 200 ms.
- [ ] `strip_replayable_rich_content_bench_2mib_plain` `assert!`s < 30 ms.
- [ ] `scrollback_read_all_bench_2mib_wrapped` `assert!`s < 1 ms.

## Technical Requirements

### Functional Requirements

- **FR1 — Bypass scrollback compression during snapshot replay:**
  In the snapshot-replay entry point (`TerminalCore::build_from_snapshot` or
  an internal helper it calls), enable an internal "replay-mode bypass" on
  the freshly-built `TerminalCore` before draining the payload, and disable
  it after. While the bypass is on, `ring_push_blank`'s per-row eviction step
  must skip the SlimCell intern (`cell_to_slim`), the `scrollback_slim` /
  `scrollback_wrapped` push, and (when at virtual capacity) the
  `pop_front` + `release_slim_row` dec-ref loop. All other observable
  bookkeeping the eviction step would have updated under the current
  implementation MUST be preserved byte-identically: `scrollback_evicted_total`,
  the value returned by `get_scrollback_length()` (used by `PendingPromptMark`
  / `PendingFoldMark` stamping inside `process_pty_data_fully`), the
  per-row overflow side-table clear, viewport rotation, and BCE-fill of the
  new bottom row. The bypass tracks a virtual "would-have-been" scrollback
  length internally so that `get_scrollback_length()` returns the same value
  during the bypass that the current implementation returns under the same
  payload and capacity.

- **FR2 — Restore live scrollback capacity post-replay:**
  After the replay completes (and before the function returns), the bypass
  is disabled and the core is left with `scrollback_capacity ==
  scrollback_lines` (the caller-requested value), so that *subsequent* live
  PTY output evicts viewport rows into scrollback normally up to that limit.
  The post-replay scrollback content is empty (`scrollback_count() == 0`);
  only the **capacity** is in effect for the live path. The post-replay
  `TerminalCore` must satisfy the same invariants `ring_buffer.rs` assumes
  on a `TerminalCore::new(cols, rows, scrollback_lines)` — i.e. `ring_capacity
  = rows + scrollback_capacity`, no leaked SlimCell intern entries, etc.

- **FR3 — Caller signature and observable contract are preserved:**
  `TerminalCore::build_from_snapshot(cols, rows, scrollback_lines, payload,
  cancel)` retains its current public signature and externally observable
  behavior except for the scrollback-population side effect described in
  FR2. Specifically, the returned `SnapshotReplay`'s `evicted_total`,
  `prompt_marks` (`abs_row` + `evicted_total` per mark), `fold_marks`
  (`abs_row` + `evicted_total` per mark), and `actions` must be byte-identical
  to today's path on the same payload (the viewport grid stays byte-identical
  via the existing `test_build_from_snapshot_matches_reset_and_replay`
  equivalence). The cancel-flag contract (return `None` if cancelled
  mid-drain) is unchanged. The function's empty-payload contract is
  unchanged.

- **FR4 — Add a regression-guard assert to the main replay bench:**
  `crates/term_core/src/bench.rs::snapshot_replay_bench_2mib_seq` gains an
  `assert!(per_call < Duration::from_millis(1000))` after the existing
  `eprintln!` reporting. The bench remains `#[ignore]`.

- **FR5 — Add regression-guard asserts to the supporting benches:**
  Three additional thresholds are added (all `#[ignore]` benches retained):
  - `crates/term_core/src/bench.rs::snapshot_replay_attribution_2mib_seq`:
    the scrollback-disabled configuration must `assert!(per_call <
    Duration::from_millis(200))`. The baseline (scrollback-enabled) and
    no-scroll configurations remain reported via `eprintln!` only — they
    are diagnostic, not regression-gated.
  - `src-tauri/src/mux/scrollback_filter.rs::
    strip_replayable_rich_content_bench_2mib_plain`:
    `assert!(per_call < Duration::from_millis(30))`.
  - `src-tauri/src/mux/scrollback_buffer.rs::
    scrollback_read_all_bench_2mib_wrapped`:
    `assert!(per_call < Duration::from_millis(1))`.

### Non-Functional Requirements

- **NFR1 — Performance:** `snapshot_replay_bench_2mib_seq` per-call < 1000 ms
  (MUST), targeted < 200 ms (SHOULD), best-effort < 100 ms (STRETCH). Measured
  in `--release` on the user's local machine; CI tolerance absorbed by the
  10–20× safety margin in the assert thresholds.

- **NFR2 — Protocol / format stability:** No change to the mux IPC protocol,
  no change to the snapshot byte stream sent by the daemon, no change to
  `src-tauri/src/mux/` daemon-side logic.

- **NFR3 — Correctness equivalence on the live path:** All non-replay code
  paths through `TerminalCore` (interactive PTY input, scroll, search, fold,
  resize_reflow) behave byte-identically. All existing tests in
  `crates/term_core` and `src-tauri/src/mux/*` pass without modification
  except where a test is intentionally extended to cover FR2 (the
  scrollback-capacity-restored invariant).

- **NFR4 — Portability:** Linux release build (`make build`) and Windows
  cross-build (`make win-build`) succeed; CLI-only build
  (`--no-default-features`) still type-checks; `bun test` and
  `bun run typecheck` unaffected.

## Implementation Approach

### Architecture

The change is local to the `term_core` crate plus one or two assert
additions in `src-tauri/src/mux/`. There is no new module; no public API
type changes; no protocol changes.

```
┌───────────────────────────────────────────────────────────────┐
│  mux client (src-tauri/src/mux/)                              │
│  ─ receives Snapshot bytes from daemon                        │
│  ─ calls term_core::TerminalCore::build_from_snapshot(...)    │
└──────────────────────────────┬────────────────────────────────┘
                               │  (unchanged call site)
                               ▼
┌───────────────────────────────────────────────────────────────┐
│  term_core::TerminalCore::build_from_snapshot                 │
│  ─ NEW: construct with scrollback_capacity = 0                │
│  ─ existing: process_pty_data_fully(payload, cancel)          │
│  ─ NEW: restore scrollback_capacity = caller scrollback_lines │
│  ─ return Some(core)                                          │
└──────────────────────────────┬────────────────────────────────┘
                               │
                               ▼
┌───────────────────────────────────────────────────────────────┐
│  term_core::ring_buffer::ring_push_blank                      │
│  ─ unchanged                                                  │
│  ─ while replaying: scrollback_capacity == 0 branch taken     │
│    (skip cell_to_slim / release_slim_row hot loop)            │
│  ─ post-replay: live input takes scrollback_capacity > 0      │
│    branch as before                                           │
└───────────────────────────────────────────────────────────────┘
```

### Data Flow

```
daemon → snapshot bytes (~2 MiB)
       → build_from_snapshot(cols, rows, scrollback_lines, payload, cancel)
           ├─ new TerminalCore with scrollback_capacity = 0
           ├─ process_pty_data_fully(&payload, cancel)
           │      └─ many ring_push_blank calls, each in the
           │         scrollback_capacity == 0 branch (cheap)
           ├─ promote scrollback_capacity to caller-requested value
           └─ return Some(core)   (or None if cancelled)
```

### Restoration mechanism

Two implementation options, to be chosen in `sdd.2-create-plan`:

**Option A — capacity promotion in place.**
Add a private helper on `TerminalCore` (or `RingBuffer`) like
`fn promote_scrollback_capacity(&mut self, new_capacity: usize)` that:
- Precondition asserts `self.scrollback_capacity == 0 &&
  self.scrollback_slim.is_empty() && self.scrollback_wrapped.is_empty()`.
- Sets `self.scrollback_capacity = new_capacity`.
- Recomputes / restores any other capacity-derived bookkeeping
  (`ring_capacity` if it tracks `rows + scrollback_capacity`, etc.).

This avoids cloning the ring at all and keeps the change very small.

**Option B — build small, then transplant grid into a properly-sized core.**
Construct the working core with `scrollback_capacity = 0`, drain the
payload, then `TerminalCore::new(cols, rows, scrollback_lines)` a fresh
target core and copy the viewport grid (and cursor / SGR / scroll-region /
alt-screen / mode state) into it. More allocation, more lines of code, but
makes the capacity-mutation contract impossible to misuse from outside the
replay path.

Both options preserve the `build_from_snapshot` signature and external
contract. Option A is the leading candidate on grounds of footprint and risk;
Option B is the fallback if Option A's invariants prove fragile.

### Dependencies

**Internal:**
- `crates/term_core` (modified): `terminal_core.rs::build_from_snapshot`,
  `ring_buffer.rs` (new private helper for Option A or no change for
  Option B), `bench.rs` (assert additions).
- `src-tauri/src/mux/scrollback_filter.rs`,
  `src-tauri/src/mux/scrollback_buffer.rs` (assert additions in `#[cfg(test)]`
  bench modules).

**External:** none new.

### File Structure (changes only)

```
crates/term_core/src/
├── terminal_core.rs       # build_from_snapshot body change (FR1, FR2, FR3)
├── ring_buffer.rs         # promote_scrollback_capacity helper (Option A) — or unchanged (Option B)
└── bench.rs               # assert!() added to two benches (FR4, FR5)

src-tauri/src/mux/
├── scrollback_filter.rs   # assert!() added to bench mod (FR5)
└── scrollback_buffer.rs   # assert!() added to bench mod (FR5)
```

## Test Scenarios

### Unit Tests

- [ ] **TS-1** Existing `test_build_from_snapshot_matches_reset_and_replay`
      continues to pass byte-identically (proves FR3 preserves visual
      output).
- [ ] **TS-2** Existing `test_build_from_snapshot_empty_payload` passes
      (proves empty-payload contract is preserved).
- [ ] **TS-3** Existing `test_build_from_snapshot_is_send_across_threads`
      passes (proves the `Send` boundary is intact).
- [ ] **TS-4** Existing `test_build_from_snapshot_cancelled_returns_none`
      passes (proves cancel-flag contract is preserved).
- [ ] **TS-5** New `test_build_from_snapshot_restores_scrollback_capacity`:
      build a core with `scrollback_lines = 10_000` from a payload large
      enough to scroll many lines, assert `core.scrollback_count() == 0`
      immediately after, then feed additional PTY bytes that scroll N more
      lines and assert `core.scrollback_count() == N`. Locks down FR2.

### Integration / Bench Tests

- [ ] **TS-6** `snapshot_replay_bench_2mib_seq` runs with `--include-ignored`
      and `assert!(per_call < 1000ms)` passes (FR4).
- [ ] **TS-7** `snapshot_replay_attribution_2mib_seq` runs with
      `--include-ignored` and the scrollback-disabled configuration's
      `assert!(per_call < 200ms)` passes (FR5).
- [ ] **TS-8** `strip_replayable_rich_content_bench_2mib_plain` runs and
      `assert!(per_call < 30ms)` passes (FR5).
- [ ] **TS-9** `scrollback_read_all_bench_2mib_wrapped` runs and
      `assert!(per_call < 1ms)` passes (FR5).

### Cross-build / CLI Tests

- [ ] **TS-10** `cargo check --no-default-features` on `src-tauri` is green
      (NFR4).
- [ ] **TS-11** `cargo test --manifest-path src-tauri/Cargo.toml` is green
      (NFR3).

### Manual / Experiential Tests

- [ ] **TS-12** Build a release binary (`make build`), open mux with two
      tabs, run `seq 1 10000000` in tab A, switch to tab B then back to
      tab A. Tab-switch back into A is qualitatively comparable to tmux
      (no multi-second stall). See bottleneck report §"体感確認" for the
      procedure.

### Edge Cases

- [ ] **EC-1** `scrollback_lines = 0` requested by the caller: behavior
      should be identical to today (the bypass is a no-op promotion).
- [ ] **EC-2** Payload payload contains DECSTBM / alt-screen / DECSC etc.:
      TS-1 already exercises representative ANSI; no extra scope.
- [ ] **EC-3** Snapshot replay during a window resize: out of scope —
      replay is single-shot and the caller already serializes it relative
      to resize. No mitigation required.

## Security Considerations

Not applicable — this change does not cross any trust or network boundary and
does not alter how external data is parsed or escaped.

## Error Handling

No new error paths. The existing `Option<TerminalCore>` return for the
cancel-flag case is preserved. If FR2's promotion helper panics on its
precondition (Option A), that indicates a programmer-error misuse and
should fail loudly in tests — not be silently caught.

## Performance Goals

| Bench                                                  | Current   | Target   | Assert threshold |
| ------------------------------------------------------ | --------- | -------- | ---------------- |
| `snapshot_replay_bench_2mib_seq`                       | ~4040 ms  | < 1000 ms (MUST), ideally < 200 ms (SHOULD), best-effort < 100 ms (STRETCH) | < 1000 ms |
| `snapshot_replay_attribution_2mib_seq` (scrollback off)| ~51 ms    | preserve | < 200 ms |
| `strip_replayable_rich_content_bench_2mib_plain`       | ~3.0 ms   | preserve | < 30 ms |
| `scrollback_read_all_bench_2mib_wrapped`               | ~60 µs    | preserve | < 1 ms |

The 10–20× headroom between observed and asserted values is intentional: it
absorbs CI machine variability without papering over genuine regressions.

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented.
- [ ] All test scenarios TS-1..TS-12 pass.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` green.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` green.
- [ ] Performance MUST (< 1000 ms) achieved on local machine; SHOULD /
      STRETCH recorded as actual numbers in `VERIFICATION_RESULT.md`.
- [ ] Manual tab-switch test (TS-12) qualitatively confirms the bottleneck
      report's prediction.
- [ ] `memory/project_mux_output_pipeline_perf.md` updated to record this
      task's outcome.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

(none — Option A vs. Option B is an implementation decision deferred to
`sdd.2-create-plan` and is not a requirement-level ambiguity.)

## References

- Investigation report: `tmp/mux-snapshot-replay-bottleneck-20260621.md`
- Related memory: `memory/project_mux_output_pipeline_perf.md`
- Predecessor task: `doc/tasks/mux-snapshot-reparse-offthread/`
- Primary source files:
  - `crates/term_core/src/terminal_core.rs` (`build_from_snapshot`)
  - `crates/term_core/src/ring_buffer.rs` (`ring_push_blank`,
    `cell_to_slim`, `release_slim_row`)
  - `crates/term_core/src/bench.rs`
  - `src-tauri/src/mux/scrollback_filter.rs`
  - `src-tauri/src/mux/scrollback_buffer.rs`
