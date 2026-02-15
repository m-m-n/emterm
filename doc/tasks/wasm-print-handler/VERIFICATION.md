# Verification Document: WASM Print Handler (Sprint 2)

## Overview

**Feature**: WASM Print Handler (Sprint 2)
**SPEC.md**: `doc/tasks/wasm-print-handler/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-print-handler/IMPLEMENTATION.md`

## Build Verification

### WASM Build
```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

### Rust Build
```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-run
```

### TypeScript Build
```bash
bun run typecheck
```

### Expected Result
- All commands exit with code 0
- No error messages
- WASM binary size < 50KB total

## Test Verification

### Rust Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Coverage Target
- **Minimum**: 80% for new Rust code in terminal_core.rs
- **Target**: 90%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-R01 | handle_print ASCII 'A' at (0,0) | cell='A', cursor (1,0), returns 0 | Rust Unit |
| TS-R02 | handle_print ASCII at (cols-1,0) with autoWrap | wrap_pending=true, returns 0 | Rust Unit |
| TS-R03 | handle_print ASCII with wrap_pending | CR+LF, print at (0,1), returns 0 | Rust Unit |
| TS-R04 | handle_print ASCII at bottom with wrap_pending | Returns 1 (scroll needed) | Rust Unit |
| TS-R05 | handle_print CJK at (0,0) | width=2 + placeholder, cursor (2,0) | Rust Unit |
| TS-R06 | handle_print CJK at (cols-1,0) autoWrap | Wrap to next line | Rust Unit |
| TS-R07 | handle_print Emoji (EXT_PICTOGRAPHIC) | Buffered, not written | Rust Unit |
| TS-R08 | handle_print ZWJ after emoji | Added to buffer | Rust Unit |
| TS-R09 | handle_print non-extending after buffered emoji | Flush + new cp | Rust Unit |
| TS-R10 | handle_print Regional Indicator pair | Both buffered, auto-flush | Rust Unit |
| TS-R11 | handle_print Variation Selector FE0E | Width 1 | Rust Unit |
| TS-R12 | handle_print Variation Selector FE0F | Width 2 | Rust Unit |
| TS-R13 | handle_print Skin tone modifier | Added to buffer | Rust Unit |
| TS-R14 | handle_print buffer overflow (65 cps) | Auto-flush at 64 | Rust Unit |
| TS-R15 | handle_print DEC Line Drawing 'q' (0x71) | '─' (0x2500) when active | Rust Unit |
| TS-R16 | handle_print DEC Line Drawing inactive | No translation | Rust Unit |
| TS-R17 | handle_print G1 charset with DecLineDrawing | Correct translation | Rust Unit |
| TS-R18 | handle_print autoWrap OFF at line end | Cursor stays | Rust Unit |
| TS-R19 | flush_grapheme_buffer empty | Returns 0 | Rust Unit |
| TS-R20 | flush_grapheme_buffer single emoji | Width from EmojiPresentation | Rust Unit |
| TS-R21 | flush_grapheme_buffer ZWJ sequence | Width 2 | Rust Unit |
| TS-R22 | flush_grapheme_buffer flag (RI pair) | Width 2 | Rust Unit |
| TS-R23 | flush_grapheme_buffer with wrap_pending | CR+LF+write, returns scroll | Rust Unit |
| TS-R24 | scroll_region: LF within region | No scroll past bottom | Rust Unit |
| TS-R25 | scroll_region: LF at region bottom | Returns 1 | Rust Unit |
| TS-R26 | Charset getter/setter round-trip | Values preserved | Rust Unit |
| TS-R27 | Active charset switch G0/G1 | Correct behavior | Rust Unit |
| TS-R28 | wrap_pending getter/setter | Values preserved | Rust Unit |
| TS-R29 | scroll_region getter/setter | Values preserved | Rust Unit |
| TS-T01 | handlePrint WASM path: ASCII | Correct output | TS Integration |
| TS-T02 | handlePrint WASM path: CJK | Correct width | TS Integration |
| TS-T03 | handlePrint WASM path: emoji | Correct rendering | TS Integration |
| TS-T04 | handlePrint WASM path: DEC Line Drawing | Correct chars | TS Integration |
| TS-T05 | handlePrint WASM path: autoWrap scroll | Scroll performed | TS Integration |
| TS-T06 | handlePrint JS fallback | Works when wasmGrid null | TS Integration |
| TS-T07 | flushGraphemeBuffer WASM delegation | Flush before non-Print | TS Integration |
| TS-T08 | wasmRowToLine optimized: ASCII | Identical to per-cell | TS Integration |
| TS-T09 | wasmRowToLine optimized: CJK | Identical to per-cell | TS Integration |
| TS-T10 | wasmRowToLine optimized: overflow | Identical to per-cell | TS Integration |
| TS-T11 | dispose: no errors | Resources freed | TS Integration |
| TS-T12 | Cross-validation: WASM vs JS identical | All cases match | TS Cross-Val |
| TS-T13 | DEC Line Drawing: all 32 entries | Rust = TS output | TS Cross-Val |
| TS-T14 | Regression: all existing tests | 1822+ pass | TS Regression |

## File Structure Verification

### Files to Modify
- `wasm/src/terminal_core.rs` - Add handle_print, grapheme buffer, charset, scroll region (~400 lines)
- `wasm/src/lib.rs` - Minor export additions if needed
- `src/terminal/handlers/print_handler.ts` - WASM dispatch wrapper
- `src/terminal/state.ts` - flushGraphemeBuffer delegation, charset sync
- `src/terminal/unified-buffer.ts` - Scroll region sync to WASM
- `src/terminal/wasm/terminal-core.ts` - wasmRowToLine optimization

### Files NOT Modified (verify no changes)
- `wasm/src/cell.rs` - No changes
- `wasm/src/unicode.rs` - No changes
- `src/terminal/wasm/loader.ts` - No changes
- `src/terminal/wasm/unicode.ts` - No changes
- `src/terminal/canvas-renderer.ts` - No changes

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-01 | `wasm-pack build` succeeds with Sprint 2 additions | Run build, exit code 0 |
| SC-02 | WASM binary size < 50KB total | Check file size of emterm_wasm_bg.wasm |
| SC-03 | All Rust unit tests pass | `cargo test`, all pass |
| SC-04 | All existing TS tests pass (1822+) | `bun test`, count >= 1822 |
| SC-05 | ASCII throughput >= 2x Sprint 1 | Benchmark test |
| SC-06 | Emoji sequences render correctly | Cross-validation + manual test |
| SC-07 | DEC Line Drawing renders correctly | All 30 entries test + manual vim test |
| SC-08 | dispose() works correctly | Unit test, no errors |
| SC-09 | wasmRowToLine single WASM call | Performance comparison |
| SC-10 | `bun tauri dev` working terminal | Manual verification |
| SC-11 | vttest basic tests unchanged | Manual verification |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: handle_print(cp) -> u8 | Phase 1 | TS-R01 through TS-R18 |
| FR2: flush_grapheme_buffer() -> u8 | Phase 1 | TS-R19 through TS-R23 |
| FR3: Grapheme buffer Vec<u32> in WASM | Phase 1 | TS-R07, TS-R08, TS-R14 |
| FR4: wrap_pending in WASM | Phase 1 | TS-R02, TS-R03, TS-R28 |
| FR5: Charset state in WASM | Phase 1 | TS-R15, TS-R16, TS-R17, TS-R26, TS-R27 |
| FR6: DEC Line Drawing table | Phase 1 | TS-R15, TS-T13 |
| FR7: ASCII fast path | Phase 1 | TS-R01, TS-R02, TS-R03, TS-R04 |
| FR8: Slow path (charWidth + charset + wrap) | Phase 1 | TS-R05, TS-R06, TS-R15 |
| FR9: Scroll region in WASM | Phase 1 | TS-R24, TS-R25, TS-R29 |
| FR10: wrap_pending getter/setter | Phase 1 | TS-R28 |
| FR11: scroll_region setter | Phase 1 | TS-R29 |
| FR12: WasmGrid.dispose() | Phase 3 | TS-T11 |
| FR13: wasmRowToLine optimization | Phase 3 | TS-T08, TS-T09, TS-T10 |

## E2E Testing (Docker)

### Setup
- Dockerfile: existing `docker-compose.e2e.yml`
- Build service: `build`
- Run: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "..."`

### Build Verification
- [ ] WASM build: `cd wasm && wasm-pack build --target web --out-dir pkg`
- [ ] Rust build: `cargo test --manifest-path src-tauri/Cargo.toml --no-run`
- [ ] TS type check: `bun run typecheck`

### Test Execution
- [ ] Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] TS tests: `bun test`

### Binary Size Check
- [ ] WASM binary < 50KB: `ls -la wasm/pkg/emterm_wasm_bg.wasm`

## Manual Testing (E2E Not Possible)

Items requiring the full Tauri application:

- [ ] `bun tauri dev` → terminal starts and accepts input
- [ ] Type ASCII text → characters appear correctly
- [ ] Type CJK characters → correct double-width display
- [ ] Copy-paste emoji (flags, ZWJ family) → correct display
- [ ] Open vim → border characters (DEC Line Drawing) display correctly
- [ ] Open htop/top → borders display correctly
- [ ] `cat` a large file (>1MB) → output completes, visually faster than Sprint 1
- [ ] Scroll through output → scrollback works correctly
- [ ] Resize terminal window → display reflows correctly
- [ ] vttest basic tests → no regressions

## Performance Verification

### ASCII Throughput Benchmark
- **Requirement**: >= 2x Sprint 1 baseline
- **Method**: Time N Print actions (e.g., 100,000 ASCII chars) via WASM path vs JS fallback
- **Expected**: WASM path at least 2x faster

### wasmRowToLine Optimization
- **Requirement**: Single WASM call per row (vs cols*5 currently)
- **Method**: Compare execution time for wasmRowToLine before and after optimization
- **Expected**: >= 3x improvement

### WASM Binary Size
- **Requirement**: Total < 50KB (Sprint 1 baseline: 39.5KB)
- **Command**: `wc -c wasm/pkg/emterm_wasm_bg.wasm`
- **Expected**: < 51200 bytes

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 3 | - | ✅ | - |
| Rust Tests | 29 | ✅ | ✅ | - |
| TS Tests | 14 | ✅ | ✅ | - |
| File Structure | 12 | ✅ | - | - |
| SPEC Compliance | 11 | Partial | ✅ | ✅ |
| Performance | 3 | Partial | - | ✅ |
| Manual Testing | 10 | - | - | ✅ |

**Total**: 43 automated items, 5 E2E (Docker) items, 10 manual items
