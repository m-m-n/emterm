# Feature: WASM Renderer Zero-Copy + Carry-over (Sprint 7)

## Overview

Sprint 7 optimizes the WASM → Renderer data transfer path by replacing per-cell WASM calls with batch binary parsing, completes the WasmLineProxy dirty getter delegation (carry-over from Sprint 1), and implements Kitty image_id correlation for reliable multi-image transfer.

## Objectives

- Eliminate per-cell WASM boundary crossings in the renderer (cols×4+ → 1 call per row)
- Remove JS intermediate object allocation (Cell, Line, CellAttributes) from the rendering hot path
- Make WasmLineProxy.dirty a true view of WASM core dirty bitset
- Generate unique Kitty image_id for reliable response correlation

## User Stories

### US1: Fast Terminal Rendering
As a terminal user, I want the terminal to render efficiently, so that high-throughput output (e.g., `cat large_file.txt`) feels smooth.

**Acceptance Criteria:**
- [ ] Dirty row rendering path completes within 2ms per row
- [ ] No JS Line/Cell/CellAttributes objects are created during packed rendering
- [ ] WASM boundary crossings are reduced to 1 per dirty row

### US2: Smooth Scrollback Browsing
As a terminal user, I want scrollback browsing to be responsive, so that I can review past output without lag.

**Acceptance Criteria:**
- [ ] Scrollback rows use the same packed rendering path as viewport rows
- [ ] forceRender() during scroll uses get_scrollback_row_packed() directly

### US3: Reliable Multi-Image Display
As a CLI user running `emterm image`, I want concurrent image commands to complete reliably, so that scripted image display works correctly.

**Acceptance Criteria:**
- [ ] Each `emterm image` invocation uses a unique image_id
- [ ] CLI correctly correlates response with its own image_id
- [ ] Concurrent `emterm image` commands do not interfere with each other

## Technical Requirements

### Functional Requirements

- **FR1:** New `groupPackedCellsIntoSpans(packed: Uint8Array, cols: number): TextSpan[]` function that parses packed binary data directly into TextSpan array without creating intermediate Cell/Line objects
- **FR2:** New `renderLinePacked(rowIndex: number, packed: Uint8Array)` method in CanvasRenderer that uses FR1 for both background and text rendering
- **FR3:** `render()` method uses packed path for dirty rows when WASM core is available: `core.get_row_packed(row)` → `renderLinePacked()`
- **FR4:** `forceRender()` method uses packed path for all visible rows, including scrollback rows via `core.get_scrollback_row_packed(index)`
- **FR5:** `getVisibleLinesPacked()` returns packed binary data (Uint8Array[]) instead of LineAccessor[] for the WASM rendering path
- **FR6:** WasmLineProxy.dirty becomes a getter that delegates to `core.is_row_dirty(this.row)`, removing the local boolean field
- **FR7:** WasmLineProxy.clearDirty() becomes a no-op (dirty state is cleared through `core.clear_dirty()` at the renderer level)
- **FR8:** `generate_kitty_sequence()` uses an `AtomicU32` counter to generate unique image_id per invocation instead of hardcoded `1`
- **FR9:** `wait_for_kitty_response()` parses the response `ESC _G i={id};{status} ESC \` and verifies the image_id matches the one sent
- **FR10:** Existing JS fallback rendering path (LineAccessor-based) is preserved for WASM initialization failure

### Non-Functional Requirements

- **NFR1 - Performance:** Single dirty row rendering (data fetch + parse + Canvas draw) completes within 2ms
- **NFR2 - Memory:** Zero Cell/CellAttributes/Line object allocation per packed rendering call
- **NFR3 - Binary Size:** WASM binary stays under 80KB (no new WASM API needed for this sprint)
- **NFR4 - Compatibility:** All existing tests pass without modification
- **NFR5 - Robustness:** Packed binary parsing handles truncated data safely (bounds checking)

## Implementation Approach

### Architecture

**Current Rendering Path (Before Sprint 7):**
```
CanvasRenderer.render()
  → state.getDirtyRows()                    // 1 WASM call
  → buffer.getLine(row)                     // WasmLineProxy (no WASM call)
    → groupCellsIntoSpans(line)
      → line.getCell(i)                     // WasmCellProxy per cell
        → core.get_cell_char(col, row)      // WASM call ×1
        → core.get_cell_width(col, row)     // WASM call ×1
        → core.get_cell_fg(col, row)        // WASM call ×1
        → core.get_cell_bg(col, row)        // WASM call ×1
        → core.get_cell_flags(col, row)     // WASM call ×1
  → renderSpanText(span, row)               // Canvas 2D API

Total: 1 + (cols × 5) WASM calls per dirty row
  80 cols = 401 WASM calls per row
```

**Optimized Rendering Path (After Sprint 7):**
```
CanvasRenderer.render()
  → state.getDirtyRows()                    // 1 WASM call
  → core.get_row_packed(row)                // 1 WASM call → Uint8Array
  → groupPackedCellsIntoSpans(packed, cols) // Pure JS binary parse
  → renderSpanText(span, row)               // Canvas 2D API

Total: 2 WASM calls per dirty row (getDirtyRows + get_row_packed)
```

**Scrollback Path:**
```
forceRender() with scrollOffset > 0
  → core.get_scrollback_row_packed(index)   // 1 WASM call → Uint8Array
  → groupPackedCellsIntoSpans(packed, cols) // Pure JS binary parse
  → renderSpanText(span, row)               // Canvas 2D API
```

### Data Flow

```
WASM Linear Memory
  ↓ get_row_packed(row) / get_scrollback_row_packed(index)
Uint8Array (packed binary: char + width + fg + bg + flags per cell)
  ↓ groupPackedCellsIntoSpans()
TextSpan[] (text + attrs + startCol + cellCount + cells)
  ↓ renderSpanText() / renderLineBackground()
Canvas 2D API
```

### Core Algorithm: groupPackedCellsIntoSpans()

```typescript
/**
 * Parse packed binary row data directly into TextSpan array.
 * Avoids creating Cell, CellAttributes, or Line objects.
 *
 * Binary format per cell:
 *   Inline: char_len(1) + char_data(char_len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 *   Overflow: 0xFF(1) + len_hi(1) + len_lo(1) + utf8_data(len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 */
function groupPackedCellsIntoSpans(packed: Uint8Array, cols: number): TextSpan[] {
  const spans: TextSpan[] = [];
  let offset = 0;

  // Track current span state
  let currentText = "";
  let currentStartCol = 0;
  let currentCellCount = 0;
  let currentCells: Array<[string, number]> = [];

  // Track previous cell's attribute bytes for fast comparison
  let prevAttrOffset = -1;  // offset into packed where attrs start
  let currentAttrs: CellAttributes | null = null;

  for (let col = 0; col < cols; col++) {
    if (offset + 12 > packed.length) break;

    // Parse character data
    const charLen = packed[offset++];
    let ch: string;
    if (charLen === 0xFF) { /* overflow parse */ }
    else if (charLen === 0) { ch = ""; }
    else if (charLen === 1) { ch = String.fromCharCode(packed[offset++]); }
    else { /* multi-byte UTF-8 parse */ }

    // Read width
    const width = packed[offset++];

    // Attribute bytes start here (10 bytes: fg 4 + bg 4 + flags 2)
    const attrStart = offset;
    offset += 10; // skip fg(4) + bg(4) + flags(2)

    // Handle zero-width cells
    if (width === 0) {
      if (ch === "" || ch === " ") continue;  // wide char placeholder
      // Combining mark - merge with previous
      if (currentCells.length > 0) {
        currentCells[currentCells.length - 1][0] += ch;
        currentText += ch;
      }
      continue;
    }

    // Fast attribute comparison: compare 10 bytes inline
    const attrsMatch = prevAttrOffset >= 0 &&
      packedAttrsEqual(packed, prevAttrOffset, attrStart);

    if (currentAttrs === null || !attrsMatch) {
      // Save previous span
      if (currentAttrs !== null) {
        spans.push({ text: currentText, attrs: currentAttrs,
                     startCol: currentStartCol, cellCount: currentCellCount,
                     cells: currentCells });
      }
      // Start new span
      currentAttrs = unpackAttrsFromBinary(packed, attrStart);
      currentText = ch;
      currentStartCol = col;
      currentCellCount = width;
      currentCells = [[ch, width]];
    } else {
      // Extend current span
      currentText += ch;
      currentCellCount += width;
      currentCells.push([ch, width]);
    }

    prevAttrOffset = attrStart;
  }

  // Final span
  if (currentAttrs !== null && currentText.length > 0) {
    spans.push({ text: currentText, attrs: currentAttrs,
                 startCol: currentStartCol, cellCount: currentCellCount,
                 cells: currentCells });
  }

  return spans;
}

/** Compare 10 attribute bytes (fg 4 + bg 4 + flags 2) at two offsets. */
function packedAttrsEqual(buf: Uint8Array, a: number, b: number): boolean {
  return buf[a]===buf[b] && buf[a+1]===buf[b+1] && buf[a+2]===buf[b+2] &&
         buf[a+3]===buf[b+3] && buf[a+4]===buf[b+4] && buf[a+5]===buf[b+5] &&
         buf[a+6]===buf[b+6] && buf[a+7]===buf[b+7] && buf[a+8]===buf[b+8] &&
         buf[a+9]===buf[b+9];
}

/** Unpack CellAttributes from 10 binary bytes (only when starting new span). */
function unpackAttrsFromBinary(buf: Uint8Array, offset: number): CellAttributes {
  // fg: buf[offset..offset+4], bg: buf[offset+4..offset+8], flags: buf[offset+8..offset+10]
  // Same logic as parsePackedRow() but without creating Cell object
}
```

### WasmLineProxy Dirty Getter

```typescript
export class WasmLineProxy implements LineAccessor {
  // dirty becomes a getter delegating to WASM bitset
  get dirty(): boolean {
    return this.core.is_row_dirty(this.row);
  }

  set dirty(_: boolean) {
    // No-op: dirty is managed by WASM core
  }

  markDirty(): void {
    this.core.mark_row_dirty(this.row);
  }

  clearDirty(): void {
    // No-op: dirty cleared at renderer level via core.clear_dirty()
  }
}
```

### Kitty image_id Correlation

**Rust side (generate_kitty_sequence):**
```rust
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

pub fn generate_kitty_sequence(img: &DynamicImage) -> Result<(String, u32), CommandError> {
    let image_id = NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed);
    // Wrap around: skip 0 (0 means "no image_id" in Kitty protocol)
    let image_id = if image_id == 0 {
        NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        image_id
    };

    // Use image_id in all chunks
    // ... existing chunking logic with dynamic image_id ...

    Ok((output, image_id))
}
```

**Rust side (wait_for_kitty_response):**
```rust
pub fn wait_for_kitty_response(expected_id: u32) -> Result<(), CommandError> {
    // State machine reads ESC _G i={id};{status} ESC \
    // Verify parsed id matches expected_id
    // Retry/ignore responses with non-matching id
}
```

### Dependencies

**Internal Dependencies:**
- TerminalCore WASM: `get_row_packed()`, `get_scrollback_row_packed()`, `is_row_dirty()`, `get_dirty_rows()` (all existing)
- CanvasRenderer: rendering pipeline (existing)
- Kitty protocol: `generate_kitty_sequence()`, `wait_for_kitty_response()` (existing)

**External Dependencies:**
- None (no new crate or npm dependencies)

### File Structure

```
wasm/src/
  (no changes - all needed WASM APIs already exist)

src/terminal/
  canvas-renderer.ts       # Add renderLinePacked(), integrate packed path
  canvas-renderer.test.ts  # Add packed rendering tests
  wasm/terminal-core.ts    # WasmLineProxy dirty getter delegation

src-tauri/src/protocols/
  kitty.rs                 # AtomicU32 image_id, return (String, u32)

src-tauri/src/commands/
  image.rs                 # Pass image_id to wait_for_kitty_response()

src-tauri/src/image/
  kitty.rs                 # (no changes expected)
```

## Test Scenarios

### Unit Tests (Rust)
- [ ] `generate_kitty_sequence()` returns unique image_id on each call
- [ ] image_id skips 0 on wrap-around
- [ ] `wait_for_kitty_response()` correctly parses response with matching id
- [ ] `wait_for_kitty_response()` handles mismatched id gracefully

### Unit Tests (TypeScript)
- [ ] `groupPackedCellsIntoSpans()` produces identical spans to `groupCellsIntoSpans()` for same cell data
- [ ] `groupPackedCellsIntoSpans()` handles empty row (all default cells)
- [ ] `groupPackedCellsIntoSpans()` handles wide characters (width=2 + placeholder width=0)
- [ ] `groupPackedCellsIntoSpans()` handles combining marks (width=0 with character)
- [ ] `groupPackedCellsIntoSpans()` handles overflow characters (char_len=0xFF)
- [ ] `groupPackedCellsIntoSpans()` handles truncated packed data safely
- [ ] `groupPackedCellsIntoSpans()` correctly groups consecutive cells with same attributes
- [ ] `groupPackedCellsIntoSpans()` splits spans at attribute boundaries
- [ ] WasmLineProxy.dirty getter returns WASM core dirty state
- [ ] WasmLineProxy.markDirty() sets WASM core dirty bit

### Integration Tests
- [ ] CanvasRenderer.render() uses packed path when WASM core available
- [ ] CanvasRenderer.forceRender() uses packed path for viewport + scrollback
- [ ] All existing TS tests pass (1824+ tests)
- [ ] All existing Rust tests pass (398+ WASM, 362+ backend)

### Edge Cases
- [ ] Packed data with mixed inline/overflow characters in same row
- [ ] Row with all wide characters (80 cols → 40 visible chars)
- [ ] Row with combining marks creating complex grapheme clusters
- [ ] Scrollback row at ring buffer boundary
- [ ] image_id AtomicU32 wrap-around from u32::MAX to 1 (skipping 0)
- [ ] Concurrent `emterm image` commands with different image_ids

### Performance Tests
- [ ] Benchmark: packed vs LineAccessor rendering path (time per row)
- [ ] Verify dirty row rendering completes within 2ms target

## Error Handling

### Packed Data Safety
- Bounds checking: `offset + 12 > packed.length` guard prevents out-of-bounds read
- Truncated data: parsing stops at current column, remaining cells are skipped
- Invalid char_len: values 0x00-0xFE are valid lengths, 0xFF is overflow marker

### Image ID Correlation
- Mismatched response id: log warning, continue waiting (with timeout)
- No response received: existing timeout mechanism handles this

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] dirty row rendering within 2ms (NFR1)
- [ ] Zero intermediate object allocation in packed path (NFR2)
- [ ] WASM binary under 80KB (NFR3)
- [ ] All existing tests pass (NFR4)
- [ ] Packed data parsing handles edge cases safely (NFR5)
- [ ] Concurrent `emterm image` commands work correctly with unique image_ids
- [ ] Code review completed

## Open Questions

> **Note**: No unresolved requirements.

## References

- Packed binary format: `src/terminal/wasm/terminal-core.ts:parsePackedRow()`
- Current renderer: `src/terminal/canvas-renderer.ts`
- Kitty protocol: `src-tauri/src/protocols/kitty.rs`
- WASM roadmap: `tmp/wasm.md`
