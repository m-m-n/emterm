# Feature: IME Position Auto-Adjustment for TUI Applications

## Overview

When the terminal cursor is hidden (as TUI applications like Claude Code do), automatically position the IME input area and composition view at the bottom-left of the terminal area instead of following the (invisible) cursor position. This ensures IME candidate windows appear at a predictable, visible location during TUI application usage.

## Objectives

- Auto-detect TUI application mode via cursor visibility (`cursorVisible === false`)
- Position IME at bottom-left of terminal area when cursor is hidden
- Maintain current cursor-following behavior when cursor is visible
- No user-facing configuration needed (fully automatic)

## User Stories

### US1: IME Input in TUI Applications
As a user running Claude Code (or other TUI applications), I want the IME candidate window to appear at a predictable position (bottom-left of terminal), so that I can see and interact with the composition text regardless of the internal cursor position.

**Acceptance Criteria:**
- [ ] When cursor is hidden, IME input area is positioned at bottom-left of terminal area
- [ ] When cursor is hidden, composition view appears at bottom-left of terminal area
- [ ] IME candidate window appears near the bottom-left (determined by OS/IME framework)
- [ ] Text input and composition work correctly in this mode

### US2: IME Input in Standard Shell (No Regression)
As a user running a standard shell (bash, zsh), I want the IME to follow the cursor position as it does today.

**Acceptance Criteria:**
- [ ] When cursor is visible, behavior is identical to current implementation
- [ ] No regression in cursor-following IME positioning

## Technical Requirements

### Functional Requirements
- **FR1: Auto-detect cursor visibility** - Read `terminalState.cursorVisible` in `updatePosition()` and `updateEditContextBounds()` to determine positioning mode
- **FR2: Bottom-left positioning** - When `cursorVisible === false`, position IME input area at bottom-left of terminal canvas area
- **FR3: Composition view positioning** - When `cursorVisible === false`, position composition view at bottom-left of terminal canvas area

### Non-Functional Requirements
- **NFR1 - Backward Compatibility:** When `cursorVisible === true`, behavior is identical to current implementation
- **NFR2 - Cross-Platform:** Works on both Linux (WebKitGTK textarea) and Windows (WebView2 EditContext)
- **NFR3 - Zero Configuration:** No settings or user intervention required

## Implementation Approach

### Architecture

The change is minimal — a conditional branch in the existing position calculation functions.

```
updatePosition() / updateEditContextBounds()
    │
    ├── cursorVisible === true  → current logic (cursor position)
    │
    └── cursorVisible === false → bottom-left of terminal area
```

### Affected Files

```
src/terminal-app/handlers/ime.ts    # Position calculation (updatePosition, updateEditContextBounds, updateCompositionView)
```

No settings changes, no Rust changes, no i18n changes needed.

### Position Calculation

**Current (cursor-following, when `cursorVisible === true`):**
```typescript
const x = cursorCol * charSize.width + paddingLeft;
const y = cursorRow * charSize.height + paddingTop - scrollOffset;
```

**New (bottom-left fixed, when `cursorVisible === false`):**
```typescript
// Position at bottom-left of terminal area
const x = paddingLeft;
const y = rect.height - charSize.height;
```

For textarea mode:
```typescript
this.imeInput.style.left = `${rect.left + x}px`;
this.imeInput.style.top = `${rect.top + y}px`;
```

For EditContext mode:
```typescript
this.editContext.updateSelectionBounds(
    new DOMRect(rect.left + x, rect.top + y, this.charSize.width, this.charSize.height)
);
```

For composition view:
```typescript
this.compositionView.style.left = `${rect.left + x}px`;
this.compositionView.style.top = `${rect.top + y}px`;
```

### Edge Cases

1. **Cursor visibility toggles during composition** — If cursor becomes visible/hidden mid-composition, the IME position should update on next `updatePosition()` call. The composition view should also move.
2. **Window resize** — `updatePosition()` is called on resize, so bottom-left position recalculates automatically.
3. **Tab switching** — Each tab has its own terminal state with independent cursor visibility.

## Test Scenarios

### Unit Tests
- [ ] When `cursorVisible === false`, position calculation produces bottom-left coordinates
- [ ] When `cursorVisible === true`, position calculation produces cursor-following coordinates (current behavior)

### Integration Tests
- [ ] IME position updates correctly when cursor visibility changes

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (30+ specs)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression

## Success Criteria

- [ ] FR1-FR3 implemented
- [ ] Cursor-visible mode behavior unchanged
- [ ] Works on Linux (WebKitGTK) and Windows (WebView2)
- [ ] No regression in existing IME functionality

## References

- Debug log confirming cursor state: `cursor=(0,51) visible=false` during Claude Code usage
- Current IME implementation: `src/terminal-app/handlers/ime.ts`
- Cursor visibility mode: DECTCEM (DEC mode 25) handled in `wasm/src/csi_modes.rs`
