# Implementation Plan: Word Selection Drag Extension

## Overview

Enable word-unit selection extension when dragging after a double-click. Modify `setSelection()` to accept an optional `isSelecting` parameter, allowing double-click and triple-click to initiate draggable selections.

## Objectives

- Allow word-unit selection extension when dragging after double-click
- Allow line-unit selection extension when dragging after triple-click
- Maintain backward compatibility with existing code

## Prerequisites

### Development Environment
- Bun (package manager and test runner)
- TypeScript type checking

### Dependencies
- None (uses existing selection-v2 infrastructure)

### Knowledge Requirements
- Selection model state management
- Mouse event handling flow

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Framework**: Vanilla JS (no framework)
- **Test**: Bun test

### Design Approach

Add optional `isSelecting` parameter to `setSelection()` method:
- Default `false` maintains backward compatibility
- When `true`, enables drag tracking after selection is set

### Component Interaction

```
onMouseDown (clickCount=2)
    │
    ▼
setSelection(wordRange, "word", true)
    │
    ├─ isSelecting: true (NEW)
    │
    ▼
onMouseMove
    │
    ├─ isActivelySelecting() → true
    │
    ▼
expandWordSelection()
```

## Implementation Phases

### Phase 1: Modify SelectionModel

**Goal**: Add `isSelecting` parameter to `setSelection()` method

**Files to Modify**:
- `src/selection-v2/SelectionModel.ts`:
  - Add optional third parameter to `setSelection()`
  - Conditionally emit "end" event based on `isSelecting`
  - Add `updateSelectionRange()` method for drag extension (emits "update" event)
- `src/selection-v2/SelectionModel.test.ts`:
  - Add tests for new behavior

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| setSelection() | Set selection range with optional drag tracking | Valid range and mode | State updated, events emitted |
| updateSelectionRange() | Update selection range during drag | isSelecting=true | Range updated, "update" event emitted |

**Processing Flow**:
```
1. Receive range, mode, and isSelecting parameters
2. Update internal state with provided values
3. Emit "start" event
4. If isSelecting is false
   └─ Emit "end" event
```

**Implementation Steps**:

1. **Modify setSelection() Signature**
   - Add optional `isSelecting: boolean = false` parameter
   - Update state assignment to use this value
   - Conditionally emit "end" event

2. **Add updateSelectionRange() Method**
   - Accept `range: SelectionRange` parameter
   - Update `this.state.range` only (preserve mode and isSelecting)
   - Emit "update" event (not "start")

3. **Add Unit Tests**
   - Test `setSelection()` with `isSelecting=true`
   - Test `setSelection()` with `isSelecting=false` (default)
   - Verify `isActivelySelecting()` returns correct value
   - Test `updateSelectionRange()` emits "update" event
   - Test `updateSelectionRange()` preserves mode and isSelecting

**Dependencies**:
- Requires: None
- Blocks: Phase 2

**Testing Approach**:

*Unit Tests*:
- `setSelection()` with `isSelecting=true` keeps selection active
- `setSelection()` with `isSelecting=false` ends selection
- Default behavior unchanged
- `updateSelectionRange()` emits "update" event
- `updateSelectionRange()` preserves mode and isSelecting state

**Acceptance Criteria**:
- [ ] `setSelection(range, mode, true)` sets `isSelecting=true`
- [ ] `setSelection(range, mode)` sets `isSelecting=false` (backward compatible)
- [ ] `updateSelectionRange(range)` emits "update" event, not "start" event
- [ ] All existing tests pass

**Estimated Effort**: 小 (1-2 hours)

---

### Phase 2: Update SelectionController

**Goal**: Use new `setSelection()` parameter for double-click and triple-click

**Files to Modify**:
- `src/selection-v2/SelectionController.ts`:
  - Update double-click handling
  - Update triple-click handling

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| onMouseDown() | Handle click events and initiate selection | Mouse event received | Selection started with drag enabled |
| onMouseMove() | Extend selection during drag | isSelecting=true | Selection expanded |

**Processing Flow**:
```
1. Detect click count (double or triple)
2. For double-click
   ├─ Calculate word range
   └─ Call setSelection(wordRange, "word", true)
3. For triple-click
   ├─ Calculate line range
   └─ Call setSelection(lineRange, "line", true)
4. onMouseMove detects isActivelySelecting()=true
   └─ Expands selection appropriately
```

**Implementation Steps**:

1. **Update Double-Click Handling**
   - Change `setSelection(wordRange, mode)` to `setSelection(wordRange, mode, true)`

2. **Update Triple-Click Handling**
   - Change `setSelection(lineRange, mode)` to `setSelection(lineRange, mode, true)`

3. **Modify onMouseMove() for Mode-Aware Expansion**
   - Check current selection mode via `model.mode`
   - For "word" mode: call `expandWordSelection(pos)`
   - For "line" mode: call `expandLineSelection(pos)`
   - For "char" mode: keep existing behavior
   - Use `updateSelectionRange()` instead of `setSelection()` for range updates during drag

**Dependencies**:
- Requires: Phase 1
- Blocks: None

**Testing Approach**:

*Manual Tests*:
- Double-click on word, verify selection
- Double-click and drag forward, verify word expansion
- Double-click and drag backward, verify word expansion
- Triple-click and drag, verify line expansion

**Acceptance Criteria**:
- [ ] Double-click + drag extends selection word-by-word
- [ ] Triple-click + drag extends selection line-by-line
- [ ] Anchor word/row always included in selection
- [ ] Single-click + drag still works for character selection
- [ ] onMouseMove uses `updateSelectionRange()` for drag extension (not `setSelection()`)

**Estimated Effort**: 小 (30 minutes)

---

## Complete File Structure

```
src/selection-v2/
├── SelectionModel.ts       # Modify setSelection() signature
├── SelectionModel.test.ts  # Add tests for new behavior
├── SelectionController.ts  # Update double/triple-click handling
└── (other files unchanged)
```

**File Descriptions**:
- `SelectionModel.ts`: Core selection state management
- `SelectionModel.test.ts`: Unit tests for model behavior
- `SelectionController.ts`: Mouse event handling and coordination

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test framework
- Test state changes in SelectionModel

**Test Coverage Goals**:
- SelectionModel: 100% for modified code paths

**Key Test Areas**:
1. **SelectionModel** (`src/selection-v2/SelectionModel.ts`)
   - `setSelection()` with `isSelecting=true`
   - `setSelection()` with `isSelecting=false`
   - Default behavior preservation
   - `updateSelectionRange()` emits "update" event
   - `updateSelectionRange()` preserves mode and isSelecting state

### Integration Testing

**Scenarios**:
- Double-click sets `isSelecting=true`
- Mouse move after double-click extends selection using `updateSelectionRange()`
- Mouse up ends selection

### Manual Testing Checklist

Based on spec test scenarios:
- [ ] Double-click on a word selects the word
- [ ] Dragging after double-click extends selection word-by-word
- [ ] The anchor word remains in selection
- [ ] Selection extends in both forward and backward directions
- [ ] Triple-click + drag extends selection line-by-line
- [ ] Single-click + drag still works for character selection
- [ ] Shift+click still works for extending selection

## Dependencies

### External Dependencies

None

### Internal Dependencies

**Implementation Order**:
1. Phase 1: Modify SelectionModel
2. Phase 2: Update SelectionController

**Component Dependencies**:
- `SelectionController.ts` depends on `SelectionModel.ts`

## Risk Assessment

### Technical Risks

1. **Backward Compatibility**
   - **Risk**: Existing code calling `setSelection()` may break
   - **Likelihood**: Low (parameter has default value)
   - **Impact**: High
   - **Mitigation**: Use default parameter `isSelecting=false`

### Implementation Risks

None identified - this is a minimal, well-scoped change.

## Performance Considerations

None - no performance-sensitive changes.

## Security Considerations

None - no security-sensitive changes.

## Open Questions

None - specification is complete and unambiguous.

## Future Enhancements

None planned.

## Success Metrics

### Functional Completeness
- [ ] Double-click + drag extends selection word-by-word
- [ ] Triple-click + drag extends selection line-by-line
- [ ] All existing tests pass
- [ ] New tests for word selection drag pass

### Quality Metrics
- [ ] No regression in existing selection behavior

## References

- **Specification**: `doc/tasks/word-selection-drag/SPEC.md`
- **Existing Implementation**: `src/selection-v2/SelectionController.ts`
- **Selection Model**: `src/selection-v2/SelectionModel.ts`

## Next Steps

1. **Begin Implementation**
   - Start with Phase 1 (SelectionModel modification)
   - Write tests first (TDD approach)
   - Proceed to Phase 2 after Phase 1 is verified

2. **Manual Testing**
   - Test in running application
   - Verify all selection modes work correctly
