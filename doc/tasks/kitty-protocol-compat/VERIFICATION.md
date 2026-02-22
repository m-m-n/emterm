# Verification Document: Kitty Protocol Compatibility

## Overview
**Feature**: Kitty Protocol Compatibility
**SPEC.md**: `doc/tasks/kitty-protocol-compat/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/kitty-protocol-compat/IMPLEMENTATION.md`

## Build Verification

### WASM Build
- Command: `cd wasm && wasm-pack build --target web --out-dir pkg`
- Expected: exit code 0, no errors

### TypeScript Build
- Command: `bun run build`
- Expected: exit code 0, no errors

### Rust Backend Build
- Command: `cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

### WASM Tests
- Command: `cargo test --manifest-path wasm/Cargo.toml`
- Coverage target: minimum 80%, target 90% for modified files

### TypeScript Tests
- Command: `bun test`
- Expected: All existing tests pass, new tests for cell size propagation pass

### Rust Backend Tests
- Command: `cargo test --manifest-path src-tauri/Cargo.toml`
- Expected: All existing tests pass

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Kitty query with image ID | Response: `ESC_Gi=31;OK ESC\` | Unit (WASM) |
| TS-02 | Kitty query without image ID | Response: `ESC_G;OK ESC\` | Unit (WASM) |
| TS-03 | Kitty query with quiet=1 | No response generated | Unit (WASM) |
| TS-04 | Non-query Kitty APC | Passes through to APC callback | Unit (WASM) |
| TS-05 | CSI 16t cell size | Response: `ESC[6;<h>;<w>t` with actual cell dims | Unit (WASM) |
| TS-06 | CSI 14t text area pixels | Response: `ESC[4;<rows*h>;<cols*w>t` | Unit (WASM) |
| TS-07 | CSI 18t text area chars | Response: `ESC[8;<rows>;<cols>t` | Unit (WASM) |
| TS-08 | Cell size defaults to 8x16 | Default cell size used when not set | Unit (WASM) |
| TS-09 | syncModesFromWasm reads mode bits | TS modes match WASM mode bits | Unit (TS) |
| TS-10 | set_cell_size_px on init | Called with measured character size | Unit (TS) |
| TS-11 | set_cell_size_px on resize | Called with updated character size | Unit (TS) |
| TS-12 | Cell size on buffer switch | Alternate core gets correct cell size | Unit (TS) |
| TS-13 | Capability detection sequence | All 4 responses in correct order | Integration |
| TS-14 | Buffer switch cell size | Alternate core CSI 16t returns actual values | Integration |
| TS-15 | Kitty image data routing | a=T APC reaches Rust image handler | Integration |

## Code Quality Verification

### WASM
- Format: `cargo fmt --manifest-path wasm/Cargo.toml`
- Expected: No formatting changes

### TypeScript
- Typecheck: `bun run typecheck`
- Expected: No type errors

### Rust Backend
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- Expected: No formatting changes

## File Structure Verification

### Files Modified (Phase 2)
- `src/terminal/state.ts` - Cell size propagation in switchToAlternateBuffer
- `src/terminal-app/index.ts` - Cell size set on core change in data handler

### Files Verified (Phase 3)
- `src-tauri/src/image/kitty.rs` - Animation frame/control handling
- `src-tauri/src/image/animation.rs` - Animation state management

### Files Already Complete (Phase 1)
- `wasm/src/apc_handler.rs` - Kitty query synchronous response
- `wasm/src/csi_device.rs` - XTWINOPS handlers
- `wasm/src/csi_dispatch.rs` - CSI 't' dispatch
- `wasm/src/terminal_core.rs` - cell_size_px fields

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All FR1-FR5 implemented and tested | Run all test suites; check test scenario coverage |
| SC-02 | treemd displays without red background | Manual: `treemd README.md` in eMterm |
| SC-03 | kitten icat displays images | Manual: `kitten icat image.png` in eMterm |
| SC-04 | All existing tests pass | Run WASM (448+), TS (1909+), Rust all |
| SC-05 | No regression in emterm image | Manual: `emterm image image.png` |
| SC-06 | Kitty animation frames handled | Code review of handle_frame/handle_animate |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Synchronous Kitty query response | Phase 1 (done) | TS-01 through TS-04, TS-13 |
| FR2: XTWINOPS device responses | Phase 1 (done) | TS-05 through TS-08 |
| FR3: Cell size sync on buffer switch | Phase 2 | TS-12, TS-14 |
| FR4: Kitty image pipeline compatibility | Phase 2 | TS-15, SC-03, manual kitten icat test |
| FR5: Kitty animation frame support | Phase 3 | SC-06, code review + manual test |

## Manual Testing (E2E)

### treemd Rendering
- [ ] Run `treemd README.md` in eMterm
- [ ] Verify: No "No interactive elements" warning
- [ ] Verify: No red background
- [ ] Verify: Content renders correctly with formatting

### Kitty Image Display
- [ ] Run `kitten icat <test-image.png>` in eMterm
- [ ] Verify: Image displays in viewer overlay
- [ ] Verify: Image dimensions are correct
- [ ] Verify: Viewer closes cleanly with Escape key

### Existing Functionality Regression
- [ ] Run `emterm image <test-image.png>`
- [ ] Verify: Image displays in viewer overlay (same as before)
- [ ] Verify: No visual artifacts or timing issues

### Kitty Animation
- [ ] Test with treemd Kitty-mode animation content (if available)
- [ ] Verify: Animation frames update correctly

## Performance Verification

- **NFR1 — Response timing**: All device responses (Kitty query, DA1, CSI 16t, DSR) must arrive within 2000ms
  - Verification: ratatui-image's detection timeout is 2000ms; if tools work, timing is met
- **NFR3 — No latency impact**: Synchronous query handling must not add measurable latency
  - Verification: Kitty query handler operates on stack buffer (no allocation), response is < 32 bytes

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 3 | 3 | 0 |
| Unit Tests (WASM) | 8 | 8 | 0 |
| Unit Tests (TS) | 4 | 4 | 0 |
| Integration Tests | 3 | 3 | 0 |
| Code Quality | 3 | 3 | 0 |
| E2E / Manual | 4 | 0 | 4 |
| Performance | 2 | 0 | 2 |
| **Total** | **27** | **21** | **6** |
