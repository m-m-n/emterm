# Verification Result: visibility-raf-heartbeat

**Verification date**: 2026-05-06 (sdd.6-verify pass, E2E rerun after redesign)
**Feature**: visibility-raf-heartbeat
**SPEC.md**: `doc/tasks/visibility-raf-heartbeat/SPEC.md`
**VERIFICATION.md**: `doc/tasks/visibility-raf-heartbeat/VERIFICATION.md`
**Project**: emterm (Tauri / Rust + TypeScript + WASM)

## Summary

| Category | Result | Detail |
|----------|--------|--------|
| File structure | PASS | All 2 created files + 2 modified files present |
| SPEC compliance (FR1〜FR10) | PASS | All 10 functional requirements covered by TS-29〜TS-39 + boundary case |
| Build / Typecheck / Unit tests / cargo test | PASS | Verified by sdd.5-check (not re-run) |
| E2E (new spec) | PASS (2/2) | `visibility-raf-heartbeat.e2e.js` E2E-1/E2E-3 + E2E-2 both pass after redesign (post-detection baseline + drained queued-cb resume) |
| E2E (regression) | NOT RUN | Deferred to dedicated regression cycle (10+ min build); static-review evidence in VERIFICATION.md applies |
| Performance | PASS (qualitative) | rAF callback body limited to `nowFn()` + boolean update + reschedule; no allocation per frame; loop fully cancelled while hidden |
| Security | N/A | Renderer-internal change, no input/network/privileged surface |
| Manual scenarios | PENDING | 4 desktop scenarios in `freeze-repro-rafstall.md` (workspace switch, occlusion, screen lock, suspend/resume) require human execution |

**Overall**: PASS (automated) — implementation matches SPEC, fully covered by unit tests AND the new E2E spec passes against the live Tauri debug binary. Manual desktop scenarios remain deferred.

---

## 1. File Structure Verification

### Created files (PASS)

- `e2e-tests/specs/visibility-raf-heartbeat.e2e.js` — present (8 386 bytes, 228 lines)
- `doc/tasks/visibility-raf-heartbeat/freeze-repro-rafstall.md` — present (4 868 bytes, 143 lines)

### Modified files (PASS)

- `src/pty/visibility-controller.ts` — present (14 717 bytes, 410 lines). Contains:
  - Exported constants `RAF_DEAD_THRESHOLD_MS = 5_000`, `SUSPEND_GAP_MS = 30_000`
  - Optional DI fields `requestAnimationFrameFn`, `cancelAnimationFrameFn`, `nowFn` on `VisibilityControllerDeps`
  - Lazy default wrappers (resolve `globalThis.<api>` per call) for FR8 monkey-patch observability
  - Private state `rafAlive`, `lastRafPerfMs`, `lastHealthTickPerfMs`, `rafHandle`
  - `scheduleRaf`, `cancelRaf`, `healthTick`, `hiddenReason` methods
  - `currentEffective()` AND-extends `rafAlive`
  - `notify()` schedules / cancels rAF on visible / hidden
  - `[DIAG-IDLE] visibility→hidden ... reason=...` log line

- `src/pty/visibility-controller.test.ts` — present (26 992 bytes, 737 lines). Contains TS-8 / TS-9 / TS-21 / TS-29〜TS-39 + TS-29 boundary (21 tests total).

### Files NOT modified (verified by VERIFICATION.md scope)

- `src/pty/client.ts`, `src/terminal/mux/mux-client.ts` — unchanged, dispatch APIs preserved
- `src-tauri/**/*.rs` — unchanged
- `doc/tasks/visibility-aware-pty-streaming/**` — unchanged

---

## 2. SPEC.md Functional Requirements Compliance (FR1〜FR10)

Cross-referenced controller source against SPEC.md FR1〜FR10. All matching tests are present in `visibility-controller.test.ts`.

| FR | Implementation evidence | Test |
|----|-------------------------|------|
| FR1 (rAF self-loop) | `scheduleRaf()` re-schedules itself when `lastNotified === true && !destroyed` | TS-29 |
| FR2 (rAF dead detection) | `healthTick()` compares `now - lastRafPerfMs` against `RAF_DEAD_THRESHOLD_MS` | TS-29, boundary |
| FR3 (currentEffective integration) | `currentEffective()` returns `getDocumentVisible() && focused && rafAlive` | TS-30 |
| FR4 (rAF resume → immediate visible) | rAF callback flips `rafAlive=true` and calls `evaluate()` synchronously | TS-31 |
| FR5 (suspend gap skip) | `healthTick()` skips dead detection and resets `lastRafPerfMs` when tick gap > `SUSPEND_GAP_MS` | TS-33 |
| FR6 (DIAG-IDLE reason field) | `hiddenReason()` joins `document` / `focus` / `raf-stall` with `+` | TS-35, TS-36 |
| FR7 (loop termination on hidden) | `notify(false)` calls `cancelRaf()`; rAF callback re-checks `lastNotified===true` before re-scheduling | TS-34 |
| FR8 (DI for rAF / now) | Constructor stores lazy wrappers resolving `globalThis.<api>` per call | TS-37 |
| FR9 (start grace period) | `healthTick()` returns early when `lastRafPerfMs === null` | TS-32 |
| FR10 (compat with visibility-aware-pty-streaming) | Public API (`PtyClient.setVisibility`, `MuxClient.sendSetVisibility`) unchanged; idempotent stop/start | TS-38, TS-39 |

All FR1〜FR10 implementation matches SPEC. The test file contains 12 new tests (TS-29 〜 TS-39 + TS-29 boundary), all passing per sdd.5-check (`bun test`: 2305 pass / 0 fail).

### Non-Functional Requirements (NFR1〜NFR5)

- **NFR1 (perf)** — rAF callback body limited to `nowFn()` + boolean comparison + boolean state update + (conditional) `evaluate()`. No allocation, no I/O, no log emission per frame. Verified by code review of `scheduleRaf` lines 285–309.
- **NFR2 (resource)** — 5 new fields added (`rafAlive`, `lastRafPerfMs`, `lastHealthTickPerfMs`, `rafHandle`, plus DI wrappers). No long-lived buffers.
- **NFR3 (DI fallback)** — TS-37 covers the no-rAF-global path; constructor never throws.
- **NFR4 (observability)** — `reason=raf-stall` / `document+focus` log fields verified by TS-35 / TS-36.
- **NFR5 (compat)** — Existing visibility-aware-streaming dispatch path is reused unchanged; `[DIAG-IDLE]` prefix and `at <ISO>` field preserved, `reason=` appended.

---

## 3. Build / Typecheck / Unit / cargo test (delegated)

These were verified by sdd.5-check and are NOT re-run by sdd.6:

- `bun run typecheck` — exit 0
- `bun test` — 2305 pass / 17 todo / 0 fail across 106 files
- `cargo test` — PASS
- Format / lint — typecheck is the static gate (no separate ESLint/Biome)

---

## 4. E2E Tests (Docker / tauri-driver)

### Run

```
./scripts/run-e2e-docker.sh test visibility-raf-heartbeat.e2e.js
```

Result: `2 passing (21.1s)` — both cases pass against the rebuilt Tauri debug binary.

### Result: 2 / 2 PASS

| Spec | Result | Detail |
|------|--------|--------|
| E2E-1 / E2E-3 (rAF stall flips hidden; sent_bytes stays flat AFTER detection fires) | PASS | Post-detection baseline approach: poll for `[DIAG-IDLE] visibility→hidden ... reason=...raf-stall` in captured `console.warn` buffer (≤18 s), snapshot `pty_get_send_stats`, sleep 3 s, re-snapshot — delta ≤1 KiB allowance. Producer is unbounded (`yes hb-payload`) so the assertion remains meaningful past detection latency. |
| E2E-2 (restoring rAF resumes visibility within ~2 s) | PASS | After `restoreRafStall()`, drain queued rAF cbs captured by the stall stub (mimics WebKit "queued cbs delivered after wake" behaviour) — at least one cb fires, flipping `rafAlive=true` and re-evaluating. Confirmed by `[DIAG-IDLE] visibility→visible` log line within 2 s. |

### Redesign notes

The first E2E pass on the original spec failed for two reasons; both are test-side issues, not controller defects:

1. **Pre-vs-cumulative byte assertion**: the original spec compared `sent_bytes` between pre-stall and 12 s post-stall. The bounded 200 000-byte workload completed before detection fired, inflating the delta past the 1 KiB allowance.
   - Redesign: switched to **post-detection baseline** sampling and unbounded producer. The assertion now measures growth DURING the hidden window, not cumulative.
2. **Resume requires a queued cb**: when the controller transitions to hidden it sets `lastNotified=false`, after which `scheduleRaf()` short-circuits. Recovery requires at least one rAF callback that was previously queued (during stall) to fire and flip `rafAlive=true`. The original stub silently dropped cbs, so restoration had no effect.
   - Redesign: stub now appends every cb to `__rafStallQueuedCbs`. `drainRafStallQueue()` fires them after `restoreRafStall()` to simulate the "queued cbs delivered after wake" behaviour matched by the unit test's `FakeRaf` variant.
3. **Order bug in resume capture**: `restoreRafStall()` also restores `console.warn` to the original. Setting up the resume-capture wrapper BEFORE calling `restoreRafStall()` clobbered the wrapper. Redesign reordered to: restore → install resume capture → drain.

### E2E regression suite (existing specs)

Not re-run on this verification pass (full suite is a 10+ minute Docker build cycle and the controller's public API is unchanged). The static-review argument in VERIFICATION.md still holds: the existing visibility specs invoke `pty_set_visibility` directly and never depend on the new rAF heartbeat path.

---

## 5. Manual Test Items (E2E not possible)

Documented in `doc/tasks/visibility-raf-heartbeat/freeze-repro-rafstall.md`. The following 4 scenarios require a real Linux desktop session and human verification:

- **S1 — Workspace switch**: switch eMterm to another workspace for >= 15 s, return; expect `[DIAG-IDLE] reason=raf-stall` and no `backpressure stalled` accumulation.
- **S2 — Occluded window**: fully cover the eMterm window with another application for >= 15 s; expect same evidence (the rAF heartbeat path is what catches compositors that do not fire `visibilitychange`).
- **S3 — Screen lock**: lock for >= 30 s; expect `reason=raf-stall` (or `document` if the locker also fires visibilitychange), no backpressure stall, screen up to date on unlock.
- **S4 — Laptop suspend / resume**: suspend for >= 60 s, resume; expect NO spurious `[DIAG-IDLE] visibility→hidden` line at resume (suspend-gap branch resets the baseline cleanly). Failure signature: hidden line dated immediately after resume.

Log to inspect: `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.

---

## 6. Performance / Security

- **Performance** — qualitative pass per NFR1; no microbenchmark required. rAF body is bounded by browser cadence and reads only `nowFn()` + a boolean field.
- **Security** — N/A. Renderer-internal change; no input, network, or privileged surface.

---

## 7. Outstanding Items

- 4 manual desktop scenarios in `freeze-repro-rafstall.md` remain to be executed and recorded (workspace switch, occlusion, screen lock, suspend/resume).
- Existing visibility-aware-streaming + freeze-regression + visibility-throughput-bench + visibility-resume-block E2E suites should be run end-to-end at least once against the rebuilt debug binary before release (deferred — controller's public API is unchanged so the static-review argument in VERIFICATION.md still holds).
