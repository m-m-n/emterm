# Implementation Plan: WASM Recovery Hardening

## Overview

Restore working WASM auto-recovery by fixing the broken `__wbg_reset` patch, widen crash detection to the exports-lost mode, and cap total scrollback across panes to reduce memory-exhaustion crashes.

## Objectives

- Make `reinitWasm()` complete without `ReferenceError` so crashes self-heal (FR1).
- Catch patch/generated-module drift at build time (FR2).
- Detect exports-lost `TypeError` as a recoverable crash (FR3).
- Cap total scrollback across all panes to prevent `Out of bounds` (FR4).

## Prerequisites

### Development Environment
- Bun (package manager / bundler), wasm-pack, Rust wasm32 toolchain.
- Docker for test execution (preferred per project policy).

### Dependencies
- Existing recovery machinery in `src/terminal-app/pty-handler.ts` (retry counter, `wasmUnrecoverable`, `wasmRecoveryInProgress`, single entry `tryRecoverFromWasmCrash`).
- Existing per-core scrollback eviction in `wasm/src/ring_buffer.rs`.
- Existing pane registry in `src/terminal-app/mux-state.ts` (`muxPaneGrids`, `muxDetachedGrids`).

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust/WASM (grid + scrollback), POSIX sh (build patch)
- **Build**: `bun run build:wasm` (wasm-pack + `scripts/patch-wasm-bindgen.sh`)
- **Key components**: `pty-handler.ts` (recovery entry), `loader.ts` (reinit), `ring_buffer.rs` (scrollback), `mux-state.ts` (pane registry)

### Design Approach

Three independent changes, ordered by priority. Phase 1 (FR1+FR2) is the core fix that restores recovery; Phase 2 (FR3) widens detection; Phase 3 (FR4) is a preventive memory measure. The existing recovery flow structure (recreateWasmCore → reinitWasm, single classifier entry) is preserved; only the broken reset body, the classifier condition, and a new cross-pane scrollback budget are touched.

### Component Interaction

The recovery entry `tryRecoverFromWasmCrash` is the single convergence point for all crash paths (render, focus probe, resize, global handler). Widening its classifier (Phase 2) automatically covers every path. The reinit step calls the patched `reset` (Phase 1). The scrollback budget (Phase 3) lives in the pane-spanning layer, which aggregates per-core usage and requests eviction from the WASM grid when the global cap is exceeded.

## Implementation Phases

### Phase 1: Fix reinit reset and add build-time guard (FR1, FR2)

**Goal**: `reinitWasm()` completes without `ReferenceError`, and the build fails if the injected reset ever references a symbol absent from the generated module.

**Files to Modify**:
- `scripts/patch-wasm-bindgen.sh` - Remove the `heap` / `heap_next` reset lines from the injected `__wbg_reset`; add a post-patch validation step.

**Files to Create**: none (validation lives inside the patch script).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Injected reset body | Reset only state that exists in the current generated module | Patch about to be applied | Reset references no undefined identifier |
| Post-patch guard | Verify every identifier referenced by the injected reset body is declared/defined in the generated module | Patch applied | Build fails (non-zero) listing any missing identifier; otherwise continues |

**Processing Flow**:
1. wasm-pack generates the module.
2. Patch injects the reset body (now without `heap` / `heap_next` lines).
3. Guard scans the injected body's referenced identifiers.
   - All present -> continue (exit 0)
   - Any missing -> print missing identifier(s) and fail (non-zero)

**Implementation Steps**:
1. **Trim the reset body** - Keep only the state resets that exist in the generated module (`wasm`, the cached-memory views, the vector-length counter); drop the object-table reset that the current wasm-bindgen no longer emits.
2. **Add reference guard** - After patching, extract the identifiers the injected body reads/writes and assert each is declared in the generated module; fail the build otherwise.

**Dependencies**: Blocks effective recovery (Phase 2 recovery depends on a working reinit).

**Testing Approach**:
- Unit: after build, the reset runs and does not throw; calling reset then re-init yields a fresh instance.
- Integration: full build pipeline passes the guard; a deliberately-broken injected reference makes the guard fail (negative test).

**Acceptance Criteria**:
- [ ] Patched `__wbg_reset` references no identifier absent from the generated module.
- [ ] `reinitWasm()` completes without `ReferenceError`.
- [ ] Guard fails the build on a missing-identifier reference.

**Estimated Effort**: small

---

### Phase 2: Detect exports-lost crashes (FR3)

**Goal**: An exports-lost `TypeError` (message contains the stable `terminalcore_` export prefix) is classified as a recoverable WASM crash and routed through the existing recovery flow on every entry path.

**Files to Modify**:
- `src/terminal-app/pty-handler.ts` - Extend the crash classifier in `tryRecoverFromWasmCrash` to include the exports-lost condition; update the adjacent explanatory comment.

**Files to Create**: none.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Crash classifier | Decide whether an error represents a recoverable WASM crash | An error surfaced from a WASM call | Returns true for exports-lost TypeError; unrelated TypeErrors unaffected |

**Processing Flow**:
1. An error reaches `tryRecoverFromWasmCrash` from any entry path.
2. Classifier evaluates existing conditions plus the exports-lost condition.
   - Error is a runtime crash / borrow error / uninitialized / exports-lost -> enter recovery
   - Otherwise -> not handled (unchanged behavior)

**Implementation Steps**:
1. **Add exports-lost condition** - Treat a TypeError whose message includes the stable wasm-bindgen export prefix as a WASM crash (do not key on the bundle-dependent local alias).
2. **Update comment** - Document the new failure mode beside the existing classifier conditions.

**Dependencies**: Requires Phase 1 (so the recovery it triggers can actually succeed).

**Testing Approach**:
- Unit: a TypeError containing the export prefix is classified as a crash and routed to recovery; an unrelated TypeError is not.
- Integration: render-path failure of the exports-lost kind enters the recovery flow.

**Acceptance Criteria**:
- [ ] Exports-lost TypeError enters the recovery flow.
- [ ] Unrelated TypeErrors are not treated as WASM crashes.

**Estimated Effort**: small

---

### Phase 3: Cap total scrollback across panes (FR4)

**Goal**: Total scrollback across all panes stays at or below a global cap; when exceeded, oldest scrollback is evicted, keeping memory pressure away from the `memory.grow` ceiling, without adding measurable hot-path overhead.

**Files to Modify**:
- `src/terminal-app/mux-state.ts` (or the pane-spanning layer it backs) - Track and enforce the global scrollback budget across `muxPaneGrids` / `muxDetachedGrids`.
- `wasm/src/ring_buffer.rs` / `wasm/src/terminal_core.rs` - Expose current scrollback usage and a bounded eviction operation if the existing surface is insufficient.

**Files to Create**: none expected (extend existing modules).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Per-core usage reporter | Report a core's current scrollback usage (lines and/or bytes) | Core exists | Caller can sum usage across panes |
| Bounded eviction | Drop oldest scrollback rows from a core down to a target | Target ≤ current usage | Core usage ≤ target; per-pane minimum respected |
| Global budget enforcer | Keep summed usage ≤ global cap by evicting from the oldest/largest contributors | Aggregated usage available | Total usage ≤ cap |

**Processing Flow**:
1. Scrollback grows on PTY output (existing per-core eviction at the per-pane line limit still applies).
2. The enforcer observes aggregated usage at a coarse cadence (not per byte).
   - Total ≤ cap -> no action
   - Total > cap -> evict oldest scrollback from contributor(s) until total ≤ cap
3. Eviction respects each pane's usable minimum and the existing per-pane 10000-line limit.

**Implementation Steps**:
1. **Decide the budget metric and cap value** - Choose lines vs. bytes and the cap, sized from the observed ~99MB ceiling with safety margin; record the rationale (resolves the Open Question).
2. **Expose usage + bounded eviction** - Ensure the grid layer can report usage and evict oldest rows to a target; reuse existing eviction where possible.
3. **Implement the global enforcer** - Aggregate usage across active and detached panes and evict from oldest/largest contributors when over cap.
4. **Avoid hot-path cost** - Run enforcement on a coarse cadence / threshold check, not on every PTY byte.

**Dependencies**: Independent of Phases 1–2; can land separately.

**Testing Approach**:
- Unit: usage at/below cap evicts nothing; usage above cap evicts oldest down to cap; per-pane minimum respected.
- Unit: enforcement cadence does not run per byte (threshold/coarse check verified).
- Manual: heavy multi-pane output no longer drives memory to the crash ceiling (observed via heap heartbeat in `emterm.log`).

**Acceptance Criteria**:
- [ ] Total scrollback stays ≤ global cap under heavy output.
- [ ] Oldest scrollback is evicted; per-pane minimum preserved.
- [ ] No measurable PTY hot-path overhead.

**Estimated Effort**: medium

---

## Complete File Structure

```
scripts/
└── patch-wasm-bindgen.sh            # Phase 1: trimmed reset body + build-time guard
src/terminal-app/
├── pty-handler.ts                   # Phase 2: classifier; Phase 3: global budget hook
├── pty-handler.test.ts              # Phase 2 unit tests
└── mux-state.ts                     # Phase 3: cross-pane scrollback budget
src/terminal/
└── canvas-renderer-recovery.test.ts # recovery wiring unit tests
wasm/src/
├── ring_buffer.rs                   # Phase 3: usage report + bounded eviction
└── terminal_core.rs                 # Phase 3: usage surface (if needed)
doc/tasks/wasm-recovery-hardening/
├── 要件定義書.md
├── SPEC.md
├── IMPLEMENTATION.md
├── VERIFICATION.md
└── tasks.yaml
```

## Testing Strategy

- Unit: recovery classifier and reset behavior (Phase 1–2); scrollback budget logic (Phase 3). Target high coverage on the classifier and eviction logic.
- Integration: build pipeline guard (Phase 1); recovery routing (Phase 2).
- E2E: existing suite regression only (covered by sdd.6-verify); no new E2E per agreed test policy.
- Manual: memory-pressure observation via `emterm.log` heap heartbeat (Phase 3).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| wasm-bindgen | 0.2.x | Generated glue (no longer emits `heap`/`heap_next`) |
| wasm-pack | existing | WASM build |
| Bun | existing | Bundler / test runner |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Future wasm-bindgen changes the surviving reset variable names | Medium | High | Phase 1 build-time guard catches it |
| Scrollback eviction loses user history | Medium | Medium | Size cap generously; evict oldest only; respect per-pane minimum |
| Global enforcer adds hot-path cost | Low | Medium | Coarse cadence / threshold check, not per byte |
| reinit succeeds but exports lost again later | Low | Low | Phase 2 detects start; existing max-retry prevents loops |

## Open Questions

- [x] FR4: Global scrollback cap metric (lines vs. bytes), cap value, and discard granularity (per-line vs. per-pane-oldest) — **RESOLVED in Phase 3 Step 1** (see below).

### FR4 resolution (Phase 3 Step 1)

**Metric: total scrollback LINES across all live panes.**
Byte usage is not measurable per-core: `getWasmMemoryBytes()` reports module-wide
`WebAssembly.Memory` size, shared by every core in the single WASM instance, so it
cannot attribute bytes to an individual pane. Scrollback rows are SlimCell-compressed
with style/char-table dedup, so per-row byte cost is variable and not directly
queryable. Line count is the only cheap, per-core-attributable, deterministic metric
(`WasmGrid.getScrollbackLength()` → `TerminalCore::get_scrollback_length()`).

**Scope: live panes only.** The enforcer aggregates the active grid plus every live
`WasmGrid` in `muxPaneGrids` (primary + alternate). `muxDetachedGrids` holds frozen
serialized `Uint8Array` snapshots that do not grow and are not the source of
`memory.grow` pressure during active output, so they are excluded.

**Cap value: `GLOBAL_SCROLLBACK_LINE_BUDGET = 60000` total lines.**
Sizing from the observed ~99MB heap ceiling: the per-pane limit is 10000 lines, so a
worst case of ~6 simultaneously-full panes (60000 lines) is allowed before eviction.
Empirically a single 10000-line pane stays well under ~20MB of SlimCell + interned-table
storage, so 60000 lines keeps total scrollback storage on the order of ~tens of MB —
comfortably below the ~99MB ceiling with margin for viewport, image cache, and bundler
overhead. This caps unbounded growth across many panes (the failure mode) while not
shrinking the normal 1–4 pane workflow at all.

**Per-pane minimum: `PER_PANE_SCROLLBACK_MIN = 1000` lines.**
Eviction never reduces any single pane below 1000 retained scrollback lines, so even
under global pressure each pane keeps a usable history window. The existing per-pane
10000-line `scrollback_capacity` limit remains the upper bound per pane.

**Discard granularity: per-pane bounded eviction of oldest rows.** A new
`TerminalCore::evict_oldest_scrollback(target_len)` drops oldest rows down to a target
length (releasing intern refcounts), reusing the existing oldest-first eviction logic.
The enforcer evicts from the largest contributors first until the global total ≤ cap,
respecting each pane's minimum.

**Cadence (NFR2 / TS-12): coarse, not per byte.** Enforcement runs only when a growth
counter crosses `ENFORCE_CHECK_INTERVAL_LINES` newly-added scrollback lines (default
512), never on every PTY byte. The hot path only increments a counter and compares it
to the threshold.

## Success Metrics

- [ ] FR1–FR4 implemented and unit-tested.
- [ ] `reinitWasm()` recovers a crashed instance without manual restart.
- [ ] Exports-lost TypeError detected as recoverable.
- [ ] Total scrollback ≤ global cap under heavy output.
- [ ] Build guard catches missing-identifier references.
- [ ] Existing unit + E2E tests pass without regression.
