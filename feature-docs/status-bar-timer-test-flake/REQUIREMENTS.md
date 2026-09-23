---
title: "status-bar-timer-test-flake"
created_date: 2026-09-23
status: draft
---

# status-bar-timer-test-flake - Requirements

## 1. Overview

### 1.1 Background

Repeated runs of
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
fail on the status-bar TimeProvider timer tests. These tests currently count
wakes and version bumps inside a fixed real-time window
(`std::thread::sleep`), so their result depends on how much real time passes.

The TimeProvider timer loop takes its ticks from
`Condvar::wait_timeout(guard, interval)` in `timer_loop` (time.rs:158).

### 1.2 Objectives

- Make the pass/fail result of the status-bar TimeProvider timer tests
  independent of real time passing.
- Add a regression test that catches the flake if it comes back.

### 1.3 Scope

In scope:

- An internal seam that makes the TimeProvider timer tick source replaceable
  (crate-internal or test-only).
- Rewriting `time_provider_timer_thread_calls_wake_on_interval`,
  `time_provider_timer_thread_bumps_version_per_tick_without_get_value`
  (time.rs) and `runtime_time_provider_timer_fires_wake` (runtime.rs) on top
  of the seam.
- A new delayed-tick regression test.
- Correcting the stale comment in `runtime_time_provider_timer_fires_wake`.

Out of scope:

- `time_provider_without_timer_does_not_spawn_thread`. It sleeps 40 ms but
  asserts a count of 0, which real time passing cannot break. It is left
  unchanged.
- Any change to production timer behaviour or to public signatures.

## 2. Business Requirements

### 2.1 Business Goals

- Make the pass/fail result of the status-bar TimeProvider timer tests
  independent of real time passing, so repeated runs of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  stop failing on these tests.
- Add a regression test that catches the flake if it comes back.

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| Developer | Runs the crate's `--lib` test suite |

### 2.3 Expected Effects

- Repeated `--lib` test runs show no failures from the timer tests listed in
  section 11.1.
- A reintroduced real-time-window check is caught by the regression test.

## 3. Use Cases

Not applicable.

## 4. Functional Requirements

### 4.1 Function List

| ID | Name | Priority |
|----|------|----------|
| FR1 | Injectable tick source for the TimeProvider timer loop | High |
| FR2 | Production timer behaviour unchanged | High |
| FR3 | Synchronous manual-tick delivery | High |
| FR4 | Seam-driven shutdown | High |
| FR5 | Rewrite time_provider_timer_thread_calls_wake_on_interval | High |
| FR6 | Rewrite time_provider_timer_thread_bumps_version_per_tick_without_get_value | High |
| FR7 | Rewrite runtime_time_provider_timer_fires_wake | High |
| FR8 | Delayed-tick regression test | High |

### 4.2 Function Details

#### FR1: Injectable tick source for the TimeProvider timer loop

The TimeProvider timer loop gets its ticks from
`Condvar::wait_timeout(guard, interval)` in `timer_loop` (time.rs:158). This
tick source becomes replaceable through an internal seam that is visible only
inside the crate or to tests. Tests supply a manual tick source and deliver
ticks one at a time.

#### FR2: Production timer behaviour unchanged

- The production path (`TimeProvider::with_wake` and `StatusBarRuntime::new`)
  keeps using `Condvar::wait_timeout` with the configured
  `RefreshConfig.interval`. The interval defaults to 1000 ms or comes from
  `refresh_rates["time"]`.
- Each tick still does `version.fetch_add(1)` before calling `wake()`.
- `Drop` still sets `stop`, calls `notify_all`, and joins the timer thread
  promptly.
- `TimeProvider::new` still spawns no thread.
- `version()` stays a pure atomic load.
- The public signatures of `TimeProvider::new`, `TimeProvider::with_wake`,
  `TimeProvider::set_format`, `RefreshConfig`, and `StatusBarRuntime::new` do
  not change.
- Any additions are limited to what the seam needs, and are crate-internal or
  test-only.

#### FR3: Synchronous manual-tick delivery

Delivering a manual tick through the seam lets the test wait until that
tick's version bump and `wake()` call have finished. The wait uses std
synchronization primitives and does not depend on how much real time has
passed. A test can then assert exact wake and version counts right after
delivering N ticks.

#### FR4: Seam-driven shutdown

When a TimeProvider is driven by the manual tick source, dropping it still
stops and joins the timer thread. This holds even when the thread is waiting
for a tick that never comes. No tick delivered after stop causes a version
bump or a `wake()` call.

#### FR5: Rewrite time_provider_timer_thread_calls_wake_on_interval

In time.rs, `time_provider_timer_thread_calls_wake_on_interval` drives ticks
through the manual tick seam and asserts that the wake count exactly equals
the number of ticks delivered. It no longer uses `std::thread::sleep` or a
fixed real-time window.

#### FR6: Rewrite time_provider_timer_thread_bumps_version_per_tick_without_get_value

In time.rs,
`time_provider_timer_thread_bumps_version_per_tick_without_get_value` drives
ticks through the manual tick seam, never calls `get_value`, and asserts that
the version advances by exactly the number of ticks delivered. It no longer
uses `std::thread::sleep` or a fixed real-time window.

#### FR7: Rewrite runtime_time_provider_timer_fires_wake

In runtime.rs, `runtime_time_provider_timer_fires_wake` checks that the
runtime passes its `WakeFn` into the TimeProvider timer. It reaches the
manual tick seam through a crate-internal or test-only way of building the
`StatusBarRuntime`, and asserts exact wake counts after delivering ticks,
with no real-time sleep window.

The test's comment currently claims the git-branch worker fires `wake` once
at startup. That claim is wrong: `clear_cache` in git_branch.rs:215-224 only
wakes when a branch or status was cached before. The comment is corrected.

#### FR8: Delayed-tick regression test

A new regression test holds ticks back through the manual tick seam, with no
real sleeping, to simulate a timer thread that is scheduled late. It then
delivers the ticks and uses the same shared wait-and-check helper as FR5 to
FR7, and must pass every time. If that check goes back to counting within a
fixed real-time window, the test must fail.

## 5. Non-Functional Requirements

### 5.1 Performance

Not applicable.

### 5.2 Security

Not applicable.

### 5.3 Availability

Not applicable.

### 5.4 Maintainability

- **NFR1 - No new crates:** No new crates are added, including
  dev-dependencies. std primitives are enough.
- **NFR2 - No dependence on real time in the timer tests:** The tests in FR5
  to FR8 contain no `std::thread::sleep` and no assertion tied to a fixed
  real-time window. Their result is the same whether they run in parallel,
  with `--test-threads=1`, or on a heavily loaded machine.
- **NFR4 - Seam hidden from the public API:** The seam is not part of the
  crate's public API. Its visibility is `pub(crate)` or `#[cfg(test)]`,
  whichever is enough.

### 5.5 Compatibility

- **NFR3 - Platform and feature-gate compatibility:** The seam builds and
  behaves the same on Linux and Windows. The status_bar module keeps its
  current `gui` feature gating, and `cargo check --no-default-features` still
  succeeds.

## 6. UI/UX Requirements

Not applicable.

## 7. Data Requirements

Not applicable.

## 8. External Integration

Not applicable.

## 9. Constraints

### 9.1 Technical Constraints

- No new crates, including dev-dependencies (NFR1).
- The seam is `pub(crate)` or `#[cfg(test)]` (NFR4).
- Linux and Windows builds, `gui` feature gating unchanged (NFR3).
- Public signatures listed in FR2 unchanged.

### 9.2 Business Constraints

Not applicable.

### 9.3 Schedule Constraints

Not applicable.

### 9.4 Declared Change Set

Feature-specific paths are not listed by hand here. They are derived at
create-plan from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):

- `feature-docs/status-bar-timer-test-flake/**`
- `test-docs/status-bar-timer-test-flake/**`

`feature-docs/status-bar-timer-test-flake/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and any design
artifacts the design step produces. Their producers are defined by each
phase document and by `references/phase-state.md` (cited only; rules are not
restated).

`test-docs/status-bar-timer-test-flake/**` covers `{T}.tests.yaml` (path
form: `test-docs/status-bar-timer-test-flake/{T}.tests.yaml`). Its producer
is defined by `implement-phase.md` (cited only; rules are not restated).

**Semantics**:

- Default members are part of the declaration unless the SPEC author
  explicitly removes them. Removal is a deliberate narrowing, never an
  omission by silence.
- The declaration is a superset assertion: the actual change set must be
  CONTAINED IN the declared set. A declared path that is never produced is
  not a violation. A feature that generates no implement tasks produces no
  `test-docs/status-bar-timer-test-flake/` directory, and the declared
  `test-docs/status-bar-timer-test-flake/**` entry is still correct.

## 10. Issues and Risks

Not applicable.

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] AC1: Repeated runs of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  show no failures from `runtime_time_provider_timer_fires_wake`,
  `time_provider_timer_thread_calls_wake_on_interval`,
  `time_provider_timer_thread_bumps_version_per_tick_without_get_value`, or
  the new regression test. (FR5, FR6, FR7, FR8, NFR2)
- [ ] AC2: None of the three rewritten tests or the regression test contains
  `std::thread::sleep` or an assertion tied to a fixed real-time window. Each
  asserts exact counts after delivering a known number of manual ticks.
  (FR3, FR5, FR6, FR7, NFR2)
- [ ] AC3: The regression test passes with the ticks held back and then
  delivered. When its check is temporarily changed to count within a fixed
  real-time window, it fails. This is confirmed at verification time and the
  temporary change is reverted. (FR8)
- [ ] AC4: The existing tests `time_provider_drop_joins_timer_thread`,
  `time_provider_version_is_pure_load`,
  `time_provider_without_timer_does_not_spawn_thread`,
  `provider_set_format_bumps_version`, and every other status_bar test pass
  without changes to their assertions. (FR2, FR4)
- [ ] AC5: The production path still waits with `Condvar::wait_timeout` for
  the configured interval, bumps the version before calling wake, and
  notifies all and joins on Drop. The public signatures listed in FR2 are
  unchanged, as the diff shows. (FR1, FR2, NFR4)
- [ ] AC6:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  and
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  both succeed, and no new crate appears in Cargo.toml or Cargo.lock.
  (NFR1, NFR3)
- [ ] AC7: The comment in `runtime_time_provider_timer_fires_wake` no longer
  says the git-branch worker fires an initial wake when `cwd_source` returns
  None. (FR7)

### 11.2 KPI

Not applicable.

## 12. Test Scenarios

### 12.1 Test Perspectives

- [ ] TS-1 (FR1, FR3, FR5): Build a TimeProvider with the manual tick source
  and a counting WakeFn. Deliver N ticks, waiting for each to finish. The
  wake count equals N exactly. Before any tick is delivered, it is 0.
- [ ] TS-2 (FR3, FR6): Build a TimeProvider with the manual tick source.
  Record v0 without calling get_value, deliver N ticks, and check that
  version equals v0 + N exactly.
- [ ] TS-3 (FR7): Build a StatusBarRuntime whose TimeProvider uses the manual
  tick source, with a counting WakeFn and no custom commands. Deliver N ticks
  and check that the runtime-supplied wake count equals N exactly.
- [ ] TS-4 (FR8): Delayed-tick regression: hold ticks back with no real sleep
  and check that no wake or version bump has happened. Then deliver the held
  ticks, check exact counts with the shared helper, and confirm the test
  passes deterministically. At verification time, temporarily change the
  check to count within a fixed real-time window and confirm the test fails.
- [ ] TS-5 (FR4): Drop a TimeProvider that uses the manual tick source while
  its thread waits for a tick that never comes. Drop returns, the thread is
  joined, and no wake or version bump happens afterwards.
- [ ] TS-6 (FR2): Regression of the existing production-path tests:
  time_provider_drop_joins_timer_thread (60 s interval, Drop within 1 s),
  time_provider_version_is_pure_load, and
  time_provider_without_timer_does_not_spawn_thread still pass.
- [ ] TS-7 (NFR2): Run the targeted tests repeatedly, both in parallel and
  with `--test-threads=1`. There are no failures.
- [ ] TS-8 (NFR3): `cargo check` succeeds with default features and with
  `--no-default-features`, on the Linux host.

## 13. Glossary

| Term | Definition |
|------|------------|
| Tick source | Where the TimeProvider timer loop gets its ticks from. In production, `Condvar::wait_timeout(guard, interval)` in `timer_loop`. |
| Manual tick source | A test-supplied tick source through which a test delivers ticks one at a time. |
| Seam | The crate-internal or test-only point that makes the tick source replaceable. |

## 14. Confirmed Items

### 14.1 Confirmed

- [x] a1: The timer tick source becomes injectable through an internal seam.
  Production TimeProvider behaviour does not change: the default interval
  stays 1000 ms, the version is bumped before wake, and Drop still notifies
  all, joins, and returns promptly.
- [x] a2: The two same-shaped time.rs tests are fixed with the same seam as
  the named runtime test.
- [x] a3: A dedicated regression test simulates delayed ticks through the
  manual tick seam, with no real sleep, and fails if the check goes back to a
  fixed real-time window.
- [x] a4: The stale comment about the git worker's initial wake in
  runtime_time_provider_timer_fires_wake is corrected.
- [x] a5: No new crates are added, including dev-dependencies.
- [x] a6: `time_provider_without_timer_does_not_spawn_thread` sleeps 40 ms
  but asserts a count of 0, which real time passing cannot break. It is out
  of scope and left unchanged.
- [x] Design step: skipped. Only test code and an internal Rust seam change;
  there is no UI or visual surface.

### 14.2 Open Items

None.

## 15. References

- SPEC: `feature-docs/status-bar-timer-test-flake/SPEC.md`
