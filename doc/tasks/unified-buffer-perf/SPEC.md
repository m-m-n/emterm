# Feature: UnifiedBuffer Performance Improvements

## Overview

Optimize the UnifiedBuffer implementation by eliminating unnecessary memory allocations and redundant processing. Four targeted improvements address: full-scrollback cloning in the renderer hot path, incorrect capacity recalculation on row-only resize, unnecessary cell cloning during reflow, and string allocation for empty-line detection.

## Objectives

- Eliminate per-frame O(n) memory allocation when rendering scrollback
- Fix capacity tracking in `adjustRowCount()` for correct scrollback limits
- Replace `cloneCell()` with direct assignment during reflow (drain invalidates source)
- Add `Line.isEmpty()` to avoid string allocation for empty-line checks

## Technical Requirements

### Functional Requirements

- **FR1:** Renderer accesses scrollback lines via `getScrollbackLine(index)` instead of `getScrollbackBuffer()` array
- **FR2:** `getScrollbackBuffer()` is either removed or retained as a non-critical convenience method (not called from renderer)
- **FR3:** `adjustRowCount()` recalculates `capacity` as `(capacity - oldRows) + newRows` when row count changes
- **FR4:** `resize()` reflow assigns cells by reference (`trimmedCells[offset + j]`) instead of `cloneCell(trimmedCells[offset + j])`
- **FR5:** `Line` class provides an `isEmpty()` method that returns `true` if all cells with `width > 0` have `char === " "` (matching the semantics of `getText().trim() === ""` which skips `width === 0` cells)
- **FR6:** All occurrences of `getText().trim() === ""` in `unified-buffer.ts` are replaced with `isEmpty()`

### Non-Functional Requirements

- **NFR1 - Performance:** Scrollback rendering allocates O(rows) memory per frame, not O(scrollbackLength)
- **NFR2 - Compatibility:** All existing 1741 tests pass without modification
- **NFR3 - Compatibility:** Rendering output is visually identical (no behavioral changes)

## Implementation Approach

### FR1/FR2: Scrollback Access Optimization

**Problem:**
`canvas-renderer.ts` calls `state.getScrollbackBuffer()` which clones every scrollback line:
```typescript
// state.ts (current)
getScrollbackBuffer(): Line[] {
  const result: Line[] = [];
  const len = this.primaryBuffer.scrollbackLength;
  for (let i = 0; i < len; i++) {
    result.push(this.primaryBuffer.getScrollbackLine(i).clone());
  }
  return result;
}
```

With 10,000 scrollback lines, this creates 10,000 Line clones per render frame.

**Solution:**
Refactor the two call sites in `canvas-renderer.ts` to use index-based access:

```typescript
// canvas-renderer.ts: getVisibleLines (scrolled-back case)
// Before:
const scrollbackBuffer = state.getScrollbackBuffer();
const combinedBuffer = [...scrollbackBuffer];
for (let screenRow = 0; screenRow < visibleRows; screenRow++) {
  combinedBuffer.push(buffer.getLine(screenRow));
}

// After:
const scrollbackLength = state.getScrollbackLength();
const totalLines = scrollbackLength + visibleRows;
const startIndex = totalLines - visibleRows - scrollOffset;
const linesToRender: Line[] = [];
for (let i = 0; i < visibleRows; i++) {
  const lineIndex = startIndex + i;
  if (lineIndex < scrollbackLength) {
    linesToRender.push(state.getScrollbackLine(lineIndex));
  } else {
    linesToRender.push(buffer.getLine(lineIndex - scrollbackLength));
  }
}
```

**New public method on TerminalState:**
```typescript
// state.ts
getScrollbackLine(index: number): Line {
  return this.primaryBuffer.getScrollbackLine(index);
}
```

**getVisibleLinesWithFolding:** Apply same pattern — use index-based access instead of materializing the full scrollback array.

**getScrollbackBuffer() disposition:** Retain the method but mark it as non-performance-critical. If no callers remain after refactoring, remove it.

### FR3: adjustRowCount() Capacity Fix

**Problem:**
```typescript
// Current code has dead/incorrect capacity logic:
private adjustRowCount(rows, cursorRow, cursorCol) {
  // ... row adjustment ...
  this._rows = rows;

  // BUG: dead code, capacity is never updated
  const scrollbackCap = this.capacity - (this._rows + (this._rows - rows));
  // Actually just keep the same capacity, only _rows changed
  // ...
}
```

When rows decrease, scrollback capacity implicitly grows (because `capacity` stays the same but `scrollbackLength = size - rows` increases). When rows increase, lines may be pushed and evicted unnecessarily because capacity is too small.

**Solution:**
```typescript
private adjustRowCount(rows, cursorRow, cursorCol) {
  const oldRows = this._rows;
  // ... row adjustment logic (add/trim) ...

  this._rows = rows;

  // Recalculate capacity: preserve original scrollback limit
  const scrollbackCapacity = this.capacity - oldRows;
  this.capacity = scrollbackCapacity + rows;

  // Resize ring array if capacity changed
  if (this.capacity !== this.ring.length) {
    const newRing = new Array(this.capacity).fill(null);
    // If _size exceeds new capacity, evict oldest lines
    const startIndex = Math.max(0, this._size - this.capacity);
    const evicted = startIndex;
    let newSize = 0;
    for (let i = startIndex; i < this._size; i++) {
      newRing[newSize] = this.getAbsolute(i);
      newSize++;
    }
    this.ring = newRing;
    this.head = 0;
    this._size = newSize;
    if (evicted > 0 && this.onEvict) {
      this.onEvict(evicted);
    }
  }

  // ... rest of method ...
}
```

### FR4: Reflow Cell Reference Move

**Problem:**
```typescript
// resize() line 733
for (let j = 0; j < lineLength; j++) {
  newLine.setCell(j, cloneCell(trimmedCells[offset + j]!));
}
```

After `drain()`, the source lines are no longer referenced by the ring buffer. The cells from `trimmedCells` can be assigned directly without cloning.

**Solution:**
```typescript
for (let j = 0; j < lineLength; j++) {
  newLine.setCell(j, trimmedCells[offset + j]!);
}
```

Remove the `cloneCell` import if no longer used elsewhere in the file.

### FR5/FR6: Line.isEmpty() Method

**Problem:**
```typescript
// unified-buffer.ts lines 750, 891
const isEmpty = lastLine.getText().trim() === "";
```

`getText()` concatenates all cell characters into a string, then `trim()` creates another string.

**Solution:**

Add to `grid.ts`:
```typescript
class Line {
  /**
   * Check if this line contains only empty (space) cells.
   * More efficient than getText().trim() === "" as it avoids string allocation.
   * Matches getText() semantics: width-0 cells (CJK placeholders) are skipped.
   */
  isEmpty(): boolean {
    for (const cell of this.cells) {
      if (cell.width > 0 && cell.char !== " ") {
        return false;
      }
    }
    return true;
  }
}
```

Replace in `unified-buffer.ts`:
```typescript
// Before:
const isEmpty = lastLine.getText().trim() === "";
// After:
if (lastLine.isEmpty()) {

// Before:
if (line.getText().trim() === "") {
// After:
if (line.isEmpty()) {
```

### Dependencies

**Internal Dependencies:**
- `grid.ts`: Add `isEmpty()` to `Line` class (FR5)
- `state.ts`: Add `getScrollbackLine()` public method (FR1)
- `canvas-renderer.ts`: Refactor scrollback access (FR1)
- `unified-buffer.ts`: Fix `adjustRowCount()`, remove `cloneCell()` in reflow, use `isEmpty()` (FR3, FR4, FR6)

**External Dependencies:**
- None

### File Structure

```
src/terminal/
├── grid.ts                # MODIFIED: Add Line.isEmpty()
├── unified-buffer.ts      # MODIFIED: FR3 (capacity), FR4 (cloneCell), FR6 (isEmpty)
├── state.ts               # MODIFIED: Add getScrollbackLine(), maybe deprecate getScrollbackBuffer()
├── canvas-renderer.ts     # MODIFIED: Index-based scrollback access
```

Note: `handlers/types.ts` (`TerminalStateAccessor`) does not need modification. `getScrollbackLine()` is only used by the renderer, which accesses `TerminalState` directly, not the handler interface.

## Test Scenarios

### Unit Tests

- [ ] `Line.isEmpty()` returns `true` for a line with only spaces
- [ ] `Line.isEmpty()` returns `false` for a line with non-space content
- [ ] `Line.isEmpty()` returns `true` for a line with width-0 placeholder cells (skipped like getText())
- [ ] `Line.isEmpty()` returns `false` for a line with non-space content (CJK character)
- [ ] `adjustRowCount()` preserves correct capacity after row decrease
- [ ] `adjustRowCount()` preserves correct capacity after row increase
- [ ] `adjustRowCount()` does not evict scrollback when rows decrease by small amount
- [ ] `adjustRowCount()` maintains `size <= capacity` invariant after row changes
- [ ] `adjustRowCount()` calls `onEvict()` when capacity shrinks below current size
- [ ] `resize()` reflow produces identical results without `cloneCell()`
- [ ] `getScrollbackLine()` on TerminalState returns correct line

### Integration Tests

- [ ] Renderer displays correct content when scrolled back (index-based access)
- [ ] Renderer displays correct content with fold regions when scrolled back
- [ ] Resize followed by scroll-back shows correctly reflowed content

### Performance Tests

- [ ] Reflow 10,000 lines still completes within budget (<500ms in Docker)
- [ ] `isEmpty()` is faster than `getText().trim() === ""` for typical line lengths

## Implementation Phases

### Phase 1: Line.isEmpty() and getText().trim() Replacement

**Goals:** Add `isEmpty()` to `Line`, replace usage in `unified-buffer.ts`.

**Deliverables:**
- `grid.ts`: `Line.isEmpty()` method
- `unified-buffer.ts`: Replace 2 occurrences of `getText().trim() === ""`
- Unit tests for `isEmpty()`

### Phase 2: Reflow Cell Reference Move

**Goals:** Replace `cloneCell()` with direct assignment in reflow.

**Deliverables:**
- `unified-buffer.ts`: Remove `cloneCell()` in `resize()` line 733
- Verify all existing reflow tests still pass

### Phase 3: adjustRowCount() Capacity Fix

**Goals:** Fix capacity recalculation when only row count changes.

**Deliverables:**
- `unified-buffer.ts`: Correct capacity logic in `adjustRowCount()`
- New unit tests for capacity preservation
- Remove dead code comments

### Phase 4: Scrollback Access Optimization

**Goals:** Refactor renderer to use index-based scrollback access.

**Deliverables:**
- `state.ts`: Add `getScrollbackLine()` public method
- `canvas-renderer.ts`: Refactor `getVisibleLines()` and `getVisibleLinesWithFolding()`
- Evaluate `getScrollbackBuffer()` removal or deprecation

## Success Criteria

- [ ] All existing 1741+ tests pass
- [ ] New unit tests for `isEmpty()`, capacity fix, and scrollback access pass
- [ ] Type check passes (`bun run typecheck`)
- [ ] No `cloneCell()` calls in reflow path
- [ ] No `getText().trim()` calls in `unified-buffer.ts`
- [ ] No `getScrollbackBuffer()` calls in `canvas-renderer.ts`
- [ ] `adjustRowCount()` updates capacity correctly

## Open Questions

- [ ] Should `getScrollbackBuffer()` be removed entirely or kept for potential external consumers?
