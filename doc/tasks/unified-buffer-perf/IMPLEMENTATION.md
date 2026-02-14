# Implementation Plan: UnifiedBuffer Performance Improvements

## Overview

Optimize the UnifiedBuffer implementation by eliminating unnecessary memory allocations and redundant processing across four targeted improvements: full-scrollback cloning in the renderer hot path, incorrect capacity recalculation on row-only resize, unnecessary cell cloning during reflow, and string allocation for empty-line detection.

## Objectives

- Eliminate per-frame O(n) memory allocation when rendering scrollback
- Fix capacity tracking in `adjustRowCount()` for correct scrollback limits
- Replace `cloneCell()` with direct assignment during reflow (drain invalidates source)
- Add `Line.isEmpty()` to avoid string allocation for empty-line checks

## Prerequisites

### Development Environment
- Bun (package manager and test runner)
- Docker (for test execution)

### Dependencies
- No new external dependencies required
- All changes are within existing TypeScript source files

### Knowledge Requirements
- Understanding of ring buffer data structure used in `UnifiedBuffer`
- Familiarity with the rendering pipeline (`TerminalState` -> `CanvasRenderer`)
- Understanding of reflow algorithm in `resize()`

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (Vanilla, no framework)
- **Runtime**: Bun (test), Tauri WebView (production)
- **Key Libraries**: None added

### Design Approach

All four improvements are independent optimizations that can be applied in any order. The approach is strictly subtractive/replacive -- removing unnecessary work rather than adding new abstractions.

### Component Interaction

```
CanvasRenderer (render loop)
    |
    +-- getVisibleLines() / getVisibleLinesWithFolding()
    |       |
    |       +-- state.getScrollbackLine(index) [NEW: replaces getScrollbackBuffer()]
    |       +-- state.getScrollbackLength()
    |       +-- buffer.getLine(row)
    |
TerminalState
    |
    +-- primaryBuffer: UnifiedBuffer
            |
            +-- getScrollbackLine(index) [existing]
            +-- resize() -> reflow -> setCell() [FR4: direct assign]
            +-- adjustRowCount() -> capacity fix [FR3]
            +-- Line.isEmpty() [FR5/FR6: replaces getText().trim()]
```

## Implementation Phases

### Phase 1: Line.isEmpty() and getText().trim() Replacement (FR5/FR6)

**Goal**: Add `isEmpty()` method to `Line` class and replace 2 occurrences of `getText().trim() === ""` in `unified-buffer.ts` with `isEmpty()`.

**Files to Modify**:
- `src/terminal/grid.ts`: Add `isEmpty()` method to `Line` class
- `src/terminal/unified-buffer.ts`:
  - Line 750: Replace `lastLine.getText().trim() === ""` with `lastLine.isEmpty()`
  - Line 891: Replace `line.getText().trim() === ""` with `line.isEmpty()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Line.isEmpty() | Determine if all visible cells (width > 0) are spaces | Line instance exists | Returns boolean without string allocation |

**Processing Flow**:
```
1. isEmpty() iterates cells
   +-- Cell has width > 0 and char !== " " -> return false (early exit)
   +-- Cell has width == 0 -> skip (CJK placeholder, matches getText() behavior)
   +-- All cells checked -> return true
```

**Implementation Steps**:

1. **Add isEmpty() to Line class**
   - Add method to `Line` in `grid.ts`
   - Semantics: returns `true` if all cells with `width > 0` have `char === " "` (matches `getText().trim() === ""` which skips width-0 cells)
   - Early-return on first non-empty cell for short-circuit behavior

2. **Replace getText().trim() in unified-buffer.ts**
   - Replace at line 750 (resize reflow empty-line trimming)
   - Replace at line 891 (adjustRowCount empty-line trimming)

3. **Add unit tests for isEmpty()**
   - Test with all-space line -> true
   - Test with non-space content -> false
   - Test with width-0 placeholder cells -> true (skipped like getText())
   - Test with non-space CJK content (width 2) -> false
   - Test with empty (0-column) line -> true

**Dependencies**:
- Requires: None (self-contained)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- `Line.isEmpty()` returns `true` for line of spaces
- `Line.isEmpty()` returns `false` for line with content
- `Line.isEmpty()` returns `true` for line with width-0 placeholder cells (matches getText() behavior)
- `Line.isEmpty()` returns `false` for line with non-space CJK character
- `Line.isEmpty()` returns `true` for 0-column line

*Integration Tests*:
- Existing reflow/resize tests continue to pass (behavior unchanged)

**Acceptance Criteria**:
- [ ] `Line.isEmpty()` exists and returns correct results
- [ ] No `getText().trim() === ""` calls remain in `unified-buffer.ts`
- [ ] All existing tests pass
- [ ] New unit tests for `isEmpty()` pass

**Estimated Effort**: Small (< 1 day)

**Risks and Mitigation**:
- **Risk**: `isEmpty()` semantics differ from `getText().trim() === ""` for edge cases (e.g., width-0 placeholder cells)
  - **Mitigation**: `getText()` skips width-0 cells (only includes `cell.width > 0`). Therefore `isEmpty()` must match this behavior by only checking cells with `width > 0`. The implementation uses `cell.width > 0 && cell.char !== " "` to return false, skipping width-0 cells entirely. This ensures exact semantic parity with `getText().trim() === ""`.

---

### Phase 2: Reflow Cell Reference Move (FR4)

**Goal**: Replace `cloneCell()` with direct cell assignment in `resize()` reflow, since `drain()` invalidates the source lines.

**Files to Modify**:
- `src/terminal/unified-buffer.ts`:
  - Line 733: Replace `cloneCell(trimmedCells[offset + j]!)` with `trimmedCells[offset + j]!`
  - Remove `cloneCell` from the import if no other usages remain in the file

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| resize() reflow loop | Assign cells from drained logical lines to new physical lines | drain() has been called; source lines are orphaned | Cells are moved by reference, no deep copy |

**Processing Flow**:
```
1. drain() extracts all lines from ring buffer (source is orphaned)
2. Logical lines are joined and trimmed
3. Re-split into new physical lines
   +-- For each cell: assign by reference (not clone)
4. New lines pushed into ring buffer
```

**Implementation Steps**:

1. **Replace cloneCell with direct assignment**
   - Change `newLine.setCell(j, cloneCell(trimmedCells[offset + j]!))` to `newLine.setCell(j, trimmedCells[offset + j]!)`
   - This is safe because `drain()` detaches lines from the ring buffer

2. **Clean up unused import**
   - If `cloneCell` is no longer used in `unified-buffer.ts`, remove it from the import statement
   - Verify no other call sites exist in the file

**Dependencies**:
- Requires: None (independent of other phases)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- Existing reflow tests verify identical output (cells have same content)
- No new tests needed -- this is a transparent optimization

*Integration Tests*:
- Resize followed by content read produces identical results

**Acceptance Criteria**:
- [ ] No `cloneCell()` calls in `resize()` reflow path
- [ ] `cloneCell` removed from import if unused
- [ ] All existing reflow tests pass unchanged

**Estimated Effort**: Small (< 1 hour)

**Risks and Mitigation**:
- **Risk**: Shared cell references could cause aliasing bugs if cells are mutated after reflow
  - **Mitigation**: After reflow, the old lines from `drain()` are unreferenced (local variable `allLines` goes out of scope). The cells are only referenced by the new lines in the ring buffer. No aliasing occurs.

---

### Phase 3: adjustRowCount() Capacity Fix (FR3)

**Goal**: Fix capacity recalculation in `adjustRowCount()` so that scrollback limits are preserved when row count changes without width change.

**Files to Modify**:
- `src/terminal/unified-buffer.ts`:
  - `adjustRowCount()` method: Add capacity recalculation and ring buffer rebuild

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| adjustRowCount() | Adjust viewport row count while preserving scrollback capacity | cols unchanged, rows changed | capacity = scrollbackCapacity + newRows; ring buffer resized if needed |

**Processing Flow**:
```
1. Record oldRows before modification
2. Add or trim viewport lines (existing logic)
3. Set _rows = newRows
4. Recalculate capacity
   +-- scrollbackCapacity = capacity - oldRows
   +-- new capacity = scrollbackCapacity + newRows
5. If capacity changed:
   +-- Rebuild ring array with new capacity
   +-- Copy existing lines in order
   +-- Reset head to 0
6. Invalidate scroll region if needed
7. Mark all lines dirty
```

**Implementation Steps**:

1. **Record oldRows at method entry**
   - Store `this._rows` before any modifications

2. **Add capacity recalculation after row adjustment**
   - Compute `scrollbackCapacity = this.capacity - oldRows`
   - Set `this.capacity = scrollbackCapacity + rows`

3. **Rebuild ring buffer if capacity changed**
   - Allocate new array of size `this.capacity`
   - If `this._size > this.capacity`, evict oldest lines: `startIndex = this._size - this.capacity`
   - Copy lines from `startIndex..this._size` using `getAbsolute(i)` in order
   - Reset `this.head = 0`, update `this._size` to new count
   - Call `this.onEvict(evicted)` if lines were evicted (mirrors `resize()` pattern at line 777-789)

4. **Remove dead code**
   - Remove the commented-out/unused capacity calculation at line 906

5. **Add unit tests for capacity preservation**
   - Row decrease: verify capacity decreases and scrollback limit is preserved
   - Row increase: verify capacity increases and no spurious eviction occurs

**Dependencies**:
- Requires: None (independent)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- `adjustRowCount()` row decrease: capacity = scrollbackCap + newRows
- `adjustRowCount()` row increase: capacity = scrollbackCap + newRows
- Row decrease does not evict scrollback when within limits
- Row decrease then increase round-trips correctly
- `_size <= capacity` invariant holds after all row changes
- `onEvict()` called with correct count when new capacity < current size

*Integration Tests*:
- Resize with same cols triggers `adjustRowCount()` and produces correct buffer state

**Acceptance Criteria**:
- [ ] `adjustRowCount()` recalculates capacity as `(capacity - oldRows) + newRows`
- [ ] Ring buffer is rebuilt when capacity changes
- [ ] Dead capacity code is removed
- [ ] New unit tests pass
- [ ] All existing tests pass

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Ring buffer rebuild during adjustRowCount may introduce off-by-one errors
  - **Mitigation**: The same rebuild pattern is already used in `resize()` (Phase 4 of reflow). Follow the identical approach. Thorough unit tests for boundary conditions.
- **Risk**: Changing capacity may cause scrollback eviction in edge cases
  - **Mitigation**: The formula preserves the original scrollback capacity. When rows decrease, capacity decreases by the same amount, so `scrollbackCapacity` stays constant. When rows increase, capacity grows, allowing more total lines.

---

### Phase 4: Scrollback Access Optimization (FR1/FR2)

**Goal**: Refactor the renderer to use index-based scrollback access instead of materializing the full scrollback array. Add `getScrollbackLine()` to `TerminalState` public API.

**Files to Modify**:
- `src/terminal/state.ts`:
  - Add `getScrollbackLine(index: number): Line` public method
- `src/terminal/canvas-renderer.ts`:
  - Refactor `getVisibleLines()` to use index-based access
  - Refactor `getVisibleLinesWithFolding()` to use index-based access

Note: `handlers/types.ts` (`TerminalStateAccessor`) does NOT need modification. `getScrollbackLine()` is renderer-facing only. The renderer uses `TerminalState` directly, not the handler interface.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TerminalState.getScrollbackLine() | Expose single scrollback line from primary buffer | Valid index within scrollback bounds | Returns Line reference (no clone) |
| getVisibleLines() | Build visible line array for rendering | scrollOffset >= 0 | Returns O(visibleRows) lines, no full scrollback clone |
| getVisibleLinesWithFolding() | Build visible line array with fold regions | Fold manager available | Returns O(visibleRows) lines, no full scrollback clone |

**Processing Flow for getVisibleLines()**:
```
1. If scrollOffset == 0
   +-- Return viewport lines directly (no change from current)
2. If scrollOffset > 0
   +-- Get scrollbackLength and visibleRows
   +-- Calculate startIndex = scrollbackLength + visibleRows - visibleRows - scrollOffset
   +-- For each visible row (0..visibleRows):
       +-- lineIndex = startIndex + row
       +-- If lineIndex < scrollbackLength -> state.getScrollbackLine(lineIndex)
       +-- Else -> buffer.getLine(lineIndex - scrollbackLength)
```

**Processing Flow for getVisibleLinesWithFolding()**:
```
1. Get scrollbackLength via state.getScrollbackLength()
2. For each display row:
   +-- Map display line to actual line (via foldManager)
   +-- If actualLine < scrollbackLength -> state.getScrollbackLine(actualLine)
   +-- Else -> buffer.getLine(actualLine - scrollbackLength)
3. No full scrollback array materialization
```

**Implementation Steps**:

1. **Add getScrollbackLine() to TerminalState**
   - Delegates to `this.primaryBuffer.getScrollbackLine(index)`
   - Returns direct reference (no clone) since renderer reads synchronously

2. **Refactor getVisibleLines()**
   - Replace `state.getScrollbackBuffer()` with index-based loop
   - Use `state.getScrollbackLine(i)` for scrollback lines
   - Use `buffer.getLine(row)` for screen lines
   - Maintain identical output semantics

3. **Refactor getVisibleLinesWithFolding()**
   - Replace `state.getScrollbackBuffer()` with `state.getScrollbackLength()`
   - Access individual scrollback lines via `state.getScrollbackLine()`
   - Remove `scrollbackBuffer` local variable

4. **Evaluate getScrollbackBuffer() disposition**
   - `terminal-app/index.ts` still calls `getScrollbackBuffer()` for search
   - Retain `getScrollbackBuffer()` in `state.ts` for non-performance-critical callers
   - Verify no renderer path calls it

**Dependencies**:
- Requires: None (independent, but implemented last due to larger scope)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- `TerminalState.getScrollbackLine()` returns correct line
- `getVisibleLines()` with scrollOffset > 0 returns correct lines (existing tests)
- `getVisibleLines()` with scrollOffset = 0 unchanged behavior

*Integration Tests*:
- Renderer displays correct content when scrolled back
- Renderer displays correct content with fold regions when scrolled back
- Resize followed by scroll-back shows correctly reflowed content

**Acceptance Criteria**:
- [ ] `TerminalState.getScrollbackLine()` exists and works
- [ ] No `getScrollbackBuffer()` calls in `canvas-renderer.ts`
- [ ] `getVisibleLines()` allocates O(visibleRows), not O(scrollbackLength)
- [ ] `getVisibleLinesWithFolding()` allocates O(visibleRows), not O(scrollbackLength)
- [ ] All existing tests pass
- [ ] Type check passes

**Estimated Effort**: Medium (2-3 days)

**Risks and Mitigation**:
- **Risk**: Returning direct Line references (no clone) from `getScrollbackLine()` could allow renderer to mutate buffer state
  - **Mitigation**: The renderer only reads cell data for rendering; it does not mutate lines. The existing `buffer.getLine(row)` in the non-scrolled path already returns references. The same safety model applies.
- **Risk**: Index calculation errors in refactored `getVisibleLines()` could cause incorrect rendering
  - **Mitigation**: Existing tests for `getVisibleLines()` with various scrollOffset values provide regression coverage. Add explicit tests comparing old and new behavior.

---

## Complete File Structure

```
src/terminal/
+-- grid.ts                 # MODIFIED: Add Line.isEmpty() (Phase 1)
+-- unified-buffer.ts       # MODIFIED: FR3 capacity, FR4 cloneCell, FR6 isEmpty (Phases 1-3)
+-- state.ts                # MODIFIED: Add getScrollbackLine() (Phase 4)
+-- canvas-renderer.ts      # MODIFIED: Index-based scrollback access (Phase 4)
+-- grid.test.ts            # MODIFIED: Add isEmpty() tests (Phase 1)
+-- unified-buffer.test.ts  # MODIFIED: Add capacity tests (Phase 3)
+-- canvas-renderer.test.ts # MODIFIED: Update/add scrollback access tests (Phase 4)
+-- state.test.ts           # MODIFIED: Add getScrollbackLine() tests (Phase 4)
```

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner (`bun test`)
- Table-driven tests for `isEmpty()` edge cases
- Direct buffer manipulation for capacity tests

**Test Coverage Goals**:
- `Line.isEmpty()`: 100% (small method, all branches)
- `adjustRowCount()` capacity logic: All boundary conditions
- `getScrollbackLine()` on TerminalState: Happy path + bounds

**Key Test Areas**:

1. **Line.isEmpty()** (`grid.test.ts`)
   - All-space line (true)
   - Non-space content (false)
   - Width-0 placeholder cells (true, skipped like getText())
   - Non-space CJK character (false)
   - Zero-length line (true)

2. **adjustRowCount() capacity** (`unified-buffer.test.ts`)
   - Row decrease preserves scrollback capacity
   - Row increase preserves scrollback capacity
   - No spurious eviction after row decrease
   - Round-trip row decrease + increase
   - `_size <= capacity` invariant after all changes
   - `onEvict()` called when capacity shrinks below current size

3. **getScrollbackLine() on TerminalState** (`state.test.ts`)
   - Returns correct line by index
   - Returns direct reference (not clone)
   - Delegates to primaryBuffer

4. **getVisibleLines() refactored** (`canvas-renderer.test.ts`)
   - scrollOffset = 0 (unchanged behavior)
   - scrollOffset > 0 (index-based access)
   - scrollOffset = maxScrollback (oldest lines)

### Integration Testing

**Scenarios**:
- Reflow with cloneCell removal produces identical cell content
- Resize with row-only change maintains correct scrollback

### E2E Testing (Docker)

- [ ] All 1741+ existing tests pass via `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- [ ] TypeScript type check passes via `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`

### Manual Testing (E2E Not Possible)

- [ ] Scrollback rendering is visually identical (requires terminal with real scrollback)
- [ ] Resize followed by scroll-back shows correct content (visual verification)

## Dependencies

### External Dependencies

None added.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 (Line.isEmpty + getText replacement) - no dependencies
2. Phase 2 (cloneCell removal) - no dependencies
3. Phase 3 (adjustRowCount capacity fix) - no dependencies
4. Phase 4 (scrollback access optimization) - no dependencies

All four phases are independent and can be implemented in any order. The specified order goes from smallest to largest scope.

**Component Dependencies**:
- Phase 4 requires `getScrollbackLine()` on `TerminalState` (public method, not on handler interface), which delegates to existing `UnifiedBuffer.getScrollbackLine()`
- Phase 1 requires `isEmpty()` on `Line`, used by `UnifiedBuffer.resize()` and `adjustRowCount()`

## Risk Assessment

### Technical Risks

1. **Cell Reference Aliasing (Phase 2)**
   - **Risk**: Direct cell assignment could cause shared mutable state
   - **Likelihood**: Low (drain() orphans source lines)
   - **Impact**: High (data corruption)
   - **Mitigation**: Source lines from `drain()` are local to `resize()` and go out of scope. No aliasing possible.

2. **Capacity Calculation Errors (Phase 3)**
   - **Risk**: Incorrect capacity formula could cause scrollback eviction or buffer overflow
   - **Likelihood**: Medium (arithmetic edge cases)
   - **Impact**: High (data loss)
   - **Mitigation**: Formula mirrors `resize()` which already works correctly. Dedicated boundary-condition unit tests.

3. **Index Calculation Errors in Renderer (Phase 4)**
   - **Risk**: Off-by-one errors when mapping scrollOffset to line indices
   - **Likelihood**: Medium (complex index math)
   - **Impact**: High (incorrect rendering)
   - **Mitigation**: Existing `getVisibleLines()` tests provide regression safety. Add explicit comparison tests.

### Implementation Risks

1. **Regression in Existing Tests**
   - **Risk**: Changes break existing behavior
   - **Mitigation**: All 1741+ tests must pass after each phase. Run full test suite in Docker after each change.

## Performance Considerations

1. **Scrollback Rendering (Phase 4)**
   - Before: O(scrollbackLength) Line clones per frame
   - After: O(visibleRows) direct references per frame
   - With 10,000 scrollback lines and 24 visible rows: ~400x reduction in allocations

2. **isEmpty() (Phase 1)**
   - Before: String concatenation + trim() creates 2 temporary strings
   - After: Direct cell scan with early exit, zero allocations
   - Improvement is proportional to line length for non-empty lines (early exit)

3. **cloneCell Removal (Phase 2)**
   - Before: Deep copy of each cell (object allocation + attribute clone)
   - After: Reference assignment (no allocation)
   - Proportional to total cells reflowed

4. **Capacity Fix (Phase 3)**
   - Correctness fix, not a performance improvement per se
   - Ring buffer rebuild only occurs when row count changes (infrequent)

## Open Questions

### From Specification:
- [ ] Should `getScrollbackBuffer()` be removed entirely or kept for potential external consumers?
  - **Current answer**: Keep it. `terminal-app/index.ts` uses it for search (`getAllLineTexts()`). It's not on the render hot path.

### Implementation-Specific:
- [ ] Should `getScrollbackLine()` on `TerminalState` return a clone for safety, or a direct reference for performance?
  - **Proposed answer**: Direct reference. The renderer reads synchronously within a single frame and does not mutate lines. This matches the existing pattern where `buffer.getLine(row)` returns direct references.

## Future Enhancements

- Optimize `terminal-app/index.ts` `getAllLineTexts()` to use index-based access (currently calls `getScrollbackBuffer()` but only for search, not per-frame)
- Add lazy loading for very large scrollback (virtual scrolling)
- Consider `Line.isEmpty()` caching via dirty flag

## Success Metrics

### Functional Completeness
- [ ] All four FRs (FR1-FR6) implemented
- [ ] All existing 1741+ tests pass
- [ ] New unit tests pass

### Quality Metrics
- [ ] Type check passes (`bun run typecheck`)
- [ ] No `cloneCell()` in reflow path
- [ ] No `getText().trim()` in `unified-buffer.ts`
- [ ] No `getScrollbackBuffer()` in `canvas-renderer.ts`
- [ ] `adjustRowCount()` updates capacity correctly

### Performance Metrics
- [ ] Scrollback rendering allocates O(visibleRows) per frame, not O(scrollbackLength)
- [ ] Reflow 10,000 lines completes within budget (< 500ms in Docker)

## References

- **Specification**: `doc/tasks/unified-buffer-perf/SPEC.md`
- **Requirements**: `doc/tasks/unified-buffer-perf/要件定義書.md`
- **Ring Buffer Implementation**: `src/terminal/unified-buffer.ts`
- **Renderer**: `src/terminal/canvas-renderer.ts`
- **Grid/Line**: `src/terminal/grid.ts`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - `/sdd.3-verify-plan` for consistency verification and design review
   - Address open questions
   - Confirm approach

2. **Begin Implementation**
   - Start with Phase 1 (smallest, most isolated)
   - Run full test suite after each phase
   - `/sdd.4-implement` to execute
