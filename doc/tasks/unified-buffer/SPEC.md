# Feature: Unified Buffer

## Overview

Replace the current split buffer architecture (`scrollbackBuffer: Line[]` + `ScreenBuffer`) with a single `UnifiedBuffer` class backed by a ring buffer. This eliminates the boundary between scrollback and screen lines, enables full-buffer reflow on resize, and implements cursor position tracking through the reflow process.

## Objectives

- Eliminate display corruption caused by content loss during terminal resize
- Track cursor position correctly through reflow (logical line offset approach)
- Unify scrollback and screen buffers into a single data structure
- Maintain compatibility with existing 1712 tests and external interfaces

## User Stories

### US1: Resize without content loss

As a terminal user, I want to resize the terminal window without losing visible content, so that I can rearrange my workspace freely.

**Acceptance Criteria:**
- [ ] Narrowing the terminal wraps long lines without losing text
- [ ] Widening the terminal unwraps previously wrapped lines
- [ ] Content near the cursor is preserved during resize
- [ ] Shell prompt redraws correctly after resize

### US2: Seamless scrollback after resize

As a terminal user, I want scrollback history to remain intact and correctly reflowed after resize, so that I can review past output at any terminal width.

**Acceptance Criteria:**
- [ ] Scrollback lines are reflowed along with screen lines
- [ ] Scrolling back after resize shows correctly wrapped history
- [ ] Scrollback line count limit is still enforced

### US3: Alternate screen compatibility

As a terminal user running full-screen applications (vim, less, etc.), I want alternate screen switching to work correctly with the unified buffer.

**Acceptance Criteria:**
- [ ] Switching to alternate screen preserves primary buffer state
- [ ] Alternate screen does NOT reflow on resize (simple truncate/extend)
- [ ] Switching back restores primary buffer with correct cursor position

## Technical Requirements

### Functional Requirements

- **FR1:** `UnifiedBuffer` class replaces `ScreenBuffer` as the sole buffer implementation
- **FR2:** Ring buffer stores up to `capacity` lines (scrollback + rows for primary, rows-only for alternate)
- **FR3:** Viewport is defined as the last `rows` lines of the buffer
- **FR4:** `resize(cols, rows, cursorRow, cursorCol)` performs full-buffer reflow and returns adjusted cursor position
- **FR5:** Reflow joins wrapped lines into logical lines, then re-splits at new width (same algorithm as current, applied to all lines)
- **FR6:** Cursor position is tracked through reflow via logical line offset accumulation
- **FR7:** Alternate buffer does NOT reflow on width change (lines are resized in place)
- **FR8:** Empty lines are trimmed from the bottom before pushing lines out of viewport on shrink
- **FR9:** `TerminalState` no longer maintains a separate `scrollbackBuffer`
- **FR10:** `onLinesRemovedCallback` is removed; scrollback is implicit in the unified buffer
- **FR11:** Ring buffer eviction (capacity overflow) triggers notification to `SemanticZoneTracker` and `FoldManager` for index adjustment (replacing the prune logic in the removed `addToScrollback`)

### Non-Functional Requirements

- **NFR1 - Performance:** Full reflow of 10,000 scrollback + screen lines completes within 100ms
- **NFR2 - Memory:** Ring buffer pre-allocates capacity; no per-operation allocation overhead for push/pop
- **NFR3 - Compatibility:** All 1712 existing tests pass without modification (external API compatibility)

## Implementation Approach

### Architecture

**Current Architecture (Split Buffer):**
```
TerminalState
├── scrollbackBuffer: Line[]        ← separate scrollback storage
├── primaryBuffer: ScreenBuffer     ← screen lines only
│   └── lines: Line[]               ← fixed to `rows` entries
│   └── onLinesRemovedCallback      ← sends lines to scrollbackBuffer
└── alternateBuffer: ScreenBuffer
```

**New Architecture (Unified Buffer):**
```
TerminalState
├── primaryBuffer: UnifiedBuffer    ← scrollback + screen in one ring buffer
│   └── ring: Line[]                ← capacity = scrollbackLines + rows
│   └── head: number                ← ring buffer head pointer
│   └── size: number                ← current number of lines
│   └── viewportRows: number        ← number of visible rows
└── alternateBuffer: UnifiedBuffer  ← capacity = rows, no scrollback
```

### Data Flow

**Resize flow:**
```
ResizeObserver detects size change
  → TerminalState.resize(cols, rows)
    → primaryBuffer.resize(cols, rows, cursorRow, cursorCol)
      → Phase 1: Drain all lines from ring buffer
      → Phase 2: Join wrapped lines into logical lines
                  (track cursor position as logical offset)
      → Phase 3: Re-split logical lines at new width
                  (convert cursor offset back to physical x,y)
      → Phase 4: Trim empty lines from bottom
      → Phase 5: Write reflowed lines back to ring buffer
      → Phase 6: Return adjusted (cursorCol, cursorRow)
    → Update cursor position from returned values
    → ptyClient.resize(cols, rows)
```

**Scroll-up flow (line scrolls off top):**
```
scrollUp() called (e.g., from LF at bottom of scroll region)
  → Remove top line of viewport (stays in ring buffer as scrollback)
  → Insert blank line at bottom of viewport
  → No callback needed; line is already in the unified buffer
```

**Viewport access:**
```
getLine(row)           → ring[(head + scrollbackLength + row) % capacity]
getScrollbackLine(idx) → ring[(head + idx) % capacity]
scrollbackLength       → size - viewportRows  (clamped to >= 0)
```

### UnifiedBuffer Class Design

```typescript
class UnifiedBuffer {
  // Ring buffer storage
  private ring: (Line | null)[];
  private head: number;       // index of oldest line
  private _size: number;      // current number of lines in buffer
  private capacity: number;   // max lines (scrollback + rows)

  // Dimensions
  private _cols: number;
  private _rows: number;      // viewport rows
  private allowScrollback: boolean;

  // Scroll region
  private scrollRegion: ScrollRegion | null;

  // Public accessors
  get cols(): number;
  get rows(): number;
  get size(): number;
  get scrollbackLength(): number;

  // Line access (viewport-relative)
  getLine(row: number): Line;
  setCell(col: number, row: number, cell: Cell): void;
  getCell(col: number, row: number): Cell;

  // Scrollback access
  getScrollbackLine(index: number): Line;

  // Scroll operations (within scroll region)
  scrollUp(count?: number): void;
  scrollDown(count?: number): void;
  getEffectiveScrollRegion(): { top: number; bottom: number };

  // Line manipulation
  insertLines(row: number, count: number): void;
  deleteLines(row: number, count: number): void;

  // Character editing
  insertCharacters(row: number, col: number, count: number): void;
  deleteCharacters(row: number, col: number, count: number): void;
  eraseCharacters(row: number, col: number, count: number): void;

  // Resize with reflow
  resize(cols: number, rows: number,
         cursorRow: number, cursorCol: number
  ): { col: number; row: number };

  // Buffer operations
  clearAll(): void;
  clearScrollback(): void;
  clone(): UnifiedBuffer;
}
```

### Cursor Tracking During Reflow

The cursor position is tracked through the reflow process using the WezTerm-inspired approach.

**`wrapped` flag convention**: In this codebase, `line.wrapped = true` means "this line is a continuation of the previous line" (set on offset > 0 during line splitting). The reflow algorithm groups lines by checking the **next** line's `wrapped` flag, matching the existing `buffer.ts` pattern.

```typescript
// Logical line joining with cursor tracking:
// Uses the same grouping pattern as existing reflowLarger/reflowSmaller
let outputLineCount = 0;
let i = 0;
while (i < lines.length) {
  // Collect all cells from a logical line (consecutive wrapped lines)
  const logicalCells: Cell[] = [];
  let cursorInThisLogical = false;
  let logicalCursorX = 0;

  // First physical line of this logical group
  if (i === cursorPhysRow) {
    cursorInThisLogical = true;
    logicalCursorX = cursorCol;
  }
  logicalCells.push(...lines[i].getCells());
  i++;

  // Merge continuation lines (wrapped = true means "I am a continuation")
  while (i < lines.length && lines[i].wrapped) {
    if (i === cursorPhysRow) {
      cursorInThisLogical = true;
      logicalCursorX = cursorCol + logicalCells.length;
    }
    logicalCells.push(...lines[i].getCells());
    i++;
  }

  // Logical line complete — re-split at new width
  if (cursorInThisLogical) {
    const newRow = Math.floor(logicalCursorX / newCols);
    const newCol = logicalCursorX % newCols;
    adjustedCursor = { col: newCol, row: outputLineCount + newRow };
  }

  // ... trim trailing empty cells, re-split, emit lines ...
  // outputLineCount += number of physical lines emitted
}
```

### Ring Buffer Implementation

```typescript
// Core operations:

// Push a line (append to end)
push(line: Line): void {
  if (this._size < this.capacity) {
    // Buffer not full: append
    const index = (this.head + this._size) % this.capacity;
    this.ring[index] = line;
    this._size++;
  } else {
    // Buffer full: overwrite oldest (head advances)
    this.ring[this.head] = line;
    this.head = (this.head + 1) % this.capacity;
  }
}

// Access by absolute index (0 = oldest line)
get(index: number): Line {
  return this.ring[(this.head + index) % this.capacity]!;
}

// Access viewport line (0 = top of screen)
getViewportLine(row: number): Line {
  const scrollbackLen = Math.max(0, this._size - this._rows);
  return this.get(scrollbackLen + row);
}

// Drain all lines for reflow
drain(): Line[] {
  const lines: Line[] = [];
  for (let i = 0; i < this._size; i++) {
    lines.push(this.get(i));
  }
  this.head = 0;
  this._size = 0;
  return lines;
}
```

### Changes to TerminalState

```typescript
class TerminalState {
  // REMOVED: private scrollbackBuffer: Line[] = [];
  // REMOVED: private addToScrollback(lines: Line[]): void

  private primaryBuffer: UnifiedBuffer;  // was: ScreenBuffer
  private alternateBuffer: UnifiedBuffer | null;  // was: ScreenBuffer | null

  // Changed: resize now uses cursor-tracked reflow
  resize(cols: number, rows: number): void {
    if (!this.useAlternate) {
      const adjusted = this.primaryBuffer.resize(
        cols, rows,
        this.primaryCursor.row, this.primaryCursor.col
      );
      this.primaryCursor.row = adjusted.row;
      this.primaryCursor.col = adjusted.col;
    }

    if (this.alternateBuffer) {
      // Alternate: no reflow, just resize lines
      this.alternateBuffer.resizeNoReflow(cols, rows);
    }

    this.primaryCursor.resize(cols, rows);
    if (this.alternateCursor) {
      this.alternateCursor.resize(cols, rows);
    }
    // ... tab stops, wrapPending reset ...
  }

  // Changed: delegate to unified buffer
  // Note: getScrollbackLine returns direct references from ring buffer.
  // getScrollbackBuffer() is called by external consumers (renderer, search)
  // that may retain references. Clone lines at this API boundary to
  // prevent external mutations from corrupting ring buffer contents.
  getScrollbackBuffer(): Line[] {
    const result: Line[] = [];
    const len = this.primaryBuffer.scrollbackLength;
    for (let i = 0; i < len; i++) {
      result.push(this.primaryBuffer.getScrollbackLine(i).clone());
    }
    return result;
  }

  getScrollbackLength(): number {
    return this.primaryBuffer.scrollbackLength;
  }
}
```

### Changes to Renderer

Minimal changes needed in `canvas-renderer.ts`:

```typescript
// getVisibleLines(): no change needed
// - state.getActiveBuffer().getLine(row) still works
// - state.getScrollbackBuffer() still returns Line[]
// - state.getScrollbackLength() still returns number

// Key difference: ring buffer stores lines without cloning on scroll-off.
// - getLine(row) returns direct reference (safe: renderer reads synchronously)
// - getScrollbackBuffer() clones at API boundary (safe: prevents external mutation)
// - getScrollbackLine() returns direct reference (internal use only)
```

### File Structure

```
src/terminal/
├── unified-buffer.ts          # NEW: UnifiedBuffer class + ring buffer
├── buffer.ts                  # REMOVED (or kept as re-export for gradual migration)
├── grid.ts                    # UNCHANGED: Line, Cell
├── cursor.ts                  # UNCHANGED: CursorState
├── state.ts                   # MODIFIED: use UnifiedBuffer, remove scrollbackBuffer
├── canvas-renderer.ts         # MINIMAL CHANGES: update imports
├── handlers/                  # UNCHANGED: all handlers use TerminalStateAccessor
│   ├── types.ts               # POSSIBLY UPDATED: if getActiveBuffer() return type changes
│   ├── print_handler.ts
│   ├── c0_handlers.ts
│   ├── csi_handlers.ts
│   └── esc_handlers.ts
└── buffer.test.ts             # UPDATED: import UnifiedBuffer instead of ScreenBuffer
```

### Dependencies

**Internal Dependencies:**
- `grid.ts`: `Line`, `Cell`, `cloneCell`, `createEmptyCell` (unchanged)
- `cursor.ts`: `CursorState` (unchanged)
- `attributes.ts`: `CellAttributes`, `cloneAttributes` (unchanged)
- `state.ts`: Primary consumer, needs significant changes

**External Dependencies:**
- None (self-contained TypeScript implementation)

## Test Scenarios

### Unit Tests

- [ ] Ring buffer: push, get, drain, capacity overflow
- [ ] Ring buffer: viewport line access
- [ ] Ring buffer: scrollback line access
- [ ] UnifiedBuffer: getLine/setCell/getCell compatibility with ScreenBuffer API
- [ ] UnifiedBuffer: scrollUp/scrollDown within scroll region
- [ ] UnifiedBuffer: insertLines/deleteLines
- [ ] UnifiedBuffer: insertCharacters/deleteCharacters/eraseCharacters
- [ ] Reflow: narrowing wraps lines (scrollback + screen)
- [ ] Reflow: widening unwraps lines (scrollback + screen)
- [ ] Reflow: cursor tracking through narrow reflow
- [ ] Reflow: cursor tracking through wide reflow
- [ ] Reflow: empty line trimming from bottom
- [ ] Reflow: preserves hard line breaks
- [ ] Reflow: handles wide characters (CJK) correctly
- [ ] Reflow: alternate buffer does NOT reflow
- [ ] Resize: row increase adds lines at bottom
- [ ] Resize: row decrease trims empty lines first

### Integration Tests

- [ ] TerminalState.resize() updates cursor correctly
- [ ] TerminalState scrollback access returns correct lines
- [ ] Alternate screen switch preserves primary buffer
- [ ] getVisibleLines() returns correct viewport with scroll offset

### Edge Cases

- [ ] Resize to 1 column width
- [ ] Resize with empty buffer
- [ ] Resize with buffer at scrollback capacity
- [ ] Reflow with line containing only wide characters
- [ ] Cursor at column 0 after reflow (boundary case)
- [ ] Cursor on a wrapped continuation line during reflow

## Performance Optimization

### Performance Goals

- Full reflow (10,000 lines): < 100ms
- Single line access (getLine): O(1) via ring buffer index arithmetic
- Scroll-up (line exits viewport): O(1), no clone needed

### Optimization Strategies

- Ring buffer avoids array shift/splice for head removal
- Reflow operates on drained array (sequential access pattern)
- No line cloning for scrollback (unified storage eliminates the clone-on-scroll-off)

## Success Criteria

- [ ] All 1712 existing tests pass
- [ ] New unit tests for UnifiedBuffer and ring buffer pass
- [ ] Reflow cursor tracking tests pass
- [ ] Manual test: narrow→wide resize preserves all content
- [ ] Manual test: image viewer show→resize→hide displays correctly
- [ ] Manual test: vim/less alternate screen works correctly
- [ ] Type check passes (`bun run typecheck`)

## Implementation Phases

### Phase 1: Ring Buffer + UnifiedBuffer Core

**Goals:** Implement the ring buffer data structure and basic UnifiedBuffer with viewport access.

**Deliverables:**
- `unified-buffer.ts` with ring buffer and line access methods
- Unit tests for ring buffer operations
- Unit tests for viewport/scrollback line access

### Phase 2: Scroll Operations + Line Manipulation

**Goals:** Implement scroll, insert, delete, erase operations compatible with ScreenBuffer API.

**Deliverables:**
- scrollUp/scrollDown with scroll region support
- insertLines/deleteLines/insertCharacters/deleteCharacters/eraseCharacters
- Existing ScreenBuffer operation tests ported to UnifiedBuffer

### Phase 3: Full-Buffer Reflow with Cursor Tracking

**Goals:** Implement the resize/reflow algorithm with cursor position tracking.

**Deliverables:**
- `resize()` method with full-buffer reflow
- Cursor tracking via logical line offset (WezTerm approach)
- Reflow tests for narrow/wide/cursor scenarios

### Phase 4: TerminalState Integration

**Goals:** Replace ScreenBuffer usage in TerminalState with UnifiedBuffer.

**Deliverables:**
- TerminalState uses UnifiedBuffer for primary and alternate buffers
- Remove separate scrollbackBuffer
- Update resize() to use cursor-tracked reflow result
- All existing tests pass

### Phase 5: Renderer + Final Integration

**Goals:** Update renderer and verify end-to-end behavior.

**Deliverables:**
- Update canvas-renderer.ts for UnifiedBuffer compatibility
- Manual testing of resize scenarios
- Performance validation

## Open Questions

- [ ] Should `getScrollbackBuffer()` return a lazy iterator instead of materializing an array? (Performance consideration for large scrollback)
- [ ] Should logical line length be capped (WezTerm uses 1024 cells) to prevent pathological reflow?
