# Verification Document: microtask-driven-pty-flow

## Overview

**Feature**: microtask-driven-pty-flow
**SPEC.md**: `doc/tasks/microtask-driven-pty-flow/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/microtask-driven-pty-flow/IMPLEMENTATION.md`

This document defines automated and manual verification for the replacement of the `requestAnimationFrame`-driven WASM parsing scheduler with a `MessageChannel`-based microtask scheduler in `src/terminal-app/pty-handler.ts`.

## Build Verification

| Component | Command | Expected |
|-----------|---------|----------|
| Frontend (TS bundle) | `bun run build` | Exit code 0, no type errors |
| Backend (Rust crate) | `cargo build --manifest-path src-tauri/Cargo.toml` | Exit code 0 (sanity only — no backend change) |

## Test Verification

| Component | Command | Expected |
|-----------|---------|----------|
| Frontend unit tests | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"` | All tests pass including TS-MT-1〜TS-MT-11 |
| Backend unit tests | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"` | All tests pass (sanity) |

Coverage target for `src/terminal-app/pty-handler.ts`: maintain at or above the pre-change line/branch coverage (no regression). New scheduler / destroy paths at ≥ 90%.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | One `scheduleProcessing()` call results in exactly one delivery via the chosen primitive (deduplicated by `processScheduled`) | One `port1.postMessage` (or one `queueMicrotask` / `setTimeout`) call observed | Unit |
| TS-2 | Two consecutive `scheduleProcessing()` calls before the microtask fires produce only one delivery | Single delivery observed; second call short-circuits at the `processScheduled` guard | Unit |
| TS-3 | When `MessageChannel` is undefined, `queueMicrotask` is selected; when both are unavailable, `setTimeout(0)` is selected and trigger label is `"timer"` | Trigger label observed in `processPendingData` matches the active fallback | Unit |
| TS-4 | A microtask whose captured `myToken` no longer matches `scheduleToken` (e.g. after `processPendingData` increments it directly) does NOT call `processPendingData` again | `processPendingData` invocation count matches expected (no double-fire) | Unit |
| TS-5 | `processPendingData` is called with `trigger="microtask"` on the primary path; no remaining call site passes `"raf"` | Trigger label observed; codebase has no `"raf"` literal in this file | Unit |
| TS-6 | When `process_pty_data` returns `consumed < input.length`, the handler schedules another microtask before returning | Second delivery observed after the first microtask completes with leftover | Unit |
| TS-7 | `pty-handler.ts` data path does not call `globalThis.requestAnimationFrame`. Real `CanvasRenderer` may still rAF for paint; this assertion is scoped to the data-consumption path only | `globalThis.requestAnimationFrame` spy receives zero calls when the test injects chunks through a stub renderer; static source scan in `pty-handler.ts` returns no `requestAnimationFrame(` (outside comments) | Unit + source-grep |
| TS-8 | Renderer call site (`forceRender` / equivalent) still operates as today; no structural change to the renderer path | Existing renderer-related tests pass without modification | Unit (regression) |
| TS-9 | `destroy()` closes both `MessagePort` instances and clears any pending `setTimeout` handle | Port `close` spy invoked twice; `clearTimeout` spy invoked when timer path is active | Unit |
| TS-10 | `pendingHandle` is `null` for `MessageChannel` / `queueMicrotask` paths, and a non-null `ReturnType<typeof setTimeout>` value for the `setTimeout` fallback | Field value matches expected primitive (typed identically to existing `ackFlushTimer`) | Unit |
| TS-11 | `rafScheduled` and `rafHandle` identifiers are removed from `pty-handler.ts` | Static source scan returns zero matches for those identifiers | Unit / source-grep assertion |
| TS-12 | Data path keeps draining when `globalThis.requestAnimationFrame` is monkey-patched to a no-op | `pty_get_send_stats(sessionId)` `sent_bytes` keeps increasing across samples (proves the reader is not blocked in `wait_for_drain` — i.e. acks are still flowing). No `backpressure stalled` warn captured during the rAF-stall window. | E2E (new spec) |
| TS-13 | Existing `visibility-raf-heartbeat` E2E continues to pass; rAF heartbeat still fires `setVisibility(false)` on rAF stall | Spec exits 0 | E2E (regression) |
| TS-14 | Sustained throughput within ±10% of baseline | Throughput-bench spec / manual measurement within tolerance | E2E + manual |
| TS-15 | All other existing E2E specs in `e2e-tests/specs/` pass without modification | Suite exits 0 | E2E (regression) |

## Code Quality Verification

| Item | Command / Method | Expected |
|------|------------------|----------|
| TypeScript typecheck | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"` | Exit 0, no errors |
| Rust format | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | No diff (no Rust change expected) |
| Source-grep on rAF removal | `git grep -n "rafScheduled\|rafHandle\|requestAnimationFrame" src/terminal-app/pty-handler.ts` | Zero hits for `rafScheduled` / `rafHandle`; `requestAnimationFrame` hits only in unrelated comments / log strings (or zero — the data path no longer calls it) |

## File Structure Verification

### Files to Create

- `e2e-tests/specs/microtask-data-flow.e2e.js` — new E2E spec proving NFR1 (data path drains when rAF stalled).

### Files to Modify

- `src/terminal-app/pty-handler.ts` — scheduler swap, rename `rafScheduled` -> `processScheduled`, rename `rafHandle` -> `pendingHandle`, update `ProcessTrigger` union, update `destroy()` cleanup.
- `src/terminal-app/pty-handler.test.ts` — add TS-MT-1〜TS-MT-11.
- `doc/tasks/microtask-driven-pty-flow/sdd.yaml` — fill in `requirements.{ID}.tasks` / `tests` arrays (done by sdd.2).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1〜FR11 implemented and covered by unit tests TS-MT-1〜TS-MT-11 | Run `bun test`; cross-check via the FR-to-test mapping in the next subsection |
| SC-2 | NFR1 satisfied (hidden-state operation) | E2E `microtask-data-flow.e2e.js` (TS-12) passes |
| SC-3 | NFR2 / NFR3 satisfied (throughput / latency within ±10%) | TS-14 + manual Perf-1 / Perf-2 |
| SC-4 | NFR5 satisfied (existing tests / safety nets unchanged) | TS-13, TS-15 + Phase 2 verification of `visibility-raf-heartbeat` / `visibility-aware-streaming` / `visibility-render-recovery` regression specs |
| SC-5 | All commands listed in §Build / §Test / §Code Quality Verification exit 0 | Local + Docker run |
| SC-6 | Manual freeze reproduction (US1 / US2) shows no freeze across 30-minute hidden window | Manual scenario in §Manual Testing |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (MessageChannel primary, port1.postMessage scheduling) | Phase 1 | TS-1, TS-2 |
| FR2 (queueMicrotask + setTimeout fallback chain) | Phase 1 | TS-3 |
| FR3 (rAF removed from data path; identifiers renamed) | Phase 1 | TS-7, TS-11 |
| FR4 (scheduleToken stale-callback invalidation preserved) | Phase 2 | TS-4 |
| FR5 (`"raf"` removed from trigger union; `"microtask"` / `"timer"` added) | Phase 1 | TS-5 |
| FR6 (leftover-data re-schedule via `scheduleProcessing()`) | Phase 1 | TS-6 |
| FR7 (ack coalescing unchanged) | regression | Existing pty-handler tests pass; TS-12 verifies acks continue under rAF stall |
| FR8 (WASM time budget preserved) | regression | Existing behavior preserved by rewrite scope; covered by TS-12 throughput observation |
| FR9 (Canvas rendering still rAF-driven) | unchanged | TS-8 |
| FR10 (visibility safety nets coexist unmodified) | regression | TS-13, TS-15 |
| FR11 (destroy closes ports, clears pendingHandle, flushes ack) | Phase 2 | TS-9, TS-10 |

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1 (hidden-state data path keeps running) | TS-12 (E2E) + manual US1 / US2 |
| NFR2 (throughput within ±10%) | TS-14 + manual Perf-1 |
| NFR3 (typing latency within ±10%) | manual Perf-2 |
| NFR4 (Linux + Windows compatibility) | TS-12 / TS-14 in Docker (Linux) + manual smoke on Windows build |
| NFR5 (existing tests / safety nets pass unmodified) | TS-13, TS-15 |
| NFR6 (log format unchanged except trigger label) | Inspection of `[DIAG-*]` lines during TS-12; assertion that warn-line shape is identical to baseline (only `trigger=microtask|timer` differs) |

## E2E Testing

Project E2E framework: WebdriverIO + tauri-driver under Docker (`./scripts/run-e2e-docker.sh test`).

- [ ] **TS-12 (NFR1)** — `microtask-data-flow.e2e.js` (new):
  - Stall `globalThis.requestAnimationFrame` (record-but-don't-invoke pattern from `visibility-raf-heartbeat.e2e.js`).
  - Drive a busy PTY producer (shell loop, ~5–8s).
  - Sample `pty_get_send_stats(sessionId)` returning `(sent_count, sent_bytes)` at intervals (~500ms) and assert that **both counters keep increasing** across the observation window. If the frontend stopped acking, `wait_for_drain` would block the reader thread and `sent_bytes` would plateau within ~1 second once `in_flight` reaches `HIGH_WATER_BYTES` (8 MiB). Continued growth therefore proves the data path is consuming and acking even with rAF stalled.
  - Assert no `backpressure stalled` warn line in the captured warn buffer during the rAF-stall window.
  - Restore real rAF; Ctrl+C the producer.

  Note: `pty_get_send_stats` does not currently expose `in_flight` directly. The `sent_bytes` growth invariant is the available proxy for "the reader is not blocked", and is sufficient because backend `wait_for_drain` is the only mechanism that would stall the reader on the PTY-output path.
- [ ] **TS-13 (FR10)** — `visibility-raf-heartbeat.e2e.js` (existing) passes unchanged.
- [ ] **TS-14 (NFR2)** — `visibility-throughput-bench.e2e.js` (existing) reports throughput within ±10% of pre-change baseline.
- [ ] **TS-15 (NFR5)** — `./scripts/run-e2e-docker.sh test` runs the full suite and exits 0:
  - `freeze-regression.e2e.js`
  - `visibility-aware-streaming.e2e.js`
  - `visibility-resume-block.e2e.js`
  - `multi-tab.e2e.js`
  - `mux.e2e.js`, `mux-multi-session.e2e.js`, `mux-reattach.e2e.js`, `mux-move-window.e2e.js`
  - `cursor-blink.e2e.js`, `cursor-visibility.e2e.js`
  - All other specs in `e2e-tests/specs/`.

## Manual Testing (E2E Not Possible)

These require subjective evaluation, physical hardware behaviour, or wall-clock durations beyond Docker E2E's reasonable runtime budget.

- [ ] **Perf-1 (NFR2)** — Throughput micro-benchmark:
  - Run `yes | head -c 100M` in eMterm before and after the change. Record total wall time. Verify within ±10%.
- [ ] **Perf-2 (NFR3)** — Typing latency:
  - Type a sustained sequence and observe key-to-paint round trip subjectively (or via existing benchmark spec). Verify no perceptible regression.
- [ ] **US1** — Long workspace-switch reproduction:
  - Launch eMterm, run `while true; do date; sleep 0.05; done`.
  - Switch workspace and lock the desktop for ≥ 30 minutes.
  - Return; assert UI responds immediately and the most recent timestamps are visible.
- [ ] **US2** — Long-minimize reproduction:
  - Start a long-running build that emits to stdout. Minimize the window for ≥ 30 minutes. Restore.
  - Assert no frontend backlog accumulates and the build output is fully delivered.
- [ ] **Cross-platform smoke (NFR4)** — Run a short streaming workload on the Windows (WebView2) build and verify the same trigger labels appear in the Windows log file.

## Performance Verification

- **NFR2 (throughput)**: ±10% of `yes | head -c 100M` baseline — verified via TS-14 (E2E) and Perf-1 (manual).
- **NFR3 (latency)**: ±10% of typing-latency baseline — verified via Perf-2 (manual).
- **CPU regression**: realistic-load CPU utilization within ~1% of baseline (rough manual observation; no automated check).

## Security Verification

Not applicable. The change is internal to the renderer process and uses standard browser primitives. No cross-origin or network surface is introduced.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | 11 (TS-1〜TS-11) | 11 | 0 | 0 |
| E2E specs | 4 scenarios (TS-12〜TS-15) | 4 | 4 | 0 |
| Manual scenarios | 5 (Perf-1, Perf-2, US1, US2, NFR4 smoke) | 0 | 0 | 5 |
| Build / typecheck | 3 (frontend build, frontend typecheck, backend build) | 3 | 0 | 0 |
| **Total** | **23** | **18** | **4** | **5** |

(One item — TS-14 — is counted under both Automated/E2E and Manual because it has both an E2E component and a manual cross-check; it is counted once in the row totals.)
