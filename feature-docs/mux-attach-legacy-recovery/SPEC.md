# Feature: mux attach legacy daemon recovery

## Overview

`emterm mux attach` exits silently after an eMterm binary upgrade because a
long-lived mux daemon started by the old binary (protocol v1) rejects the new
client's handshake (`Protocol version mismatch: client=2, server=1`). The
recovery path for this exact situation — `recover_from_legacy_daemon()`
(Strategy B) — already exists and is used by `ensure_daemon_running()`, but
`execute_attach` bypasses it entirely: it only checks `sock_path.exists()`
and jumps straight into `run_bridge()`. This feature inserts the recovery
step (and, when recovery shut the legacy daemon down, a daemon respawn) into
the attach path while preserving attach's existing semantics (no daemon →
error, never silently create a fresh daemon).

## Objectives

- Make `emterm mux attach` succeed right after an eMterm upgrade without the
  user manually killing the stale daemon.
- Keep attach's "no daemon → error" semantics intact.
- Keep `emterm mux` / `emterm mux script` behavior unchanged.

## User Stories

### US1: Attach after upgrade
As a mux user, I want `emterm mux attach` to transparently recover from a
stale legacy daemon, so that re-attaching works right after upgrading eMterm.

**Acceptance Criteria:**
- [ ] With a v1 (legacy) daemon listening, `emterm mux attach`'s pre-bridge
      path shuts it down, spawns a new daemon from the current binary, and
      proceeds to a successful handshake.
- [ ] With a compatible (current-version) daemon, attach behaves exactly as
      today.
- [ ] With no daemon at all (no socket), attach fails with the current error
      message unchanged.

## Technical Requirements

### Functional Requirements

- **FR1:** Extract the daemon-spawn logic of `ensure_daemon_running`
  (`src-tauri/src/mux/daemon.rs`, the `!daemon_running` branch: socket parent
  directory creation with restricted permissions, process spawn with
  platform-specific detach flags, readiness wait with exponential backoff)
  into a dedicated function (working name `spawn_daemon(sock_path: &Path)
  -> Result<(), String>`; exact signature at the planner's discretion).
  `ensure_daemon_running` is refactored to call
  `recover_from_legacy_daemon` → `spawn_daemon` as needed, with no
  externally observable behavior change.
- **FR2:** `execute_attach` (`src-tauri/src/mux/cli.rs`) runs the recovery
  step after its `sock_path.exists()` check and before `run_bridge()`:
  - socket absent → return the current error (semantics preserved),
  - `recover_from_legacy_daemon(&sock_path)` → `LegacyRecovery::Compatible`
    → proceed to `run_bridge()` as today,
  - `LegacyRecovery::Recovered` → the legacy daemon has been shut down;
    spawn a new daemon via `spawn_daemon(&sock_path)` and then proceed to
    `run_bridge()`,
  - recovery/spawn errors propagate as the command's error result.
- **FR3:** Tests cover the attach-path recovery using the existing
  `FAKE_LEGACY_VERSION` fake-legacy-daemon test infrastructure
  (`src-tauri/src/mux/daemon.rs` test module):
  - legacy (v1) daemon listening → recovery runs, a new daemon comes up,
    handshake succeeds,
  - compatible daemon listening → attach path is a no-op pass-through,
  - no daemon → the current error message is returned unchanged.

### Non-Functional Requirements

- **NFR1 - Compatibility:** Behavior parity on Linux and Windows — the
  extracted `spawn_daemon` keeps the existing `#[cfg(unix)]` /
  `#[cfg(windows)]` branches (setsid / DETACHED_PROCESS) exactly as they are
  today. Existing daemon/mux tests keep passing.

## Implementation Approach

### Affected code

| File | Change |
|------|--------|
| `src-tauri/src/mux/daemon.rs` | Extract `spawn_daemon`; refactor `ensure_daemon_running` to use it; widen `recover_from_legacy_daemon` visibility (currently private `fn`) so the cli module can call it (`pub(crate)` or module-appropriate) |
| `src-tauri/src/mux/cli.rs` | Insert recovery + conditional spawn into `execute_attach` between the socket-existence check and `run_bridge()` |

### Control flow (attach)

```
execute_attach
  ├─ check_nesting / init_bridge_logger        (unchanged)
  ├─ sock_path.exists()? ─ no → Err("No mux sessions to attach to …")   (unchanged)
  ├─ recover_from_legacy_daemon(&sock_path)?
  │    ├─ Compatible → (nothing)
  │    └─ Recovered  → spawn_daemon(&sock_path)?
  └─ run_bridge(&sock_path)                    (unchanged)
```

### Testability note

`run_bridge()` is a long-running interactive bridge, so tests do not drive
`execute_attach` end-to-end. The pre-bridge sequence (socket check →
recovery → conditional spawn) may be extracted into a testable helper that
`execute_attach` calls before `run_bridge()`; tests then exercise that
helper directly against the fake daemons. The existing daemon test module
already provides a fake legacy (v1) server and a fake compatible server
(`recover_from_legacy_daemon_*` tests) to build on.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/daemon.rs`: `recover_from_legacy_daemon`,
  `LegacyRecovery`, `is_daemon_running`, `socket_path`
- `crates/mux_ipc/src/protocol.rs`: `PROTOCOL_VERSION`,
  `PREVIOUS_PROTOCOL_VERSION` (read-only; unchanged)

**External Dependencies:** none added.

## Test Scenarios

### Unit / Integration Tests (cargo, `--lib`)

- [ ] TS-1: legacy (v1) fake daemon listening → attach pre-bridge helper
      returns success after shutting down the fake and spawning a real
      daemon; a subsequent handshake to the socket is `Accepted`.
- [ ] TS-2: compatible fake daemon listening → attach pre-bridge helper
      returns success without spawning anything; the fake daemon still owns
      the socket.
- [ ] TS-3: no socket present → attach pre-bridge helper returns the
      existing "No mux sessions to attach to" error unchanged.
- [ ] TS-4: `ensure_daemon_running` regression — existing tests
      (`recover_from_legacy_daemon_*` and daemon lifecycle tests) keep
      passing after the `spawn_daemon` extraction.

### E2E Tests
**Existing E2E tests**: None (per test/README.md).
**Run command**: Not applicable.

### Edge Cases

- [ ] Recovery probe errors (e.g. connection refused on a stale socket
      file): propagate as an error result — same as `ensure_daemon_running`
      does today via `recover_from_legacy_daemon(...)?`.
- [ ] `spawn_daemon` readiness timeout: returns the existing
      "Failed to start mux daemon" error.

## Security Considerations

- No new input surfaces. Socket directory permissions (0o700) behavior is
  preserved verbatim inside the extracted `spawn_daemon`.

## Error Handling

| Case | Result |
|------|--------|
| Socket absent | `Err("No mux sessions to attach to (daemon not running)\nUse 'emterm mux' to start a new session.")` — unchanged |
| Recovery probe failure | `Err(<recover_from_legacy_daemon error>)` |
| Spawn failure / readiness timeout | `Err(<spawn_daemon error>)` (same strings as today's `ensure_daemon_running`) |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass (`cargo test --lib`, quick-check target dir)
- [ ] `emterm mux` / `emterm mux script` behavior unchanged (existing tests)
- [ ] Code review is completed

## Assumptions

Batch-mode decisions taken without user confirmation (Codex CLI was
unavailable; decided from the task description and code inspection):

- **A1:** The exact `spawn_daemon` signature (return `()` vs `PathBuf`) is
  left to the planner; the task description suggested
  `Result<PathBuf, String>`, and either is acceptable as long as
  `ensure_daemon_running`'s observable behavior is unchanged.
- **A2:** Tests exercise an extracted pre-bridge helper rather than driving
  `execute_attach` (and thus `run_bridge`) end-to-end, because `run_bridge`
  is a long-running interactive process. This satisfies the task's "attach
  経路からも integration test で確認" intent at the highest testable layer.
- **A3:** `recover_from_legacy_daemon`'s visibility is widened to
  `pub(crate)` (or an equivalent module-visible form) so `cli.rs` can call
  it; no public API surface is added.
- **A4:** The design step is skipped — this is a CLI/daemon bug fix with no
  visible UI.

## References

- Notion task: https://www.notion.so/3a73509ec8ee819b9a8cd346a7360a51
- REQUIREMENTS.md (Japanese requirements document, same directory)
- Related memory: `project_mux_conpty_asymmetric_transport.md`,
  `project_mux_reattach_da1_leak.md`
