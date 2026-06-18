# Implementation Plan: Reap Detached Pane Exits in the Mux Daemon

## Overview

Make "PTY death → pane reap" a single authority in the mux daemon that is
independent of attach state, by routing pane-exit events from the per-pane reader
threads to a daemon-level reap task over a dedicated channel.

## Objectives

- Reap a pane whenever its PTY reaches EOF, whether the session is attached or detached.
- Restore the "all sessions empty → daemon auto-shutdown" invariant for the detached case.
- Cover the connection-reset race that otherwise strands an exited pane as detached.
- Preserve the existing attached client-teardown behavior and the existing explicit-kill / shutdown paths.

## Prerequisites

### Development Environment

- Rust toolchain as already used by the project (`src-tauri/` crate).

### Dependencies

- No new external dependencies. Uses the async runtime channels and shutdown
  signal mechanism already present in the daemon.
- Internal components that must exist (all present): the per-pane reader loop, the
  session manager, the pane-destroy reap function, and the daemon shutdown signal.

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Component**: `src-tauri` mux daemon (`src-tauri/src/mux/`)
- **Key mechanisms**: per-pane blocking reader thread, async daemon tasks, an
  inter-thread notification channel, and a broadcast-style shutdown signal.

### Design Approach

The blocking reader thread cannot reap a pane itself: it holds the pane output
target, scrollback, and shadow parser, but not the session manager or the
shutdown signal, which are owned on the async daemon side. A dedicated pane-exit
notification channel bridges the reader thread to a daemon task that performs the
reap through the existing pane-destroy function. Because reap is keyed on the
pane identifier and ignores the pane's output target, it covers the detached path
and the connection-reset race uniformly, and is safe to run redundantly with the
existing attached empty-chunk reap (idempotent).

**Sender lifetime (do not swap on attach/detach).** Unlike the pane output target
— which is swapped between attached and detached states over the pane's lifetime —
the shared pane-exit sender is fixed at pane creation and is never swapped. It
follows the existing shared-notification-sender lifetime exactly. This is
precisely why a detached pane can still notify the daemon on EOF: its sender does
not depend on, and is not torn down by, any client connection.

### Component Interaction

```
reader thread (per pane) --pane id on EOF--> pane-exit channel --> daemon reap task --> pane-destroy reap --> shutdown signal (when all sessions empty)
attached client <--empty exit chunk (attached only)-- reader thread
```

## Implementation Phases

### Phase 1: Daemon reap authority

**Goal**: A daemon-level task that reaps a pane from the session manager on
receipt of a pane identifier, independent of attach state, and fires the shutdown
signal when all sessions become empty. Testable in isolation.

**Files to Modify**:
- `src-tauri/src/mux/session/pane.rs` - add a shared, optional pane-exit sender
  type alias, following the shape of the existing shared-notification sender so it
  can be cloned into reader threads and left absent in CLI/test paths.
- `src-tauri/src/mux/ipc/handlers.rs` - widen the visibility of the pane-destroy
  reap function so the daemon module can call it (or provide a thin daemon-facing
  wrapper).
- `src-tauri/src/mux/daemon.rs` - add a daemon-level receiver task that consumes
  pane identifiers and reaps each via the pane-destroy reap function.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Pane-exit sender type | Convey a pane's death from a blocking reader thread to the async daemon | A pane identifier exists | A pane identifier can be enqueued for reap |
| Pane-destroy reap function (existing) | Remove a pane, prune empty window/session, fire shutdown when all sessions empty | A pane identifier | Pane removed if present; safe no-op if already gone |
| Daemon reap task | Drive reap for each received pane identifier | Receiver and session manager + shutdown signal available | Each received pane is reaped exactly as the attached path would |

**Processing Flow** (diagram-convertible):
1. Daemon reap task receives a pane identifier.
2. It invokes the pane-destroy reap function with the session manager and shutdown signal.
   - Pane present → remove it; prune empty window/session.
     - All sessions empty → fire shutdown signal.
     - Some session non-empty → no shutdown.
   - Pane already removed → safe no-op (warn-level log).

The pane-destroy reap function returns early when the pane is not found, **before**
the "all sessions empty" check. Therefore when the attached empty-chunk path and
the daemon reap path both run for the same pane, the second (losing) call neither
double-removes the pane nor re-fires the shutdown signal — it is a pure no-op (L1,
reinforces FR4 idempotency).

**Implementation Steps** (high level):
1. **Pane-exit sender type** - introduce the shared optional sender alias next to the existing shared-notification sender alias.
2. **Expose reap function** - make the pane-destroy reap function reachable from the daemon module.
3. **Daemon reap task** - add a task that loops over received pane identifiers and reaps each; exits when all senders are dropped (daemon shutdown), mirroring the existing notification-relay task.

**Dependencies**: Blocks Phase 2 (Phase 2 instantiates and wires this task and channel).

**Testing Approach**:
- Integration: detached last-pane reap fires the shutdown signal; non-last pane reap leaves the daemon alive.
- Integration: a pane switched to the detached (network-detach) state is still reaped.
- Unit: reaping the same pane twice is a safe no-op.

**Acceptance Criteria**:
- [ ] A pane identifier fed to the reap task removes the pane regardless of its output target.
- [ ] The shutdown signal is observed only when all sessions become empty.
- [ ] Double reap of the same pane does not panic.

**Estimated Effort**: small-medium

---

### Phase 2: Reader EOF notification + run-loop wiring

**Goal**: On reader EOF, notify the daemon of the pane exit regardless of attach
state while preserving the attached client-teardown; create the channel, spawn
the reap task, and thread the shared sender down the pane-creation path on both
daemon run loops.

**Files to Modify**:
- `src-tauri/src/mux/ipc/pty_spawn.rs` - plumb the shared pane-exit sender into
  the reader loop; in the EOF arm, keep the attached empty-chunk teardown and,
  unconditionally, enqueue the pane identifier on the pane-exit channel. The
  enqueue must not block the reader thread: mirror the existing
  notification-sender send semantics (non-blocking enqueue; if the sender is
  absent or the receiver is gone, ignore — the daemon is shutting down). The
  steady-state output path is left untouched.
- `src-tauri/src/mux/daemon.rs` - in both run loops (Unix socket and Windows
  named pipe), create the pane-exit channel, spawn the daemon reap task, and build
  the shared sender to hand down the pane-creation path.
- `src-tauri/src/mux/ipc/connection.rs` (and the pane-creation handlers it calls)
  - thread the shared pane-exit sender through to the reader spawn, mirroring how
    the notification sender is already threaded.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Reader EOF handler | On EOF, signal exit to the attached client (attached only) and notify the daemon (always) | Reader observed EOF; shared sender may be present or absent | Daemon notified of the pane exit when a sender is present; attached client torn down when attached |
| Run-loop wiring | Create the channel, spawn the reap task, and propagate the shared sender to reader threads | Daemon run loop is starting | Reader threads created thereafter can notify the daemon of exits |

**Processing Flow** (diagram-convertible):
1. Reader loop observes EOF.
2. If the output target is attached → send the empty exit chunk to the client (teardown).
3. Regardless of attach state → enqueue the pane identifier on the pane-exit channel.
   - Send succeeds → daemon reap task will reap it.
   - Send fails (receiver dropped) → ignore (daemon already shutting down).

**Implementation Steps** (high level):
1. **Reader plumbing** - add the shared pane-exit sender as an input to the reader spawn and loop.
2. **EOF notification** - in the EOF arm, retain attached teardown and add the always-on pane-exit notification using a non-blocking enqueue (never block the exiting reader thread; ignore if the sender/receiver is unavailable).
3. **Channel + task per run loop** - in each daemon run loop, create the channel, spawn the reap task, and prepare the shared sender.
4. **Thread the sender** - pass the shared sender along the existing pane-creation call chain so each reader thread receives a clone.

**Dependencies**: Requires Phase 1.

**Testing Approach**:
- Manual: detach, then exit the last shell; confirm via the log that the pane is reaped and the daemon exits.
- Manual: detach with multiple panes; exit a non-last shell; confirm only that pane is reaped and the daemon stays alive.
- Manual: attached shell exit still tears the pane down (client teardown preserved).
- Automated: existing mux test suite passes; CLI-only feature check still compiles.

**Acceptance Criteria**:
- [ ] Detached last-shell exit reaps the pane and the daemon exits.
- [ ] Attached client teardown is unchanged.
- [ ] Both run loops (Unix socket, Windows named pipe) wire the channel and task.

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/mux/
├── session/
│   └── pane.rs          # MODIFY: shared optional pane-exit sender type alias
├── ipc/
│   ├── pty_spawn.rs     # MODIFY: plumb sender into reader; notify on EOF (always)
│   ├── handlers.rs      # MODIFY: expose pane-destroy reap function to daemon
│   ├── connection.rs    # MODIFY: thread shared sender through pane-creation chain
│   └── reattach.rs      # UNCHANGED: detach skip-guard; race covered by daemon reap
└── daemon.rs            # MODIFY: reap task + channel + spawn + wire on both run loops

doc/tasks/mux-detach-pane-exit-reap/
├── 要件定義書.md
├── SPEC.md
├── IMPLEMENTATION.md
├── VERIFICATION.md
└── tasks.yaml
```

## Testing Strategy

- Unit: idempotent reap behavior; reap-task drive over a constructed session manager.
- Integration: detached last/non-last reap and the connection-reset race against a real session manager and shutdown signal.
- Manual: detach → shell exit → daemon exit, and attached teardown, on a running daemon (verified via the log file).
- Regression: full mux test suite plus the CLI-only (`--no-default-features`) compile check.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | - | Uses existing async runtime channels and shutdown signal |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Double reap when attached (empty-chunk path + daemon task) | Medium | Low | Reap is idempotent; pane-not-found is a warn-level no-op (FR4) |
| Connection-reset race ordering non-determinism | Medium | Medium | Reap ignores output target and is serialized by the session manager's async lock (FR6) |
| Forgetting one of the two run loops | Low | High | Phase 2 acceptance criterion and manual check explicitly cover both Unix and Windows run loops (FR7) |
| Reader thread hangs on send during shutdown | Low | Medium | EOF notification uses a non-blocking enqueue (mirrors the existing notification sender); never blocks the exiting reader. EOF is one-shot per pane and reap drains promptly (M2) |

## Open Questions

- [ ] None outstanding. Scope (detach-EOF + connection-reset race), mechanism
      (dedicated pane-exit channel), and verification (automated + manual) were
      confirmed during spec creation.

## Success Metrics

- [ ] FR1–FR7 implemented; NFR1–NFR4 satisfied.
- [ ] Automated reap/shutdown tests pass; existing mux suite has no regression.
- [ ] Manual detach → Ctrl+D → daemon exit confirmed via the log.
- [ ] Behavior identical on both daemon run loops.
