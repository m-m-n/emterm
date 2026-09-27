# Verification Document: notify-queue-worker-gone

## Overview

**Feature**: notify-queue-worker-gone / **SPEC.md**: `feature-docs/notify-queue-worker-gone/SPEC.md` / **IMPLEMENTATION.md**: not produced (reduced tier, single task)

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, all tests pass
- Coverage target: not measured (workflow.yaml defines no coverage command)

### Test Scenarios from SPEC.md

SPEC.md numbers its scenarios TS1 to TS5; they appear here as TS-1 to TS-5.
TS-6 to TS-8 are inspection scenarios for SPEC.md requirements that are
structural or not observable without log capture.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Build a queue with `NotifyQueue::new(NOTIFY_QUEUE_CAPACITY)`, drop its receiving side, call `try_submit` once | Returns `WorkerGoneReported`; neither `DroppedEpisodeStart` nor `DroppedAlreadyWarned` | Unit |
| TS-2 | Drop the receiving side, then call `try_submit` several times | The first call returns `WorkerGoneReported`, every later call returns `WorkerGoneAlreadyReported`; no call returns a saturation variant | Unit |
| TS-3 | Hold the receiving side without draining, submit until `DroppedEpisodeStart` is returned, drop the receiving side, call `try_submit` | Returns `WorkerGoneReported` | Unit |
| TS-4 | Drop the receiving side, call `try_submit` concurrently from several threads on the shared queue | Exactly one result is `WorkerGoneReported`, all others are `WorkerGoneAlreadyReported` | Unit |
| TS-5 | Run the full `--lib` suite, including the existing worker_thread tests | All pass; `queue_drops_the_ninth_submission_without_blocking_when_receiver_is_idle`, `saturation_episode_warns_exactly_once_then_rearms_after_a_successful_submission` and `concurrent_saturated_drops_produce_exactly_one_episode_start_warning` pass without modification | Integration |
| TS-6 | Inspect `NotifyQueue::try_submit` | The send result is handled in three separate branches (accepted / full / disconnected) with no catch-all error branch; the accepted and full branches neither read nor write the worker-gone flag; the disconnected branch neither reads nor writes `armed`; the worker-gone flag is never cleared | Inspection |
| TS-7 | Inspect the worker-gone log record | Emitted at error level with `LOG_NOTIFY_WORKER_DEAD`, which sits in the same log constant section as `LOG_NOTIFY_QUEUE_SATURATED`; notification content appears only as `redact_notification` metadata (length and diag_id); no title or body text | Inspection |
| TS-8 | Inspect the change boundaries | `NotificationSink` trait signature and `NotifyRustSink::send` contract unchanged; no blocking or notification-daemon-dependent operation added to `try_submit`; neither the change nor the new tests are platform-gated; the new tests are in the worker_thread module of `src-tauri/src/callbacks/tests.rs` and use neither `NotifyRustSink` nor notify-rust / D-Bus | Inspection |

## Code Quality Verification

- Format: not configured (`format_command` is empty in workflow.yaml)
- Static analysis: not configured

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The first `try_submit` on a queue whose receiver was dropped returns the worker-gone "record emitted" variant, not `DroppedEpisodeStart` / `DroppedAlreadyWarned` | TS-1 |
| AC2 | After a saturation episode disarmed `armed`, dropping the receiver still makes the next `try_submit` return the worker-gone "record emitted" variant | TS-3 |
| AC3 | Repeated `try_submit` after receiver drop emits the error record once; later calls return the worker-gone "already recorded" variant; the flag is not reset by the episode re-arm | TS-2, TS-4, TS-6 |
| AC4 | The existing saturation tests pass without modification | TS-5 |
| AC5 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes | TS-5 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6 |
| FR2 | task0001 | TS-3, TS-5 |
| FR3 | task0001 | TS-1, TS-7 |
| FR4 | task0001 | TS-2, TS-3, TS-4, TS-6 |
| FR5 | task0001 | TS-1, TS-2 |
| FR6 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-8 |
| NFR1 | task0001 | TS-5, TS-8 |
| NFR2 | task0001 | TS-4, TS-5, TS-8 |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-8 |

## Manual Testing (E2E Not Possible)

- [ ] TS-6: inspect the branch structure of `NotifyQueue::try_submit` and the flag isolation
- [ ] TS-7: inspect the worker-gone log record (level, constant, redaction)
- [ ] TS-8: inspect the change boundaries (sink trait, non-blocking, platform gating, test placement)

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests (TS-1 to TS-4) | 4 | 4 | 0 | 0 |
| Integration (TS-5) | 1 | 1 | 0 | 0 |
| Inspection (TS-6 to TS-8) | 3 | 0 | 0 | 3 |
