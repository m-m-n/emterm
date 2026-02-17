# Verification Document: WASM Renderer Zero-Copy + Carry-over (Sprint 7)

## Overview

**Feature**: WASM Renderer Zero-Copy + Carry-over
**SPEC.md**: `doc/tasks/wasm-renderer-zero-copy/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-renderer-zero-copy/IMPLEMENTATION.md`

## Build Verification

### TypeScript (main)

- Command: `bun run build:wasm && bun build src/index.html --outdir dist --minify`
- Expected: Exit code 0, no errors

### Rust (backend)

- Command: `cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: Exit code 0, no warnings on new code

### WASM

- Command: `cd wasm && wasm-pack build --target web --out-dir pkg`
- Expected: Exit code 0 (no WASM source changes expected)

## Test Verification

### TypeScript Tests

- Command: `bun test`
- Coverage target: minimum 80%, target 90%+ on new code

### Rust Tests

- Command: `cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: All existing + new tests pass

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | groupPackedCellsIntoSpans produces identical spans to groupCellsIntoSpans for same cell data | TextSpan arrays are deeply equal | Unit |
| TS-02 | groupPackedCellsIntoSpans handles empty row (all default cells) | Returns spans with default attributes | Unit |
| TS-03 | groupPackedCellsIntoSpans handles wide characters (width=2 + placeholder width=0) | Wide char occupies 2 cells, placeholder skipped | Unit |
| TS-04 | groupPackedCellsIntoSpans handles combining marks (width=0 with character) | Mark merged into previous cell's character | Unit |
| TS-05 | groupPackedCellsIntoSpans handles overflow characters (char_len=0xFF) | Character correctly decoded from overflow format | Unit |
| TS-06 | groupPackedCellsIntoSpans handles truncated packed data safely | Parsing stops without crash, partial spans returned | Unit |
| TS-07 | groupPackedCellsIntoSpans correctly groups consecutive cells with same attributes | Single span for consecutive matching cells | Unit |
| TS-08 | groupPackedCellsIntoSpans splits spans at attribute boundaries | New span starts when attributes differ | Unit |
| TS-09 | WasmLineProxy.dirty getter returns WASM core dirty state | true after setCell, false after clear_dirty | Unit |
| TS-10 | WasmLineProxy.markDirty() sets WASM core dirty bit | dirty getter returns true after markDirty | Unit |
| TS-11 | WasmLineProxy.clearDirty() is a no-op | dirty state unchanged (still managed by core.clear_dirty) | Unit |
| TS-12 | CanvasRenderer.render() uses packed path when WASM core available | renderLinePacked called instead of renderLine | Integration |
| TS-13 | CanvasRenderer.forceRender() uses packed path for viewport + scrollback | Packed data fetched for all visible rows | Integration |
| TS-14 | Fallback to LineAccessor path when WASM unavailable | Existing renderLine path used | Integration |
| TS-15 | All existing TS tests pass (1824+ tests) | No regressions | Regression |
| TS-16 | generate_kitty_sequence returns unique image_id on each call | Sequential calls produce different ids | Unit (Rust) |
| TS-17 | image_id skips 0 on wrap-around | Counter increments past 0 | Unit (Rust) |
| TS-18 | wait_for_kitty_response correctly parses response with matching id | Returns Ok | Unit (Rust) |
| TS-19 | wait_for_kitty_response handles mismatched id gracefully | Continues waiting or returns error | Unit (Rust) |
| TS-20 | All existing Rust tests pass (398+ WASM, 362+ backend) | No regressions | Regression |

## Code Quality Verification

- Format (Rust): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Type check (TypeScript): `bun run typecheck`

## File Structure Verification

### Files to Modify

- `src/terminal/canvas-renderer.ts` — groupPackedCellsIntoSpans, packedAttrsEqual, unpackAttrsFromBinary, renderLinePacked, packed path in render() and forceRender()
- `src/terminal/canvas-renderer.test.ts` — Packed parser tests, packed rendering integration tests
- `src/terminal/unified-buffer.ts` — getRowPacked, getScrollbackRowPacked methods
- `src/terminal/state.ts` — getRowPacked, getScrollbackRowPacked delegation
- `src/terminal/wasm/terminal-core.ts` — WasmLineProxy dirty getter delegation
- `src/terminal/wasm/__tests__/terminal-core.test.ts` — Dirty delegation tests
- `src-tauri/src/protocols/kitty.rs` — AtomicU32 counter, return (String, u32) tuple
- `src-tauri/src/commands/image.rs` — Pass image_id to response parser, parse id from response

### Files NOT Modified (Verification)

- `wasm/src/**` — No WASM source changes in this sprint
- `src/terminal/grid.ts` — Line/Cell types unchanged
- `src/terminal/attributes.ts` — CellAttributes type unchanged

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All FR1-FR10 implemented and tested | Test suite covers each FR, all pass |
| SC-02 | Dirty row rendering within 2ms (NFR1) | Performance benchmark test |
| SC-03 | Zero intermediate object allocation (NFR2) | Code review: no Cell/Line/CellAttributes constructors in packed path |
| SC-04 | WASM binary under 80KB (NFR3) | Check wasm binary size (no WASM changes expected) |
| SC-05 | All existing tests pass (NFR4) | Full test suite: bun test + cargo test |
| SC-06 | Packed data parsing bounds safety (NFR5) | Truncated data test cases |
| SC-07 | Concurrent emterm image commands work correctly | Manual test with parallel image commands |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: groupPackedCellsIntoSpans function | Phase 1 | TS-01 through TS-08 |
| FR2: renderLinePacked method | Phase 2 | TS-12, TS-13 |
| FR3: render() packed path for dirty rows | Phase 2 | TS-12 |
| FR4: forceRender() packed path for all visible rows | Phase 2 | TS-13 |
| FR5: getVisibleLinesPacked for scrollback | Phase 2 | TS-13 |
| FR6: WasmLineProxy dirty getter delegation | Phase 3 | TS-09 |
| FR7: WasmLineProxy clearDirty no-op | Phase 3 | TS-11 |
| FR8: Kitty AtomicU32 image_id generation | Phase 4 | TS-16, TS-17 |
| FR9: Kitty response image_id correlation | Phase 4 | TS-18, TS-19 |
| FR10: JS fallback rendering path preserved | Phase 2 | TS-14 |

## E2E Testing (Docker)

- [ ] Full TypeScript test suite passes: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- [ ] Full Rust test suite passes: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- [ ] TypeScript type check passes: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- [ ] Rust format check passes: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml --check"`

## Manual Testing (E2E Not Possible)

- [ ] Run `emterm` and verify terminal renders correctly with packed path (visual inspection)
- [ ] Run `cat large_file.txt` and verify smooth rendering (no visual artifacts)
- [ ] Scroll back through terminal history and verify scrollback renders correctly
- [ ] Run `emterm image <file>` and verify single image displays correctly
- [ ] Run two `emterm image` commands concurrently and verify both complete without interference
- [ ] Verify WASM initialization failure falls back to JS rendering path gracefully

## Performance Verification

- NFR1: Single dirty row rendering within 2ms — benchmark test comparing packed vs LineAccessor paths
- NFR3: WASM binary size check — verify wasm binary stays under 80KB (existing test covers this)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Packed Parser | 8 | 8 | — | — |
| Rendering Integration | 3 | 3 | — | 3 |
| Dirty Delegation | 3 | 3 | — | — |
| Kitty image_id | 4 | 4 | — | 2 |
| Regression | 2 | 2 | 2 | — |
| Code Quality | 2 | — | 2 | — |
| Performance | 2 | 1 | — | 1 |
| **Total** | **24** | **21** | **4** | **6** |
