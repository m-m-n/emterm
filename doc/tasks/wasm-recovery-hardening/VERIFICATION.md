# Verification Document: WASM Recovery Hardening

## Overview
**Feature**: wasm-recovery-hardening
**SPEC.md**: `doc/tasks/wasm-recovery-hardening/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-recovery-hardening/IMPLEMENTATION.md`

## Build Verification
- Command (main): `bun run build:wasm`
- Command (wasm): `cargo build --manifest-path wasm/Cargo.toml`
- Expected: exit code 0; the post-patch guard passes (no missing-identifier references in the injected reset).

### Actual results (sdd.4-implement)
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run build:wasm"` → exit 0.
  - `Patched wasm/pkg/emterm_wasm.js: added reset() export`
  - `Patch guard passed: __wbg_reset references only declared identifiers`
  - `Patched wasm/pkg/emterm_wasm.d.ts: added reset() declaration`
- Patched `__wbg_reset` body confirmed trimmed (no `heap.length` / `heap_next`); resets only `wasm`, `cachedDataViewMemory0`, `cachedUint16ArrayMemory0`, `cachedUint8ArrayMemory0`, `WASM_VECTOR_LEN`.

## Test Verification
- Command (main): `bun test`
- Command (wasm): `cargo test --manifest-path wasm/Cargo.toml`
- Typecheck: `bun run typecheck`
- Coverage target: minimum 80%, target 90% on the crash classifier and scrollback eviction logic.

### Actual results (sdd.4-implement, all in Docker)
- `bun test` → **2369 pass, 0 fail, 17 todo** (2386 tests / 111 files). Includes new FR3 (TS-3/TS-4) and FR4 (TS-5/TS-6/TS-7/TS-12) tests + the patch-script tests (TS-1/TS-8/TS-9).
- `cd wasm && cargo test` → **600 pass, 0 fail, 3 ignored**. Includes new bounded-eviction tests.
- `bun run typecheck` (`tsc --noEmit`) → exit 0, no errors.
- New test files: `scripts/patch-wasm-bindgen.test.ts` (TS-1/TS-8/TS-9), `src/terminal-app/mux-scrollback-budget.test.ts` (TS-5/TS-6/TS-7/TS-12). FR3 tests added to existing `src/terminal-app/pty-handler.test.ts` (TS-3/TS-4). Rust eviction tests added to `wasm/src/ring_buffer.rs`.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Call patched `reset` after build | No `ReferenceError`; only existing state reset | Unit |
| TS-2 | `recreateWasmCore` fails then `reinitWasm` runs | Reinit completes; terminal recovered | Unit/Integration |
| TS-3 | Error is a TypeError whose message contains `terminalcore_` | Classified as WASM crash; routed to recovery | Unit |
| TS-4 | Error is an unrelated TypeError (no `terminalcore_`) | NOT classified as WASM crash | Unit |
| TS-5 | Total scrollback exceeds the global cap | Oldest scrollback evicted down to cap | Unit |
| TS-6 | Total scrollback at/below the global cap | No eviction performed | Unit |
| TS-7 | Eviction with a small per-pane residual | Per-pane minimum / per-pane 10000-line limit respected | Unit |
| TS-8 | Build pipeline applies patch | Guard passes; injected reset references only existing identifiers | Integration |
| TS-9 | Injected reset references a missing identifier (negative) | Guard fails the build with a non-zero exit naming the identifier | Integration |
| TS-10 | Multiple recovery triggers during an in-flight reinit | Deduplicated; single reinit (existing idempotency preserved) | Unit |
| TS-11 | Recovery attempts exceed the max | `wasmUnrecoverable` set; retries stop | Unit |
| TS-12 | Scrollback enforcement under sustained byte stream | Enforcement runs on a coarse cadence, not per PTY byte | Unit |

## Code Quality Verification
- Format (main): `bun run typecheck`
- Format (wasm): `cargo fmt --manifest-path wasm/Cargo.toml`

### Actual results (sdd.4-implement)
- `cd wasm && cargo fmt` → applied (reordered imports in `wasm/src/ring_buffer.rs`); wasm tests re-run green afterward.
- `bun run typecheck` → exit 0, no errors.

## File Structure Verification

### Files to Create
- none

### Files to Modify
- [x] `scripts/patch-wasm-bindgen.sh` - Trimmed reset body (removed `heap`/`heap_next`) + post-patch guard (FR1/FR2)
- [x] `src/terminal-app/pty-handler.ts` - Exports-lost crash classifier condition (FR3); coarse `enforceScrollbackBudget` hook (FR4/NFR2)
- [x] `wasm/src/ring_buffer.rs` - `evict_oldest_scrollback(target_len)` bounded eviction + Rust unit tests (FR4)
- [x] `src/terminal-app/pty-handler.test.ts` - FR3 unit tests (TS-3/TS-4)

### Files Created (deviation from plan — extracted for testability)
- [x] `scripts/patch-wasm-bindgen.test.ts` - Patch guard / reset tests (TS-1/TS-8/TS-9)
- [x] `src/terminal-app/mux-scrollback-budget.ts` - Pure planner + `ScrollbackBudgetEnforcer` (FR4). The plan named `mux-state.ts` as the host; the budget logic was extracted into a dedicated, unit-testable module and wired from `src/terminal-app/index.ts` (the live pane-spanning orchestrator) instead, since `mux-state.ts` holds only context builders, not the live grid registry.
- [x] `src/terminal-app/mux-scrollback-budget.test.ts` - Budget unit tests (TS-5/TS-6/TS-7/TS-12)

### Files Modified (additional)
- [x] `src/terminal/wasm/terminal-core.ts` - `WasmGrid.evictOldestScrollback()` wrapper (FR4)
- [x] `src/terminal/state.ts` - `getPrimaryWasmGrid()` accessor for the enforcer (FR4)
- [x] `src/terminal-app/index.ts` - Enforcer instance + `collectLiveScrollbackPanes()` / `enforceScrollbackBudget()` wiring (FR4)

### Not needed
- `wasm/src/terminal_core.rs` - existing `get_scrollback_length()` usage surface was sufficient; only `ring_buffer.rs` needed the new eviction op.
- `src/terminal/canvas-renderer-recovery.test.ts` - existing recovery-wiring tests already cover the render-path routing; FR3 classification is unit-tested at the classifier in `pty-handler.test.ts`.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `reset()` runs without `ReferenceError` | TS-1 |
| SC-2 | `reinitWasm()` recovers a crashed instance | TS-2 |
| SC-3 | Exports-lost TypeError detected as recoverable | TS-3, TS-4 |
| SC-4 | Total scrollback ≤ global cap; oldest evicted | TS-5, TS-6, TS-7 |
| SC-5 | Build guard catches missing-identifier references | TS-8, TS-9 |
| SC-6 | Existing unit + E2E tests pass without regression | Full `bun test` + `cargo test` + sdd.6 E2E |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (remove stale heap refs) | Phase 1 | TS-1, TS-2 |
| FR2 (build-time guard) | Phase 1 | TS-8, TS-9 |
| FR3 (exports-lost detection) | Phase 2 | TS-3, TS-4 |
| FR4 (cap total scrollback) | Phase 3 | TS-5, TS-6, TS-7 |
| NFR1 (auto-recovery) | Phase 1+2 | TS-2, TS-3, TS-10, TS-11 |
| NFR2 (no hot-path overhead) | Phase 3 | TS-12 |
| NFR3 (traceable logs, build-time drift) | Phase 1 | TS-9 + recovery log lines in `emterm.log` |

## E2E Testing
Existing suite regression only (no new E2E per agreed test policy).
- [ ] Existing E2E tests pass without regression: `./scripts/run-e2e-docker.sh test`
  - **Not run in sdd.4-implement** per the agreed TDD-scope policy (E2E deferred to sdd.6-verify). Unit + integration only in this phase.

## Manual Testing (E2E Not Possible)
- [ ] Heavy multi-pane output no longer drives WASM heap to the crash ceiling (observe heap heartbeat in `emterm.log`).
- [ ] After an induced WASM crash, recovery log line `WASM module reinitialized — terminal recovered` appears and the terminal stays usable without a manual restart.

## Performance Verification
- NFR2: the global scrollback enforcement runs on a coarse cadence / threshold, not per PTY byte — verified by Phase 3 unit tests asserting enforcement is not invoked per-byte.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration | 12 | 12 | 0 | 0 |
| E2E regression | 1 | 0 | 1 | 0 |
| Manual | 2 | 0 | 0 | 2 |
