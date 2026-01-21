# Feature: Word Selection Drag Extension

## Overview

Enable word-unit selection extension when dragging after a double-click. Currently, double-clicking selects a word but subsequent dragging does not extend the selection. This feature aligns eMterm with standard terminal emulator behavior.

## Objectives

- Allow word-unit selection extension when dragging after double-click
- Maintain existing selection functionality (single-click drag, triple-click line selection)
- Match behavior of common terminal emulators

## User Stories

### US1: Word Selection Drag
As a user, I want to extend word selection by dragging after double-click, so that I can efficiently select multiple words.

**Acceptance Criteria:**
- [ ] Double-click on a word selects the word
- [ ] Dragging after double-click extends selection word-by-word
- [ ] The anchor word (initially double-clicked) remains in selection
- [ ] Selection extends in both forward and backward directions

## Technical Requirements

### Functional Requirements
- **FR1:** Double-click must set `isSelecting: true` to enable drag tracking
- **FR2:** Mouse move after double-click must extend selection using word boundaries
- **FR3:** Anchor word must always be included in the selection range
- **FR4:** Selection range updates during drag must emit "update" events, not "start" events

### Non-Functional Requirements
- **NFR1 - Performance:** Mouse move event handling must complete within 16ms (60fps)
- **NFR2 - Compatibility:** Existing selection modes must remain functional

## Implementation Approach

### Root Cause Analysis

Current implementation in `SelectionController.onMouseDown()`:

```typescript
if (this.clickCount === 2) {
  mode = "word";
  const wordRange = this.wordBoundary.getWordAt(pos.col, pos.row);
  this.anchorWord = wordRange;
  this.model.setSelection(wordRange, mode);  // Problem: sets isSelecting=false
}
```

The `setSelection()` method in `SelectionModel` sets `isSelecting: false`:

```typescript
setSelection(range: SelectionRange, mode: SelectionMode = "char"): void {
  this.state = {
    range: { ...range },
    mode,
    isSelecting: false,  // This prevents drag tracking
  };
  // ...
}
```

As a result, `isActivelySelecting()` returns `false`, and `onMouseMove()` ignores drag events.

### Solution

Add a new method `startWordSelection()` to `SelectionModel` that:
1. Sets the initial word selection range
2. Sets `mode: "word"`
3. Sets `isSelecting: true` to enable drag tracking

Alternatively, modify `setSelection()` to accept an optional `isSelecting` parameter.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  SelectionController                         │
│                                                              │
│  onMouseDown() ──────────┬────────────────────────────────► │
│       │                  │                                   │
│       │ clickCount==2    │ clickCount==3                     │
│       ▼                  ▼                                   │
│  startSelection()    setSelection()                          │
│  (mode="word")       (mode="line")                           │
│  isSelecting=true    isSelecting=true (NEW)                  │
│       │                  │                                   │
│  onMouseMove() ◄─────────┴──────────────────────────────────│
│       │                                                      │
│       │ isActivelySelecting() == true                        │
│       ▼                                                      │
│  expandWordSelection() / expandLineSelection()               │
└─────────────────────────────────────────────────────────────┘
```

### Code Changes

#### File: `src/selection-v2/SelectionModel.ts`

Add new method or modify existing:

**Option A: New method (Recommended)**

```typescript
/**
 * Start a word or line selection that can be extended by dragging.
 *
 * Unlike setSelection(), this keeps isSelecting=true to allow drag extension.
 *
 * @param range - Initial selection range
 * @param mode - Selection mode (word or line)
 */
startSelection(pos: GridPosition, mode: SelectionMode = "char"): void {
  // Existing implementation already sets isSelecting: true
  // Use this for word selection instead of setSelection()
}
```

**Option B: Modify setSelection()**

```typescript
setSelection(
  range: SelectionRange,
  mode: SelectionMode = "char",
  isSelecting: boolean = false
): void {
  this.state = {
    range: { ...range },
    mode,
    isSelecting,
  };
  // ...
}
```

**Option C: Add updateSelectionRange() method (Recommended for FR4)**

```typescript
/**
 * Update selection range during drag without emitting "start" event.
 * Emits "update" event instead.
 *
 * @param range - New selection range
 */
updateSelectionRange(range: SelectionRange): void {
  this.state.range = { ...range };
  this.emit({
    type: "update",
    range: this.state.range,
    mode: this.state.mode,
  });
}
```

#### File: `src/selection-v2/SelectionController.ts`

Modify `onMouseDown()` for double-click handling:

```typescript
if (this.clickCount === 2) {
  mode = "word";
  const wordRange = this.wordBoundary.getWordAt(pos.col, pos.row);
  this.anchorWord = wordRange;
  // Use startSelection pattern to enable drag tracking
  this.model.startSelection(wordRange.start, mode);
  this.model.updateSelectionRange(wordRange);  // Set full word range
}
```

Or with Option B:

```typescript
if (this.clickCount === 2) {
  mode = "word";
  const wordRange = this.wordBoundary.getWordAt(pos.col, pos.row);
  this.anchorWord = wordRange;
  this.model.setSelection(wordRange, mode, true);  // isSelecting=true
}
```

### Recommended Implementation: Combined Approach

Combine Option A (add `isSelecting` parameter to `setSelection()`) and Option C (add `updateSelectionRange()` method) for clear separation of concerns:

- **setSelection()**: Set initial selection with optional drag tracking
- **updateSelectionRange()**: Update range during drag (emits "update" event)

#### SelectionModel.ts Changes

```typescript
/**
 * Set selection range with optional drag tracking.
 * @param range - Selection range
 * @param mode - Selection mode (char, word, line)
 * @param isSelecting - If true, enables drag tracking (default: false)
 */
setSelection(
  range: SelectionRange,
  mode: SelectionMode = "char",
  isSelecting: boolean = false
): void {
  this.state = {
    range: { ...range },
    mode,
    isSelecting,
  };

  this.emit({
    type: "start",
    range: this.state.range,
    mode: this.state.mode,
  });

  if (!isSelecting) {
    this.emit({
      type: "end",
      range: this.state.range,
      mode: this.state.mode,
    });
  }
}

/**
 * Update selection range during drag without emitting "start" event.
 * Emits "update" event instead.
 * @param range - New selection range
 */
updateSelectionRange(range: SelectionRange): void {
  this.state.range = { ...range };
  this.emit({
    type: "update",
    range: this.state.range,
    mode: this.state.mode,
  });
}
```

#### SelectionController.ts Changes

```typescript
// onMouseDown() - Use setSelection with isSelecting=true for initial selection
if (this.clickCount === 2) {
  mode = "word";
  const wordRange = this.wordBoundary.getWordAt(pos.col, pos.row);
  this.anchorWord = wordRange;
  this.model.setSelection(wordRange, mode, true);  // Enable drag tracking
} else if (this.clickCount >= 3) {
  mode = "line";
  this.anchorRow = pos.row;
  const lineRange = this.wordBoundary.getLineAt(pos.row);
  this.model.setSelection(lineRange, mode, true);  // Enable drag tracking
  this.clickCount = 3;
}

// onMouseMove() - Use updateSelectionRange for drag extension
if (this.model.isActivelySelecting()) {
  const newRange = this.calculateExpandedRange(pos);
  this.model.updateSelectionRange(newRange);  // Emits "update", not "start"
}
```

#### Method Usage Summary

| Action | Method | Event Emitted |
|--------|--------|---------------|
| Double-click (initial word select) | `setSelection(range, "word", true)` | "start" |
| Triple-click (initial line select) | `setSelection(range, "line", true)` | "start" |
| Drag to extend selection | `updateSelectionRange(range)` | "update" |
| Mouse up (end selection) | `endSelection()` | "end" |

### File Structure

```
src/selection-v2/
├── SelectionModel.ts       # Modify setSelection() signature
├── SelectionModel.test.ts  # Add tests for new behavior
├── SelectionController.ts  # Update double/triple-click handling
└── (other files unchanged)
```

## Test Scenarios

### Unit Tests

#### SelectionModel Tests
- [ ] `setSelection()` with `isSelecting=true` keeps selection active
- [ ] `setSelection()` with `isSelecting=false` (default) ends selection
- [ ] `isActivelySelecting()` returns correct value after `setSelection()`
- [ ] `updateSelectionRange()` emits "update" event, not "start" event
- [ ] `updateSelectionRange()` updates range without changing mode or isSelecting

#### SelectionController Tests
- [ ] Double-click sets `isSelecting=true`
- [ ] Double-click + drag extends word selection forward
- [ ] Double-click + drag extends word selection backward
- [ ] Double-click + drag across multiple lines
- [ ] Double-click without drag selects single word
- [ ] Mouse up after double-click+drag ends selection
- [ ] onMouseMove uses `updateSelectionRange()` instead of `setSelection()` for drag extension

### Integration Tests
- [ ] Complete flow: double-click, drag, release, verify selection text

### Regression Tests
- [ ] Single-click + drag still works for character selection
- [ ] Triple-click + drag still works for line selection
- [ ] Shift+click still works for extending selection

## Success Criteria

- [ ] Double-click + drag extends selection word-by-word
- [ ] Anchor word is always included in selection
- [ ] Selection extends in both directions (forward/backward)
- [ ] Triple-click + drag works for line selection (same fix applies)
- [ ] All existing tests pass
- [ ] New tests for word selection drag pass

## Open Questions

- None

## References

- Existing implementation: `src/selection-v2/SelectionController.ts`
- Selection model: `src/selection-v2/SelectionModel.ts`
- Word boundary detection: `src/selection-v2/WordBoundary.ts`
