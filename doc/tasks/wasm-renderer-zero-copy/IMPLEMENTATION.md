# Implementation Plan: WASM Renderer Zero-Copy + Carry-over (Sprint 7)

## Overview

Replace per-cell WASM boundary crossings in the renderer with a batch binary parsing path, delegate WasmLineProxy dirty state to the WASM bitset, and implement Kitty image_id correlation for reliable multi-image transfer.

## Objectives

- Reduce WASM boundary crossings from cols×5+1 to 2 per dirty row
- Eliminate intermediate Cell/Line/CellAttributes object allocation in the packed rendering path
- Make WasmLineProxy.dirty a true view of the WASM core dirty bitset
- Generate unique Kitty image_id per invocation for response correlation

## Prerequisites

### Development Environment

- Rust toolchain (for backend changes)
- Bun (for TypeScript build/test)
- Docker (for isolated test execution)

### Dependencies

- Existing WASM API: `get_row_packed()`, `get_scrollback_row_packed()`, `is_row_dirty()`, `mark_row_dirty()`, `clear_dirty()`, `get_dirty_rows()`
- No new external dependencies (crates or npm packages)

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (frontend rendering), Rust (backend Kitty protocol)
- **Framework**: Tauri (desktop shell), Canvas 2D (rendering)
- **Key Libraries**: wasm-pack (WASM build tooling)

### Design Approach

The optimization targets the rendering hot path by moving from per-cell WASM calls to a single packed binary fetch per row. The binary data is parsed entirely in JavaScript without creating intermediate objects. This is a **read-path-only** optimization — write paths (input, escape sequences) are unchanged.

The Kitty image_id change is independent and addresses a correctness issue in concurrent image display.

### Component Interaction

```
Rendering Data Flow (After):

TerminalState
  → getDirtyRows()          [1 WASM call → row indices]
  → getRowPacked(row)        [1 WASM call → Uint8Array]

groupPackedCellsIntoSpans()  [Pure JS binary parse → TextSpan[]]

CanvasRenderer
  → renderLinePacked()       [TextSpan[] → Canvas 2D draw calls]
```

```
Kitty image_id Flow (After):

generate_kitty_sequence()    [Atomic counter → unique id per call]
  → (sequence_string, image_id)
execute_image_command()      [Output sequence, pass id to response parser]
wait_for_kitty_response(id)  [Parse response, verify id match]
```

## Implementation Phases

### Phase 1: Packed Binary Span Parser

**Goal**: Create a standalone function that parses packed binary row data directly into TextSpan array without intermediate object allocation. Fully testable in isolation.

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` — Add packed span grouping function and helpers
- `src/terminal/canvas-renderer.test.ts` — Add comprehensive packed parser tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| groupPackedCellsIntoSpans | Parse packed binary → TextSpan[] | Valid Uint8Array from WASM, column count | Identical TextSpan[] output as groupCellsIntoSpans for same data |
| packedAttrsEqual | Compare 10 attribute bytes at two offsets | Two valid offsets within buffer | Returns true iff all 10 bytes match |
| unpackAttrsFromBinary | Extract CellAttributes from 10 bytes | Valid offset within buffer | CellAttributes matching the binary encoding |

**Processing Flow**:

1. Read character data from packed buffer
   - char_len < 0xFF → inline character (0 = empty, 1 = ASCII, 2+ = multi-byte UTF-8)
   - char_len == 0xFF → overflow character (length in next 2 bytes, then UTF-8 data)
2. Read cell width (1 byte)
3. Record attribute byte offset (10 bytes: fg 4 + bg 4 + flags 2 LE)
   - width == 0 with empty/space → skip (wide char placeholder)
   - width == 0 with character → merge as combining mark into previous cell
4. Compare attribute bytes with previous cell
   - Match → extend current span
   - Mismatch → finalize previous span, start new span (decode attrs only here)
5. After all columns processed → finalize last span

**Implementation Steps**:

1. **Add packedAttrsEqual helper** — byte-level comparison of 10 attribute bytes at two buffer offsets
2. **Add unpackAttrsFromBinary helper** — decode fg/bg colors and style flags from binary, same byte layout as existing parsePackedRow
3. **Add groupPackedCellsIntoSpans** — main parser with inline/overflow character handling, span accumulation, bounds checking
4. **Add unit tests** — equivalence with groupCellsIntoSpans, edge cases (empty row, wide chars, combining marks, overflow chars, truncated data, attribute boundaries)

**Dependencies**: None (pure algorithm, no integration needed)

**Testing Approach**:
- Unit: Construct packed Uint8Array test data manually, verify TextSpan[] output matches expected
- Equivalence: Generate packed data from known Cell arrays, compare groupPackedCellsIntoSpans output with groupCellsIntoSpans output

**Acceptance Criteria**:
- [ ] groupPackedCellsIntoSpans produces identical spans to groupCellsIntoSpans for the same cell data
- [ ] Truncated packed data is handled without crash (bounds checking)
- [ ] Wide characters, combining marks, and overflow characters are parsed correctly

**Estimated Effort**: Medium

---

### Phase 2: Packed Rendering Integration

**Goal**: Wire the packed parser into the CanvasRenderer pipeline so that dirty rows and scrollback rows use the packed path when WASM core is available. Parse packed data once per row and use the result for both background and text rendering.

**Files to Modify**:
- `src/terminal/unified-buffer.ts` — Expose packed data access methods
- `src/terminal/state.ts` — Add packed data access at state level
- `src/terminal/canvas-renderer.ts` — Add renderLinePacked, modify render() and forceRender()
- `src/terminal/canvas-renderer.test.ts` — Add integration tests for packed path

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UnifiedBuffer.getRowPacked | Return packed binary for viewport row | WASM grid available, valid row index | Uint8Array or null (null = no WASM) |
| UnifiedBuffer.getScrollbackRowPacked | Return packed binary for scrollback row | WASM grid available, valid scrollback index | Uint8Array or null |
| TerminalState.getRowPacked | Delegate to active buffer | Active buffer set | Uint8Array or null |
| TerminalState.getScrollbackRowPacked | Delegate to active buffer | Active buffer set, scrollOffset > 0 | Uint8Array or null |
| renderLinePacked | Parse packed data once, render background then text | Valid packed Uint8Array | Row fully rendered to canvas |
| render() packed path | Use packed path for dirty rows when WASM available | WASM core initialized | Fewer WASM boundary crossings |
| forceRender() packed path | Use packed path for all visible rows including scrollback | WASM core initialized | All rows rendered via packed path |

**Processing Flow**:

1. render() receives dirty row indices
   - For each dirty row: attempt getRowPacked(row)
     - Packed data available → renderLinePacked(rowIndex, packed)
     - Packed data unavailable → fallback to existing renderLine(rowIndex, line)
2. forceRender() renders all visible rows
   - scrollOffset == 0: use getRowPacked for viewport rows
   - scrollOffset > 0: use getScrollbackRowPacked for scrollback rows
   - Same fallback logic for unavailable packed data
3. renderLinePacked parses packed data once via groupPackedCellsIntoSpans
   - Use parsed spans for background rendering (same logic as renderLineBackground)
   - Use parsed spans for text rendering (same logic as renderLineText)
   - Avoids double-parsing that would occur if using existing separate methods

**Implementation Steps**:

1. **Expose packed data from UnifiedBuffer** — Add getRowPacked and getScrollbackRowPacked methods that delegate to wasmGrid when available, return null otherwise
2. **Add packed data access to TerminalState** — Delegate to active buffer's packed methods
3. **Add renderLinePacked to CanvasRenderer** — Parse once, render background pass then text pass from same TextSpan array
4. **Modify render() dirty row loop** — Try packed path first, fall back to LineAccessor path
5. **Modify forceRender() visible row loop** — Use packed path for both viewport and scrollback rows, preserve fallback
6. **Add integration tests** — Verify packed path is used when WASM available, fallback when not

**Dependencies**: Requires Phase 1 (packed parser)

**Testing Approach**:
- Unit: Mock WASM core returning known packed data, verify renderLinePacked draws correct spans
- Integration: Verify render() and forceRender() select correct path based on WASM availability
- Regression: All existing canvas-renderer tests continue to pass

**Acceptance Criteria**:
- [ ] render() uses packed path for dirty rows when WASM core is available
- [ ] forceRender() uses packed path for viewport and scrollback rows
- [ ] Fallback to LineAccessor path works when WASM is unavailable
- [ ] Packed data is parsed only once per row (not twice for bg+text)

**Estimated Effort**: Medium

---

### Phase 3: WasmLineProxy Dirty Delegation

**Goal**: Make WasmLineProxy.dirty a true getter that delegates to the WASM core's dirty bitset, eliminating the local boolean field that can drift out of sync.

**Files to Modify**:
- `src/terminal/wasm/terminal-core.ts` — Change dirty from field to getter/setter, update clearDirty
- `src/terminal/wasm/__tests__/terminal-core.test.ts` — Update/add dirty delegation tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| WasmLineProxy.dirty (getter) | Delegate to WASM is_row_dirty | WASM core initialized | Returns true iff WASM bitset has row marked dirty |
| WasmLineProxy.dirty (setter) | No-op (dirty managed by WASM core) | N/A | No state change |
| WasmLineProxy.clearDirty | No-op (dirty cleared at renderer level) | N/A | No state change, log note only |
| WasmLineProxy.markDirty | Delegate to WASM mark_row_dirty (unchanged) | WASM core initialized | WASM bitset updated |

**Processing Flow**:

1. Reading dirty state: getter calls WASM is_row_dirty(row) directly
   - No local boolean caching, always reflects WASM truth
2. Writing dirty state: setter is no-op
   - Callers that set dirty should use markDirty() instead
3. Clearing dirty: clearDirty is no-op
   - Dirty state is cleared in bulk via TerminalState.clearDirty() → core.clear_dirty()

**Implementation Steps**:

1. **Replace dirty field with getter/setter pair** — Remove `dirty = true` field, add getter delegating to WASM is_row_dirty, setter as no-op
2. **Make clearDirty a no-op** — Remove local state mutation, dirty clearing happens at renderer level
3. **Update existing tests** — Verify dirty getter reflects WASM state changes, verify clearDirty no-op behavior

**Dependencies**: Independent (can be done in parallel with Phase 1/2, but logically related)

**Testing Approach**:
- Unit: Set cell via WASM → verify dirty getter returns true → clear_dirty via core → verify getter returns false
- Regression: Existing dirty tracking tests adapted to new delegation behavior

**Acceptance Criteria**:
- [ ] WasmLineProxy.dirty always reflects WASM core dirty bitset
- [ ] clearDirty() is a no-op (dirty cleared only through core.clear_dirty())
- [ ] markDirty() continues to delegate to WASM core

**Estimated Effort**: Small

---

### Phase 4: Kitty image_id Correlation

**Goal**: Generate unique image_id per Kitty sequence invocation and verify response correlation, enabling reliable concurrent image display.

**Files to Modify**:
- `src-tauri/src/protocols/kitty.rs` — Atomic counter for image_id, return tuple
- `src-tauri/src/commands/image.rs` — Pass image_id to response parser, parse id from response

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Atomic image_id counter | Generate process-wide unique image_id | Static initialization | Monotonically increasing, skips 0 |
| generate_kitty_sequence | Produce Kitty protocol escape sequence with unique id | Valid image data | Returns (sequence, image_id) tuple |
| wait_for_kitty_response | Parse response and verify image_id match | Expected image_id provided | Returns Ok only if response id matches |

**Processing Flow**:

1. generate_kitty_sequence called
   - Atomically fetch-and-increment counter
   - If counter == 0 (wrapped around) → increment again to skip 0
   - Use counter value as image_id in all chunks of the sequence
   - Return (sequence_string, image_id) tuple
2. execute_image_command receives tuple
   - Output sequence to stdout
   - Pass image_id to wait_for_kitty_response
3. wait_for_kitty_response with expected_id
   - Existing state machine reads ESC _G response
   - Additionally parse `i={id}` from response body
   - Verify parsed id matches expected_id
   - Mismatched id → log warning, continue waiting (within existing timeout)

**Implementation Steps**:

1. **Add atomic counter** — Process-wide static atomic integer, initialized to 1
2. **Update generate_kitty_sequence return type** — Return (String, u32) tuple, use counter for image_id in all chunks
3. **Add image_id parsing to wait_for_kitty_response** — Extract `i={id}` from response body during state machine parsing, accept expected_id parameter
4. **Update execute_image_command** — Destructure tuple, pass image_id to response parser
5. **Add Rust tests** — Unique id generation, wrap-around skipping 0, response parsing with matching/mismatching ids

**Dependencies**: Independent (no TypeScript changes, no WASM changes)

**Testing Approach**:
- Unit: Verify counter produces unique values across multiple calls
- Unit: Verify wrap-around from max value skips 0
- Unit: Verify response parser extracts and validates image_id
- Unit: Verify mismatched id is handled gracefully (continues waiting)

**Acceptance Criteria**:
- [ ] Each generate_kitty_sequence call produces a unique image_id
- [ ] image_id 0 is never used (skipped on wrap-around)
- [ ] wait_for_kitty_response correctly validates response image_id
- [ ] Concurrent emterm image commands use distinct image_ids

**Estimated Effort**: Small

---

## Complete File Structure

```
src/terminal/
  canvas-renderer.ts          # [Modify] Add groupPackedCellsIntoSpans, packedAttrsEqual,
                               #          unpackAttrsFromBinary, renderLinePacked,
                               #          packed path in render() and forceRender()
  canvas-renderer.test.ts     # [Modify] Add packed parser and packed rendering tests
  unified-buffer.ts           # [Modify] Add getRowPacked, getScrollbackRowPacked
  state.ts                    # [Modify] Add getRowPacked, getScrollbackRowPacked delegation
  wasm/
    terminal-core.ts          # [Modify] WasmLineProxy dirty getter delegation
    __tests__/
      terminal-core.test.ts   # [Modify] Add/update dirty delegation tests

src-tauri/src/
  protocols/
    kitty.rs                  # [Modify] AtomicU32 counter, return (String, u32)
  commands/
    image.rs                  # [Modify] Pass image_id, parse response id
```

## Testing Strategy

- **Unit**: Core parsing logic (Phase 1), dirty delegation (Phase 3), image_id generation (Phase 4) — target 90%+ coverage on new code
- **Integration**: Renderer packed path selection (Phase 2) — verify correct path based on WASM availability
- **Regression**: All existing 1824+ TypeScript tests and 760+ Rust tests must pass unchanged
- **Performance**: Benchmark packed vs LineAccessor path (informal, target 2ms/row)

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | —       | No new dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Packed parser produces different output than existing path | Medium | High | Equivalence tests comparing both paths on same data |
| Performance target (2ms/row) not met | Low | Medium | Profiling; packed path is strictly fewer operations than current |
| Scrollback ring buffer boundary edge case | Low | Medium | Bounds checking in packed parser, WASM already handles boundary |
| AtomicU32 wrap-around race | Very Low | Low | Skip-zero logic tested; wrap-around takes billions of calls |

## Open Questions

> **Note**: No unresolved requirements.

## Success Metrics

- [ ] FR1-FR10 implemented and tested
- [ ] NFR1: Dirty row rendering within 2ms
- [ ] NFR2: Zero intermediate object allocation in packed path
- [ ] NFR3: WASM binary under 80KB (no WASM changes in this sprint)
- [ ] NFR4: All existing tests pass
- [ ] NFR5: Packed data parsing handles truncated data safely
