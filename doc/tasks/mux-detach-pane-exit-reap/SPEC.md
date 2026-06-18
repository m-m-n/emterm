# Feature: Reap Detached Pane Exits in the Mux Daemon

## Overview

The recent fix `fix(mux): reap exited panes ...` (commit `0966e67`) only reaps a
pane when the GUI is attached (`output_target == Connected`): the PTY reader
thread emits an empty "exit" chunk on EOF *only* in the `Connected` arm, and the
daemon's reap (`connection.rs` PTY-output `select!` loop → `handle_destroy_pane`)
is driven by that empty chunk. When a shell dies while the session is
**detached**, no empty chunk is produced, so the pane is never reaped and stays
in the daemon's `SessionManager` as a zombie. This breaks the
"all sessions empty → daemon auto-shutdown" invariant, so `mux kill` /
auto-shutdown can be blocked indefinitely.

This feature makes "PTY death → pane reap" a single authority in the daemon that
is independent of attach state, by adding a dedicated pane-exit channel from the
per-pane reader threads to a daemon-level receiver task.

**Current behavior:** Reap is triggered only via the `Connected` empty-chunk
path in `connection.rs`.

**Target behavior:** The reader thread notifies the daemon of a pane exit on EOF
regardless of attach state; a daemon-level task reaps the pane authoritatively.
The `Connected` empty-chunk path remains for client UI teardown.

## Objectives

- Reap a pane whenever its PTY dies, regardless of attach state (Connected / Detached).
- Restore the "all sessions empty → daemon auto-shutdown" invariant for the detach case.
- Cover the connection-reset race where `detach_session_panes` would otherwise
  strand an exited pane as `Detached(NetworkDetach)`.
- Keep the existing `Connected` client-teardown (`PtyExited`) behavior intact.
- Wire the mechanism on both the Unix-socket and Windows-named-pipe daemon run loops.

## User Stories

### US1: Detached last-shell exit shuts the daemon down
As a user who detached the GUI and left the daemon running, when the last shell
exits while detached, the daemon should reap the pane and auto-shutdown so no
zombie daemon/session remains.

**Acceptance Criteria:**
- [ ] Reader EOF while `Detached` sends a pane-exit notification to the daemon.
- [ ] The daemon reaps the pane via `handle_destroy_pane`.
- [ ] When all sessions become empty, `shutdown_tx.send(true)` fires.

### US2: Detached non-last pane exit reaps only that pane
As a user with multiple detached panes, when one shell exits, only that pane is
reaped and the daemon keeps running.

**Acceptance Criteria:**
- [ ] Only the exited pane is removed from `SessionManager`.
- [ ] Empty windows/sessions are pruned; non-empty sessions keep the daemon alive.

### US3: Connection-reset race does not strand a pane
As a user whose GUI disconnects at the same moment the last shell exits, the pane
must not be left as `Detached(NetworkDetach)` in `SessionManager`.

**Acceptance Criteria:**
- [ ] The pane is reaped even if `detach_session_panes` switched it to
      `Detached(NetworkDetach)` first.
- [ ] Reap is keyed on `pane_id` and ignores `output_target` state.

### US4: Connected teardown is preserved
As the GUI, when a shell exits while attached, I still receive the exit signal so
the UI tears the pane down.

**Acceptance Criteria:**
- [ ] The `Connected` arm still sends the empty `PtyOutputChunk` to the client.
- [ ] Reap remains correct and idempotent if both the Connected empty-chunk path
      and the daemon reap path run for the same pane.

## Technical Requirements

### Functional Requirements

- **FR1:** On reader EOF (`pty_reader_loop` `Ok(0)`), the reader shall send the
  `pane_id` on a dedicated pane-exit channel **regardless of attach state**.
- **FR2:** A daemon-level receiver task shall consume `pane_id`s from that channel
  and reap each via `handle_destroy_pane(pane_id, &session_manager, &shutdown_tx)`,
  making "PTY death → reap" the single authority independent of attach state.
- **FR3:** When the `output_target` is `Connected`, the reader shall still send the
  empty `PtyOutputChunk` to the client (existing UI-teardown / `PtyExited` path).
- **FR4:** Reap shall be idempotent: a `pane_id` that is already removed is a safe
  no-op (the existing `handle_destroy_pane` "pane not found" warn-and-return).
  The Connected empty-chunk reap and the daemon reap may both run for the same
  pane without error.
- **FR5:** When the reaped pane is the last one (all sessions empty),
  `handle_destroy_pane` shall fire `shutdown_tx.send(true)` (existing behavior;
  reachable for the detached case via FR2).
- **FR6:** The connection-reset race (client disconnect + last shell exit
  concurrent → `detach_session_panes` switches the pane to
  `Detached(NetworkDetach)` because `pane.exited` is still `false`) shall not
  strand the pane; the daemon reap shall remove it regardless of `output_target`.
- **FR7:** The dedicated pane-exit channel, its receiver task, and the per-pane
  sender wiring shall be set up on **both** daemon run loops (Unix socket and
  Windows named pipe).

### Non-Functional Requirements

- **NFR1 - Reliability:** Reap behavior must not diverge between attach and detach
  states. Cross-task access to `SessionManager` is serialized by its async
  `Mutex`; the pane-exit channel decouples the blocking reader thread from the
  async daemon.
- **NFR2 - Compatibility:** Must not regress `mux-kill-shutdown` (explicit kill)
  or `close-window-on-shell-exit` (shell exit → tab close). The `Connected`
  empty-chunk reap in `connection.rs` and client teardown remain unchanged.
- **NFR3 - Performance:** The notification is a single send on EOF; the
  steady-state output path (`Ok(n)`) is untouched.
- **NFR4 - Maintainability:** Use a channel distinct from the existing
  `NotificationSender = mpsc::Sender<(PaneId, String)>` (OSC-notification relay),
  whose type and purpose differ.

## Implementation Approach

### Architecture

```
                         per-pane std::thread
┌──────────────────────────────────────────────────────────┐
│ pty_reader_loop  (src-tauri/src/mux/ipc/pty_spawn.rs)      │
│   reader.read() == Ok(0)  (EOF)                            │
│     ├─ if Connected: send empty PtyOutputChunk → client   │  (FR3, UI teardown)
│     └─ always: pane_exit_tx.send(pane_id)                 │  (FR1)
└──────────────────────────────────────────────────────────┘
                              │  mpsc<PaneId>  (NEW dedicated channel)
                              ▼
┌──────────────────────────────────────────────────────────┐
│ run_pane_exit_task  (src-tauri/src/mux/daemon.rs, NEW)     │  (FR2)
│   while let Some(pane_id) = pane_exit_rx.recv().await {    │
│       handle_destroy_pane(pane_id, &mgr, &shutdown_tx)     │
│   }                                                        │
└──────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────┐
│ handle_destroy_pane  (src-tauri/src/mux/ipc/handlers.rs)   │  (existing, FR4/FR5)
│   mark_exited → prune empty window/session →               │
│   if mgr.is_empty(): shutdown_tx.send(true)                │
└──────────────────────────────────────────────────────────┘
```

The `Connected` empty-chunk reap in `connection.rs` (the `select!` PTY-output
loop) stays as-is for the client-drop race it was added for; it coexists with the
daemon reap because reap is idempotent (FR4).

### Mechanism (prescribed)

1. **Dedicated channel type.** Add a pane-exit sender type distinct from
   `NotificationSender`, e.g. in `src-tauri/src/mux/session/pane.rs`:
   ```rust
   pub type PaneExitSender = mpsc::Sender<PaneId>;
   pub type SharedPaneExitSender = Arc<StdMutex<Option<PaneExitSender>>>;
   ```
   Following the existing `SharedNotificationSender` shape (an `Arc<Mutex<Option<_>>>`
   so it can be cloned into reader threads and left `None` in CLI/test paths).

2. **Plumb the sender into the reader.** Pass a `SharedPaneExitSender` into
   `pty_spawn` → `pty_reader_loop` alongside `notification_sender`
   (`src-tauri/src/mux/ipc/pty_spawn.rs`).

3. **Send on EOF regardless of attach state.** In the `Ok(0)` arm of
   `pty_reader_loop` (currently `pty_spawn.rs:197-218`):
   - keep the existing `Connected` empty-chunk send (FR3),
   - then, unconditionally, take the `SharedPaneExitSender` lock and
     `blocking_send(pane_id)` (the reader is a blocking `std::thread`). Ignore a
     send error (receiver dropped == daemon shutting down).

4. **Daemon receiver task.** Add `run_pane_exit_task(session_manager, shutdown_tx,
   pane_exit_rx)` in `src-tauri/src/mux/daemon.rs`, mirroring
   `run_notification_task`. For each `pane_id`, call
   `handle_destroy_pane(pane_id, &session_manager, &shutdown_tx)`. (Note:
   `handle_destroy_pane` is `pub(super)` within `ipc`; expose it to `daemon.rs`
   as needed, e.g. widen visibility or add a thin wrapper.)

5. **Wire both run loops.** In each daemon run function (Unix socket and Windows
   named pipe) create the `(pane_exit_tx, pane_exit_rx)` channel, spawn
   `run_pane_exit_task`, and pass a `SharedPaneExitSender` (built from
   `pane_exit_tx`) down the pane-creation path so reader threads get a clone.

### Why the reader cannot reap directly

`pty_reader_loop` holds `output_target`, `scrollback`, `shadow_parser`,
`notification_sender`, etc., but **not** `shutdown_tx` or `session_manager`.
Reaping mutates `SessionManager` and may fire `shutdown_tx`, both owned on the
async daemon side. The dedicated channel bridges the blocking reader thread to
the async daemon, where `handle_destroy_pane` already does the reap + shutdown.

### cwd map

`pane_cwd_map` (status bar) is per-connection (`connection.rs:158`). The daemon
reap task has no connection and therefore no cwd map; while detached no such map
exists for the session, so daemon-side reap needs no cwd cleanup. The
`Connected` path keeps cleaning its own cwd map as today.

### Data Flow

```
Shell exits (exit / Ctrl+D / process death)
    ↓
pty_reader_loop: reader.read() → Ok(0)  (EOF)
    ↓
[Connected only] send empty PtyOutputChunk → client (PtyExited / UI teardown)
    ↓
[always] pane_exit_tx.send(pane_id)
    ↓
run_pane_exit_task: pane_exit_rx.recv()
    ↓
handle_destroy_pane(pane_id, &session_manager, &shutdown_tx)
    ↓ mark_exited + prune empty window/session
    ↓
if all sessions empty → shutdown_tx.send(true) → daemon exits
```

### File Structure

```
src-tauri/src/mux/
├── session/pane.rs        # NEW type aliases: PaneExitSender, SharedPaneExitSender
├── ipc/
│   ├── pty_spawn.rs       # plumb SharedPaneExitSender into reader; send pane_id on EOF (Ok(0) arm)
│   ├── handlers.rs        # handle_destroy_pane (reap body; expose to daemon.rs)
│   ├── connection.rs      # unchanged Connected empty-chunk reap (coexists, idempotent)
│   └── reattach.rs        # detach_session_panes skip-guard (unchanged; race covered by daemon reap)
└── daemon.rs              # NEW run_pane_exit_task; create channel + spawn + wire on both run loops
```

### Dependencies

**Internal:** `SessionManager` / `handle_destroy_pane`, `pty_reader_loop`,
`PaneOutputTarget`, `shutdown_tx` (`tokio::sync::watch`).

**External:** `tokio` (mpsc, watch) — already present. No new crates.

## Test Scenarios

### Unit / Integration Tests (Rust)

Run with: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`

- [ ] **Detached last-pane reap → shutdown:** Build a `SessionManager` with one
      session/window/pane and a `shutdown_tx`; drive the reap path
      (`handle_destroy_pane` for the pane, or `run_pane_exit_task` fed one
      `pane_id`) and assert the pane is removed, the session is gone,
      `mgr.is_empty()` is true, and the watch channel observed `true`.
- [ ] **Detached non-last pane reap:** Two panes in distinct windows; reap one and
      assert only it is removed and `shutdown_tx` did **not** fire.
- [ ] **Connection-reset race (FR6):** Switch a pane to
      `Detached(NetworkDetach)` (as `detach_session_panes` does), then run the
      reap and assert the pane is removed despite the `Detached` `output_target`.
- [ ] **Idempotent reap (FR4):** Reap the same `pane_id` twice; the second call is
      a safe no-op (no panic; warn logged).

> Note: directly driving the blocking `pty_reader_loop` thread to real EOF is
> impractical in a unit test. Tests target the reap authority
> (`run_pane_exit_task` / `handle_destroy_pane` over a `SessionManager`), which is
> the behavior FR2/FR4/FR5/FR6 specify. The reader→channel send (FR1/FR3) is
> covered by manual verification.

### Manual Verification

Documented in `VERIFICATION.md`:

- [ ] Start the daemon, attach the GUI, then detach. With the daemon detached,
      cause the last shell to exit (Ctrl+D). Confirm via `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
      that the pane is reaped (`Destroyed pane`, `All sessions empty, daemon shutting down`)
      and the daemon process exits.
- [ ] Repeat with a non-last pane and confirm only that pane is reaped and the
      daemon stays alive.

### Edge Cases

- [ ] Daemon already shutting down when the reader sends → `blocking_send` errors;
      reader ignores it; no panic.
- [ ] Both Connected empty-chunk reap and daemon reap run for the same pane →
      idempotent (FR4).

## Security Considerations

Not applicable — in-process channel between threads/tasks; no external input, no
network endpoints, no persisted data.

## Error Handling

| Condition | Handling |
|-----------|----------|
| `pane_exit_tx.send` fails (receiver dropped) | Reader ignores (daemon shutting down) |
| `handle_destroy_pane` pane not found | warn log + return (idempotent no-op) |

## Success Criteria

- [ ] FR1–FR7 implemented and verified.
- [ ] Rust reap/shutdown tests pass (detached last/non-last, race, idempotency).
- [ ] Manual detach → Ctrl+D → daemon exits confirmed via log.
- [ ] No regression in existing mux tests; `cargo check --no-default-features` passes.
- [ ] Behavior identical on Unix-socket and Windows-named-pipe daemon run loops.

## References

- Draft: `tmp/mux-detach-pane-exit-reap.md`
- Requirements: `doc/tasks/mux-detach-pane-exit-reap/要件定義書.md`
- Adjacent: `doc/tasks/mux-kill-shutdown/SPEC.md`, `doc/tasks/close-window-on-shell-exit/SPEC.md`
- Related commit: `0966e67` (Connected-path reap fix)
- Key code: `src-tauri/src/mux/ipc/pty_spawn.rs` (reader EOF),
  `src-tauri/src/mux/ipc/connection.rs` (Connected reap, `detach_session_panes` call),
  `src-tauri/src/mux/ipc/handlers.rs` (`handle_destroy_pane`),
  `src-tauri/src/mux/ipc/reattach.rs` (`detach_session_panes` skip-guard),
  `src-tauri/src/mux/session/pane.rs` (`mark_exited`, sender type aliases),
  `src-tauri/src/mux/daemon.rs` (notification task pattern, `shutdown_tx`, run loops)
