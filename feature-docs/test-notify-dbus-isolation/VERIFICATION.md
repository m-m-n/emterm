# Verification Document: test-notify-dbus-isolation

## Overview
**Feature**: test-notify-dbus-isolation / **SPEC.md**: `feature-docs/test-notify-dbus-isolation/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/test-notify-dbus-isolation/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/test-notify-dbus-isolation/THREAT-MODEL.md`

Run every command from the integration worktree root (the project root of the
checkout under verification), without changing directory.

## Build Verification
- Command (default features, from workflow.yaml `project.components.src-tauri.build_command`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, NFR3): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (Windows target including test code, NFR4): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests`
- Expected: exit code 0 and no errors for all three; no new compiler warning originating in a file this feature touches.

## Test Verification
- Command (from workflow.yaml `project.components.src-tauri.test_command`): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not measured (no coverage tool is configured for this component); every scenario below and the full pre-existing `--lib` suite must pass.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | In the test build, build `App::new()` and identify whether its `notification_sink` is a `NotifyRustSink` | Identified as not a `NotifyRustSink` | Unit (Rust, --lib) |
| TS-2 | In the test build, build `App::new()` and call `App::notify("t", "b")` | Returns without panicking | Unit (Rust, --lib) |
| TS-3 | Build `NotifyRustSink` with a recording fake send (on unix also a fake capability query) and send `NOTIFY_QUEUE_CAPACITY * 4` notifications | The sends complete in under 100 ms and the fake send receives at least one notification | Unit (Rust, --lib, unix / Windows) |
| TS-4 | Build `NotifyRustSink` with fakes, send 3 notifications, drop it | The drop returns in under 2 s and the fake send receives the 3 sent notifications | Unit (Rust, --lib, unix / Windows) |
| TS-5 | Build `NotifyRustSink` with fakes and drop it without sending | The drop returns in under 2 s | Unit (Rust, --lib, unix / Windows) |
| TS-6 | Run the default-features cargo check, the `--no-default-features` cargo check, and the Windows-target check including test code | All succeed | Build check |
| TS-7 | In a Linux desktop session, run the `--lib` tests and compare the notification daemon's history before and after | No test-originated notification is added | Manual |
| TS-8 | Only if a `worker_thread` test was renamed: match the `--lib -- --list` output against `test-docs/notification-worker-thread/task0001.tests.yaml` | Each name in the record appears as a `<name>: test` line | Record check (conditional) |

Windows: TS-3 to TS-5 are compiled for the Windows target by TS-6 and are not
executed on the Linux host (SPEC A7).

TS-8 listing command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`

## Code Quality Verification
- Format: none configured (`project.components.src-tauri.format_command` is empty); no crate-wide formatter run is part of this verification.
- Static analysis: compiler warnings from the test build (output of the test command) and from the three build commands; no new warning in a touched file (in particular no unused import of `NotifyRustSink` in `src-tauri/src/app/mod.rs` in the test build).
- NFR1 source check: search the lib's test sources (`src-tauri/src/**/tests.rs` and `src-tauri/src/**/tests/`) for constructions of `NotifyRustSink` through `new()` or `Default`. Expected: none. The remaining production callers are the non-test branch of `App::with_settings` and the `Default` implementation.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The test-build `App::new()` holds a sink that is not `NotifyRustSink`, and a test verifies it | TS-1 |
| AC-2 | The test-build `App::notify(title, body)` returns without panicking and sends nothing | TS-2; code review of the no-op sink |
| AC-3 | A `NotifyRustSink` built with a fake send delivers to the fake, and the existing timing checks still pass | TS-3, TS-4, TS-5 |
| AC-4 | The non-test `App::with_settings` uses `NotifyRustSink::new()`, which passes notify-rust's capability query and send (unix) / send (Windows) | Code review; TS-6 |
| AC-5 | Running the `--lib` tests in a Linux desktop session adds no t0-style notification to the daemon's history | TS-7 |
| AC-6 | The `--no-default-features` check and the Windows-target check including tests pass | TS-6 |
| AC-7 | If a `worker_thread` test was renamed, the predecessor record carries the new names and each resolves in the test listing | TS-8 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | Code review of the non-test branch; TS-6 |
| FR3 | task0002 | TS-3, TS-4, TS-5; code review of `NotifyRustSink::new()`; TS-6 |
| FR4 | task0002 | Code review of the Windows worker; TS-6 (Windows-target check) |
| FR5 | task0002 | TS-3, TS-4, TS-5 |
| FR6 | task0001, task0002 | TS-1 (App sink), TS-3 and TS-4 (fake reached) |
| FR7 | task0002 | TS-8 (conditional) |
| NFR1 | task0001, task0002 | TS-7; NFR1 source check |
| NFR2 | task0001, task0002 | TS-6; code review of constants, log wording and the unix gate order; pre-existing `worker_injection_points` / escape / redaction tests pass unmodified |
| NFR3 | task0001, task0002 | TS-6 (`--no-default-features` check) |
| NFR4 | task0001, task0002 | TS-6 (Windows-target check including tests) |

## Manual Testing (E2E Not Possible)
- [ ] TS-7: In a Linux desktop session, record the notification daemon's history, run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`, and compare the history again: no notification titled t0, t1, … or otherwise originating from the test run is added.

## Performance / Security Verification
- NFR2 timing invariants: TS-3 send burst under 100 ms; TS-4 and TS-5 drop under 2 s.
- TM-1: injected functions replace only the outermost capability query and send, and the unix worker order redact → on-demand capability gate → escape → send is unchanged — checked by code review of `NotifyRustSink::new()`, the worker start and the unix worker, plus the pre-existing `worker_injection_points`, `body_markup_escape`, `summary_markup_escape` and `notification_redaction` tests passing with their source unmodified.
- TM-2: the Windows worker logs only the redacted rendering on success and only the error value on failure — checked by code review of its two log records plus the Windows-target check (TS-6).
- TM-3: the test-build `App` holds a no-op sink — checked by TS-1 and TS-2, and by TS-7.
- TM-4: the `worker_thread` tests build the sink with fakes and no lib test uses the production constructor — checked by TS-3, TS-4, TS-5, the NFR1 source check, and TS-7.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (TS-6) | 3 | 3 | 0 | 0 |
| Unit tests (TS-1 to TS-5) | 5 | 5 | 0 | 0 |
| Record check (TS-8, conditional) | 1 | 1 | 0 | 0 |
| Code quality (warnings, NFR1 source check) | 2 | 2 | 0 | 0 |
| Manual (TS-7) | 1 | 0 | 0 | 1 |
| Code review (FR2, FR4, NFR2) | 3 | 0 | 0 | 3 |
| Security (TM-1 to TM-4) | 4 | 2 | 0 | 2 |
