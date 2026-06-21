# Feature: snapshot-replay-scrollback-restore

## Overview

After the recent perf work (`6b47754` / `9ceac36`), mux window/tab
switches with a payload ≥ `OFFTHREAD_REPLAY_THRESHOLD_BYTES` (64 KiB)
run `TerminalCore::build_from_snapshot` on a worker thread with
`scrollback_bypass = true`. This restores the visible grid quickly
(2 MiB / ~51 ms) but leaves `scrollback_slim` empty, so scrolling up
after a large switch shows no history. This feature adds a **2nd-pass
restore** that, immediately after the 1st-pass swap, reparses the same
payload on a second worker with bypass *disabled* and merges the
resulting scrollback into the live core — invisibly to the user.

## Objectives

- Restore the scrollback after every off-thread snapshot replay
  (`payload.len() >= OFFTHREAD_REPLAY_THRESHOLD_BYTES`) without
  re-introducing the UI block the perf work removed.
- Eliminate the threshold-dependent contract drift flagged by
  `codex-architecture` (multi-review high): both the synchronous
  `reset_frame_for_replay` path and the off-thread path end in the same
  observable state.
- Keep the wire format, the `SnapshotReplay` type, and the existing
  `build_from_snapshot` API source-compatible (additive only).

## User Stories

### US1: Scroll back through history after a large mux switch

As an emterm user heavily using mux, I want to scroll up after
switching to a window with a long scrollback so that I can reference
past output (e.g. a Claude Code conversation) without first hitting an
empty scrollback area.

**Acceptance Criteria:**
- [ ] After switching to a pane whose snapshot payload is ≥ 64 KiB, the
      visible grid appears within the perf-tuned latency budget
      (matches today's bypass-on path within noise).
- [ ] Within ~5 seconds of the switch (single 2 MiB payload),
      scrolling up shows the pane's history.
- [ ] The restored history is *byte-equivalent* to what the synchronous
      build path produces from the same payload.

### US2: Switch again before the restore finishes

As a user rapidly switching between mux windows, I want consecutive
switches to never wait for an in-flight restore, so that responsiveness
is not regressed.

**Acceptance Criteria:**
- [ ] A new switch supersedes the in-flight 2nd-pass via the same
      cancel mechanism the existing 1st-pass uses.
- [ ] Superseded 2nd-pass output is discarded (never merged into a
      core that is no longer displayed).

### US3: Resize the window during a restore

As a user resizing the terminal during a switch, I want the resize to
take effect immediately, even if it costs me the in-flight history
restore.

**Acceptance Criteria:**
- [ ] A grid resize during an in-flight 2nd-pass cancels it.
- [ ] No partial / mis-sized scrollback rows are merged into the live
      core after a resize.

## Technical Requirements

### Functional Requirements

- **FR1 — 2nd-pass spawn:** When `apply_offthread_swap` finishes
  swapping in a bypass-on core (i.e. the 1st-pass path), spawn a second
  worker thread that runs a bypass-*off* `build_from_snapshot` over a
  cloned copy of the same payload and reports its result back via a
  `mpsc::Receiver<ScrollbackBuild>`.
- **FR2 — Merge primitive:** Add a `TerminalCore::merge_scrollback_from`
  (working name; final placement decided in `sdd.2-create-plan`) that
  prepends another core's `scrollback_slim` / `scrollback_wrapped`
  rows into `self`, re-interning each `SlimCell`'s `style_id` /
  `char_id` against `self.styles` / `self.chars`.
- **FR3 — Live-drain reconciliation:** Capture
  `scrollback_evicted_total` at 1st-pass swap completion
  (`base_evicted_total`). At merge time, compute
  `live_growth = current_total - base` and drop the **trailing**
  `live_growth` rows from the 2nd-pass result before prepending; those
  rows are already present in the live core from PTY drain that ran
  during the 2nd-pass.
- **FR4 — Polling integration:** Add `Tab::poll_pending_scrollback_restore`
  alongside `poll_pending_switch`, and call it from `App::pump_all` on
  every pump (active and inactive tabs), with non-blocking `try_recv`
  semantics.
- **FR5 — Cancel / supersede:** A new `dispatch_offthread_replay` on
  the same tab sets the previous `PendingScrollbackRestore.cancel` and
  drops the receiver. A grid resize does the same. App shutdown signals
  cancel on all tabs.
- **FR6 — Threshold parity:** When
  `payload.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES`, no 2nd-pass is
  scheduled; the synchronous `reset_frame_for_replay` path remains the
  sole responsible code path. No behavior change for that range.
- **FR7 — Failure tolerance:** `std::thread::spawn` failure and worker
  panic (observed as `Disconnected` on `try_recv`) result in a
  `log::warn!` and the in-flight restore being abandoned. No
  synchronous fallback re-parse is attempted.
- **FR8 — Mark non-duplication:** The 2nd-pass result's
  `prompt_marks` / `fold_marks` / `bypass_b_mark_texts` are NOT applied
  to the live core. Those were already applied during the 1st-pass
  swap.

### Non-Functional Requirements

- **NFR1 — Performance (latency):** 1st-pass swap latency does not
  regress relative to today's bypass-on path. Target: ≤ 60 ms for the
  2 MiB `seq 1 N`-shaped benchmark payload on the project's reference
  machine.
- **NFR2 — Performance (restore):** Single-shot 2nd-pass completes
  within ≤ 5 s for a 2 MiB payload (today's bypass-off measurement is
  ~4040 ms; the budget allows ~1 s for the merge + safety margin).
- **NFR3 — UI non-blocking:** The 2nd-pass worker, the polling, and
  the merge must not block the UI thread for a perceivable duration.
  `try_recv` + per-pump poll is the contract.
- **NFR4 — One worker per tab:** At most one in-flight 2nd-pass per
  tab. The existing "one core per tab" invariant on the displayed core
  is preserved (the rebuilt core is transient and dropped on merge or
  cancel).
- **NFR5 — Monotonicity:** `scrollback_evicted_total` remains monotonic
  through the merge. The merge inserts rows at the **front** without
  bumping the counter (matching its semantics — these rows were not
  evicted, they pre-date the bypass swap).
- **NFR6 — Equivalence:** For any payload, the post-merge observable
  state (`scrollback_slim` contents, `scrollback_wrapped` contents,
  visible grid, `scrollback_evicted_total`) matches the synchronous
  `build_from_snapshot(bypass = false)` reference for the same input.
- **NFR7 — Logging:** 2nd-pass spawn / cancel / completion / failure
  use `log::warn!` or `log::info!`. Release builds persist `warn` and
  above only, so meaningful diagnostics survive.
- **NFR8 — Branch policy:** WebView (`src/`) is not touched. The whole
  change lives in `crates/term_core/` and `src-tauri/src/` per
  `refactor/native-terminal-hybrid` branch policy.

## Implementation Approach

### Architecture

**Component layout (additive boxes marked `+`):**

```
┌────────────────────────────────────────────────────────────────────┐
│ App::pump_all                                                      │
│  ├─ Tab::poll_pending_switch       (existing — 1st-pass swap)      │
│  └─ Tab::poll_pending_scrollback_restore  +  (new — 2nd-pass merge)│
└────────────────────────────────────────────────────────────────────┘
        ▲                                          ▲
        │ try_recv done                            │ try_recv done
        │                                          │
┌────────────────────┐                  ┌──────────────────────────┐
│ off-thread worker  │                  │ off-thread worker        │
│ (1st-pass, bypass) │                  │ (2nd-pass, no bypass) +  │
│ build_from_snapshot│                  │ build_from_snapshot_full │
└────────────────────┘                  └──────────────────────────┘
        │                                          │
        ▼                                          ▼
  SnapshotReplay  ───► apply_offthread_swap ──► PendingScrollbackRestore +
                            │                            │
                            └────► live TerminalCore ◄───┘
                                       ▲
                                       │
                              merge_scrollback_from + (new API on TerminalCore)
```

**Key components (new pieces marked `+`):**

- `PendingScrollbackRestore` (+) — per-tab state for the in-flight
  2nd-pass, modeled after `PendingSwitch`.
- `ScrollbackBuild` (+) — handoff value sent through the mpsc channel
  from the 2nd-pass worker.
- `Tab::poll_pending_scrollback_restore` (+) — non-blocking poll that
  drives the merge or the supersede.
- `Tab::apply_scrollback_restore` (+) — merge implementation.
- `TerminalCore::build_from_snapshot_full` (+) **or** a bypass-flag
  parameter on the existing `build_from_snapshot` — final shape
  decided in `sdd.2-create-plan`. The constraint is that bypass-on
  and bypass-off must remain a single tested code path with one
  difference (the bypass flag).
- `TerminalCore::merge_scrollback_from` (+) — the actual prepend +
  intern primitive.

### Data Flow

**Happy path (payload ≥ 64 KiB):**

```
[user switch event]
       │
       ▼
dispatch_offthread_replay(payload)
       │
       ├──► spawn worker A (bypass = true)
       │
       │ ... worker A finishes (~50ms for 2 MiB) ...
       │
       ▼
poll_pending_switch → SwapOutcome::Swapped
       │
       ▼
apply_offthread_swap
       │
       ├──► live core = bypass-on core (visible grid populated)
       ├──► record base_evicted_total = live.scrollback_evicted_total
       └──► spawn worker B (bypass = false, cancel = AtomicBool)
       │
       │ ... worker B finishes (~4s for 2 MiB) ...
       │
       ▼
poll_pending_scrollback_restore → Ok(ScrollbackBuild)
       │
       ▼
apply_scrollback_restore
       │
       ├──► live_now = live.scrollback_evicted_total
       ├──► live_growth = live_now - base_evicted_total
       ├──► drop trailing live_growth rows from rebuilt scrollback
       ├──► intern each row's SlimCells into live.styles / live.chars
       └──► prepend the intern-rewritten rows to live.scrollback_slim
       │
       ▼
(silent — UI is unchanged unless the user scrolls back)
```

**Supersede paths:**

```
new switch on same tab  ──► set old.cancel ; drop old receiver ; spawn new A
grid resize             ──► set old.cancel ; drop old receiver ; do NOT respawn
app shutdown            ──► set old.cancel ; drop old receiver
worker panic            ──► try_recv → Disconnected ; log::warn! ; clear state
```

### API Design

There is no external API (no HTTP endpoint, no IPC additions). The
internal surface is the in-process Rust API below.

#### `TerminalCore::merge_scrollback_from`

```rust
/// Prepend `other`'s scrollback (`scrollback_slim` / `scrollback_wrapped`)
/// onto `self`, re-interning each `SlimCell`'s `style_id` and `char_id`
/// against `self.styles` / `self.chars`.
///
/// Caller is responsible for trimming `other`'s scrollback to account for
/// live drain that occurred between the 1st-pass swap and this call (see
/// FR3). This function only does the intern-and-prepend.
///
/// Preserves `self.scrollback_evicted_total` (does not bump it — the
/// rows being prepended pre-date the bypass swap and have already been
/// counted at the point they were originally evicted in `other`).
pub(crate) fn merge_scrollback_from(&mut self, other: TerminalCore);
```

#### `TerminalCore::build_from_snapshot_full` (working name)

Either:

```rust
/// Like `build_from_snapshot`, but does NOT enable the bypass.
/// Used by the 2nd-pass scrollback-restore worker.
pub fn build_from_snapshot_full(
    cols: u16,
    rows: u16,
    scrollback_lines: u32,
    payload: &[u8],
    cancel: &std::sync::atomic::AtomicBool,
) -> Option<SnapshotReplay>;
```

— **or** an additional parameter on the existing function. The final
shape is a `sdd.2-create-plan` decision; the contract is the same.

#### `Tab::poll_pending_scrollback_restore`

```rust
/// Non-blocking. Returns one of:
///   - Idle:    no restore in flight
///   - Pending: worker still parsing; no state change
///   - Merged:  worker finished and the scrollback has been merged
///   - Failed:  worker panicked / spawn failed; state cleared
pub(crate) fn poll_pending_scrollback_restore(&mut self)
    -> ScrollbackRestoreOutcome;
```

### Dependencies

**Internal Dependencies:**

- `doc/tasks/snapshot-replay-perf/` — introduced the
  `scrollback_bypass` mechanism this feature reuses.
- `doc/tasks/snapshot-replay-daemon-routing/` — routed real-world
  switches through the bypass path. The known-limitation note in
  its `Out of Scope` section is what this feature retires.
- `crates/term_core` — `TerminalCore`, `SlimCell`, `StyleTable`,
  `CharTable`, `SnapshotReplay`, `build_from_snapshot`.
- `src-tauri/src/tabs.rs` — `Tab`, `PendingSwitch`,
  `dispatch_offthread_replay`, `apply_offthread_swap`,
  `poll_pending_switch`, `App::pump_all`.

**External Dependencies:**

No new crate dependencies. Uses `std::sync::mpsc`,
`std::sync::atomic::AtomicBool`, and `std::thread::spawn`, all already
in use for the 1st-pass.

### File Structure

```
crates/term_core/src/
  terminal_core.rs   # + build_from_snapshot variant (or bypass-flag param)
                     # + merge_scrollback_from
  ring_buffer.rs     # + push_front helpers if needed for merge_scrollback_from

src-tauri/src/
  tabs.rs            # + PendingScrollbackRestore struct
                     # + ScrollbackBuild struct
                     # + Tab::poll_pending_scrollback_restore
                     # + Tab::apply_scrollback_restore
                     # ~ apply_offthread_swap: capture base_evicted_total,
                     #                          spawn 2nd-pass worker at the end
                     # ~ dispatch_offthread_replay: supersede any existing
                     #                              PendingScrollbackRestore
                     # ~ resize path: cancel any in-flight restore
  app/pump.rs        # ~ pump_all: call poll_pending_scrollback_restore

doc/tasks/snapshot-replay-scrollback-restore/
  README.md          # existing follow-up note (kept, links to SPEC)
  要件定義書.md      # this round
  SPEC.md            # this file
  sdd.yaml           # this round
```

## Test Scenarios

### Unit Tests

- [ ] `merge_scrollback_from_intern_rewrites_ids`: Build core A with a
      synthetic `scrollback_slim` whose `style_id`s differ from core B's
      table, call `merge_scrollback_from`, and assert the merged rows
      now reference B's table correctly.
- [ ] `merge_scrollback_from_preserves_evicted_total`: Capture
      `B.scrollback_evicted_total` before the merge, assert unchanged
      after.
- [ ] `merge_scrollback_from_respects_capacity`: When merging would
      exceed `B`'s `scrollback_lines`, the oldest merged rows are
      dropped to fit (or, depending on the merge contract decided in
      `sdd.2`, the result matches what the synchronous path produces
      when its own ring evicts).
- [ ] `build_from_snapshot_full_matches_sync_build`: For a fixed
      payload, `build_from_snapshot_full(... bypass=false ...)`
      produces a `SnapshotReplay` whose core equals a synchronously
      built core in `scrollback_slim`, `scrollback_wrapped`,
      `scrollback_evicted_total`, and visible grid.
- [ ] `bypass_plus_merge_equivalence`: 1st-pass (bypass-on) +
      apply_scrollback_restore (with `live_growth = 0`) yields a state
      observably equal to `build_from_snapshot(bypass=false)`. This is
      the primary contract-parity test for FR1/NFR6.

### Integration Tests (`tabs.rs` level)

- [ ] `offthread_switch_then_scrollback_restored`: drive
      `dispatch_offthread_replay` with a payload ≥ 64 KiB containing
      observable history; spin `poll_pending_switch` to swap; then
      spin `poll_pending_scrollback_restore` until `Merged`; assert
      the resulting `core.scrollback_slim` matches the synchronous
      reference.
- [ ] `offthread_switch_supersede_cancels_restore`: dispatch switch A,
      poll-swap, then immediately dispatch switch B; assert the
      A-restore receiver is dropped and A's payload is not merged.
- [ ] `restore_with_concurrent_live_drain`: between the 1st-pass swap
      and `Merged`, feed live PTY bytes onto the target pane; assert
      the final scrollback contains exactly the live drain *plus* the
      historical rows, with no duplication.
- [ ] `restore_resize_cancel`: trigger a resize between swap and
      `Merged`; assert the restore was cancelled and no rows were
      merged.
- [ ] `restore_worker_panic_warn_and_continue`: inject a worker panic
      (test-only hook on the cancel path or a `panic!`-payload
      fixture) and assert app state is consistent and a `log::warn!`
      was emitted.

### E2E Tests

**Existing E2E tests**: None. There is no `docker-compose.e2e.yml` or
`e2e-tests/` directory in this repo.
**Run command**: Not detected.

- [ ] Manual smoke (user-driven, not automated): switch to a mux
      window with a ~2 MiB scrollback, immediately scroll up after the
      visible grid appears, then again after ~5 s, and confirm
      history becomes visible.

### Edge Cases

- [ ] **Threshold-boundary payload (64 KiB ± 1):** below threshold
      runs synchronously and no 2nd-pass is scheduled; at or above
      threshold takes the bypass + 2nd-pass path.
- [ ] **Live drain larger than 2nd-pass scrollback:** the trimming
      step drops more rows than `other` holds; the merge becomes a
      no-op rather than a panic.
- [ ] **`scrollback_lines` reached before merge:** merge respects the
      ring capacity (test `merge_scrollback_from_respects_capacity`).
- [ ] **Empty scrollback in payload:** 2nd-pass produces a core with
      empty scrollback; merge is a no-op; no warning emitted.

### Performance Tests

- [ ] **Restore bench (new):** `scrollback_restore_bench_2mib_seq` —
      mirror `snapshot_replay_bench_2mib_seq` but measure end-to-end
      `bypass-on swap → 2nd-pass build → merge` total time. Target:
      ≤ 5 s for 2 MiB on the reference machine.
- [ ] **1st-pass non-regression:** `snapshot_replay_bench_2mib_seq`
      continues to report the same per-call time as today (worker B
      spawn must not slow worker A).

## Security Considerations

- **Authentication / Authorization / Input Validation:** No external
  inputs. The snapshot payload originates from the same trust domain
  (the daemon side of the in-process mux); this feature does not widen
  trust.
- **Memory:** A 2nd-pass run instantiates a second `TerminalCore` of
  roughly the same scrollback size as the 1st-pass core. Worst-case
  peak ~ 2× the live core for the duration of the 2nd-pass. Bounded
  by the daemon-side 2 MiB scrollback cap.
- **Thread safety:** The 2nd-pass worker holds no references to the
  live core; the merge runs on the UI thread inside the existing
  `Tab` mutability scope.

## Error Handling

### Failure Modes

| Mode                         | Detection                              | Response                                                       |
| ---------------------------- | -------------------------------------- | -------------------------------------------------------------- |
| `thread::spawn` returns `Err`  | spawn site                             | `log::warn!`; no `PendingScrollbackRestore` created            |
| worker panics                | `try_recv` → `Disconnected`            | `log::warn!`; clear `PendingScrollbackRestore`                 |
| cancel observed mid-build    | `process_pty_data_fully_cancellable`  | worker returns `None`; sender drops; receiver gets `Disconnected` → same as panic path |
| `live_growth` > merged rows  | merge-time arithmetic                  | drop all merged rows; no error                                 |
| merge would overflow ring    | merge-time bookkeeping                 | drop oldest merged rows to fit                                 |

### Error Flow

```
2nd-pass error → log::warn! → clear PendingScrollbackRestore → continue
```

No user-visible error is surfaced. The fallback is "history stays
empty until live drain re-fills it," which matches the pre-feature
status quo.

## Performance Optimization

### Performance Goals

- 1st-pass swap latency (2 MiB): ≤ 60 ms (today's measurement: ~51 ms)
- 2nd-pass + merge total time (2 MiB): ≤ 5 s
- Merge step alone: a single UI frame (`try_recv` poll, then in-frame
  intern + push_front of up to `scrollback_lines` rows)

### Optimization Strategies

- **Reuse the bypass infrastructure**: the 2nd-pass uses the exact same
  `build_from_snapshot` entry point with one different argument. No
  new parser path, no new SlimCell logic.
- **No extra payload copies on the wire**: the 1st-pass already holds
  the raw payload in `PendingSwitch.payload` for resize-supersede
  re-dispatch. The 2nd-pass clones this once.
- **Defer merge work**: the merge runs on the UI thread but only when
  `try_recv` is `Ok`. It is not paid every frame.

### Caching Strategy

Not applicable.

## Success Criteria

- [ ] All FRs implemented and covered by unit + integration tests
- [ ] NFR1 (1st-pass non-regression) verified by
      `snapshot_replay_bench_2mib_seq` continuing to match its pre-feature
      number
- [ ] NFR2 (2nd-pass within budget) verified by new
      `scrollback_restore_bench_2mib_seq`
- [ ] NFR6 (equivalence with synchronous build) verified by
      `bypass_plus_merge_equivalence` unit test
- [ ] Threshold contract drift eliminated: both code paths end in
      observably equivalent state for the same payload
- [ ] `scrollback_evicted_total` monotonicity preserved (asserted in
      the integration tests above)
- [ ] No new `cargo` warnings; `cargo check` and `cargo test --lib`
      pass cleanly
- [ ] CLI-only feature build (`--no-default-features`) is unaffected
      (this feature lives entirely under the GUI path)

## Open Questions

> **Note**: Unresolved items are tracked as `status: tbd` in
> `sdd.yaml`. Resolve them before `/em-sdd:sdd.2-create-plan`.

- [ ] FR2 API placement: standalone `merge_scrollback_from` on
      `TerminalCore` vs. a typed `ScrollbackOverlay` intermediate
      (`tbd_reason`: clearer once the prepend + intern code is
      sketched in `sdd.2-create-plan`).

## Implementation Phases

The whole feature lands in a single phase. There is no incremental
user-facing rollout step.

### Phase 1: 2nd-pass restore

**Goals:** Implement FR1–FR8, satisfy NFR1–NFR8, ship the test
matrix in §Test Scenarios.

**Deliverables:**

- `TerminalCore::merge_scrollback_from` (+ `build_from_snapshot_full`
  or its parameterized equivalent) in `crates/term_core`
- `PendingScrollbackRestore` + polling + merge in `src-tauri/src/tabs.rs`
- `App::pump_all` wiring for the new poll
- `scrollback_restore_bench_2mib_seq` new benchmark
- Unit + integration tests listed above

## References

- Plan report: `tmp/snapshot-replay-scrollback-restore-plan-2026-06-21.md`
- Follow-up note: `doc/tasks/snapshot-replay-scrollback-restore/README.md`
- 要件定義書: `doc/tasks/snapshot-replay-scrollback-restore/要件定義書.md`
- Predecessor — bypass mechanism: `doc/tasks/snapshot-replay-perf/`
- Predecessor — routing: `doc/tasks/snapshot-replay-daemon-routing/`
- Relevant commits: `6b47754`, `9ceac36`
- Code anchors:
  - `src-tauri/src/tabs.rs` — `OFFTHREAD_REPLAY_THRESHOLD_BYTES`,
    `PendingSwitch`, `dispatch_offthread_replay`,
    `apply_offthread_swap`, `poll_pending_switch`, `App::pump_all`
  - `crates/term_core/src/terminal_core.rs` — `build_from_snapshot`,
    `enable_snapshot_bypass`, `disable_snapshot_bypass`,
    `scrollback_slim`, `scrollback_wrapped`, `scrollback_evicted_total`
  - `crates/term_core/src/ring_buffer.rs` — `ring_push_blank` bypass
    branch
  - `crates/term_core/src/bench.rs` — `snapshot_replay_bench_2mib_seq`
