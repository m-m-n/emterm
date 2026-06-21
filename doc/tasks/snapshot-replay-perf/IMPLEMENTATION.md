# Implementation Plan: snapshot-replay-perf

## Overview

Make mux tab-switching feel sub-second by bypassing the per-row SlimCell compression hot loop inside `ring_push_blank` for the duration of a snapshot replay, then restoring normal scrollback retention so subsequent live PTY output keeps accumulating into scrollback as it does today.

## Objectives

- Bring `snapshot_replay_bench_2mib_seq` per-call from ~4040 ms to < 1000 ms (MUST), targeting < 200 ms (SHOULD).
- Preserve every externally observable contract of `TerminalCore::build_from_snapshot`: returned grid contents, `SnapshotReplay.evicted_total`, prompt/fold mark `abs_row` + `evicted_total` stamping, `cancel` semantics, empty-payload behavior.
- Add `assert!` thresholds to the existing `#[ignore]` benches so CI flags a regression rather than silently letting the perf rot.

## Prerequisites

### Development Environment

- Rust toolchain pinned by `rust-toolchain` (already present)
- `bun` for the GUI build (only required for full release; not required for the changes in this feature)

### Dependencies

- Existing `crates/term_core` (modified in place)
- Existing `src-tauri/src/mux/` bench modules (assert-only additions)
- No new external crates

## Architecture Overview

### Technology Stack

- **Language**: Rust (detected from `crates/term_core/Cargo.toml`, `src-tauri/Cargo.toml`)
- **Framework**: in-tree workspace; no framework dependency
- **Key Libraries**: `std` only for the changes proper; the existing `term_core` internal types (`RingBuffer`, `SlimCell`, `StyleTable`, `CharTable`)

### Design Approach

A snapshot replay is a closed-form operation: it owns a fresh `TerminalCore`, drains a finite byte payload, then hands the core back to the caller. During that window the client-side scrollback **content** does not need to exist — viewport-only restoration is acceptable (confirmed in `SPEC.md §1.3` / 要件定義書 §1.3). The scrollback **bookkeeping** (`scrollback_evicted_total`, the value `get_scrollback_length()` returns, `PendingPromptMark.abs_row` / `.evicted_total`, `PendingFoldMark.abs_row` / `.evicted_total`) MUST be byte-identical to today, because the consumer in `src-tauri/src/tabs.rs::backfill_prompt_marks` derives mark placement via `row = m.abs_row - (current_evicted_total - m.evicted_total)`. A divergence in those numbers shifts every mark by the same amount, dropping early marks and misplacing late ones.

So the bypass is a "replay-mode" inside `ring_push_blank`'s per-row eviction path that:

1. Skips the SlimCell intern (`cell_to_slim`) loop — this is the 4 s sink.
2. Skips push/pop on `scrollback_slim` / `scrollback_wrapped` (so the deque stays empty, no allocation, no `release_slim_row` dec-ref loop on eviction).
3. **Maintains a virtual scrollback length** internally so that the existing
   `get_scrollback_length()` return value and the existing
   `scrollback_evicted_total` increment cadence are byte-identical to today's
   path — see Design Decision D1.

After the drain, the bypass is disabled and the live `scrollback_capacity` is the caller-requested `scrollback_lines`, so PTY output appended after the swap accumulates into scrollback exactly as it does today.

### Design Decisions

**D1 — Preserving observable bookkeeping via a "virtual scrollback count".**
Under the current implementation, after a replay that scrolls `S` viewport rows off the top into a core with capacity `C`:
- `scrollback_count() == min(S, C)`
- `scrollback_evicted_total == max(0, S - C)`
- A mark stamped at scroll index `N` carries
  `abs_row = min(N, C) + cursor.row` and `evicted_total = max(0, N - C)`
- The caller installs `evicted_total` as `evicted_baseline` and uses
  `row = m.abs_row - (current_evicted_total - m.evicted_total)` to
  normalize marks into the current frame.

The bypass MUST produce the same four values on the same payload + the same `C`. Concretely the bypass maintains an internal scalar `virtual_scrollback_len: u32` (alongside the existing `scrollback_capacity` and `scrollback_evicted_total`) that:

1. Resets to `0` when the bypass is enabled.
2. On each scroll-off (i.e. each `ring_push_blank` eviction step taken while the bypass is on):
   - If `virtual_scrollback_len < scrollback_capacity` → increment `virtual_scrollback_len`.
   - Else (`virtual_scrollback_len == scrollback_capacity`) → increment `scrollback_evicted_total`.
3. While the bypass is on, `get_scrollback_length()` returns `virtual_scrollback_len` rather than `scrollback_count() as u32`.

This makes the stamping site (`abs_row = get_scrollback_length() + cursor.row`, `evicted_total = scrollback_evicted_total`) byte-identical to today on every mark, and the returned `SnapshotReplay.evicted_total` byte-identical to today on every payload. The only observable difference is the post-replay `scrollback_count()` (= `0`, because the SlimCell data was intentionally not retained), which is the documented spec change (FR2).

This bypass deliberately diverges from the live `scrollback_capacity == 0` branch: that path silently drops scroll-offs without counting them or maintaining any virtual length. The bypass keeps both counters faithful.

**D2 — Where the bypass switch lives.**
The switch is internal to `term_core`. `build_from_snapshot` is the only entry point that flips it on, drains, then flips it off. No external caller sees a new parameter; FR3 (signature + observable contract preservation) is satisfied. Implementation may realize the switch as either a `RingBuffer` boolean field or a parameterized eviction path; the interface to `build_from_snapshot` does not change either way. The `virtual_scrollback_len` field lives on `RingBuffer` (alongside `scrollback_capacity` / `scrollback_evicted_total`) so the eviction step and `get_scrollback_length()` can both see it without further wiring.

**D3 — Capacity is constant across the bypass window.**
There is no "capacity promotion" step. The fresh core is constructed with `TerminalCore::new(cols, rows, scrollback_lines)` (the caller's requested capacity), the bypass is enabled, the drain runs (with the bypass causing scrollback content to be skipped), the bypass is disabled, and the core is returned. `scrollback_capacity` is the caller-requested value throughout. The bypass is purely about *what work* the eviction step does, not about what `scrollback_capacity` is set to.

**D4 — Capacity restoration timing.**
Because D3 keeps `scrollback_capacity` constant, there is no restoration step. The TerminalCore the caller receives already has `scrollback_capacity == scrollback_lines`. The `SnapshotReplay` is constructed in the existing order (drain → snapshot drain outputs → return), with the bypass disabled before the return so any subsequent operations on the returned core (live mode) take the normal eviction branches.

### Component Interaction

```
                   ┌───────────────────────────────────┐
caller (mux) ────► │ TerminalCore::build_from_snapshot │
                   └─────────────────┬─────────────────┘
                                     │
                  ┌──────────────────┼──────────────────┐
                  ▼                  ▼                  ▼
        TerminalCore::new   replay-mode ON         drain payload via
        (cols, rows,         (RingBuffer)          process_pty_data_fully_cancellable
         scrollback_lines)         │                     │
                                   │              (many ring_push_blank calls
                                   │               that take the bypass branch:
                                   │               skip intern, skip slim ops,
                                   │               still bump evicted_total)
                                   ▼                     │
                          replay-mode OFF ◄──────────────┘
                                   │
                                   ▼
                        return SnapshotReplay{ core, actions, evicted_total,
                                                 prompt_marks, fold_marks }
```

## Implementation Phases

### Phase 1: Add the ring-buffer replay-mode bypass

**Goal**: A toggleable bypass inside `ring_push_blank`'s eviction step that skips the SlimCell intern + `scrollback_slim` push/pop work while maintaining a virtual scrollback length so that `get_scrollback_length()` and `scrollback_evicted_total` evolve byte-identically to today's path (D1). With the bypass off, `ring_push_blank` and `get_scrollback_length()` are byte-identical to today.

**Files to Modify**:
- `crates/term_core/src/ring_buffer.rs` — add the bypass branch inside the eviction step of `ring_push_blank`, add the `virtual_scrollback_len` field on `RingBuffer`, branch `get_scrollback_length()` to return the virtual count when the bypass is on.
- `crates/term_core/src/terminal_core.rs` — add a private setter/clearer the replay entry point can call to toggle the bypass; no public API change.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `RingBuffer` bypass flag | Marks the eviction path as "skip SlimCell ops, maintain virtual length" | `false` outside of replay | While `true`, `ring_push_blank` evicts without SlimCell intern / `scrollback_slim` push/pop; `virtual_scrollback_len` and `scrollback_evicted_total` evolve as D1 specifies |
| `RingBuffer.virtual_scrollback_len` | Stand-in for `scrollback_count() as u32` while bypass is on | Reset to `0` when bypass is enabled | After an eviction under bypass: saturating-increments up to `scrollback_capacity`; once saturated, further evictions bump `scrollback_evicted_total` instead |
| `ring_push_blank` bypass branch | Skip SlimCell intern + scrollback deque ops; advance `virtual_scrollback_len` / `scrollback_evicted_total` per D1; clear overflow side-table; rotate ring_head; BCE-fill new bottom row | Bypass flag is `true` | Viewport rotation and BCE-fill are byte-identical to today's path |
| `get_scrollback_length()` branch | Return `virtual_scrollback_len` while bypass is on; otherwise existing `scrollback_count() as u32` | None | Stamping sites that use `get_scrollback_length() + cursor.row` produce byte-identical `abs_row` values to today |
| Internal setter/clearer on `TerminalCore` | Flip the bypass on/off, asserting state-transition preconditions | None | Bypass state toggled exactly once per replay; on enable, `virtual_scrollback_len` is reset to `0` |

**Processing Flow** (diagram-convertible):

1. Caller invokes setter to enable bypass (resets `virtual_scrollback_len` to `0`).
2. PTY drain runs many `ring_push_blank` invocations.
   - Bypass on, eviction needed:
     - If `virtual_scrollback_len < scrollback_capacity` → increment `virtual_scrollback_len`.
     - Else → increment `scrollback_evicted_total`.
     - Clear overflow side-table for the evicted abs row; rotate `ring_head`; BCE-fill new bottom row. SlimCell intern and deque push/pop are skipped.
   - Bypass off (live mode) → take the existing branches unchanged (`scrollback_capacity > 0` → intern + push; `scrollback_capacity == 0` → drop silently).
3. Caller invokes clearer to disable bypass.

**Implementation Steps** (5-7 max):

1. **Define the bypass state and `virtual_scrollback_len` on `RingBuffer`** — both private, default `false` / `0`.
2. **Branch `ring_push_blank`'s eviction step** so the bypass case is mutually exclusive with the existing capacity-driven branches and updates `virtual_scrollback_len` / `scrollback_evicted_total` per D1.
3. **Branch `get_scrollback_length()`** to return the virtual length while bypass is on.
4. **Expose private toggles on `TerminalCore`** — minimum surface needed by `build_from_snapshot`. Both toggles assert state-transition preconditions (e.g. `enable_bypass` requires `scrollback_slim.is_empty()` and `virtual_scrollback_len == 0`).
5. **Document the new invariant** in the existing field-level docs (`scrollback_evicted_total`, `scrollback_capacity`, plus the new `virtual_scrollback_len`) so future readers understand the bypass keeps bookkeeping intact while skipping the hot loop.
6. **Add focused unit tests** for the bypass branch in isolation: enable bypass with capacity `C`, push more than `C` viewport rows, assert `virtual_scrollback_len == C`, `scrollback_evicted_total == S - C`, and `get_scrollback_length() == C`; with `S < C` assert `virtual_scrollback_len == S`, `scrollback_evicted_total == 0`, `get_scrollback_length() == S`.

**Dependencies**: None. Blocks Phase 2.

**Testing Approach**:
- Unit: bypass-on bookkeeping behavior under D1; bypass-off byte-identical to today (existing `ring_buffer` tests).

**Acceptance Criteria**:
- [ ] All existing `term_core` tests pass without modification.
- [ ] New unit tests for the bypass branch (saturated and unsaturated cases) pass.

**Estimated Effort**: small

---

### Phase 2: Wire the bypass into `build_from_snapshot`

**Goal**: `TerminalCore::build_from_snapshot` enables the bypass before draining the payload and disables it after, while keeping the public signature and the `SnapshotReplay` return contract unchanged. The returned `core` reaches the caller with `scrollback_capacity == scrollback_lines` (the caller-requested live value).

**Files to Modify**:
- `crates/term_core/src/terminal_core.rs` — body change to `build_from_snapshot`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `build_from_snapshot` body | Toggle bypass on, drain, snapshot drain outputs (evicted_total, prompt_marks, fold_marks), toggle bypass off | `cols > 0 && rows > 0` (existing) | Returns `Some(SnapshotReplay{...})` with grid byte-identical to today's behavior; cancel-flag path returns `None` |

**Processing Flow** (diagram-convertible):

1. Construct `TerminalCore::new(cols, rows, scrollback_lines)` and `core.reset()` (unchanged).
2. Enable bypass via Phase 1 setter.
3. Call `process_pty_data_fully_cancellable(payload, cancel)`; on `None` (cancelled) — disable bypass to leave the core consistent if it is ever observed in a debugger / panic handler, then return `None`.
4. Capture `evicted_total`, `prompt_marks`, `fold_marks` (existing code).
5. Disable bypass.
6. Return `SnapshotReplay { core, actions, evicted_total, prompt_marks, fold_marks }`.

**Implementation Steps** (5-7 max):

1. **Insert bypass enable** immediately after `core.reset()`.
2. **Handle the cancel path** so bypass is disabled before returning `None`.
3. **Insert bypass disable** immediately after the existing drain-outputs are captured and before the `SnapshotReplay` literal is constructed.
4. **Update the long-form doc comment** on `build_from_snapshot` to record that scrollback content is intentionally not populated by the replay, but `scrollback_capacity` and mark/eviction accounting are preserved.

**Dependencies**: Requires Phase 1.

**Testing Approach**:
- Unit (equivalence): keep `test_build_from_snapshot_matches_reset_and_replay` passing untouched (proves grid and replay outputs are still equal to the synchronous path).
- Unit (new): `test_build_from_snapshot_restores_scrollback_capacity` covering FR2 — replay a payload that scrolls many rows, assert `core.scrollback_count() == 0` immediately, then feed N more lines and assert scrollback fills as configured up to `scrollback_lines`.
- Unit (new): `test_build_from_snapshot_bypass_preserves_evicted_total` covering D1 — build with `scrollback_lines = small_C` from a payload that scrolls `S > small_C` lines, assert `replay.evicted_total == S - small_C` (byte-identical to today's path).
- Unit (new): `test_build_from_snapshot_bypass_preserves_mark_stamping` covering D1 — feed a payload that emits two OSC 133 marks separated by enough scrolling to span the `C` threshold; assert that each mark's `abs_row` and `evicted_total` equal what today's path produces on the same payload (compare side-by-side with `reset_and_replay` on a freshly-built core).

**Acceptance Criteria**:
- [ ] `test_build_from_snapshot_matches_reset_and_replay` still passes.
- [ ] `test_build_from_snapshot_empty_payload`, `test_build_from_snapshot_is_send_across_threads`, `test_build_from_snapshot_cancelled_returns_none` still pass.
- [ ] `test_build_from_snapshot_restores_scrollback_capacity` passes.
- [ ] `test_build_from_snapshot_bypass_preserves_evicted_total` passes (byte-identical to today's `S - C` value).
- [ ] `test_build_from_snapshot_bypass_preserves_mark_stamping` passes.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` green.

**Estimated Effort**: small

---

### Phase 3: Regression-guard asserts on the perf benches

**Goal**: Convert the four existing `#[ignore]` benches into perf-regression guards by adding `assert!(per_call < threshold)` to each. Benches stay `#[ignore]` (run explicitly), but when run their thresholds bound future drift.

**Files to Modify**:
- `crates/term_core/src/bench.rs` — add asserts to `snapshot_replay_bench_2mib_seq` (FR4) and to the scrollback-disabled configuration inside `snapshot_replay_attribution_2mib_seq` (FR5).
- `src-tauri/src/mux/scrollback_filter.rs` (test mod) — add assert to `strip_replayable_rich_content_bench_2mib_plain` (FR5).
- `src-tauri/src/mux/scrollback_buffer.rs` (test mod) — add assert to `scrollback_read_all_bench_2mib_wrapped` (FR5).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `snapshot_replay_bench_2mib_seq` assert | Fail the bench if per-call ≥ 1000 ms | Bench is being executed (`--include-ignored`) | Regression in main replay flagged at bench time |
| `snapshot_replay_attribution_2mib_seq` assert | Fail if the scrollback-disabled configuration's per-call ≥ 200 ms | Bench is being executed | Regression in the underlying parse + scroll path flagged independently of FR1 |
| `strip_replayable_rich_content_bench_2mib_plain` assert | Fail if per-call ≥ 30 ms | Bench is being executed | Filter perf pinned |
| `scrollback_read_all_bench_2mib_wrapped` assert | Fail if per-call ≥ 1 ms | Bench is being executed | Ring-read perf pinned |

**Processing Flow** (diagram-convertible):

1. Bench measures per-call duration (existing).
2. Bench `eprintln!`s the measured value (existing).
3. Bench `assert!`s the duration is below the threshold (new). If above, the test fails with a clear message including the measured value and the threshold.

**Implementation Steps** (5-7 max):

1. **Add `assert!` after the `eprintln!` in each of the four benches**, with a panic message that includes the measured per-call duration and the threshold for ease of triage.
2. **Add a one-line comment** at each assert site that points to the SPEC.md threshold table (FR4 / FR5 / NFR1) so a future reader knows why the number is what it is.

**Dependencies**: Phase 1 + 2 must be in to make the main replay bench fast enough to pass its assert.

**Testing Approach**:
- Bench (manual): run `cargo test … --include-ignored` for each bench listed in SPEC.md §"Performance Goals" and confirm each prints a measurement and the assert passes.

**Acceptance Criteria**:
- [ ] `snapshot_replay_bench_2mib_seq` passes its assert.
- [ ] `snapshot_replay_attribution_2mib_seq` passes its assert on the scrollback-disabled configuration.
- [ ] `strip_replayable_rich_content_bench_2mib_plain` passes its assert.
- [ ] `scrollback_read_all_bench_2mib_wrapped` passes its assert.

**Estimated Effort**: small

---

## Complete File Structure

```
crates/term_core/src/
├── ring_buffer.rs          # Phase 1: bypass branch in ring_push_blank + bypass state
├── terminal_core.rs        # Phase 1: internal bypass toggles
│                           # Phase 2: build_from_snapshot body change
└── bench.rs                # Phase 3: assert!() in two benches

src-tauri/src/mux/
├── scrollback_filter.rs    # Phase 3: assert!() in #[ignore] bench
└── scrollback_buffer.rs    # Phase 3: assert!() in #[ignore] bench

doc/tasks/snapshot-replay-perf/
├── 要件定義書.md           # produced by sdd.1-create-spec
├── SPEC.md                 # produced by sdd.1-create-spec
├── IMPLEMENTATION.md       # this file
├── VERIFICATION.md         # produced by sdd.2-create-plan
├── VERIFICATION_RESULT.md  # produced by sdd.6-verify
├── sdd.yaml                # workflow state
└── tasks.yaml              # phase / task mapping
```

## Testing Strategy

- **Unit**: existing `test_build_from_snapshot_*` family (unchanged); two new unit tests (`restores_scrollback_capacity`, `bypass_preserves_evicted_total`); Phase 1 internal bypass unit test.
- **Integration**: covered by the existing replay-equivalence test (`test_build_from_snapshot_matches_reset_and_replay`) — replay output must remain byte-identical to the synchronous `reset_and_replay` path.
- **Perf benches**: four `#[ignore]` benches with `assert!` thresholds (Phase 3).
- **Manual / experiential**: tab-switch into a heavy-output mux tab to confirm the qualitative improvement.
- **Cross-build**: `cargo check --no-default-features` for the CLI-only build; Windows cross-build smoke-check on demand.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (no new packages) | — | All work is internal to existing crates |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `ring_push_blank` invariants subtler than they look — wrong overflow / wrap-bit / `virtual_scrollback_len` handling under bypass produces hard-to-spot corruption | Medium | High | Phase 1 keeps the bypass branch local to the eviction block; viewport rotation + BCE fill are reused verbatim. Phase 2's `matches_reset_and_replay` test catches grid divergence. Phase 1 unit tests cover the bypass counter math at the boundary `S < C` and `S > C`. Phase 2's new `bypass_preserves_mark_stamping` test exercises the stamping site through real OSC marks. |
| Bench assert thresholds (FR4 / FR5) are too tight for slower CI hardware and flake | Low | Low | Thresholds carry a 10-20× margin over local measurements (NFR1 §"Performance Goals"). If a real CI machine still flakes, the threshold is a single-number tweak. Benches are `#[ignore]`, so they don't run on default CI by default — no broad fallout. |
| Replay-mode toggle leaks state if `build_from_snapshot` panics between enable + disable | Low | Low | The fresh `TerminalCore` is local to `build_from_snapshot`; if the function panics the core is dropped along with the bypass flag. No global state involved. |
| `get_scrollback_length()` is called from a hot path other than mark stamping and the bypass branch silently changes its behavior there too | Low | Medium | Read all call sites in `crates/term_core/` during Phase 1 (the function is small; grep for `get_scrollback_length`). All current call sites are stamping or read-only inspection; if a write-back path is found, the bypass condition is conservative (only effective during the narrow `build_from_snapshot` window with no external observers). |

## Open Questions

- [ ] (none — D1 v2 preserves observable bookkeeping byte-identically, so the Option B fallback considered earlier is no longer needed.)

## Success Metrics

- [ ] `snapshot_replay_bench_2mib_seq` per-call < 1000 ms (MUST achieved).
- [ ] All four perf benches pass their asserts on the local machine.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` green.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` green.
- [ ] Manual TS-12 (tab-switch into a heavy mux tab) qualitatively confirms the predicted improvement.
- [ ] `memory/project_mux_output_pipeline_perf.md` updated.
