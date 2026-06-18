# Verification Document: Reap Detached Pane Exits in the Mux Daemon

## Overview

**Feature**: mux-detach-pane-exit-reap
**SPEC.md**: `doc/tasks/mux-detach-pane-exit-reap/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-detach-pane-exit-reap/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only gate: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Release (the binary the user runs): `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors.

### Actual Results (implement phase)

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` — exit 0, no warnings.
- CLI-only gate `... cargo check ... --no-default-features` — exit 0.
- Windows cfg wiring (FR7): `CARGO_TARGET_DIR=src-tauri/target-win cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` — exit 0 (validates the `#[cfg(windows)]` named-pipe run-loop wiring compiles; full cross-build not run).
- Release build (`target-host`): not run during the implement phase (deferred to the user per project policy "do not run unsolicited release builds").

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: the new reap-authority path (daemon reap task + pane-destroy
  reap behavior) covered by automated tests; the reader→channel send and run-loop
  wiring covered by manual verification (driving a real blocking reader thread to
  EOF in a unit test is impractical).

### Actual Results (implement phase)

- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`:
  - lib: 1770 passed, 0 failed, 1 ignored.
  - integration (`tests/cli_subcommands.rs`): 12 passed, 0 failed.
  - doc-tests: 0.
- New automated tests added to `src-tauri/src/mux/daemon.rs` (all passing, gated `#[cfg(all(test, unix))]`):
  - TS-1 `test_pane_exit_task_last_pane_reap_fires_shutdown` (drives `run_pane_exit_task`; asserts pane/session removed, manager empty, watch channel observes `true`).
  - TS-2 `test_pane_exit_task_non_last_pane_reap_keeps_daemon_alive` (two panes in distinct windows; only the reaped one is removed; shutdown NOT fired).
  - TS-3 `test_pane_exit_reap_removes_network_detached_pane` (pane in `Detached(NetworkDetach)`; reaped regardless of output target).
  - TS-4 `test_pane_exit_reap_is_idempotent` (double reap of the same pane is a safe no-op; shutdown not re-fired).
- TS-7 regression: full pre-existing mux suite (285 mux tests) plus the rest of the lib/integration suite all pass; CLI-only compile passes.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Detached last-pane reap drives shutdown: one session/window/pane, feed the pane id to the reap path | Pane removed, session gone, session manager empty, shutdown signal observed `true` | Integration |
| TS-2 | Detached non-last pane reap: two panes in distinct windows, reap one | Only that pane removed; shutdown signal NOT fired | Integration |
| TS-3 | Connection-reset race: pane switched to detached (network-detach), then reaped | Pane removed despite detached output target | Integration |
| TS-4 | Idempotent reap: reap the same pane id twice | Second call is a safe no-op; no panic; warn logged | Unit |
| TS-5 | Attached client teardown preserved: shell exit while attached | Empty exit chunk still delivered to the client (PtyExited / UI teardown) | Manual |
| TS-6 | Detached last-shell exit → daemon exits, on both run loops | Log shows pane reaped and "all sessions empty" shutdown; daemon process exits (Unix socket and Windows named pipe) | Manual |
| TS-7 | No regression: full mux test suite + CLI-only compile | All existing tests pass; `--no-default-features` compiles | Automated |
| TS-8 | Reader/channel review: steady-state output path untouched; pane-exit channel is distinct from the OSC-notification channel | Code review confirms `Ok(n)` path unchanged and a dedicated channel/type is used | Manual (review) |

## Code Quality Verification

- Format: per-file `rustfmt --edition 2021 <file>` — this is the project's
  formatter (the PostToolUse hook runs `rustfmt --edition 2021`), NOT crate-wide
  `cargo fmt`. The crate is `edition = "2024"`, so `cargo fmt` would reorder
  imports to the 2024 style and reformat ~43 unrelated, pre-existing files that
  the committed code never migrated — that is out of scope for this task and was
  reverted.
- Static analysis: `CARGO_TARGET_DIR=src-tauri/target cargo clippy --manifest-path src-tauri/Cargo.toml` (if used by the project; otherwise rely on `cargo check` warnings)

### Actual Results (implement phase)

- The 5 changed files were formatted with the project formatter
  (`rustfmt --edition 2021`); the diff is scoped to those files only (no
  unrelated reformatting).
- `cargo check` (default features): no warnings. `#[allow(clippy::too_many_arguments)]` added to `handle_create_window`, `handle_connection`, and `handle_cli_client` (each gained one argument).

## File Structure Verification

### Files to Modify
- [x] `src-tauri/src/mux/session/pane.rs` - added `PaneExitSender` + `SharedPaneExitSender` type aliases (distinct from the OSC `NotificationSender`, NFR4).
- [x] `src-tauri/src/mux/ipc/pty_spawn.rs` - plumbed `SharedPaneExitSender` into `register_pane_and_start_reader` → `pty_reader_loop`; in the `Ok(0)` EOF arm, kept the Connected empty-chunk teardown (FR3) and added an unconditional non-blocking `try_send(pane_id)` (FR1, M2). Steady-state `Ok(n)` path untouched (NFR3).
- [x] `src-tauri/src/mux/ipc/handlers.rs` - widened `handle_destroy_pane` to `pub(in crate::mux)` (FR2); threaded the sender through `handle_create_window`.
- [x] `src-tauri/src/mux/ipc/connection.rs` - threaded `SharedPaneExitSender` through `handle_connection` → `route_message` / `handle_cli_client` → `handle_create_window`.
- [x] `src-tauri/src/mux/daemon.rs` - added `run_pane_exit_task` (FR2/FR4/FR5/FR6); created the pane-exit channel + spawned the task + built `SharedPaneExitSender` on BOTH run loops (Unix socket + Windows named pipe, FR7); passed it into `handle_connection`. Added TS-1..TS-4 tests.

### Files Unchanged (intentionally)
- [x] `src-tauri/src/mux/ipc/reattach.rs` - detach skip-guard kept; the race it could cause is covered by the daemon reap (verified: not modified).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR7 implemented | TS-1..TS-8 + code review |
| SC-2 | Reap/shutdown tests pass | TS-1..TS-4 (automated) |
| SC-3 | Manual detach → Ctrl+D → daemon exits | TS-6 |
| SC-4 | No regression; CLI-only compiles | TS-7 |
| SC-5 | Identical on both run loops | TS-6 (both platforms) |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 reader notifies on EOF regardless of attach state | Phase 2 | TS-6 |
| FR2 daemon reap task = single authority | Phase 1 | TS-1, TS-2 |
| FR3 attached empty-chunk teardown preserved | Phase 2 | TS-5 |
| FR4 idempotent reap | Phase 1 | TS-4 |
| FR5 shutdown when all sessions empty | Phase 1 | TS-1 |
| FR6 connection-reset race not stranded | Phase 1 | TS-3 |
| FR7 wired on both run loops | Phase 2 | TS-6 |
| NFR1 no attach/detach divergence; serialized | Phase 1 | TS-1, TS-2, TS-3 |
| NFR2 no regression to kill/shutdown/tab-close | Phase 2 | TS-7 |
| NFR3 steady-state output path untouched | Phase 2 | TS-8 |
| NFR4 distinct channel from OSC notification | Phase 1 | TS-8 |

## Existing E2E Regression (implement Phase 3.8)

- `sdd.yaml` `e2e_test_command` is empty and the project ships no E2E framework for
  daemon-process-lifetime / detach-reattach scenarios (these require a running
  daemon and are manual). E2E regression skipped; covered by the manual items below
  and by the full automated suite under Test Verification.

## Manual Testing (E2E Not Possible)

E2E framework: none in this project. The detach/attach + daemon-process-lifetime
scenarios require a running daemon and are verified manually via the log file
(`~/.local/share/net.laser5.app.emterm/logs/emterm.log`); release builds persist
`warn` and higher, so reap/shutdown logs (`log::info!`) are visible only in a
debug/dev run — verify TS-5/TS-6 against a dev build or temporarily raise the log
level.

- [ ] TS-5: Attached shell exit tears the pane/tab down as before.
- [ ] TS-6 (Linux): Attach GUI, detach, exit the last shell (Ctrl+D) while
      detached; log shows the pane reaped and "all sessions empty, daemon
      shutting down"; the daemon process exits.
- [ ] TS-6 (Linux, non-last): With multiple detached panes, exit a non-last
      shell; only that pane is reaped and the daemon stays alive.
- [ ] TS-6 (Windows): Same detached last-shell-exit check on the Windows
      named-pipe daemon.
- [ ] TS-8: Review confirms the `Ok(n)` output path is unchanged and the pane-exit
      channel/type is distinct from the OSC-notification channel.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional (FR1–FR7) | 7 | FR2,FR4,FR5,FR6 | 0 | FR1,FR3,FR7 |
| Non-functional (NFR1–NFR4) | 4 | NFR1,NFR2 | 0 | NFR3,NFR4 |
| Test scenarios (TS-1..TS-8) | 8 | TS-1,TS-2,TS-3,TS-4,TS-7 | 0 | TS-5,TS-6,TS-8 |
