# Verification Document: mux Client + Tab Bar UI + Windows IME (Phase 4)

## Overview

**Feature**: mux-tabs-windows-ime
**SPEC.md**: `doc/tasks/mux-tabs-windows-ime/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-tabs-windows-ime/IMPLEMENTATION.md`

## Build Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
- **Expected**: exit 0, no errors. Forward-staged dead-code warnings allowed only if recorded in `sdd.yaml` notes (Phase 3 precedent).
- **Linux**: required on every sub-phase boundary.
- **Windows**: required at end of Phase 4-E; cross-build via `cargo build --workspace --target x86_64-pc-windows-msvc` or native build on a Windows host.

## Test Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**:
  - `native-poc`: +30 tests minimum (target: 1801 -> ≥ 1831).
  - `mux_ipc`: 100% preservation of pre-move tests (no test count regression vs `src-tauri/src/mux/ipc/` at Phase 4-A start).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-mux-1 | `crates/mux_ipc::protocol` after `git mv`: all preexisting `src-tauri/src/mux/ipc/protocol.rs` unit tests pass | Test count and pass/fail unchanged | Unit |
| TS-wire-1 | `native_poc::mux::wire` round-trips a `MuxMessage` via length-prefix + bincode without loss | Encode → decode returns equivalent message | Unit |
| TS-wire-2 | `native_poc::mux::wire` rejects frames larger than `MAX_FRAME_LENGTH` | Returns `WireError::FrameTooLarge` | Unit |
| TS-tab-1 | `tab_bar::draw` returns expected `TabEvent` for new/close/switch simulated input | Event matches input | Unit |
| TS-tab-2 | Closing the last tab emits `AppEvent::ExitWindow` | Event observed | Unit |
| TS-tab-3 | Tab in mux mode renders title with `[mux:<session>]` prefix | Title string matches expected prefix | Unit |
| TS-kb-1 | `keybinds::dispatch` maps `Ctrl+Shift+T/W/Tab/digit` correctly | Action enum matches table | Unit |
| TS-prefix-1 | Prefix state machine: single `Ctrl+B` arms; next valid key triggers action | Latch + action match table | Unit |
| TS-prefix-2 | Double prefix sends literal `0x02` to PTY | PTY writer receives `0x02` exactly once | Unit |
| TS-prefix-3 | Prefix latch timeout (3 s) cancels armed state | Subsequent keys are passthrough | Unit |
| TS-osc777-1 | Valid `OSC 777 ; emterm ; mux ; attach ; <socket> ; <id> ST` parses + validates | `MuxOscAction::Attach{socket,id}` returned | Unit |
| TS-osc777-2 | Detach OSC parses | `MuxOscAction::Detach` returned | Unit |
| TS-osc777-3 | Invalid socket path / session ID rejected | `None`/`Err`; warn log | Unit |
| TS-status-1 | `status_bar::draw` renders for representative `StatusUpdateMsg` | egui panel content matches expected | Unit |
| TS-status-2 | Status bar shows only clock when no mux state present | Panel does not show session/window strip | Unit |
| TS-status-3 | `statusbar.enabled = false` hides the panel | No panel inserted | Unit |
| TS-ime-1 | `Ime::Preedit(text)` updates preedit state | State holds sanitized text | Unit |
| TS-ime-2 | `Ime::Commit(text)` enqueues bytes to active PTY writer (mocked) | Exactly one write | Unit |
| TS-ime-3 | Preedit containing C0/C1 bytes is sanitized before rendering | Control chars stripped | Unit |
| TS-settings-1 | Missing `mux.prefix_key` / `statusbar.*` fields fall back to defaults | Settings struct holds defaults; no parse error | Unit |
| TS-mux-int-1 | Mock daemon round trip: connect → Hello → Snapshot → SelectWindow → Snapshot → Detach | All exchanges complete; grid state matches snapshots | Integration |
| TS-mux-int-2 | Connect to nonexistent socket | `ConnectError::Io(ENOENT)`; tab stays in native PTY mode | Integration |
| TS-mux-int-3 | Daemon-side abrupt close | Client observes channel close; falls back to native PTY | Integration |
| TS-mux-int-4 | Pause/resume: bytes during pause accumulate in 256 KB ring buffer; resume replays | No byte loss on detach | Integration |
| TS-perf-1 | Snapshot apply latency for 1 MB snapshot | < 200 ms on dev machine | Performance |
| TS-perf-2 | Prefix detect → daemon send round trip | < 5 ms | Performance |

## Code Quality Verification

- **Format**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all -- --check"` exits 0.
- **Static analysis**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings"` exits 0. Forward-staged dead-code may exist in `term_core` (preexisting) and is out of scope.

## File Structure Verification

### Files to Create

- `crates/mux_ipc/Cargo.toml` — crate manifest (Phase 4-A).
- `crates/mux_ipc/src/lib.rs` — module root, `pub mod protocol;` (Phase 4-A).
- `crates/mux_ipc/src/protocol.rs` — moved from `src-tauri/src/mux/ipc/protocol.rs` (Phase 4-A).
- `native-poc/src/mux/mod.rs` (Phase 4-C).
- `native-poc/src/mux/wire.rs` (Phase 4-C, sync length-prefix + bincode framing).
- `native-poc/src/mux/osc777.rs` (Phase 4-C).
- `native-poc/src/mux/client.rs` (Phase 4-C, blocking std UnixStream).
- `native-poc/src/mux/prefix.rs` (Phase 4-C).
- `native-poc/src/mux/mock.rs` (Phase 4-C, cfg(test)).
- `native-poc/src/ime/mod.rs` (Phase 4-E).
- `native-poc/src/ime/preedit.rs` (Phase 4-E).
- `native-poc/src/ime/commit.rs` (Phase 4-E).
- `native-poc/src/ui/status_bar.rs` (Phase 4-D).

### Files to Modify

- `Cargo.toml` (workspace root) — add `crates/mux_ipc` member.
- `src-tauri/Cargo.toml` — add `mux_ipc` path dep.
- `src-tauri/src/mux/ipc/protocol.rs` — replace with shim (`pub use mux_ipc::protocol::*;`).
- `src-tauri/src/mux/mod.rs` — only if internal references need adjustment (best-effort no-op).
- `native-poc/Cargo.toml` — add `mux_ipc` + `bincode` deps.
- `native-poc/src/app.rs` — wire tab bar / status bar / mux / IME events.
- `native-poc/src/tabs.rs` — extend `Tab` with `mux_client` and pause/ring buffer fields.
- `native-poc/src/callbacks.rs` — surface OSC 777 events.
- `native-poc/src/pty/*.rs` — honor `paused` flag.
- `native-poc/src/settings.rs` — add `mux.prefix_key`, `statusbar.{enabled,position}` fields.
- `native-poc/src/ui/tab_bar.rs` — full implementation (replace stub).
- `native-poc/src/ui/keybinds.rs` — full implementation (replace stub).
- `native-poc/src/ui/mod.rs` — export new widgets + AppAction.
- `native-poc/src/render/cursor.rs` — preedit overlay extension.
- `native-poc/README.md` — Phase 4 feature matrix (Phase 4-F).

### Files Moved

- `src-tauri/src/mux/ipc/protocol.rs` → `crates/mux_ipc/src/protocol.rs` (then re-create the original path as a 1-line shim).

`codec.rs` and `connection.rs` are intentionally NOT moved (server-only logic with tokio_util + session manager refs).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR13 implemented; all listed unit + integration tests pass | `cargo test --workspace` exit 0 + per-test mapping in this document |
| SC-2 | `cargo build --workspace` succeeds on Linux + Windows | Build commands above run on both platforms |
| SC-3 | `cargo test --workspace` exit 0 | Test command above |
| SC-4 | `cargo fmt --all -- --check` clean | Format command above |
| SC-5 | `cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings` zero errors | Static analysis above |
| SC-6 | Manual TS-manual-mux-1/2, TS-manual-ime-linux, TS-manual-ime-windows pass | Manual section below |
| SC-7 | 12 h Claude Code session under mux: no crash, no screen loss, RSS growth < 50 MB/hour | Manual TS-manual-soak |
| SC-8 | Legacy `src-tauri` build/test unaffected | Workspace build/test runs before & after each sub-phase |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (tab bar widget) | 4-B | TS-tab-1, TS-tab-2, TS-tab-3 (mux mode title prefix), manual smoke in 4-F |
| FR2 (tab keybinds) | 4-B | TS-kb-1, manual smoke in 4-F |
| FR3 (mux_ipc protocol extraction) | 4-A | TS-mux-1, full workspace build/test |
| FR4 (mux attach) | 4-C | TS-osc777-1, TS-osc777-3, TS-wire-1, TS-wire-2, TS-mux-int-1, TS-manual-mux-1 |
| FR5 (mux detach) | 4-C | TS-osc777-2, TS-prefix-1, TS-mux-int-1, TS-mux-int-4, TS-manual-mux-2 |
| FR6 (mux window switch) | 4-C | TS-prefix-1, TS-mux-int-1, TS-manual-mux-1 |
| FR7 (native PTY pause) | 4-C | TS-mux-int-4 |
| FR8 (prefix key handling) | 4-C | TS-prefix-1, TS-prefix-2, TS-prefix-3, TS-settings-1 |
| FR9 (status bar widget) | 4-D | TS-status-1, TS-status-2, TS-manual-mux-1 |
| FR10 (status bar settings) | 4-D | TS-status-3, TS-settings-1 |
| FR11 (IME preedit) | 4-E | TS-ime-1, TS-ime-3, TS-manual-ime-linux, TS-manual-ime-windows |
| FR12 (IME commit) | 4-E | TS-ime-2, TS-manual-ime-linux, TS-manual-ime-windows |
| FR13 (settings additions) | 4-C/4-D | TS-settings-1 |
| NFR1 (performance) | 4-F | TS-perf-1, TS-perf-2 |
| NFR2 (12 h stability) | 4-F | TS-manual-soak |
| NFR3 (Linux fcitx5 parity) | 4-E | TS-manual-ime-linux |
| NFR4 (workspace compat) | all | Build/test before & after each sub-phase |
| NFR5 (module layout) | all | File structure check above |
| NFR6 (logging) | 4-C/4-E | Manual log inspection during TS-manual-mux-1, soak run |

## E2E Testing

The project E2E framework (`./scripts/run-e2e-docker.sh`) targets the legacy Tauri build only. Native-poc does not have an automated E2E surface (chrome-devtools MCP does not support Tauri/WebKitGTK native windows).

- [ ] Legacy E2E (`./scripts/run-e2e-docker.sh test`) shows the same preexisting fail list as `main` (no Phase 4-introduced regressions). Treated as a regression check, not a gate.

## Manual Testing (E2E Not Possible)

- [ ] **TS-manual-mux-1**: launch native-poc, run `emterm mux new`, attach via OSC 777, confirm snapshot draws, switch windows with prefix `n/p/0-9`. Result captured in VERIFICATION_RESULT.md.
- [ ] **TS-manual-mux-2**: detach via `prefix d`, re-attach via `emterm mux attach`, confirm state is preserved.
- [ ] **TS-manual-ime-linux**: launch on Linux with fcitx5, type Japanese, verify preedit + commit (Phase 1 parity).
- [ ] **TS-manual-ime-windows**: launch on Windows with MS-IME, type Japanese, verify preedit + commit. Candidate position is best effort.
- [ ] **TS-manual-soak**: 12 h Claude Code session under mux. Sample RSS hourly (`ps -o rss= -p <pid>` snapshots). Record any crash / screen-loss event.

## Performance Verification

- TS-perf-1: snapshot apply for 1 MB scrollback < 200 ms on dev machine. Measured via `Instant::now()` around `term_core::reset_and_replay` in a benchmark test.
- TS-perf-2: prefix detect to daemon send < 5 ms. Measured by injecting timestamps in mock-daemon round trip.

## Security Verification

- [ ] OSC 777 socket path validation: only `/tmp/emterm-mux/` or `$XDG_RUNTIME_DIR/emterm-mux/` allowed. Negative test TS-osc777-3.
- [ ] OSC 777 session ID validation: `^[A-Za-z0-9_-]{1,64}$`. Negative test TS-osc777-3.
- [ ] IME preedit/commit sanitization: C0/C1 bytes dropped. TS-ime-3.
- [ ] Settings validation: unknown `statusbar.position` falls back to default with warn log. TS-status-3 / TS-settings-1.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit | 20 (TS-mux-1, TS-wire-1/2, TS-tab-1/2/3, TS-kb-1, TS-prefix-1/2/3, TS-osc777-1/2/3, TS-status-1/2/3, TS-ime-1/2/3, TS-settings-1) | 20 | 0 | 0 |
| Integration | 4 (TS-mux-int-1..4) | 4 | 0 | 0 |
| Performance | 2 (TS-perf-1, TS-perf-2) | 2 | 0 | 0 |
| Manual | 5 (TS-manual-mux-1/2, TS-manual-ime-linux/windows, TS-manual-soak) | 0 | 0 | 5 |
| Legacy regression | 1 (legacy E2E preexisting fail list) | 0 | 1 | 0 |
| **Total** | **32** | **26** | **1** | **5** |
