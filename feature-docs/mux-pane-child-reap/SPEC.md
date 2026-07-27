# Feature: mux pane child process reaping

## Overview

The mux daemon spawns one shell process per pane on the slave side of a PTY,
but discards the `Box<dyn Child + Send + Sync>` handle that
`portable_pty::SlavePty::spawn_command` returns. Because neither
`std::process::Child` nor portable_pty's Unix implementation calls `wait()` on
drop, nothing ever reaps an exited shell and its process control block stays in
the kernel as a zombie (`<defunct>`). This feature retains the child handle from
spawn through to `MuxPane`, and reaps it — off the daemon's async runtime, with
a bounded grace period and a kill escalation — on every path that tears a pane
down.

## Objectives

- Retain the child handle returned by `spawn_command` instead of dropping it.
- Reap the shell process on every pane-teardown path (PTY EOF reap task,
  `DestroyPane`, `DestroyWindow`, `graceful_shutdown`).
- Never block the daemon's async runtime or a `SessionManager` lock holder on
  a child's exit.
- Bound the reap so a shell that ignores `SIGHUP` cannot wedge it forever.

## User Stories

### US1: A long-running daemon accumulates no zombies

As an eMterm mux user who keeps the daemon resident for days, I want exited
pane shells to be reaped, so that `ps` / `pgrep` / `pstree` show only the
panes that actually exist and the PID space is not consumed.

**Acceptance Criteria:**

- [ ] After opening and closing panes repeatedly, the daemon has no child
      process in state `Z`.
- [ ] Closing a pane does not measurably delay the daemon's handling of other
      panes.

### US2: Force-closing a wedged pane still terminates cleanly

As a user closing a pane whose shell ignores the hangup, I want the pane to go
away and the process to be reaped, so that a stuck shell does not leak.

**Acceptance Criteria:**

- [ ] A child that does not exit within the grace period is killed and then
      reaped.
- [ ] The teardown path returns without waiting for the child.

## Technical Requirements

### Functional Requirements

- **FR1:** `SpawnedPty` (`src-tauri/src/mux/ipc/pty_spawn.rs`) gains a `child`
  field holding the `Box<dyn portable_pty::Child + Send + Sync>` returned by
  `pair.slave.spawn_command(cmd)`. The value is no longer dropped at the spawn
  site.
- **FR2:** `MuxPane` (`src-tauri/src/mux/session/pane.rs`) holds the child
  handle as an `Option`, populated by `MuxPane::new` from `SpawnedPty.child`.
  Test constructors (`new_test`, `new_test_with_writer`) construct the pane
  with `None`.
- **FR3:** `MuxPane::mark_exited()` takes the child handle out of the pane
  (`Option::take`) and hands it to the reaper. Because the handle is taken, a
  second `mark_exited()` on the same pane finds `None` and starts no second
  reap.
- **FR4:** `mark_exited()` returns without waiting for the child to exit. The
  reap itself runs outside the calling thread, so a caller holding the
  `SessionManager` tokio mutex (`handle_destroy_pane`,
  `handle_destroy_window`, `graceful_shutdown`) is never blocked on process
  exit.
- **FR5:** The reap procedure polls `Child::try_wait()` at a fixed interval up
  to a bounded grace period. If the child is still alive when the grace period
  elapses, the reaper calls `ChildKiller::kill()` and then `Child::wait()`.
  The procedure has no unbounded wait.
- **FR6:** The reap procedure is a standalone function taking a
  `Box<dyn Child + Send + Sync>` (plus its timing parameters), so it can be
  unit-tested against a real child process without opening a PTY or building a
  `MuxPane`.
- **FR7:** A failing `kill()` (child already gone) does not abort the reap —
  the procedure proceeds to `wait()`. A failing `wait()` (e.g. `ECHILD`) is
  logged at `warn` and ends the procedure without panicking.
- **FR8:** Every pane-teardown path reaps: the PTY-EOF reap task
  (`run_pane_exit_task` → `handle_destroy_pane`), `handle_destroy_pane` itself,
  `handle_destroy_window`, and `graceful_shutdown`. Each already calls
  `mark_exited()`, so FR3 covers them; this requirement is the verification
  that no teardown path bypasses `mark_exited()`.
- **FR9:** A regression test opens and closes panes repeatedly and asserts that
  no child process remains in zombie state afterwards.

### Non-Functional Requirements

- **NFR1 - Performance:** `mark_exited()` completes in time independent of the
  child's exit behavior. Reaping one pane does not affect the I/O latency of
  other panes.
- **NFR2 - Security:** Only handles the daemon itself obtained from
  `spawn_command` are killed or waited on. No code path accepts a PID from
  outside the daemon and signals it.
- **NFR3 - Robustness:** A reap failure never crashes the daemon and never
  fails the pane-teardown operation that triggered it.
- **NFR4 - Observability:** Reap failures are logged at `log::warn!` or above
  so they survive in release builds (release persists `warn` and higher).
- **NFR5 - Portability:** The change builds on Linux and Windows and does not
  affect the `--no-default-features` (CLI-only) build. The child handle is
  retained on both platforms even though Windows Job Objects mean the zombie
  symptom itself is Unix-only.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────────┐
│ pty_spawn::spawn_pty                                     │
│   pair.slave.spawn_command(cmd) -> child                 │
│   SpawnedPty { master, writer, reader, child }           │
└───────────────────────┬──────────────────────────────────┘
                        │ register_pane_and_start_reader
                        ▼
┌──────────────────────────────────────────────────────────┐
│ MuxPane { .., master: Option<..>, child: Option<..> }    │
└───────────────────────┬──────────────────────────────────┘
                        │ mark_exited()  (takes child, returns immediately)
                        ▼
┌──────────────────────────────────────────────────────────┐
│ reaper (off the async runtime)                           │
│   try_wait() poll loop, bounded grace period             │
│     └─ still alive ─> kill() ─> wait()                   │
└──────────────────────────────────────────────────────────┘
```

### Data Flow

Pane teardown paths, all converging on `mark_exited()`:

```
PTY reader EOF ──> pane_exit_sender ──> run_pane_exit_task
                                             │
DestroyPane request ─────────────────────────┤
                                             ▼
                                    handle_destroy_pane
                                             │
DestroyWindow request ──> handle_destroy_window
                                             │
daemon shutdown ────────> graceful_shutdown  │
                                             ▼
                                      MuxPane::mark_exited()
                                             │ child.take()
                                             ▼
                                        reap procedure
```

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/mux/ipc/pty_spawn.rs`: `SpawnedPty`, `spawn_pty`,
  `register_pane_and_start_reader`.
- `src-tauri/src/mux/session/pane.rs`: `MuxPane` struct, `MuxPane::new`,
  `MuxPane::new_test`, `MuxPane::new_test_with_writer`, `mark_exited`.
- `src-tauri/src/mux/daemon.rs`: `run_pane_exit_task`, `graceful_shutdown`.
- `src-tauri/src/mux/ipc/handlers.rs`: `handle_destroy_pane`,
  `handle_destroy_window`.

**External Dependencies:**

- `portable-pty 0.8.1`: `Child` trait (`try_wait`, `wait`, `process_id`) and
  its supertrait `ChildKiller` (`kill`, `clone_killer`). `spawn_command`
  returns `Box<dyn Child + Send + Sync>`, so the handle can be moved across
  threads.

### File Structure

```
src-tauri/src/mux/
├── ipc/
│   ├── pty_spawn.rs      # FR1: SpawnedPty.child, spawn_pty retains handle
│   └── handlers.rs       # FR8: destroy paths verified to reach mark_exited
├── session/
│   └── pane.rs           # FR2/FR3/FR4: MuxPane.child, mark_exited hands off
└── daemon.rs             # FR8: reap task / graceful_shutdown paths
```

The reap procedure (FR5/FR6/FR7) is placed in the module that owns the pane
lifecycle; the planner chooses between a new small module and an addition to
`pane.rs` based on the surrounding code's shape.

## Test Scenarios

### Unit Tests

- [ ] `mark_exited()` on a pane with no child (test constructor) is a no-op and
      does not panic.
- [ ] `mark_exited()` called twice on the same pane starts at most one reap
      (the child handle is `None` on the second call).
- [ ] `mark_exited()` clears `writer` and `master` and sets `exited` (existing
      behavior preserved — `test_mark_exited_clears_writer_and_master`).
- [ ] The reap procedure reaps a child that exits promptly, without reaching
      the kill escalation.
- [ ] The reap procedure kills and reaps a child that does not exit within the
      grace period.
- [ ] The reap procedure returns (rather than panicking) when `wait()` fails.

### Integration Tests

- [ ] Spawning a pane via `spawn_pty` and tearing it down leaves no zombie:
      after teardown, the spawned PID is no longer in zombie state.
- [ ] FR9 regression: repeat pane open/close N times; afterwards no child of
      the test process is in state `Z`.

### E2E Tests

**Existing E2E tests**: None detected (no `e2e-tests/`, `tests/e2e/`,
`playwright.config.*`, `cypress.config.*`, or `docker-compose.e2e.yml` in this
repository).
**Run command**: Not detected.

### Edge Cases

- [ ] Child already reaped elsewhere → `wait()` returns `ECHILD`; logged at
      `warn`, no panic.
- [ ] Child exits during the poll loop between `try_wait()` calls → reaped on
      the next poll, no kill sent.
- [ ] Pane destroyed while the PTY reader thread is concurrently signalling
      exit → `Option::take` makes the second `mark_exited()` a no-op.
- [ ] Daemon process exits before a reaper finishes → the orphaned child is
      re-parented to init, which reaps it. No leak survives daemon exit.

### Performance Tests

- [ ] `mark_exited()` on a pane whose child is still running returns without
      waiting for the child's exit.

## Security Considerations

- **Authentication:** Not applicable — no external interface is added.
- **Authorization:** Not applicable.
- **Input Validation:** No externally supplied value reaches `kill()` or
  `wait()`. The only signalled processes are those the daemon itself spawned,
  identified by an owned handle rather than a numeric PID.
- **Data Protection:** No user data is read, stored, or logged by this feature.
  Log lines carry pane IDs and error kinds only.

## Error Handling

### Error Codes

No user-facing error codes. Internal failure handling:

| Condition | Handling |
|-----------|----------|
| `kill()` returns `Err` | Ignore; proceed to `wait()` |
| `wait()` returns `Err` | `log::warn!` with the pane ID and error; end the procedure |
| `try_wait()` returns `Err` | `log::warn!`; treat as "cannot determine" and escalate to kill + wait |

### Error Flow

```
reap error occurs → log at warn → return from the reap procedure
                                   (never propagated to the teardown caller)
```

## Performance Optimization

### Performance Goals

- `mark_exited()`: no wait on process exit; bounded by an `Option::take` and a
  handoff.
- Reap grace period: bounded, so no reaper thread lives indefinitely.

### Optimization Strategies

- Poll with `try_wait()` rather than blocking `wait()` during the grace period,
  so the kill escalation has a deterministic upper bound.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] `cargo test` passes on the default feature set
- [ ] `cargo check --no-default-features` passes (CLI-only build unaffected)
- [ ] Reap failures are visible at `warn` level in release logs
- [ ] No pane-teardown path bypasses the reap

## Assumptions

Recorded per batch-mode protocol — these were decided by the spec agent
without user confirmation because the run was unattended. The Codex
consultation loop was skipped: `codex` is not on this machine's PATH.

- **A1 (reap execution mechanism):** The reap runs off the async runtime. The
  concrete mechanism (a dedicated reaper thread fed by a channel, a per-exit
  detached `std::thread`, or `tokio::task::spawn_blocking`) is left to the
  planner, constrained only by FR4 (`mark_exited()` must not wait) and FR6
  (the procedure must be unit-testable in isolation). A per-exit detached
  thread is the simplest option that satisfies both and needs no new plumbing
  through pane creation; pane exits occur at human rates, so thread churn is
  not a concern.
- **A2 (grace period and poll interval):** Concrete values are left to the
  planner. The constraint is that the grace period is short enough that a
  wedged shell does not keep a thread alive for long, and long enough that a
  normally-exiting shell is reaped without a kill. A grace period on the order
  of a few hundred milliseconds with a poll interval an order of magnitude
  smaller satisfies both.
- **A3 (test-constructor child):** `MuxPane::new_test` / `new_test_with_writer`
  construct with `child: None` rather than gaining a child parameter, keeping
  the ~10 existing call sites unchanged.
- **A4 (zombie assertion in tests):** The FR9 regression test asserts on the
  spawned PIDs' process state (via `/proc/<pid>/stat` or an equivalent
  observation), gated to Unix (`#[cfg(all(test, unix))]`), and skips cleanly
  when a PTY cannot be opened in the test environment.
- **A5 (no Windows special-casing):** The child handle is retained and the reap
  is invoked on Windows too, with no `#[cfg(windows)]` divergence in the reap
  logic, per the task's stated scope.
- **A6 (design step):** This feature has no user-visible UI surface, so the
  `design` step is skipped.

## References

- Requirements document: `feature-docs/mux-pane-child-reap/REQUIREMENTS.md`
- Notion task: [https://www.notion.so/3a73509ec8ee8164a65de98cb7b217df](https://www.notion.so/3a73509ec8ee8164a65de98cb7b217df)
- `src-tauri/src/mux/ipc/pty_spawn.rs:79-127` — `spawn_pty`
- `src-tauri/src/mux/session/pane.rs:939-1275` — `MuxPane`, `mark_exited`,
  test constructors
- `src-tauri/src/mux/daemon.rs:1120-1151` — `run_pane_exit_task`,
  `graceful_shutdown`
- `src-tauri/src/mux/ipc/handlers.rs:169-375` — `handle_destroy_pane`,
  `handle_destroy_window`
- portable-pty 0.8.1 `src/lib.rs:126-159` — `Child` / `ChildKiller` traits
