# Implementation Plan: WASM ESC Handlers + Ring Buffer Integration (Sprint 5)

## Overview

Port all 9 ESC handlers to WASM and replace the flat viewport grid with a unified Ring Buffer in WASM linear memory. This integrates viewport and scrollback into a single data structure, eliminates the scroll bridge pattern, implements reflow in Rust, and removes `syncCursorAttrsToWasm()` entirely.

## Objectives

- Port all ESC handlers (DECSC, DECRC, IND, NEL, RI, HTS, RIS, SetG0, SetG1) to WASM
- Replace flat `grid: Vec<Cell>` with Ring Buffer (`ring_cells: Vec<Cell>`)
- Eliminate WASM-TS scroll bridge (all scroll operations WASM-internal)
- Implement reflow algorithm in Rust
- Remove `syncCursorAttrsToWasm()` completely
- Convert UnifiedBuffer to thin WASM wrapper

## Prerequisites

### Development Environment

- Rust toolchain with `wasm32-unknown-unknown` target
- `wasm-pack` for WASM builds
- Bun for TypeScript builds and tests
- Docker for isolated testing

### Dependencies

- No new crate dependencies
- No new npm dependencies

### Knowledge Requirements

- Ring buffer data structure (circular buffer with head/size/capacity)
- WASM linear memory model (flat arrays, no GC)
- Terminal escape sequence semantics (VT100/VT220)
- Existing eMterm WASM architecture (Sprint 1-4 patterns)

## Architecture Overview

### Technology Stack

- **Language**: Rust (WASM) + TypeScript (frontend)
- **Build**: `wasm-pack build --target web` + `bun build`
- **Key Patterns**:
  - Action code dispatch (established in Sprint 3-4)
  - Packed binary format for cell data export
  - Sentinel return values (BEL 0xFE, ED scrollback 0xFF)

### Design Approach

Replace the flat viewport grid with a ring buffer that unifies viewport and scrollback:

```
Ring Buffer (capacity = scrollback_lines + rows):
┌──────────────────────────────────────────┐
│ ... scrollback lines ...  │ viewport     │
│ (ring_head → oldest)      │ (last N rows)│
└──────────────────────────────────────────┘
  ring_size lines total, wrapping at ring_capacity
```

### Component Interaction

```
TerminalCore (Rust/WASM)
  ├── ring_buffer.rs      ← Ring Buffer operations, reflow
  ├── esc_handler.rs      ← ESC dispatch + implementations
  ├── print_handler.rs    ← Uses scroll_up_internal (modified)
  ├── c0_handler.rs       ← Uses scroll_up_internal (modified)
  ├── csi_scroll.rs       ← Uses scroll_up/down_internal (modified)
  └── terminal_core.rs    ← Ring Buffer fields, index mapping (modified)

TypeScript
  ├── state.ts            ← handleEscWasm dispatch (modified)
  ├── unified-buffer.ts   ← Thin WASM wrapper (modified)
  └── terminal-core.ts    ← WasmGrid constructor + scrollback APIs (modified)
```

## Implementation Phases

### Phase 1: Ring Buffer Foundation

**Goal**: Replace flat `grid: Vec<Cell>` with Ring Buffer fields. All existing viewport operations continue working transparently via index mapping.

**Files to Create**:
- `wasm/src/ring_buffer.rs` — Ring Buffer operations and index mapping

**Files to Modify**:
- `wasm/src/terminal_core.rs`:
  - Replace `grid`, `wrapped` with ring buffer fields
  - Update constructor to accept `scrollback_lines` parameter
  - Update `cell_index()` to use ring buffer mapping
- `wasm/src/lib.rs`:
  - Add `mod ring_buffer`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ring_cells | Flat cell storage for all lines | Allocated at capacity × cols | Viewport and scrollback cells stored |
| ring_head | Index of oldest line in ring | Valid index into ring | Advances on eviction |
| ring_size | Current number of lines | 0 ≤ ring_size ≤ ring_capacity | Equals rows initially, grows with scrollback |
| ring_capacity | Maximum line count | scrollback_lines + rows | Fixed after construction |
| viewport_abs(row) | Map viewport row to ring index | row < rows | Returns absolute ring index |
| scrollback_abs(i) | Map scrollback index to ring index | i < scrollback_length | Returns absolute ring index |
| ring_cell_index(abs, col) | Map (absolute line, col) to cell offset | Valid abs and col | Returns index into ring_cells |

**Ring Buffer Invariants**:
- `ring_size >= rows` always holds (viewport is always fully populated)
- `ring_size <= ring_capacity` always holds
- `ring_head` is the index of the oldest stored line (0 ≤ ring_head < ring_capacity)
- Lines are stored contiguously (modulo ring_capacity): `(ring_head + n) % ring_capacity` for n in `0..ring_size`
- When `ring_size == ring_capacity`, next `ring_push_blank()` evicts the oldest line (advances `ring_head`)
- Scrollback count = `ring_size - rows` (≥ 0)
- Initial state: `ring_head = 0`, `ring_size = rows` (no scrollback)

**Processing Flow**:
```
Cell access (read or write):
1. Determine target type (viewport or scrollback)
   ├─ Viewport row r → viewport_abs(r) = (ring_head + ring_size - rows + r) % ring_capacity
   └─ Scrollback index i → scrollback_abs(i) = (ring_head + i) % ring_capacity
2. Compute cell offset: abs_line * cols + col
3. Access ring_cells at computed offset
```

**Implementation Steps**:

1. **Add Ring Buffer fields to TerminalCore**
   - Replace `grid: Vec<Cell>` with `ring_cells: Vec<Cell>` sized `capacity × cols`
   - Replace `wrapped: Vec<bool>` with `ring_wrapped: Vec<bool>` sized `capacity`
   - Add `ring_head`, `ring_size`, `ring_capacity` fields
   - Key consideration: Initially `ring_size = rows`, `ring_head = 0`, so viewport starts at index 0

2. **Implement index mapping functions**
   - `viewport_abs(row)` for viewport row → absolute line index
   - `scrollback_abs(index)` for scrollback index → absolute line index
   - `ring_cell_index(abs_line, col)` for absolute line + col → cell offset
   - Key consideration: All wrapping uses modulo `ring_capacity`

3. **Update cell access paths**
   - `cell_index(col, row)` uses viewport mapping
   - `get_row_packed(row)` uses viewport mapping
   - `shift_rows_up/down` operates on ring buffer indices
   - `fill_row_default`, `copy_row`, `clear_line` use ring buffer indices
   - **Overflow table migration**: Current `HashMap<(u16, u16), String>` uses `(col, viewport_row)` keys. Re-key to `(col, absolute_ring_index)`:
     - All overflow write sites: use `viewport_abs(row)` to compute absolute index
     - All overflow read sites: use `viewport_abs(row)` or `scrollback_abs(index)` for lookup
     - `ring_push_blank()`: clean up overflow entries for evicted lines (when `ring_size == ring_capacity`)
     - `shift_rows_up/down`: remap overflow keys for shifted rows
     - Key consideration: Overflow entries in scrollback must be preserved until line eviction

4. **Update constructor**
   - Change signature to `new(cols: u16, rows: u16, scrollback_lines: u32)`
   - Allocate ring buffer at `(scrollback_lines + rows) × cols`
   - **Test migration**: All 174 existing `TerminalCore::new(cols, rows)` call sites across 10 test files must be updated to `TerminalCore::new(cols, rows, 0)`. This is a mechanical search-and-replace.
   - Key consideration: `scrollback_lines = 0` produces identical behavior to the current flat grid (ring_capacity = rows, no scrollback)

**Dependencies**:
- Requires: None (first phase)
- Blocks: Phase 2, 3, 4, 5, 6

**Testing Approach**:

*Unit Tests*:
- Constructor initializes ring buffer with correct capacity
- viewport_abs returns correct indices for all viewport rows
- Cell access through ring buffer returns same results as flat grid
- All existing Sprint 1-4 tests pass unchanged

*Integration Tests*:
- Existing TS tests pass with updated constructor

**Acceptance Criteria**:
- [ ] Ring buffer fields replace flat grid in TerminalCore
- [ ] Constructor accepts scrollback_lines parameter
- [ ] All existing Rust tests pass
- [ ] All existing TS tests pass
- [ ] `get_row_packed()` returns correct data via ring buffer

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: Performance regression from index mapping overhead
  - **Mitigation**: Ring buffer index computation is simple modulo arithmetic, negligible cost
- **Risk**: Overflow table migration complexity
  - **Mitigation**: Overflow table re-keying is performed in Phase 1 alongside cell access path updates. All overflow read/write sites are updated atomically. Eviction cleanup is deferred to Phase 2 (`ring_push_blank` implementation).

---

### Phase 2: Scroll Internal

**Goal**: Make all scroll operations WASM-internal. Scroll pushes top lines to scrollback via ring buffer instead of returning count for TS bridge.

**Files to Modify**:
- `wasm/src/ring_buffer.rs`:
  - Add `ring_push_blank()`, `scroll_up_internal()`, `scroll_down_internal()`
- `wasm/src/c0_handler.rs`:
  - `execute_line_feed()` calls `scroll_up_internal()` instead of returning count
- `wasm/src/print_handler.rs`:
  - Print with wrap-scroll calls `scroll_up_internal()` instead of returning count
- `wasm/src/csi_scroll.rs`:
  - `handle_scroll_up()` always returns 0, uses `scroll_up_internal()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ring_push_blank | Add blank line to ring end | ring_size ≤ ring_capacity | New blank line at end, oldest evicted if at capacity |
| scroll_up_internal | Full-screen or region scroll up | Valid scroll region | Top line saved to scrollback (full screen) or discarded (region) |
| scroll_down_internal | Full-screen or region scroll down | Valid scroll region | Content shifts down, top cleared |

**Processing Flow**:
```
scroll_up_internal(count):
1. Determine scroll type
   ├─ Full screen (region == entire viewport)
   │   → For each line: ring_push_blank (grows scrollback)
   │   → Shift viewport cells up within ring
   │   → Clear bottom rows
   └─ Scroll region (partial)
       → Shift rows within region (no scrollback)
       → Clear bottom rows of region
2. Mark affected rows dirty
```

**Implementation Steps**:

1. **Implement ring_push_blank**
   - Adds a blank line at ring end
   - If at capacity: advance ring_head (evicts oldest), ring_size stays
   - If below capacity: ring_size increases
   - Key consideration: Viewport relationship maintained automatically

2. **Implement scroll_up_internal and update line_feed**
   - `scroll_up_internal(count)`:
     - Full screen: ring_push_blank per line, then shift viewport cells, clear bottom
     - Scroll region: shift within region only, no scrollback interaction
     - Key consideration: Must handle both cases in one method
   - **Modify `line_feed()`**: Current signature returns `bool` (true = scroll needed). Change to call `scroll_up_internal(1)` directly when at scroll region bottom, instead of returning bool to the caller. Update `execute_line_feed()` in `c0_handler.rs` accordingly (no longer checks `line_feed()` return value).

3. **Update handle_print, handle_execute, handle_scroll_up**
   - Replace scroll count return with internal scroll call
   - handle_print: always returns 0 (scroll handled internally)
   - handle_execute: returns 0 for LF (scroll internal), 0xFE for BEL
   - handle_scroll_up: always returns 0
   - **Test assertion changes required**:
     - Tests asserting `handle_print() == 1` (scroll count) → change to `== 0`, verify scrollback via `get_scrollback_length()`
     - Tests asserting `execute_line_feed() == 1` → change to `== 0`, verify scrollback growth
     - Tests asserting `handle_scroll_up() == count` (full screen) → change to `== 0`, verify scrollback
     - Scroll region tests asserting `handle_scroll_up() == 0` → unchanged (already 0)
   - Key consideration: New tests should verify scrollback side effects instead of return values

**Dependencies**:
- Requires: Phase 1 (Ring Buffer fields)
- Blocks: Phase 3, 4, 6

**Testing Approach**:

*Unit Tests*:
- ring_push_blank increases scrollback count
- ring_push_blank at capacity evicts oldest line
- scroll_up_internal (full screen): top line in scrollback, bottom cleared
- scroll_up_internal (region): lines shift within region, no scrollback
- handle_print with wrap-scroll returns 0
- handle_execute LF returns 0 (not scroll count)
- handle_scroll_up returns 0 always

*Integration Tests*:
- Content scrolls correctly in WASM mode
- Scrollback grows with full-screen scroll

**Acceptance Criteria**:
- [ ] scroll_up_internal works for full screen and scroll region
- [ ] handle_print returns 0 (scroll internal)
- [ ] handle_execute returns 0 for LF (scroll internal), 0xFE for BEL
- [ ] handle_scroll_up returns 0 always
- [ ] Scrollback lines preserved in ring buffer
- [ ] All Rust tests pass (updated assertions)

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: Existing TS tests expect non-zero scroll return values
  - **Mitigation**: TS scroll bridge code path becomes no-op (`if scrollCount > 0` is never true), tests should still pass

---

### Phase 3: Scrollback Access APIs

**Goal**: Expose scrollback data to TS renderer via WASM APIs.

**Files to Modify**:
- `wasm/src/ring_buffer.rs`:
  - Add `get_scrollback_length()`, `get_scrollback_row_packed()`, `get_scrollback_text()`
- `wasm/src/terminal_core.rs`:
  - Export scrollback APIs via `#[wasm_bindgen]`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| get_scrollback_length | Return number of scrollback lines | Ring buffer initialized | Returns ring_size - rows (≥ 0) |
| get_scrollback_row_packed | Export scrollback line in packed format | index < scrollback_length | Returns packed binary data |
| get_scrollback_text | Export scrollback line as text | index < scrollback_length | Returns string content |

**Implementation Steps**:

1. **Implement get_scrollback_length**
   - Returns `ring_size.saturating_sub(rows as usize)` as u32
   - Key consideration: Must be clamped to 0 when no scrollback

2. **Implement get_scrollback_row_packed**
   - Uses `scrollback_abs(index)` to find ring line
   - Packs cells in same format as `get_row_packed()`
   - Key consideration: Share packing logic with existing `get_row_packed`

3. **Implement get_scrollback_text**
   - Concatenate cell text from scrollback line
   - Trim trailing whitespace
   - Key consideration: Handle overflow cells (>16B graphemes)

**Dependencies**:
- Requires: Phase 1 (Ring Buffer), Phase 2 (scroll generates scrollback)
- Blocks: Phase 6 (TS integration needs these APIs)

**Testing Approach**:

*Unit Tests*:
- get_scrollback_length returns 0 initially
- get_scrollback_length increases after full-screen scroll
- get_scrollback_row_packed returns correct data for oldest scrollback line
- get_scrollback_text returns trimmed text content

**Acceptance Criteria**:
- [ ] Scrollback length correctly tracks scrollback count
- [ ] Scrollback row packed format matches viewport packed format
- [ ] Scrollback text extraction works correctly
- [ ] All Rust tests pass

**Estimated Effort**: 小 (1-2 days)

---

### Phase 4: ESC Handlers

**Goal**: Port all 9 ESC handlers to WASM. ESC Index/NEL/RI use scroll_up/down_internal for WASM-internal scroll.

**Files to Create**:
- `wasm/src/esc_handler.rs` — ESC dispatch and handler implementations

**Files to Modify**:
- `wasm/src/lib.rs`:
  - Add `mod esc_handler`
- `wasm/src/terminal_core.rs`:
  - Export `handle_esc()` via `#[wasm_bindgen]`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_esc | Dispatch ESC action by code | Valid action code 0-8 | Action performed, returns 0 |
| esc_save_cursor | Save cursor position + attrs | — | Cursor state saved |
| esc_restore_cursor | Restore saved cursor state | — | Cursor state restored (or defaults) |
| esc_index | Cursor down, scroll if at bottom | Valid scroll region | Cursor moved, scroll if needed |
| esc_next_line | CR + Index | — | Col=0, then index behavior |
| esc_reverse_index | Cursor up, scroll down if at top | Valid scroll region | Cursor moved, scroll down if needed |
| esc_horizontal_tab_set | Set tab stop at cursor col | — | Tab stop added |
| esc_reset | Full terminal reset | — | All state reset including ring buffer |

**Processing Flow**:
```
handle_esc(action, data):
1. Match action code
   ├─ 0 (SaveCursor) → save cursor position + attrs + charset + origin mode + wrap_pending
   ├─ 1 (RestoreCursor) → restore all saved state (or defaults)
   ├─ 2 (Index) → cursor down; if at scroll bottom → scroll_up_internal
   ├─ 3 (NextLine) → col=0, then Index logic
   ├─ 4 (ReverseIndex) → cursor up; if at scroll top → scroll_down_internal
   ├─ 5 (HTS) → set tab stop at cursor.col
   ├─ 6 (RIS) → full reset (ring buffer, cursor, modes, tabs, charsets)
   ├─ 7 (SetG0) → g0_charset = data
   ├─ 8 (SetG1) → g1_charset = data
   └─ _ → no-op
2. Return 0
```

**Implementation Steps**:

1. **Create esc_handler.rs with dispatch**
   - `handle_esc(action: u8, data: u8) -> u8` matches action code to internal method
   - Key consideration: Reuse existing `save_cursor()`/`restore_cursor()` from terminal_core

2. **Implement SaveCursor/RestoreCursor enhancements**
   - Extend `CursorState` struct with additional fields:
     ```rust
     pub(crate) struct CursorState {
         pub col: u16,
         pub row: u16,
         pub fg: PackedColor,
         pub bg: PackedColor,
         pub flags: u16,
         // NEW fields for Sprint 5 (SaveCursor/RestoreCursor):
         pub g0_charset: u8,
         pub g1_charset: u8,
         pub origin_mode: bool,
         pub wrap_pending: bool,
     }
     ```
   - SaveCursor: save all fields including new charset/origin_mode/wrap_pending
   - RestoreCursor: restore all saved state; if no saved state, reset cursor to (0, 0) with default attributes, ASCII charsets, origin_mode=false, wrap_pending=false
   - Key consideration: `CursorState::default()` must set sensible defaults for all new fields

3. **Implement Index/NextLine/ReverseIndex**
   - Index: if cursor at scroll_region_bottom, call scroll_up_internal(1)
   - NextLine: set col=0, then call index logic
   - ReverseIndex: if cursor at scroll_region_top, call scroll_down_internal(1)
   - Key consideration: These use Phase 2's scroll_up/down_internal

4. **Implement HTS and RIS**
   - HTS: `tab_stops[cursor.col] = true`
   - RIS: reinitialize all state, reset ring buffer to viewport-only
   - Key consideration: RIS must reset ring_size back to rows

**Dependencies**:
- Requires: Phase 2 (scroll_up/down_internal for IND/NEL/RI)
- Blocks: Phase 6 (TS dispatch)

**Testing Approach**:

*Unit Tests*:
- handle_esc SaveCursor: saves position + attributes
- handle_esc RestoreCursor: restores saved state
- handle_esc RestoreCursor with no saved state: resets to defaults
- handle_esc Index mid-screen: cursor moves down
- handle_esc Index at scroll bottom (full screen): scrolls up, scrollback grows
- handle_esc Index at scroll bottom (region): scrolls within region
- handle_esc NextLine: col=0 + index behavior
- handle_esc ReverseIndex mid-screen: cursor moves up
- handle_esc ReverseIndex at scroll top: scrolls down
- handle_esc HTS: tab stop set at cursor column
- handle_esc RIS: all state reset (cursor, modes, tabs, ring buffer)
- handle_esc SetG0/SetG1: charset updated

**Acceptance Criteria**:
- [ ] All 9 ESC handlers work correctly in WASM
- [ ] Index/NEL scroll is WASM-internal (uses scroll_up_internal)
- [ ] ReverseIndex scroll is WASM-internal (uses scroll_down_internal)
- [ ] RIS resets ring buffer
- [ ] All Rust tests pass

**Estimated Effort**: 中 (3-5 days)

---

### Phase 5: Reflow

**Goal**: Implement full reflow algorithm in Rust for WASM-internal resize.

**Files to Modify**:
- `wasm/src/ring_buffer.rs`:
  - Add `resize_reflow()`, `resize_no_reflow()`
  - Add internal reflow helpers (drain, join, split, write back)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| resize_reflow | Full reflow on width change | Valid new_cols, new_rows | Ring buffer resized, cursor adjusted |
| resize_no_reflow | Simple resize (alternate buffer) | Valid new_cols, new_rows | Ring buffer resized, no reflow |
| reflow_drain | Extract all lines from ring | Ring buffer populated | All lines collected in order |
| reflow_join_wrapped | Join consecutive wrapped lines | Drained physical lines | Logical lines (concatenated cells) |
| reflow_split_at_width | Re-split logical lines at new width | Logical lines with cells | Physical lines at new width |
| reflow_write_back | Write reflowed lines to resized ring | New ring buffer allocated | Lines written in order |

**Processing Flow**:
```
resize_reflow(new_cols, new_rows, cursor_col, cursor_row):
1. Same width check
   ├─ Same cols → adjust row count only (add/trim blank lines)
   └─ Different cols → full reflow
2. Full reflow:
   a. Drain all lines from ring buffer (cells + wrapped flags)
   b. Join consecutive wrapped lines into logical lines
   c. Trim trailing blank cells from each logical line
   d. Re-split logical lines at new column width
   e. Trim empty lines from bottom if total exceeds new rows
   f. Ensure at least new_rows lines exist
   g. Reallocate ring buffer with new capacity
   h. Write reflowed lines to new ring buffer
   i. Track cursor position through reflow
3. Reset scroll region
4. Return packed cursor (col << 16 | row)
```

**Implementation Steps**:

1. **Implement same-width resize**
   - If cols unchanged: grow (add blank lines) or shrink (trim empty bottom lines)
   - Adjust ring_capacity to new scrollback_lines + new_rows
   - Key consideration: Must preserve scrollback content

2. **Implement reflow_drain**
   - Iterate ring buffer from ring_head through ring_size lines
   - Collect cells and wrapped flags in order
   - Key consideration: Handle wrap-around at ring_capacity boundary

3. **Implement reflow_join_wrapped and reflow_split_at_width**
   - Join: concatenate cells of consecutive wrapped lines into logical lines
   - Split: divide logical lines at new_cols boundary, set wrapped flag on continuations
   - **Wide character handling**: CJK characters and emoji occupy 2 columns. During `reflow_split_at_width`:
     - If a wide character would start at the last column of a line (column `new_cols - 1`), pad that position with a space and place the wide char at the start of the next line
     - Preserve the wide char's placeholder cell (width=0 cell at col+1)
   - Key consideration: Track cursor position through join and split operations

4. **Implement cursor tracking through reflow**
   - Map (cursor_row, cursor_col) through drain → join → split pipeline
   - Return adjusted position as packed u32
   - Key consideration: Cursor on split line must track to correct physical line

5. **Implement resize_no_reflow for alternate buffer**
   - Resize ring buffer without reflow logic
   - Clamp cursor to new dimensions
   - Clear or add lines as needed

6. **Remove old `resize(cols, rows)` method**
   - Delete the current `resize()` method from `terminal_core.rs` (line ~503)
   - All callers must migrate to `resize_reflow()` or `resize_no_reflow()`
   - Update any Rust tests that call `resize()` to use the new methods
   - Key consideration: This is a breaking API change; coordinate with Phase 6 TS integration

**Dependencies**:
- Requires: Phase 1 (Ring Buffer structure)
- Blocks: Phase 6 (TS resize integration)

**Testing Approach**:

*Unit Tests*:
- resize_reflow same width: row count change only
- resize_reflow wider: wrapped lines merge
- resize_reflow narrower: long lines split
- resize_reflow cursor tracking: position correctly adjusted
- resize_reflow empty lines: trailing empties trimmed
- resize_reflow scrollback: scrollback lines included
- resize_reflow capacity overflow: oldest scrollback evicted
- resize_no_reflow: simple resize without reflow
- resize_reflow scroll region: region invalidated

*Edge Cases*:
- Reflow with very long logical line (>1000 cols → 80 cols)
- Reflow with cursor on a split line
- Reflow with cursor past end of trimmed content
- Multiple rapid resizes

**Acceptance Criteria**:
- [ ] resize_reflow produces correct results for width changes
- [ ] Cursor position correctly tracked through reflow
- [ ] Same-width resize optimized (no reflow)
- [ ] resize_no_reflow works for alternate buffer
- [ ] Scroll region invalidated after resize
- [ ] All Rust tests pass

**Estimated Effort**: 大 (1-2 weeks)

**Risks and Mitigation**:
- **Risk**: Reflow edge cases (very long lines, cursor tracking)
  - **Mitigation**: Port existing TS reflow test cases, add extensive edge case tests
- **Risk**: Temporary memory usage during reflow (2x ring buffer)
  - **Mitigation**: Acceptable for transient resize operation

---

### Phase 6: TypeScript Integration

**Goal**: Wire WASM ESC handlers into TS dispatch, thin-wrap UnifiedBuffer, update WasmGrid constructor, and remove `syncCursorAttrsToWasm()`.

**Files to Modify**:
- `src/terminal/state.ts`:
  - Add `handleEscWasm()` dispatcher
  - Update processAction ESC case to try WASM first
  - Remove `syncCursorAttrsToWasm()` method
- `src/terminal/unified-buffer.ts`:
  - WASM mode: scrollUp/Down delegate to WASM
  - WASM mode: resize delegates to WASM reflow
  - WASM mode: scrollback access via WASM APIs
- `src/terminal/wasm/terminal-core.ts`:
  - Update WasmGrid constructor to pass scrollback_lines
  - Add scrollback API wrappers
- `src/terminal/handlers/esc_handlers.ts`:
  - Remove `syncCursorAttrsToWasm()` call from RestoreCursor
- `src/terminal/handlers/types.ts`:
  - Remove `syncCursorAttrsToWasm` from TerminalStateAccessor interface

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleEscWasm | Dispatch ESC to WASM, sync cursor back | WASM grid available | ESC action processed, TS cursor synced |
| UnifiedBuffer scrollUp (WASM) | Delegate scroll to WASM | WASM grid active | Scroll handled internally |
| UnifiedBuffer resize (WASM) | Delegate resize to WASM reflow | WASM grid active | Buffer resized, cursor returned |

**Processing Flow**:
```
ESC dispatch in processAction:
1. Check for active WASM grid
   ├─ WASM available → handleEscWasm(grid, action)
   │   → Call grid.core.handle_esc(actionCode, data)
   │   → Sync cursor position from WASM to TS
   │   → Handle special cases (RIS resets TS state too)
   └─ WASM unavailable → handleEsc(this, action) [existing TS path]
```

**Implementation Steps**:

1. **Add handleEscWasm to state.ts**
   - Map ESC action names to action codes (0-8)
   - Call WASM handle_esc, sync cursor position back to TS
   - Handle RIS: also reset TS-side state
   - Key consideration: Follow established pattern from handleCsiWasm

2. **Update UnifiedBuffer for WASM mode**
   - scrollUp: delegate to WASM (no-op, scroll is internal)
   - resize: call resize_reflow, unpack cursor from return value
   - scrollback access: delegate to WASM scrollback APIs
   - Key consideration: Keep JS fallback paths unchanged

3. **Update WasmGrid constructor**
   - Pass `scrollback_lines` from settings to WASM TerminalCore constructor
   - Key consideration: Setting value available from existing config

4. **Remove syncCursorAttrsToWasm**
   - Remove method from TerminalState class
   - Remove from TerminalStateAccessor interface
   - Remove call sites in esc_handlers.ts and state.ts
   - Key consideration: JS fallback call sites (csi_char_attrs, csi_modes) are no-ops when WASM inactive

**Dependencies**:
- Requires: Phase 1-5 (all WASM-side work)
- Blocks: Phase 7 (verification)

**Testing Approach**:

*Integration Tests*:
- ESC SaveCursor + RestoreCursor via WASM: position and attrs preserved
- ESC Index at bottom via WASM: scrolls up, scrollback grows
- Scrollback readable via WASM APIs
- Resize via WASM reflow: cursor correctly positioned
- syncCursorAttrsToWasm removed: no calls anywhere

*Regression Tests*:
- All existing Sprint 1-4 TS tests pass
- All existing ESC handler tests pass
- All existing UnifiedBuffer tests pass
- All existing cursor tests pass

**Acceptance Criteria**:
- [ ] handleEscWasm dispatches all 9 ESC actions to WASM
- [ ] UnifiedBuffer WASM mode delegates scroll/resize/scrollback
- [ ] WasmGrid constructor passes scrollback_lines
- [ ] syncCursorAttrsToWasm completely removed
- [ ] All TS tests pass (1824+)
- [ ] JS fallback path unchanged

**Estimated Effort**: 中 (3-5 days)

---

### Phase 7: Verification and Regression Testing

**Goal**: Full regression test, cross-validation, binary size check, and manual testing.

**Files to Modify**: None (verification only)

**Implementation Steps**:

1. **Run all automated tests**
   - Rust: `cargo test --manifest-path src-tauri/Cargo.toml`
   - TypeScript: `bun test` and `bun run typecheck`
   - Key consideration: Run in Docker environment

2. **Binary size verification**
   - Build WASM: `wasm-pack build --target web --out-dir pkg`
   - Verify `.wasm` file < 70KB
   - Key consideration: Sprint 4 baseline is 51.4KB

3. **Manual testing**
   - `bun tauri dev` smoke test
   - vttest basic tests
   - vim/less/top application switching
   - Scrollback view (scroll up to see history)
   - Resize with scrollback content

**Dependencies**:
- Requires: Phase 1-6 (all implementation complete)

**Acceptance Criteria**:
- [ ] All Rust unit tests pass
- [ ] All TS tests pass (1824+)
- [ ] WASM binary < 70KB
- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests unchanged
- [ ] vim/less/top work correctly
- [ ] Scrollback view works
- [ ] Resize preserves scrollback content
- [ ] `bun run typecheck` passes

**Estimated Effort**: 小 (1-2 days)

---

## Complete File Structure

```
wasm/src/
├── lib.rs              # MODIFIED: add ring_buffer, esc_handler modules
├── terminal_core.rs    # MODIFIED: ring buffer fields, updated constructor, index mapping
├── ring_buffer.rs      # NEW: ring buffer ops, scroll internal, reflow
├── esc_handler.rs      # NEW: ESC dispatch and handler implementations
├── print_handler.rs    # MODIFIED: use scroll_up_internal, return 0
├── c0_handler.rs       # MODIFIED: use scroll_up_internal, return 0
├── csi_scroll.rs       # MODIFIED: use scroll_up/down_internal, return 0
├── csi_cursor.rs       # MODIFIED: viewport_abs for row calculations
├── csi_screen.rs       # MODIFIED: viewport_abs for row calculations
├── csi_edit.rs         # MODIFIED: viewport_abs for row calculations
├── csi_modes.rs        # UNCHANGED
├── csi_device.rs       # UNCHANGED
├── cell.rs             # UNCHANGED
├── unicode.rs          # UNCHANGED
├── sgr.rs              # UNCHANGED

src/terminal/
├── state.ts            # MODIFIED: handleEscWasm, remove syncCursorAttrsToWasm
├── unified-buffer.ts   # MODIFIED: thin WASM wrapper
├── wasm/terminal-core.ts  # MODIFIED: constructor + scrollback APIs
├── handlers/
│   ├── esc_handlers.ts    # MODIFIED: remove syncCursorAttrsToWasm call
│   ├── types.ts           # MODIFIED: remove syncCursorAttrsToWasm from interface
│   ├── csi_char_attrs.ts  # MODIFIED: remove syncCursorAttrsToWasm call
│   └── csi_modes.ts       # MODIFIED: remove syncCursorAttrsToWasm call
```

## Testing Strategy

### Unit Testing

**Approach**:
- Rust tests via `cargo test`
- TypeScript tests via `bun test`
- Docker-first execution

**Test Coverage Goals**:
- Ring Buffer operations: 90%+
- ESC handlers: 90%+
- Reflow: 90%+
- Integration paths: 80%+

**Key Test Areas**:
1. **Ring Buffer** — Index mapping, wrap-around, eviction
2. **Scroll Internal** — Full screen vs region, scrollback growth
3. **ESC Handlers** — All 9 handlers, edge cases
4. **Reflow** — Width changes, cursor tracking, edge cases

### Integration Testing

**Scenarios**:
1. ESC handler dispatch through WASM (TS → WASM → result)
2. Scrollback growing during terminal output
3. Resize with scrollback content
4. JS fallback path still works

### Manual Testing

- [ ] `bun tauri dev` shows working terminal with typing
- [ ] vim/less/top switch correctly (alternate buffer)
- [ ] Scrollback view (scroll up to see history)
- [ ] Resize preserves scrollback content
- [ ] vttest basic tests unchanged

## Dependencies

### External Dependencies

None (no new dependencies).

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Ring Buffer Foundation (no dependencies)
2. Phase 2: Scroll Internal (depends on Phase 1)
3. Phase 3: Scrollback Access APIs (depends on Phase 1, 2)
4. Phase 4: ESC Handlers (depends on Phase 2)
5. Phase 5: Reflow (depends on Phase 1)
6. Phase 6: TypeScript Integration (depends on Phase 1-5)
7. Phase 7: Verification (depends on Phase 1-6)

**Parallelizable**: Phase 3, 4, 5 can be developed in parallel after Phase 2.

## Risk Assessment

### Technical Risks

1. **Ring Buffer index mapping correctness**
   - **Risk**: Off-by-one errors in modulo arithmetic
   - **Likelihood**: Medium
   - **Impact**: High (data corruption)
   - **Mitigation**: Extensive unit tests for boundary conditions and wrap-around

2. **Reflow edge cases**
   - **Risk**: Cursor tracking through join/split loses position
   - **Likelihood**: Medium
   - **Impact**: Medium (cursor misplacement)
   - **Mitigation**: Port existing TS reflow tests, add cursor tracking tests

3. **Existing test compatibility**
   - **Risk**: Scroll return value changes break TS tests
   - **Likelihood**: Low
   - **Impact**: Medium (test failures)
   - **Mitigation**: TS scroll bridge code becomes dead code (no-op), shouldn't affect tests

4. **WASM binary size**
   - **Risk**: Reflow code pushes binary over 70KB
   - **Likelihood**: Low
   - **Impact**: Low (soft limit)
   - **Mitigation**: Estimated ~7.5KB additional code, well within budget

## Performance Considerations

1. **Scroll operations**: 0 WASM-TS boundary crossings (vs Sprint 4: 1+ crossings)
2. **Reflow**: Single WASM call (vs Sprint 4: N wasmRowToLine + TS reflow + N write-back)
3. **ESC handlers**: 1 WASM call per ESC sequence
4. **Memory**: Ring buffer in WASM linear memory, no JS GC pressure for scrollback

## Open Questions

### From Specification:
- None (all decisions confirmed during spec creation)

### Implementation-Specific:
- None

## Future Enhancements

### From Specification (Deferred):
- Sprint 6: OSC/APC/DCS handler WASM migration
- Sprint 7: Full integration and optimization

### Not in Current Spec:
- Ring buffer memory optimization (e.g., compressed storage for sparse lines)
- Scrollback search in WASM

## Success Metrics

### Functional Completeness
- [ ] All 9 ESC handlers work in WASM
- [ ] Ring Buffer stores viewport + scrollback
- [ ] Scroll operations WASM-internal
- [ ] Reflow works in WASM
- [ ] syncCursorAttrsToWasm removed

### Quality Metrics
- [ ] All Rust tests pass
- [ ] All TS tests pass (1824+)
- [ ] WASM binary < 70KB
- [ ] `bun run typecheck` passes

### Performance Metrics
- [ ] 0 WASM-TS boundary crossings for scroll
- [ ] Single WASM call for resize/reflow

## References

- **Specification**: `doc/tasks/wasm-esc-ring-buffer/SPEC.md`
- **Requirements**: `doc/tasks/wasm-esc-ring-buffer/要件定義書.md`
- **WASM Roadmap**: `tmp/wasm.md`
- **Sprint 4 SPEC**: `doc/tasks/wasm-sgr-edit-scroll/SPEC.md`
