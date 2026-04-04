# Verification Document: Windows Mux Support

## Overview
**Feature**: Windows Mux Support
**SPEC.md**: `doc/tasks/windows-mux-support/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/windows-mux-support/IMPLEMENTATION.md`

## Build Verification
- Command: `cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors on both Linux and Windows targets
- Cross-check: `cargo build --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` (if cross-compilation available)

## Test Verification
- Command: `cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: minimum 70%, target 80% for new code

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | pipe_path() returns valid Named Pipe path | `\\.\pipe\emterm-mux-default` | Unit |
| TS-02 | is_daemon_running() with no daemon | Returns false | Unit |
| TS-03 | Console mode save/restore round-trip | Original mode preserved | Unit |
| TS-04 | Daemon starts and listens on Named Pipe | Accepts client connection | Integration |
| TS-05 | Bridge connects and exchanges handshake | Hello/Welcome messages | Integration |
| TS-06 | CLI ls command returns session list | Formatted output to stdout | Integration |
| TS-07 | CLI kill command terminates daemon | Daemon process exits | Integration |
| TS-08 | CLI new-window creates window | Window appears in session | Integration |
| TS-09 | CLI switch-window changes active window | Active window ID changes | Integration |
| TS-10 | Detach and reattach restores session | Buffered output replayed | Integration |
| TS-11 | Daemon survives terminal closure | Process continues after parent exit | Integration |
| TS-12 | Multiple simultaneous clients | All clients served | Integration |
| TS-13 | Stale pipe detection and recovery | New daemon starts successfully | Integration |
| TS-14 | Non-console stdin (piped input) | Graceful error or fallback | Edge case |
| TS-15 | Named Pipe access from other user | Connection rejected | Security |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

## File Structure Verification
### Files to Create
- None (all changes are in existing files)

### Files to Modify
- `src-tauri/Cargo.toml` — windows-sys dependency added
- `src-tauri/src/mux/ipc/mod.rs` — cfg gates updated
- `src-tauri/src/mux/ipc/connection.rs` — Stream type generified
- `src-tauri/src/mux/ipc/handlers.rs` — Stream type generified
- `src-tauri/src/mux/ipc/reattach.rs` — Stream type generified
- `src-tauri/src/mux/daemon.rs` — Windows Named Pipe daemon added
- `src-tauri/src/mux/bridge.rs` — Windows bridge with Console API added
- `src-tauri/src/mux/cli.rs` — Windows CLI stubs replaced with implementations

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All FR1-FR7 implemented | Code review: each FR has corresponding #[cfg(windows)] implementation |
| SC-02 | All unit and integration tests pass on Windows | Windows CI: `cargo test` green |
| SC-03 | Existing Unix tests pass without regression | Linux CI: `cargo test` green |
| SC-04 | Daemon survives terminal closure | TS-11: Start daemon, close terminal, verify daemon process |
| SC-05 | GitHub Actions CI passes for both platforms | CI status check |
| SC-06 | No new unsafe code beyond Console API FFI | Code review: grep for `unsafe` blocks |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — Named Pipe daemon | Phase 2 | TS-04: Daemon accepts connections |
| FR2 — Daemon process detachment | Phase 2 | TS-11: Daemon survives terminal closure |
| FR3 — Bridge connection | Phase 3 | TS-05: Bridge handshake succeeds |
| FR4 — Session management CLI | Phase 4 | TS-06, TS-07: ls and kill work |
| FR5 — Window operations CLI | Phase 4 | TS-08, TS-09: new-window and switch-window work |
| FR6 — Reattach/detach | Phase 5 | TS-10: Buffered output replayed on reattach |
| FR7 — Stale pipe detection | Phase 2 | TS-13: Stale pipe detected and cleaned up |

## E2E Testing (Docker)
- [ ] Existing Linux E2E tests pass without regression (`./scripts/run-e2e-docker.sh`)

## Manual Testing (E2E Not Possible)
- [ ] Start mux session on Windows (`emterm mux`), verify shell prompt appears
- [ ] Type commands in mux session, verify output is correct
- [ ] Close terminal window, verify daemon still running (Task Manager)
- [ ] Reopen terminal, run `emterm mux`, verify session restored with previous output
- [ ] Run `emterm mux new-window`, verify new window created
- [ ] Run `emterm mux switch-window`, verify window switch
- [ ] Run `emterm mux ls`, verify session/window list displayed
- [ ] Run `emterm mux kill`, verify daemon terminated
- [ ] Try `emterm mux` commands when daemon not running, verify descriptive error messages

## Security Verification
- [ ] Named Pipe created with PIPE_REJECT_REMOTE_CLIENTS
- [ ] Other user accounts cannot connect to the pipe (TS-15)
- [ ] No sensitive data (credentials, tokens) in Named Pipe path or log files
- [ ] `unsafe` blocks limited to Console API FFI calls, each with safety comment

## Verification Summary
| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit Tests | 3 | 3 | 0 | 0 |
| Integration Tests | 10 | 10 | 0 | 0 |
| Code Quality | 2 | 2 | 0 | 0 |
| Regression | 2 | 1 | 1 | 0 |
| Security | 4 | 1 | 0 | 3 |
| Manual E2E | 9 | 0 | 0 | 9 |
| **Total** | **32** | **19** | **1** | **12** |
