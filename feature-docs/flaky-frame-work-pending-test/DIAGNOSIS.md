# Diagnosis: restart-flag order dependence in the `--lib` suite

## AC-1: contended state, owning file, party enumeration, mechanism, baseline

### Contended state

`RESTART_REQUIRED` (`static RESTART_REQUIRED: AtomicBool`), owned by
`src-tauri/src/self_exec.rs`. It is process-global by design: a self-spawn
failure can originate off the App thread (image-viewer spawn worker, mux
daemon connect, settings launcher), and the single App instance drains it
once per frame.

### Every party in the `--lib` binary that raises, clears, consumes, or
observes it

The hypothesis in `tasks/task0001.md` named 7 parties (1 raiser+observer, 4
clearers-as-precondition, 2 self_exec raiser/clearer-or-consumer pairs).
Enumerating from the source turned up **11** — the plan's 7 plus 4 more that
touch the flag indirectly through `App::frame_work_pending` /
`App::pump_toasts` without setting it as an explicit precondition:

| # | Test (full path) | Role | How it touches the flag |
|---|---|---|---|
| 1 | `app::tests::timing::next_toast_deadline_none_when_no_toast_active` | observer | `next_toast_deadline` → `frame_work_pending` → `restart_pending()` (peek); assertion requires the flag be **false** |
| 2 | `app::tests::timing::next_toast_deadline_some_when_restart_toast_active` | observer | same call chain; assertion is `.is_some()`, tolerant of either flag value |
| 3 | `app::tests::timing::next_toast_deadline_some_when_sftp_toast_active` | observer | same call chain; also tolerant |
| 4 | `app::tests::timing::pump_toasts_runs_both_pumps_unconditionally` | **consumer** | `pump_toasts` → `pump_restart_toast` → `self_exec::restart_required()` (**consuming** read) unconditionally; if the flag happens to read `true` here, `pump_restart_toast` re-arms `restart_toast` with a fresh deadline, which flips this test's own `!app.restart_toast.active()` assertion |
| 5 | `app::tests::timing::frame_work_pending_false_on_fresh_app` | clearer (precondition) + observer | set flag false, then peek |
| 6 | `app::tests::timing::frame_work_pending_true_when_progress_channel_nonempty_and_consumes_nothing` | clearer (precondition) + observer | same |
| 7 | `app::tests::timing::frame_work_pending_true_when_result_channel_nonempty_and_consumes_nothing` | clearer (precondition) + observer | same |
| 8 | `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing` | **raiser** + observer | sets flag true, then peeks twice — **the failing test** |
| 9 | `app::tests::timing::next_toast_deadline_some_when_pretoast_progress_event_pending` | clearer (precondition) + observer | same as #5–7 |
| 10 | `self_exec::tests::restart_pending_reports_true_across_consecutive_peeks` | raiser + observer + clearer | raises, peeks twice, explicitly clears |
| 11 | `self_exec::tests::restart_required_still_consumes_once_after_peeks` | raiser + observer + **consumer** | raises, peeks twice, then the consuming `restart_required()` twice |

**Indirect raise path, checked not assumed (FR1):** `note_spawn_failure()` is
also called from `settings_launcher.rs` (`ProcessSettingsLauncher::open`),
`mux/daemon/connect.rs` (daemon spawn failure), and `viewer/image.rs`
(`SpawnWorker::start`'s worker thread) / `viewer/mod.rs`
(`ProcessViewerSink::spawn_child`). None of these call sites is reachable
from a `#[test]` in the `--lib` binary:
- `settings_launcher.rs`'s tests exercise only `watch_child_stdout` /
  `is_saved_event`, never `ProcessSettingsLauncher::open`.
- `mux/daemon/connect.rs` has no `#[cfg(test)]` module at all.
- `viewer/mod.rs`'s tests drive `ViewerSpawner`/`CapturingSink` fakes, never
  `ProcessViewerSink::spawn_child`.
- `viewer/image.rs`'s tests exercise `ImageStore` / `ImageViewerRouter::route`
  directly; `router_new_derives_chrome_tokens_from_settings` asserts
  `router.worker.is_none()` — confirming the worker (and its
  `SpawnWorker::start` thread that could call `note_spawn_failure`) is never
  started in this binary.

So the indirect raise path is confirmed inert for `--lib`; only the 11 tests
above contend on the flag.

### Mechanism

`RESTART_REQUIRED` is one process-wide `AtomicBool`. Cargo's default `--lib`
invocation runs the ~3250 tests across multiple threads. Test #8
(`frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`)
does, with no exclusivity:

```
test_set_restart_required(true);      // raise
let app = App::new();                 // no other predicate term becomes true
assert!(app.frame_work_pending());    // reads restart_pending() — must observe true
```

`App::new()` starts with empty SFTP channels and no toast, so
`frame_work_pending()`'s other three OR terms are false by construction —
the assertion depends on `restart_pending()` alone. If any of #4, #5, #6,
#7, #9, #10, or #11 is scheduled on another thread and clears or consumes
the flag (`test_set_restart_required(false)`, or the consuming
`restart_required()` inside `pump_toasts`/directly) in the window between
test #8's raise and its `assert!`, the read observes `false` and the
assertion fails with exactly the panic reproduced below. The contention is
symmetric — a raise from #8, #10, or #11 landing inside #1's window (which
requires `false`) would equally break #1's assertion; #8 is simply the one
whose narrow window and single-term-dependent assertion made it the party
observed failing under real scheduling.

### Baseline failure (captured before any source change)

Command:
```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Failing test path: `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`

Assertion message (verbatim):
```
thread 'app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing' (350448) panicked at src-tauri/src/app/tests/timing.rs:135:5:
assertion failed: app.frame_work_pending()
```

Summary line:
```
test result: FAILED. 3247 passed; 1 failed; 3 ignored; 0 measured; 0 filtered out; finished in 14.55s
```

## AC-2: fix side and rationale

**Chosen: test-side isolation** (mutual exclusion covering every contending
party), not a production-side revision.

### Why not production-side

`RESTART_REQUIRED`'s process-global shape is load-bearing, not incidental:
`note_spawn_failure()` must be callable from any thread (the image-viewer
spawn worker thread, the mux daemon connect path, the settings launcher),
and the single App instance drains it on the App thread once per frame —
exactly the off-thread signaling the module's own doc describes. D1 permits
a production-side revision only if that path keeps working and no lock is
added to the per-frame predicate path. In production there is exactly one
App instance, so "making the state non-process-global" would mean either
(a) giving `App` its own flag and somehow still routing every off-thread
`note_spawn_failure()` call to that one instance — which reconstructs a
single shared instance under a different name, not a real decomposition —
or (b) accepting a lock on the per-frame read, which D1 forbids outright.
Neither is a genuine fix; both are over-engineering a problem that is
actually confined to the test binary, which is exactly the failure mode
D1's rationale warns against.

### Why test-side isolation is sound here

The defect is 11 independent unit tests sharing one process-global flag with
no exclusivity, under a suite that runs multi-threaded by default. A single
seam that every touching test acquires for the whole span between its first
touch and its last observation removes the race without touching production
code, production behavior, or the consuming/peeking distinction.

### The seam

`self_exec::RestartFlagTestGuard` (`#[cfg(test)]`), backed by a
`std::sync::Mutex<()>`:
- `acquire()` locks the mutex (recovering from poison via
  `unwrap_or_else(PoisonError::into_inner)`) and resets the flag to `false`
  before returning the guard — a span never inherits a value it did not
  itself establish.
- `set(value)` raises or clears the flag within the held span.
- `Drop` resets the flag to `false` unconditionally, including when the
  span ends via a panic (Rust still runs `Drop` during unwind) — a span
  never leaves a value behind for the next one.

All 11 parties listed under AC-1 now acquire this guard for their full
touching span (`src-tauri/src/self_exec.rs`,
`src-tauri/src/app/tests/timing.rs`).

### Rejected resolutions

1. **`-- --test-threads=1` for `--lib`** (workflow.yaml / CI / `test/README.md`
   / the documented command) — forbidden outright by D3(a)/FR6; would hide
   the race project-wide rather than remove it, and violates an explicit
   requirement.
2. **`#[ignore]` / delete / gate the target test out of the default run** —
   forbidden by D3(b)/FR5.
3. **Remove / invert / relax / conditionalize / retry-wrap the target
   assertion** — forbidden by D3(c)/FR4. TS-05 (below) also demonstrates the
   assertion has real discriminating power, so weakening it would hide a
   genuine race instead of fixing it.
4. **Reorder or rename tests to make the interleaving less likely** —
   forbidden by D3(d); probability shaping, not isolation, and would still
   be flaky under a different core count or `--test-threads=N`.
5. **Production-side revision** (non-process-global state) — rejected per
   the "Why not production-side" reasoning above.
6. **A blanket per-test reset hook** (e.g. a `#[ctor]`-style "clear the flag
   before every test" attribute, or pulling in a `serial_test`-style crate) —
   rejected: NFR5 forbids a new test framework crate, and a bare reset
   between tests does not establish exclusivity for the read-then-assert
   window itself — a concurrent party could still clear the flag between
   the reset and the assertion. Only holding a lock for the whole span
   closes that window.

## AC-3 / AC-4: stability evidence

Three consecutive default-parallelism runs, no `--test-threads` override
(command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
src-tauri/Cargo.toml --lib`), all after the fix landed:

```
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 15.99s
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 16.52s
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 15.14s
```

Serial run (command: `CARGO_TARGET_DIR=src-tauri/target cargo test
--manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`):

```
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 54.99s
```

(Two additional consecutive-run triads were also captured earlier in the same
session, immediately after the fix landed and again after the TS-05 revert
below — all with the identical `3250 passed; 0 failed` summary line; the
triad quoted above is the final one, captured after the AC-6
red-confirmation scratch breaks described below were reverted.)

`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
appears in the executed test list of the default-parallelism run (confirmed
via `... ok` in the run output) — not `#[ignore]`d, not deleted, not gated
out.

## AC-3 / AC-4: stability evidence re-captured at the verify phase

The triad above was captured at the implementation commit `41793c8a`. Two
review auto-fix commits landed afterwards — `37bc9811` (added the
non-resetting `RestartFlagTestGuard::acquire_preserving_flag()` and moved the
two guard regression tests' observations inside the exclusive span) and
`a8d8168b` (re-added a plain `acquire()` re-acquisition so the panic-span
test keeps exercising `acquire()`'s poison-recovery path). The verify phase
therefore re-ran the same approved commands against the final source state
(`a8d8168b`):

Three consecutive default-parallelism runs (command:
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
src-tauri/Cargo.toml --lib`):

```
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 16.01s
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 67.00s
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 15.66s
```

Serial run (command: `CARGO_TARGET_DIR=src-tauri/target cargo test
--manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`):

```
test result: ok. 3250 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 58.87s
```

Build gates on the same source state:
`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
src-tauri/Cargo.toml` and the same command with `--no-default-features` both
finished with exit code 0.

`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
reported `... ok` in every one of the four runs above.

## AC-5: assertion strength (TS-05)

The target assertion was left unmodified in shape (still asserts
`app.frame_work_pending()` under "restart state raised, nothing consumed").
To confirm it retains discriminating power, its precondition was
deliberately broken (`guard.set(true)` temporarily changed to
`guard.set(false)` in `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`)
and the approved default-parallelism command was run:

```
thread 'app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing' (452456) panicked at src-tauri/src/app/tests/timing.rs:149:5:
assertion failed: app.frame_work_pending()

test result: FAILED. 3249 passed; 1 failed; 3 ignored; 0 measured; 0 filtered out; finished in 14.99s
```

The break was reverted immediately after (`guard.set(true)` restored), and
the suite was re-confirmed green over three fresh consecutive runs (see
AC-3/AC-4 above, which are the post-revert numbers). The break did not
survive into the final change set.

## AC-6: regression guard

Two deterministic tests in `self_exec.rs`'s own test module exercise the
seam's stated contract directly — no sleep, no wall-clock threshold, no
thread-interleaving assumption:

- `self_exec::tests::restart_flag_test_guard_clears_the_flag_when_the_span_ends` —
  asserts the flag reads `false` once a guard has dropped, regardless of
  what the span left it at (the seam's postcondition).
- `self_exec::tests::restart_flag_test_guard_stays_usable_after_a_panicking_span` —
  panics inside a held guard's span via `std::panic::catch_unwind`, then
  asserts a fresh `acquire()` still succeeds and the flag is not stuck
  raised (the seam's failure-isolation clause: a panic inside one span must
  not poison it for every later span).

**`restart_flag_test_guard_stays_usable_after_a_panicking_span` plays the
AC-6 regression-guard role**: it exercises acquire → mutate → panic →
poison-recover → reacquire → verify-clean end to end, and both tests
reference `RestartFlagTestGuard` directly — reverting the fix back to the
old bare `test_set_restart_required` setter removes that type entirely, so
these tests (and the crate) fail to build.

Both were also red-confirmed empirically (TDD discipline, not required by
AC-6 itself but recorded here as evidence the checks discriminate): the
`Drop` body was temporarily reduced to a no-op, and
`restart_flag_test_guard_clears_the_flag_when_the_span_ends` failed exactly
as expected —

```
thread 'self_exec::tests::restart_flag_test_guard_clears_the_flag_when_the_span_ends' panicked at src-tauri/src/self_exec.rs:357:9:
the flag must read clear once the exclusive span has ended
```

— then, separately, `acquire()`'s poison recovery was temporarily replaced
with a bare `.unwrap()`, and running the suite did not just fail the target
test but **cascaded into two unrelated tests failing alongside it** —
`restart_pending_reports_true_across_consecutive_peeks` and
`restart_required_still_consumes_once_after_peeks`, both aborting on the
same poisoned-lock panic:

```
thread 'self_exec::tests::restart_flag_test_guard_stays_usable_after_a_panicking_span' panicked at src-tauri/src/self_exec.rs:368:13:
simulated panic inside an exclusive span
thread 'self_exec::tests::restart_flag_test_guard_stays_usable_after_a_panicking_span' panicked at src-tauri/src/self_exec.rs:208:50:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
...
test result: FAILED. 3247 passed; 3 failed; 3 ignored; 0 measured; 0 filtered out; finished in 14.78s
```

This is a direct, empirical demonstration of the Risk Assessment row in
IMPLEMENTATION.md ("a serialization seam converts one failing test into a
cascade of failures in unrelated tests when a test panics inside its span")
— and of why the poison-recovery line in `acquire()` is load-bearing, not
decorative. Both scratch breaks were reverted immediately after observing
the failure, and the suite was re-confirmed green over three fresh
consecutive runs (the triad quoted above).

## AC-7: gates

- CLI-only check (`CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml --no-default-features`) succeeds
  after the final source state — the fix is entirely inside `#[cfg(test)]`
  code in `self_exec.rs`, so it is compiled out of the CLI-only build by
  construction.
- No `-- --test-threads=1` was introduced anywhere for the `--lib` suite
  (`workflow.yaml`, CI configuration, `test/README.md`, or the documented
  test command are all untouched by this task).
- Every new/modified test uses the existing inline `#[cfg(test)] mod tests`
  convention, `<subject>_<scenario>_<expected>` naming, per-test
  construction of the unit under test, and no new test framework crate.
- The change set is `src-tauri/src/self_exec.rs` and
  `src-tauri/src/app/tests/timing.rs` — both inside this task's declared
  file list.

## Out-of-scope flakes

`ASM-02` (`tabs.rs` replay non-determinism) and `ASM-03` (`tmux_sockets`
discovery) did not surface in any of the runs recorded above.
