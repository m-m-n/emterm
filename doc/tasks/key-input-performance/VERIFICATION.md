# Verification Document: Key Input Performance Optimization

## Overview
**Feature**: key-input-performance
**SPEC.md**: `doc/tasks/key-input-performance/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/key-input-performance/IMPLEMENTATION.md`

## Build Verification

### Rust Backend
- Command: `cargo test --manifest-path src-tauri/Cargo.toml` (via Docker)
- Expected: exit code 0, no errors, no warnings

### TypeScript Frontend
- Command: `bun test` (via Docker)
- Expected: exit code 0, all tests pass

### Type Check
- Command: `bun run typecheck` (via Docker)
- Expected: exit code 0, no type errors

### Full Build
- Command: `bun tauri build`
- Expected: exit code 0, application binary produced

## Test Verification

### Rust Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Coverage target: minimum 80%, target 90% for new code

### TypeScript Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: minimum 80%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | WriterRegistry lookup for valid session ID | Returns sender handle | Unit |
| TS-02 | WriterRegistry lookup for invalid session ID | Returns not-found error | Unit |
| TS-03 | Writer thread receives data and writes to PTY | Data appears in PTY output | Integration |
| TS-04 | Writer thread exits on channel close | Thread terminates cleanly | Unit |
| TS-05 | Session create → write → close lifecycle | Full lifecycle completes without errors | Integration |
| TS-06 | Multiple concurrent sessions write independently | Each session receives its own data | Integration |
| TS-07 | PtyClient.write() sends correct byte sequence | Backend receives expected bytes | Unit (TS) |
| TS-08 | Key repeat at OS maximum rate | No dropped keystrokes | Manual |
| TS-09 | IME composition and commit | Japanese input works correctly | Manual |
| TS-10 | All keybindings (copy, paste, search, tabs) | All shortcuts function correctly | Manual |

## Code Quality Verification

### Rust
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

### TypeScript
- Type check: `bun run typecheck`

## File Structure Verification

### Files to Create
- `src-tauri/src/pty/writer.rs` - Writer channel registry and writer thread

### Files to Modify
- `src-tauri/src/pty/mod.rs` - Add writer module
- `src-tauri/src/pty/session.rs` - Writer handle extraction method
- `src-tauri/src/pty/manager.rs` - WriterRegistry integration
- `src-tauri/src/lib.rs` - Simplified pty_write command
- `src/pty/client.ts` - Serialization optimization

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | Key repeat speed perceptually equivalent to WezTerm/Alacritty | Manual side-by-side comparison with identical OS key repeat settings |
| SC-02 | Single-key latency not degraded | Manual interactive typing test; no perceivable delay increase |
| SC-03 | IME (Japanese input) works correctly | Manual test: EditContext mode + textarea fallback mode |
| SC-04 | All existing keybindings and shortcuts work | Manual test: copy, paste, search, tab management, prompt jump |
| SC-05 | All existing tests pass | Automated: Rust + TypeScript test suites |
| SC-06 | No regressions in terminal functionality | Manual test: basic shell operations, vim, htop |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Replace JSON-based invoke IPC with lower-overhead mechanism | Phase 1 | pty_write command simplified to sync with channel send |
| FR2: Eliminate Uint8Array-to-number[] conversion | Phase 2 | PtyClient.write() code review; no Array.from() in hot path |
| FR3: Reduce lock acquisition overhead | Phase 1 | pty_write acquires at most 1 read lock (code review + test) |
| FR4: Maintain fire-and-forget semantics | Phase 1, 2 | KeyboardHandler still uses fire-and-forget write pattern |
| FR5: Preserve IME input path | Phase 3 | Manual IME test: composition, conversion, commit |
| NFR1: Key repeat throughput comparable | Phase 3 | Manual comparison with WezTerm/Alacritty |
| NFR2: Single-key latency not increased | Phase 3 | Manual interactive typing test |
| NFR3: Minimize CPU usage | Phase 3 | Writer thread blocks on receive (no busy-wait) |
| NFR4: All keybindings work | Phase 3 | Manual test of all configured shortcuts |
| NFR5: No test regressions | Phase 1, 2 | Automated test suites pass |

## Manual Testing (E2E Not Possible)

### Performance Comparison
- [ ] Hold 'a' key for 3 seconds in eMterm and WezTerm side-by-side
- [ ] Compare character count output (should be within 10% of each other)
- [ ] Hold arrow key in vim/nano and verify smooth cursor movement
- [ ] Hold BackSpace to delete a long line and verify speed

### IME Compatibility
- [ ] Enable Japanese IME (fcitx5/ibus/SKK)
- [ ] Type Japanese text with conversion and commit
- [ ] Verify EditContext API mode works (Chromium-based WebView)
- [ ] Verify Ctrl+J is still blocked (SKK compatibility)

### Input Isolation
- [ ] Open multiple tabs, verify input goes only to active tab
- [ ] Switch tabs during key repeat, verify clean transition
- [ ] Close tab during key repeat, verify no crash

### Keybinding Verification
- [ ] Ctrl+Shift+C (copy with selection)
- [ ] Ctrl+Shift+V (paste)
- [ ] Ctrl+Shift+F (search)
- [ ] Ctrl+Shift+T (new tab)
- [ ] Ctrl+Shift+Arrow (prompt jump)
- [ ] Ctrl+C, Ctrl+D, Ctrl+Z (terminal signals)
- [ ] Arrow keys in normal and application cursor mode

### Edge Cases
- [ ] Rapid alternating keys (e.g., 'j' and 'k' in vim)
- [ ] Large paste operation (>1000 bytes) still works with chunking
- [ ] Session close during active write (no hang or crash)

## Performance Verification

| Metric | Expected | How to Measure |
|--------|----------|----------------|
| Key repeat throughput | Within 10% of WezTerm | Manual: character count over 3-second hold |
| Single-key latency | No perceivable increase | Manual: interactive typing feel |
| Lock acquisitions per write | ≤ 1 read lock | Code review of pty_write handler |
| Writer thread CPU usage | ~0% when idle | System monitor during idle terminal |

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 4 | 4 | 0 |
| Unit Tests | 4 | 4 | 0 |
| Integration Tests | 3 | 3 | 0 |
| Performance | 4 | 0 | 4 |
| IME Compatibility | 4 | 0 | 4 |
| Keybindings | 8 | 0 | 8 |
| Edge Cases | 4 | 0 | 4 |
| **Total** | **31** | **11** | **20** |
