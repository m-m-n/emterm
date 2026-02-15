# Implementation Plan: WASM C0 + CSI Cursor + CSI Screen Handlers (Sprint 3)

## Overview

Port C0 control character handlers, CSI cursor movement handlers, and CSI screen erase handlers from TypeScript to Rust/WebAssembly. This is Sprint 3 of the WASM migration roadmap, building on Sprint 1 (TerminalCore data layer) and Sprint 2 (Print handler). Combined with Sprint 2, this brings 95%+ of all terminal actions under WASM processing.

## Objectives

- Move C0 control handlers (BEL, BS, HT, LF/VT/FF, CR, SO, SI) to WASM
- Move CSI cursor handlers (CUU/CUD/CUF/CUB/CNL/CPL/CHA/CUP/VPA) to WASM
- Move CSI screen handlers (ED, EL, ECH) to WASM
- Achieve 95%+ terminal actions processing in WASM

## Prerequisites

### Development Environment
- Rust toolchain with `wasm32-unknown-unknown` target
- wasm-pack
- Bun runtime

### Dependencies
- No new crate or npm dependencies required
- All state fields (cursor, tab_stops, wrap_pending, scroll_region, active_charset) already exist in TerminalCore from Sprint 1-2

### Knowledge Requirements
- Existing TerminalCore data layout and methods
- ANSI terminal semantics for C0 and CSI sequences

## Architecture Overview

### Technology Stack
- **Language**: Rust (WASM crate) + TypeScript (frontend)
- **Build**: wasm-pack (Rust to WASM), Bun (TypeScript)
- **Key Libraries**:
  - wasm_bindgen - Rust/JS interop

### Design Approach

Bottom-up extension of existing TerminalCore. Each handler category (C0, CSI cursor, CSI screen) is added as methods on the existing `TerminalCore` struct. The TypeScript `processAction()` dispatch is modified to route qualifying actions to WASM when available, falling back to existing TS handlers when WASM is unavailable.

### Component Interaction

```
TS processAction()
  |
  +-- Execute action --> getActiveWasmGrid() --> core.handle_execute(byte) --> scroll bridge / BEL dispatch
  |                      |-- null --> handleExecute() (TS fallback, unchanged)
  |
  +-- CSI action ------> getActiveWasmGrid() --> handleCsiWasm(grid, action) --> WASM methods
  |                      |-- null or unhandled --> handleCsi() (TS fallback, unchanged)
  |
  +-- Print action ----> (unchanged Sprint 2 WASM path)
  +-- Esc/Osc/etc -----> (unchanged TS handlers)
```

## Implementation Phases

### Phase 1: Rust C0 Handler + BEL Sentinel

**Goal**: Implement `handle_execute` in Rust with BEL sentinel return value. All C0 control codes processed in a single WASM call.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add `handle_execute(byte: u8) -> u8` public method
  - Add `find_next_tab_stop() -> u16` internal method
  - Add `execute_line_feed() -> u8` internal helper
  - Add Rust unit tests for all C0 cases

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_execute | Dispatch C0 byte to appropriate handler, return scroll count or sentinel | byte is a C0 control code (0x00-0x1F) | Cursor/state updated, scroll count (0-N) or 0xFE (BEL) returned |
| find_next_tab_stop | Locate next tab stop column after current cursor position | tab_stops populated, cursor.col valid | Returns next tab stop column or cols-1 |
| execute_line_feed | Call line_feed() and clear wrap_pending | Cursor at valid row | Row advanced or scroll flag returned, wrap_pending cleared |

**Processing Flow**:
```
handle_execute(byte):
1. Match byte
   +-- 0x07 (BEL) --> return 0xFE (BEL sentinel, TS dispatches onBell)
   +-- 0x08 (BS) --> decrement cursor.col (clamped to 0), clear wrap_pending, return 0
   +-- 0x09 (HT) --> find_next_tab_stop(), set cursor.col, clear wrap_pending, return 0
   +-- 0x0A/0x0B/0x0C (LF/VT/FF) --> execute_line_feed(), return scroll count
   +-- 0x0D (CR) --> set cursor.col = 0, clear wrap_pending, return 0
   +-- 0x0E (SO) --> set active_charset = 1, return 0
   +-- 0x0F (SI) --> set active_charset = 0, return 0
   +-- other --> no-op, return 0
```

**Implementation Steps**:

1. **Sentinel constants**
   - Define `const BEL_SENTINEL: u8 = 0xFE;` and `const SCROLLBACK_SENTINEL: u8 = 0xFF;` as named constants in terminal_core.rs
   - Avoids magic numbers in match arms and return values

2. **handle_execute method**
   - C0 byte dispatch with match statement
   - Key considerations:
     - BEL returns BEL_SENTINEL (no extern callback needed, fully testable in Rust)
     - LF/VT/FF all delegate to the same execute_line_feed helper
     - Unknown bytes are silently ignored (return 0)

3. **Internal helpers**
   - find_next_tab_stop: iterate tab_stops from cursor.col+1, similar to existing next_tab_stop but used internally
   - execute_line_feed: wraps line_feed() + wrap_pending clear

4. **Rust unit tests**
   - Tests for every C0 byte listed in SPEC test scenarios
   - BEL test verifies BEL_SENTINEL is returned

**Dependencies**:
- Requires: Sprint 2 complete (TerminalCore with cursor, tab_stops, wrap_pending, line_feed)
- Blocks: Phase 4 (TS integration for Execute path)

**Testing Approach**:

*Unit Tests*:
- handle_execute: BEL - returns 0xFE sentinel
- handle_execute: BS at col=5 and col=0 (clamped)
- handle_execute: HT with default/custom tab stops, boundary cases
- handle_execute: LF/VT/FF at mid-screen (return 0) and at scroll_region_bottom (return 1)
- handle_execute: CR sets col=0
- handle_execute: SO/SI switch active_charset
- handle_execute: Unknown byte no-op
- handle_execute: LF/CR clear wrapPending

**Acceptance Criteria**:
- [ ] `wasm-pack build` succeeds with handle_execute added
- [ ] All C0 Rust unit tests pass
- [ ] WASM binary size increase < 1KB from Sprint 2 baseline

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- No significant risks for Phase 1. BEL uses return value sentinel (0xFE) instead of extern callback, so all paths are fully testable in Rust.

---

### Phase 2: Rust CSI Cursor Handlers

**Goal**: Implement 9 CSI cursor movement functions in Rust. Each operation completes in 1 WASM call.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add 9 public CSI cursor handler methods
  - Add `to_zero_indexed_col` and `to_zero_indexed_row` internal helpers
  - Add Rust unit tests for all cursor operations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_cursor_up | Move cursor up by count, clamped to row 0 | count >= 0 | cursor.row decreased, wrap_pending cleared |
| handle_cursor_down | Move cursor down by count, clamped to rows-1 | count >= 0 | cursor.row increased, wrap_pending cleared |
| handle_cursor_forward | Move cursor right by count, clamped to cols-1 | count >= 0 | cursor.col increased, wrap_pending cleared |
| handle_cursor_back | Move cursor left by count, clamped to col 0 | count >= 0 | cursor.col decreased, wrap_pending cleared |
| handle_cursor_next_line | Move cursor down + set col to 0 | count >= 0 | cursor.row increased, cursor.col = 0, wrap_pending cleared |
| handle_cursor_previous_line | Move cursor up + set col to 0 | count >= 0 | cursor.row decreased, cursor.col = 0, wrap_pending cleared |
| handle_cursor_horizontal_absolute | Set cursor col from 1-indexed input | col is 1-indexed ANSI param | cursor.col set (0-indexed, clamped), wrap_pending cleared |
| handle_cursor_position | Set cursor row and col from 1-indexed inputs | row, col are 1-indexed ANSI params | cursor positioned (0-indexed, clamped), wrap_pending cleared |
| handle_cursor_vertical_absolute | Set cursor row from 1-indexed input | row is 1-indexed ANSI param | cursor.row set (0-indexed, clamped), wrap_pending cleared |
| to_zero_indexed_col | Convert 1-indexed col to 0-indexed, clamped | ANSI col parameter | 0-indexed col, clamped to 0..cols-1 |
| to_zero_indexed_row | Convert 1-indexed row to 0-indexed, clamped | ANSI row parameter | 0-indexed row, clamped to 0..rows-1 |

**Processing Flow**:
```
handle_cursor_up(count):
1. Subtract count from cursor.row using saturating subtraction
2. Clear wrap_pending
```

```
handle_cursor_position(row, col):
1. Convert row: 1-indexed to 0-indexed, clamped to 0..rows-1
2. Convert col: 1-indexed to 0-indexed, clamped to 0..cols-1
3. Set cursor position
4. Clear wrap_pending
```

**Implementation Steps**:

1. **Coordinate conversion helpers**
   - to_zero_indexed_col and to_zero_indexed_row handle 1-indexed to 0-indexed conversion with clamping
   - Key considerations:
     - Input 0 maps to index 0 (ANSI treats 0 same as 1 for positioning)
     - Maximum clamped to dimension - 1

2. **Relative movement handlers (up/down/forward/back)**
   - Use saturating arithmetic for clamping
   - All clear wrap_pending

3. **Compound movement handlers (next_line/previous_line)**
   - Combine vertical movement with carriage return

4. **Absolute positioning handlers (CHA/CUP/VPA)**
   - Apply coordinate conversion before setting cursor

**Dependencies**:
- Requires: Phase 1 complete (shared internal helpers)
- Blocks: Phase 4 (TS integration for CSI cursor path)

**Testing Approach**:

*Unit Tests*:
- Each handler: normal movement, boundary clamping, wrap_pending clearing
- CHA/CUP/VPA: 1-indexed to 0-indexed conversion with edge cases (0, 1, max, overflow)

**Acceptance Criteria**:
- [ ] All 9 CSI cursor Rust unit tests pass
- [ ] `wasm-pack build` succeeds
- [ ] WASM binary size increase < 1KB from Phase 1

**Estimated Effort**: Small (1-2 days)

---

### Phase 3: Rust CSI Screen Handlers

**Goal**: Implement ED, EL, ECH in Rust. Screen erase operations execute in 1 WASM call instead of N per-row calls.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add `handle_erase_in_display(mode: u8) -> u8`
  - Add `handle_erase_in_line(mode: u8)`
  - Add `handle_erase_characters(count: u16)`
  - Add Rust unit tests for all erase operations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_erase_in_display | Erase display region by mode | mode is 0-3 | Cells cleared with Cell::EMPTY, dirty rows marked. Returns 0xFF for Scrollback |
| handle_erase_in_line | Erase current line region by mode | mode is 0-2 | Cells on current row cleared, dirty marked |
| handle_erase_characters | Erase N characters at cursor position | count >= 1, cursor valid | count cells cleared from cursor position, clamped to end of line |

**Processing Flow**:
```
handle_erase_in_display(mode):
1. Match mode
   +-- 0 (Below): clear_line_range(cursor.row, cursor.col, cols), then clear_line for each row below
   +-- 1 (Above): clear_line for each row above, then clear_line_range(cursor.row, 0, cursor.col + 1)
   +-- 2 (All): clear_line for all rows
   +-- 3 (Scrollback): return 0xFF sentinel (TS handles scrollback)
   +-- other: no-op, return 0
2. Return 0
```

```
handle_erase_in_line(mode):
1. Match mode
   +-- 0 (ToEnd): clear_line_range(row, cursor.col, cols)
   +-- 1 (ToStart): clear_line_range(row, 0, cursor.col + 1)
   +-- 2 (All): clear_line(row)
   +-- other: no-op
```

```
handle_erase_characters(count):
1. Compute end = min(cursor.col + count, cols)
2. clear_line_range(cursor.row, cursor.col, end)
```

**Implementation Steps**:

1. **handle_erase_in_display**
   - Reuse existing clear_line and clear_line_range methods
   - Key considerations:
     - Mode 3 (Scrollback) returns 0xFF sentinel to signal TS fallback
     - ED(1) Above: must include current row cells up to and including cursor column

2. **handle_erase_in_line**
   - Delegate to clear_line_range for partial clears
   - Key considerations:
     - Mode 1 (ToStart) is inclusive of cursor column

3. **handle_erase_characters**
   - Clear N cells from cursor position, clamped to end of line
   - Key considerations:
     - Count overflow past cols is clamped

**Dependencies**:
- Requires: Phase 1 complete (clear_line/clear_line_range already exist)
- Blocks: Phase 4 (TS integration for CSI screen path)

**Testing Approach**:

*Unit Tests*:
- ED: Below, Above, All, Scrollback (0xFF return), dirty marking
- EL: ToEnd, ToStart, All, dirty marking
- ECH: Normal erase, overflow clamping, dirty marking

**Acceptance Criteria**:
- [ ] All erase Rust unit tests pass
- [ ] `wasm-pack build` succeeds
- [ ] WASM binary size increase < 1KB from Phase 2
- [ ] ED Scrollback returns 0xFF sentinel

**Estimated Effort**: Small (1-2 days)

---

### Phase 4: TypeScript Integration

**Goal**: Wire WASM handlers into TS processAction dispatch. Handle BEL sentinel. Maintain JS fallback path.

**Files to Modify**:
- `src/terminal/state.ts`:
  - Add WASM dispatch path in processAction for Execute actions (with BEL sentinel and scroll bridge)
  - Add `handleCsiWasm()` private method for CSI WASM dispatch
  - Add `eraseModeToByte()` helper function

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| processAction (Execute path) | Route Execute to WASM or TS fallback | Action type is Execute | C0 processed via WASM (with scroll bridge and BEL dispatch) or TS fallback |
| handleCsiWasm | Attempt WASM dispatch for CSI cursor/screen actions | Active WASM grid available | Returns true if handled by WASM, false for TS fallback |
| eraseModeToByte | Convert EraseMode string to numeric mode | EraseMode string | Numeric mode (0-3) |

**Processing Flow**:
```
processAction(action):
1. Flush grapheme buffer if non-Print action (unchanged)
2. Switch on action.type:
   +-- "Print": (unchanged Sprint 2 WASM path)
   +-- "Execute":
       +-- getActiveWasmGrid() available:
           1. result = core.handle_execute(byte)
           2. if result == 0xFE: call state.onBell?.() (BEL sentinel)
           3. else if result > 0: call buffer.scrollUp() for each scroll
       +-- null: handleExecute() TS fallback (unchanged)
   +-- "Csi":
       +-- getActiveWasmGrid() available AND handleCsiWasm() returns true: done
       +-- else: handleCsi() TS fallback (unchanged)
   +-- other: unchanged
```

```
handleCsiWasm(grid, action):
1. Switch on action.action:
   +-- CursorUp/Down/Forward/Back/NextLine/PreviousLine/CHA/CUP/VPA:
       Call corresponding grid.core.handle_cursor_*(), return true
   +-- EraseInDisplay:
       Convert mode to byte, call handle_erase_in_display()
       If result == 0xFF: call buffer.clearScrollback() directly, return true
       Note: Do NOT fall back to existing handleEraseInDisplay() for Scrollback,
             because the existing TS handler incorrectly calls clearAll() instead of
             clearScrollback(). The WASM dispatch path must call buffer.clearScrollback()
             directly to avoid this bug.
       return true
   +-- EraseInLine:
       Convert mode to byte, call handle_erase_in_line(), return true
   +-- EraseCharacters:
       Call handle_erase_characters(), return true
   +-- default: return false (not handled by Sprint 3)
```

**Implementation Steps**:

1. **Sentinel constants (TypeScript)**
   - Define `const WASM_BEL_SENTINEL = 0xFE;` and `const WASM_SCROLLBACK_SENTINEL = 0xFF;` in state.ts
   - Use these constants in all comparisons (not magic numbers)

2. **processAction Execute WASM path**
   - Add WASM grid check and dispatch in Execute case
   - Key considerations:
     - BEL sentinel (WASM_BEL_SENTINEL): invoke state.onBell?.() in TS
     - Scroll bridge: convert WASM scroll count to buffer.scrollUp() calls
     - Fallback to existing handleExecute when WASM unavailable

3. **handleCsiWasm dispatcher**
   - New private method on TerminalState
   - Key considerations:
     - CSI actions not covered by Sprint 3 return false for TS fallback
     - EraseInDisplay Scrollback (WASM_SCROLLBACK_SENTINEL): call buffer.clearScrollback() directly in the WASM dispatch path. Do NOT fall back to the existing TS handleEraseInDisplay(), which has a known bug calling clearAll() instead of clearScrollback()
     - Default parameter handling (count || 1) done in TS before WASM call

4. **eraseModeToByte helper**
   - Convert EraseMode string union to numeric byte
   - Key considerations:
     - Below=0, Above=1, All=2, Scrollback=3

**Dependencies**:
- Requires: Phases 1-3 complete (all Rust handlers built into WASM)
- Blocks: Phase 5 (verification)

**Testing Approach**:

*Integration Tests*:
- Execute LF via WASM: cursor moves down, scrollUp triggered at bottom
- Execute CR, BS, HT, BEL, SO/SI via WASM
- CSI cursor operations via WASM: position, movement, clamping
- CSI screen operations via WASM: ED, EL, ECH
- ED Scrollback: verify 0xFF returned and buffer.clearScrollback() called directly (NOT clearAll)

*Regression Tests*:
- All existing c0_handlers, csi_cursor, csi_screen tests pass unchanged
- All existing print_handler tests pass (no Sprint 2 regression)

**Acceptance Criteria**:
- [ ] All existing TypeScript tests pass (1824+)
- [ ] BEL sentinel (0xFE) correctly triggers onBell in TS
- [ ] ED Scrollback returns 0xFF, WASM dispatch calls buffer.clearScrollback() directly
- [ ] WASM path produces same results as TS path for all C0/CSI cases

**Estimated Effort**: Medium (3-5 days)

---

### Phase 5: Verification and Regression Testing

**Goal**: Full regression test, cross-validation, binary size verification, and smoke test.

**Files to Modify**: None (verification only)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cross-validation | Verify WASM and TS produce identical results | Both paths functional | Behavioral parity confirmed |
| Binary size check | Verify WASM binary < 50KB total | WASM built | Size within budget |
| Smoke test | Verify working terminal in dev mode | All phases complete | Terminal operational |

**Implementation Steps**:

1. **Run all test suites**
   - Rust: cargo test
   - TypeScript: bun test
   - Type check: bun run typecheck

2. **WASM binary size verification**
   - Check wasm/pkg/*.wasm file size
   - Must be < 50KB total (Sprint 2 baseline ~43.6KB, budget +5KB)

3. **Cross-validation**
   - Verify C0/CSI operations produce identical grid state via WASM and TS paths
   - Compare cursor position, cell contents, dirty rows

4. **Smoke test**
   - Run `bun tauri dev`
   - Verify terminal is functional with WASM handlers active
   - Test common sequences: typing, cursor movement, screen clear

**Dependencies**:
- Requires: All previous phases complete

**Testing Approach**:

*E2E Testing (Docker)*:
- Full test suite execution
- WASM build verification
- Binary size check

*Manual Testing*:
- `bun tauri dev` smoke test
- vttest basic tests

**Acceptance Criteria**:
- [ ] `wasm-pack build` succeeds
- [ ] WASM binary size < 50KB total
- [ ] All Rust unit tests pass (Sprint 1-2 + Sprint 3)
- [ ] All TypeScript tests pass (1824+)
- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests unchanged

**Estimated Effort**: Small (1-2 days)

---

## Complete File Structure

```
wasm/src/
  lib.rs              # UNCHANGED
  terminal_core.rs    # MODIFIED: +handle_execute, +CSI cursor, +CSI screen handlers
  cell.rs             # UNCHANGED
  unicode.rs          # UNCHANGED

src/terminal/
  state.ts            # MODIFIED: WASM paths for Execute/CSI in processAction
  wasm/
    loader.ts         # UNCHANGED
    terminal-core.ts  # UNCHANGED
    unicode.ts        # UNCHANGED
  handlers/
    c0_handlers.ts    # UNCHANGED (JS fallback)
    csi_cursor.ts     # UNCHANGED (JS fallback)
    csi_screen.ts     # UNCHANGED (JS fallback)
    index.ts          # UNCHANGED
    types.ts          # UNCHANGED
    semantics.ts      # UNCHANGED
    (others)          # UNCHANGED

doc/tasks/wasm-c0-csi-handlers/
  SPEC.md
  IMPLEMENTATION.md
  VERIFICATION.md
```

**File Descriptions**:
- `wasm/src/lib.rs` - WASM crate entry point; unchanged
- `wasm/src/terminal_core.rs` - Core WASM terminal state; gains C0, CSI cursor, CSI screen handler methods
- `src/terminal/state.ts` - TS action dispatcher; adds WASM routing for Execute and CSI actions
- `src/terminal/wasm/loader.ts` - WASM initialization; unchanged
- `src/terminal/wasm/terminal-core.ts` - WASM grid adapter; unchanged
- All `src/terminal/handlers/*.ts` - Remain unchanged as JS fallback path

## Testing Strategy

### Unit Testing

**Approach**:
- Rust (WASM crate): `cargo test --manifest-path wasm/Cargo.toml` for Sprint 3 unit tests
- Rust (Tauri): `cargo test --manifest-path src-tauri/Cargo.toml` for backend regression
- TypeScript: `bun test` for integration tests

**Test Coverage Goals**:
- C0 handlers (Rust): 90%+ coverage
- CSI cursor handlers (Rust): 90%+ coverage
- CSI screen handlers (Rust): 90%+ coverage
- TS integration layer: 80%+ coverage

**Key Test Areas**:
1. **C0 dispatch** - Every handled byte, boundary clamping, scroll return
2. **CSI cursor** - Movement, clamping, 1-indexed conversion, wrap_pending clear
3. **CSI screen** - Erase modes, boundary handling, dirty marking, sentinel return

### Integration Testing

**Scenarios**:
1. Execute action routed through WASM produces correct cursor/grid state
2. CSI cursor actions produce same result as TS handlers
3. CSI screen erase produces same grid state as TS handlers
4. ED Scrollback returns 0xFF and WASM dispatch calls buffer.clearScrollback() directly
5. WASM unavailable: all paths fall back to TS handlers

### E2E Testing (Docker)

- [ ] Full Rust test suite passes in Docker
- [ ] Full TypeScript test suite passes in Docker
- [ ] WASM build succeeds in Docker
- [ ] Type checking passes in Docker

### Manual Testing (E2E Not Possible)

- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests produce expected results
- [ ] Typing, cursor movement, screen clear behave correctly

## Dependencies

### External Dependencies

No new dependencies.

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| wasm-pack | existing | Rust to WASM build | already installed |
| wasm_bindgen | existing | Rust/JS interop | already in Cargo.toml |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: C0 handlers + BEL sentinel (depends on Sprint 2 TerminalCore)
2. Phase 2: CSI cursor handlers (depends on Phase 1 for shared helpers)
3. Phase 3: CSI screen handlers (depends on Phase 1 for clear_line/clear_line_range)
4. Phase 4: TS integration (depends on Phases 1-3 for WASM API)
5. Phase 5: Verification (depends on Phase 4)

**Component Dependencies**:
- handle_execute depends on existing line_feed(), tab_stops, cursor state
- CSI cursor handlers depend on existing cursor fields and clamping logic
- CSI screen handlers depend on existing clear_line/clear_line_range methods
- TS integration depends on all Rust handlers being exported via wasm_bindgen
- BEL sentinel (0xFE) is checked by TS dispatch layer after handle_execute call

## Risk Assessment

### Technical Risks

1. **WASM binary size budget**
   - **Risk**: Sprint 3 additions exceed 5KB budget
   - **Likelihood**: Low (estimated ~1.5KB)
   - **Impact**: Medium
   - **Mitigation**: Monitor size after each phase; functions are simple dispatchers

2. **Behavioral parity between WASM and TS paths**
   - **Risk**: Subtle differences in clamping, indexing, or edge cases
   - **Likelihood**: Medium
   - **Impact**: High
   - **Mitigation**: Comprehensive unit tests for both paths; cross-validation in Phase 5

### Implementation Risks

1. **Scope Creep**
   - **Risk**: Adding features beyond spec
   - **Mitigation**: Stick to spec; only C0/CSI cursor/CSI screen handlers in Sprint 3

## Performance Considerations

1. **C0 handle_execute**: 1 WASM call per Execute action (vs. current TS: 2-4 WASM boundary crossings for cursor access)
2. **CSI cursor**: 1 WASM call per CSI action (vs. current TS: cursor read + compute + write + wrapPending = 4 crossings)
3. **ED/EL**: 1 WASM call per erase action (vs. current TS: N getLine+clear calls through WASM proxy)

## Open Questions

### From Specification:
- None (all requirements resolved, status: ok)

### Implementation-Specific:
- None (BEL uses return value sentinel, no extern callback concerns)

### To Clarify with User:
- None

## Future Enhancements

### From Specification (Deferred):
- Sprint 4+ handlers (SGR, modes, ESC, scroll operations)

## Success Metrics

### Functional Completeness
- [ ] All C0 controls processed via WASM
- [ ] All 9 CSI cursor operations processed via WASM
- [ ] All 3 CSI screen operations processed via WASM
- [ ] BEL sentinel (0xFE) returned and dispatched correctly
- [ ] ED Scrollback returns 0xFF, WASM dispatch calls buffer.clearScrollback() directly

### Quality Metrics
- [ ] Rust unit test coverage 90%+ for new handlers
- [ ] All existing TypeScript tests pass (1824+)
- [ ] All E2E tests pass in Docker environment

### Performance Metrics
- [ ] WASM binary size < 50KB total
- [ ] Each C0/CSI operation: 1 WASM call

### User Experience
- [ ] Terminal functions identically to pre-Sprint 3

## References

- **Specification**: `doc/tasks/wasm-c0-csi-handlers/SPEC.md`
- **Sprint 2 SPEC**: `doc/tasks/wasm-print-handler/SPEC.md`
- **WASM Roadmap**: `tmp/wasm.md`
- **Current TS implementations**:
  - `src/terminal/handlers/c0_handlers.ts`
  - `src/terminal/handlers/csi_cursor.ts`
  - `src/terminal/handlers/csi_screen.ts`
  - `src/terminal/state.ts`
- **WASM crate**: `wasm/src/terminal_core.rs`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Verify phase decomposition is appropriate
   - Confirm BEL sentinel approach
   - Address any open questions

2. **Environment Setup**
   - Verify wasm-pack build works with current codebase
   - Run existing test suites as baseline

3. **Begin Implementation**
   - Start with Phase 1 (Rust C0 handlers)
   - Follow TDD approach (write tests first)
   - Commit incrementally per phase

4. **Continuous Integration**
   - Run `cargo test` after each Rust change
   - Run `bun test` after TS integration
   - Verify WASM binary size after each phase
