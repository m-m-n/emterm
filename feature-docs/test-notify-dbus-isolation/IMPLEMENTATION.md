# Implementation Plan: test-notify-dbus-isolation

## Overview

The lib's unit-test build never reaches the real desktop-notification path
(notify-rust over D-Bus on Linux, the toast API on Windows): the `App` picks a
no-op notification sink in the test build, and `NotifyRustSink` can be
constructed with injected worker functions so the `worker_thread` tests run on
fakes on both OSes. The production notification behavior is unchanged.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate; every touched module is
  behind the `gui` feature.
- **Key libraries (all already dependencies)**:
  - notify-rust — the production send and, on unix, the capability query
  - crossbeam-channel — the bounded queue between `NotifyRustSink` and its
    worker thread
  - parking_lot — the existing locks
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Responsibility |
|-------|----------|----------------|
| App construction | `src-tauri/src/app/mod.rs` | Chooses the notification sink the `App` holds, by build configuration |
| Notification surface | `src-tauri/src/callbacks.rs` | `NotificationSink` abstraction, `NotifyRustSink`, bounded queue, worker thread, notify-rust wiring |
| Unit tests | `src-tauri/src/app/tests.rs`, `src-tauri/src/app/tests/`, `src-tauri/src/callbacks/tests.rs` | The lib's own test build (`--lib`) |

Allowed dependency direction: `app` → `callbacks` only. `callbacks` never
refers to `app`. Each test module uses only its parent module's items; no new
cross-module test helper is introduced.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `NotificationSink` trait (`crate::callbacks`) | OS-notification surface held by `App`, every tab and `NativeCallbacks` | The production surface stays exactly one operation, `send(title, body)`, with unchanged signature and semantics. Anything added for tests exists only in the test build and carries a default behavior, so every existing implementation (`NotifyRustSink`; the test sinks in `callbacks/tests.rs`, `app/tests/agent_status.rs`, `tabs/tests.rs`) compiles without edits. | task0001 (adds the test-build-only sink identification), task0002 (`NotifyRustSink` keeps implementing the trait unchanged) |
| `NotifyRustSink::new()` | Production construction of the desktop-notification sink | Public, no arguments; signature, visibility and `Default` delegation unchanged. Postcondition: a sink whose worker thread is already running, whose queue capacity is `NOTIFY_QUEUE_CAPACITY`, whose drop waits at most `NOTIFY_WORKER_JOIN_TIMEOUT`, and whose worker uses notify-rust's capability query (unix only) and notify-rust's send. It is the only sink constructor the non-test `App` calls. | task0001 (the non-test branch of `App::with_settings` calls it), task0002 (re-implements its body on top of the injection constructor) |
| `NotifyRustSink` type identity | The production sink type | Keeps its name and module path `crate::callbacks::NotifyRustSink`. task0002 adds no override of any test-build-only operation task0001 adds to `NotificationSink`, so the default answer for `NotifyRustSink` stays derived from its own type. | task0001 (its regression test identifies the held sink against this type), task0002 |

## Conventions

- **Build-configuration switch**: test-only behavior is selected by the lib's
  unit-test build condition (`cfg(test)`) only — never a cargo feature, an
  environment variable or a runtime flag. The production branch is its exact
  negation, so every build compiles exactly one of the two branches.
- **Platform split**: follows the existing unix / non-unix split in
  `callbacks.rs`. Windows code is compile-checked on the Linux host, not run
  (SPEC A7).
- **Logging**: no new log record. Existing notification records keep their
  exact wording and their redaction (NFR2).
- **Test isolation**: no lib test constructs a sink or a worker that can reach
  notify-rust. Tests inject fakes or rely on the test-build `App` sink (NFR1).
  Fakes record into thread-safe state owned by the test; no process-global
  state.
- **Timing assertions**: the existing bounds stay (send burst under 100 ms,
  drop under 2 s). Waiting for the worker to deliver to a fake is a bounded
  poll with a generous upper bound — never an unbounded wait, never reliant on
  the drop's own timing.
- **Warnings**: neither the test build nor the non-test build gains a new
  compiler warning in a touched file; an item used by only one build branch
  is scoped to that branch.

## Cross-task Design Decisions

### D1: Two isolation seams, no crate-wide detector

- **Decision**: isolation happens at `App` construction (task0001) and at
  `NotifyRustSink` construction (task0002). No mechanism scans or intercepts
  every test (SPEC A2, FR6).
- **Rationale**: these are the only two places that construct
  `NotifyRustSink` (SPEC A8).
- **Affected tasks**: task0001, task0002.

### D2: Production path unchanged; injection replaces only the outermost calls

- **Decision**: `NotifyRustSink::new()` and the non-test `App::with_settings`
  keep their observable behavior. Injected functions replace only the
  notify-rust capability query and send at the worker's edge; redaction, the
  unix on-demand capability gate and escape, the queue and the bounded
  shutdown remain shared production code that the tests also exercise.
- **Affected tasks**: task0001 (production branch), task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Integration tests under `src-tauri/tests/` link the non-test lib, so an `App::new()` there would hold the real sink | Low (none today, SPEC A4) | Medium | Out of scope (SPEC technical constraints); NFR1 is scoped to `--lib` |
| Windows-only code paths are compiled but not executed on the Linux host | Medium | Medium | Windows-target check including tests (TS-6) for both tasks; the Windows worker stays structurally parallel to the unix worker |
| Bounded-wait assertions flake under host load | Low | Low | Generous upper bounds; poll instead of a single fixed sleep |
| Both tasks edit `callbacks.rs` | Medium | Low | Disjoint regions (trait declaration vs. worker / sink section); parent-side adoption on merge |
| A build-configuration branch leaves an unused import or dead code in the non-selected branch | High | Low | Conventions: items used by one branch only are scoped to that branch; checked in both builds |

## Open Questions

- None.
