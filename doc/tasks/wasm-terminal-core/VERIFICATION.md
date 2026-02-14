# Verification Document: WASM TerminalCore (Grid + State Data Layer)

## Overview

**Feature**: WASM TerminalCore - viewport grid and terminal state in WebAssembly
**SPEC.md**: `doc/tasks/wasm-terminal-core/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-terminal-core/IMPLEMENTATION.md`

## Build Verification

### WASM Build

```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

**Expected Result**:
- Exit code: 0
- `wasm/pkg/emterm_wasm_bg.wasm` generated
- `wasm/pkg/emterm_wasm.d.ts` contains `TerminalCore` class

### TypeScript Type Check

```bash
bun run typecheck
```

**Expected Result**:
- Exit code: 0
- No type errors

### WASM Binary Size

```bash
ls -la wasm/pkg/emterm_wasm_bg.wasm
```

**Expected Result**:
- Size < 64KB (65,536 bytes)

## Test Verification

### Rust Unit Tests

```bash
cd wasm && cargo test
```

**Coverage Target**:
- **Minimum**: 80%
- **Target**: 90%

### TypeScript Tests

```bash
bun test
```

**Coverage Target**:
- **Minimum**: Existing 1779+ tests all pass
- **Target**: 1779 + ~30 new tests all pass

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-R01 | Cell: ASCII character | char="A", width=1, attrs default | Unit (Rust) |
| TS-R02 | Cell: CJK character | char="漢", width=2 | Unit (Rust) |
| TS-R03 | Cell: Emoji character | char stored, width=2 | Unit (Rust) |
| TS-R04 | Cell: Overflow grapheme (>16 bytes) | Stored in side table, retrievable | Unit (Rust) |
| TS-R05 | PackedColor: Default | tag=0, payload ignored | Unit (Rust) |
| TS-R06 | PackedColor: Indexed(0) and Indexed(255) | tag=1, index correct | Unit (Rust) |
| TS-R07 | PackedColor: RGB(0,0,0) and RGB(255,255,255) | tag=2, r/g/b correct | Unit (Rust) |
| TS-R08 | Style flags: Each individually | Correct bit set | Unit (Rust) |
| TS-R09 | Style flags: All combined | All bits set | Unit (Rust) |
| TS-R10 | Grid: new(80, 24) | 1920 empty cells | Unit (Rust) |
| TS-R11 | Grid: setCell/getCell ASCII | Round-trip correct | Unit (Rust) |
| TS-R12 | Grid: setCell/getCell wide char | Round-trip correct | Unit (Rust) |
| TS-R13 | Grid: Out-of-bounds access | No-op for write, default for read | Unit (Rust) |
| TS-R14 | Cursor: Initial position | (0, 0) | Unit (Rust) |
| TS-R15 | Cursor: setCursor clamps | Clamped to bounds | Unit (Rust) |
| TS-R16 | Cursor: save/restore | Position + attrs preserved | Unit (Rust) |
| TS-R17 | Modes: Default values | autoWrap=true, etc. | Unit (Rust) |
| TS-R18 | Modes: Set/get bits | Correct bit values | Unit (Rust) |
| TS-R19 | TabStops: Default every 8 | Correct positions | Unit (Rust) |
| TS-R20 | TabStops: Set/clear/next | Correct behavior | Unit (Rust) |
| TS-R21 | Dirty: setCell marks dirty | Row dirty after write | Unit (Rust) |
| TS-R22 | Dirty: clearDirty resets | No dirty rows after clear | Unit (Rust) |
| TS-R23 | Dirty: resize marks all | All rows dirty after resize | Unit (Rust) |
| TS-R24 | clearLine fills with empty | All cells are spaces | Unit (Rust) |
| TS-R25 | clearLineRange partial | Only range cleared | Unit (Rust) |
| TS-R26 | getLineText skips width=0 | Text excludes placeholders | Unit (Rust) |
| TS-R27 | isLineEmpty checks all | True when all spaces | Unit (Rust) |
| TS-R28 | shiftRowsUp | Data moved correctly | Unit (Rust) |
| TS-R29 | shiftRowsDown | Data moved correctly | Unit (Rust) |
| TS-R34 | copyRow copies data | Destination matches source | Unit (Rust) |
| TS-R35 | fillRowDefault clears row | All cells are empty after fill | Unit (Rust) |
| TS-R36 | Overflow side table remapped on shiftRowsUp | Overflow data accessible at new position | Unit (Rust) |
| TS-R30 | Resize: grow cols | Existing data preserved | Unit (Rust) |
| TS-R31 | Resize: shrink cols | Data truncated | Unit (Rust) |
| TS-R32 | Resize: grow/shrink rows | Correct behavior | Unit (Rust) |
| TS-R33 | Reset | All state to default | Unit (Rust) |
| TS-T01 | WasmGrid construct | No errors, correct dims | Unit (TS) |
| TS-T02 | setCell + getCell ASCII | Round-trip correct | Unit (TS) |
| TS-T03 | setCell + getCell CJK | Round-trip correct | Unit (TS) |
| TS-T04 | Attribute round-trip (default) | null fg/bg preserved | Unit (TS) |
| TS-T05 | Attribute round-trip (RGB) | r,g,b preserved | Unit (TS) |
| TS-T06 | Attribute round-trip (indexed) | Index preserved | Unit (TS) |
| TS-T07 | Style flags round-trip | All 8 flags preserved | Unit (TS) |
| TS-T08 | WasmLineProxy.getText() | Matches Line.getText() | Unit (TS) |
| TS-T09 | Cursor via WASM | Position/attrs correct | Integration (TS) |
| TS-T10 | Modes via WASM | All modes correct | Integration (TS) |
| TS-T11 | Scroll up | Top row → scrollback | Integration (TS) |
| TS-T12 | Alternate buffer switch | Clean grid, primary preserved | Integration (TS) |
| TS-T13 | Resize integration | Viewport resized, all dirty | Integration (TS) |
| TS-T14 | Full regression | All 1779+ tests pass | Regression (TS) |

## Code Quality Verification

### Rust Format Check

```bash
cd wasm && cargo fmt -- --check
```

### TypeScript Type Check

```bash
bun run typecheck
```

## File Structure Verification

### Files to Create

- `wasm/src/cell.rs` - Cell struct, PackedColor, style flags
- `wasm/src/terminal_core.rs` - TerminalCore struct and all operations
- `src/terminal/wasm/terminal-core.ts` - WasmGrid, WasmLineProxy, converters
- `src/terminal/wasm/__tests__/terminal-core.test.ts` - Cross-validation and benchmarks

### Files to Modify

- `wasm/src/lib.rs` - Add module declarations and wasm_bindgen exports
- `src/terminal/state.ts` - Use WasmGrid for viewport
- `src/terminal/unified-buffer.ts` - Delegate viewport to WasmGrid
- `src/terminal/attributes.ts` - Add pack/unpack conversion functions
- `src/terminal/cursor.ts` - Delegate to WASM cursor state
- `src/terminal/modes.ts` - Delegate to WASM modes

### Files Unchanged

- `src/terminal/wasm/loader.ts` - No changes
- `src/terminal/wasm/unicode.ts` - No changes
- `src/terminal/handlers/*` - No changes (Sprint 2+)
- `src/terminal/canvas-renderer.ts` - Minimal changes (adapter layer absorbs)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | wasm-pack build succeeds with TerminalCore exports | `wasm-pack build` exit code 0 |
| SC-2 | WASM binary size < 64KB total | `ls -la` check |
| SC-3 | All Rust unit tests pass | `cargo test` exit code 0 |
| SC-4 | All existing TypeScript tests pass (1779+) | `bun test` exit code 0 |
| SC-5 | Renderer correctly displays from WASM grid | Cross-validation tests + manual |
| SC-6 | Cursor operations work through WASM | Cursor-related tests pass |
| SC-7 | Mode operations work through WASM | Mode-related tests pass |
| SC-8 | Scroll works (viewport → scrollback) | Scroll tests pass |
| SC-9 | `bun tauri dev` shows working terminal | Manual verification |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: TerminalCore viewport grid | Phase 1-2 | Rust unit tests + WASM build |
| FR2: Cell stores char/width/attrs | Phase 1 | TS-R01 through TS-R04 |
| FR3: PackedColor 4 bytes | Phase 1 | TS-R05 through TS-R07 |
| FR4: Style flags u16 bitfield | Phase 1 | TS-R08, TS-R09 |
| FR5: CursorState in WASM | Phase 1-2 | TS-R14 through TS-R16 |
| FR6: TerminalModes bitfield | Phase 1-2 | TS-R17, TS-R18 |
| FR7: TabStops in WASM | Phase 1-2 | TS-R19, TS-R20 |
| FR8: Dirty tracking | Phase 1-2 | TS-R21 through TS-R23 |
| FR9: wasm_bindgen exports | Phase 2 | wasm-pack build + .d.ts check |
| FR10: resize | Phase 1-4 | TS-R30 through TS-R32, TS-T13 |
| FR11: Line clear/fill | Phase 1 | TS-R24, TS-R25 |
| FR12: getLineText | Phase 1 | TS-R26 |

## Docker E2E Testing

### Setup

```bash
# Rust unit tests (WASM crate)
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo test"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"

# Rust format check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && cargo fmt -- --check"
```

### Automated Checks

- [ ] WASM build succeeds
- [ ] Rust unit tests pass (33+ tests)
- [ ] TypeScript tests pass (1779+ existing + ~30 new)
- [ ] TypeScript type check passes
- [ ] Rust format check passes
- [ ] WASM binary < 64KB

### Integration Checks

- [ ] Cell round-trip: set via handler → read via renderer adapter
- [ ] Scroll: viewport row correctly copied to JS scrollback
- [ ] Alternate buffer: switch/restore cycle preserves primary grid
- [ ] Resize: viewport changes, data preserved where possible

## Manual Testing (E2E Not Possible)

- [ ] `bun tauri dev` launches and terminal accepts input
- [ ] Basic commands (ls, cat, echo) display correctly
- [ ] vim: open/edit/save/quit works (alternate buffer)
- [ ] htop: displays correctly (alternate buffer + colors)
- [ ] less: scrolling works correctly
- [ ] Colored output (e.g., `ls --color`) renders correctly
- [ ] Wide characters (CJK) display at correct width
- [ ] Emoji display correctly

## Performance Verification

### Benchmarks

| Metric | Requirement | Command |
|--------|-------------|---------|
| setCell/getCell latency | < 100ns per call | Custom benchmark in terminal-core.test.ts |
| Full viewport read | < 1ms for 80x120 | Custom benchmark in terminal-core.test.ts |
| WASM binary size | < 64KB | `ls -la wasm/pkg/emterm_wasm_bg.wasm` |

## Dead Code Verification

After integration, verify no leftover unused code:

- [ ] `src/terminal/grid.ts`: `createEmptyCell`, `createCell`, `createAsciiCell` still used (for scrollback Line creation)
- [ ] `src/terminal/grid.ts`: `Line` class still used (for scrollback)
- [ ] No unused imports in modified files
- [ ] No commented-out old code left behind

## Verification Summary

| Category | Items | Automated | Docker E2E | Manual |
|----------|-------|-----------|------------|--------|
| Build | 3 | ✅ | ✅ | - |
| Rust Tests | 36 | ✅ | ✅ | - |
| TS Tests | 14 | ✅ | ✅ | - |
| Code Quality | 2 | ✅ | ✅ | - |
| File Structure | 10 | ✅ | - | - |
| SPEC Compliance | 9 | Partial | - | ✅ |
| Performance | 3 | ✅ | - | - |
| Dead Code | 4 | - | - | ✅ |
| Manual E2E | 8 | - | - | ✅ |

**Total**: 52 automated items, 6 Docker E2E items, 21 manual items
