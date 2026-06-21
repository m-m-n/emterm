# Implementation Plan: snapshot-replay-scrollback-restore

## Overview

After a large (≥ 64 KiB) mux snapshot replay, the live core's visible
grid is up-to-date but its `scrollback_slim` is empty (the bypass
suppressed SlimCell compression). This plan adds a **2nd-pass restore
worker** that re-runs `build_from_snapshot` with the bypass disabled on
a background thread, then prepends the resulting scrollback rows onto
the live core without blocking the UI.

## Objectives

- Restore the scrollback after every off-thread (`payload.len() ≥ 64 KiB`)
  snapshot replay without regressing the 1st-pass swap latency.
- Eliminate the threshold-dependent contract drift: both the synchronous
  `reset_frame_for_replay` path and the off-thread path settle in the
  same observable state for the same payload.
- Keep the wire format, the `SnapshotReplay` type, and the existing
  `build_from_snapshot` API source-compatible (additive only).

## Prerequisites

### Development Environment

- Rust toolchain pinned in `rust-toolchain.toml` (rustfmt
  style_edition=2024, formatter via PostToolUse hook).
- Linux host (Windows cross-build via `make win-build` is unaffected).

### Dependencies

- `crates/term_core` — bypass mechanism from
  `doc/tasks/snapshot-replay-perf/` (`scrollback_bypass`,
  `enable_snapshot_bypass`, `disable_snapshot_bypass`,
  `virtual_scrollback_len`).
- `src-tauri/src/tabs.rs` — off-thread switch wiring from
  `doc/tasks/snapshot-replay-daemon-routing/` (`PendingSwitch`,
  `dispatch_offthread_replay`, `apply_offthread_swap`,
  `poll_pending_switch`, `OFFTHREAD_REPLAY_THRESHOLD_BYTES`).
- `src-tauri/src/app.rs` — `App::pump_all` (line 2693), where the
  existing `poll_pending_switch` call lives (line 2766) and the new
  poll will sit beside it.

### No external crate additions

The implementation uses `std::sync::mpsc`,
`std::sync::atomic::AtomicBool`, and `std::thread::spawn` — all already
in use for the 1st-pass.

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition 2024)
- **Concurrency primitives**: `std::sync::mpsc::channel` for handoff,
  `Arc<AtomicBool>` for cooperative cancel, `std::thread::spawn` for the
  worker. Identical to the 1st-pass machinery.
- **Test framework**: `cargo test --lib` (term_core crate-pub helpers
  for white-box tests; tabs.rs `test_*` helpers for tab-level
  integration tests; `--test-threads=1` is required for tabs.rs replay
  tests because they share `std::env`).

### Design Approach

- **Additive only**: the existing bypass-on path (1st-pass) is
  unchanged. A new bypass-off path (2nd-pass) is added that shares the
  same parser entry point.
- **One-method merge primitive on `TerminalCore`** (FR2 decision —
  see below). The 2nd-pass worker produces a full `TerminalCore`; the
  merge consumes it whole.
- **Cancel-on-supersede contract identical to `PendingSwitch`**: a new
  switch on the same tab, a grid resize, or app shutdown signals
  cancel and drops the receiver. Resize does *not* re-dispatch the
  2nd-pass (history-restore is abandoned).
- **Reconciliation via `scrollback_evicted_total` delta**: the
  1st-pass swap records `base_evicted_total`; the merge subtracts the
  live-side growth from the rebuilt core's trailing rows before
  prepending. This is the same accounting primitive
  `apply_replay_reconcile` already uses for the swap.

### FR2 Decision — Merge Primitive API Placement

**Resolution**: standalone method `TerminalCore::merge_scrollback_from`
(no intermediate `ScrollbackOverlay` type).

**Sketch of the contract (NOT executable code — contract description):**

```
TerminalCore::merge_scrollback_from(&mut self, other: TerminalCore)
  Precondition:
    - `self.cols == other.cols` (else: no-op + log::warn)
    - `other.scrollback_bypass == false` (the caller's 2nd-pass build
      ran with bypass off)
    - Caller has already trimmed `other`'s trailing rows for live drain
      (FR3 — see apply_scrollback_restore)
  Postcondition:
    - `other.scrollback_slim` / `other.scrollback_wrapped` rows are
      prepended (push_front) onto `self.scrollback_slim` /
      `self.scrollback_wrapped`, each SlimCell re-interned against
      `self.styles` / `self.chars` (style always; char only when
      `SLIM_FLAG_CHAR_TABLE` is set — inline-ASCII and wide-cont cells
      carry their `char_ref` through unchanged because inline cells
      hold packed UTF-8 directly, not a `CharTable` id)
    - If the prepend would exceed `self.scrollback_capacity`, the
      oldest *merged* rows are dropped (the front-most rows of the
      incoming sequence) — `self`'s existing rows are preserved
    - `self.scrollback_evicted_total` is UNCHANGED (these rows pre-date
      the bypass swap; bumping the counter would violate
      monotonicity-against-already-emitted-deltas)
    - `other` is consumed (drop frees its styles/chars/SlimCells)
```

**Why this shape (vs. a typed `ScrollbackOverlay` intermediate)**:

1. The merge needs four pieces from the 2nd-pass build:
   `scrollback_slim`, `scrollback_wrapped`, `styles`, and `chars`. A
   `ScrollbackOverlay { slim, wrapped, styles, chars }` intermediate
   would carry the same data with no encapsulation gain.
2. `build_from_snapshot` already returns a full `TerminalCore` inside
   `SnapshotReplay`. The 2nd-pass worker can return either a
   `TerminalCore` or a `ScrollbackBuild` wrapper — but in either case
   the *primitive* the live core needs is "consume another core's
   scrollback." Defining the primitive on `TerminalCore` itself keeps
   `crates/term_core` self-contained (no public re-export of an
   intermediate type).
3. The asymmetry with `apply_offthread_swap` (which moves a whole core
   in via `*self.core.lock() = replay.core`) is removed: both the swap
   and the merge consume a whole worker-built core.
4. The intern + push_front loop fits in one private method on
   `TerminalCore` — short enough that an extra projection step costs
   readability rather than buys it.

**ScrollbackBuild wrapper retained**: even though the merge primitive
takes a whole `TerminalCore`, the *channel handoff* from worker to UI
thread carries a small wrapper:

```
ScrollbackBuild {
  rebuilt_core: TerminalCore,
  evicted_total_at_end: u64,   // for FR3 trim arithmetic
}
```

This keeps the worker→UI contract explicit about what bookkeeping the
caller needs, without introducing a typed overlay.

### Component Interaction

```
┌────────────────────────────────────────────────────────────────────┐
│ App::pump_all  (src-tauri/src/app.rs ≈ line 2693)                  │
│  ├─ Tab::poll_pending_switch              (existing — 1st-pass)    │
│  └─ Tab::poll_pending_scrollback_restore  (NEW   — 2nd-pass)       │
└────────────────────────────────────────────────────────────────────┘
        ▲                                          ▲
        │ try_recv done                            │ try_recv done
        │                                          │
┌────────────────────┐                  ┌──────────────────────────┐
│ off-thread worker  │                  │ off-thread worker        │
│ build_from_snapshot│                  │ build_scrollback_only_   │
│  (bypass = true)   │                  │   from_snapshot          │
└────────────────────┘                  │  (bypass = false) — NEW  │
        │                               └──────────────────────────┘
        ▼                                          │
  SnapshotReplay                                   ▼
        │                                ScrollbackBuild
        ▼                                          │
  apply_offthread_swap ──────► PendingScrollbackRestore (NEW)
        │                                          │
        │ (records base_evicted_total              │
        │  + spawns 2nd-pass worker)               │
        ▼                                          ▼
  live TerminalCore  ◄────  apply_scrollback_restore (NEW)
                                       │
                                       └─► merge_scrollback_from
                                           (TerminalCore method, NEW)
```

## Implementation Phases

### Phase 1: term_core merge primitive + bypass-off build entry

**Goal**: Land the merge primitive and the bypass-off build entry on
`TerminalCore` with unit-level coverage of the equivalence contract
(NFR6), independent of the tab wiring. The tab layer (Phase 2) plugs
into these.

**Files to Modify**:

- `crates/term_core/src/terminal_core.rs` —
  - Add `build_scrollback_only_from_snapshot` (working name) alongside
    the existing `build_from_snapshot`. Same signature + same parser
    entry point; the only behavior difference is that
    `enable_snapshot_bypass` is NOT called. The two functions are
    sibling thin wrappers over a private helper that owns the shared
    "reset + drain + take marks + assemble SnapshotReplay" recipe with
    a `bypass: bool` parameter.
  - Add `merge_scrollback_from(&mut self, other: TerminalCore)` per the
    contract sketched above.
- `crates/term_core/src/ring_buffer.rs` —
  - Add a private `prepend_scrollback_rows` helper that push_fronts a
    sequence of (SlimCell row, wrapped flag) pairs onto
    `self.scrollback_slim` / `self.scrollback_wrapped`, respecting
    `scrollback_capacity` (dropping the front-most *incoming* rows
    when the combined length would overflow). Used by
    `merge_scrollback_from`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `build_scrollback_only_from_snapshot` | bypass-off build entry for the 2nd-pass worker | Same as `build_from_snapshot` (fresh core, payload, cancel) | Returns `SnapshotReplay` whose `core.scrollback_slim` is populated; `bypass_b_mark_texts` may be empty (caller MUST ignore — FR8) |
| `merge_scrollback_from` (method on `TerminalCore`) | Prepend `other`'s scrollback onto `self`, re-interning ids | `self.cols == other.cols`; `other.scrollback_bypass == false`; caller pre-trimmed for live drain | Rows prepended; `scrollback_evicted_total` unchanged; `other` consumed |
| `prepend_scrollback_rows` (private on `RingBuffer`/`TerminalCore`) | Capacity-respecting push_front of a row sequence | New row count + existing count may exceed capacity | Front-most *incoming* rows dropped to fit; existing rows preserved |

**Processing Flow** (`merge_scrollback_from` — diagram-convertible):

1. Validate preconditions
   - `self.cols == other.cols` → continue
   - mismatch → log::warn, return (no-op)
2. For each (row, wrapped) pair in `other.scrollback_slim` /
   `other.scrollback_wrapped` (oldest first), build a re-interned row.
   The re-intern loop mirrors the **same flag dispatch as
   `release_slim_row`** (`ring_buffer.rs:228`) so refcount accounting
   stays symmetric.
   - For each SlimCell in the row:
     - **Style**: id 0 (the pinned default) maps to id 0 unchanged;
       otherwise look up the `StyleEntry` in `other.styles`
       via `get_or_default(style_id)`, then
       `let new_style_id = self.styles.intern(entry)`.
     - **char_ref**: dispatch on `flags`:
       - `SLIM_FLAG_INLINE_ASCII` set → `char_ref` already holds the
         packed UTF-8 bytes (1–4 B). Copy `char_ref` as-is; **do NOT
         touch `self.chars`**. (Inline cells are not interned.)
       - `SLIM_FLAG_CHAR_TABLE` set → `char_ref` is a `CharTable` id in
         `other.chars`. Resolve via
         `other.chars.get_or_default(char_ref)` (returns `&str`), then
         `let new_char_id = self.chars.intern(s)`. Replace `char_ref`
         with `new_char_id`.
       - `SLIM_FLAG_WIDE_CONT` set (and neither of the above) →
         `char_ref` is unused. Copy as-is.
     - emit a SlimCell with `style_id = new_style_id`, the rewritten
       `char_ref`, and `width`/`flags` copied unchanged.
3. Pass the re-interned rows + wrapped flags to
   `prepend_scrollback_rows` for capacity-aware push_front.
4. `self.scrollback_evicted_total` is NOT touched (NFR5 monotonicity).
5. `other` falls out of scope and is dropped.

**Implementation Steps** (5 max):

1. **Extract shared build helper** — refactor `build_from_snapshot` so
   its body becomes a thin wrapper over a private
   `build_from_snapshot_inner(bypass: bool, …)`. No behavior change for
   the existing call sites.
2. **Add `build_scrollback_only_from_snapshot`** — second thin wrapper
   over the same inner helper with `bypass = false`. Public, same
   signature as `build_from_snapshot`.
3. **Add `prepend_scrollback_rows`** in `ring_buffer.rs` —
   capacity-aware push_front. Returns the number of incoming rows
   actually inserted (caller logs the dropped count at `info!`).
4. **Add `merge_scrollback_from`** — re-intern loop + delegate to
   `prepend_scrollback_rows`. Asserts `self.cols == other.cols` only
   in debug; release logs warn + returns.
5. **Add tests** (see Testing Approach below).

**Dependencies**: Requires the bypass mechanism (already shipped).
Blocks Phase 2.

**Testing Approach**:

- Unit (`crates/term_core/src/{terminal_core,ring_buffer}.rs` tests
  module):
  - `merge_scrollback_from_intern_rewrites_ids`: build core A with a
    synthetic scrollback row whose style_id/char_id differ from B's
    tables; merge into B; assert merged row's ids resolve in B's
    tables to the same observable styles/chars.
  - `merge_scrollback_from_preserves_evicted_total`: snapshot
    `B.scrollback_evicted_total` before merge, assert unchanged after.
  - `merge_scrollback_from_respects_capacity`: merge N rows into a
    nearly-full ring; assert ring length == capacity and the
    front-most *incoming* rows were dropped (B's existing rows kept).
  - `merge_scrollback_from_cols_mismatch_is_noop`: build A at width 80,
    B at width 100; merge; assert B unchanged.
  - `build_scrollback_only_from_snapshot_matches_sync_build`: same
    payload, compare `scrollback_slim`, `scrollback_wrapped`,
    `scrollback_evicted_total`, and viewport grid against a
    synchronously-built reference core. **The contract-parity test
    for NFR6.**
  - `bypass_plus_merge_equivalence`: run `build_from_snapshot` (bypass
    on) + `merge_scrollback_from(build_scrollback_only_from_snapshot)`
    with `live_growth = 0`; assert state is observably equal to
    `build_scrollback_only_from_snapshot` alone. **The primary FR1 /
    NFR6 unit gate.**

**Acceptance Criteria**:

- [ ] `cargo test --lib` passes including all new tests
- [ ] No new `cargo check` warnings
- [ ] `build_from_snapshot` callers untouched (refactor was internal
      only)
- [ ] `--no-default-features` (CLI) build still compiles
      (`term_core` is part of the CLI path)

**Estimated Effort**: medium

---

### Phase 2: tabs.rs PendingScrollbackRestore + poll + merge

**Goal**: Wire the 2nd-pass worker into `Tab`, plumb the polling
through `App::pump_all`, and reconcile against live drain. Cover the
supersede / resize / panic paths and the threshold parity. After this
phase the end-to-end happy path works on a real mux switch.

**Files to Modify**:

- `src-tauri/src/tabs.rs` —
  - Add `PendingScrollbackRestore` struct beside `PendingSwitch`.
  - Add `ScrollbackBuild` struct (carries `rebuilt_core` and
    `evicted_total_at_end`).
  - Add `ScrollbackRestoreOutcome` enum (Idle / Pending / Merged /
    Failed) parallel to `SwapOutcome`.
  - Add field `pending_scrollback_restore:
    Option<PendingScrollbackRestore>` on `Tab`.
  - Extend `apply_offthread_swap` (line 751): at the end of the swap
    (after `apply_queued_live_output` so `scrollback_evicted_total` is
    settled), capture `base_evicted_total` and spawn the 2nd-pass
    worker thread; install `PendingScrollbackRestore`.
  - Extend `dispatch_offthread_replay` (line 623): cancel any existing
    `pending_scrollback_restore` (set its `cancel` to true, drop the
    Option). This is the supersede arm (FR5).
  - Extend `Tab::resize` (line 2545): cancel any existing
    `pending_scrollback_restore` and drop it. Do NOT respawn the
    2nd-pass at the new grid (history-restore is abandoned per UC03).
  - Add `Tab::poll_pending_scrollback_restore` — non-blocking
    `try_recv`; on `Ok` calls `apply_scrollback_restore` (consumes
    `ScrollbackBuild`, calls `merge_scrollback_from` on the live core
    after FR3 trim); on `Disconnected` clears state with `log::warn`.
  - Add `Tab::apply_scrollback_restore` — the FR3 trim arithmetic
    (`live_growth = live_now - base_evicted_total`; drop trailing
    `live_growth` rows from `rebuilt_core.scrollback_slim` /
    `scrollback_wrapped` *before* the merge) and the
    `merge_scrollback_from` call. Hold `core.lock()` only for the
    snapshot of `live_now` and again for the merge — not across
    intern work that doesn't need it.
  - Add test helpers: `test_has_pending_scrollback_restore`,
    `test_drain_pending_scrollback_restore_for_blocking_recv`.
- `src-tauri/src/app.rs` —
  - In `App::pump_all` (line 2693), after the existing
    `poll_pending_switch` arm (line 2766), call
    `tab.poll_pending_scrollback_restore` for every tab. A `Merged`
    or `Failed` outcome sets `changed = true` and, for the active
    tab, `active_changed = true` (so search overlay rebuild fires —
    the buffer changed). No `active_offthread_swapped` analogue is
    needed: the merge doesn't touch the viewport, so no full redraw
    or scroll restore is required (the search overlay rebuild on
    `active_changed` is the only consumer).
- `src-tauri/src/window_host.rs` —
  - On `WindowEvent::CloseRequested` (line 2146), **before** the
    existing `self.app.tabs.clear()` (line 2153), iterate
    `self.app.tabs` and call `Tab::cancel_pending_scrollback_restore`
    (new tiny helper on `Tab` that stores `true` into
    `pending_scrollback_restore.as_ref().map(|p| &p.cancel)`). The
    `pending_scrollback_restore` Option is then dropped by
    `tabs.clear()`, the receiver disappears, and the worker either
    observes cancel at the next chunk boundary or completes and
    drops the orphan `ScrollbackBuild` on `send`. Best-effort; no
    join. **Rationale**: drop alone does NOT fire the cancel flag
    (the worker holds an `Arc<AtomicBool>` independently of the
    receiver), so an explicit cancel store is required to bound
    wasted worker CPU on shutdown. An equivalent alternative is a
    `Drop` impl on `PendingScrollbackRestore` that sets the cancel
    flag; we prefer the explicit-helper shape so the call site
    documents the intent and stays consistent with how
    `dispatch_offthread_replay` cancels the 1st-pass.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `PendingScrollbackRestore` | Per-tab state for the in-flight 2nd-pass | At most one per tab; cleared on Merged/Failed/cancel | `done` receiver drained or cancel observed |
| `ScrollbackBuild` | Worker→UI handoff payload | `rebuilt_core` built with bypass off | Caller calls merge or drops |
| `ScrollbackRestoreOutcome` | Polling result | — | One of Idle / Pending / Merged / Failed |
| `Tab::poll_pending_scrollback_restore` | Non-blocking poll + dispatch | — | On Merged: scrollback prepended; on Failed: warn + state cleared |
| `Tab::apply_scrollback_restore` | FR3 trim + invoke `merge_scrollback_from` | `ScrollbackBuild` available | Live core scrollback updated; `pending_scrollback_restore` cleared |

**Processing Flow** (`apply_offthread_swap` extension, FR1):

1. Existing swap work completes (live_queue replayed, reconcile done).
2. Read `live.get_scrollback_evicted_total()` → `base_evicted_total`.
3. Read `(cols, rows, scrollback_capacity)` from the now-swapped core.
4. Clone the 1st-pass `payload` (it is still owned by the consumed
   `PendingSwitch`; the existing resize-supersede path already keeps
   it for this reason).
5. Allocate `cancel = Arc<AtomicBool>` and an mpsc channel.
6. Try `std::thread::Builder::name("mux-scrollback-restore").spawn(...)`:
   - In the worker: call
     `build_scrollback_only_from_snapshot(cols, rows, scrollback_lines,
     &payload, &cancel)`.
   - On `Some(replay)`: send `ScrollbackBuild { rebuilt_core:
     replay.core, evicted_total_at_end: replay.evicted_total }`.
   - On `None` (cancelled mid-parse): drop `tx`.
7. On spawn success: install `PendingScrollbackRestore`.
8. On spawn failure: log::warn, do NOT install. (FR7)

**Processing Flow** (`apply_scrollback_restore` merge, FR3):

1. `try_recv` returned `Ok(ScrollbackBuild { rebuilt_core, evicted_total_at_end })`.
2. Snapshot `live_now = self.core.lock().get_scrollback_evicted_total()`.
3. Compute `live_growth = live_now.saturating_sub(base_evicted_total)`.
4. If `live_growth >= rebuilt_core.scrollback_count()`: full no-op
   (everything was already drained live). Clear state, log::info, done.
5. Else: drop trailing `live_growth` rows from
   `rebuilt_core.scrollback_slim` and `scrollback_wrapped`
   (`truncate(len - live_growth)`).
6. Acquire `core.lock()`, call
   `live_core.merge_scrollback_from(rebuilt_core)`. (The lock is held
   only for the merge call itself.)
7. Clear `pending_scrollback_restore`, log::info with merged row count.

**Processing Flow** (`Tab::resize` extension, FR5):

1. Existing resize logic runs (PTY resize, core resize, pending_switch
   re-dispatch).
2. If `pending_scrollback_restore.is_some()`:
   - Set its `cancel = true`.
   - Drop the Option.
   - No respawn (history-restore is abandoned per UC03).

**Implementation Steps** (5–7 max):

1. **Define the three new types** (`PendingScrollbackRestore`,
   `ScrollbackBuild`, `ScrollbackRestoreOutcome`) at the top of
   `tabs.rs` alongside `PendingSwitch` / `SwapOutcome`.
2. **Spawn the worker at the end of `apply_offthread_swap`** —
   capture `base_evicted_total` *after* the live_queue replay, clone
   the payload, spawn, install state. spawn-fail = `log::warn` + no
   state.
3. **Implement `poll_pending_scrollback_restore`** — `try_recv`
   dispatch to `apply_scrollback_restore` (Ok), warn + clear
   (Disconnected), or Pending (Empty).
4. **Implement `apply_scrollback_restore`** — FR3 trim + merge call.
5. **Cancel paths**: extend `dispatch_offthread_replay` and
   `Tab::resize` to signal cancel + drop the pending state.
6. **Wire `App::pump_all`** — call the new poll for every tab right
   after the existing `poll_pending_switch` match; route Merged →
   `changed = true` (+ `active_changed` for the active tab). **Also**
   in `src-tauri/src/window_host.rs` (line 2146,
   `WindowEvent::CloseRequested`), insert a cancel sweep over
   `self.app.tabs` immediately before the existing
   `self.app.tabs.clear()` call.
7. **Add `tabs.rs`-level integration tests** (see Testing Approach).

**Dependencies**: Requires Phase 1's
`build_scrollback_only_from_snapshot` and
`TerminalCore::merge_scrollback_from`. Blocks Phase 3.

**Testing Approach**:

- Integration (in the `#[cfg(test)] mod tests` of `tabs.rs`,
  the existing replay test pattern — payload feed via a helper +
  blocking `recv` on a test-only drain):
  - `offthread_switch_then_scrollback_restored`: dispatch a 65 KiB+
    payload; spin `poll_pending_switch` to swap; wait on the
    scrollback-restore receiver; assert post-merge `scrollback_slim`
    matches the synchronous reference for the same payload.
  - `offthread_switch_supersede_cancels_restore`: dispatch A,
    poll-swap, then dispatch B; assert A's restore receiver was
    dropped before merge; assert A's payload is not in the final
    scrollback.
  - `restore_with_concurrent_live_drain`: between swap and Merged,
    feed live PTY bytes; assert final scrollback = (live drain rows
    appended) ∪ (restored historical rows prepended), with **no
    duplication** (FR3 contract).
  - `restore_resize_cancel`: trigger a `Tab::resize` between swap and
    Merged; assert `pending_scrollback_restore` cleared; assert no
    rows merged. (UC03)
  - `restore_worker_panic_warn_and_continue`: inject a fault (a
    test-only mode toggling a feature or via a payload fixture that
    triggers a panic in the parser; if no such fixture is feasible,
    use a custom mpsc setup that mimics `Disconnected` to exercise
    the `Disconnected` arm of `poll_pending_scrollback_restore`);
    assert `pending_scrollback_restore` cleared.
  - `threshold_boundary_no_restore_below`: payload of exactly
    `OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1` → synchronous path → no
    `pending_scrollback_restore` installed. (FR6)
  - `threshold_boundary_restore_at_or_above`: payload of exactly
    `OFFTHREAD_REPLAY_THRESHOLD_BYTES` → off-thread path → restore
    installed.

- Unit (added in this phase, on `Tab` private logic where exposing
  state is straightforward):
  - `apply_scrollback_restore_live_growth_exceeds_drops_all`: synthesize
    a `ScrollbackBuild` with 100 rows, set `live_growth = 150`; assert
    the merge is a no-op and the call returns cleanly.

**Acceptance Criteria**:

- [ ] All integration tests above pass under `cargo test --lib
      --test-threads=1`
- [ ] `App::pump_all` calls the new poll on every pump (active and
      background tabs)
- [ ] Resize during restore cancels the restore (no rows merged)
- [ ] No new `cargo check` warnings; format clean

**Estimated Effort**: large

---

### Phase 3: bench + observability + cleanup

**Goal**: Land the performance non-regression gates (NFR1, NFR2) as
runnable benches, add the operational `log::warn!` / `log::info!`
emissions called out in NFR7, and confirm CLI-only build is
unaffected.

**Files to Modify**:

- `crates/term_core/src/bench.rs` — add `scrollback_restore_bench_2mib_seq`:
  - Build the same 2 MiB seq-N payload as
    `snapshot_replay_bench_2mib_seq`.
  - Measure end-to-end: `build_from_snapshot` (bypass on) +
    `build_scrollback_only_from_snapshot` (bypass off) +
    `merge_scrollback_from`.
  - Assert: per-call total < 5 s (NFR2). The existing 1 s threshold on
    the bypass-on call alone is left in place as NFR1's non-regression
    gate.
  - Same `#[ignore]` gating as `snapshot_replay_bench_2mib_seq`; same
    invocation conventions.
- `src-tauri/src/tabs.rs` — operational logging:
  - `log::info!` at 2nd-pass spawn ("scrollback restore worker spawned
    for tab {…}, payload {N} B").
  - `log::info!` at successful merge ("scrollback restored: {N} rows
    prepended").
  - `log::warn!` at spawn-fail, panic-disconnect, cols-mismatch
    (defensive), and cancel-on-supersede / cancel-on-resize.
  - Drop any redundant lines; release builds persist `warn` and above
    only, so the spawn + merge `info!` lines surface only at the
    `RUST_LOG=info` opt-in.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `scrollback_restore_bench_2mib_seq` | NFR2 gate | Reference machine | Reports per-call time; asserts < 5 s |
| Existing `snapshot_replay_bench_2mib_seq` | NFR1 non-regression gate (unchanged) | — | Continues to assert < 1 s |
| Operational logs | NFR7 traceability | — | warn/info emitted at every state transition |

**Implementation Steps** (3 max):

1. **Add the new bench** mirroring the existing one's structure.
2. **Audit the new `tabs.rs` code paths** for log coverage; add
   missing `log::info!` / `log::warn!`.
3. **Run `--no-default-features` `cargo check`** to confirm the CLI
   build is unaffected (term_core is part of the CLI path but the new
   `merge_scrollback_from` and `build_scrollback_only_from_snapshot`
   are dead-code-pruned in CLI mode, since neither
   `dispatch_offthread_replay` nor the bench is referenced).

**Dependencies**: Requires Phase 2.

**Testing Approach**:

- Bench: `cargo test --release -- snapshot_replay_bench_2mib_seq
  --nocapture --include-ignored` (NFR1 — non-regression) and
  `scrollback_restore_bench_2mib_seq` (NFR2 — new gate).
- Manual log inspection: run `RUST_LOG=info` against a real mux
  switch ≥ 64 KiB and confirm spawn / merge lines appear; confirm a
  rapid double-switch produces the supersede warn.

**Acceptance Criteria**:

- [ ] `scrollback_restore_bench_2mib_seq` passes its < 5 s assertion
- [ ] `snapshot_replay_bench_2mib_seq` matches its pre-feature
      per-call time within noise (NFR1)
- [ ] `--no-default-features` `cargo check` is clean
- [ ] Every state-transition in `PendingScrollbackRestore`'s
      lifecycle is covered by a `log::warn!` or `log::info!` (NFR7)

**Estimated Effort**: small

---

## Complete File Structure

```
crates/term_core/src/
  terminal_core.rs        # + build_scrollback_only_from_snapshot
                          # + private build_from_snapshot_inner (shared)
                          # + merge_scrollback_from method
  ring_buffer.rs          # + prepend_scrollback_rows (capacity-aware
                          #   push_front)
  bench.rs                # + scrollback_restore_bench_2mib_seq

src-tauri/src/
  tabs.rs                 # + PendingScrollbackRestore struct
                          # + ScrollbackBuild struct
                          # + ScrollbackRestoreOutcome enum
                          # + Tab.pending_scrollback_restore field
                          # + Tab::poll_pending_scrollback_restore
                          # + Tab::apply_scrollback_restore
                          # ~ apply_offthread_swap: spawn 2nd-pass at end
                          # ~ dispatch_offthread_replay: supersede
                          # ~ Tab::resize: cancel + drop, no respawn
                          # + test_has_pending_scrollback_restore helper
  app.rs                  # ~ pump_all: call poll_pending_scrollback_restore
                          #            after poll_pending_switch
  window_host.rs          # ~ CloseRequested: cancel restores best-effort
                          #   right before self.app.tabs.clear()

doc/tasks/snapshot-replay-scrollback-restore/
  IMPLEMENTATION.md       # this file
  VERIFICATION.md         # the verification plan
  tasks.yaml              # phase / requirement mapping
  sdd.yaml                # FR2 status: tbd → ok
```

## Testing Strategy

- **Unit (term_core)**: NFR6 equivalence and the merge primitive
  contract are gated here. Coverage target ≥ 90% on
  `merge_scrollback_from` and `build_scrollback_only_from_snapshot`.
- **Integration (tabs.rs)**: end-to-end happy path + the four
  cancel/failure paths. Run with `--test-threads=1` because tabs.rs
  replay tests touch shared state (per project memory:
  `project_test_execution_notes`).
- **Bench (term_core)**: NFR1 (existing,
  `snapshot_replay_bench_2mib_seq`, asserts < 1 s) and NFR2 (new,
  `scrollback_restore_bench_2mib_seq`, asserts < 5 s). Both gated
  `#[ignore]`; run on demand.
- **Manual**: one user-driven smoke per `README.md` (switch to a mux
  window with ~2 MiB scrollback, scroll up after the visible grid
  paints, then again after ~5 s). VERIFICATION.md documents this.
- **No E2E**: project has no `docker-compose.e2e.yml`; not applicable.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `std::sync::mpsc` | std | worker→UI handoff |
| `std::sync::atomic::AtomicBool` | std | cooperative cancel |
| `std::thread::spawn` | std | 2nd-pass worker |
| `log` | already in tree | NFR7 emissions |

No new crate dependencies.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `StyleTable` / `CharTable` id mis-remap on merge corrupts scrollback rendering | Medium | High | `merge_scrollback_from_intern_rewrites_ids` unit test + `bypass_plus_merge_equivalence` NFR6 gate |
| Live-drain reconciliation off-by-one duplicates rows | Medium | Medium | `restore_with_concurrent_live_drain` integration test; FR3 arithmetic via `saturating_sub` |
| Worker panic leaks the worker thread or leaves `cancel` set forever | Low | Low | mpsc `Disconnected` clears state; `cancel` Arc drops with the receiver |
| `scrollback_evicted_total` accidentally bumped during merge | Low | High (NFR5 violation) | `merge_scrollback_from_preserves_evicted_total` unit test |
| Bench machine variance makes NFR2 (< 5 s) flake | Medium | Low | 1 s headroom over the 4040 ms measured baseline; bench is `#[ignore]` so CI is not bound to it |
| Memory peak doubles during 2nd-pass | Low | Low | Documented in SPEC §Security; scrollback cap is 2 MiB; second core is dropped on merge/cancel |

## Open Questions

- [ ] None remaining for FR2 (resolved above to standalone
      `merge_scrollback_from` method on `TerminalCore`).
- [ ] Reference-machine 2nd-pass measurement is currently ~4040 ms
      (per `snapshot_replay_bench_2mib_seq` bypass-off run); the
      < 5 s NFR2 budget includes the merge cost. If the merge alone
      exceeds 500 ms on the reference machine, revisit the
      re-intern loop (this is tracked under §10.1 of 要件定義書.md as
      a 中-priority risk).

## Success Metrics

- [ ] All FRs implemented and covered by unit + integration tests
- [ ] NFR1 (1st-pass non-regression) verified by
      `snapshot_replay_bench_2mib_seq` matching its pre-feature time
- [ ] NFR2 (2nd-pass within budget) verified by
      `scrollback_restore_bench_2mib_seq`
- [ ] NFR6 (equivalence with synchronous build) verified by
      `bypass_plus_merge_equivalence` + `build_scrollback_only_from_snapshot_matches_sync_build`
- [ ] Threshold contract drift eliminated: both code paths end in
      observably equivalent state for the same payload (FR6 +
      threshold boundary tests)
- [ ] `scrollback_evicted_total` monotonicity preserved (NFR5;
      asserted in unit + integration tests)
- [ ] No new `cargo` warnings; `cargo check` and `cargo test --lib`
      pass cleanly
- [ ] CLI-only build (`--no-default-features`) unaffected
- [ ] WebView (`src/`) untouched (NFR8)
