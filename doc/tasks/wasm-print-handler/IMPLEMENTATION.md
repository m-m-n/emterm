# Implementation Plan: WASM Print Handler (Sprint 2)

## Overview

Port `handlePrintDispatch()` and `flushGraphemeBuffer()` from TypeScript to Rust/WASM, moving grapheme buffering, wrapPending, charSet state, and DEC Line Drawing translation into TerminalCore. Also address Sprint 1 carry-over items (dispose, wasmRowToLine optimization).

## Objectives

- Eliminate JS-WASM boundary cost for the hottest code path (print, 80%+ of actions)
- Move print-related state (graphemeBuffer, wrapPending, charSet) to WASM
- Add resource cleanup (dispose) and optimize row conversion (wasmRowToLine)
- Maintain full backward compatibility via JS fallback

## Prerequisites

### Development Environment

- Rust toolchain with `wasm-pack` (existing from Sprint 0)
- Bun package manager (existing)
- Docker for testing (existing `docker-compose.e2e.yml`)

### Dependencies

- No new crate or npm dependencies
- Sprint 1 TerminalCore (commit 55733ee) must be complete

### Knowledge Requirements

- `wasm/src/unicode.rs` internal API: `char_width()`, `classify_codepoint()`, `is_emoji_presentation()`, constants (`COMBINING`, `EXT_PICTOGRAPHIC`, `REGIONAL_IND`, `SKIN_TONE`, `VARIATION_SEL`)
- TerminalCore cell write operations: `set_cell()`, `set_cell_ascii()`, `mark_row_dirty()`
- UnifiedBuffer scroll region and scrollUp semantics

## Architecture Overview

### Design Approach

**Return-value scroll bridge**: `handle_print(cp) -> u8` returns the number of scrollUp operations needed. The TS caller performs scrollUp (which involves WASM→JS Line conversion for scrollback). This keeps WASM free of JS callbacks and maintains a simple call flow.

**State ownership boundary**: Print-related state (graphemeBuffer, wrapPending, charSet, scrollRegion) lives in WASM. TS handlers that change these states (ESC charset, CSI DECSTBM) sync via setter calls.

### Component Interaction

```
TS processAction("Print")
  → core.handle_print(cp) [WASM]
  → returns scroll_count
  → TS: for i in 0..scroll_count: buffer.scrollUp()

WASM handle_print internally:
  → classify_codepoint(cp) [internal Rust call]
  → grapheme buffer management [internal state]
  → char_width(cp) [internal Rust call]
  → translate_charset(cp) [internal lookup]
  → grid write + cursor advance + dirty mark [direct memory]
  → line_feed() if wrap → returns scroll needed flag
```

## Implementation Phases

### Phase 1: Rust Print Core Logic

**Goal**: Implement handle_print, flush_grapheme_buffer, charSet, and scroll region in Rust with full unit test coverage

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add new fields (grapheme_buffer, wrap_pending, g0_charset, g1_charset, active_charset, scroll_region_top, scroll_region_bottom)
  - Add handle_print, flush_grapheme_buffer, and all internal helper functions
  - Add charset getter/setter, scroll region getter/setter, wrap_pending getter/setter
  - Add DEC Line Drawing translation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_print | Route codepoint through grapheme buffering → print path | Valid u32 codepoint | Cell written, cursor advanced, scroll_count returned |
| flush_grapheme_buffer | Convert buffered codepoints to cluster string → write to grid | Buffer may be empty | Buffer cleared, cell written if non-empty |
| handle_print_ascii | ASCII fast path: direct grid byte write | cp in 0x20-0x7E, G0/Ascii, no wrap_pending | Cell written, cursor advanced; wrap_pending set if at last column with autoWrap |
| handle_print_slow | General path: charWidth + charset + wrap | Any printable codepoint | Cell written with correct width/translation |
| write_grapheme_to_grid | Write multi-char string to grid at cursor | Valid cluster string + width | Cell + optional placeholder written |
| line_feed | Advance cursor row, detect scroll need | Cursor at some row | Row advanced, returns true if scroll_region_bottom exceeded |
| carriage_return | Reset cursor column to 0 | Any state | cursor.col = 0 |
| translate_charset | Apply active charset translation | Codepoint + charset state | Translated codepoint (or original) |
| translate_line_drawing | DEC Line Drawing map (0x5F-0x7E) | Codepoint in range | Mapped Unicode box-drawing codepoint |

**Processing Flow (handle_print)**:
```
1. Safety check: grapheme_buffer.len >= 64 → flush
2. classify_codepoint(cp) → props byte
3. If grapheme_buffer non-empty:
   ├─ cp extends cluster (ZWJ/VS/SkinTone/Combining/RI-pair/EP-after-ZWJ) → push, return
   └─ cp does not extend → flush buffer, fall through
4. If grapheme_buffer empty:
   ├─ cp starts buffering (EXT_PICTOGRAPHIC | REGIONAL_IND) → push, return
   └─ Otherwise → fall through
5. If ASCII fast path eligible (0x20-0x7E, G0, Ascii, no wrap_pending):
   └─ handle_print_ascii
6. Else:
   └─ handle_print_slow (charWidth, charset, wrap handling)
```

**Processing Flow (flush_grapheme_buffer)**:
```
1. If buffer empty → return 0
2. Build cluster string from codepoints
3. Determine width:
   ├─ Has FE0E → width = 1
   ├─ Has FE0F → width = 2
   ├─ Single codepoint → is_emoji_presentation check
   └─ Multi-codepoint → width = 2
4. Clear buffer
5. write_grapheme_to_grid(cluster_string, width) → scroll_count
```

**Processing Flow (write_grapheme_to_grid)**:
```
1. If wrap_pending:
   ├─ carriage_return + line_feed
   ├─ If scroll needed → increment scroll_count
   └─ Mark line as wrapped
2. If width=2 and cursor.col >= cols-1 and autoWrap:
   ├─ carriage_return + line_feed
   ├─ If scroll needed → increment scroll_count
   └─ Mark line as wrapped
3. Write cell to grid at cursor position
4. If width=2 and cursor.col < cols-1:
   └─ Write placeholder cell (width=0, empty char) at cursor.col+1
5. Advance cursor:
   ├─ newCol = cursor.col + width
   ├─ If newCol >= cols and autoWrap → set wrap_pending, cursor.col = cols-1
   └─ Else → cursor.col = newCol
6. Return scroll_count
```

**Implementation Steps**:

1. **Add new fields to TerminalCore**
   - grapheme_buffer: Vec<u32> (initialized empty)
   - wrap_pending: bool (initialized false)
   - g0_charset/g1_charset: u8 (initialized 0 = Ascii)
   - active_charset: u8 (initialized 0 = G0)
   - scroll_region_top/bottom: u16 (initialized 0, rows-1)
   - Update `new()` and `reset()` to initialize these fields

2. **Implement internal helpers**
   - `carriage_return()`, `line_feed() -> bool`
   - `translate_charset()`, `translate_line_drawing()`
   - `write_grapheme_to_grid()` with wrap and scroll logic
   - `handle_print_ascii()` and `handle_print_slow()`
   - Key considerations:
     - `line_feed` compares cursor.row against scroll_region_bottom
     - `handle_print_ascii` writes byte directly to `cell.char_data[0]` (same pattern as existing `set_cell_ascii`)
     - `handle_print_slow` calls `char_width(cp)` and `translate_charset(cp)` internally

3. **Implement handle_print and flush_grapheme_buffer**
   - Grapheme buffering logic matching TS behavior exactly
   - Return scroll_count (u8) from both functions
   - Key considerations:
     - `classify_codepoint()` called as `crate::unicode::classify_codepoint(cp)`
     - Grapheme buffer safety limit at 64
     - `flush_grapheme_buffer` builds cluster via `char::from_u32` and `String::push`

4. **Implement wasm_bindgen exports**
   - handle_print, flush_grapheme_buffer
   - Charset getters/setters (g0, g1, active)
   - Scroll region getter/setter
   - wrap_pending getter/setter
   - get_grapheme_buffer_len, clear_grapheme_buffer

5. **Write Rust unit tests**
   - ASCII print: cell content, cursor position, scroll return
   - CJK: width-2 cell + placeholder
   - Grapheme: buffer → flush → cell
   - DEC Line Drawing: all 32 entries
   - Wrap: wrapPending, autoWrap ON/OFF
   - Scroll region: LF at region bottom
   - Edge cases: 1-column, buffer overflow

**Dependencies**:
- Requires: Sprint 1 TerminalCore (complete)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:
- handle_print ASCII round-trip
- handle_print CJK with placeholder
- Grapheme buffer extend/flush sequences
- DEC Line Drawing all 32 entries
- wrap_pending → scroll behavior
- Scroll region boundary behavior

**Acceptance Criteria**:
- [ ] All Rust unit tests pass (`cargo test`)
- [ ] `wasm-pack build` succeeds
- [ ] DEC Line Drawing table matches TS `translateLineDrawing` output for all entries

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: Grapheme buffering behavior diverges from TS
  - **Mitigation**: Port TS logic line-by-line, cross-validate with existing test cases
- **Risk**: scroll_region interaction with line_feed edge cases
  - **Mitigation**: Comprehensive unit tests for region boundary behavior

---

### Phase 2: TypeScript Integration

**Goal**: Wire WASM handle_print into the TS dispatch pipeline with JS fallback

**Files to Modify**:
- `src/terminal/handlers/print_handler.ts`:
  - Add WASM dispatch path alongside existing JS fallback
- `src/terminal/state.ts`:
  - `flushGraphemeBuffer()`: delegate to WASM when available
  - `processAction()`: use WASM flush before non-Print actions
  - Charset property setters: sync to WASM
  - `reset()`: sync new state to WASM
- `src/terminal/unified-buffer.ts`:
  - `setScrollRegion()`: sync to WASM via `core.set_scroll_region()`
  - `clearScrollRegion()`: sync to WASM
- `src/terminal/handlers/types.ts`:
  - No changes needed (wasmGrid access via `state.getActiveBuffer()` pattern)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handlePrint (TS) | Dispatch to WASM or JS fallback | processAction called with "Print" | Character printed, scroll performed if needed |
| flushGraphemeBuffer (TS) | Delegate to WASM or use existing JS | Buffer may have content | Buffer flushed, scroll performed if needed |
| charSet sync | Sync TS charset changes to WASM | ESC handler changes charset | WASM charset state matches TS |
| scrollRegion sync | Sync scroll region changes to WASM | CSI DECSTBM sets region | WASM scroll_region matches buffer |

**Processing Flow (TS handlePrint with WASM)**:
```
1. Get codepoint from char
2. If wasmGrid exists:
   ├─ scroll_count = core.handle_print(cp)
   └─ For each scroll: buffer.scrollUp()
3. Else:
   └─ handlePrintDispatch(state, char) [existing JS]
```

**Implementation Steps**:

1. **Modify print_handler.ts**
   - Add WASM-aware `handlePrint` wrapper function
   - Keep existing `handlePrintDispatch` as JS fallback
   - WASM path: single call + scrollUp loop
   - Key consideration: existing tests call `handlePrintDispatch` directly; add new tests for `handlePrint` with WASM path

2. **Modify state.ts flushGraphemeBuffer**
   - Check wasmGrid existence
   - WASM path: `core.flush_grapheme_buffer()` → scrollUp loop
   - JS path: existing implementation
   - Key consideration: `processAction` already calls `flushGraphemeBuffer` before non-Print actions; ensure WASM path is used

3. **Add charset sync to state.ts**
   - Setter hooks on g0CharSet, g1CharSet, activeCharSet
   - Map CharSet type to u8 (Ascii=0, DecLineDrawing=1)
   - Call WASM setters when wasmGrid exists
   - Key consideration: `reset()` must sync ALL WASM state:
     - `core.set_g0_charset(0)`, `core.set_g1_charset(0)`, `core.set_active_charset(0)`
     - `core.set_wrap_pending(false)`
     - `core.set_scroll_region(0, rows - 1)`
     - `core.clear_grapheme_buffer()`

4. **Add scroll region sync to unified-buffer.ts**
   - `setScrollRegion()`: also call `core.set_scroll_region(top, bottom)`
   - `clearScrollRegion()`: also call `core.set_scroll_region(0, rows-1)`
   - Key consideration: resize resets scroll region; ensure WASM sync on resize path

**Dependencies**:
- Requires: Phase 1 (Rust implementation complete)
- Blocks: Phase 3, Phase 5

**Testing Approach**:

*Integration Tests (TypeScript)*:
- handlePrint WASM path: ASCII, CJK, emoji
- flushGraphemeBuffer WASM delegation
- charSet sync: set in TS, verify behavior in print output
- scrollRegion sync: set region, verify LF boundary

*Regression*:
- All existing print_handler.test.ts cases pass
- All existing state tests pass

**Acceptance Criteria**:
- [ ] WASM print path produces identical output to JS path for all existing test cases
- [ ] JS fallback works when wasmGrid is null
- [ ] Charset changes in TS are reflected in WASM print behavior
- [ ] Scroll region changes are synced to WASM

**Estimated Effort**: 小 (1-2 days)

---

### Phase 3: Sprint 1 Carry-over (dispose + wasmRowToLine)

**Goal**: Add WasmGrid resource cleanup and optimize row-to-line conversion

**Files to Modify**:
- `src/terminal/wasm/terminal-core.ts`:
  - `wasmRowToLine()`: rewrite to use `get_row_packed()` binary parsing
  - `WasmGrid.dispose()`: already exists (calls `core.free()`), verify correctness
- `src/terminal/state.ts`:
  - Ensure `dispose()` is called on terminal close and buffer switch

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| wasmRowToLine (optimized) | Parse get_row_packed binary into JS Line | Valid row index | JS Line with identical data to per-cell version |
| dispose | Free WASM TerminalCore resources | WasmGrid exists | Memory freed, no dangling references |

**Processing Flow (wasmRowToLine optimized)**:
```
1. Call core.get_row_packed(row) → Uint8Array (single WASM call)
2. For each column, parse binary format:
   ├─ Read char_len byte
   │   ├─ 0xFF → overflow: read u16 byte count, then UTF-8 data
   │   ├─ 0 → empty cell (" ")
   │   └─ 1-16 → inline UTF-8 data
   ├─ Read width byte
   ├─ Read fg (4 bytes: tag, r, g, b)
   ├─ Read bg (4 bytes: tag, r, g, b)
   └─ Read flags (2 bytes, little-endian)
3. Construct JS Cell and set on Line
4. Copy wrapped flag
```

**Implementation Steps**:

1. **Rewrite wasmRowToLine**
   - Parse binary format from get_row_packed
   - Use TextDecoder for UTF-8 → string conversion
   - Key considerations:
     - Reuse single TextDecoder instance for performance
     - Handle overflow marker (0xFF) with u16 byte count prefix
     - Verify identical output via cross-validation test

2. **Verify dispose**
   - WasmGrid.dispose() already calls core.free()
   - Add call sites in state.ts for terminal close and alternate buffer teardown
   - Key consideration: ensure dispose is not called on active grid

**Dependencies**:
- Requires: Phase 1 (for any new WASM API if needed)
- Can run in parallel with Phase 2 (independent changes)

**Testing Approach**:

*Cross-Validation Tests*:
- wasmRowToLine: compare output of optimized version vs original per-cell version for various content (ASCII, CJK, emoji, overflow)

*Unit Tests*:
- dispose: verify no errors on free

**Acceptance Criteria**:
- [ ] wasmRowToLine produces identical output to per-cell version
- [ ] wasmRowToLine uses single WASM call (verify via call count or performance measurement)
- [ ] dispose frees resources without error

**Estimated Effort**: 小 (1 day)

---

### Phase 4: Verification and Benchmarking

**Goal**: Full regression, cross-validation, and performance measurement

**Files to Create**:
- None (test cases added to existing test files)

**Implementation Steps**:

1. **Run full test suite**
   - Rust tests: `cargo test`
   - TS tests: `bun test`
   - Type check: `bun run typecheck`

2. **Cross-validation tests**
   - For each existing print_handler.test.ts case: run both WASM and JS paths, compare output
   - DEC Line Drawing: all 32 entries match between Rust and TS

3. **Performance benchmark**
   - ASCII throughput: measure time for N characters via WASM vs JS
   - wasmRowToLine: measure per-cell vs get_row_packed

4. **WASM binary size check**
   - Verify total < 50KB (Sprint 1: 39.5KB, target increase < 10KB)

**Dependencies**:
- Requires: Phase 1, Phase 2, Phase 3

**Testing Approach**:

*E2E Testing (Docker)*:
- [ ] Full Rust test suite: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- [ ] Full TS test suite: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- [ ] Type check: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- [ ] WASM build: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && wasm-pack build --target web"`

*Manual Testing*:
- [ ] `bun tauri dev` → terminal works correctly with WASM print handler
- [ ] vim/nano/htop display correctly (DEC Line Drawing box drawing)
- [ ] Emoji display (flags, ZWJ family, skin tones) in terminal
- [ ] Large file output (`cat` large file) is noticeably faster

**Acceptance Criteria**:
- [ ] All Rust tests pass
- [ ] All TS tests pass (1822+)
- [ ] Type check passes
- [ ] WASM binary < 50KB
- [ ] ASCII throughput >= 2x Sprint 1 baseline
- [ ] vttest basic tests unchanged

**Estimated Effort**: 小 (1 day)

---

## Complete File Structure

```
wasm/src/
├── lib.rs              # MODIFIED: minor exports if needed
├── unicode.rs          # UNCHANGED
├── terminal_core.rs    # MODIFIED: +~400 lines (handle_print, grapheme, charset, scroll_region)
└── cell.rs             # UNCHANGED

src/terminal/wasm/
├── loader.ts           # UNCHANGED
├── unicode.ts          # UNCHANGED
└── terminal-core.ts    # MODIFIED: wasmRowToLine rewrite, dispose verification

src/terminal/
├── state.ts            # MODIFIED: flushGraphemeBuffer delegation, charset sync, scroll region sync
├── unified-buffer.ts   # MODIFIED: scroll region sync to WASM
├── handlers/
│   ├── print_handler.ts      # MODIFIED: WASM dispatch wrapper
│   └── print_handler.test.ts # MODIFIED: add WASM path tests
└── (others unchanged)
```

**File Descriptions**:

| File | Changes | Estimated Lines Changed |
|------|---------|------------------------|
| `wasm/src/terminal_core.rs` | Add handle_print, grapheme buffer, charset, scroll region, tests | +400 |
| `src/terminal/handlers/print_handler.ts` | Add WASM dispatch, keep JS fallback | +20, ~5 modified |
| `src/terminal/state.ts` | flushGraphemeBuffer WASM, charset sync, reset sync | +30, ~15 modified |
| `src/terminal/unified-buffer.ts` | Scroll region sync to WASM | +10, ~5 modified |
| `src/terminal/wasm/terminal-core.ts` | wasmRowToLine rewrite | ~60 modified |

## Testing Strategy

### Unit Testing (Rust)

**Test Coverage Goals**: 90%+ for new code in terminal_core.rs

| Category | Test Count | Key Scenarios |
|----------|------------|---------------|
| handle_print ASCII | 5+ | Basic, cursor advance, wrap, scroll, autoWrap OFF |
| handle_print CJK | 3+ | Width-2, placeholder, line-end wrap |
| handle_print emoji | 6+ | Buffer, ZWJ extend, RI pair, VS, flush trigger |
| flush_grapheme_buffer | 5+ | Empty, single, ZWJ seq, flag, with scroll |
| DEC Line Drawing | 2+ | All 32 entries, inactive charset |
| Charset | 3+ | G0/G1 switch, active switch |
| Scroll region | 3+ | LF at bottom, within region, full screen |
| Edge cases | 3+ | 1-column, buffer overflow, NUL |

### Integration Testing (TypeScript)

| Category | Test Count | Key Scenarios |
|----------|------------|---------------|
| WASM print path | 5+ | ASCII, CJK, emoji, DEC Line, autoWrap |
| JS fallback | 1+ | wasmGrid null |
| Cross-validation | 3+ | WASM vs JS identical output |
| wasmRowToLine | 3+ | ASCII, CJK, overflow |
| Regression | 1822+ | All existing tests pass |

### E2E Testing (Docker)

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `bun test`
- [ ] `bun run typecheck`
- [ ] `wasm-pack build --target web` (from wasm/ directory)

### Manual Testing

- [ ] Terminal display with WASM print handler (`bun tauri dev`)
- [ ] vim/nano border display (DEC Line Drawing)
- [ ] Emoji display (flags, ZWJ, skin tones)
- [ ] Large output performance (`cat` large file)

## Dependencies

### External Dependencies

No new dependencies.

### Internal Dependencies

**Implementation Order**:
1. Phase 1 (Rust core) - no dependencies
2. Phase 2 (TS integration) - depends on Phase 1
3. Phase 3 (carry-over) - can run in parallel with Phase 2
4. Phase 4 (verification) - depends on Phase 1, 2, 3

**Component Dependencies**:
- `handle_print` depends on `crate::unicode::{char_width, classify_codepoint, is_emoji_presentation}` (existing)
- `state.ts` charset sync depends on WASM charset setter exports (Phase 1)
- `unified-buffer.ts` scroll region sync depends on WASM scroll region setter (Phase 1)

## Risk Assessment

### Technical Risks

1. **Grapheme buffering behavior divergence**
   - **Risk**: WASM grapheme handling differs subtly from TS
   - **Likelihood**: Medium
   - **Impact**: High (emoji rendering bugs)
   - **Mitigation**: Line-by-line port from TS, cross-validation tests comparing WASM and JS output

2. **Scroll region interaction complexity**
   - **Risk**: line_feed + scroll_region + wrapPending edge cases
   - **Likelihood**: Low
   - **Impact**: Medium (display glitches)
   - **Mitigation**: Comprehensive unit tests for region boundary behavior

3. **State sync timing**
   - **Risk**: TS charset/scrollRegion changes not synced before handle_print
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Sync in setters (immediate sync, not deferred)

## Performance Considerations

1. **ASCII fast path**: Direct byte write to `cell.char_data[0]` without string allocation (same as existing `set_cell_ascii`)
2. **Internal function calls**: `char_width`, `classify_codepoint`, `is_emoji_presentation` are Rust function calls, no JS-WASM boundary
3. **Grapheme buffer**: `Vec<u32>` in WASM linear memory, no JS array allocation
4. **wasmRowToLine**: Single `get_row_packed` call replaces cols*5 individual WASM calls

## Open Questions

### From Specification:
- [ ] scroll_region stored in WASM vs passed as argument: **Decided** → stored in WASM (sync via setter)

### Implementation-Specific:
- (None - all key decisions made during spec phase)

## References

- **Specification**: `doc/tasks/wasm-print-handler/SPEC.md`
- **Sprint 1 SPEC**: `doc/tasks/wasm-terminal-core/SPEC.md`
- **WASM Roadmap**: `tmp/wasm.md`
- **Current TS print handler**: `src/terminal/handlers/print_handler.ts`
- **WASM Unicode module**: `wasm/src/unicode.rs`
- **TerminalCore (Rust)**: `wasm/src/terminal_core.rs`
