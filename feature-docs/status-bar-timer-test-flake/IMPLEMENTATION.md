# Implementation Plan: status-bar-timer-test-flake

## Overview

Make the tick source of the status-bar `TimeProvider` timer loop replaceable
through a crate-internal / test-only seam, drive the three timer tests and a
new delayed-tick regression test with manually delivered ticks and exact
counts, and keep the production timer path unchanged.

Source locations: the TimeProvider lives in
`src-tauri/src/status_bar/providers/time.rs` (SPEC.md's "time.rs"); the
runtime lives in `src-tauri/src/status_bar/runtime.rs`.

## Technology Stack

- **Language**: Rust, crate `emterm` under `src-tauri/`. The `status_bar`
  module is compiled only with the `gui` feature; that gating does not change.
- **Synchronization**: Rust standard library only — mutex, condition
  variable, atomics, thread join.
- **New dependencies**: none (NFR1). There is no new dependency license to
  record.

## Layer Structure

| Layer | Location | Responsibility | Visibility |
|-------|----------|----------------|------------|
| Timer loop | providers/time.rs | One loop body shared by every tick source: obtain the next tick outcome → stop check → version bump → wake → completion notice to the source | module-private (unchanged) |
| Tick source contract | providers/time.rs | The three operations in Shared Components | module-private or crate-visible |
| Production tick source | providers/time.rs | The existing timed wait on the provider's own (mutex, condition variable) pair for `RefreshConfig.interval` | module-private |
| Manual tick source + controller | providers/time.rs | Test-driven ticks with a completion handshake | test-only, crate-visible |
| Shared wait-and-check helper | providers/time.rs | The only place the FR5–FR8 tests wait for timer progress | test-only, crate-visible |
| Runtime construction | runtime.rs | `StatusBarRuntime::new` delegates to an internal constructor that takes the time tick-source choice; a test-only builder passes the manual source | internal constructor private; builder test-only |

Dependency direction: runtime.rs test code may use the test-only items of
providers/time.rs. Production code in runtime.rs never refers to manual-source
items. providers/time.rs never refers to runtime.rs.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Tick source contract | Decouples "where the next tick comes from" from the loop body | **next tick** — pre: called only by the timer thread. Post: returns either *Tick* or *Stop*; returns *Stop* whenever the provider's stop flag is set at decision time, even if ticks are available. **tick done** — called by the loop after `wake()` returns for the tick just obtained. **interrupt** — called by `Drop` after the stop flag is set; post: a blocked *next tick* re-evaluates the stop flag and returns *Stop* promptly | task0001 |
| Production tick source | Keeps today's timing behaviour | *next tick* = exactly one timed wait on the provider's existing pair for `RefreshConfig.interval`; returns *Tick* whenever that wait returns (timeout, notification or spurious wake-up) and stop is not set — identical to the current loop. *tick done* has no effect. *interrupt* adds nothing beyond `Drop`'s existing notify-all on the same pair | task0001 |
| Manual tick source / controller (test-only) | Lets a test deliver ticks and wait for their completion | Created as a pair sharing one mutex-protected state and one condition variable. **Source side**: *next tick* blocks without any timeout until stop is set (→ *Stop*, takes priority) or a released tick is available (→ consume one, *Tick*); it is marked *parked* while blocked with nothing available; when the source side is dropped (timer-thread exit, including unwinding) the state is marked *closed* and all controller waiters are woken. *tick done* records one completed tick under the shared mutex. *interrupt* takes the shared mutex before notifying, and *next tick* re-checks stop under that mutex before blocking (no lost wake-up). **Controller side**: *hold(k)* — stages k ticks the timer thread cannot see; no effect on counts. *release and wait* — makes every held tick available, then blocks without any timeout until each released tick has been recorded as completed or the state is closed; returns the number completed. *wait parked* — blocks until the timer thread is parked or the state is closed. *closed* — reports whether the timer-thread side is gone. Ordering: because completion is recorded under the shared mutex after `wake()` returns, once *release and wait* returns the caller observes the version bump and every effect of `wake()` for each completed tick | task0001 |
| Shared wait-and-check helper (test-only) | Single wait-and-assert point for FR5–FR8 | Inputs: a controller, a reader for the wake count, a reader for the provider version, the expected wake baseline, the expected version baseline, and the expected tick count n. Behaviour: *release and wait* on the held ticks; assert the completed count equals n; assert wake count equals wake baseline + n and version equals version baseline + n, all exactly. Contains no sleep, no deadline and no timeout | task0001 |
| TimeProvider manual constructor (test-only) | Builds a provider driven by the manual source | Takes the format, a `WakeFn` and the manual tick source; spawns the same timer loop that `TimeProvider::with_wake` spawns. `TimeProvider::new` still spawns no thread; `TimeProvider::with_wake` keeps its signature and uses the production source | task0001 |
| Runtime internal constructor + test-only builder | Proves the production `WakeFn` wiring while swapping only the time tick source | `StatusBarRuntime::new` keeps its public signature and delegates to the internal constructor with the production source configured from `refresh_rates["time"]` (default 1000 ms). The test-only builder calls the same internal constructor with a manual source; everything else (the `WakeFn` clone handed to the TimeProvider, other providers, dispatcher) runs through the same code | task0001 |

## Conventions

- **Visibility (NFR4)**: nothing added is `pub`. Seam parts that production
  code reaches stay private to providers/time.rs / runtime.rs or are
  `pub(crate)`. Parts that only tests use are `cfg(test)`. Non-test builds
  gain no dead-code warnings from the seam.
- **No real time in the FR5–FR8 tests (NFR2)**: no thread sleep, no
  deadline, and no timeout in the handshake. A handshake defect shows up as
  a hanging test; when running tests, bound the run with the Bash tool's
  `timeout` parameter rather than adding timeouts to the code.
- **Test names**: the three rewritten tests keep their current names. New
  tests go in the providers/time.rs test module and their names start with
  `time_provider_`.
- **Existing tests**: `time_provider_drop_joins_timer_thread`,
  `time_provider_version_is_pure_load`,
  `time_provider_without_timer_does_not_spawn_thread`,
  `provider_set_format_bumps_version`, and every other status_bar test keep
  their assertions unchanged.
- **Comments**: English, factual, in the existing file style.

## Cross-task Design Decisions

### D1: One task

The seam and the shared helper (providers/time.rs) and the runtime test
builder (runtime.rs) depend on each other at compile time, and the whole
change spans two files. Splitting them into parallel tasks would force a
placeholder plus an integration-wiring owner for no gain. Affected: task0001.

### D2: Only "obtain the next tick" is swapped

The loop body — stop check, `version` bump before `wake()`, then the
completion notice — exists once and runs for both sources, so the tests
exercise the production loop body. The production source reproduces today's
wait exactly, including counting an early or spurious return as a tick when
stop is not set (FR2). Affected: task0001.

### D3: Drop sequence

`Drop` keeps its existing steps — set stop, notify-all on the existing pair,
join — and adds one step before the join: *interrupt* the tick source. For
the production source this adds no behaviour. For the manual source it is
what lets a thread parked on a tick that never comes exit (FR4). Affected:
task0001.

### D4: Held ticks and the regression property (FR8)

The regression test waits until the timer thread is parked, holds N ticks,
checks that wake and version are unchanged, and only then calls the shared
helper. Timer progress then depends only on the helper's *release and wait*
handshake. If the helper's check goes back to counting inside a fixed
real-time window, meaning its handshake is replaced by a sleep with the
counts checked afterwards, the held ticks are never released, the counts stay
at their baselines, and the exact-count assertion fails whatever the window
length. Affected: task0001.

### D5: Exact counts at the runtime level (FR7)

The runtime `WakeFn` is shared by every provider. The FR7 test isolates the
timer's wakes by using a cwd source that returns none, no custom commands,
and a long `git_branch` interval. Per SPEC.md, the git worker's cache clear
wakes only when a branch or status was cached before, so it adds no wake in
this setup. The test asserts exact equality and never falls back to a lower
bound. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Lost wake-up or deadlock in the manual handshake makes a test hang | Medium | High | Lock discipline pinned in Shared Components (stop checked under the shared mutex, stop takes priority, *closed* on source drop including unwinding); run tests with the Bash tool `timeout` parameter |
| Production behaviour drifts during the refactor (spurious-wake handling, bump/wake order, Drop latency) | Low | High | D2 keeps one loop body; existing production-path tests keep their assertions; diff inspection (VERIFICATION.md AC5); optional manual clock check |
| Another provider fires the runtime `WakeFn` during the FR7 test, so exact counts become flaky | Low | High | D5 setup; the implementer confirms by reading the git-branch and cwd providers that none of them wakes in that setup; if one does, report a plan deviation instead of loosening the assertion |
| Seam parts cause dead-code warnings in non-test builds | Medium | Low | Parts only tests use are `cfg(test)`; both `cargo check` runs are checked for new warnings |
| After the rewrite no automated test observes the production source actually ticking (both interval tests move to the manual source) | Medium | Medium | Diff inspection of the production source (AC5) plus a manual clock check in VERIFICATION.md; listed as an open question |

## Open Questions

- [ ] Production-path tick liveness has no automated test after the rewrite
  (covered by diff inspection and a manual check). SPEC.md does not require
  one, so none is planned.
