# Verification Document: visibility-raf-heartbeat

## Overview

**Feature**: visibility-raf-heartbeat
**SPEC.md**: `doc/tasks/visibility-raf-heartbeat/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/visibility-raf-heartbeat/IMPLEMENTATION.md`

## Build Verification

- Frontend command: `bun run build`
- Tauri build (smoke): `bun tauri build` (CI / pre-release only)
- WASM build: `bun run build:wasm` (only if WASM module changes — not expected in this feature)
- Expected: exit code 0, no errors

### Implementation Results

- TypeScript typecheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"` → exit code 0 (no errors)
- No frontend build / Tauri build executed in this implementation pass (deferred to sdd.6-verify); the TypeScript compiler is the static gate.

## Test Verification

### Unit (Bun)

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: minimum 90% on `src/pty/visibility-controller.ts` new lines, target 95%

#### Implementation Results

- Full bun test suite: 2305 pass, 17 todo (in unrelated test files), 0 fail across 106 files (6.66 s).
- Visibility controller suite specifically: 21 pass / 0 fail (9 pre-existing TS-8/TS-9/TS-21/F11/DIAG-IDLE + 12 new TS-29 〜 TS-39 cases including TS-29 boundary).
- The 12 new tests cover every FR1〜FR10 and NFR3 path enumerated in the test scenario table below.

### Rust

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Expected: All existing tests pass without modification (this feature does not change Rust code)
- Implementation Results: not executed in this pass — feature is purely TypeScript and modifies no Rust source. Deferred to sdd.6-verify.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-29 | rAF callbacks halted >= 5 s while controller is visible | Hidden notification fires; `setVisibility(false)` dispatched once | Unit |
| TS-30 | `document.visible` and `focused` both true but rAF dead | `currentEffective()` returns false | Unit |
| TS-31 | rAF callback fires after dead state | Immediate `setVisibility(true)` dispatch with no debounce | Unit |
| TS-32 | Health-tick runs while `lastRafPerfMs` is null (initial grace) | No `setVisibility(false)` fires | Unit |
| TS-33 | Two consecutive health-ticks separated by > 30 s | Dead detection skipped that tick; `lastRafPerfMs` reset to now | Unit |
| TS-34 | `notify(false)` is invoked | `cancelAnimationFrame` called once; no further rAF scheduled until next visible notify | Unit |
| TS-35 | rAF dead but document visible & focused | Hidden log line includes `reason=raf-stall` | Unit |
| TS-36 | document hidden and focus lost simultaneously | Hidden log line includes `reason=document+focus` | Unit |
| TS-37 | Constructor invoked without injected rAF function in env where global is absent | No throw; degraded mode acceptable | Unit |
| TS-38 | `start()` → `stop()` → `start()` cycle | rAF loop is recreated cleanly; no orphaned handles | Unit |
| TS-39 | rAF dead detected with a connected MuxClient mock | `MuxClient.sendSetVisibility(false)` dispatched once; on resume `(true)` dispatched once | Unit |
| E2E-1 | Live webview with `globalThis.requestAnimationFrame` overridden by a stub that records but never invokes callbacks (lazy default observes the override) | After ~6 s `pty_set_visibility(false)` was dispatched (verified by log scan for `[DIAG-IDLE] reason=raf-stall`) | E2E |
| E2E-2 | After E2E-1, restore real rAF | `pty_set_visibility(true)` dispatched within 100 ms; subsequent output renders | E2E |
| E2E-3 | Busy PTY producer running during the rAF stall in E2E-1 | `pty_get_send_stats` `sent_bytes` does not materially grow past pre-stall snapshot (only small in-flight residue accepted) | E2E |

## Code Quality Verification

- TypeScript typecheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Format: project does not enforce TS formatter; consistency reviewed in code review
- Static analysis: typecheck above is the static gate

### Implementation Results

- `bun run typecheck` exit code 0 across the full TS surface (post-modification).
- No new TS lint warnings introduced (project uses tsc as the static gate; no ESLint/Biome integration to flag style issues).

## File Structure Verification

### Files to Create

- [x] `e2e-tests/specs/visibility-raf-heartbeat.e2e.js` — rAF monkey-patch E2E spec (created)
- [x] `doc/tasks/visibility-raf-heartbeat/freeze-repro-rafstall.md` — manual repro procedure (created)

### Files to Modify

- [x] `src/pty/visibility-controller.ts` — added `RAF_DEAD_THRESHOLD_MS`, `SUSPEND_GAP_MS`, three optional DI fields (`requestAnimationFrameFn`, `cancelAnimationFrameFn`, `nowFn`), private state (`rafAlive`, `lastRafPerfMs`, `lastHealthTickPerfMs`, `rafHandle`), `scheduleRaf` / `cancelRaf` / `healthTick` / `hiddenReason` methods, `currentEffective` AND-extension, notify-side rAF wiring, hidden-log `reason=` field
- [x] `src/pty/visibility-controller.test.ts` — added FakeRaf helper, MockMux helper, harness extension (raf, mux, nowFn), TS-29 〜 TS-39 plus a TS-29 boundary case (12 new tests)

### Files NOT to Modify

- `src/pty/client.ts` — `setVisibility` API stays unchanged
- `src/terminal/mux/mux-client.ts` — `sendSetVisibility` stays unchanged
- `src-tauri/**/*.rs` — no backend change
- `doc/tasks/visibility-aware-pty-streaming/**` — sibling feature SPEC stays untouched

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1〜FR10 implemented | TS-29〜TS-38 + E2E-1〜E2E-3 cover each FR |
| SC-2 | NFR1 — minimal rAF callback work | Code review: callback body does only `nowFn()` + boolean check |
| SC-3 | NFR2 — small resource footprint | Code review: only ~5 numeric / boolean fields added |
| SC-4 | NFR3 — DI fallback when global rAF absent | TS-37 |
| SC-5 | NFR4 — `reason=...` field present in hidden log | TS-35 / TS-36 |
| SC-6 | NFR5 — visibility-aware-streaming compat | Run unchanged E2E suite (regression) |
| SC-7 | `bun test` passes | CI |
| SC-8 | `cargo test` passes | CI |
| SC-9 | `bun run typecheck` passes | CI |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 | Phase 1 + 2 | TS-29 (loop runs), E2E-1 (real timing) |
| FR2 | Phase 2 | TS-29, TS-32 (grace) |
| FR3 | Phase 2 | TS-30 |
| FR4 | Phase 2 | TS-31, E2E-2 |
| FR5 | Phase 2 | TS-33 |
| FR6 | Phase 3 | TS-35, TS-36 |
| FR7 | Phase 2 | TS-34 |
| FR8 | Phase 1 | TS-37 |
| FR9 | Phase 2 | TS-32 |
| FR10 | Phase 4 | TS-38 (idempotency), TS-39 (mux dispatch), and existing visibility-aware-streaming.e2e.js / freeze-regression.e2e.js / visibility-throughput-bench.e2e.js / visibility-resume-block.e2e.js pass unchanged |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 | Phase 2 | Code review (callback body); no per-frame log assertion |
| NFR2 | Phase 1 | Code review (field count) |
| NFR3 | Phase 1 | TS-37 |
| NFR4 | Phase 3 | TS-35, TS-36, manual log inspection |
| NFR5 | Phase 4 | Regression suite |

## E2E Testing

### Existing E2E Regression

- Spec: `visibility-aware-streaming.e2e.js`, `freeze-regression.e2e.js`, `visibility-throughput-bench.e2e.js`, `visibility-resume-block.e2e.js`
- Run: `./scripts/run-e2e-docker.sh test`
- Expected: PASS without modification

#### Implementation Results

- Full E2E regression run (`./scripts/run-e2e-docker.sh test`) deferred to sdd.6-verify (10+ minute Docker build cycle).
- Static-review evidence that no regression is plausible: the existing visibility specs invoke `pty_set_visibility` directly via Tauri internals and never exercise the new rAF heartbeat path; the controller modifications keep every public API and dispatch sequence on the existing path unchanged. The 9 pre-existing controller unit tests (TS-8 / TS-9 / TS-21 / F11 / DIAG-IDLE) continue to pass.

### New E2E Test Scenarios

- [ ] E2E-1: rAF monkey-patched → backend hidden notification within 6 s
- [ ] E2E-2: rAF restored → backend visible notification within 100 ms; rendering resumes
- [ ] E2E-3: `pty_get_send_stats.in_flight` does not grow during the stall

## Manual Testing (E2E Not Possible)

Procedure documented in `doc/tasks/visibility-raf-heartbeat/freeze-repro-rafstall.md`. Each scenario reports pass / fail by inspecting `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.

- [ ] Workspace switch: emterm window moved to another workspace; `[DIAG-IDLE] reason=raf-stall` recorded; no `backpressure stalled` accumulates; on return `[DIAG-IDLE] visibility→visible` recorded and screen up to date
- [ ] Window occluded: window fully covered by another application for >= 10 s; same observable evidence
- [ ] Screen lock: screen locked for >= 30 s; same observable evidence
- [ ] Laptop suspend / resume: `freeze-repro-rafstall.md` notes that the first health-tick after resume must skip dead detection (suspend-gap path); no spurious hidden notification

## Performance Verification

- NFR1 (qualitative): rAF callback body runs only `nowFn()` + boolean comparison + boolean state update. Verified by code review (no allocations, no I/O).
- Implicit: no microbenchmark required; the loop is bounded by the browser's natural rAF cadence.

## Security Verification

Not applicable. Change is internal to the renderer and handles no input, network, or privileged data.

## Known Limitations / Deferred Work

- Full E2E regression (`./scripts/run-e2e-docker.sh test`) was not executed in this implementation pass. Reason: cycle time (~10 min build + run) and the static-review argument above (existing specs invoke backend Tauri commands directly and do not depend on the new rAF logic). To be executed by `sdd.6-verify`.
- New E2E spec `visibility-raf-heartbeat.e2e.js` was authored but not executed in this pass. Reason: same cycle-time concern. To be executed by `sdd.6-verify`.
- Manual repro per `freeze-repro-rafstall.md` is user-verifiable and intentionally deferred to a real desktop session.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests | 11 | 11 | 0 | 0 |
| E2E (new) | 3 | 0 | 3 | 0 |
| E2E (regression) | 4 | 0 | 4 | 0 |
| Typecheck | 1 | 1 | 0 | 0 |
| Manual scenarios | 4 | 0 | 0 | 4 |
| **Total** | **24** | **13** | **7** | **4** |
