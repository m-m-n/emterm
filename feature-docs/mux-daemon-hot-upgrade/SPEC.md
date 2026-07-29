# Feature: mux daemon hot-upgrade via execve

## Overview

The mux daemon replaces itself in place with a newly installed eMterm binary
using `execve()`, instead of shutting down and respawning. PTY master file
descriptors survive `execve`, so every pane's shell keeps running and never
sees a hangup. State (session/window/pane tree, titles, cwd, scrollback,
agent status, ID allocation counters, incarnation token) is handed off through
a versioned, 0600 state file, and the listen socket plus every PTY master FD
is inherited across the exec.

## Objectives

- Updating the eMterm binary must not kill the shells running inside mux panes.
- Replace the pane-destroying `graceful_shutdown()` path taken by
  `recover_from_legacy_daemon` with an in-place upgrade whenever the running
  daemon supports it.
- Fail safe: every failure detected before `execve`, and `execve` failure
  itself, leaves the old daemon running with its panes untouched.

## User Stories

### US1: Explicit hot-upgrade

As a mux user, I want to run `emterm mux upgrade` after installing a new
eMterm build, so that the daemon picks up the new binary without killing my
ssh sessions and long-running processes.

**Acceptance Criteria:**
- [ ] `emterm mux upgrade` triggers an in-place upgrade of the running daemon.
- [ ] After the upgrade, each pane's shell process has the same PID as before.
- [ ] A file created by a shell before the upgrade is still observable from
      that same shell afterwards.
- [ ] Attached clients reconnect automatically and see their panes restored.

### US2: Automatic hot-upgrade on attach

As a mux user, I want `emterm mux attach` to hot-upgrade a protocol-mismatched
daemon rather than shutting it down, so that a package update does not destroy
my open work.

**Acceptance Criteria:**
- [ ] `recover_from_legacy_daemon` attempts `Upgrade` before shutdown.
- [ ] When the running daemon does not support `Upgrade`, the client falls
      back to the existing shutdown-then-respawn path after a bounded timeout.

### US3: Safe fallback

As a mux user, I want a failed upgrade to be a no-op, so that a broken or
incompatible new binary never costs me my panes.

**Acceptance Criteria:**
- [ ] A handoff-schema mismatch aborts the upgrade before `execve`; the old
      daemon keeps running.
- [ ] An `execve` failure leaves the old daemon running; the error is logged
      and returned to the requesting client.
- [ ] No pane's shell is killed on any abort path.

## Technical Requirements

### Functional Requirements

- **FR1:** Add `MessageType::Upgrade` (client → daemon, empty payload) to
  `crates/mux_ipc/src/protocol.rs`, mirroring the wire shape of
  `MessageType::Shutdown = 0x18` (type byte + `pane_id: 0` + empty payload).
  Older peers must discard it via the existing unknown-frame path in
  `src-tauri/src/mux/ipc/codec.rs:37-54` rather than erroring.
- **FR2:** Add `MessageType::Upgrading` (daemon → client, empty payload),
  broadcast to every connected client immediately before `execve`, so a client
  can distinguish an upgrade-induced disconnect from a shutdown.
- **FR3:** Handle `Upgrade` in the daemon's connection layer next to the
  existing `Shutdown` arm (`src-tauri/src/mux/ipc/connection.rs:693`) by
  signalling an upgrade watch channel, so the accept loop
  (`src-tauri/src/mux/daemon.rs:648`) breaks into an upgrade branch that
  **skips** `graceful_shutdown()` (`daemon.rs:1134`) and **skips**
  `remove_file(&sock_path)` (`daemon.rs:687`).
- **FR4:** Serialize the daemon's in-memory state into a versioned handoff
  document covering: `SessionManager.incarnation`, `next_session_id`,
  `next_pane_id`; per session `id`, `name`, `window_order`,
  `active_window_id`, `next_window_id`; per window `id`, `name`,
  `active_pane_id`, `next_pane_id`; per pane `id`, `cols`, `rows`, `cwd`,
  `title`, `agent_status`, `exited`, `child_pid`, `master_fd`, and the
  scrollback byte contents.
- **FR5:** Clear `FD_CLOEXEC` (`fcntl(fd, F_SETFD, 0)`) on the listen socket
  FD and on every live pane's PTY master FD (`MasterPty::as_raw_fd`) before
  `execve`, and record those FD numbers in the handoff document.
- **FR6:** Write the handoff document to a file created with mode `0600` in
  the socket's parent directory (already `0700`), and pass its path to the new
  process via an environment variable so the new daemon can find it.
- **FR7:** Perform `execve()` only after the tokio runtime has been fully shut
  down, from the process's main thread. `run_daemon()` returns an outcome
  value indicating "upgrade requested" instead of exec'ing from inside async
  code; `execute_daemon()` (`src-tauri/src/mux/cli.rs:248-254`) drops the
  runtime and then execs.
- **FR8:** On startup, detect handoff mode from the environment, skip the
  normal socket bind / stale-socket cleanup, and adopt the inherited listen
  socket FD as the daemon's `UnixListener`.
- **FR9:** Deserialize the handoff document and restore `SessionManager`,
  including the private ID allocation counters (`MuxSession::next_window_id`,
  `MuxWindow::next_pane_id`) and the `incarnation` token, so already-running
  shells' `EMTERM_PANE_ID` values stay valid.
- **FR10:** Re-adopt each inherited PTY master FD as a `portable_pty::MasterPty`
  implementation (portable-pty exposes no public constructor from a raw fd, so
  a Unix-only inherited-master type is required), and re-establish each pane's
  writer and reader thread, restoring the scrollback buffer contents.
- **FR11:** Log handoff startup in a form distinguishable from a normal start,
  including the number of panes and FDs adopted.
- **FR12:** Make the client reconnect: after receiving `Upgrading`, the bridge
  (`src-tauri/src/mux/bridge.rs:181`) enters a bounded reconnect loop and
  re-attaches to the same session instead of exiting. A disconnect **without**
  a preceding `Upgrading` keeps today's behavior (exit).
- **FR13:** Every failure detected while the async runtime is still alive
  (schema probe, snapshot, state-file write, `FD_CLOEXEC` clear) aborts the
  upgrade, is logged, and is reported to the requesting client, after which
  the daemon keeps serving normally. If `execve` itself fails — necessarily
  after runtime shutdown, when no client connection remains — the process
  logs the failure at `error` and re-enters service in the same process by
  restoring from the handoff document it just wrote, so panes stay attached
  and reconnecting clients find a working daemon.
- **FR14:** Define a handoff schema version in `crates/mux_ipc`, versioned
  independently of `PROTOCOL_VERSION`. Before `execve`, probe the new binary
  for its supported handoff schema version(s) and abort the upgrade when
  incompatible.
- **FR15:** Add an `emterm mux upgrade` subcommand to the mux CLI dispatch
  table (`src-tauri/src/mux/cli.rs:60-206`, including its usage text), which
  connects to the daemon, sends `Upgrade`, waits for the upgraded daemon to
  become reachable, and reports the outcome. On Windows it reports that
  hot-upgrade is unsupported.
- **FR16:** Change `recover_from_legacy_daemon` (`daemon.rs:419`) to attempt
  `Upgrade` first and fall back to the existing shutdown-then-respawn path
  only after a bounded timeout with no upgraded daemon reachable.
- **FR17:** Do not close the listen socket during the handoff, so connections
  arriving mid-upgrade stay queued in the kernel listen backlog and are
  accepted after `execve`. No explicit queueing or EAGAIN path is implemented.
- **FR18:** Serialize each pane's child PID and reap children after an upgrade
  through a PID-based path, since `Box<dyn portable_pty::Child>` cannot be
  reconstructed after `execve`.
- **FR19:** Unlink the handoff state file after a successful restore and on
  every abort path.

### Non-Functional Requirements

- **NFR1 - Security:** The handoff state file is created with mode `0600`
  inside the `0700` socket directory and unlinked promptly after restore or
  abort. Handoff parameters carry only the file path and FD numbers.
- **NFR2 - Reliability:** No pane's shell process is killed by a successful,
  aborted, or failed upgrade. Every failure path leaves either the old daemon
  running or the shells attached to the new daemon.
- **NFR3 - Performance:** For a typical session (single-digit pane count,
  default scrollback capacity) the window between `Upgrade` receipt and the
  new daemon accepting connections is a few seconds at most, and clients
  reconnect within their retry window.
- **NFR4 - Platform:** The upgrade path is Unix-only (`#[cfg(unix)]`). The
  Windows daemon (`daemon.rs:696`, named pipes) and the
  `--no-default-features` CLI build must continue to compile and behave as
  today.
- **NFR5 - Observability:** Handoff startup is distinguishable from normal
  startup in `mux-daemon.log`, and abort reasons are logged at `warn` or above
  so they persist in release builds.
- **NFR6 - Compatibility:** Introducing the new message types must not break
  peers built before this feature. `PROTOCOL_VERSION` stays at its current
  value because no existing bincode struct changes.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ client (GUI bridge / CLI)                                    │
│   emterm mux upgrade  ──Upgrade──►                           │
│   bridge reconnect loop ◄──Upgrading──                       │
├──────────────────────────────────────────────────────────────┤
│ mux_ipc (protocol.rs)                                        │
│   MessageType::Upgrade / Upgrading                           │
│   HANDOFF_SCHEMA_VERSION + handoff document types            │
├──────────────────────────────────────────────────────────────┤
│ daemon (daemon.rs, connection.rs, cli.rs)                    │
│   upgrade watch channel → accept-loop upgrade branch         │
│   snapshot → schema probe → FD_CLOEXEC clear → execve        │
│   handoff-mode startup → restore → adopt FDs                 │
├──────────────────────────────────────────────────────────────┤
│ session (manager.rs / session.rs / window.rs / pane.rs)      │
│   serializable projection of the session tree                │
│   inherited-master PTY adoption, PID-based reaping           │
└──────────────────────────────────────────────────────────────┘
```

### Data Flow

```
Upgrade ──► daemon
  probe new binary handoff schema ─┬─ incompatible ─► abort (daemon continues)
                                   └─ compatible
  snapshot session tree (+ scrollback under lock) ─► 0600 state file
  clear FD_CLOEXEC on listen fd + each pane master fd
  broadcast Upgrading ─► clients enter reconnect
  stop tokio runtime ─► execve(self_exe, ["mux","--daemon"], env+handoff)
                                   │
                                   ├─ execve fails ─► unlink state, log, continue
                                   └─ execve succeeds
  new daemon: detect handoff ─► read+verify state ─► restore SessionManager
             ─► adopt listen fd ─► adopt master fds, respawn readers/writers
             ─► unlink state file ─► accept (backlog drains) ─► clients re-attach
```

### Handoff document

A serde-serializable document defined in `crates/mux_ipc`, versioned by
`HANDOFF_SCHEMA_VERSION` (independent of `PROTOCOL_VERSION`). Shape (fields
per FR4/FR5/FR18):

```
HandoffDocument {
  schema_version: u32,
  incarnation: String,
  listen_fd: RawFd,
  next_session_id, next_pane_id,
  sessions: [ { id, name, window_order, active_window_id, next_window_id,
                windows: [ { id, name, active_pane_id, next_pane_id,
                             panes: [ { id, cols, rows, cwd, title,
                                        agent_status, exited,
                                        child_pid, master_fd,
                                        scrollback: Vec<u8> } ] } ] } ]
}
```

### Schema compatibility probe

The old daemon runs the new binary with a probe subcommand that prints the
handoff schema version range it can restore, and compares it against
`HANDOFF_SCHEMA_VERSION` of the document it is about to write. Probing before
`execve` is required: after `execve` the old daemon no longer exists, so a
post-exec mismatch has no safe fallback.

### Dependencies

**Internal Dependencies:**
- `crates/mux_ipc`: message types, handoff schema version and document types.
- `src-tauri/src/mux/daemon.rs`: accept loop, upgrade branch, recovery path.
- `src-tauri/src/mux/ipc/connection.rs`: `Upgrade` handling, `Upgrading` broadcast.
- `src-tauri/src/mux/session/{manager,session,window,pane}.rs`: serializable
  projection, ID-counter restore, inherited-master adoption.
- `src-tauri/src/mux/scrollback_buffer.rs`: scrollback snapshot/restore.
- `src-tauri/src/mux/ipc/pty_spawn.rs`: reader-thread wiring reused for
  re-adopted panes.
- `src-tauri/src/mux/bridge.rs`: client reconnect loop.
- `src-tauri/src/mux/cli.rs`: `upgrade` subcommand, probe subcommand, daemon
  entry that performs the exec after runtime shutdown.
- `src-tauri/src/mux/session/child_reaper.rs`: PID-based reaping.
- `src-tauri/src/self_exec.rs`: `self_exe_path()` for the exec target.

**External Dependencies:**
- `portable-pty` 0.8 — `MasterPty::as_raw_fd`; no public raw-fd constructor,
  hence the inherited-master type.
- `libc` (already a `cfg(unix)` dependency) — `fcntl`, `execv`, `waitpid`,
  terminal size ioctls.
- `serde` / `bincode` — handoff document encoding, matching the existing
  control-message encoding style.

### File Structure

```
crates/mux_ipc/src/
  protocol.rs            # Upgrade / Upgrading message types
  handoff.rs             # HANDOFF_SCHEMA_VERSION + HandoffDocument (new)
src-tauri/src/mux/
  daemon.rs              # upgrade branch, exec preparation, handoff startup
  upgrade.rs             # snapshot / restore / FD preparation (new, cfg(unix))
  inherited_pty.rs       # MasterPty over an inherited raw fd (new, cfg(unix))
  cli.rs                 # `mux upgrade`, handoff probe, exec after runtime drop
  bridge.rs              # reconnect loop
  ipc/connection.rs      # Upgrade handling, Upgrading broadcast
  session/{manager,session,window,pane}.rs
  session/child_reaper.rs
src-tauri/tests/
  mux_hot_upgrade.rs     # integration test (new, cfg(unix))
```

## Test Scenarios

### Unit Tests
- [ ] `MessageType::Upgrade` / `Upgrading` round-trip through
      `to_frame_body` / `from_frame_body`.
- [ ] A frame carrying the new message types is discarded, not fatal, by a
      codec that does not know them (mirrors
      `test_codec_unknown_frame_is_discarded_not_fatal`).
- [ ] Handoff document serialize → deserialize round-trip preserves the
      session tree, ID counters, incarnation token, and scrollback bytes.
- [ ] Handoff schema version mismatch is rejected by the restore path.
- [ ] The upgrade branch of the accept loop does not call `graceful_shutdown`
      and does not unlink the socket (asserted on a `SessionManager` built
      with `MuxPane::new_test`).
- [ ] `recover_from_legacy_daemon` falls back to shutdown-then-respawn when
      the daemon never becomes reachable after `Upgrade` (fake daemon that
      ignores the frame, following `spawn_fake_legacy_daemon`'s pattern).
- [ ] `emterm mux upgrade` reports unsupported on non-Unix builds.

### Integration Tests
- [ ] Start a daemon in an isolated `XDG_RUNTIME_DIR`, open a pane running
      `/bin/sh`, have the shell `touch` a marker file, trigger the upgrade,
      then send a command to the same pane and confirm both that the shell's
      PID is unchanged and that the marker file is visible from inside that
      shell.
- [ ] After the upgrade, the daemon log contains the handoff-startup marker
      and the adopted pane count.
- [ ] Upgrading a daemon with zero panes succeeds.
- [ ] An upgrade pointed at an incompatible handoff schema aborts and the
      original daemon still answers a handshake.

### E2E Tests
**Existing E2E tests**: None (per `test/README.md`; there is no
`docker-compose.e2e.yml` and no `e2e-tests/` directory).
**Run command**: Not detected

### Edge Cases
- [ ] A pane whose shell exited before the upgrade is restored as `exited`
      and is not re-adopted.
- [ ] A connection that arrives during the handoff is served after `execve`
      from the listen backlog.
- [ ] Output produced between the scrollback snapshot and `execve` may be
      missing from scrollback; the shell process is unaffected.
- [ ] The handoff state file is unlinked even when restore fails.

### Performance Tests
- [ ] Upgrade of a daemon with several panes at default scrollback capacity
      completes within a few seconds end to end.

## Security Considerations

- **Data Protection:** The handoff state file contains pane scrollback. It is
  created `0600` inside the existing `0700` socket directory and unlinked
  after restore or abort.
- **Input Validation:** The handoff document's schema version is validated
  before any field is trusted; inherited FD numbers are validated as live
  descriptors before adoption.
- **Privilege:** The upgrade executes `self_exec::self_exe_path()` only; the
  binary path is never taken from client input.

## Error Handling

| Condition | Behavior |
|---|---|
| Handoff schema incompatible | Abort before `execve`; log at `warn`; error to client; daemon continues |
| State file write failure | Abort before `execve`; same as above |
| `FD_CLOEXEC` clear failure | Abort before `execve`; same as above |
| `execve` failure | Log at `error`; restore from the handoff document in-process; unlink state file; daemon continues serving |
| Restore failure in the new daemon | Log at `error`; unlink state file; panes that cannot be restored are marked `exited` |
| Client reconnect exhausted | Client exits with today's behavior after the retry window |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Security requirements are satisfied
- [ ] `cargo test --lib` and the CLI-only `cargo check --no-default-features`
      both pass
- [ ] No pane's shell is killed on any upgrade path

## Assumptions

Recorded because this feature was specified in batch mode with no user
dialogue, and Codex CLI was unavailable (`command -v codex` failed). Each item
is a decision taken from codebase evidence rather than a confirmed answer.

- **A1:** The handoff medium is a `0600` temp file rather than
  `memfd_create`, keeping the handoff inspectable and avoiding extra libc
  surface. The task's acceptance criteria allow either.
- **A2:** Handoff-schema compatibility is probed by executing the new binary
  with a probe subcommand **before** `execve`, because a post-exec mismatch
  leaves no old daemon to fall back to.
- **A3:** Accepted client connections are **not** preserved across `execve`.
  Clients reconnect, per the task's stated acceptance criterion.
- **A4:** Connections arriving during the handoff rely on the kernel listen
  backlog; no explicit queue or EAGAIN handling is implemented.
- **A5:** A Unix-only inherited-master type implementing
  `portable_pty::MasterPty` over the inherited raw fd is introduced, because
  portable-pty 0.8 has no public raw-fd constructor.
- **A6:** `Box<dyn Child>` is not reconstructed; the child PID is carried in
  the handoff document and reaping after an upgrade uses a PID-based path.
- **A7:** Output produced between the scrollback snapshot and `execve` may be
  lost from scrollback. Shell processes are unaffected. This is consistent
  with the task's scope-out of byte-exact VT parser state restoration.
- **A8:** Auto-triggered upgrade from the recovery path only helps when the
  **running** daemon already ships this feature; upgrading from a
  pre-feature daemon still falls back to shutdown-then-respawn.
- **A9:** Hot-upgrade is Unix-only. On Windows `emterm mux upgrade` reports
  unsupported and the recovery path keeps today's shutdown-then-restart.
- **A10:** The integration test lives in `src-tauri/tests/mux_hot_upgrade.rs`,
  is `cfg(unix)`, and follows the isolated-`XDG_RUNTIME_DIR` daemon-spawn
  pattern of `src-tauri/tests/mux_throughput.rs`.
- **A11:** The `incarnation` token is preserved so already-running shells'
  `EMTERM_PANE_ID` stays valid.
- **A12:** VT parser / shadow-parser state is rebuilt by replaying restored
  scrollback rather than serialized byte-for-byte.
- **A13:** `PROTOCOL_VERSION` is not bumped, since no existing bincode struct
  changes and new message types are discarded gracefully by old peers.
- **A14:** `execve` failure is recovered by restoring the just-written handoff
  document **in the same process** rather than by returning an error over the
  requesting client's connection, because that connection is necessarily gone
  once the runtime has been shut down. All earlier abort causes are reported
  to the client directly, while the runtime is still alive.

## References

- Notion task: [https://www.notion.so/3a73509ec8ee81f2afecf815ededbe4c](https://www.notion.so/3a73509ec8ee81f2afecf815ededbe4c)
- `REQUIREMENTS.md` (this feature)
- `src-tauri/src/mux/daemon.rs:1134` — `graceful_shutdown` (current pane-killing path)
- `src-tauri/src/mux/session/pane.rs:782-838` — PTY master / writer / child storage
- `crates/mux_ipc/src/protocol.rs:47` — `PROTOCOL_VERSION`
- `src-tauri/tests/mux_throughput.rs` — daemon-spawning integration test pattern
