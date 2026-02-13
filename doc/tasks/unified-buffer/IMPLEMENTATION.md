# Implementation Plan: Unified Buffer

## Overview

Replace the split buffer architecture (`scrollbackBuffer: Line[]` + `ScreenBuffer`) with a `UnifiedBuffer` class backed by a ring buffer. Implement full-buffer reflow with WezTerm-style cursor tracking to eliminate display corruption during terminal resize.

## Objectives

- Implement ring buffer data structure for unified line storage
- Maintain ScreenBuffer-compatible API for all existing operations
- Implement full-buffer reflow (scrollback + screen) with cursor position tracking
- Integrate into TerminalState, removing separate scrollbackBuffer
- Pass all 1712 existing tests

## Prerequisites

### Development Environment

- Bun (package manager and test runner)
- Docker (for test execution per project conventions)

### Dependencies

- None (self-contained TypeScript; reuses existing `Line`, `Cell`, `CursorState`)

### Knowledge Requirements

- Current ScreenBuffer API (buffer.ts) — all public methods
- TerminalState scrollback integration (state.ts) — addToScrollback, getScrollbackBuffer
- Reflow algorithm (reflowLarger, reflowSmaller) — logical line reconstruction via wrapped flag
- Handler interface (handlers/types.ts) — TerminalStateAccessor.getActiveBuffer()

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (Vanilla)
- **Test Runner**: Bun test
- **Existing Modules**: `grid.ts` (Line, Cell), `cursor.ts` (CursorState), `attributes.ts`

### Design Approach

Bottom-up: build the ring buffer core, then layer operations, then reflow, then integrate. Each phase produces independently testable results. The existing `ScreenBuffer` is not modified — the new `UnifiedBuffer` is built from scratch with the same external API, then swapped in at the integration phase.

### Component Interaction

```
UnifiedBuffer
├── RingBuffer core (push, get, drain, capacity management)
├── Viewport abstraction (getLine, setCell — last N lines)
├── Scroll operations (scrollUp/Down — viewport-relative, with scroll region)
├── Line/character manipulation (insert/delete lines, insert/delete/erase chars)
├── Reflow engine (drain → join → re-split → refill, with cursor tracking)
└── Scrollback access (getScrollbackLine, scrollbackLength)

TerminalState
├── primaryBuffer: UnifiedBuffer (capacity = scrollbackLines + rows)
├── alternateBuffer: UnifiedBuffer (capacity = rows, no reflow)
└── resize() → delegates to buffer.resize() → uses returned cursor position
```

## Implementation Phases

### Phase 1: Ring Buffer + UnifiedBuffer Core

**Goal**: Implement the ring buffer data structure and basic UnifiedBuffer with line access, cell access, and viewport abstraction. All basic read/write operations pass tests.

**Files to Create**:
- `src/terminal/unified-buffer.ts` — UnifiedBuffer class with ring buffer internals

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| RingBuffer internals | Fixed-capacity circular storage of Line objects | Capacity > 0 | Lines stored/retrieved by index in O(1) |
| UnifiedBuffer constructor | Initialize ring buffer with `rows` empty lines | cols > 0, rows > 0 | Buffer has `rows` viewport lines, scrollbackLength = 0 |
| getLine(row) | Access viewport line by screen-relative row | 0 <= row < rows | Returns the Line at viewport position |
| getCell(col, row) / setCell(col, row, cell) | Cell-level access delegating to getLine | Valid col/row | Cell read/written |
| getScrollbackLine(index) | Access scrollback line by index | 0 <= index < scrollbackLength | Returns historical line |
| push(line) | Append line to ring, evict oldest if at capacity | Line is valid | size incremented or head advanced |
| drain() | Extract all lines as array, reset ring | Buffer non-empty | Returns Line[], buffer empty |

**Processing Flow**:
```
Ring buffer index mapping:
1. Absolute index i → ring[(head + i) % capacity]
2. Viewport row r → absolute index (size - rows + r)
3. Scrollback index s → absolute index s
4. scrollbackLength → max(0, size - rows)
```

**Implementation Steps**:

1. **Ring buffer core**
   - Fixed-capacity array with head pointer and size counter
   - push: append or overwrite-oldest with head advance
   - get: index-mapped access
   - drain: collect all lines sequentially, reset state

2. **UnifiedBuffer constructor and basic accessors**
   - Accept cols, rows, capacity (scrollbackLines + rows for primary, rows for alternate)
   - Initialize with `rows` empty Line objects
   - Expose cols, rows, size, scrollbackLength as getters

3. **Viewport and scrollback line access**
   - getLine(row): delegate to ring get with viewport offset
   - getScrollbackLine(index): delegate to ring get directly
   - getCell/setCell: delegate to getLine

4. **Scroll region management**
   - setScrollRegion, clearScrollRegion, getScrollRegion, getEffectiveScrollRegion
   - Same logic as current ScreenBuffer

5. **Clear operations**
   - clearAll, clearLine, clearLineFromCursor, clearLineToCursor, clearBelow, clearAbove
   - clearScrollback: reset ring buffer to retain only viewport lines (head=0, size=rows). Used by ED 3 (Erase Scrollback).
   - Same logic as ScreenBuffer, operating on viewport lines

6. **Buffer utility operations**
   - clone(): create a deep copy of the UnifiedBuffer (ring buffer, head, size, capacity, dimensions). Used for alternate screen buffer operations.

**Dependencies**:
- Requires: `grid.ts` (Line, Cell, createEmptyCell)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests*:
- Ring buffer push/get with various fill levels
- Ring buffer capacity overflow (oldest evicted)
- Ring buffer drain returns all lines in order
- UnifiedBuffer constructor initializes correct number of lines
- Viewport getLine returns correct lines
- Scrollback access with varying buffer sizes
- Cell read/write through viewport
- Scroll region set/clear/get

**Acceptance Criteria**:
- [ ] Ring buffer handles push/get/drain for buffers below, at, and above capacity
- [ ] UnifiedBuffer viewport access matches ScreenBuffer behavior for same operations
- [ ] Clear operations work identically to ScreenBuffer
- [ ] Type check passes

**Estimated Effort**: 中 (3-5 days)

---

### Phase 2: Scroll Operations + Line/Character Manipulation

**Goal**: Implement scroll, insert, delete, and erase operations compatible with ScreenBuffer API. Existing buffer.test.ts operation tests pass when migrated to UnifiedBuffer.

**Files to Modify**:
- `src/terminal/unified-buffer.ts` — Add scroll and manipulation methods

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| scrollUp(count) | Remove top viewport lines within scroll region, add blanks at bottom. Lines scroll off into scrollback implicitly. | count > 0 | Top lines become scrollback, blanks at bottom |
| scrollDown(count) | Remove bottom viewport lines within scroll region, add blanks at top | count > 0 | Bottom lines removed, blanks at top |
| insertLines(row, count) | Insert blank lines at row, push down within scroll region | row within scroll region | Blanks inserted, bottom lines removed |
| deleteLines(row, count) | Delete lines at row, pull up within scroll region | row within scroll region | Lines removed, blanks at bottom |
| insertCharacters(row, col, count) | Shift cells right, insert blanks | Valid row, col | Cells shifted, blanks at position |
| deleteCharacters(row, col, count) | Shift cells left, blanks at end | Valid row, col | Cells shifted, blanks at end |
| eraseCharacters(row, col, count) | Replace cells with blanks (no shift) | Valid row, col | Cells blanked in-place |
| getDirtyRows() / clearAllDirty() | Dirty tracking for renderer | - | Dirty state managed |

**Processing Flow**:
```
scrollUp within unified buffer:
1. Determine effective scroll region (top, bottom)
2. If full-screen scroll (top=0 AND bottom=rows-1):
   ├─ Push new blank line to ring buffer end
   ├─ Old viewport top line implicitly becomes scrollback (ring size grows or head advances)
   └─ No callback or clone needed (key difference from ScreenBuffer)
3. If partial scroll region (top > 0 OR bottom < rows-1):
   └─ Lines within region are rearranged in-place (same as ScreenBuffer)
   └─ Lines outside region are NOT affected
4. Insert blank lines at bottom of region (for case 3; case 2 handles this via push)
```

**Implementation Steps**:

1. **scrollUp with implicit scrollback**
   - For full-screen scroll (top=0 AND bottom=rows-1): push blank line to ring buffer end — the line that was at viewport row 0 implicitly becomes the last scrollback line
   - For partial scroll region (top>0 OR bottom<rows-1): rearrange lines within the region in-place (same as ScreenBuffer). Note: top=0 with bottom<rows-1 is NOT full-screen and must use the in-place path.
   - Key insight: no onLinesRemovedCallback needed; scrollback is automatic

2. **scrollDown**
   - Same logic as ScreenBuffer but operating through viewport abstraction

3. **Line insertion/deletion**
   - insertLines, deleteLines — operate within scroll region, same as ScreenBuffer

4. **Character manipulation**
   - insertCharacters, deleteCharacters, eraseCharacters — operate on individual lines via getLine

5. **Dirty tracking**
   - getDirtyRows, clearAllDirty — iterate viewport lines

**Dependencies**:
- Requires: Phase 1 (core ring buffer and viewport access)
- Blocks: Phase 4 (TerminalState integration)

**Testing Approach**:

*Unit Tests*:
- scrollUp: line content shifts, new blank at bottom
- scrollUp: line becomes scrollback (verify via getScrollbackLine)
- scrollUp: with scroll region (partial area only)
- scrollDown: line content shifts, new blank at top
- insertLines/deleteLines within scroll region
- Character insert/delete/erase operations
- Dirty tracking after mutations

**Acceptance Criteria**:
- [ ] scrollUp at top of screen makes lines accessible as scrollback
- [ ] scrollUp with scroll region only affects region
- [ ] All line/character manipulation matches ScreenBuffer behavior
- [ ] Dirty tracking works correctly
- [ ] Type check passes

**Estimated Effort**: 中 (3-5 days)

---

### Phase 3: Full-Buffer Reflow with Cursor Tracking

**Goal**: Implement the resize/reflow algorithm that operates on the entire buffer (scrollback + screen) with WezTerm-style cursor position tracking. This is the core of the feature.

**Files to Modify**:
- `src/terminal/unified-buffer.ts` — Add resize() and resizeNoReflow() methods

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| resize(cols, rows, cursorRow, cursorCol) | Full-buffer reflow with cursor tracking | Valid dimensions and cursor | Buffer reflowed, adjusted cursor returned |
| resizeNoReflow(cols, rows) | Resize lines in-place without reflow (for alternate) | Valid dimensions | Lines resized, no content rearrangement |
| reflowWithCursorTracking | Join wrapped lines, track cursor, re-split at new width | Lines drained, cursor position known | Reflowed lines, adjusted cursor position |

**Processing Flow**:
```
resize(cols, rows, cursorRow, cursorCol):
1. If cols unchanged → skip reflow, just resize lines and adjust row count
2. Drain all lines from ring buffer
3. Compute cursor's absolute position (scrollbackLength + cursorRow)
4. Join wrapped lines into logical lines (same grouping as existing reflowLarger/reflowSmaller)
   ├─ Start with first physical line's cells
   ├─ While next line has wrapped=true → merge continuation cells
   ├─ If cursor is on any physical line in group → record logical offset
   └─ When no more continuations → logical line complete, re-split
5. Re-split logical lines at new width
   ├─ Set wrapped flag on continuation lines
   ├─ If cursor was in this logical line → compute new (col, row)
   └─ Count output lines
6. Trim empty lines from bottom if total > rows
7. If still excess after trimming → lines at top become scrollback
8. Refill ring buffer with reflowed lines
9. Return adjusted cursor position
```

**Implementation Steps**:

1. **Drain and cursor position mapping**
   - Drain all lines from ring buffer
   - Map cursor from viewport-relative to absolute index in drained array

2. **Logical line joining with cursor accumulation**
   - Walk through drained lines
   - On wrapped lines, accumulate cell count into running offset
   - When cursor's absolute row matches current physical line, record accumulated x offset

3. **Re-split with cursor reverse-mapping**
   - For each logical line, distribute cells at new width
   - When cursor was in this logical line, compute: row = offset / newCols, col = offset % newCols
   - Set wrapped flag on continuation lines (offset > 0)

4. **Post-reflow row adjustment**
   - Trim empty trailing lines if total exceeds rows
   - Enforce capacity limit (scrollback + rows)
   - If reflowed lines exceed capacity, oldest lines are discarded

5. **resizeNoReflow for alternate buffer**
   - Resize each line in-place without reflow
   - Add/remove rows as needed

**Dependencies**:
- Requires: Phase 1 (drain/push), Phase 2 (not strictly, but scroll operations should exist)
- Blocks: Phase 4

**Testing Approach**:

*Unit Tests*:
- Narrowing: 10-char line in 10-col buffer → resize to 5 cols → 2 wrapped lines
- Widening: 2 wrapped 5-char lines → resize to 10 cols → 1 merged line
- Cursor tracking narrow: cursor at col 7 → resize narrower → cursor at correct new row/col
- Cursor tracking wide: cursor on wrapped continuation → resize wider → cursor on merged line
- Scrollback reflow: lines in scrollback region are also reflowed
- Empty line trimming: excess empty lines removed from bottom first
- Hard line breaks preserved (non-wrapped lines stay separate)
- Wide characters (CJK): width-2 chars handled correctly during re-split
- Edge case: resize to 1 column
- Edge case: empty buffer resize
- Edge case: cursor at column 0 boundary
- resizeNoReflow: lines resized without content rearrangement

**Acceptance Criteria**:
- [ ] Narrowing wraps lines and cursor position is correct
- [ ] Widening unwraps lines and cursor position is correct
- [ ] Scrollback lines are reflowed together with screen lines
- [ ] Empty trailing lines trimmed before pushing to scrollback
- [ ] Hard line breaks preserved across reflow
- [ ] resizeNoReflow works for alternate buffer
- [ ] Type check passes

**Estimated Effort**: 大 (1-2 weeks)

**Risks and Mitigation**:
- **Risk**: Wide character handling at split boundaries may cause off-by-one errors
  - **Mitigation**: Port existing CJK tests from buffer.test.ts, add explicit boundary tests

---

### Phase 4: TerminalState Integration

**Goal**: Replace ScreenBuffer with UnifiedBuffer in TerminalState. Remove separate scrollbackBuffer. Update handler types. All 1712 existing tests pass.

**Files to Modify**:
- `src/terminal/state.ts`:
  - Change primaryBuffer/alternateBuffer type to UnifiedBuffer
  - Remove scrollbackBuffer field and addToScrollback method
  - Update resize() to use cursor-tracked reflow result
  - Update getScrollbackBuffer()/getScrollbackLength() to delegate to UnifiedBuffer
  - Update switchToAlternateBuffer/switchToPrimaryBuffer
- `src/terminal/handlers/types.ts`:
  - Change getActiveBuffer() return type from ScreenBuffer to UnifiedBuffer
- `src/terminal/index.ts`:
  - Export UnifiedBuffer instead of (or in addition to) ScreenBuffer
- `src/terminal/buffer.test.ts`:
  - Update imports to use UnifiedBuffer

**Files to Potentially Modify**:
- `src/terminal/canvas-renderer.ts` — Update import if type name changed
- Any file importing ScreenBuffer from index.ts

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TerminalState.resize() | Delegate to UnifiedBuffer.resize with cursor, apply returned position | New dimensions valid | Cursor correctly positioned after reflow |
| TerminalState.getScrollbackBuffer() | Materialize scrollback from UnifiedBuffer | Primary buffer active | Returns Line[] of scrollback lines |
| TerminalState.getScrollbackLength() | Query UnifiedBuffer.scrollbackLength | - | Returns count of scrollback lines |
| TerminalState constructor | Create UnifiedBuffer with scrollback capacity | Valid cols, rows, scrollbackLines | Primary buffer with full capacity, alternate null |
| switchToAlternateBuffer | Create UnifiedBuffer with rows-only capacity, no reflow | Not already alternate | Alternate buffer created, primary preserved |

**Processing Flow**:
```
TerminalState.resize(cols, rows):
1. If primary buffer active:
   ├─ Call primaryBuffer.resize(cols, rows, cursor.row, cursor.col)
   ├─ Apply returned cursor position to primaryCursor
   └─ Clamp cursor within new dimensions
2. If alternate buffer exists:
   └─ Call alternateBuffer.resizeNoReflow(cols, rows)
3. Resize cursor objects (clamp)
4. Reset tab stops, wrapPending, graphemeBuffer
```

**Implementation Steps**:

1. **Update type references**
   - handlers/types.ts: change getActiveBuffer return type
   - state.ts: import UnifiedBuffer instead of ScreenBuffer
   - index.ts: export UnifiedBuffer (keep ScreenBuffer export temporarily for backward compat)

2. **Refactor TerminalState constructor**
   - Create primaryBuffer as UnifiedBuffer with capacity = scrollbackLines + rows
   - Remove onLinesRemoved callback (no longer needed)

3. **Refactor resize()**
   - Pass cursor position to buffer.resize()
   - Apply returned adjusted cursor
   - Handle alternate buffer with resizeNoReflow

4. **Remove scrollback management and add eviction notification**
   - Remove scrollbackBuffer field
   - Remove addToScrollback method
   - Delegate getScrollbackBuffer/Length to primaryBuffer
   - Add `onEvict` callback to UnifiedBuffer (called when ring buffer evicts oldest lines on capacity overflow)
   - In TerminalState constructor, set onEvict to call `semanticZoneTracker.pruneBeforeLine(count)` and `foldManager.pruneBeforeLine(count)` (replacing the prune logic from the removed `addToScrollback`)

5. **Update alternate screen switching**
   - Create alternate as UnifiedBuffer with rows-only capacity
   - Ensure primary buffer state preserved during switch

6. **Run existing tests and fix compatibility issues**
   - Run all 1712 tests
   - Fix any API mismatches discovered

**Dependencies**:
- Requires: Phase 1, Phase 2, Phase 3 (complete UnifiedBuffer)
- Blocks: Phase 5

**Testing Approach**:

*Unit Tests*:
- All existing buffer.test.ts tests pass with UnifiedBuffer
- All existing state.*.test.ts tests pass
- TerminalState.resize() correctly updates cursor
- scrollback access returns correct historical lines
- Alternate screen creation and switching

*Integration Tests*:
- Process terminal actions → verify buffer state
- Resize during active session → verify no content loss

**Acceptance Criteria**:
- [ ] All 1712 existing tests pass
- [ ] ScreenBuffer no longer used in any production code
- [ ] scrollbackBuffer field removed from TerminalState
- [ ] Type check passes

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: Hidden dependencies on ScreenBuffer internals (constructor signature, etc.)
  - **Mitigation**: Search all imports/references before migration; maintain type-compatible API

---

### Phase 5: Renderer + Final Integration

**Goal**: Update renderer for UnifiedBuffer compatibility. Verify end-to-end behavior with manual testing. Performance validation.

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` — Update imports and type references if needed
- `src/terminal/buffer.ts` — Remove or deprecate (keep as re-export if needed)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| getVisibleLines() | Return viewport lines from unified buffer | TerminalState uses UnifiedBuffer | Same Line[] output as before |
| getVisibleLinesWithFolding() | Fold-aware line access | Fold regions defined | Correct lines with fold markers |
| Scroll navigation | scrollUp/scrollDown in renderer | scrollbackLength available | Correct scroll offset behavior |

**Implementation Steps**:

1. **Update renderer imports**
   - If type name changed, update import in canvas-renderer.ts
   - Verify getVisibleLines still works (should work via TerminalState API)

2. **Verify scrollback rendering**
   - Scrollback lines are now direct references (not clones)
   - Verify renderer's synchronous read pattern is safe

3. **Remove old buffer.ts**
   - Remove ScreenBuffer class
   - Or keep as re-export alias: `export { UnifiedBuffer as ScreenBuffer }`

4. **Manual testing**
   - Narrow → wide resize with content preservation
   - Image viewer show → resize → hide
   - vim/less alternate screen
   - Large scrollback + resize
   - Rapid consecutive resizes

5. **Performance validation**
   - Measure reflow time with 10,000 scrollback lines
   - Compare getLine access time before/after

**Dependencies**:
- Requires: Phase 4 (full integration)
- Blocks: None (final phase)

**Testing Approach**:

*Manual Testing*:
- [ ] Echo long text → narrow terminal → wide terminal → verify text preserved
- [ ] Echo multiple commands → narrow → wide → verify all output visible
- [ ] Show image viewer → resize window → close viewer → verify display
- [ ] Open vim → resize → exit → verify terminal state
- [ ] Scroll back through history after resize → verify correctly wrapped
- [ ] Rapid resize (drag window edge) → verify no crash or corruption

*Performance Testing*:
- [ ] Generate 10,000 lines of scrollback → resize → measure time < 100ms

**Acceptance Criteria**:
- [ ] All 1712 tests pass
- [ ] Type check passes
- [ ] Manual resize test: content preserved through narrow → wide cycle
- [ ] Manual image viewer test: display correct after viewer close + resize
- [ ] Manual alternate screen test: vim/less work correctly
- [ ] Reflow performance: < 100ms for 10,000 lines

**Estimated Effort**: 小 (1-2 days)

---

## Complete File Structure

```
src/terminal/
├── unified-buffer.ts          # NEW: UnifiedBuffer class (ring buffer + reflow + cursor tracking)
├── buffer.ts                  # DEPRECATED: re-export UnifiedBuffer as ScreenBuffer (for gradual migration)
├── grid.ts                    # UNCHANGED
├── cursor.ts                  # UNCHANGED
├── attributes.ts              # UNCHANGED
├── state.ts                   # MODIFIED: use UnifiedBuffer, remove scrollbackBuffer
├── canvas-renderer.ts         # MINIMAL: update imports if needed
├── handlers/
│   ├── types.ts               # MODIFIED: getActiveBuffer() returns UnifiedBuffer
│   ├── print_handler.ts       # UNCHANGED
│   ├── c0_handlers.ts         # UNCHANGED
│   ├── csi_handlers.ts        # UNCHANGED
│   ├── esc_handlers.ts        # UNCHANGED
│   └── index.ts               # UNCHANGED
├── index.ts                   # MODIFIED: export UnifiedBuffer
├── buffer.test.ts             # MODIFIED: import UnifiedBuffer
└── unified-buffer.test.ts     # NEW: ring buffer + reflow + cursor tracking tests
```

## Testing Strategy

### Unit Testing

**Approach**: Bun test, Docker-first execution

**Test Coverage Goals**:
- Ring buffer operations: 95%+
- Reflow + cursor tracking: 90%+
- Integration (TerminalState): existing test coverage maintained

**Key Test Areas**:

1. **Ring Buffer** (`unified-buffer.test.ts`)
   - Push/get below, at, above capacity
   - Drain correctness
   - Viewport vs scrollback index mapping

2. **Scroll Operations** (`unified-buffer.test.ts`)
   - Full-screen scroll with implicit scrollback
   - Partial scroll region
   - Line/character manipulation

3. **Reflow** (`unified-buffer.test.ts`)
   - Narrow/wide transitions
   - Cursor tracking through reflow
   - Wide character handling
   - Empty line trimming
   - Hard line break preservation

4. **Integration** (existing test files)
   - All existing buffer.test.ts tests pass
   - All existing state.*.test.ts tests pass

### E2E Testing (Docker)

```bash
# Full test suite
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# Type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Manual Testing

- [ ] Narrow → wide resize preserves content
- [ ] Image viewer + resize + close displays correctly
- [ ] Alternate screen (vim) works after resize
- [ ] Scrollback browsing after resize shows reflowed content
- [ ] Rapid consecutive resizes don't crash

## Risk Assessment

### Technical Risks

1. **scrollUp behavior change (implicit scrollback vs callback)**
   - **Risk**: The current scrollUp clones lines via callback. UnifiedBuffer keeps lines in-place. If any code depends on scrollback lines being independent copies, mutations could affect history.
   - **Likelihood**: Low (renderer reads synchronously, no concurrent mutation)
   - **Mitigation**: `getScrollbackBuffer()` clones lines at the API boundary (matching current behavior). Internal `getScrollbackLine()` returns direct references for performance but is used only within TerminalState.

2. **Reflow cursor tracking edge cases**
   - **Risk**: Cursor at column 0, cursor on wrapped continuation, cursor beyond line content
   - **Likelihood**: Medium
   - **Mitigation**: Port WezTerm's boundary handling logic; comprehensive edge case tests

3. **Performance regression for large scrollback**
   - **Risk**: Draining 10,000+ lines for reflow may cause noticeable latency
   - **Likelihood**: Low (single sequential pass)
   - **Mitigation**: Measure early in Phase 3; consider incremental reflow if needed

## Open Questions

- [ ] Should getScrollbackBuffer() return a lazy iterator instead of materializing an array? (Affects canvas-renderer.ts getVisibleLinesWithFolding)
- [ ] Should logical line length be capped at 1024 cells (WezTerm approach) to prevent pathological reflow?

## References

- **Specification**: `doc/tasks/unified-buffer/SPEC.md`
- **WezTerm source**: `term/src/screen.rs` — rewrap_lines() algorithm
- **Current implementation**: `src/terminal/buffer.ts` (ScreenBuffer), `src/terminal/state.ts`
