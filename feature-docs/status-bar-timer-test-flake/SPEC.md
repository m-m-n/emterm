# Feature: status-bar-timer-test-flake

## Overview

The status-bar TimeProvider timer tests count wakes and version bumps inside
a fixed real-time window, so repeated `--lib` test runs fail on them. This
feature makes the timer tick source replaceable through a crate-internal or
test-only seam, rewrites the affected tests to drive ticks manually and
assert exact counts, and adds a delayed-tick regression test. Production
timer behaviour does not change.

Requirements: `feature-docs/status-bar-timer-test-flake/REQUIREMENTS.md`.

## Objectives

- Make the pass/fail result of the status-bar TimeProvider timer tests
  independent of real time passing, so repeated runs of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  stop failing on these tests.
- Add a regression test that catches the flake if it comes back.

## User Stories

Not applicable. Acceptance criteria are listed under Success Criteria.

## Technical Requirements

### Functional Requirements

- **FR1:** Injectable tick source for the TimeProvider timer loop. The tick
  source of the timer loop (currently `Condvar::wait_timeout(guard, interval)`
  in `timer_loop`, time.rs:158) becomes replaceable through an internal seam
  visible only inside the crate or to tests. Tests supply a manual tick
  source and deliver ticks one at a time.
- **FR2:** Production timer behaviour unchanged. The production path
  (`TimeProvider::with_wake` and `StatusBarRuntime::new`) keeps using
  `Condvar::wait_timeout` with the configured `RefreshConfig.interval`
  (default 1000 ms, or `refresh_rates["time"]`). Each tick still does
  `version.fetch_add(1)` before calling `wake()`. `Drop` still sets `stop`,
  calls `notify_all`, and joins the timer thread promptly.
  `TimeProvider::new` still spawns no thread. `version()` stays a pure atomic
  load. The public signatures of `TimeProvider::new`,
  `TimeProvider::with_wake`, `TimeProvider::set_format`, `RefreshConfig`, and
  `StatusBarRuntime::new` do not change. Any additions are limited to what
  the seam needs, and are crate-internal or test-only.
- **FR3:** Synchronous manual-tick delivery. Delivering a manual tick through
  the seam lets the test wait until that tick's version bump and `wake()`
  call have finished. The wait uses std synchronization primitives and does
  not depend on how much real time has passed. A test can assert exact wake
  and version counts right after delivering N ticks.
- **FR4:** Seam-driven shutdown. Dropping a TimeProvider driven by the manual
  tick source still stops and joins the timer thread, even when the thread is
  waiting for a tick that never comes. No tick delivered after stop causes a
  version bump or a `wake()` call.
- **FR5:** Rewrite `time_provider_timer_thread_calls_wake_on_interval`
  (time.rs). It drives ticks through the manual tick seam and asserts that
  the wake count exactly equals the number of ticks delivered. It no longer
  uses `std::thread::sleep` or a fixed real-time window.
- **FR6:** Rewrite
  `time_provider_timer_thread_bumps_version_per_tick_without_get_value`
  (time.rs). It drives ticks through the manual tick seam, never calls
  `get_value`, and asserts that the version advances by exactly the number of
  ticks delivered. It no longer uses `std::thread::sleep` or a fixed
  real-time window.
- **FR7:** Rewrite `runtime_time_provider_timer_fires_wake` (runtime.rs). It
  checks that the runtime passes its `WakeFn` into the TimeProvider timer,
  reaches the manual tick seam through a crate-internal or test-only way of
  building the `StatusBarRuntime`, and asserts exact wake counts after
  delivering ticks, with no real-time sleep window. The test comment claiming
  the git-branch worker fires `wake` once at startup is corrected:
  `clear_cache` in git_branch.rs:215-224 only wakes when a branch or status
  was cached before.
- **FR8:** Delayed-tick regression test. A new test holds ticks back through
  the manual tick seam, with no real sleeping, to simulate a timer thread
  that is scheduled late. It then delivers the ticks and uses the same shared
  wait-and-check helper as FR5 to FR7, and must pass every time. If that
  check goes back to counting within a fixed real-time window, the test must
  fail.

### Non-Functional Requirements

- **NFR1 - No new crates:** No new crates are added, including
  dev-dependencies. std primitives are enough.
- **NFR2 - No dependence on real time in the timer tests:** The tests in FR5
  to FR8 contain no `std::thread::sleep` and no assertion tied to a fixed
  real-time window. Their result is the same whether they run in parallel,
  with `--test-threads=1`, or on a heavily loaded machine.
- **NFR3 - Platform and feature-gate compatibility:** The seam builds and
  behaves the same on Linux and Windows. The status_bar module keeps its
  current `gui` feature gating, and `cargo check --no-default-features` still
  succeeds.
- **NFR4 - Seam hidden from the public API:** The seam is not part of the
  crate's public API. Its visibility is `pub(crate)` or `#[cfg(test)]`,
  whichever is enough.

## Implementation Approach

### Architecture

**Component Diagram:**
```
StatusBarRuntime::new ──(WakeFn)──> TimeProvider::with_wake
                                          │
                                          ▼
                                   timer thread: timer_loop
                                          │
                     ┌────────────────────┴────────────────────┐
                     ▼                                         ▼
   production tick source                        manual tick source (seam,
   Condvar::wait_timeout(guard, interval)        pub(crate) / #[cfg(test)])
   interval = RefreshConfig.interval             test delivers ticks one at a
                                                 time and waits for completion
```

### Data Flow

```
tick → version.fetch_add(1) → wake()
Drop → set stop → notify_all → join timer thread
```

### API Design

Not applicable. Public signatures listed in FR2 do not change.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- status_bar module, time.rs: `TimeProvider`, `timer_loop`, `RefreshConfig`
  (FR1 to FR6, FR8).
- status_bar module, runtime.rs: `StatusBarRuntime::new`,
  `runtime_time_provider_timer_fires_wake` (FR7).
- status_bar module, git_branch.rs: `clear_cache` (referenced by the FR7
  comment correction only).

**External Dependencies:**
- None added (NFR1).

### File Structure

Not applicable. Feature-specific paths are derived at create-plan (see
Declared Change Set).

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/status-bar-timer-test-flake/**`
- `test-docs/status-bar-timer-test-flake/**`

`feature-docs/status-bar-timer-test-flake/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/status-bar-timer-test-flake/**` covers
`test-docs/status-bar-timer-test-flake/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/status-bar-timer-test-flake/` directory at all; the declared
`test-docs/status-bar-timer-test-flake/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (FR1, FR3, FR5): Build a TimeProvider with the manual tick source
  and a counting WakeFn. Deliver N ticks, waiting for each to finish. The
  wake count equals N exactly. Before any tick is delivered, it is 0.
- [ ] TS-2 (FR3, FR6): Build a TimeProvider with the manual tick source.
  Record v0 without calling get_value, deliver N ticks, and check that
  version equals v0 + N exactly.
- [ ] TS-4 (FR8): Delayed-tick regression. Hold ticks back with no real
  sleep and check that no wake or version bump has happened. Then deliver
  the held ticks, check exact counts with the shared helper, and confirm the
  test passes deterministically. At verification time, temporarily change
  the check to count within a fixed real-time window and confirm the test
  fails.
- [ ] TS-5 (FR4): Drop a TimeProvider that uses the manual tick source while
  its thread waits for a tick that never comes. Drop returns, the thread is
  joined, and no wake or version bump happens afterwards.
- [ ] TS-6 (FR2): Existing production-path tests still pass:
  time_provider_drop_joins_timer_thread (60 s interval, Drop within 1 s),
  time_provider_version_is_pure_load, and
  time_provider_without_timer_does_not_spawn_thread.

### Integration Tests
- [ ] TS-3 (FR7): Build a StatusBarRuntime whose TimeProvider uses the
  manual tick source, with a counting WakeFn and no custom commands. Deliver
  N ticks and check that the runtime-supplied wake count equals N exactly.
- [ ] TS-7 (NFR2): Run the targeted tests repeatedly, both in parallel and
  with `--test-threads=1`. There are no failures.
- [ ] TS-8 (NFR3): `cargo check` succeeds with default features and with
  `--no-default-features`, on the Linux host.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- Not applicable.

### Edge Cases
- [ ] Drop while the timer thread waits for a manual tick that never comes
  (TS-5, FR4).
- [ ] Ticks held back and delivered late (TS-4, FR8).

### Performance Tests
Not applicable.

## Security Considerations

Not applicable.

## Error Handling

Not applicable.

## Performance Optimization

Not applicable.

## Success Criteria

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

## Assumptions

- a1: The timer tick source becomes injectable through an internal seam.
  Production TimeProvider behaviour does not change: the default interval
  stays 1000 ms, the version is bumped before wake, and Drop still notifies
  all, joins, and returns promptly.
- a2: The two same-shaped time.rs tests are fixed with the same seam as the
  named runtime test.
- a3: A dedicated regression test simulates delayed ticks through the manual
  tick seam, with no real sleep, and fails if the check goes back to a fixed
  real-time window.
- a4: The stale comment about the git worker's initial wake in
  runtime_time_provider_timer_fires_wake is corrected.
- a5: No new crates are added, including dev-dependencies.
- a6: `time_provider_without_timer_does_not_spawn_thread` sleeps 40 ms but
  asserts a count of 0, which real time passing cannot break. It is out of
  scope and left unchanged.

## Open Questions

None.

## Implementation Phases

Not applicable.

## References

- Requirements: `feature-docs/status-bar-timer-test-flake/REQUIREMENTS.md`
