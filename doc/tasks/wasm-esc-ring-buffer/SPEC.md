# Feature: WASM ESC Handlers + Ring Buffer Integration (Sprint 5)

## Overview

Port ESC handlers (DECSC/DECRC, IND, NEL, RI, HTS, RIS, Charset) from TypeScript to Rust/WebAssembly and integrate a unified Ring Buffer into WASM linear memory. This merges the viewport (currently flat `Vec<Cell>`) and scrollback (currently JS `Line` objects) into a single WASM Ring Buffer, eliminating the scroll bridge pattern and JS GC pressure from scrollback management. The reflow algorithm is also implemented in Rust for WASM-internal resize. This eliminates `syncCursorAttrsToWasm()` entirely.

## Objectives

- Port all 9 ESC handlers to WASM, achieving 100% handler coverage (Print, C0, CSI, ESC)
- Implement unified Ring Buffer in WASM linear memory (viewport + scrollback in single flat array)
- Eliminate the scroll bridge pattern (handle_print/execute/scroll_up return values for TS scrollback)
- Implement reflow algorithm in Rust for WASM-internal resize
- Remove `syncCursorAttrsToWasm()` completely (last call site was cursor restore, now WASM-internal)
- Convert UnifiedBuffer to thin WASM wrapper (JS fallback path maintained)

## User Stories

### US1: ESC Handlers via WASM

As a terminal handler, I want all ESC sequences processed entirely within WASM, so that DECSC/DECRC, index operations, and charset changes require only one WASM boundary crossing.

**Acceptance Criteria:**
- [ ] `core.handle_esc(action_code: u8, data: u8)` dispatches to appropriate WASM handler
- [ ] SaveCursor (DECSC): saves cursor position + attrs in WASM
- [ ] RestoreCursor (DECRC): restores cursor position + attrs from WASM, no `syncCursorAttrsToWasm()` needed
- [ ] Index (IND): cursor moves down, scrolls up if at bottom of scroll region — scroll handled entirely in WASM Ring Buffer
- [ ] NextLine (NEL): carriage return + index (WASM-internal)
- [ ] ReverseIndex (RI): cursor moves up, scrolls down if at top of scroll region (WASM-internal)
- [ ] HorizontalTabSet (HTS): sets tab stop at current cursor column
- [ ] ResetToInitialState (RIS): full terminal reset including Ring Buffer
- [ ] SetG0CharSet/SetG1CharSet: charset set via existing WASM setters
- [ ] JS fallback: when WASM unavailable, existing TS handlers used unchanged

### US2: Unified WASM Ring Buffer

As the terminal buffer manager, I want the viewport and scrollback stored in a single WASM Ring Buffer, so that JS GC pressure from thousands of Line/Cell objects is eliminated and scroll operations are WASM-internal.

**Acceptance Criteria:**
- [ ] Ring Buffer stores all lines (scrollback + viewport) in a flat `Vec<Cell>` with `capacity × cols` entries
- [ ] Viewport = last `rows` lines of the ring buffer
- [ ] Scrollback = lines before the viewport
- [ ] Ring buffer operations (push, get_line, viewport access) work correctly
- [ ] Scrollback capacity configurable via existing `scrollback_lines` setting (default 10,000)
- [ ] Line eviction on capacity overflow works correctly
- [ ] `get_row_packed()` updated to index into ring buffer viewport area
- [ ] Scrollback read API: `get_scrollback_row_packed(index)` for renderer
- [ ] `get_scrollback_length()` returns current scrollback line count

### US3: Scroll Bridge Elimination

As the terminal state manager, I want scroll operations to complete entirely within WASM, eliminating all WASM-TS bridge calls for scrolling.

**Acceptance Criteria:**
- [ ] `handle_print(cp)` no longer returns scroll count (returns 0, scroll handled internally)
- [ ] `handle_execute(byte)` returns only BEL sentinel (0xFE), LF scroll handled internally (returns 0 instead of count)
- [ ] `handle_scroll_up(count)` always returns 0 (scroll region and full screen both WASM-internal)
- [ ] IND/NEL scroll operations are WASM-internal (no bridge)
- [ ] TS-side scroll bridge code becomes dead code for WASM path (kept for JS fallback)
- [ ] `wasmRowToLine()` no longer needed for scroll bridge (kept for renderer/fallback)

### US4: WASM Reflow

As the terminal, I want resize/reflow to execute entirely within WASM, so that the large data transfer between TS and WASM for reflow is eliminated.

**Acceptance Criteria:**
- [ ] `core.resize_reflow(new_cols, new_rows, cursor_col, cursor_row) -> (u16, u16)` performs full reflow in WASM
- [ ] Algorithm: drain ring → join wrapped lines → trim trailing blanks → re-split at new width → write back
- [ ] Cursor position tracked through reflow and returned as (new_col, new_row)
- [ ] Empty line trimming from bottom when reflowed lines exceed new row count
- [ ] Same-width resize handles row count change only (no reflow)
- [ ] Scroll region invalidated after resize
- [ ] Alternate buffer: `resize_no_reflow(new_cols, new_rows)` — resize without reflow

### US5: syncCursorAttrsToWasm Elimination

As the terminal state manager, I want cursor save/restore to complete entirely within WASM, eliminating the last `syncCursorAttrsToWasm()` call site.

**Acceptance Criteria:**
- [ ] `syncCursorAttrsToWasm()` method removed from TerminalState
- [ ] `syncCursorAttrsToWasm()` removed from TerminalStateAccessor interface
- [ ] All call sites removed (esc_handlers.ts:76, state.ts:959, csi_char_attrs.ts:27, csi_modes.ts:51)
- [ ] Cursor restore in WASM restores fg, bg, flags (already implemented in WASM `restore_cursor()`)

### US6: UnifiedBuffer Thin Wrapper

As the frontend architecture, I want UnifiedBuffer to be a thin WASM wrapper in WASM mode while maintaining the full JS implementation as fallback.

**Acceptance Criteria:**
- [ ] WASM mode: scroll operations delegate to WASM Ring Buffer
- [ ] WASM mode: viewport/scrollback access via WASM APIs
- [ ] WASM mode: resize delegates to WASM reflow
- [ ] JS fallback mode: unchanged behavior
- [ ] Renderer can read both viewport and scrollback from WASM

## Technical Requirements

### Functional Requirements

#### ESC Handlers
- **FR1:** `TerminalCore.handle_esc(action: u8, data: u8) -> u8` dispatches ESC actions. Action codes: 0=SaveCursor, 1=RestoreCursor, 2=Index, 3=NextLine, 4=ReverseIndex, 5=HorizontalTabSet, 6=ResetToInitialState, 7=SetG0CharSet, 8=SetG1CharSet. Returns 0 on success.
- **FR2:** SaveCursor saves cursor position (col, row), attributes (fg, bg, flags), charset state, origin mode, and wrap pending flag.
- **FR3:** RestoreCursor restores all saved state. If no saved state, resets cursor to (0, 0) with default attributes.
- **FR4:** Index (IND) moves cursor down one row. If cursor is at bottom of scroll region, scrolls up within WASM Ring Buffer (pushes top line to scrollback for full-screen, or shifts lines for scroll region).
- **FR5:** NextLine (NEL) sets cursor column to 0, then performs Index.
- **FR6:** ReverseIndex (RI) moves cursor up one row. If cursor is at top of scroll region, scrolls down (inserts blank line at top, pushes bottom line out).
- **FR7:** HorizontalTabSet (HTS) sets a tab stop at the current cursor column.
- **FR8:** ResetToInitialState (RIS) resets all terminal state: Ring Buffer cleared, cursor reset, modes reset, tab stops reset to every 8 columns, charsets reset.
- **FR9:** SetG0CharSet/SetG1CharSet set character set (0=ASCII, 1=DecLineDrawing) via existing WASM setters.

#### Ring Buffer
- **FR10:** `TerminalCore` replaces flat `grid: Vec<Cell>` with Ring Buffer: `ring_cells: Vec<Cell>` of size `capacity × cols`, `ring_wrapped: Vec<bool>` of size `capacity`, `ring_head: usize`, `ring_size: usize`, `ring_capacity: usize`.
- **FR11:** Viewport occupies the last `rows` lines of the ring buffer. Viewport row `r` maps to absolute index `(ring_head + ring_size - rows + r) % ring_capacity`.
- **FR12:** Scrollback line `i` (0 = oldest) maps to absolute index `(ring_head + i) % ring_capacity`.
- **FR13:** `ring_push()` adds a blank line to the end. If at capacity, head advances (oldest line evicted).
- **FR14:** `get_scrollback_length() -> u32` returns `ring_size - rows` (clamped to >= 0).
- **FR15:** Ring Buffer capacity = `scrollback_lines + rows`. Configured via `new(cols, rows, scrollback_lines)`.
- **FR16:** Dirty tracking: `dirty: Vec<u64>` bitset covers viewport rows only (unchanged from current).

#### Scroll Operations (WASM-internal)
- **FR17:** Full-screen scroll up: read top viewport row into scrollback area (ring_push), shift viewport up, clear bottom row. No TS bridge call.
- **FR18:** Scroll region scroll up: shift rows within region, clear bottom rows. No scrollback interaction.
- **FR19:** Scroll down (full screen or region): shift rows down, clear top rows. No scrollback interaction.
- **FR20:** `handle_print()` return type changes: returns `u8` but value is always 0 in Ring Buffer mode (scroll handled internally). BEL from C0 still returns 0xFE sentinel.
- **FR21:** `handle_execute()` return type unchanged: returns BEL sentinel (0xFE) or 0. LF/VT/FF scroll is WASM-internal (returns 0 instead of scroll count).
- **FR22:** `handle_scroll_up()` returns 0 always (both full-screen and scroll region are WASM-internal).

#### Reflow
- **FR23:** `resize_reflow(new_cols: u16, new_rows: u16, cursor_col: u16, cursor_row: u16) -> u32` performs full reflow. Returns packed cursor position (col << 16 | row).
- **FR24:** Reflow algorithm:
  1. Save all ring buffer lines (cells + wrapped flags)
  2. Join consecutive wrapped lines into logical lines
  3. Trim trailing blank cells from each logical line
  4. Re-split logical lines at new column width, setting wrapped flag on continuation lines
  5. Trim empty lines from bottom if total exceeds new row count
  6. Ensure at least `new_rows` lines exist
  7. Write reflowed lines to resized ring buffer
  8. Track cursor position through reflow and return adjusted position
- **FR25:** Same-width resize: skip reflow, adjust row count only. Add blank lines (grow) or trim empty bottom lines (shrink).
- **FR26:** `resize_no_reflow(new_cols: u16, new_rows: u16)` for alternate buffer: resize ring buffer without reflow, clear/add lines as needed.
- **FR27:** Scroll region invalidated after any resize.

#### Scrollback Access (Renderer)
- **FR28:** `get_scrollback_row_packed(index: u32) -> Vec<u8>` returns scrollback line in same packed format as `get_row_packed()`.
- **FR29:** `get_scrollback_length() -> u32` returns current number of scrollback lines.
- **FR30:** `get_scrollback_text(index: u32) -> String` returns text content of scrollback line (for search/selection).

#### syncCursorAttrsToWasm Removal
- **FR31:** Remove `syncCursorAttrsToWasm()` method from TerminalState class.
- **FR32:** Remove `syncCursorAttrsToWasm()` from TerminalStateAccessor interface.
- **FR33:** Remove all call sites: esc_handlers.ts (RestoreCursor), state.ts (executeModAction RestoreCursor), csi_char_attrs.ts (handleSgr fallback), csi_modes.ts (handleRestoreCursor fallback).

#### Integration
- **FR34:** `state.ts` processAction: add WASM ESC dispatch similar to existing CSI dispatch. If WASM grid available, call `handleEscWasm()`. Otherwise fallback to TS `handleEsc()`.
- **FR35:** UnifiedBuffer WASM mode: scrollUp/Down delegate to WASM. No wasmRowToLine bridge for scroll.
- **FR36:** UnifiedBuffer WASM mode: resize calls `core.resize_reflow()` instead of TS reflow logic.
- **FR37:** JS fallback: all existing TS code paths unchanged and functional.

### Non-Functional Requirements

- **NFR1 - Performance:** All scroll operations are WASM-internal (0 WASM-TS boundary crossings for scroll).
- **NFR2 - Performance:** Reflow performance >= TS implementation (measured by resize benchmark).
- **NFR3 - Memory:** Scrollback memory usage = `(scrollback_lines + rows) × cols × 32` bytes in WASM linear memory.
- **NFR4 - Compatibility:** All existing TypeScript tests pass (1824+).
- **NFR5 - Compatibility:** JS fallback path unchanged and functional.
- **NFR6 - Compatibility:** vttest basic tests unchanged.
- **NFR7 - Binary size:** WASM binary < 70KB (Sprint 4: 51.4KB, ~19KB budget for Ring Buffer + reflow).
- **NFR8 - Compatibility:** Existing `scrollback_lines` setting works with WASM Ring Buffer.

## Implementation Approach

### Architecture

**Before (Sprint 4):**
```
┌─────────────────────────────────────────────────┐
│ WASM TerminalCore                               │
│                                                  │
│  grid: Vec<Cell>           [flat viewport]       │
│  cursor, modes, tab_stops                        │
│  All C0/CSI/Print handlers                       │
│  Scroll: returns count for TS bridge             │
└──────────────────────┬──────────────────────────┘
                       │ scroll bridge (return values)
                       ↓
┌─────────────────────────────────────────────────┐
│ TypeScript                                      │
│                                                  │
│  UnifiedBuffer                                   │
│    ring: (Line | null)[]   [JS scrollback]       │
│    wasmGrid: WasmGrid      [viewport delegation] │
│    scrollUp() → wasmRowToLine → push → shift     │
│    resize() → materialize → reflow → write back  │
│                                                  │
│  ESC handlers (TS)                               │
│  syncCursorAttrsToWasm() (1 call site)           │
└─────────────────────────────────────────────────┘
```

**After (Sprint 5):**
```
┌─────────────────────────────────────────────────┐
│ WASM TerminalCore                               │
│                                                  │
│  Ring Buffer (flat Vec<Cell>)                    │
│    ring_cells: Vec<Cell>  [scrollback+viewport]  │
│    ring_wrapped: Vec<bool>                       │
│    ring_head, ring_size, ring_capacity           │
│    viewport = last N rows                        │
│                                                  │
│  cursor, modes, tab_stops                        │
│  All C0/CSI/Print/ESC handlers                   │
│  Scroll: WASM-internal (no bridge)               │
│  Reflow: Rust implementation                     │
│  No syncCursorAttrsToWasm needed                 │
└──────────────────────┬──────────────────────────┘
                       │ WASM-TS boundary (minimal)
                       ↓
┌─────────────────────────────────────────────────┐
│ TypeScript                                      │
│                                                  │
│  UnifiedBuffer (thin wrapper)                    │
│    WASM mode: all ops delegate to WASM           │
│    JS fallback: full implementation maintained   │
│                                                  │
│  Renderer reads from WASM:                       │
│    get_row_packed(row)      [viewport]           │
│    get_scrollback_row_packed(i) [scrollback]     │
│                                                  │
│  Remaining TS: OSC/APC/DCS callbacks, UI, BEL   │
└─────────────────────────────────────────────────┘
```

### Data Flow

**ESC Index (IND) — Full Screen, WASM-internal:**
```
TS processAction("Esc", { action: "Index" })
  → grid = getActiveWasmGrid()
  → grid.core.handle_esc(2, 0)  [single WASM call]
  → done

  Inside WASM handle_index():
    → cursor at bottom of scroll region?
    → YES: scroll_up_internal(1)
      → ring_push(blank_line)  [old top row becomes scrollback]
      → shift viewport cells up by 1
      → clear bottom row
      → mark dirty
    → move cursor down by 1
```

**Print with scroll — WASM-internal (no bridge):**
```
TS processAction("Print", cp)
  → scrollCount = grid.core.handle_print(cp)
  → scrollCount == 0 → done (scroll handled internally)

  Inside WASM handle_print(cp):
    → write cell at cursor position
    → advance cursor (autoWrap handling)
    → if wrap causes scroll: scroll_up_internal(1)
      → ring_push(blank_line)
      → shift + clear
    → return 0  [not count]
```

**Resize with Reflow — WASM-internal:**
```
TS onResize(newCols, newRows)
  → const packed = grid.core.resize_reflow(newCols, newRows, cursorCol, cursorRow)
  → const newCol = packed >> 16
  → const newRow = packed & 0xFFFF
  → cursor.col = newCol
  → cursor.row = newRow
  → renderer.invalidateAll()

  Inside WASM resize_reflow():
    → save all lines from ring buffer
    → join wrapped lines into logical lines
    → trim trailing blanks
    → re-split at new column width
    → trim empty bottom lines
    → resize ring buffer capacity (scrollback_cap + newRows)
    → write scrollback + viewport back
    → track and return cursor position
```

**Scrollback Read (Renderer):**
```
Renderer needs to display scrolled-back content:
  → const len = grid.core.get_scrollback_length()
  → for (let i = offset; i < offset + visibleRows; i++)
      → const packed = grid.core.get_scrollback_row_packed(i)
      → parse and render cells from packed data
```

### WASM API Changes

```rust
#[wasm_bindgen]
impl TerminalCore {
    // ── Constructor (MODIFIED) ────────────────────────────
    /// Create with scrollback capacity.
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u16, rows: u16, scrollback_lines: u32) -> Self;

    // ── ESC Handlers (NEW) ────────────────────────────────
    /// Handle ESC action. Returns 0 on success.
    /// action: 0=SaveCursor, 1=RestoreCursor, 2=Index, 3=NextLine,
    ///         4=ReverseIndex, 5=HTS, 6=RIS, 7=SetG0, 8=SetG1
    /// data: charset value for SetG0/SetG1 (0=ASCII, 1=DecLineDrawing)
    pub fn handle_esc(&mut self, action: u8, data: u8) -> u8;

    // ── Ring Buffer APIs (NEW) ────────────────────────────
    /// Get number of scrollback lines.
    pub fn get_scrollback_length(&self) -> u32;

    /// Get scrollback line in packed format (same as get_row_packed).
    pub fn get_scrollback_row_packed(&self, index: u32) -> Vec<u8>;

    /// Get text content of scrollback line.
    pub fn get_scrollback_text(&self, index: u32) -> String;

    // ── Resize/Reflow (NEW) ──────────────────────────────
    /// Resize with full reflow. Returns packed cursor (col << 16 | row).
    pub fn resize_reflow(
        &mut self, new_cols: u16, new_rows: u16,
        cursor_col: u16, cursor_row: u16
    ) -> u32;

    /// Resize without reflow (alternate buffer).
    pub fn resize_no_reflow(&mut self, new_cols: u16, new_rows: u16);

    // ── Modified APIs ────────────────────────────────────
    // handle_print(cp) → still returns u8, but always 0 (scroll internal)
    // handle_execute(byte) → returns 0 or BEL sentinel (scroll internal)
    // handle_scroll_up(count) → returns 0 always (scroll internal)

    // ── Existing APIs (UPDATED for Ring Buffer) ──────────
    // get_row_packed(row) → indexes into ring buffer viewport area
    // reset() → also resets ring buffer
    // resize(cols, rows) → REMOVED (replaced by resize_reflow/resize_no_reflow)

    // ── REMOVED APIs ─────────────────────────────────────
    // set_cursor_fg/bg/flags → still exist (for JS fallback SGR sync)
    // save_cursor/restore_cursor → now internal (called from handle_esc)
}
```

### Internal Rust Implementation

```rust
// ── Ring Buffer fields in TerminalCore ────────────────
pub(crate) ring_cells: Vec<Cell>,     // capacity × cols
pub(crate) ring_wrapped: Vec<bool>,   // capacity
pub(crate) ring_head: usize,          // oldest line index
pub(crate) ring_size: usize,          // current line count
pub(crate) ring_capacity: usize,      // max lines (scrollback + rows)

impl TerminalCore {
    // ── Ring Buffer internals ─────────────────────────
    /// Get cell index in ring_cells for (absolute_line, col).
    fn ring_cell_index(&self, abs_line: usize, col: u16) -> usize;

    /// Get absolute line index for viewport row.
    fn viewport_abs(&self, row: u16) -> usize;

    /// Get absolute line index for scrollback line.
    fn scrollback_abs(&self, index: u32) -> usize;

    /// Push a new blank line to ring buffer.
    /// Returns evicted line count (0 or 1).
    fn ring_push_blank(&mut self) -> u32;

    /// Internal scroll up: push top to scrollback (or discard), shift, clear bottom.
    fn scroll_up_internal(&mut self, count: u16);

    /// Internal scroll down: shift down, clear top.
    fn scroll_down_internal(&mut self, count: u16);

    // ── ESC Handler internals ─────────────────────────
    fn esc_save_cursor(&mut self);
    fn esc_restore_cursor(&mut self);
    fn esc_index(&mut self);
    fn esc_next_line(&mut self);
    fn esc_reverse_index(&mut self);
    fn esc_horizontal_tab_set(&mut self);
    fn esc_reset(&mut self);

    // ── Reflow internals ──────────────────────────────
    fn reflow_drain(&self) -> Vec<Cell>;
    fn reflow_join_wrapped(&self, cells: &[Cell]) -> Vec<LogicalLine>;
    fn reflow_split_at_width(&self, logical: &[LogicalLine], new_cols: u16) -> Vec<PhysicalLine>;
    fn reflow_write_back(&mut self, lines: Vec<PhysicalLine>);
}

/// ESC action code constants
const ESC_SAVE_CURSOR: u8 = 0;
const ESC_RESTORE_CURSOR: u8 = 1;
const ESC_INDEX: u8 = 2;
const ESC_NEXT_LINE: u8 = 3;
const ESC_REVERSE_INDEX: u8 = 4;
const ESC_HTS: u8 = 5;
const ESC_RIS: u8 = 6;
const ESC_SET_G0: u8 = 7;
const ESC_SET_G1: u8 = 8;
```

### TS handleEscWasm Changes

```typescript
/**
 * Handle ESC action via WASM.
 */
private handleEscWasm(grid: WasmGrid, action: EscAction): boolean {
    switch (action.action) {
      case "SaveCursor":
        grid.core.handle_esc(ESC_SAVE_CURSOR, 0);
        return true;
      case "RestoreCursor":
        grid.core.handle_esc(ESC_RESTORE_CURSOR, 0);
        // Sync cursor position from WASM to TS
        this.cursor.col = grid.core.get_cursor_col();
        this.cursor.row = grid.core.get_cursor_row();
        // attrs are in WASM, no syncCursorAttrsToWasm needed
        this.wrapPending = false;
        return true;
      case "Index":
        grid.core.handle_esc(ESC_INDEX, 0);
        this.cursor.row = grid.core.get_cursor_row();
        return true;
      case "NextLine":
        grid.core.handle_esc(ESC_NEXT_LINE, 0);
        this.cursor.col = grid.core.get_cursor_col();
        this.cursor.row = grid.core.get_cursor_row();
        return true;
      case "ReverseIndex":
        grid.core.handle_esc(ESC_REVERSE_INDEX, 0);
        this.cursor.row = grid.core.get_cursor_row();
        return true;
      case "HorizontalTabSet":
        grid.core.handle_esc(ESC_HTS, 0);
        return true;
      case "ResetToInitialState":
        grid.core.handle_esc(ESC_RIS, 0);
        // Also reset TS-side state
        this.resetTsState();
        return true;
      case "SetG0CharSet":
        grid.core.handle_esc(ESC_SET_G0, charsetToByte(action.data));
        return true;
      case "SetG1CharSet":
        grid.core.handle_esc(ESC_SET_G1, charsetToByte(action.data));
        return true;
      case "Unknown":
        return true; // Ignore
      default:
        return false;
    }
}
```

### UnifiedBuffer Changes (WASM Mode)

```typescript
// scrollUp - WASM mode: delegate entirely to WASM
scrollUp(count: number = 1): void {
    if (this.wasmGrid) {
        // WASM Ring Buffer handles everything internally
        // scroll_up_internal is called by handle_scroll_up, handle_execute, etc.
        // This method is only called for explicit SU CSI in WASM mode
        this.wasmGrid.core.handle_scroll_up(count);
        return;
    }
    // JS fallback: unchanged
    ...
}

// resize - WASM mode: delegate to WASM reflow
resize(cols, rows, cursorRow, cursorCol): { col, row } {
    if (this.wasmGrid) {
        const packed = this.wasmGrid.core.resize_reflow(cols, rows, cursorCol, cursorRow);
        this._cols = cols;
        this._rows = rows;
        this.scrollRegion = null;
        return { col: packed >> 16, row: packed & 0xFFFF };
    }
    // JS fallback: unchanged
    ...
}

// getLine - WASM mode: unchanged (WasmLineProxy)
// getScrollbackLine - WASM mode: read from WASM
getScrollbackLine(index: number): LineAccessor {
    if (this.wasmGrid) {
        // Return a proxy that reads from WASM scrollback
        return new WasmScrollbackLineProxy(this.wasmGrid.core, index);
    }
    // JS fallback: unchanged
    return this.getAbsolute(index);
}
```

### Scroll Bridge Elimination

After Sprint 5, the following TS-side scroll bridge patterns become no-ops in WASM mode:

| Component | Before (Sprint 4) | After (Sprint 5) |
|-----------|-------------------|-------------------|
| handle_print | Returns scroll count → TS calls buffer.scrollUp | Returns 0, scroll internal |
| handle_execute (LF) | Returns scroll count → TS calls buffer.scrollUp | Returns 0, scroll internal |
| handle_scroll_up | Full screen: returns count → TS bridge | Returns 0, scroll internal |
| ESC Index/NEL | TS calls buffer.scrollUp() | WASM-internal |
| ESC ReverseIndex | TS calls buffer.scrollDown() | WASM-internal |
| wasmRowToLine | Used in scroll bridge | No longer needed for scroll |

### syncCursorAttrsToWasm Removal

All remaining call sites:

| File | Line | Context | Action |
|------|------|---------|--------|
| `esc_handlers.ts:76` | RestoreCursor | ESC 8 handler | Remove (WASM handles restore) |
| `state.ts:959` | executeModAction | Mode action RESTORE_CURSOR | Remove (WASM handles restore) |
| `csi_char_attrs.ts:27` | handleSgr fallback | JS fallback path | Keep for JS fallback |
| `csi_modes.ts:51` | handleRestoreCursor | JS fallback path | Keep for JS fallback |

**Decision:** Remove `syncCursorAttrsToWasm()` from TerminalState. Keep the method on TerminalStateAccessor interface for JS fallback, but make it a no-op when WASM is active.

**Revised:** Actually, the JS fallback handlers (csi_char_attrs.ts, csi_modes.ts) use this via the TerminalStateAccessor interface. These paths are only taken when WASM is unavailable (getActiveWasmGrid() returns null), so `syncCursorAttrsToWasm()` would be a no-op anyway in that case. We can safely make it a no-op or remove it entirely since:
- When WASM is active: ESC/CSI handlers go through WASM path, never reaching JS fallback
- When WASM is inactive: `syncCursorAttrsToWasm()` does nothing (no WasmGrid)

**Final decision:** Remove the method entirely. The implementation already guards with `if (!grid) return` so it's always a no-op when WASM isn't available.

### File Structure Changes

```
wasm/src/
├── lib.rs              # MODIFIED: add ring_buffer mod
├── unicode.rs          # UNCHANGED
├── cell.rs             # UNCHANGED
├── terminal_core.rs    # MODIFIED: replace grid with Ring Buffer fields, update constructor
├── ring_buffer.rs      # NEW: Ring Buffer operations, reflow
├── esc_handler.rs      # NEW: ESC handler dispatch and implementations
├── print_handler.rs    # MODIFIED: scroll calls scroll_up_internal instead of returning count
├── c0_handler.rs       # MODIFIED: LF/VT/FF call scroll_up_internal instead of returning count
├── csi_cursor.rs       # MODIFIED: viewport_abs() for row calculations
├── csi_screen.rs       # MODIFIED: viewport_abs() for row calculations
├── csi_edit.rs         # MODIFIED: viewport_abs() for row calculations
├── csi_scroll.rs       # MODIFIED: scroll_up/down use scroll_up/down_internal
├── csi_modes.rs        # UNCHANGED
├── csi_device.rs       # UNCHANGED
├── sgr.rs              # UNCHANGED

src/terminal/wasm/
├── loader.ts           # UNCHANGED
├── unicode.ts          # UNCHANGED
└── terminal-core.ts    # MODIFIED: add scrollback APIs, update WasmGrid constructor

src/terminal/
├── state.ts            # MODIFIED: add handleEscWasm, remove syncCursorAttrsToWasm,
│                       #   update resize to use WASM reflow
├── unified-buffer.ts   # MODIFIED: thin wrapper for WASM mode
├── cursor.ts           # MODIFIED: remove syncCursorAttrsToWasm usage
├── handlers/
│   ├── esc_handlers.ts    # MODIFIED: remove syncCursorAttrsToWasm call
│   ├── csi_char_attrs.ts  # MODIFIED: remove syncCursorAttrsToWasm call
│   ├── csi_modes.ts       # MODIFIED: remove syncCursorAttrsToWasm call
│   ├── types.ts           # MODIFIED: remove syncCursorAttrsToWasm from interface
│   ├── index.ts           # UNCHANGED
│   └── (others)           # UNCHANGED
└── (others)               # UNCHANGED
```

### Dependencies

**No new crate dependencies.**

**No new npm dependencies.**

## Test Scenarios

### Unit Tests (Rust, `cargo test`)

#### ESC Handlers
- [ ] handle_esc SaveCursor: saves cursor position and attributes
- [ ] handle_esc RestoreCursor: restores position and attributes
- [ ] handle_esc RestoreCursor with no saved state: resets to defaults
- [ ] handle_esc Index at mid-screen: cursor moves down
- [ ] handle_esc Index at scroll region bottom (full screen): scrolls up, top line to scrollback
- [ ] handle_esc Index at scroll region bottom (partial region): scrolls within region
- [ ] handle_esc NextLine: cursor col=0 + index behavior
- [ ] handle_esc ReverseIndex at mid-screen: cursor moves up
- [ ] handle_esc ReverseIndex at scroll region top: scrolls down
- [ ] handle_esc HTS: tab stop set at cursor column
- [ ] handle_esc RIS: all state reset (cursor, modes, tab stops, ring buffer)
- [ ] handle_esc SetG0CharSet ASCII: g0_charset = 0
- [ ] handle_esc SetG0CharSet DecLineDrawing: g0_charset = 1
- [ ] handle_esc SetG1CharSet: similar to G0

#### Ring Buffer
- [ ] new(): ring buffer initialized with capacity = scrollback_lines + rows
- [ ] ring_push_blank: adds line, size increases
- [ ] ring_push_blank at capacity: head advances, oldest evicted
- [ ] viewport_abs: correct mapping for viewport rows
- [ ] scrollback_abs: correct mapping for scrollback lines
- [ ] get_scrollback_length: returns correct count (0 initially, grows with scrollback)
- [ ] ring buffer wrap-around: head + size > capacity works correctly
- [ ] viewport cell access after ring wrap-around: correct cells returned
- [ ] get_row_packed with ring buffer: returns correct viewport data
- [ ] get_scrollback_row_packed: returns correct scrollback data

#### Scroll Internal
- [ ] scroll_up_internal full screen: top line saved to scrollback, bottom cleared
- [ ] scroll_up_internal scroll region: lines shift within region, no scrollback
- [ ] scroll_up_internal count=3: 3 lines scrolled
- [ ] scroll_up_internal count > region height: clamped
- [ ] scroll_down_internal: top lines cleared, content shifts down
- [ ] handle_print with wrap causing scroll: scroll_up_internal called, returns 0
- [ ] handle_execute LF: scroll_up_internal called if at bottom, returns 0
- [ ] handle_scroll_up: always returns 0

#### Reflow
- [ ] resize_reflow same width: row count change only, no reflow
- [ ] resize_reflow wider: wrapped lines merge into single lines
- [ ] resize_reflow narrower: long lines split into multiple wrapped lines
- [ ] resize_reflow cursor tracking: cursor position correctly adjusted
- [ ] resize_reflow empty lines: trailing empty lines trimmed
- [ ] resize_reflow scrollback: scrollback lines included in reflow
- [ ] resize_reflow capacity overflow: oldest scrollback lines evicted
- [ ] resize_no_reflow: lines resized without reflow
- [ ] resize_reflow scroll region: region invalidated after resize

### Integration Tests (TypeScript, `bun test`)

#### ESC WASM Path
- [ ] SaveCursor + RestoreCursor via WASM: position and attrs preserved
- [ ] Index at bottom via WASM: scrolls up, scrollback grows
- [ ] NextLine via WASM: CR + scroll behavior
- [ ] ReverseIndex at top via WASM: scrolls down
- [ ] HTS via WASM: tab stop set correctly
- [ ] RIS via WASM: full reset

#### Ring Buffer Integration
- [ ] Scrollback grows as content scrolls: get_scrollback_length increases
- [ ] Scrollback content readable: get_scrollback_row_packed returns correct data
- [ ] Scrollback eviction at capacity: oldest lines removed

#### Scroll Bridge Elimination
- [ ] Print with scroll: no buffer.scrollUp call needed
- [ ] LF with scroll: no buffer.scrollUp call needed
- [ ] CSI SU: no buffer.scrollUp call needed

#### Reflow Integration
- [ ] Resize via WASM reflow: cursor correctly positioned
- [ ] Resize preserves scrollback content
- [ ] Alternate buffer resize: no reflow

#### syncCursorAttrsToWasm Removal
- [ ] RestoreCursor: no syncCursorAttrsToWasm call
- [ ] Mode RESTORE_CURSOR action: no syncCursorAttrsToWasm call
- [ ] WASM cursor restore preserves attributes

#### Regression
- [ ] All existing Sprint 1-4 tests pass
- [ ] All existing ESC handler tests pass
- [ ] All existing UnifiedBuffer tests pass
- [ ] All existing cursor tests pass

### Edge Cases

- [ ] Ring buffer with 0 scrollback lines (alternate buffer): no scrollback, scroll discards lines
- [ ] Ring buffer with 1 scrollback line: single line stored then evicted
- [ ] Scrollback at max capacity: eviction works correctly
- [ ] Reflow with very long logical line (>1000 cols → 80 cols): splits into many physical lines
- [ ] Reflow with cursor on a split line: cursor tracks to correct physical line
- [ ] Reflow with cursor past end of trimmed content: clamped to end
- [ ] RIS during scroll region: region cleared, ring buffer reset
- [ ] SaveCursor → resize → RestoreCursor: saved cursor clamped to new dimensions
- [ ] Index in origin mode: respects scroll region boundaries
- [ ] Multiple rapid resizes: ring buffer state consistent
- [ ] Scrollback with overflow cells (>16B graphemes): correctly stored and retrieved

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| Invalid ESC action code | action > 8 | Return 0 (no-op) |
| Scrollback index out of bounds | index >= scrollback_length | Panic in debug, clamp in release |
| Resize to 0 cols/rows | cols=0 or rows=0 | Clamp to minimum 1 |
| Ring buffer capacity overflow | scrollback_lines + rows > u32::MAX | Clamp capacity |
| WASM not initialized | getActiveWasmGrid() is null | Fall back to TS handlers |
| Reflow memory exhaustion | Very large scrollback during reflow | Temporary Vec allocation may be large |

## Performance Optimization

### Performance Goals

- Scroll: 0 WASM-TS boundary crossings (vs Sprint 4: 1+ crossings per scroll)
- Reflow: single `resize_reflow()` call (vs Sprint 4: N wasmRowToLine + TS reflow + N setRowFromCells)
- ESC handlers: 1 WASM call per ESC sequence
- Print/C0 with scroll: 1 WASM call total (vs Sprint 4: 1 WASM call + 1 TS buffer.scrollUp)

### Memory Budget (Sprint 5 additions)

| Component | Estimated Size |
|-----------|---------------|
| ESC handler code | ~1.0 KB |
| Ring Buffer ops | ~2.0 KB |
| Reflow algorithm | ~3.0 KB |
| Scrollback access APIs | ~1.0 KB |
| Ring Buffer data overhead | ~0.5 KB |
| **Total additional code** | **~7.5 KB** |

WASM binary estimate: 51.4KB + 7.5KB = ~59KB (well under 70KB limit)

### Runtime Memory

| Scrollback Lines | Cols | Viewport Rows | Total Lines | Cell Memory | Total |
|-----------------|------|---------------|-------------|-------------|-------|
| 10,000 | 80 | 24 | 10,024 | 25.7 MB | ~26 MB |
| 5,000 | 80 | 24 | 5,024 | 12.9 MB | ~13 MB |
| 10,000 | 120 | 40 | 10,040 | 38.6 MB | ~39 MB |

## Success Criteria

- [ ] `wasm-pack build` succeeds with all Sprint 5 additions
- [ ] WASM binary size < 70KB total
- [ ] All Rust unit tests pass (Sprint 1-4 tests + Sprint 5 new tests)
- [ ] All existing TypeScript tests pass (1824+)
- [ ] ESC operations produce identical results to TS handlers
- [ ] Scrollback storage in WASM linear memory (no JS Line objects for scrollback)
- [ ] Scroll operations are WASM-internal (0 bridge calls verified by test/logging)
- [ ] Reflow produces identical results to TS implementation
- [ ] `syncCursorAttrsToWasm()` completely removed
- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests unchanged
- [ ] vim/less/top switch correctly
- [ ] Scrollback view works (scroll up to see history)
- [ ] Resize with scrollback content preserves data correctly

## Implementation Phases

### Phase 1: Ring Buffer Foundation
**Goals:** Replace flat grid with Ring Buffer in TerminalCore
**Deliverables:**
- Ring Buffer fields and operations (ring_cells, ring_head, ring_size, ring_capacity)
- Constructor with scrollback_lines parameter
- Viewport/scrollback index mapping
- Updated `get_row_packed()` for ring buffer
- All existing tests pass (ring buffer is transparent for viewport-only usage)

### Phase 2: Scroll Internal
**Goals:** Make scroll operations WASM-internal
**Deliverables:**
- `scroll_up_internal()` and `scroll_down_internal()`
- Update `handle_print`, `handle_execute`, `handle_scroll_up` to use internal scroll
- Scrollback push during full-screen scroll
- All scroll-related tests pass

### Phase 3: Scrollback Access APIs
**Goals:** Expose scrollback data to TS renderer
**Deliverables:**
- `get_scrollback_length()`
- `get_scrollback_row_packed()`
- `get_scrollback_text()`
- Tests for scrollback access

### Phase 4: ESC Handlers
**Goals:** Port all ESC handlers to WASM
**Deliverables:**
- `handle_esc()` dispatcher
- All 9 ESC handler implementations
- ESC handlers use `scroll_up_internal` / `scroll_down_internal` for IND/NEL/RI
- Rust unit tests

### Phase 5: Reflow
**Goals:** Implement reflow algorithm in Rust
**Deliverables:**
- `resize_reflow()` with cursor tracking
- `resize_no_reflow()` for alternate buffer
- Same-width resize optimization
- Rust unit tests for all reflow cases

### Phase 6: TypeScript Integration
**Goals:** Wire WASM handlers into TS dispatch and thin-wrap UnifiedBuffer
**Deliverables:**
- `handleEscWasm()` in state.ts
- UnifiedBuffer thin wrapper for WASM mode
- WasmGrid constructor updated for scrollback_lines
- `syncCursorAttrsToWasm()` removed
- All TS tests pass

### Phase 7: Verification and Regression Testing
**Goals:** Full regression test and cross-validation
**Deliverables:**
- All existing tests passing
- Cross-validation: WASM vs TS produce identical results for ESC handlers
- Binary size verification
- Memory usage verification
- `bun tauri dev` smoke test
- vttest verification
- Scrollback view verification

## References

- WASM roadmap: `tmp/wasm.md`
- Sprint 4 SPEC: `doc/tasks/wasm-sgr-edit-scroll/SPEC.md`
- Current implementations:
  - `src/terminal/handlers/esc_handlers.ts` — ESC handlers (TS)
  - `src/terminal/unified-buffer.ts` — UnifiedBuffer (TS)
  - `src/terminal/cursor.ts` — CursorState with save/restore (TS)
  - `src/terminal/state.ts` — processAction dispatch (TS)
  - `wasm/src/terminal_core.rs` — TerminalCore (Rust)
  - `wasm/src/csi_scroll.rs` — Current scroll handlers (Rust)
  - `wasm/src/print_handler.rs` — Print handler with scroll return (Rust)
  - `wasm/src/c0_handler.rs` — C0 handler with scroll return (Rust)
- Settings: `src-tauri/src/commands/config.rs` — `scrollback_lines` setting (default 10,000)
