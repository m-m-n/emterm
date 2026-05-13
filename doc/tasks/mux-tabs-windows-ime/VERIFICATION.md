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

### Results (auto-scope, through Phase 4-E)

- `cargo build --workspace` → exit 0 (Linux/Docker).
- Phase 4-E build warning count: 12 (down from 15 baseline at Phase 4-D commit f14f7ba — preedit code removed two `#[allow(dead_code)]` previously needed elsewhere).
- Windows build is deferred to the manual host gate; the `cfg(windows)` compile-only smoke test in `ime::commit` covers Event::Ime variant shape.

## Test Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**:
  - `native-poc`: +30 tests minimum (target: 1801 -> ≥ 1831).
  - `mux_ipc`: 100% preservation of pre-move tests (no test count regression vs `src-tauri/src/mux/ipc/` at Phase 4-A start).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-mux-1 | `crates/mux_ipc::protocol` after `git mv`: all preexisting `src-tauri/src/mux/ipc/protocol.rs` unit tests pass | Test count and pass/fail unchanged | Unit |
| TS-apc-1 | `mux::apc::try_decode_emterm_mux` round-trips a well-formed `emterm-mux;<base64>` payload into a `MuxMessage` | Decoded message matches original | Unit |
| TS-apc-2 | Kitty Graphics + vendor-specific APC payloads return `None` | Image pipeline keeps seeing them | Unit |
| TS-apc-3 | Invalid base64 / truncated frame body → `None` + warn log | No panic, no false positive | Unit |
| TS-apc-4 | Empty / bare-prefix / non-UTF8 payloads → `None` | Boundary inputs handled | Unit |
| TS-mux-msg-1 | `App::on_mux_message` with `Snapshot` resets and replays via `term_core::reset_and_replay` | Grid contents reflect the payload | Unit (App-level integration) |
| TS-mux-msg-2 | `App::on_mux_message` with `StatusUpdate` caches the decoded `StatusUpdateMsg` on the target tab | `Tab::mux_status_state` populated | Unit (App-level integration) |
| TS-tab-1 | `tab_bar::draw` returns expected `TabEvent` for new/close/switch simulated input | Event matches input | Unit |
| TS-tab-2 | Closing the last tab emits `AppEvent::ExitWindow` | Event observed | Unit |
| TS-tab-3 | Tab in mux mode renders title with `[mux:<session>]` prefix | Title string matches expected prefix | Unit |
| TS-kb-1 | `keybinds::dispatch` maps `Ctrl+Shift+T/W/Tab/digit` correctly | Action enum matches table | Unit |
| TS-prefix-1 | Prefix state machine: single `Ctrl+B` arms; next valid key triggers action | Latch + action match table | Unit |
| TS-prefix-2 | Double prefix sends literal `0x02` to PTY | PTY writer receives `0x02` exactly once | Unit |
| TS-prefix-3 | Prefix latch timeout (3 s) cancels armed state | Subsequent keys are passthrough | Unit |
| TS-status-1 | `status_bar::draw` renders for representative `StatusUpdateMsg` | egui panel content matches expected | Unit |
| TS-status-2 | Status bar shows only clock when no mux state present | Panel does not show session/window strip | Unit |
| TS-status-3 | `statusbar.enabled = false` hides the panel | No panel inserted | Unit |
| TS-ime-1 | `Ime::Preedit(text)` updates preedit state | State holds sanitized text | Unit |
| TS-ime-2 | `Ime::Commit(text)` enqueues bytes to active PTY writer (mocked) | Exactly one write | Unit |
| TS-ime-3 | Preedit containing C0/C1 bytes is sanitized before rendering | Control chars stripped | Unit |
| TS-settings-1 | Missing `mux.prefix_key` / `statusbar.*` fields fall back to defaults | Settings struct holds defaults; no parse error | Unit |
| TS-perf-1 | Snapshot apply latency for 1 MB snapshot via `term_core::reset_and_replay` | < 200 ms on dev machine | Performance |

(Replaced by the redesign: TS-wire-1/2, TS-osc777-1/2/3, TS-mux-int-1..4,
TS-perf-2 targeted the direct UnixStream + mock daemon path that no longer
exists. Their concerns are covered by TS-apc-1..4 + TS-mux-msg-1/2 +
existing `mux_ipc::protocol` round-trip tests.)

## Code Quality Verification

- **Format**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all -- --check"` exits 0.
- **Static analysis**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings"` exits 0. Forward-staged dead-code may exist in `term_core` (preexisting) and is out of scope.

### Results (auto-scope, through Phase 4-E)

- `cargo fmt --all -- --check` → clean (exit 0).
- `cargo clippy -p emterm-native-poc -p mux_ipc --no-deps -- -D warnings` → identical 19-error baseline preserved (every error string matches Phase 4-D byte-for-byte; zero new clippy warnings introduced by Phase 4-E). The `--no-deps` flag is required because workspace clippy currently surfaces 24 preexisting term_core errors that are out of scope for this phase.
- Phase 4-E test count delta: +42 tests (1898 → 1940 workspace total).

## File Structure Verification

### Files to Create

- `crates/mux_ipc/Cargo.toml` — crate manifest (Phase 4-A).
- `crates/mux_ipc/src/lib.rs` — module root, `pub mod protocol;` (Phase 4-A).
- `crates/mux_ipc/src/protocol.rs` — moved from `src-tauri/src/mux/ipc/protocol.rs` (Phase 4-A).
- `native-poc/src/mux/mod.rs` (Phase 4-C).
- `native-poc/src/mux/apc.rs` (Phase 4-C redesign 2026-05-13 — APC payload decoder).
- `native-poc/src/mux/prefix.rs` (Phase 4-C; forward-staged under the redesign).

(Removed by the redesign: `wire.rs`, `client.rs`, `osc777.rs`, `mock.rs`,
`perf_tests.rs`. Tracked in Appendix A of VERIFICATION_RESULT.md.)
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

- [ ] **TS-manual-mux-1**: launch native-poc, run `emterm mux new` at the shell prompt, confirm APC-decoded `StatusUpdate` appears in the status bar, switch windows with `Ctrl+B n/p/<digit>` (the bytes go to the bridge CLI via PTY stdin; the daemon's reaction returns as APC `Snapshot` frames). Result captured in VERIFICATION_RESULT.md.
- [ ] **TS-manual-mux-2**: press `Ctrl+B d` (bridge CLI exits); confirm prompt returns. Re-run `emterm mux attach <id>` and confirm the prior screen state is restored from the snapshot.
- [ ] **TS-manual-ime-linux**: **N/A — tao 0.34 limitation.** tao 0.34 has no XIM integration, so fcitx5 / IBus on X11 / Wayland cannot deliver preedit / commit events to the native-poc window. The auto-scope `ime::*` wiring is verified by `TS-ime-1/2/3`; production use is gated on the WebView hybrid fallback (`tmp/restruct.md`) or a tao replacement.
- [ ] **TS-manual-ime-windows**: **N/A — tao 0.34 limitation.** tao 0.34 does not surface the IMM32 / TSF preedit text or expose `ImmSetCompositionWindow`. Same fallback trigger as Linux.
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
