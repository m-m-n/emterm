# Verification Document: Background OSC Notification Detection

## Overview
**Feature**: background-osc-notification
**SPEC.md**: `doc/tasks/background-osc-notification/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/background-osc-notification/IMPLEMENTATION.md`

## Build Verification
- Backend: `cargo build --manifest-path src-tauri/Cargo.toml` — exit 0, no errors.
- Frontend: `bun run typecheck` — exit 0, no type errors.
- (Docker-first per project policy.)

### Actual Results (implementation)
- Backend build (gui feature), Docker: `cargo build --manifest-path src-tauri/Cargo.toml --features gui` — exit 0, no errors.
- Frontend typecheck, Docker: `bun run typecheck` (`tsc --noEmit`) — exit 0, no type errors.
- Code quality: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — FMT CLEAN; `cargo clippy --manifest-path src-tauri/Cargo.toml` — no feature-related warnings (Finished).

## Test Verification
- Backend: `cargo test --manifest-path src-tauri/Cargo.toml`
- Frontend: `bun test`
- Coverage target: scanner/recognition logic 90%+, delivery wiring covered by unit tests.

### Actual Results (implementation, Docker)
- Backend `cargo test` — 1017 unit passed, 0 failed, 1 ignored (pre-existing); integration suites 10/10, 7/7, 6/6, 4/4 — all green.
  - New backend tests: `passthrough_scanner` (10 new OSC 9 cases), `visibility` (3 new TS-8/FR4/FR5 cases), `protocol` (3 new Notify cases), `pty_spawn::tests` (3 new TS-9/FR4/chunk-split cases).
- Frontend `bun test` — 2374 passed (feature tests all green). The single remaining `fail`/`error` is pre-existing in `src/clipboard/manager.test.ts` ("Permission denied" rejected-promise path; file unmodified by this feature; passes in isolation 35/35).
  - New frontend tests: `background-notification-listener.test.ts` (5: TS-10, TS-12, subscribe, fire-once, unlisten), `mux-client.test.ts` (8: Notify type, `decodeBincodeString`, TS-11, TS-12, truncated), `osc-handler-notification.test.ts` (3: TS-13, SC-4, FR6).
- Mock-fidelity fix: added `info` to the leaked `muxLog` mock in `pty-handler.test.ts` (the real `muxLog` has `info`); resolved 3 full-suite mock-leak failures surfaced by the new dispatch path.

### Test Scenario Results
| ID | Result | Where |
|----|--------|-------|
| TS-1 | PASS | `passthrough_scanner::osc9_notification_bel_terminated_is_recognized` |
| TS-2 | PASS | `passthrough_scanner::osc9_notification_st_terminated_is_recognized` |
| TS-3 | PASS | `passthrough_scanner::osc9_progress_is_not_a_notification` |
| TS-4 | PASS | `passthrough_scanner::osc9_split_across_chunks_is_recovered` |
| TS-5 | PASS (unchanged) | `passthrough_scanner::partial_buffer_overflow_drops_sequence_and_warns_once` |
| TS-6 | PASS | `passthrough_scanner::osc0_title_is_not_a_notification` |
| TS-7 | PASS | `passthrough_scanner::osc9_notification_and_replay_passthrough_are_separated`, `osc9999_markdown_is_replay_not_notification` |
| TS-8 | PASS | `visibility::visibility_process_hidden_surfaces_osc9_notification` |
| TS-9 | PASS | `pty_spawn::capture_passthrough_forwards_osc9_notification` |
| TS-10 | PASS | `background-notification-listener.test.ts` |
| TS-11 | PASS | `mux-client.test.ts` MuxClient Notify dispatch |
| TS-12 | PASS | `background-notification-listener.test.ts` + `mux-client.test.ts` + `osc-handler-notification.test.ts` (FR6) |
| TS-13 | PASS | `osc-handler-notification.test.ts` |
| TS-14 | PASS (by design) | Daemon scanner runs ONLY on Detached arms (`capture_passthrough`); Connected/active panes never run it → no double-fire. |

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Scan `OSC 9 ; msg` terminated by BEL | Recognized as notification, message=`msg` | Unit (Rust) |
| TS-2 | Scan `OSC 9 ; msg` terminated by ST (`ESC \`) | Recognized as notification | Unit (Rust) |
| TS-3 | Scan `OSC 9 ; 4 ; 1 ; 50` (progress) | NOT a notification | Unit (Rust) |
| TS-4 | `OSC 9` split across chunk boundaries | Recognized after the closing chunk | Unit (Rust) |
| TS-5 | Never-terminating sequence past `PARTIAL_SEQUENCE_MAX` | Dropped, single warn, scanner resets | Unit (Rust) |
| TS-6 | `OSC 0 ; title` and other OSC | NOT a notification | Unit (Rust) |
| TS-7 | Notification output kept separate from replay-passthrough output | Replay buffer excludes OSC 9; image/Markdown extraction unchanged | Unit (Rust) |
| TS-8 | `process_hidden` with `OSC 9 ; msg` | Surfaces a notification message; passthrough buffer unaffected | Unit (Rust) |
| TS-9 | Detached pane output with `OSC 9 ; msg` | Produces a forwarded notification control message | Unit (Rust) |
| TS-10 | Frontend in-process notification event | Calls `sendNotification("eMterm", msg)` | Unit (TS) |
| TS-11 | `mux-client` receives notification message | Calls `sendNotification("eMterm", msg)` | Unit (TS) |
| TS-12 | Notification permission not granted | No notification sent | Unit (TS) |
| TS-13 | Non-active regular tab emits `OSC 9 ; msg` (window visible) | Notification callback fires | Unit/Integration (TS) |
| TS-14 | Active/`Connected` mux pane emits `OSC 9 ; msg` | Daemon does NOT forward (no double-fire); GUI foreground fires once | Unit (Rust) |

## Code Quality Verification
- Backend format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (and `--check` in CI).
- Frontend: `bun run typecheck`.

### Actual Results (implementation, Docker)
- `cargo fmt --manifest-path src-tauri/Cargo.toml` applied; `--check` → FMT CLEAN.
- `cargo clippy --manifest-path src-tauri/Cargo.toml` → no feature-related warnings.
- `bun run typecheck` → exit 0.

## File Structure Verification
### Files to Modify
- [x] `src-tauri/src/pty/passthrough_scanner.rs` — OSC 9 notification recognition (separate `take_notifications` output).
- [x] `src-tauri/src/pty/visibility.rs` — `process_hidden` returns surfaced notification messages.
- [x] `src-tauri/src/reader.rs` — `emit_osc_notifications` emits the `osc_notification` Tauri event on the hidden path.
- [x] `src-tauri/src/payloads.rs` — added `OscNotificationPayload`.
- [x] `src-tauri/src/mux/ipc/pty_spawn.rs` — `capture_passthrough` forwards OSC 9 from Detached arms only.
- [x] `src-tauri/src/mux/ipc/protocol.rs` — `MessageType::Notify (0x1C)` + `NotifyMsg`.
- [x] `src-tauri/src/mux/ipc/connection.rs` + `handlers.rs` — thread daemon notification sender; reuse `notify_tx` broadcast → GUI.
- [x] `src-tauri/src/mux/session/pane.rs` — `NotificationSender`/`SharedNotificationSender` + pane field.
- [x] `src-tauri/src/mux/daemon.rs` — daemon notification channel + `run_notification_task` (Unix + Windows).
- [x] `src/terminal/mux/mux-client.ts` — `Notify` type, dispatch case, `decodeBincodeString`, `setOnNotify`.
- [x] `src/terminal-app/mux/mux-session.ts` — wire `setOnNotify` → `sendNotification`.
- [x] `src/terminal-app/index.ts` — register in-process `osc_notification` listener + dispose cleanup.
- [x] `src/types/pty.ts` — `OscNotificationPayload` type.

### New files
- [x] `src/terminal/background-notification-listener.ts` — in-process event listener → sink.
- [x] `src/terminal/background-notification-listener.test.ts`, `src/terminal-app/osc-handler-notification.test.ts`.

### Files Reused (no behavior change)
- `src/terminal/osc-notification.ts` — `sendNotification` sink (unchanged).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | OSC 9 fires while window hidden | Manual (minimize + emit) + TS-8/TS-10 |
| SC-2 | OSC 9 fires for mux non-active pane/window | Manual (mux) + TS-9/TS-11 |
| SC-3 | OSC 9 fires for non-active regular tab | Manual + TS-13 |
| SC-4 | Progress (`9;4`) does not fire background notification | TS-3 |
| SC-5 | No duplicate firing on resume/reattach | Manual + design (excluded from replay) |
| SC-6 | Foreground OSC 9 behavior unchanged | Regression (existing foreground path untouched) |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 Hidden-window detection | Phase 2 | TS-8, TS-10, manual SC-1 |
| FR2 mux detached detection | Phase 3 | TS-9, TS-11, manual SC-2 |
| FR3 Non-active regular tab | Phase 4 | TS-13, manual SC-3 |
| FR4 Progress excluded | Phase 1 | TS-3 |
| FR5 No duplicate on resume/reattach | Phase 1/2/3 | TS-7, manual SC-5 |
| FR6 Content + permission | Phase 2/3 | TS-10, TS-11, TS-12 |
| FR7 BEL/ST + chunk split | Phase 1 | TS-1, TS-2, TS-4 |
| NFR1 Performance/no foreground impact | Phase 1 | Foreground path unchanged; scan only on background path |
| NFR2 Bounded partial buffer | Phase 1 | TS-5 |
| NFR3 GUI-only OS notification; daemon forwards | Phase 2/3 | TS-9/TS-11 (daemon forwards; sink in GUI) |
| NFR4 Linux/Windows only | all | Build on supported targets |
| NFR5 Foreground unchanged | Phase 3 | SC-6 regression; TS-14 (no active-pane double-fire) |

## E2E Testing
OS desktop notification firing is not automatable in the headless Docker E2E environment (no notification daemon under Xvfb). E2E suite is run for regression only:
- [ ] `./scripts/run-e2e-docker.sh test` passes without regression.

### Implementation-phase note (Phase 3.8)
Per project testing policy, the full E2E suite is NOT run during the TDD cycle; it is deferred to the final verify step (sdd.6). The implementation phase relied on unit/integration coverage (Rust + TS) which is all green. No automatable E2E assertion exists for OS-notification firing (manual verification below). No E2E regression executed in this phase by design.

## Manual Testing (E2E Not Possible)
- [ ] Minimize window; `printf '\033]9;done\007'`; observe one OS notification; restore window; no second notification.
- [ ] mux non-active pane/window emits the sequence; one OS notification via GUI; reattach; no second notification.
- [ ] Two regular tabs (mux off); emit from the non-active tab; one OS notification.
- [ ] Foreground (active/visible) `OSC 9 ; msg` and `OSC 9 ; 4 ; …` progress behave exactly as before.

## Performance Verification
- Background scan adds per-byte overhead comparable to the existing `PassthroughScanner`; foreground (visible/active) hot path is untouched.

## Security Verification
- [ ] Notification fires only when `isPermissionGranted()` is true (TS-12).
- [ ] Notification body is the raw OSC 9 message, identical to the existing foreground path.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Scanner recognition | TS-1..TS-7 | 7 | 0 | 0 |
| Backend delivery | TS-8, TS-9 | 2 | 0 | 0 |
| Frontend delivery | TS-10, TS-11, TS-12, TS-13 | 4 | 0 | 0 |
| Situations / no-double-fire | SC-1..SC-6 | partial | regression | 4 |
