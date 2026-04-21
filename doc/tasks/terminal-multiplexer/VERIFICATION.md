# Verification Document: Terminal Multiplexer

> **NOTE (post-cleanup)**: FR7 (Pane Layout) / FR10 (Copy Mode) / split / zoom 等、`doc/tasks/mux-feature-cleanup/` で削除された機能に関する検証項目は現行では無効。現行の検証範囲は `doc/tasks/terminal-multiplexer/SPEC.md` と `doc/tasks/mux-feature-cleanup/SPEC.md` を参照。

## Overview
**Feature**: Terminal Multiplexer
**SPEC.md**: `doc/tasks/terminal-multiplexer/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/terminal-multiplexer/IMPLEMENTATION.md`

## Build Verification
- Command: `bun tauri build`
- Expected: exit code 0, no errors
- WASM build: `cd wasm && wasm-pack build --target web`

## Test Verification

### Rust Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Coverage target: minimum 70%, target 85% for mux module

### TypeScript Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: minimum 70%, target 80% for mux module

### WASM Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo test"`
- Focus: snapshot serialization round-trips

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | IPC frame encoding/decoding round-trip | All 16 message types encode and decode correctly | Unit |
| TS-02 | Binary tree layout calculations (split, resize, remove) | Pixel bounds calculated correctly for all arrangements | Unit |
| TS-03 | Ring buffer write/read with overflow | Oldest data overwritten at 64MB cap, newest preserved | Unit |
| TS-04 | Snapshot serialization/deserialization | TerminalSnapshot round-trip preserves all persistent state, version header present | Unit |
| TS-04b | Snapshot version mismatch | Older snapshot version returns error, not crash | Unit |
| TS-05 | Socket path validation | Allowed dirs accepted, `../` and non-allowed rejected | Unit |
| TS-06 | Prefix key state machine | idle → prefix-active → dispatch transitions correct | Unit |
| TS-07 | tmux.conf parser | Supported directives parsed, unsupported produce warnings | Unit |
| TS-08 | Daemon startup and socket creation | Socket file exists, accepts connections | Integration |
| TS-09 | IPC handshake (Hello → Welcome/Rejected) | Correct version accepted, mismatched rejected | Integration |
| TS-10 | Full IPC message exchange | PtyInput → PtyOutput round-trip works | Integration |
| TS-11 | Session lifecycle | Create → use → destroy → daemon exit | Integration |
| TS-12 | Backpressure chain | Bounded channel saturation blocks PTY reader | Integration |
| TS-13 | Graceful shutdown (SIGTERM) | Socket cleaned, PTYs terminated, process exits | Integration |
| TS-14 | Stale socket detection and cleanup | Old socket file deleted, new one created | Integration |
| TS-15 | Daemon start → attach → type → detach → reattach → verify | Screen state fully restored after reattach | E2E |
| TS-16 | Pane split → resize → navigate → close | Layout updates correctly, pane operations work | E2E |
| TS-17 | Window create → switch → rename → close | Window operations reflected in tab group | E2E |
| TS-18 | Copy mode: enter → select → verify clipboard | Text selection copies to system clipboard | E2E |
| TS-19 | Daemon crash → GUI auto-recovery | GUI returns to normal mode with toast | E2E |
| TS-20 | Non-eMterm environment: emterm mux shows error | Error message, non-zero exit | Unit |
| TS-21 | Nesting: emterm mux inside mux session | Error message, non-zero exit | Unit |
| TS-22 | Minimum pane size enforcement | Split refused when pane would be < 2x10 | Unit |
| TS-23 | Ring buffer overflow during extended detach | Oldest data lost, newest 64MB preserved, screen recoverable | Integration |
| TS-24 | Snapshot deserialization failure | Empty screen reattach, no crash | Integration |
| TS-25 | Concurrent attach (second GUI evicts first) | First GUI disconnected, second takes over | Integration |
| TS-26 | Window resize during mux mode | Layout recalculated, per-pane Resize sent | Integration |
| TS-27 | High-throughput (seq benchmark) | No perceptible degradation vs normal mode | Performance |
| TS-28 | Multi-pane: high-throughput pane doesn't starve others | Other panes remain responsive | Performance |
| TS-29 | Reattach with 64MB delta | Replay completes under 2 seconds | Performance |

## Code Quality Verification
- Format (Rust): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Format (WASM): `cargo fmt --manifest-path wasm/Cargo.toml --check`
- Typecheck (TS): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`

## File Structure Verification

### Files to Create
- `src-tauri/src/mux/mod.rs` — Module root
- `src-tauri/src/mux/daemon.rs` — Daemon process entry point
- `src-tauri/src/mux/bridge.rs` — Tauri ↔ daemon bridge
- `src-tauri/src/mux/cli.rs` — CLI subcommands
- `src-tauri/src/mux/snapshot.rs` — Snapshot store
- `src-tauri/src/mux/ring_buffer.rs` — Per-pane circular buffer
- `src-tauri/src/mux/ipc/mod.rs` — IPC module root
- `src-tauri/src/mux/ipc/protocol.rs` — Message types and framing
- `src-tauri/src/mux/ipc/codec.rs` — LengthDelimited codec
- `src-tauri/src/mux/ipc/connection.rs` — Connection state machine
- `src-tauri/src/mux/session/mod.rs` — Session module root
- `src-tauri/src/mux/session/manager.rs` — Actor-model session manager
- `src-tauri/src/mux/session/session.rs` — Session state
- `src-tauri/src/mux/session/window.rs` — Window state
- `src-tauri/src/mux/session/pane.rs` — Pane state + PTY
- `src-tauri/src/mux/tmux_conf/mod.rs` — tmux.conf parser module
- `src-tauri/src/mux/tmux_conf/parser.rs` — Regex parser
- `src-tauri/src/mux/tmux_conf/converter.rs` — Directive mapper
- `src/terminal/mux/index.ts` — Mux client entry
- `src/terminal/mux/mux-client.ts` — IPC client
- `src/terminal/mux/layout.ts` — Binary tree layout
- `src/terminal/mux/pane-manager.ts` — Per-pane lifecycle
- `src/terminal/mux/pane-border.ts` — Border + drag-resize
- `src/terminal/mux/prefix-key.ts` — Prefix key handler
- `src/terminal/mux/tab-group.ts` — Tab group UI
- `src/terminal/mux/status-bar.ts` — Status bar component
- `src/terminal/mux-copy-mode/index.ts` — Copy mode manager
- `src/terminal/mux-copy-mode/vi-keybinds.ts` — Vi handler
- `src/terminal/mux-copy-mode/emacs-keybinds.ts` — Emacs handler

### Files to Modify
- `src-tauri/Cargo.toml` — Add tokio-util, interprocess, bincode
- `src-tauri/src/main.rs` — Add mux subcommand
- `src-tauri/src/lib.rs` — Declare mux module
- `src-tauri/src/pty/session.rs` — Add TERM_PROGRAM env vars
- `src-tauri/src/tauri_commands.rs` — Add mux bridge commands
- `src-tauri/src/commands/config/settings.rs` — Add MuxSettings
- `src-tauri/src/commands/config/types.rs` — Add mux enums
- `wasm/Cargo.toml` — Add serde dependency
- `wasm/src/terminal_core.rs` — Add serde derives, snapshot methods
- `wasm/src/cell.rs` — Add serde derives
- `wasm/src/parser.rs` — Add serde derives
- `wasm/src/ring_buffer.rs` — Add serialization support
- `src/terminal-app/osc-handler.ts` — Add mux OSC handlers
- `src/terminal-app/index.ts` — Add mux mode state
- `src/terminal-app/handlers/keyboard.ts` — Prefix key interception
- `src/terminal/wasm/terminal-core.ts` — Snapshot/restore, search wrappers
- `src/terminal/canvas-renderer.ts` — Copy mode selection highlight
- `src/tab-bar/tab-manager.ts` — Tab group support
- `src/tab-bar/tab-bar-ui.ts` — Tab group rendering, sub-tabs
- `src/tab-bar/types.ts` — Add MuxTab type
- `src/settings/types.ts` — Add mux settings section
- `doc/UI-DESIGN-GUIDELINES.yaml` — Status bar, pane border specs

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All functional requirements (FR1-FR15) implemented | Feature checklist per FR |
| SC-02 | All test scenarios pass | Test suite execution |
| SC-03 | No degradation vs normal mode | seq benchmark comparison |
| SC-04 | Security: socket path validation | Unit tests + manual path traversal attempts |
| SC-05 | Linux and Windows support | CI/CD on both platforms |
| SC-06 | tmux keybindings work | E2E tests for each keybinding |
| SC-07 | OSC extensions work in mux mode | E2E test with markdown/image in mux pane |
| SC-08 | Code review completed | PR review |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Daemon Process | Phase 1 | Integration test: daemon start/stop/lifecycle |
| FR2: IPC Protocol | Phase 1 | Unit test: frame round-trip for all 16 message types |
| FR3: Session Management | Phase 1 | Integration test: session CRUD + cascade |
| FR4: OSC Signaling | Phase 2 | Integration test: CLI → OSC → GUI mode switch |
| FR5: GUI Mode Switching | Phase 2 | E2E: normal → mux → normal transitions |
| FR6: Detach/Reattach | Phase 2 | E2E: detach → reattach → verify screen state |
| FR7: Pane Layout | Phase 3 | Unit test: binary tree calculations + E2E: split/resize |
| FR8: Window Management | Phase 4 | E2E: window create/switch/rename/close |
| FR9: Status Bar | Phase 5 | Manual: visual verification + integration: event updates |
| FR10: Copy Mode | Phase 6 | E2E: enter → select → clipboard verify |
| FR11: tmux.conf Conversion | Phase 7 | Unit test: parser + converter with sample configs |
| FR12: Prefix Key | Phase 3 | Unit test: state machine transitions |
| FR13: Flow Control | Phase 1 | Integration test: backpressure under high throughput |
| FR14: Environment Variables | Phase 0 | Unit test: TERM_PROGRAM set in PTY |
| FR15: WASM Instance Management | Phase 2 | Integration: per-pane WASM creation/destruction |
| NFR1: Performance | Phase 1-2 | Performance test: seq benchmark in mux mode |
| NFR2: Security | Phase 2 | Unit test: socket path validation |
| NFR3: Reliability | Phase 2 | E2E: daemon crash → GUI recovery |
| NFR4: Compatibility | Phase 1 | CI: Linux + Windows builds |
| NFR5: Resource Usage | Phase 1-2 | Unit test: ring buffer cap, frame size limit |

## E2E Testing (Docker)

Uses existing `./scripts/run-e2e-docker.sh` infrastructure.

- [ ] Start `emterm mux` → verify mux mode active → type command → see output
- [ ] Detach (prefix+d) → verify normal mode → reattach → verify restored state
- [ ] Split pane (prefix+%) → type in both panes → close one → verify layout
- [ ] Create window (prefix+c) → switch (prefix+n) → rename (prefix+,) → verify tabs
- [ ] Daemon crash → verify GUI auto-recovery to normal mode
- [ ] High-throughput output (seq) in mux pane → verify no UI jitter
- [ ] OSC Markdown display in mux pane → verify rendering
- [ ] Non-eMterm environment → `emterm mux` shows error
- [ ] Existing E2E tests pass without regression

## Manual Testing (E2E Not Possible)

- [ ] Visual: pane border appearance (1px, theme color, accent on active)
- [ ] Visual: status bar appearance and position (top/bottom)
- [ ] Visual: tab group expand/compact animation (0.3s timing)
- [ ] Visual: drag-resize cursor change and smooth resizing
- [ ] Visual: copy mode selection highlight rendering
- [ ] UX: prefix key response feel (no perceptible delay)
- [ ] UX: typing latency in mux mode vs normal mode (subjective comparison)

## Performance Verification

| Metric | Expected | How to Measure |
|--------|----------|----------------|
| Key-to-echo latency | Same as normal mode | Subjective comparison + optional instrumentation |
| seq 1 1000000 throughput | No degradation | Time comparison: normal mode vs mux mode |
| Multi-pane starvation | No starvation | High-throughput in one pane, verify others remain responsive |
| Reattach time (64MB delta) | < 2 seconds | Time from attach command to screen fully rendered |
| WASM snapshot size (typical) | < 5MB | Measure snapshot_to_bytes() output for 200-row, 10k scrollback |

## Security Verification

- [ ] Socket path validation: only allowed directories accepted
- [ ] Path traversal: `../` in socket path rejected
- [ ] Protocol version mismatch: connection rejected with clear error
- [ ] No sensitive data in IPC (raw PTY bytes only, no credentials in protocol)
- [ ] Socket file permissions: restricted to owner (Unix)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit Tests | 12 | 12 | 0 | 0 |
| Integration Tests | 10 | 10 | 0 | 0 |
| E2E Tests | 9 | 0 | 9 | 0 |
| Manual Tests | 7 | 0 | 0 | 7 |
| Performance | 5 | 2 | 1 | 2 |
| Security | 5 | 4 | 0 | 1 |
| **Total** | **51** | **31** | **10** | **10** |
