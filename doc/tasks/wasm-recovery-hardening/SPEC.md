# Feature: WASM Recovery Hardening

## Overview

WASM auto-recovery (`reinitWasm`) always fails with `ReferenceError: Can't find variable: heap`, leaving the terminal in a dead state that requires a manual restart. The root cause is the build-time patch `scripts/patch-wasm-bindgen.sh`, which injects a `__wbg_reset` function that references `heap` / `heap_next` variables that no longer exist in the current wasm-bindgen output. This feature restores working auto-recovery, hardens crash detection to cover the exports-lost failure mode, and reduces the memory-exhaustion crashes that trigger recovery by capping total scrollback across panes.

## Objectives

- Make WASM auto-recovery (`reinitWasm`) succeed so crashes self-heal without a manual restart.
- Detect the exports-lost crash mode (`TypeError` on a missing `terminalcore_*` export) as a recoverable WASM crash.
- Prevent the build-time patch from drifting against future wasm-bindgen output by validating injected references at build time.
- Reduce `Out of bounds memory access` crashes by capping total scrollback across all panes.

## User Stories

### US1: Self-healing after a WASM crash
As an eMterm user running long sessions with many panes, I want the terminal to recover automatically when WASM crashes, so that I do not have to restart the app and lose my session.

**Acceptance Criteria:**
- [ ] After a WASM `Out of bounds` or exports-lost crash, `reinitWasm()` completes without `ReferenceError`.
- [ ] The terminal becomes usable again without a manual restart, and `emterm.log` records `WASM module reinitialized — terminal recovered`.

### US2: Fewer memory-exhaustion crashes
As a user generating large amounts of terminal output, I want old scrollback to be discarded before memory runs out, so that the terminal does not crash on `Out of bounds memory access`.

**Acceptance Criteria:**
- [ ] When total scrollback across all panes exceeds the global cap, the oldest lines are discarded to keep total usage at or below the cap.

### US3: Build-time detection of patch drift
As a developer, I want the wasm-bindgen patch to fail the build when it references symbols that no longer exist, so that recovery never silently breaks at runtime again.

**Acceptance Criteria:**
- [ ] The patch script exits non-zero with a clear message when `__wbg_reset` references an identifier absent from the generated module.

## Technical Requirements

### Functional Requirements

- **FR1 — Remove stale `heap` references from `__wbg_reset`:** Remove the four `heap` / `heap_next` lines from the `RESET_FN` in `scripts/patch-wasm-bindgen.sh`. Keep only the state resets that exist in the current generated module: `wasm`, `cachedDataViewMemory0`, `cachedUint16ArrayMemory0`, `cachedUint8ArrayMemory0`, `WASM_VECTOR_LEN`. Do not edit `wasm/pkg/` generated files directly; fix the patch and regenerate via `bun run build:wasm`.
- **FR2 — Build-time patch guard:** After applying the patch, verify that every identifier referenced by the injected `__wbg_reset` body is declared/defined in `wasm/pkg/emterm_wasm.js`. If any referenced identifier is missing, fail the patch script with a non-zero exit and a message naming the missing identifier(s).
- **FR3 — Detect exports-lost crashes:** In `tryRecoverFromWasmCrash` (`src/terminal-app/pty-handler.ts`), classify `error instanceof TypeError && msg.includes("terminalcore_")` as a WASM crash so it routes into the existing recovery flow. Use the stable wasm-bindgen prefix `terminalcore_`, not the bundle-dependent `d0` name.
- **FR4 — Cap total scrollback across panes:** Enforce a global cap on total scrollback (across all panes). When the cap is exceeded, discard oldest lines to bring total usage at or below the cap, preventing `memory.grow` from reaching `Out of bounds`. Cap value and discard unit are decided in the implementation plan (see Open Questions).

### Non-Functional Requirements

- **NFR1 - Availability:** WASM crashes (`Out of bounds` and exports-lost `TypeError`) recover automatically without a manual restart, within the existing max-retry bounds.
- **NFR2 - Performance:** The total-scrollback check must not add measurable overhead to the PTY output hot path.
- **NFR3 - Maintainability:** Recovery success/failure remains traceable via `emterm.log` at warn/error level; patch drift is caught at build time.

## Implementation Approach

### Architecture

The WASM recovery flow is unchanged in structure; this feature fixes the broken reinit step, widens the crash classifier, and adds a memory-pressure preventive measure.

```
Crash surfaced (render / focus probe / resize / global handler)
        │
        ▼
tryRecoverFromWasmCrash (single entry, pty-handler.ts)
   ├─ classify: RuntimeError | "recursive use of an object"
   │           | "WASM not initialized" | TypeError w/ "terminalcore_"  ← FR3
   ├─ Step1: recreateWasmCore()
   └─ Step2 (if Step1 fails): reinitWasm()  → wasmReset() → init()
                                   └─ wasmReset() no longer ReferenceErrors  ← FR1
```

### Components

- **`scripts/patch-wasm-bindgen.sh`** — owns `__wbg_reset` injection. FR1 (remove `heap` lines) + FR2 (post-patch guard).
- **`src/terminal/wasm/loader.ts`** — `reinitWasm()` calls `wasmReset()` (the injected `reset`). No code change expected beyond benefiting from FR1; verify behavior.
- **`src/terminal-app/pty-handler.ts`** — `tryRecoverFromWasmCrash` entry classifier. FR3.
- **Scrollback storage (WASM grid / pane management)** — FR4 global cap and oldest-line eviction. Exact location determined in the plan.

### Recovery entry points (all converge on `tryRecoverFromWasmCrash`)

| Path | Source |
|------|--------|
| Render failure | `canvas-renderer.ts:604/646` → `index.ts:529` |
| Focus health probe | `pty-handler.ts:928` |
| Resize | `resize-handler.ts:96` |
| Global handler | `main.ts:406/419` |

A single classifier change (FR3) covers all paths.

### Dependencies

**Internal Dependencies:**
- WASM build pipeline: `bun run build:wasm` (`wasm-pack build` + `patch-wasm-bindgen.sh`).
- Existing recovery machinery in `pty-handler.ts` (retry counter, `wasmUnrecoverable`, `wasmRecoveryInProgress`).

**External Dependencies:**
- `wasm-bindgen` 0.2.x — generated glue uses `cachedDataViewMemory0` etc.; no longer emits `heap` / `heap_next`.

### File Structure

```
scripts/
└── patch-wasm-bindgen.sh        # FR1 (RESET_FN), FR2 (build-time guard)
src/terminal/wasm/
└── loader.ts                    # reinitWasm() — verified, benefits from FR1
src/terminal-app/
├── pty-handler.ts               # FR3 classifier; FR4 may hook here or in grid layer
└── pty-handler.test.ts          # FR3 unit tests
src/terminal/
└── canvas-renderer-recovery.test.ts  # recovery wiring unit tests
wasm/src/                        # FR4 scrollback cap (location TBD in plan)
```

## Test Scenarios

### Unit Tests
- [ ] `reset()` after patch resets only existing state and does not throw `ReferenceError`.
- [ ] Recovery flow: `recreateWasmCore()` fails → `reinitWasm()` succeeds → terminal recovered.
- [ ] FR3: a `TypeError` whose message contains `terminalcore_` is classified as a WASM crash and routed to recovery.
- [ ] FR3: an unrelated `TypeError` (no `terminalcore_`) is NOT treated as a WASM crash.
- [ ] FR4: total scrollback exceeding the cap evicts oldest lines down to the cap.
- [ ] FR4: total scrollback at/below the cap performs no eviction.

### Integration Tests
- [ ] Build pipeline: `bun run build:wasm` produces a patched module whose `__wbg_reset` references only existing identifiers (FR2 guard passes).
- [ ] FR2 guard fails the build when the injected body references a missing identifier (negative test).

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (`*.e2e.js`), `docker-compose.e2e.yml`, `scripts/run-e2e-docker.sh`
**Run command**: `./scripts/run-e2e-docker.sh test {spec}.e2e.js`
- [ ] Existing E2E tests pass without regression (covered by sdd.6-verify).

### Edge Cases
- [ ] Multiple recovery triggers while a reinit is in flight are deduplicated (existing idempotency preserved).
- [ ] Recovery exhaustion (`MAX_WASM_RECOVERY_ATTEMPTS`) still marks `wasmUnrecoverable` and stops retrying.
- [ ] Eviction never reduces a single pane below a usable minimum / respects the existing per-pane 10000-line limit.

## Error Handling

| Symptom (before) | Cause | Resolution |
|------------------|-------|------------|
| `ReferenceError: Can't find variable: heap` | `__wbg_reset` references removed `heap` | FR1 |
| `TypeError: undefined is not an object ('d0.terminalcore_*')` not recovered | classifier misses exports-lost mode | FR3 |
| `RuntimeError: Out of bounds memory access` | memory exhaustion from unbounded scrollback | FR4 (prevent), FR1/FR3 (recover) |
| Recovery silently broken after wasm-bindgen upgrade | patch drift | FR2 |

## Success Criteria

- [ ] All functional requirements (FR1–FR4) implemented and unit-tested.
- [ ] `reinitWasm()` recovers a crashed WASM instance without a manual restart.
- [ ] Exports-lost `TypeError` is detected as a recoverable crash.
- [ ] Total scrollback stays at or below the global cap under heavy output.
- [ ] Patch build-time guard catches missing-identifier references.
- [ ] Existing unit and E2E tests pass without regression.

## Open Questions

> **Note**: Unresolved requirements are tracked in sdd.yaml as `status: tbd`.
> Resolve before running `/em-sdd:sdd.2-create-plan`.

- [ ] FR4: Global scrollback cap value (total bytes vs. total lines) and discard unit (per-line vs. per-pane-oldest) — to be decided in the implementation plan with sizing rationale derived from the observed ~99MB ceiling.

## References

- Investigation report: `tmp/wasm-recovery-failure-investigation-2026-06-02.md`
- Requirements: `doc/tasks/wasm-recovery-hardening/要件定義書.md`
- Patch script: `scripts/patch-wasm-bindgen.sh`
- Recovery: `src/terminal-app/pty-handler.ts`, `src/terminal/wasm/loader.ts`
- Recovery tests: `src/terminal/canvas-renderer-recovery.test.ts`, `src/terminal-app/pty-handler.test.ts`
