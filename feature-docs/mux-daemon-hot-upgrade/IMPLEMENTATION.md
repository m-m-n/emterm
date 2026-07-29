# Implementation Plan: mux daemon hot-upgrade via execve

## Overview

The mux daemon replaces itself in place with a newly installed binary through
`execve`, handing its session state over in a versioned file and inheriting
the listen socket plus every PTY master descriptor, so pane shells never see a
hangup.

## Technology Stack

- **Language**: Rust (existing workspace; no new crates introduced)
- **portable-pty 0.8** (already a dependency) — the PTY master abstraction the
  pane layer is written against
- **libc** (already a `cfg(unix)` dependency) — descriptor flag manipulation,
  process replacement, child status collection, terminal size queries
- **serde / bincode** (already used by `mux_ipc` for control messages) — the
  handoff document encoding

No new third-party dependency is introduced, so the MIT project license
(`workflow.yaml project.license: MIT`) needs no compatibility review.

## Layer Structure

```
crates/mux_ipc          wire types + handoff document type + version constants
        ▲                        (no knowledge of daemon internals)
        │
src-tauri/src/mux/
  upgrade.rs            snapshot / restore of the live session tree ⇄ document
  inherited_pty.rs      raw descriptor → PTY master abstraction
        ▲
  daemon.rs             upgrade branch in the accept loop, exec, handoff start
  ipc/connection.rs     Upgrade handling, Upgrading broadcast
  cli.rs                `mux upgrade`, handoff probe, exec after runtime drop
  bridge.rs             client-side reconnect
  session/*             session tree, pane descriptors, child reaping
```

Allowed dependency direction is downward only: `mux_ipc` knows nothing about
`src-tauri`; `upgrade.rs` and `inherited_pty.rs` know nothing about the CLI or
the accept loop.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Handoff document type (`mux_ipc`) | Versioned, serde-encodable description of the daemon's transferable state | Carries: schema version; incarnation token; listen descriptor number; the session-manager ID counters; a nested session → window → pane structure. Each session entry carries its id, name, window ordering, active window and its window-id counter; each window entry its id, name, active pane and its pane-id counter; each pane entry its id, geometry, working directory, title, agent status, exited flag, child process id, master descriptor number and scrollback bytes. Encoded with the same mechanism the crate already uses for control-message payloads. Decoding a document whose schema version the reader cannot restore fails with a distinguishable error and never partially applies. | task0001 (owner); 0003, 0004, 0005 |
| Handoff schema version constant (`mux_ipc`) | Single source of truth for the handoff format version | A monotonically increasing integer, versioned independently of the protocol version. Readers declare the inclusive range of versions they can restore; today that range is the single current value. | task0001 (owner); 0003, 0004, 0005 |
| `Upgrade` / `Upgrading` message types | Request an in-place upgrade / announce that one is imminent | Both mirror the existing shutdown message's wire shape: type byte, pane id zero, empty payload. `Upgrade` travels client → daemon, `Upgrading` daemon → client. A peer that does not know the type discards the frame through the existing unknown-frame path and keeps its connection alive. | task0001 (owner); 0004, 0005, 0006 |
| Inherited master adapter | Presents an already-open PTY master descriptor as the PTY-master abstraction the pane layer expects | Constructed from an owned raw descriptor; takes ownership, so dropping it closes the descriptor. Supports: producing an independent reader handle, producing a writer handle, reading and setting the terminal window size, and exposing its raw descriptor. Unix only. | task0002 (owner); 0003 |
| Upgrade snapshot / restore | Converts the live session tree to and from the handoff document, and prepares or adopts descriptors | **snapshot**: precondition — the caller holds the session-manager lock; postcondition — the returned document describes every session, window and pane; every non-exited pane contributes exactly one master descriptor whose close-on-exec flag has been cleared, and its scrollback is captured while that pane's scrollback lock is held; the listen descriptor's close-on-exec flag is likewise cleared. Failure leaves no descriptor flag changed that the caller cannot revert, and reports which stage failed. **restore**: precondition — a decoded document whose schema version is supported; postcondition — a session manager whose ID counters, incarnation token, session/window/pane tree and per-pane scrollback equal the snapshot's, where every non-exited pane owns an adopted master with its writer and reader thread re-established, and panes recorded as exited are rebuilt as exited without adopting a descriptor. A pane whose descriptor cannot be adopted is rebuilt as exited and the reason logged. | task0003 (owner); 0004 |
| Handoff environment contract | How an exec'ing daemon tells its successor that this start is a handoff | A single environment variable carries the absolute path of the handoff state file. Its presence means handoff startup; its absence means normal startup. The successor removes the file once it has read it, including when restoring fails. The variable is not propagated to pane child processes. | task0004 (owner); 0003, 0005, 0008 |
| Handoff probe subcommand | Lets a running daemon ask a candidate binary which handoff schema versions it can restore | A mux subcommand that prints the supported schema version range to standard output and exits zero. Any non-zero exit, unparsable output, or a range not containing the version the caller is about to write means "incompatible". The probe never touches the daemon socket and never modifies state. | task0005 (owner); 0004 |
| Daemon run outcome | How the async daemon tells its synchronous caller that an upgrade must happen | The daemon's async entry point returns either "terminated normally" or "upgrade requested", the latter carrying everything the caller needs to perform the replacement: the target binary path, the argument vector, the environment addition, and the handoff document path. The caller must fully shut the async runtime down before performing the replacement. | task0004 (owner); 0005 |
| Process-id based child reaping | Reaps pane children whose process handle did not survive the replacement | Accepts a raw process id and applies the same grace-then-terminate policy the existing reaper uses for owned child handles. Used only for panes restored from a handoff. | task0007 (owner); 0003 |

## Conventions

- **Platform gating**: every new upgrade-related module and code path is
  compiled only on Unix. Non-Unix builds keep today's behaviour, and the
  `--no-default-features` CLI build must remain unaffected.
- **Logging**: handoff startup, upgrade abort reasons, and exec failure are
  logged at `warn` or above so they survive release-level filtering. Handoff
  startup is logged in a form that a log reader can distinguish from a normal
  start, and includes the number of panes and descriptors adopted.
- **Error policy**: any failure detected before the process is replaced aborts
  the upgrade, leaves the session tree untouched, and is reported to the
  requesting client over its still-open connection. Failure of the replacement
  itself is recovered in-process by restoring the document that was just
  written.
- **Never** call the existing pane-killing shutdown helper on the upgrade
  path, and never remove the socket file on the upgrade path.
- **Naming**: upgrade-related identifiers use the word `handoff` for the state
  transfer and `upgrade` for the operation, consistently across crates.

## Cross-task Design Decisions

### D1: The replacement happens outside async code

The daemon runs on a multi-threaded async runtime. Replacing the process while
worker threads exist is undefined behaviour, so the async entry point signals
the intent through its return value (Shared Components: daemon run outcome)
and the synchronous caller shuts the runtime down before replacing the
process. Affected tasks: 0004 (produces the outcome), 0005 (consumes it).

### D2: Client connections are not inherited

Only the listen descriptor and the pane master descriptors cross the
replacement. Accepted client connections are dropped. The daemon announces the
imminent replacement first, and clients that received the announcement
reconnect and re-attach; clients that did not receive it keep today's exit
behaviour, so an ordinary shutdown does not turn into a reconnect loop.
Affected tasks: 0004 (announce), 0006 (reconnect).

### D3: Compatibility is probed before the replacement, not after

Once the process has been replaced there is no old daemon left to fall back
to, so a schema mismatch discovered afterwards would be unrecoverable. The
running daemon therefore asks the candidate binary which handoff schema
versions it accepts, and aborts the upgrade before touching anything if the
answer does not cover the version it is about to write. Affected tasks: 0004
(asks), 0005 (answers).

### D4: The listen socket stays open and stays on disk

Neither the descriptor nor the socket file is closed or removed during the
handoff, so connection attempts that arrive mid-upgrade remain queued in the
kernel's backlog and are accepted after the replacement. No explicit queueing
or temporary-failure path is implemented. Affected tasks: 0003, 0004.

### D5: The incarnation token is part of the handoff

Pane child processes carry an environment value derived from the session
manager's incarnation token, baked in when the pane was spawned. Restoring a
fresh token would invalidate every running shell's value, so the token is
carried in the handoff document and restored verbatim. Affected tasks: 0001
(document field), 0003 (restore).

### D6: Child process handles are replaced by process ids

The PTY library's child handle cannot be reconstructed after the process is
replaced. The handoff document carries each pane's child process id instead,
and reaping for restored panes goes through a process-id based path. Affected
tasks: 0001 (document field), 0003 (capture and restore), 0007 (reaper).

### D7: The protocol version is not changed

No existing wire structure changes; only new message types are added, and the
frame decoder already discards unknown types without failing. Bumping the
protocol version would make every new client reject every old daemon for no
benefit. Affected tasks: 0001, 0004, 0005.

### D8: Terminal state is rebuilt from scrollback

Byte-exact terminal-parser state is explicitly out of scope. Restored panes
rebuild their shadow parser by replaying restored scrollback, which is what
the existing reattach path already does for clients. Affected tasks: 0003.

### D9: Integration wiring ownership

task0004 owns wiring the snapshot/restore component into the daemon lifecycle,
and task0005 owns wiring the CLI surface (`upgrade` subcommand, probe
subcommand, and the post-runtime-shutdown replacement in the daemon entry
point). No task is permitted to leave a placeholder for the other: both
implement against the contracts in Shared Components.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Replacing the process with runtime worker threads alive | Medium | High (undefined behaviour) | D1: the replacement is performed by the synchronous caller after runtime shutdown; the async path only returns intent |
| A pane master descriptor is not actually inherited (close-on-exec left set) | Medium | High (shell killed — the exact failure this feature exists to prevent) | Snapshot clears the flag for every contributed descriptor and reports failure per stage; restore verifies each descriptor before adopting it and marks unadoptable panes as exited instead of dropping the session |
| Output produced between the scrollback capture and the replacement is lost | High | Low (cosmetic; shells unaffected) | Capture scrollback under its lock as late as possible; documented as accepted in SPEC.md A7 |
| Reconnect loop turns an ordinary shutdown into a hang | Medium | Medium | D2: reconnect is armed only by the announcement message, and is bounded by a retry window |
| Restoring into a binary with an incompatible handoff format | Low | High | D3: pre-replacement probe; restore additionally validates the version and refuses to partially apply |
| Restored panes are never reaped because the child handle is gone | Medium | Medium | D6: process-id based reaping path |
| The handoff file leaks scrollback contents | Low | Medium | Created with owner-only permissions inside the already owner-only socket directory, and removed after restore or abort |

## Open Questions

- [ ] None. All decisions taken without user confirmation are recorded as
      Assumptions A1–A14 in SPEC.md.
