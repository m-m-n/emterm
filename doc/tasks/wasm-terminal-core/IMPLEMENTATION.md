# Implementation Plan: WASM TerminalCore (Grid + State Data Layer)

## Overview

Port terminal data structures (Cell, CellAttributes, CursorState, TerminalModes, viewport Grid) from TypeScript to Rust/WASM, creating a `TerminalCore` struct in WASM linear memory. TypeScript's `TerminalState` becomes a thin wrapper delegating viewport operations to WASM. Scrollback remains in JS.

## Objectives

- Move viewport cell storage from JS heap to WASM linear memory
- Export all grid/cursor/mode operations via wasm_bindgen
- Create TS adapter layer maintaining existing Line/Cell interface compatibility
- Wire WASM grid into TerminalState and UnifiedBuffer with full test regression

## Prerequisites

### Development Environment

- Rust toolchain with `wasm32-unknown-unknown` target (established in Sprint 0)
- wasm-pack (established in Sprint 0)
- Bun (existing)

### Dependencies

- `wasm-bindgen = "0.2"` (existing in wasm/Cargo.toml)
- No new npm dependencies

### Knowledge Requirements

- WASM linear memory layout and wasm_bindgen type restrictions
- Existing TerminalState/UnifiedBuffer architecture (explored in SPEC)
- Sprint 0 patterns: WASM build pipeline, test setup, loader

## Architecture Overview

### Design Approach

**Strangler fig migration:** WASM TerminalCore gradually takes over data storage from TS. In this Sprint, only viewport grid is migrated; scrollback and handlers remain in TS.

**Adapter pattern:** `WasmLineProxy` wraps WASM row access behind the existing `Line`-compatible interface, minimizing changes to renderer and handlers.

### Component Interaction

```
TerminalState (TS, wrapper)
  ├── TerminalCore (WASM, viewport data owner)
  │     ├── Grid: rows × cols cells in linear memory
  │     ├── Cursor: position + attributes
  │     ├── Modes: bitfield
  │     ├── TabStops: vec
  │     └── DirtyRows: bitset
  │
  └── UnifiedBuffer (TS, modified)
        ├── Viewport access → delegates to TerminalCore
        ├── Scrollback → JS Line[] ring (unchanged)
        └── Scroll → copies WASM row to JS Line, then pushes to ring
```

## Implementation Phases

### Phase 1: Rust Data Structures

**Goal**: Define Cell, PackedColor, CursorState, Modes, Grid, and TabStops in Rust with comprehensive unit tests.

**Files to Create**:
- `wasm/src/cell.rs` - Cell struct, PackedColor, style flag constants
- `wasm/src/terminal_core.rs` - TerminalCore struct with grid, cursor, modes, tab stops, dirty tracking

**Files to Modify**:
- `wasm/src/lib.rs` - Add `mod cell; mod terminal_core;` declarations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cell | Store character (UTF-8), width, and packed attributes in 32 bytes | Valid UTF-8 string + width + color/flags | Data accessible via field access |
| PackedColor | Encode Default/Indexed/RGB color in 4 bytes | Valid tag (0-2) + payload | Color queryable by tag + components |
| TerminalCore | Own viewport grid (Vec<Cell>), cursor, modes, tab stops, dirty bitset | cols > 0, rows > 0 | Grid of cols*rows cells initialized to empty |
| DirtyBitset | Track which rows need re-rendering | Valid row index | Bit set/cleared per row |

**Processing Flow**:
```
TerminalCore::new(cols, rows)
  1. Allocate grid: Vec<Cell> of size cols * rows, all empty
  2. Initialize cursor at (0, 0) with default attributes
  3. Initialize modes to defaults (autoWrap=true, etc.)
  4. Initialize tab stops every 8 columns
  5. Mark all rows dirty
```

**Implementation Steps**:

1. **Define Cell and PackedColor**
   - Cell: 32-byte packed struct with 16-byte inline UTF-8 buffer, overflow via side table
   - PackedColor: 4-byte struct (tag byte + 3 payload bytes)
   - Style flags: u16 bitfield with constants for each SGR attribute
   - Key consideration: char_len=0xFF signals overflow; side table keyed by (col, row)
   - Key consideration: Side table requires lifecycle management — shift_rows_up/down must remap keys, clear_line/resize must delete stale keys (see SPEC.md "Side table lifecycle rules")

2. **Define TerminalCore struct**
   - Grid stored as flat `Vec<Cell>` indexed by `row * cols + col`
   - CursorState as nested struct (col, row, attrs, saved, visible, style, blink)
   - Modes as u32 bitfield matching SPEC layout
   - TabStops as `Vec<bool>` with default every 8 columns
   - DirtyRows as `Vec<u64>` bitset (1 bit per row, packed into u64 words)

3. **Implement grid operations**
   - set_cell / get_cell with bounds checking
   - clear_line / clear_line_range
   - shift_rows_up / shift_rows_down (for scroll support)
   - get_line_text / is_line_empty / line wrapped flag

4. **Implement cursor operations**
   - Position get/set with bounds clamping
   - Attribute get/set (fg, bg, flags)
   - save/restore (ESC 7/8)

5. **Implement modes and tab stops**
   - Modes: get/set individual bits, get/set full u32
   - TabStops: set/clear/clear_all/next_tab_stop

6. **Write Rust unit tests**
   - All test scenarios from SPEC.md "Unit Tests (Rust)" section
   - Test cell overflow (grapheme > 16 bytes)
   - Test dirty tracking lifecycle

**Dependencies**:
- Requires: Sprint 0 complete (wasm-pack pipeline, Cargo.toml)
- Blocks: Phase 2

**Testing Approach**:

*Unit Tests (Rust)*:
| Scenario | Expected |
|----------|----------|
| Cell ASCII round-trip | char="A", width=1 |
| Cell CJK round-trip | char="漢", width=2 |
| Cell emoji overflow | Stored in side table, retrievable |
| PackedColor Default/Indexed/RGB | Tag + payload correct |
| Grid bounds | Out-of-bounds = no-op/default |
| Cursor save/restore | Position + attrs preserved |
| Modes default values | Match TS createDefaultModes() |
| Dirty lifecycle | set_cell → dirty, clear → clean |
| Resize | Data preserved, all dirty |

**Acceptance Criteria**:
- [ ] `cargo test` passes all unit tests
- [ ] Cell struct is exactly 32 bytes (`assert_eq!(std::mem::size_of::<Cell>(), 32)`)
- [ ] Grid correctly stores/retrieves ASCII, CJK, and emoji characters

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: 32-byte Cell alignment issues across platforms
  - **Mitigation**: Use `#[repr(C)]` and compile-time size assertion

---

### Phase 2: WASM API Exports

**Goal**: Export all TerminalCore operations via `#[wasm_bindgen]` and verify JS can construct and operate TerminalCore.

**Files to Modify**:
- `wasm/src/lib.rs` - Add all `#[wasm_bindgen]` method exports
- `wasm/src/terminal_core.rs` - Add `#[wasm_bindgen]` annotations to TerminalCore impl

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| wasm_bindgen exports | Expose TerminalCore API with primitive-friendly signatures | Phase 1 types defined | All methods callable from JS |
| Color encoding | Pack/unpack colors as u32 for JS boundary | Valid PackedColor | `tag<<24 \| r<<16 \| g<<8 \| b` |

**Processing Flow**:
```
JS call → wasm_bindgen glue
  1. Convert JS types to Rust types (primitives pass through)
  2. Call TerminalCore method
  3. Convert result to JS-compatible type
  4. Return to JS
```

**Implementation Steps**:

1. **Export cell operations**
   - `set_cell(col, row, char, width, fg_tag, fg_r, fg_g, fg_b, bg_tag, bg_r, bg_g, bg_b, flags)`
   - `set_cell_ascii(col, row, byte, ...)` fast path
   - `get_cell_char`, `get_cell_width`, `get_cell_fg`, `get_cell_bg`, `get_cell_flags`
   - `get_row_packed(row)` batch API for renderer hot path (single WASM call per row)
   - Key consideration: wasm_bindgen cannot return structs directly; use separate getters

2. **Export line and row operations**
   - clear_line, clear_line_range, get_line_text, is_line_empty
   - get/set line wrapped flag
   - shift_rows_up/down, fill_row_default

3. **Export cursor, modes, tab stops, dirty operations**
   - All getters/setters as listed in SPEC.md JS API section

4. **Verify wasm-pack build**
   - `wasm-pack build --target web --out-dir pkg`
   - Check generated `.d.ts` types match expected API
   - Verify WASM binary size within budget

**Dependencies**:
- Requires: Phase 1 complete
- Blocks: Phase 3

**Testing Approach**:

*Integration Tests (minimal, verifying wasm-pack output)*:
- wasm-pack build succeeds without errors
- Generated TypeScript declarations contain all expected methods
- WASM binary size < 64KB total

**Acceptance Criteria**:
- [ ] `wasm-pack build` succeeds
- [ ] Generated `.d.ts` contains TerminalCore class with all methods
- [ ] WASM binary size < 64KB
- [ ] JS can `new TerminalCore(80, 24)` and call basic operations

**Estimated Effort**: 小 (1-2 days)

---

### Phase 3: TypeScript Adapter Layer

**Goal**: Create WasmGrid adapter and conversion utilities that provide existing Line/Cell interface compatibility backed by WASM data.

**Files to Create**:
- `src/terminal/wasm/terminal-core.ts` - WasmGrid class, WasmLineProxy, WasmCellProxy, attribute conversion utilities

**Files to Modify**:
- `src/terminal/attributes.ts` - Add TS ↔ WASM packed format conversion functions

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| WasmGrid | Wrap TerminalCore instance, manage lifecycle | WASM loaded | Provides Line-compatible access to WASM grid |
| WasmLineProxy | Read-through proxy for WASM row, implements Line interface | Valid row index | Cell data read from WASM on demand |
| WasmCellProxy | Read-through proxy for WASM cell, implements Cell interface | Valid col/row | Cell properties read from WASM |
| Attribute converters | Convert between TS CellAttributes and WASM packed format | Valid attributes | Bidirectional conversion without data loss |

**Processing Flow**:
```
Handler calls buffer.getLine(row)
  1. WasmLineProxy created (lightweight, stores row index + core reference)
  2. Renderer calls line.getCell(col)
  3. WasmCellProxy reads char/width/attrs from WASM via get_cell_* methods
  4. Renderer uses cell.char, cell.width, cell.attrs as before

Handler calls buffer.setCell(col, row, cell)
  1. Adapter unpacks TS Cell → primitive args
  2. Calls core.set_cell(col, row, char, width, fg_tag, ..., flags)
  3. WASM writes to linear memory + marks row dirty
```

**Implementation Steps**:

1. **Create attribute conversion utilities**
   - `packColor(color: Color | null)` → `{tag, r, g, b}` (for WASM)
   - `unpackColor(packed: u32)` → `Color | null` (from WASM)
   - `packStyleFlags(attrs: CellAttributes)` → `u16`
   - `unpackStyleFlags(flags: u16)` → style booleans
   - Key consideration: null color maps to tag=0 (Default)

2. **Create WasmCellProxy and WasmLineProxy**
   - WasmCellProxy: getter-only proxy reading from WASM
   - WasmLineProxy: provides getCell(col), getText(), isEmpty(), length, wrapped
   - Key consideration: proxies are lightweight (no data copy), created per access

3. **Create WasmGrid class**
   - Wraps TerminalCore instance
   - Provides setCell(col, row, cell), getLine(row), clearLine(row), etc.
   - Manages viewport dimensions via core.resize()
   - Dirty tracking: getDirtyRows(), clearDirty()

4. **Add conversion for Line ↔ WASM**
   - `wasmRowToLine(core, row)`: reads WASM row, creates JS Line (for scroll-out)
   - `lineToWasmRow(core, row, line)`: writes JS Line data to WASM row

5. **Write TypeScript unit tests**
   - Round-trip: set cell via WasmGrid → read via WasmLineProxy
   - Attribute conversion: TS→WASM→TS preserves all fields
   - Line text extraction matches existing Line.getText()

**Dependencies**:
- Requires: Phase 2 complete (wasm-pack exports available)
- Blocks: Phase 4

**Testing Approach**:

*Unit Tests (TypeScript, bun test)*:
| Scenario | Expected |
|----------|----------|
| WasmGrid construct(80,24) | No errors, correct dimensions |
| setCell + getCell ASCII | char, width, attrs match |
| setCell + getCell CJK | width=2, char correct |
| Attribute round-trip (default) | null fg/bg preserved |
| Attribute round-trip (RGB) | r,g,b values preserved |
| Attribute round-trip (indexed) | index preserved |
| Style flags round-trip | All 8 flags preserved |
| WasmLineProxy.getText() | Matches Line.getText() behavior |
| wasmRowToLine conversion | Creates valid JS Line |

**Acceptance Criteria**:
- [ ] WasmGrid provides setCell/getCell with correct data
- [ ] WasmLineProxy returns correct cell data from WASM
- [ ] Attribute conversion is lossless (TS → WASM → TS)
- [ ] wasmRowToLine creates valid JS Line objects

**Estimated Effort**: 中 (3-5 days)

---

### Phase 4: Integration with TerminalState and UnifiedBuffer

**Goal**: Wire WasmGrid into the existing terminal pipeline, replacing JS cell storage for viewport while preserving scrollback in JS.

**Files to Modify**:
- `src/terminal/state.ts` - TerminalState uses WasmGrid for viewport
- `src/terminal/unified-buffer.ts` - Viewport delegates to WasmGrid, scrollback unchanged
- `src/terminal/grid.ts` - Mark createEmptyCell/createCell/createAsciiCell as used by scrollback only
- `src/terminal/cursor.ts` - CursorState becomes proxy: col/row get/set delegates to WASM; movement methods (moveRight, etc.) update via WASM set_cursor; save/restore delegates to WASM
- `src/terminal/modes.ts` - TerminalModes interface maintained; setDecPrivateMode() action dispatch stays in TS, mode bit storage delegates to WASM bitfield

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TerminalState (modified) | Create/hold WasmGrid, pass to UnifiedBuffer | WASM initialized | Viewport data in WASM, scrollback in JS |
| UnifiedBuffer (modified) | Route viewport ops to WasmGrid, scrollback to JS ring | WasmGrid provided | setCell/getLine work for both viewport and scrollback |
| Scroll bridge | Copy viewport row to JS Line when scrolling up | Row in WASM | JS Line in scrollback ring |
| Alternate buffer | Second WasmGrid instance for alternate screen | Primary WasmGrid exists | Separate viewport for vim/less/htop |

**Processing Flow**:
```
scrollUp() in modified UnifiedBuffer:
  1. Read top viewport row from WASM → create JS Line (wasmRowToLine)
  2. Push JS Line to scrollback ring
  3. Call core.shift_rows_up(0, rows-1, 1) to shift viewport
  4. Call core.fill_row_default(rows-1) to clear bottom row
  5. Mark affected rows dirty

switchToAlternateBuffer():
  1. Save primary WasmGrid reference
  2. Create new WasmGrid(cols, rows) for alternate
  3. Replace active grid reference
  4. Save cursor state

switchToPrimaryBuffer():
  1. Restore primary WasmGrid reference
  2. Dispose alternate WasmGrid
  3. Restore cursor state
  4. Mark all rows dirty
```

**Implementation Steps**:

1. **Modify UnifiedBuffer to accept WasmGrid**
   - Constructor receives optional WasmGrid for viewport backing
   - Viewport getLine/setCell route to WasmGrid when present
   - Scrollback operations remain in JS ring (no change)
   - Key consideration: maintain backward compatibility - if no WasmGrid, use existing JS cells

2. **Modify TerminalState to create WasmGrid**
   - Create WasmGrid in constructor after WASM initialization
   - Pass to UnifiedBuffer
   - Cursor operations delegate to WasmGrid's cursor API
   - Modes operations delegate to WasmGrid's modes API

3. **Implement scroll bridge**
   - scrollUp: read WASM row → JS Line → push to ring → shift WASM → clear bottom
   - scrollDown: similar reverse operation
   - Key consideration: scroll region must be respected (only shift within region)

4. **Implement alternate buffer support**
   - Create second WasmGrid for alternate screen
   - Swap active grid on buffer switch
   - Key consideration: alternate buffer has no scrollback (capacity = rows)

5. **Modify CursorState to delegate to WASM**
   - col/row properties: getter reads from WASM `get_cursor_col/row`, setter writes via `set_cursor`
   - Movement methods (moveRight, moveLeft, etc.): compute new position in TS, call `set_cursor`
   - Attributes (fg, bg, flags): delegate to WASM cursor attribute API
   - save/restore: delegate to WASM `save_cursor/restore_cursor`
   - Key consideration: handlers access `state.cursor.col` directly (property access) — must remain synchronous

6. **Modify TerminalModes to delegate to WASM**
   - Mode reads (autoWrap, cursorVisible, etc.): read from WASM via `get_mode(bit)`
   - Mode writes: write via `set_mode(bit, value)`
   - `setDecPrivateMode()` action dispatch: remains in TS (returns action strings for buffer switching, cursor save/restore)
   - Key consideration: multi-bit modes (mouseTracking, mouseEncoding, cursorKeys) need encoding/decoding between TS enum and WASM bit pairs

7. **Run full regression tests**
   - All existing tests must pass unchanged
   - Focus on: scroll behavior, alternate buffer (vim/less), resize

**Dependencies**:
- Requires: Phase 3 complete
- Blocks: Phase 5

**Testing Approach**:

*Integration Tests (TypeScript, bun test)*:
| Scenario | Expected |
|----------|----------|
| TerminalState constructor with WASM | No errors, viewport in WASM |
| Print character → read from renderer | Correct cell data from WASM |
| Scroll up | Top row moves to scrollback, bottom cleared |
| Scroll region | Only region rows affected |
| Alternate buffer switch | Clean grid, primary preserved |
| Alternate buffer restore | Primary grid correct |
| Resize | Viewport resized, all dirty |
| Full test suite regression | All 1779+ tests pass |

**Acceptance Criteria**:
- [ ] All existing terminal tests pass (1779+)
- [ ] Renderer correctly displays content from WASM-backed viewport
- [ ] Scroll moves viewport rows to JS scrollback correctly
- [ ] Alternate buffer (vim, less, htop) works correctly
- [ ] Resize preserves data and marks all dirty

**Estimated Effort**: 大 (1-2 weeks)

**Risks and Mitigation**:
- **Risk**: Subtle behavioral differences between WASM and JS cell storage
  - **Mitigation**: Run existing test suite at each sub-step; add cross-validation tests
- **Risk**: Scroll region edge cases
  - **Mitigation**: Focus testing on scroll-heavy applications (vim, less, htop)

---

### Phase 5: Verification and Benchmarking

**Goal**: Full regression verification, cross-validation, and performance measurement.

**Files to Create**:
- `src/terminal/wasm/__tests__/terminal-core.test.ts` - Cross-validation and benchmark tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cross-validation suite | Compare WASM Grid behavior with original TS Grid | Both implementations available | All results identical |
| Benchmark suite | Measure setCell/getCell, viewport read, memory | Working WASM integration | Performance metrics recorded |

**Implementation Steps**:

1. **Cross-validation tests**
   - For each existing grid/cursor/modes test: verify identical results
   - Run terminal action sequences on both WASM and TS paths, compare grid state

2. **Performance benchmarks**
   - setCell/getCell: 10,000 calls, measure average latency
   - Full viewport read: read 80×120 cells, measure total time
   - Memory: compare WASM linear memory usage vs JS heap baseline

3. **End-to-end verification**
   - `bun tauri dev` with WASM-backed grid
   - Manual testing: basic terminal use, vim, htop, colored output

**Dependencies**:
- Requires: Phase 4 complete

**Acceptance Criteria**:
- [ ] Cross-validation: all tests produce identical results
- [ ] setCell/getCell < 100ns per call
- [ ] Full viewport read < 1ms (80x120)
- [ ] WASM binary < 64KB total
- [ ] `bun tauri dev` shows working terminal

**Estimated Effort**: 小 (1-2 days)

---

## Complete File Structure

```
wasm/src/
├── lib.rs                # Extended: mod cell, mod terminal_core + wasm_bindgen exports
├── unicode.rs            # Existing (Sprint 0, unchanged)
├── cell.rs               # NEW: Cell, PackedColor, style flag constants
└── terminal_core.rs      # NEW: TerminalCore struct, grid/cursor/modes/dirty ops

src/terminal/wasm/
├── loader.ts             # Existing (unchanged)
├── unicode.ts            # Existing (unchanged)
└── terminal-core.ts      # NEW: WasmGrid, WasmLineProxy, WasmCellProxy, converters

src/terminal/
├── state.ts              # MODIFIED: creates WasmGrid, delegates viewport ops
├── unified-buffer.ts     # MODIFIED: viewport backed by WasmGrid
├── grid.ts               # MODIFIED: used only for scrollback Line creation
├── cursor.ts             # MODIFIED: delegates to WASM cursor state
├── modes.ts              # MODIFIED: delegates to WASM modes
├── attributes.ts         # MODIFIED: add packColor/unpackColor, packFlags/unpackFlags

src/terminal/wasm/__tests__/
└── terminal-core.test.ts # NEW: cross-validation and benchmark tests
```

## Testing Strategy

### Unit Testing (Rust)

**Approach**: `#[cfg(test)]` inline tests in each Rust module
**Coverage target**: 90%+ for cell/grid/cursor/modes logic

### Unit Testing (TypeScript)

**Approach**: bun test with WASM initialized via initSync
**Coverage target**: Adapter layer 80%+, attribute converters 100%

### Integration Testing

**Approach**: Existing test suite (1779+ tests) as regression gate
**Key areas**: scroll behavior, alternate buffer, resize, cursor operations

### Docker-first Testing

**Commands**:
```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
# Note: WASM crate tests run via: cd wasm && cargo test

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

## Dependencies

### Internal Dependencies (Implementation Order)

1. Phase 1 (Rust data structures) - no dependencies
2. Phase 2 (WASM exports) - depends on Phase 1
3. Phase 3 (TS adapter) - depends on Phase 2
4. Phase 4 (integration) - depends on Phase 3
5. Phase 5 (verification) - depends on Phase 4

### Component Dependencies

- `terminal-core.ts` depends on `wasm/pkg/emterm_wasm.js` (Phase 2 output)
- `state.ts` depends on `terminal-core.ts` (Phase 3 output)
- `unified-buffer.ts` depends on `terminal-core.ts` (Phase 3 output)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Cell 32-byte layout breaks on some targets | Low | High | `#[repr(C)]` + compile-time size assertion |
| JS-WASM boundary overhead for getCell | Medium | Medium | Batch APIs for renderer hot path |
| Behavioral differences in scroll edge cases | Medium | High | Run all tests at each phase; cross-validation |
| WASM binary size exceeds budget | Low | Low | Grid struct is simple data; no large lookup tables |
| Alternate buffer lifecycle issues | Medium | Medium | Test with vim/less/htop specifically |

## Open Questions

- [ ] Should WasmLineProxy cache cell reads or always go through WASM? (Performance trade-off)

## Resolved Questions

- **Q: How to handle `Line.getCells()` / `Line.setCells()` for reflow?**
  - **A:** Reflow reads WASM row data into temporary JS Cell arrays via `wasmRowToLine()`, performs reflow logic in JS, then writes back via `lineToWasmRow()`. WasmLineProxy provides `toCells(): Cell[]` (materializes JS array from WASM data) and WasmGrid provides `setRowFromCells(row, cells: Cell[])` (writes JS cells to WASM). This is acceptable because reflow only occurs on resize (infrequent operation).

## Success Metrics

### Functional Completeness
- [ ] All 1779+ existing tests pass
- [ ] Rust unit tests pass (30+ tests)
- [ ] Cross-validation tests pass
- [ ] Terminal works correctly in `bun tauri dev`

### Quality Metrics
- [ ] `bun run typecheck` passes
- [ ] `cargo fmt --check` passes
- [ ] No new lint warnings

### Performance Metrics
- [ ] setCell/getCell < 100ns
- [ ] Full viewport read < 1ms (80x120)
- [ ] WASM binary < 64KB total

## References

- **Specification**: `doc/tasks/wasm-terminal-core/SPEC.md`
- **Sprint 0 SPEC**: `doc/tasks/wasm-unicode-width/SPEC.md`
- **WASM roadmap**: `tmp/wasm.md`
- **wasm-bindgen types reference**: https://rustwasm.github.io/wasm-bindgen/reference/types.html
