# Implementation Plan: mux client coalesce (phase1)

## Overview

Rework the extracted-frame loop in `Tab::process_combined` so consecutive active-pane `PtyOutput` inner payloads arriving in one `pump` are concatenated and parsed once, instead of one `process_pty_data_fully` per frame.

## Objectives

- Collapse the per-frame parse flood (~1400 parses/pump, ~120k total for the 10M benchmark) into one parse per consecutive active-pane `PtyOutput` run.
- Keep output byte-for-byte equivalent to the per-frame path.
- Preserve every existing interaction of the `PtyOutput` arm (pane filter, `pending_switch` queueing, device-response write-back, OSC 133 / fold mark drains, inner image APC boundaries, detach ordering).

## Prerequisites

### Development Environment
- Rust toolchain as pinned by the repo (`rust-toolchain`).
- `CARGO_TARGET_DIR=src-tauri/target` for quick check / unit tests; release build for the E2E throughput test.

### Dependencies
- Internal only. No new crates. No daemon / bridge / transport / WebView (`src/`) changes.
- Existing test helpers in `src-tauri/src/tabs.rs` (`tabs::tests`): `test_process_combined`, `pty_output_apc`, `welcome_msg`, `mux_tab_active_pane`, `test_grid_text`, `test_row_text`, and `crate::mux::apc::encode_emterm_mux`.

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Affected file**: `src-tauri/src/tabs.rs` (production + its in-file `tabs::tests`)
- **Key components**: `Tab::process_combined`, `Tab::apply_mux_message` (`PtyOutput` arm), `term_core` streaming parser (`process_pty_data_fully`)

### Design Approach

The coalescing decision lives in the **`process_combined` extracted-frame loop**, not inside `apply_mux_message`. The loop already iterates the mux APC frames extracted from the coalesced PTS buffer; it gains a concatenation buffer for the inner payloads of frames that are eligible to be batched.

A frame is **batch-eligible** only when ALL of:
- it is a `PtyOutput` frame, AND
- it is addressed to the tab's active pane (or the tab has no window group, in which case all `PtyOutput` is accepted), AND
- there is no in-flight off-thread replay (`pending_switch` is `None`).

Any frame that is not batch-eligible is a **boundary**: the accumulation buffer is flushed (one parse + the per-batch side effects) *before* that frame is handled through the existing per-frame path. The buffer is also flushed at loop end. This preserves ordering between batched output and every control message / legacy path.

The per-batch side effects (run once after the concatenated parse) are exactly those the current `PtyOutput` arm runs per frame: device-response write-back (`take_response` → `write_device_response`) and OSC 133 / fold mark drain + backfill (`drain_marks` → `backfill_marks`). Inner image APC/DCS produced by the parse are already drained once per pump by the existing post-loop block in `process_combined`, so they need no per-batch handling.

### Component Interaction

```
process_combined
  ├─ feed_with_offsets → extracted frames
  └─ loop over frames:
       frame is batch-eligible PtyOutput  → append inner payload to accumulator
       frame is a boundary (control / non-active / pending_switch / detach)
                                          → FLUSH accumulator (1 parse + side effects)
                                          → handle frame via existing per-frame path
       end of loop                        → FLUSH accumulator
  └─ existing post-loop image / detach-tail / inner-image drains (unchanged)
```

## Implementation Phases

### Phase 1: Test instrumentation + failing tests (TDD red)

**Goal**: Establish a direct, production-safe way to observe parse-pass count, write the new required test, and restate the metric test for batched behavior — all failing against today's per-frame code.

**Files to Modify**:
- `src-tauri/src/tabs.rs` — add a `#[cfg(test)]` parse-pass observation hook and the tests.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| parse-pass counter (test-only) | Count how many times the **coalesce flush** invokes the core parse for active-pane output (this is the only site TS-1 / TS-3 exercise; the legacy per-frame `apply_mux_message` site may also increment it for symmetry, but is not required by the phase1 tests) | Compiled only under `cfg(test)` | Production build unchanged (no taint); tests can read the count |
| new required test | Prove consecutive active-pane `PtyOutput` in one buffer → one parse pass | Counter exists | Asserts count == 1 for a single consecutive run, and grid == concatenated result |
| metric test restatement | Replace the "grid grows per message ⇒ K passes" proxy with batched assertion | Counter exists | Asserts one buffer of K active-pane frames parses in 1 pass (was K) |

**Processing Flow** (new required test):
1. Build a tab with an active pane (`mux_tab_active_pane`).
2. Wire-encode K `PtyOutput` frames for the active pane (`pty_output_apc`) into ONE combined buffer.
3. Drive `test_process_combined` once with that buffer.
4. Assert: parse-pass count == 1 (consecutive run coalesced), and grid text equals the single-concatenated result.

**Implementation Steps**:
1. **Add `#[cfg(test)]` parse-pass counter** — a test-only field on `Tab` incremented at the coalesce flush site (added in Phase 2) and the legacy per-frame parse site, so both paths are counted consistently. Expose a `#[cfg(test)]` reader.
2. **Write the new required test** — consecutive active-pane `PtyOutput` (K frames, one buffer) ⇒ one parse pass + correct grid.
3. **Restate the metric test** `c_pty_output_parsed_per_message_grid_grows_step_by_step` — drive K frames through `process_combined` in one buffer and assert batched (1 pass) reflection; update its name/comment to the post-change contract.
4. **Confirm parity test unchanged** — `c_split_messages_equal_single_concatenated_message` stays as-is (must remain green after Phase 2).

**Dependencies**: Blocks Phase 2 (red tests define the target).

**Testing Approach**:
- Unit: the three `c_` tests above.
- Manual: none.

**Acceptance Criteria**:
- [ ] New required test compiles and FAILS against current per-frame code (count == K, not 1).
- [ ] Restated metric test FAILS against current code.
- [ ] Parity test still compiles.

**Estimated Effort**: small

---

### Phase 2: Coalesce implementation (TDD green)

**Goal**: Implement the accumulation + flush in `process_combined` so the Phase 1 tests pass and all existing tests stay green.

**Files to Modify**:
- `src-tauri/src/tabs.rs` — `process_combined` extracted-frame loop; factor the `PtyOutput` parse + per-batch side effects into a reusable flush.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| accumulator buffer | Hold concatenated inner payloads of consecutive batch-eligible `PtyOutput` frames | Active pane known; no `pending_switch` | Flushed at every boundary and at loop end |
| flush operation | Parse the accumulated buffer once, then run per-batch side effects (device-response write-back, mark drain + backfill), then clear | Accumulator may be empty (no-op) | Core reflects all accumulated bytes; side effects applied once; `changed` set when bytes were applied |
| boundary classifier | Decide batch-eligible vs boundary per frame | Frame decoded | Eligible frames append; boundaries trigger flush-then-handle |

**Processing Flow**:
1. For each extracted frame, decode it.
2. If it is a batch-eligible active-pane `PtyOutput` (no `pending_switch`): append its inner payload to the accumulator; continue.
3. Otherwise (control message / non-active pane / `pending_switch` active / detach):
   - Flush the accumulator (one parse + side effects).
   - Handle the frame through the existing per-frame path (`apply_mux_message`), including the detach `break` / tail-reroute behavior, non-active drop, and `pending_switch` `live_queue` push.
4. After the loop: flush the accumulator a final time.

**Implementation Steps**:
1. **Introduce the accumulator + flush helper** carrying the active-pane parse and the per-batch side effects (device response, mark drain/backfill) once.
2. **Classify each frame** in the loop; route batch-eligible active-pane `PtyOutput` to the accumulator, everything else to flush-then-handle.
3. **Preserve detach semantics** — flush before applying a frame, keep the Some→None detach detection, the loop `break`, and the post-loop tail re-route exactly as today.
4. **Preserve non-active drop and `pending_switch` legacy path** — non-active `PtyOutput` is excluded from the accumulator (dropped as before); while `pending_switch` is active, `PtyOutput` flushes the accumulator then takes the existing per-frame `live_queue` path. (Optional correctness-preserving refinement: a non-active `PtyOutput` frame with NO `pending_switch` only ever drops, so it need not force a flush — the active run may safely continue accumulating across it since the dropped bytes never touch the active pane's stream. Treating it as a boundary is also correct, just one extra flush. Choose either; keep it simple unless the E2E shows non-active interleave is common.)
5. **Increment the test-only parse-pass counter** at the flush parse site.
6. **Run the full `tabs::tests` suite** single-threaded and fix any ordering regressions.

**Dependencies**: Requires Phase 1.

**Testing Approach**:
- Unit: Phase 1 tests now green; parity test green; all existing `PtyOutput` / detach / image / `pending_switch` tests green.
- Integration: full `--lib` suite, single-threaded.
- Manual: none.

**Acceptance Criteria**:
- [ ] New required test passes (count == 1).
- [ ] Restated metric test passes (batched).
- [ ] `c_split_messages_equal_single_concatenated_message` green.
- [ ] Full `--lib` suite green (`--test-threads=1`).

**Estimated Effort**: medium

---

### Phase 3: E2E throughput measurement (before/after)

**Goal**: Quantify the improvement with the daemon-direct E2E and record it.

**Files to Modify**:
- None expected. `src-tauri/tests/mux_throughput.rs` is the existing measurement harness (`#[ignore]`, release). If it needs a small adjustment to also report frame count, treat that as a test-only change.

**Processing Flow**:
1. Capture baseline (before) numbers from the design doc: 2.85 MiB/s, 124,233 frames at N=10M.
2. Build release and run the ignored E2E test.
3. Record after MiB/s and frame count; confirm throughput up and frame count down.

**Implementation Steps**:
1. **Run the E2E throughput test** in release with `--ignored --nocapture`.
2. **Record before/after** MiB/s and frame count in the verification result.

**Dependencies**: Requires Phase 2.

**Testing Approach**:
- Performance: `mux_throughput.rs` E2E (release, ignored).
- Manual: real-environment `time seq 1 10000000` is optional context for phase2/3 decision; NOT a phase1 pass gate.

**Acceptance Criteria**:
- [ ] E2E throughput improved over 2.85 MiB/s (N=10M).
- [ ] E2E frame count reduced below 124,233 (N=10M).

**Estimated Effort**: small

---

## Complete File Structure

```
doc/tasks/mux-client-coalesce/
  ├── 要件定義書.md          # requirements (Japanese)
  ├── SPEC.md                # technical spec (English)
  ├── IMPLEMENTATION.md      # this plan
  ├── VERIFICATION.md        # verification plan
  ├── tasks.yaml             # phase/task breakdown
  └── sdd.yaml               # SDD workflow state
src-tauri/src/tabs.rs        # MODIFY: process_combined coalesce + tests
src-tauri/tests/mux_throughput.rs  # measurement harness (run only; test-only tweak if needed)
```

## Testing Strategy

- Unit: the coalesce contract (consecutive → 1 parse), output parity (split == concatenated), boundary ordering, non-active drop, `pending_switch` legacy path.
- Integration: full `tabs::tests` `--lib` suite, single-threaded (replay tests are non-deterministic under parallelism).
- Performance: daemon-direct E2E throughput + frame count (release, ignored).
- Manual: none required for phase1 pass.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | - | Internal change only |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Ordering regression between batched output and control messages | Medium | High | Flush accumulator before every boundary; full suite single-threaded |
| `pending_switch` path altered, corrupting off-thread replay | Low | High | Keep `pending_switch` frames on the existing per-frame path; flush-then-handle; rely on existing pending-switch tests |
| Inner Kitty image boundary handling changes | Low | Medium | Coalescing removes inner boundaries (safe); post-loop inner-image drain unchanged |
| Device-response / OSC 133 mark semantics shift when batched | Medium | Medium | Run side effects once per batch; parity + mark/detach regression tests |
| Test-only counter leaking into production | Low | Low | Gate counter strictly under `cfg(test)` |

## Open Questions

- [ ] None blocking. The real-environment 10s goal is explicitly out of phase1's pass gate (phase2/3 decision input).

## Success Metrics

- [ ] Functional completeness: FR1–FR5 implemented.
- [ ] Quality: parity test green; full `--lib` suite green single-threaded.
- [ ] Performance: E2E throughput up, frame count down vs. baseline.
