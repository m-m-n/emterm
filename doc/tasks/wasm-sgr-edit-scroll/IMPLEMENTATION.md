# Implementation Plan: WASM SGR + Edit/Scroll CSI + Modes + Device Response (Sprint 4)

## Overview

Port the remaining CSI handlers (SGR, Edit, Scroll, Modes, Device) from TypeScript to Rust/WebAssembly. This is Sprint 4 of the WASM migration, achieving 100% CSI routing through WASM. Key architectural additions: batch SGR API, action code pattern for mode switching, and linear memory response buffer for device queries.

## Objectives

- Move SGR processing to WASM via batch API, eliminating `syncCursorAttrsToWasm()` from non-restore call sites
- Move CSI edit operations (IL/DL/ICH/DCH) to WASM for single-call grid manipulation
- Move CSI scroll operations (SU/SD/DECSTBM) to WASM with scroll bridge pattern for SU
- Move DEC Private Mode handling to WASM with return-value action codes
- Move device response generation (DSR/DA1/DA2) to WASM with linear memory response buffer
- Achieve 100% CSI WASM routing in `handleCsiWasm()`

## Prerequisites

### Development Environment
- Rust toolchain with `wasm32-unknown-unknown` target
- `wasm-pack` CLI
- Bun runtime
- Docker (for test execution)

### Dependencies
- No new Rust crate dependencies
- No new npm dependencies

### Knowledge Requirements
- Existing `TerminalCore` struct layout (Sprint 1-3)
- `PackedColor` encoding (tag + 3 bytes)
- Style flags bitfield (`STYLE_*` constants in `cell.rs`)
- WASM/JS boundary crossing via `wasm_bindgen`
- Existing scroll bridge pattern in `UnifiedBuffer`

## Architecture Overview

### Technology Stack
- **Language**: Rust (WASM crate), TypeScript (frontend integration)
- **Build**: `wasm-pack build --target web --out-dir pkg`
- **Key Libraries**: `wasm_bindgen` (Rust-JS interop)

### Design Approach

All Sprint 4 handlers are added to the existing `TerminalCore` struct. No new files are created in the WASM crate. Three distinct patterns are introduced:

1. **Batch API** (SGR): Accept a parameter slice, parse entirely within WASM, update cursor attrs
2. **Action Code Return** (Modes): Return a `u8` code indicating what TS-side action is needed
3. **Linear Memory Response Buffer** (Device): Write response bytes to an internal buffer, return length; TS reads via pointer

### Component Interaction

```
TS processAction("Csi", action)
  |
  +--> handleCsiWasm(grid, action)
        |
        +-- SGR: grid.core.handle_sgr(Uint16Array)           --> (void)
        +-- Edit: grid.core.handle_insert_lines(count)        --> (void)
        +-- Scroll: grid.core.handle_scroll_up(count)         --> u8 (0=done, N=scrollback)
        +-- Mode: grid.core.handle_set_mode(mode, enable)     --> u8 (action code)
        +-- Device: grid.core.handle_device_status_report(ps)  --> u8 (response length)
                    grid.core.get_response_ptr()               --> *const u8
```

## Implementation Phases

### Phase 1: Rust SGR Handler

**Goal**: Implement `handle_sgr` in Rust with full SGR parameter parsing, so that all SGR processing occurs within a single WASM call.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add `handle_sgr(params: &[u16])` public method
  - Add `parse_sgr_param` internal helper
  - Add SGR-related constants or helper for extended color parsing

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_sgr` | Parse SGR parameter array and apply attributes to cursor | Valid `&[u16]` slice (may be empty) | `cursor.fg`, `cursor.bg`, `cursor.flags` updated |
| `parse_sgr_param` | Parse a single SGR sub-sequence, advancing index | Index within bounds of params | Cursor attrs partially updated, index advanced past consumed params |

**Processing Flow**:
```
1. If params is empty -> treat as Reset (SGR 0)
2. Initialize index = 0
3. While index < params.len():
   +-- param 0 -> Reset all cursor attrs to default
   +-- params 1-9 -> Set corresponding style flag
   +-- params 22-29 -> Clear corresponding style flag(s)
   +-- params 30-37 -> Set standard foreground (indexed 0-7)
   +-- param 38 -> Parse extended foreground:
   |     +-- subtype 5 -> Indexed color (consume 1 more param)
   |     +-- subtype 2 -> RGB color (consume 3 more params)
   |     +-- else -> Skip (malformed)
   +-- param 39 -> Reset foreground to default
   +-- params 40-47 -> Set standard background (indexed 0-7)
   +-- param 48 -> Parse extended background (same as 38)
   +-- param 49 -> Reset background to default
   +-- params 90-97 -> Set bright foreground (indexed 8-15)
   +-- params 100-107 -> Set bright background (indexed 8-15)
   +-- unknown -> Ignore, advance index
4. Done (no return value)
```

**Implementation Steps**:

1. **Define SGR constants and color mapping**
   - Map SGR param ranges to PackedColor values and style flag bits
   - Use existing `STYLE_*` constants from `cell.rs`

2. **Implement `handle_sgr` public method**
   - Accept `&[u16]` via `wasm_bindgen`
   - Handle empty params as Reset
   - Iterate through params with mutable index for multi-param sequences (38/48)

3. **Implement extended color parsing**
   - Handle `38;5;n` (256-color indexed) and `38;2;r;g;b` (TrueColor)
   - Handle `48;5;n` and `48;2;r;g;b` analogously
   - Gracefully handle truncated sub-params (no panic)

4. **Write Rust unit tests**
   - All individual SGR params (reset, style flags, standard/bright colors)
   - Extended colors (indexed, RGB)
   - Multiple params in one call
   - Edge cases (empty, unknown, truncated extended color)

**Dependencies**:
- Requires: Sprint 3 TerminalCore structure (already exists)
- Blocks: Phase 5 (TS integration for SGR)

**Testing Approach**:

*Unit Tests (Rust)*:
- Test each SGR param individually (0, 1-9, 22-29, 30-37, 38, 39, 40-47, 48, 49, 90-97, 100-107)
- Test extended color variants (indexed, RGB)
- Test multi-param batch (e.g., `[1, 31, 42]`)
- Test edge cases (empty, truncated `[38, 5]`, unknown param)

**Acceptance Criteria**:
- [ ] `handle_sgr` correctly parses all SGR params listed in SPEC US1
- [ ] Empty params treated as Reset
- [ ] Truncated extended colors handled without panic
- [ ] `cargo test` passes for all SGR tests
- [ ] `wasm-pack build` succeeds

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Extended color parsing edge cases (malformed sequences)
  - **Mitigation**: Follow the defensive approach used in TS `parseSgrParams` - ignore and continue on malformed input

---

### Phase 2: Rust Edit Handlers

**Goal**: Implement IL/DL/ICH/DCH in Rust, each as a single WASM call that directly manipulates the grid.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add `handle_insert_lines(count: u16)`
  - Add `handle_delete_lines(count: u16)`
  - Add `handle_insert_characters(count: u16)`
  - Add `handle_delete_characters(count: u16)`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_insert_lines` | Insert blank lines at cursor row within scroll region | count > 0, cursor.row within scroll region | Rows shifted down, blank rows inserted at cursor, dirty marked |
| `handle_delete_lines` | Delete lines at cursor row within scroll region | count > 0, cursor.row within scroll region | Rows shifted up, bottom rows cleared, dirty marked |
| `handle_insert_characters` | Insert blank characters at cursor position | count > 0 | Cells shifted right, blanks inserted at cursor col, dirty marked |
| `handle_delete_characters` | Delete characters at cursor position | count > 0 | Cells shifted left, trailing cells cleared, dirty marked |

**Processing Flow (IL)**:
```
1. Get effective scroll region (top, bottom)
2. If cursor.row outside [top, bottom] -> no-op
3. Clamp count to remaining rows (bottom - cursor.row + 1)
4. Shift rows down within region: rows [cursor.row .. bottom-count] -> [cursor.row+count .. bottom]
5. Clear rows [cursor.row .. cursor.row+count-1]
6. Mark dirty [top .. bottom]
```

**Processing Flow (ICH)**:
```
1. Clamp count to remaining columns (cols - cursor.col)
2. Shift cells right within row: cells [cursor.col .. cols-count-1] -> [cursor.col+count .. cols-1]
3. Clear cells [cursor.col .. cursor.col+count-1] with default attrs
4. Mark row dirty
```

**Implementation Steps**:

1. **Implement IL/DL using existing `shift_rows_down`/`shift_rows_up`**
   - Reuse the already-implemented `shift_rows_up` and `shift_rows_down` methods
   - Add scroll region boundary checks and count clamping
   - Handle cursor outside scroll region as no-op

2. **Implement ICH/DCH with cell-level shifting**
   - Shift cells within a single row
   - Clear vacated cells with EMPTY cell and current cursor attrs for background
   - Handle overflow table entries for shifted cells

3. **Write Rust unit tests**
   - Standard operations with various counts
   - Cursor outside scroll region (no-op)
   - Count exceeding available space (clamped)
   - Dirty row verification

**Dependencies**:
- Requires: Existing `shift_rows_up`/`shift_rows_down` (already in Sprint 1)
- Blocks: Phase 5 (TS integration for Edit)

**Testing Approach**:

*Unit Tests (Rust)*:
- IL: standard insert, cursor outside region, count exceeds region, dirty marking
- DL: standard delete, cursor outside region, count exceeds region, dirty marking
- ICH: standard insert, count exceeds remaining cols, dirty marking
- DCH: standard delete, count exceeds remaining cols, dirty marking

**Acceptance Criteria**:
- [ ] IL/DL respect scroll region boundaries
- [ ] ICH/DCH respect row boundaries
- [ ] Count clamping works correctly
- [ ] Dirty rows marked after each operation
- [ ] All Rust unit tests pass

**Estimated Effort**: Small (1-2 days)

---

### Phase 3: Rust Scroll Handlers

**Goal**: Implement SU/SD/DECSTBM in Rust. SU uses a scroll bridge pattern: returns 0 when handled by WASM (scroll region), returns count when TS should handle scrollback (full screen).

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add `handle_scroll_up(count: u16) -> u8`
  - Add `handle_scroll_down(count: u16)`
  - Add `handle_decstbm(top: u16, bottom: u16)`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_scroll_up` | Scroll content up; decide WASM-internal vs scroll bridge | count > 0 | Region scrolled or bridge return value |
| `handle_scroll_down` | Scroll content down within region | count > 0 | Region rows shifted down, top rows cleared |
| `handle_decstbm` | Set scroll region boundaries, move cursor home | 1-indexed inputs (0 = default) | scroll_region_top/bottom updated, cursor at (0,0), wrapPending cleared |

**Processing Flow (SU)**:
```
1. Determine if scroll region is full screen:
   +-- scroll_region_top == 0 AND scroll_region_bottom == rows - 1
       +-- YES (full screen): return count as u8 (TS handles scrollback)
       +-- NO (scroll region): WASM handles internally
2. WASM internal handling:
   +-- Shift rows up within [top, bottom]
   +-- Clear bottom rows
   +-- Mark dirty
   +-- Return 0
```

**Processing Flow (DECSTBM)**:
```
1. Convert inputs: top (1-indexed, 0->1) to 0-indexed, bottom (1-indexed, 0->rows) to 0-indexed
2. Validate: top < bottom
   +-- Invalid: reset to full screen (0, rows-1)
   +-- Valid: set scroll_region_top = top, scroll_region_bottom = bottom
3. Move cursor to (0, 0)
4. Clear wrapPending
```

**Implementation Steps**:

1. **Implement `handle_scroll_up` with bridge pattern**
   - Check full screen vs scroll region
   - Full screen: return count (clamped to u8 max)
   - Scroll region: call `shift_rows_up` on region, return 0
   - Key consideration: return value u8 limits count to 255, sufficient for practical scroll counts

2. **Implement `handle_scroll_down`**
   - Always WASM-internal (SD never interacts with scrollback)
   - Call `shift_rows_down` on scroll region

3. **Implement `handle_decstbm`**
   - Convert 1-indexed to 0-indexed
   - Validate top < bottom
   - Use existing `set_scroll_region` method
   - Move cursor to home, clear wrapPending

4. **Write Rust unit tests**
   - SU: scroll region case (returns 0), full screen case (returns count)
   - SD: standard operation, count clamping
   - DECSTBM: valid region, default (0,0), invalid (top >= bottom), cursor home

**Dependencies**:
- Requires: Existing `shift_rows_up`/`shift_rows_down`, `set_scroll_region`
- Blocks: Phase 5 (TS integration for Scroll)

**Testing Approach**:

*Unit Tests (Rust)*:
- SU with scroll region: rows shifted, returns 0
- SU full screen: returns count, grid unchanged
- SD: rows shifted down within region
- DECSTBM: region set, cursor at home, wrapPending cleared
- Edge: count exceeds region, invalid DECSTBM params

**Acceptance Criteria**:
- [ ] SU correctly differentiates full screen (bridge) vs scroll region (internal)
- [ ] SD always handles internally
- [ ] DECSTBM sets region and resets cursor
- [ ] All Rust unit tests pass

**Estimated Effort**: Small (1-2 days)

---

### Phase 4: Rust Mode + Device Handlers

**Goal**: Implement mode action codes and device response generation. Modes return an action code u8 that tells TS what side-effect to perform. Device responses are written to an internal buffer and read by TS from linear memory.

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Add mode action code constants
  - Add `handle_set_mode(mode: u16, enable: bool) -> u8`
  - Add response buffer field to `TerminalCore` struct
  - Add `handle_device_status_report(ps: u8) -> u8`
  - Add `handle_primary_device_attributes() -> u8`
  - Add `handle_secondary_device_attributes() -> u8`
  - Add `get_response_ptr() -> *const u8`
  - Add `get_response_len() -> u32`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_set_mode` | Process DEC private mode, return action code | Valid mode number | Boolean mode set in bitfield OR action code returned |
| `handle_device_status_report` | Generate DSR response bytes | ps is 5 or 6 | Response in buffer, length returned |
| `handle_primary_device_attributes` | Generate DA1 response | None | VT420 response in buffer |
| `handle_secondary_device_attributes` | Generate DA2 response | None | VT420 identification in buffer |
| `get_response_ptr` / `get_response_len` | Expose response buffer location | Response written | Valid pointer and length |
| `write_response` | Write bytes to response buffer (internal) | Data fits in buffer | Buffer updated, length set |

**Processing Flow (SetMode)**:
```
1. Match mode number:
   +-- 3 (DECCOLM) -> Set/clear MODE_COLUMN_132 bit, return 0
   +-- 5 (DECSCNM) -> Set/clear MODE_REVERSE_SCREEN bit, return 0
   +-- 6 (DECOM) -> Set/clear MODE_ORIGIN bit, return 0
   +-- 7 (DECAWM) -> Set/clear MODE_AUTO_WRAP bit, return 0
   +-- 12 (ATT160) -> Set/clear MODE_CURSOR_BLINK bit, return 0
   +-- 25 (DECTCEM) -> Set/clear MODE_CURSOR_VISIBLE bit, return 0
   +-- 47/1047 -> return 1 (switchToAlt) if enable, 3 (switchToMain) if disable
   +-- 1048 -> return 4 (saveCursor) if enable, 5 (restoreCursor) if disable
   +-- 1049 -> return 2 (saveAndSwitchToAlt) if enable, 3 (switchToMain) if disable
   +-- 1/1000/1002/1003/1004/1005/1006/2004 -> return 0xFF (TS fallback)
   +-- unknown -> return 0 (no-op)
```

**Processing Flow (DSR)**:
```
1. Match ps:
   +-- 5 -> Write "ESC[0n" to response buffer, return 4
   +-- 6 -> Format "ESC[{row+1};{col+1}R", write to buffer, return length
   +-- other -> Return 0 (no response)
```

**Implementation Steps**:

1. **Add response buffer to TerminalCore**
   - Fixed-size array (64 bytes, sufficient for all device responses)
   - Add `response_len: u8` field
   - Key consideration: buffer is reused across calls, no allocation needed

2. **Implement `handle_set_mode`**
   - Define action code constants
   - Match on mode number, dispatch to boolean mode set or action code return
   - Multi-valued modes (mouse, cursor keys, bracketed paste) return 0xFF for TS fallback

3. **Implement device response handlers**
   - `handle_device_status_report`: format DSR responses into buffer
   - `handle_primary_device_attributes`: static VT420 response
   - `handle_secondary_device_attributes`: static VT420 identification
   - `get_response_ptr` / `get_response_len`: expose buffer to JS

4. **Write Rust unit tests**
   - Mode: boolean modes return 0, buffer modes return correct action codes, mouse modes return 0xFF
   - Device: DSR ps=5, ps=6 at various positions, DA1, DA2
   - Response buffer: pointer valid, length correct

**Dependencies**:
- Requires: Existing mode bitfield in TerminalCore
- Blocks: Phase 5 (TS integration for Mode + Device)

**Testing Approach**:

*Unit Tests (Rust)*:
- Mode action codes for each mode number (boolean, buffer switch, TS fallback)
- DSR: ps=5 (OK status), ps=6 (cursor position at various locations)
- DA1/DA2: correct VT420 response strings
- Response buffer: valid pointer and length after each device call

**Acceptance Criteria**:
- [ ] Boolean modes set correctly in WASM bitfield
- [ ] Buffer switch modes return correct action codes
- [ ] Multi-valued modes return 0xFF for TS fallback
- [ ] Device responses match expected byte sequences
- [ ] Response buffer accessible via pointer + length
- [ ] All Rust unit tests pass

**Estimated Effort**: Medium (3-5 days)

---

### Phase 5: TypeScript Integration

**Goal**: Wire all new WASM handlers into the TS `handleCsiWasm` dispatch, add mode action dispatch helpers, device response reader, and reduce `syncCursorAttrsToWasm` call sites.

**Files to Modify**:
- `src/terminal/state.ts`:
  - Extend `handleCsiWasm` switch with all Sprint 4 CSI actions
  - Add `handleModesWasm` helper for multi-mode processing with action code dispatch
  - Add `readAndSendResponse` helper for device response linear memory read
  - Add `executeModAction` helper for action code execution
  - Remove `syncCursorAttrsToWasm()` from `switchToAlternateBuffer` and `switchToPrimaryBuffer` and `reset`
  - Keep `syncCursorAttrsToWasm()` call only after cursor restore (Approach A from SPEC)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handleCsiWasm` (extended) | Route all CSI actions to WASM | WASM grid available | Action handled or returns false |
| `handleModesWasm` | Process SetMode/ResetMode via WASM, collect and execute actions | Modes array, enable flag | Mode state updated, actions executed, modes synced |
| `readAndSendResponse` | Read device response from WASM linear memory | Response written by WASM | Response copied and added to pending responses |
| `executeModAction` | Execute a mode action code | Valid action code (1-5) | Buffer switch or cursor save/restore performed |

**Processing Flow (handleModesWasm)**:
```
1. Initialize empty actions list
2. For each mode in modes array:
   +-- Call grid.core.handle_set_mode(mode, enable)
   +-- code == 0xFF -> Handle in TS via setDecPrivateMode (fallback)
   +-- code > 0 -> Collect action code
   +-- code == 0 -> No action needed
3. Execute collected actions (buffer switch, cursor save/restore)
4. Sync modes from WASM to TS (for boolean modes set by WASM)
5. Sync TS-only modes to WASM (for multi-valued modes handled by TS)
6. Return true
```

**Processing Flow (readAndSendResponse)**:
```
1. Get response pointer from grid.core.get_response_ptr()
2. Get WASM memory buffer
3. Create Uint8Array view at pointer with given length
4. Slice (copy) the response bytes
5. Call addPendingResponse with copied bytes
```

**Implementation Steps**:

1. **Add mode action code constants to TS**
   - Mirror Rust constants: MODE_NO_ACTION(0), MODE_SWITCH_TO_ALT(1), MODE_SAVE_AND_SWITCH_TO_ALT(2), MODE_SWITCH_TO_MAIN(3), MODE_SAVE_CURSOR(4), MODE_RESTORE_CURSOR(5), MODE_TS_FALLBACK(0xFF)

2. **Extend `handleCsiWasm` switch cases**
   - Add cases for: Sgr, InsertLines, DeleteLines, InsertCharacters, DeleteCharacters, ScrollUp, ScrollDown, SetScrollRegion, SetMode, ResetMode, DeviceStatusReport, PrimaryDeviceAttributes, SecondaryDeviceAttributes, TertiaryDeviceAttributes
   - Each case delegates to the corresponding WASM method

3. **Implement `handleModesWasm` helper**
   - Process each mode, collect action codes
   - 0xFF triggers TS fallback via existing `setDecPrivateMode`
   - Execute actions after all modes processed
   - Sync modes bidirectionally

4. **Implement `readAndSendResponse` helper**
   - Access WASM linear memory via `grid.core.memory.buffer`
   - Copy response bytes before WASM may reuse buffer

5. **Reduce `syncCursorAttrsToWasm` call sites**
   - Remove from `switchToAlternateBuffer`
   - Remove from `switchToPrimaryBuffer`
   - Remove from `reset`
   - Keep only after cursor restore action (mode 1048 disable, mode 1049 switch to main with restore)
   - Key consideration: Per SPEC Approach A, cursor save/restore remains in TS for Sprint 4. One sync point remains at restore.

**Dependencies**:
- Requires: All Rust handlers from Phases 1-4
- Blocks: Phase 6 (Verification)

**Testing Approach**:

*Integration Tests (TypeScript, `bun test`)*:
- SGR via WASM: Reset, Bold+Color, 256-color, TrueColor
- Edit via WASM: InsertLines, DeleteLines, InsertCharacters, DeleteCharacters
- Scroll via WASM: ScrollUp (region), ScrollUp (full screen bridge), ScrollDown, DECSTBM
- Mode via WASM: boolean mode (DECAWM), buffer switch (1049), TS fallback (mouse)
- Device via WASM: DSR CPR, DA1, DA2

**Acceptance Criteria**:
- [ ] All Sprint 4 CSI actions handled by `handleCsiWasm`
- [ ] Mode action codes correctly dispatched
- [ ] Device responses correctly read from WASM memory
- [ ] `syncCursorAttrsToWasm` removed from all sites except cursor restore
- [ ] Fallback to TS handlers when WASM unavailable

**Estimated Effort**: Medium (3-5 days)

---

### Phase 6: Verification and Regression Testing

**Goal**: Full regression test, cross-validation, binary size verification, and smoke test.

**Files to Modify**: None (testing only)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Regression suite | Verify all existing tests still pass | All phases complete | 1824+ TS tests pass, all Rust tests pass |
| Binary size check | Verify WASM binary increase < 10KB | Build complete | Binary < 56KB total |
| Smoke test | Verify working terminal in `bun tauri dev` | Full build | Terminal functional |

**Implementation Steps**:

1. **Run full Rust test suite**
   - `cargo test --manifest-path wasm/Cargo.toml`
   - All Sprint 1-3 tests + Sprint 4 new tests must pass

2. **Run full TypeScript test suite**
   - `bun test` (1824+ tests)
   - No regressions from Sprint 4 changes

3. **Verify WASM binary size**
   - Build with `wasm-pack build --target web --out-dir pkg`
   - Check `.wasm` file size < 56KB (Sprint 3 baseline 45.8KB + <10KB)

4. **Smoke test**
   - `bun tauri dev`
   - Verify: color display (256-color, TrueColor), alternate screen (vim/top), scroll regions
   - Verify: vttest basic tests still pass

5. **Cross-validation**
   - Compare WASM vs TS handler output for representative sequences
   - Verify identical grid state after SGR, edit, scroll operations

**Dependencies**:
- Requires: All Phases 1-5 complete

**Testing Approach**:

*Regression*:
- All existing test suites unchanged and passing

*E2E (Docker)*:
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path wasm/Cargo.toml"`
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`

*Manual Testing*:
- vttest basic screen operations
- Color rendering in terminal
- Alternate screen switch with vim/top/htop

**Acceptance Criteria**:
- [ ] All Rust tests pass (Sprint 1-4)
- [ ] All TypeScript tests pass (1824+)
- [ ] TypeScript type check passes
- [ ] WASM binary < 56KB
- [ ] Terminal functional in `bun tauri dev`
- [ ] vttest basic tests pass

**Estimated Effort**: Small (1-2 days)

---

## Complete File Structure

```
wasm/src/
+-- lib.rs              # UNCHANGED
+-- unicode.rs          # UNCHANGED
+-- terminal_core.rs    # MODIFIED: +handle_sgr, +Edit(4), +Scroll(3), +Modes, +Device(3+2), +response_buffer
+-- cell.rs             # UNCHANGED

src/terminal/
+-- state.ts            # MODIFIED: handleCsiWasm extended, +handleModesWasm, +readAndSendResponse, syncCursorAttrsToWasm reduced
+-- modes.ts            # UNCHANGED (syncModesFromWasm already exists)
+-- attributes.ts       # UNCHANGED
+-- handlers/
|   +-- csi_char_attrs.ts  # UNCHANGED (JS fallback)
|   +-- csi_edit.ts        # UNCHANGED (JS fallback)
|   +-- csi_scrolling.ts   # UNCHANGED (JS fallback)
|   +-- csi_modes.ts       # UNCHANGED (JS fallback)
|   +-- csi_device.ts      # UNCHANGED (JS fallback)
|   +-- index.ts           # UNCHANGED
+-- wasm/
    +-- terminal-core.ts   # UNCHANGED

doc/tasks/wasm-sgr-edit-scroll/
+-- SPEC.md             # Existing specification
+-- IMPLEMENTATION.md   # This document
+-- VERIFICATION.md     # Verification checklist
```

**File Descriptions**:
- `wasm/src/terminal_core.rs` - All WASM-side Sprint 4 handlers. Extends existing `TerminalCore` struct with SGR, Edit, Scroll, Mode, and Device methods, plus response buffer field.
- `src/terminal/state.ts` - TypeScript integration layer. Extended `handleCsiWasm` routes all CSI actions. New helpers for mode action dispatch and device response reading. Reduced `syncCursorAttrsToWasm` call sites.

## Testing Strategy

### Unit Testing

**Approach**:
- Rust: `#[cfg(test)] mod tests` within `terminal_core.rs`
- TypeScript: `bun test` with existing test infrastructure
- All tests run in Docker environment per CLAUDE.md instructions

**Test Coverage Goals**:
- SGR parsing: 90%+ (all param types and edge cases)
- Edit operations: 90%+ (all operations, boundary conditions)
- Scroll operations: 90%+ (bridge pattern, region handling)
- Mode handling: 80%+ (all mode categories)
- Device responses: 90%+ (all response formats)

**Key Test Areas**:
1. **SGR Parsing** - All param types, multi-param batches, extended colors, edge cases
2. **Grid Operations** - Row/cell shifting, boundary clamping, dirty marking
3. **Scroll Bridge** - Full screen vs scroll region differentiation
4. **Mode Action Codes** - Correct code for each mode, TS fallback trigger
5. **Response Buffer** - Correct byte sequences, pointer validity, CPR formatting

### Integration Testing

**Scenarios**:
1. SGR followed by Print: cell has correct attributes
2. Mode 1049: save cursor, switch to alt, switch back, restore cursor
3. ScrollUp in region then Print: content positioned correctly

### E2E Testing (Docker)

Based on SPEC test scenarios:
- [ ] Rust tests: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path wasm/Cargo.toml"`
- [ ] TypeScript tests: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- [ ] Type check: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- [ ] WASM build: `cd wasm && wasm-pack build --target web --out-dir pkg`

### Manual Testing (E2E Not Possible)

- [ ] vttest basic screen operations
- [ ] 256-color and TrueColor rendering in running terminal
- [ ] Alternate screen switch (vim, top, htop)
- [ ] Scroll region behavior in less/man

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| wasm_bindgen | existing | Rust-JS FFI | Already in wasm/Cargo.toml |
| wasm-pack | existing | WASM build tool | Already installed |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 (SGR) - No dependencies on other Sprint 4 phases
2. Phase 2 (Edit) - No dependencies on other Sprint 4 phases
3. Phase 3 (Scroll) - No dependencies on other Sprint 4 phases
4. Phase 4 (Mode + Device) - No dependencies on other Sprint 4 phases
5. Phase 5 (TS Integration) - Depends on Phases 1-4
6. Phase 6 (Verification) - Depends on Phase 5

Note: Phases 1-4 are independent and could be implemented in parallel.

**Component Dependencies**:
- All Phase 1-4 handlers depend on existing `TerminalCore` struct fields (grid, cursor, modes, scroll_region)
- Phase 2 (Edit) and Phase 3 (Scroll) reuse existing `shift_rows_up`/`shift_rows_down`
- Phase 5 depends on all Rust handlers being exported via `wasm_bindgen`
- Phase 5 uses existing `syncModesFromWasm` and `setDecPrivateMode` from `modes.ts`

## Risk Assessment

### Technical Risks

1. **WASM Memory Access for Device Response**
   - **Risk**: Accessing WASM linear memory from TS requires correct pointer arithmetic
   - **Likelihood**: Low (pattern is well-documented)
   - **Impact**: Medium (device responses would fail)
   - **Mitigation**: Use `.slice()` to copy response bytes before WASM can reuse buffer

2. **SGR Extended Color Edge Cases**
   - **Risk**: Malformed SGR sequences (truncated 38;5; or 38;2;) could cause index out of bounds
   - **Likelihood**: Medium (real-world terminals send various sequences)
   - **Impact**: Low (panic in WASM = JS exception)
   - **Mitigation**: Defensive bounds checking on every param access

3. **syncCursorAttrsToWasm Removal Side Effects**
   - **Risk**: Removing sync calls could leave cursor attrs inconsistent between TS and WASM
   - **Likelihood**: Low (cursor attrs now live in WASM after SGR migration)
   - **Impact**: High (incorrect character rendering)
   - **Mitigation**: Keep one sync point at cursor restore; verify with existing test suite

### Implementation Risks

1. **Scope Creep**
   - **Risk**: Adding features beyond spec (e.g., moving ESC handlers to WASM)
   - **Mitigation**: Stick to Sprint 4 scope; ESC handler migration deferred to Sprint 5

2. **Binary Size Budget**
   - **Risk**: Implementation exceeds 10KB budget
   - **Likelihood**: Low (estimated 4.7KB per SPEC)
   - **Impact**: Low (soft limit)
   - **Mitigation**: Monitor size after each phase; optimize if approaching limit

## Performance Considerations

1. **SGR Batch Processing** - Single WASM call for entire SGR sequence vs current 3+ operations (parse + N apply + sync)
2. **Edit Operations** - Single WASM call for row/cell shifting vs current N WasmLineProxy operations
3. **Mode Handling** - Single WASM call per mode vs current TS set + sync pair
4. **Response Buffer** - Static 64-byte buffer, no allocation per device query

## Open Questions

### From Specification:
- None (all TBD items resolved)

### Implementation-Specific:
- [ ] Should `handle_scroll_up` return value be `u8` or `u16`? (u8 limits to 255 rows, sufficient for practical use)

### To Clarify with User:
- None (specification is comprehensive)

## Future Enhancements

### From Specification (Deferred to Sprint 5):
- Move cursor save/restore to WASM (eliminate last `syncCursorAttrsToWasm` call site)
- Move ESC handlers (DECSC/DECRC) to WASM

## Success Metrics

### Functional Completeness
- [ ] All Sprint 4 CSI handlers implemented in WASM
- [ ] 100% CSI routing through `handleCsiWasm`
- [ ] `syncCursorAttrsToWasm` reduced to cursor restore only
- [ ] All SPEC acceptance criteria met

### Quality Metrics
- [ ] All Rust unit tests pass
- [ ] All TypeScript tests pass (1824+)
- [ ] TypeScript type check passes
- [ ] No critical bugs in regression testing

### Performance Metrics
- [ ] SGR: 1 WASM call per sequence
- [ ] IL/DL/ICH/DCH: 1 WASM call each
- [ ] WASM binary < 56KB total

### Compatibility Metrics
- [ ] JS fallback path functional when WASM unavailable
- [ ] vttest basic tests pass
- [ ] Alternate screen applications (vim, top) work correctly

## References

- **Specification**: `doc/tasks/wasm-sgr-edit-scroll/SPEC.md`
- **Sprint 3 SPEC**: `doc/tasks/wasm-c0-csi-handlers/SPEC.md`
- **WASM Roadmap**: `tmp/wasm.md`
- **Current Implementations**:
  - `src/terminal/handlers/csi_char_attrs.ts` - SGR handler (TS fallback)
  - `src/terminal/handlers/csi_edit.ts` - Edit handlers (TS fallback)
  - `src/terminal/handlers/csi_scrolling.ts` - Scroll handlers (TS fallback)
  - `src/terminal/handlers/csi_modes.ts` - Mode handlers (TS fallback)
  - `src/terminal/handlers/csi_device.ts` - Device handlers (TS fallback)
  - `src/terminal/sgr.ts` - SGR parameter parsing (TS)
  - `src/terminal/attributes.ts` - Cell attributes and WASM pack/unpack (TS)
  - `src/terminal/modes.ts` - Mode management and WASM sync (TS)

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm approach for `syncCursorAttrsToWasm` reduction (Approach A)
   - Address any open questions

2. **Begin Implementation**
   - Start with Phases 1-4 (Rust handlers, can be parallelized)
   - Follow TDD approach (write Rust tests first)
   - Build WASM after each phase to verify compilation

3. **TypeScript Integration**
   - Phase 5 after all Rust handlers verified
   - Run full test suite after integration

4. **Final Verification**
   - Phase 6: regression, binary size, smoke test
