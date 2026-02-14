# Feature: WASM TerminalCore (Grid + State Data Layer)

## Overview

Port the terminal's core data structures (Cell, CellAttributes, CursorState, TerminalModes, viewport Grid) from TypeScript to Rust/WebAssembly. The WASM module (`TerminalCore`) owns the viewport grid in linear memory, eliminating JS heap allocation for cell data. TypeScript's `TerminalState` becomes a thin wrapper that delegates grid operations to WASM. This is Sprint 1 of the WASM migration roadmap (`tmp/wasm.md`).

## Objectives

- Move viewport cell data (rows x cols) from JS heap to WASM linear memory
- Provide JS APIs for all grid/cursor/mode operations used by existing TS handlers
- Maintain full compatibility with existing renderer and test suite
- Establish the foundation for Sprint 2+ handler migration

## User Stories

### US1: Cell Read/Write via WASM

As a terminal handler, I want to read and write cells through WASM APIs, so that cell data lives in linear memory instead of the JS heap.

**Acceptance Criteria:**
- [ ] `core.setCell(col, row, char, width, fg, bg, flags)` writes a cell to WASM grid
- [ ] `core.getCell*(col, row)` methods return cell properties (char, width, attrs)
- [ ] Setting a cell marks the row as dirty

### US2: Cursor Operations via WASM

As a terminal handler, I want to manage cursor position and attributes through WASM, so that cursor state is consistent with grid state.

**Acceptance Criteria:**
- [ ] `core.getCursorCol()` / `core.getCursorRow()` return current position
- [ ] `core.setCursor(col, row)` sets position with bounds clamping
- [ ] `core.saveCursor()` / `core.restoreCursor()` work correctly
- [ ] Cursor attributes (fg, bg, flags) are readable/writable

### US3: Viewport Rendering from WASM

As the canvas renderer, I want to read viewport cell data from WASM, so that rendering works with the new data layer.

**Acceptance Criteria:**
- [ ] Renderer can read all cell properties (char, width, fg, bg, flags) per cell
- [ ] Dirty row tracking identifies which rows need re-rendering
- [ ] `core.clearDirty()` resets dirty state after rendering

### US4: Terminal Resize

As the terminal state manager, I want to resize the WASM grid, so that viewport changes are handled correctly.

**Acceptance Criteria:**
- [ ] `core.resize(cols, rows)` reallocates the grid
- [ ] Existing cell data is preserved where possible
- [ ] All rows are marked dirty after resize

## Technical Requirements

### Functional Requirements

- **FR1:** `TerminalCore` struct in WASM holds a viewport grid of `rows * cols` cells in linear memory
- **FR2:** `Cell` stores character (UTF-8, variable length), display width (0/1/2), and packed attributes
- **FR3:** `PackedColor` encodes Default, Indexed(0-255), and RGB(r,g,b) in 4 bytes
- **FR4:** Style flags (bold, dim, italic, underline, blink, reverse, hidden, strikethrough) encoded as u16 bitfield
- **FR5:** `CursorState` in WASM holds position (col, row), attributes, visibility, style, blink, and saved state
- **FR6:** `TerminalModes` in WASM holds all DEC private modes as a bitfield
- **FR7:** `TabStops` managed in WASM as `Vec<bool>`
- **FR8:** Dirty tracking via per-row bitset, queryable from JS
- **FR9:** All operations exported via `#[wasm_bindgen]` with primitive-friendly signatures
- **FR10:** `core.resize(cols, rows)` reallocates grid and marks all rows dirty
- **FR11:** Line clear/fill operations: `clearLine(row)`, `clearRange(row, startCol, endCol)`
- **FR12:** `core.getLineText(row)` returns the text content of a viewport row (for selection/search)

### Non-Functional Requirements

- **NFR1 - Performance:** setCell/getCell round-trip < 100ns per call
- **NFR2 - Performance:** Full viewport read (80x120) < 1ms
- **NFR3 - Memory:** Viewport grid memory < 500KB for 80x120 (target ~32 bytes/cell)
- **NFR4 - Compatibility:** All existing 1779+ TypeScript tests pass
- **NFR5 - Binary size:** WASM binary increase < 50KB over Sprint 0 baseline (13.6KB)

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ wasm/ (Rust Crate - extended from Sprint 0)     │
│                                                  │
│  src/lib.rs                                      │
│    - Existing: Unicode APIs (Sprint 0)           │
│    - New: TerminalCore #[wasm_bindgen] exports   │
│                                                  │
│  src/terminal_core.rs (NEW)                      │
│    - TerminalCore struct                         │
│    - Grid (Vec<Cell>), CursorState, Modes        │
│                                                  │
│  src/cell.rs (NEW)                               │
│    - Cell struct (packed, 32 bytes)               │
│    - PackedColor (4 bytes)                        │
│    - Style flags (u16)                            │
│                                                  │
│  src/unicode.rs (existing from Sprint 0)         │
│    - char_width, classify_codepoint, etc.         │
│                                                  │
│  pkg/ (wasm-pack output)                          │
└──────────────────────┬──────────────────────────┘
                       │ import
                       ↓
┌─────────────────────────────────────────────────┐
│ src/terminal/wasm/ (TypeScript Glue)            │
│                                                  │
│  loader.ts (existing - unchanged)                │
│  unicode.ts (existing - unchanged)               │
│                                                  │
│  terminal-core.ts (NEW)                          │
│    - WasmGrid class (wraps TerminalCore)         │
│    - Adapts WASM API to existing Line/Cell       │
│      interfaces for compatibility                │
│                                                  │
└──────────────────────┬──────────────────────────┘
                       │ import
                       ↓
┌─────────────────────────────────────────────────┐
│ src/terminal/ (existing, modified)              │
│                                                  │
│  state.ts (MODIFIED)                             │
│    - TerminalState wraps WasmGrid for viewport   │
│    - UnifiedBuffer still manages scrollback      │
│    - Viewport operations delegate to WASM        │
│                                                  │
│  unified-buffer.ts (MODIFIED)                    │
│    - Viewport cell storage delegates to WASM     │
│    - Scrollback lines remain as JS Line objects  │
│    - Scroll: viewport line → JS Line → scrollback│
│                                                  │
│  handlers/ (UNCHANGED in this Sprint)            │
│    - Continue calling buffer.setCell() etc.       │
│    - Buffer methods now delegate to WASM          │
│                                                  │
│  canvas-renderer.ts (MINIMAL CHANGES)            │
│    - Reads cells via same Line/Cell interface     │
│    - Backed by WASM data via adapter              │
└─────────────────────────────────────────────────┘
```

### Data Layout

#### Cell (32 bytes, packed)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       16    char_data: [u8; 16]    UTF-8 inline storage (covers ASCII, CJK, most emoji)
16      1     char_len: u8           0-16 for inline, 0xFF for heap overflow
17      1     width: u8              Display width: 0, 1, or 2
18      4     fg: PackedColor        Foreground color
22      4     bg: PackedColor        Background color
26      2     flags: u16             Style bitfield
28      4     _padding              Alignment to 32 bytes

Note: Dirty tracking is row-level only (DirtyRows bitset in TerminalCore). No per-cell dirty flag — the existing JS `Cell.dirty` is derived from the row dirty state in the adapter layer.
```

For graphemes exceeding 16 bytes UTF-8 (rare, e.g., complex ZWJ emoji), use a side table (`HashMap<(u16, u16), String>` keyed by (col, row)) for overflow storage.

**Side table lifecycle rules:**
- `shift_rows_up(start, end, count)`: Remap keys in range `[start+count, end]` to `row - count`; delete keys in `[start, start+count)`
- `shift_rows_down(start, end, count)`: Remap keys in range `[start, end-count]` to `row + count`; delete keys in `(end-count, end]`
- `clear_line(row)` / `clear_line_range(row, ...)`: Delete all keys with matching row (and col range)
- `resize(cols, rows)`: Delete all keys where `col >= new_cols` or `row >= new_rows`
- `set_cell(col, row, ...)`: If new char fits inline (≤16 bytes), remove any existing overflow entry for (col, row)

#### Per-Row Metadata

In addition to the cell grid, TerminalCore stores per-row metadata:
- `wrapped: Vec<bool>` — whether each row is a soft-wrap continuation of the previous row
- `dirty: Vec<u64>` — bitset tracking which rows need re-rendering (1 bit per row, packed into u64 words)

#### PackedColor (4 bytes)

```
Byte 0: tag
  0x00 = Default
  0x01 = Indexed
  0x02 = RGB

Bytes 1-3: payload
  Default:  unused (0, 0, 0)
  Indexed:  (index, 0, 0)
  RGB:      (r, g, b)
```

#### Style Flags (u16 bitfield)

```
Bit 0: bold
Bit 1: dim
Bit 2: italic
Bit 3: underline
Bit 4: blink
Bit 5: reverse
Bit 6: hidden
Bit 7: strikethrough
Bits 8-15: reserved
```

#### TerminalModes (u32 bitfield)

```
Bit 0:  autoWrap (DECAWM, mode 7)
Bit 1:  originMode (DECOM, mode 6)
Bit 2:  cursorVisible (DECTCEM, mode 25)
Bit 3:  cursorBlink (ATT160, mode 12)
Bit 4:  reverseScreen (DECSCNM, mode 5)
Bit 5:  bracketedPaste (mode 2004)
Bit 6:  focusTracking (mode 1004)
Bit 7:  column132 (DECCOLM, mode 3)
Bit 8-9:  cursorKeys (2 bits: 0=normal, 1=application)
Bit 10-11: mouseTracking (2 bits: 0=none, 1=x10, 2=button, 3=any)
Bit 12-13: mouseEncoding (2 bits: 0=default, 1=utf8, 2=sgr)
Bits 14-31: reserved
```

### Data Flow

**Write Path (Handler → WASM Grid):**
```
TS Handler
  → state.getActiveBuffer().setCell(col, row, cell)
  → UnifiedBuffer.setCell() [TS]
  → core.set_cell(col, row, char, width, fg_tag, fg_r, fg_g, fg_b, bg_tag, bg_r, bg_g, bg_b, flags) [WASM]
  → Linear memory write + dirty mark
```

**Read Path (WASM Grid → Renderer):**
```
Renderer
  → state.getDirtyRows() → core.get_dirty_rows() [WASM → JS]
  → For each dirty row:
      → buffer.getLine(row) → WasmLineProxy [TS adapter]
      → line.getCell(col) → reads from WASM linear memory
      → cell.char, cell.width, cell.attrs → returned to renderer
  → state.clearDirty() → core.clear_dirty() [WASM]
```

**Scroll Path (Viewport → Scrollback):**
```
scrollUp():
  → Read top viewport row from WASM → create JS Line object
  → Push JS Line to scrollback (remains in JS UnifiedBuffer ring)
  → Shift WASM viewport rows up (memmove in linear memory)
  → Clear bottom row in WASM
```

### JS API (wasm_bindgen exports)

```rust
#[wasm_bindgen]
impl TerminalCore {
    // ── Construction ──
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u16, rows: u16) -> TerminalCore;

    // ── Grid dimensions ──
    pub fn cols(&self) -> u16;
    pub fn rows(&self) -> u16;
    pub fn resize(&mut self, cols: u16, rows: u16);

    // ── Cell write ──
    pub fn set_cell(
        &mut self, col: u16, row: u16,
        char_str: &str, width: u8,
        fg_tag: u8, fg_r: u8, fg_g: u8, fg_b: u8,
        bg_tag: u8, bg_r: u8, bg_g: u8, bg_b: u8,
        flags: u16,
    );
    pub fn set_cell_ascii(
        &mut self, col: u16, row: u16, byte: u8,
        fg_tag: u8, fg_r: u8, fg_g: u8, fg_b: u8,
        bg_tag: u8, bg_r: u8, bg_g: u8, bg_b: u8,
        flags: u16,
    );

    // ── Cell read ──
    pub fn get_cell_char(&self, col: u16, row: u16) -> String;
    pub fn get_cell_width(&self, col: u16, row: u16) -> u8;
    pub fn get_cell_fg(&self, col: u16, row: u16) -> u32;    // packed: tag<<24 | r<<16 | g<<8 | b
    pub fn get_cell_bg(&self, col: u16, row: u16) -> u32;
    pub fn get_cell_flags(&self, col: u16, row: u16) -> u16;

    // ── Batch cell read (renderer hot path) ──
    /// Returns packed cell data for an entire row in a single WASM call.
    /// Format per cell: [char_len:u8, char_data:var(char_len bytes), width:u8, fg:4B, bg:4B, flags:2B]
    /// Total bytes per row varies; worst case ~26 bytes/cell × cols.
    /// Overflow chars (char_len=0xFF) are followed by a u16 byte count then the UTF-8 data.
    pub fn get_row_packed(&self, row: u16) -> Vec<u8>;

    // ── Line operations ──
    pub fn clear_line(&mut self, row: u16);
    pub fn clear_line_range(&mut self, row: u16, start_col: u16, end_col: u16);
    pub fn get_line_text(&self, row: u16) -> String;
    pub fn is_line_empty(&self, row: u16) -> bool;
    pub fn get_line_wrapped(&self, row: u16) -> bool;
    pub fn set_line_wrapped(&mut self, row: u16, wrapped: bool);

    // ── Row operations (for scroll) ──
    pub fn shift_rows_up(&mut self, start_row: u16, end_row: u16, count: u16);
    pub fn shift_rows_down(&mut self, start_row: u16, end_row: u16, count: u16);
    pub fn copy_row(&mut self, src_row: u16, dst_row: u16);
    pub fn fill_row_default(&mut self, row: u16);

    // ── Cursor ──
    pub fn get_cursor_col(&self) -> u16;
    pub fn get_cursor_row(&self) -> u16;
    pub fn set_cursor(&mut self, col: u16, row: u16);
    pub fn set_cursor_col(&mut self, col: u16);
    pub fn set_cursor_row(&mut self, row: u16);
    pub fn get_cursor_visible(&self) -> bool;
    pub fn set_cursor_visible(&mut self, visible: bool);
    pub fn get_cursor_style(&self) -> u8;      // 0=block, 1=underline, 2=bar
    pub fn set_cursor_style(&mut self, style: u8);
    pub fn get_cursor_blink(&self) -> bool;
    pub fn set_cursor_blink(&mut self, blink: bool);

    // Cursor attributes (current text attrs applied to new cells)
    pub fn get_cursor_fg(&self) -> u32;
    pub fn set_cursor_fg(&mut self, tag: u8, r: u8, g: u8, b: u8);
    pub fn get_cursor_bg(&self) -> u32;
    pub fn set_cursor_bg(&mut self, tag: u8, r: u8, g: u8, b: u8);
    pub fn get_cursor_flags(&self) -> u16;
    pub fn set_cursor_flags(&mut self, flags: u16);
    pub fn reset_cursor_attrs(&mut self);

    // Save/restore (ESC 7 / ESC 8)
    pub fn save_cursor(&mut self);
    pub fn restore_cursor(&mut self);

    // ── Modes ──
    pub fn get_modes(&self) -> u32;            // packed bitfield
    pub fn set_modes(&mut self, modes: u32);
    pub fn get_mode(&self, bit: u8) -> bool;
    pub fn set_mode(&mut self, bit: u8, value: bool);

    // ── Tab stops ──
    pub fn set_tab_stop(&mut self, col: u16);
    pub fn clear_tab_stop(&mut self, col: u16);
    pub fn clear_all_tab_stops(&mut self);
    pub fn next_tab_stop(&self, from_col: u16) -> u16;

    // ── Dirty tracking ──
    pub fn get_dirty_rows(&self) -> Vec<u16>;
    pub fn is_row_dirty(&self, row: u16) -> bool;
    pub fn mark_row_dirty(&mut self, row: u16);
    pub fn mark_all_dirty(&mut self);
    pub fn clear_dirty(&mut self);

    // ── Reset ──
    pub fn reset(&mut self);
}
```

### TypeScript Adapter Layer

```typescript
// src/terminal/wasm/terminal-core.ts

import { TerminalCore } from "../../../wasm/pkg/emterm_wasm.js";

/**
 * WasmLineProxy: Adapts WASM grid row to existing Line interface.
 * Read-through proxy - reads cell data from WASM on demand.
 */
class WasmLineProxy {
  constructor(private core: TerminalCore, private row: number) {}

  get length(): number { return this.core.cols(); }
  getCell(col: number): Cell { /* read from WASM, return Cell-compatible object */ }
  get wrapped(): boolean { return this.core.get_line_wrapped(this.row); }
  set wrapped(v: boolean) { this.core.set_line_wrapped(this.row, v); }
  getText(): string { return this.core.get_line_text(this.row); }
  isEmpty(): boolean { return this.core.is_line_empty(this.row); }
}
```

### File Structure

```
wasm/src/
├── lib.rs              # Extended: TerminalCore exports + existing Unicode exports
├── unicode.rs          # Existing (Sprint 0)
├── terminal_core.rs    # NEW: TerminalCore struct, grid operations
└── cell.rs             # NEW: Cell, PackedColor, style flags

src/terminal/wasm/
├── loader.ts           # Existing (unchanged)
├── unicode.ts          # Existing (unchanged)
└── terminal-core.ts    # NEW: WasmGrid adapter, WasmLineProxy

src/terminal/
├── state.ts            # MODIFIED: viewport ops delegate to WASM
├── unified-buffer.ts   # MODIFIED: viewport backed by WASM
├── grid.ts             # MODIFIED: Cell/Line types retained for scrollback; viewport creation functions no longer used for viewport
├── cursor.ts           # MODIFIED: delegates to WASM cursor state
├── modes.ts            # MODIFIED: delegates to WASM modes
└── attributes.ts       # MODIFIED: conversion functions TS ↔ WASM packed format
```

### Dependencies

**WASM Crate (wasm/Cargo.toml) - no new dependencies:**
- `wasm-bindgen = "0.2"` (existing)

**No additional npm dependencies.**

### Scrollback Integration

During Sprint 1, scrollback remains in TypeScript:

1. **scrollUp()**: When a line scrolls out of viewport, `UnifiedBuffer` reads the top row from WASM (`core.get_line_text()`, `core.get_cell_*()`) and constructs a JS `Line` object. The JS Line is pushed into the scrollback ring.

2. **Scrollback reading**: Renderer reads scrollback lines as JS `Line` objects (no change from current behavior).

3. **switchToAlternateBuffer()**: Creates a new `TerminalCore` instance for the alternate viewport. The primary `TerminalCore` is preserved.

## Test Scenarios

### Unit Tests (Rust, `cargo test`)

- [ ] Cell: Create with ASCII character, verify char/width/attrs
- [ ] Cell: Create with CJK character (width=2), verify
- [ ] Cell: Create with emoji (multi-byte UTF-8), verify
- [ ] Cell: Overflow grapheme (>16 bytes), verify side table storage
- [ ] PackedColor: Default, Indexed(0), Indexed(255), RGB(0,0,0), RGB(255,255,255)
- [ ] Style flags: Each flag individually, all flags combined
- [ ] Grid: new(80, 24) creates 1920 empty cells
- [ ] Grid: setCell/getCell round-trip for ASCII
- [ ] Grid: setCell/getCell round-trip for wide character
- [ ] Grid: Out-of-bounds access returns default/no-op
- [ ] CursorState: Initial position (0, 0)
- [ ] CursorState: setCursor clamps to grid bounds
- [ ] CursorState: save/restore preserves position and attributes
- [ ] Modes: Default values match TS createDefaultModes()
- [ ] Modes: Set/get individual mode bits
- [ ] TabStops: Default every 8 columns
- [ ] TabStops: Set/clear/next operations
- [ ] Dirty: setCell marks row dirty
- [ ] Dirty: clearDirty resets all
- [ ] Dirty: resize marks all dirty
- [ ] Line operations: clearLine fills with empty cells
- [ ] Line operations: clearLineRange partial clear
- [ ] Line operations: getLineText skips width=0 cells
- [ ] Line operations: isLineEmpty checks all spaces
- [ ] Row operations: shiftRowsUp moves data correctly
- [ ] Row operations: shiftRowsDown moves data correctly
- [ ] Resize: grow cols preserves existing data
- [ ] Resize: shrink cols truncates
- [ ] Resize: grow rows adds empty rows
- [ ] Resize: shrink rows removes bottom rows
- [ ] Reset: All state returns to default

### Integration Tests (TypeScript, `bun test`)

- [ ] TerminalCore WASM loads and constructs successfully
- [ ] setCell + getCell round-trip matches for ASCII, CJK, emoji
- [ ] WasmLineProxy returns correct Cell data
- [ ] Renderer produces identical output with WASM-backed grid
- [ ] Cursor operations (save/restore) through WASM match TS behavior
- [ ] Mode setting/getting through WASM matches TS behavior
- [ ] All existing terminal tests pass (regression)

### Cross-Validation Tests

- [ ] For each existing grid.test.ts case: run with WASM Grid, compare output
- [ ] For each existing cursor test: run with WASM CursorState, compare
- [ ] For each existing modes test: run with WASM TerminalModes, compare

### Edge Cases

- [ ] Empty grid (0 cols or 0 rows) - constructor panics with debug_assert; callers must ensure cols > 0 && rows > 0
- [ ] Single cell grid (1x1)
- [ ] Maximum practical grid (500x200)
- [ ] Cell with empty string character
- [ ] Cell with very long grapheme cluster (>16 bytes UTF-8)
- [ ] Rapid resize cycles

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| Out-of-bounds cell access | col >= cols or row >= rows | No-op for write, default cell for read. Note: existing JS Line.getCell/setCell throws; the adapter layer (WasmLineProxy) should match existing throw behavior for compatibility |
| Invalid color tag | tag not in {0, 1, 2} | Treat as Default |
| Invalid cursor style | style not in {0, 1, 2} | Treat as block (0) |
| Overflow grapheme | char > 16 bytes UTF-8 | Store in side table HashMap |

## Performance Optimization

### Optimization Strategies

- **Batch row read:** `get_row_packed(row)` returns all cell data in a single WASM call, reducing boundary crossings from 5×cols to 1 per row
- **ASCII fast path:** `set_cell_ascii()` skips UTF-8 encoding, directly writes byte
- **Packed layout:** 32 bytes/cell enables cache-friendly linear scans
- **Batch row operations:** `shift_rows_up/down` uses `memmove` equivalent (ptr::copy)
- **Dirty bitset:** Compact u64-based bitset for dirty rows (not Vec<bool>)
- **Inline char storage:** 16-byte inline covers 99.9% of cells without heap allocation

### Memory Budget

| Component | Size (80x120 grid) |
|-----------|---------------------|
| Cells (80 * 120 * 32B) | 307.2 KB |
| Dirty bitset | 16 B |
| Cursor state | ~64 B |
| Modes + TabStops | ~96 B |
| Overflow side table | ~0 B (typical) |
| **Total** | **~308 KB** |

## Success Criteria

- [ ] `wasm-pack build` succeeds with TerminalCore exports
- [ ] WASM binary size < 64KB total (Sprint 0 baseline 13.6KB + Sprint 1 additions)
- [ ] All Rust unit tests pass
- [ ] All existing TypeScript tests pass (1779+)
- [ ] Renderer correctly displays terminal content from WASM grid
- [ ] Cursor operations work correctly through WASM
- [ ] Mode operations work correctly through WASM
- [ ] Scroll (viewport line → scrollback) works correctly
- [ ] `bun tauri dev` shows working terminal with WASM-backed grid

## Implementation Phases

### Phase 1: Rust Data Structures
**Goals:** Define Cell, PackedColor, CursorState, Modes in Rust
**Deliverables:**
- `wasm/src/cell.rs` with Cell, PackedColor, style flags
- `wasm/src/terminal_core.rs` with TerminalCore struct (grid storage, cursor, modes, tab stops, dirty tracking)
- Rust unit tests for all data structures

### Phase 2: WASM API Exports
**Goals:** Export all operations via wasm_bindgen
**Deliverables:**
- All `#[wasm_bindgen]` methods on TerminalCore
- `wasm-pack build` succeeds
- JS can construct and operate TerminalCore

### Phase 3: TypeScript Adapter Layer
**Goals:** Create WasmGrid adapter that implements existing interfaces
**Deliverables:**
- `src/terminal/wasm/terminal-core.ts` with WasmLineProxy
- `src/terminal/attributes.ts` conversion functions (TS ↔ packed)
- Adapter passes existing interface contracts

### Phase 4: Integration with TerminalState and UnifiedBuffer
**Goals:** Wire WASM grid into existing terminal pipeline
**Deliverables:**
- `state.ts` modified to use WasmGrid for viewport
- `unified-buffer.ts` modified to delegate viewport to WASM
- Scroll path (WASM viewport → JS scrollback) working
- Alternate buffer support (second TerminalCore instance)

### Phase 5: Verification and Benchmarking
**Goals:** Full regression test and performance measurement
**Deliverables:**
- All existing tests passing
- Cross-validation tests added
- Memory usage benchmark (WASM vs JS baseline)
- Cell operation benchmark

## References

- WASM roadmap: `tmp/wasm.md`
- Sprint 0 SPEC: `doc/tasks/wasm-unicode-width/SPEC.md`
- Current implementations:
  - `src/terminal/grid.ts` - Cell, Line, Grid
  - `src/terminal/state.ts` - TerminalState
  - `src/terminal/cursor.ts` - CursorState
  - `src/terminal/modes.ts` - TerminalModes
  - `src/terminal/attributes.ts` - CellAttributes
  - `src/terminal/unified-buffer.ts` - UnifiedBuffer (ring buffer)
  - `src/terminal/canvas-renderer.ts` - Renderer data access patterns
- wasm-bindgen reference: https://rustwasm.github.io/wasm-bindgen/
