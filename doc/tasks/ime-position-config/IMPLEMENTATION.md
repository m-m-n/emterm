# Implementation Plan: IME Position Auto-Adjustment for TUI Applications

## Overview

Automatically position the IME input area and composition view at the bottom-left of the terminal area when the terminal cursor is hidden (TUI mode), instead of following the invisible cursor position.

## Objectives

- Auto-detect TUI application mode via cursor visibility state
- Position IME at bottom-left of terminal area when cursor is hidden
- Maintain current cursor-following behavior when cursor is visible
- No user-facing configuration needed (fully automatic)

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- Docker (for testing)

### Dependencies
- No new external dependencies required
- Existing `TerminalState.cursorVisible` property already available

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Framework**: Vanilla TypeScript (no framework)
- **Key Libraries**: None additional

### Design Approach

Minimal conditional branching in existing position calculation methods. The cursor visibility state (`cursorVisible`) from `TerminalState` determines which positioning strategy to use. No new classes, modules, or abstractions needed.

### Component Interaction

```
TerminalState (cursorVisible)
    |
    v
ImeHandler.updatePosition() / updateEditContextBounds() / updateCompositionView()
    |
    +-- cursorVisible === true  --> current cursor-following logic (unchanged)
    |
    +-- cursorVisible === false --> bottom-left of terminal area
```

## Implementation Phases

### Phase 1: IME Position Conditional Branching

**Goal**: Add cursor-visibility-aware positioning to all three IME position methods so that IME appears at bottom-left when cursor is hidden.

**Files to Modify**:
- `src/terminal-app/handlers/ime.ts` - Add conditional positioning logic to `updatePosition()`, `updateEditContextBounds()`, and `updateCompositionView()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `updatePosition()` | Position textarea for IME candidate window (WebKitGTK/textarea mode) | `imeInput` exists, `terminalState` available | Textarea positioned at cursor location (visible) or bottom-left (hidden) |
| `updateEditContextBounds()` | Set selection/character bounds for IME (WebView2/EditContext mode) | `editContext` exists, `terminalState` available | Selection bounds set at cursor location (visible) or bottom-left (hidden) |
| `updateCompositionView()` | Position composition text overlay | `compositionView` exists, text is non-empty | Composition view positioned at cursor location (visible) or bottom-left (hidden) |

**Processing Flow** (diagram-convertible):
1. Read `terminalState.cursorVisible`
   - `true` -> Use current cursor-based position calculation (no change)
   - `false` -> Calculate bottom-left position of terminal area
2. Apply calculated position to the relevant DOM element or EditContext API
3. (For `updateEditContextBounds` only) Character bounds also shift to bottom-left origin

**Position Calculation Contract**:

calculateImePosition(terminalState, container, charSize) -> { x, y }
  Precondition: terminalState is valid, container has non-zero dimensions
  Postcondition (cursorVisible === true): x, y correspond to cursor grid position in pixels
  Postcondition (cursorVisible === false): x = left padding, y = container height minus one character row height

**Implementation Steps**:
1. **Add visibility check to `updatePosition()`** - Read `cursorVisible` from terminal state; when false, compute bottom-left coordinates instead of cursor-based coordinates
2. **Add visibility check to `updateEditContextBounds()`** - Same conditional for EditContext selection bounds and character bounds positioning
3. **Add visibility check to `updateCompositionView()`** - Same conditional for composition overlay positioning
4. **Add unit tests for position calculation** - Verify bottom-left output when `cursorVisible === false`, verify unchanged cursor-following output when `cursorVisible === true`
5. **Verify no regression** - Run existing E2E test suite

**Dependencies**: None (self-contained change)

**Testing Approach**:
- Unit: Verify position calculation outputs correct coordinates for both cursor-visible and cursor-hidden modes
- Integration: Verify IME position updates when cursor visibility toggles
- E2E (Docker): Run existing E2E suite to confirm no regression
- Manual: Test with actual TUI application (Claude Code) to verify IME candidate window appears at bottom-left

**Acceptance Criteria**:
- [ ] When `cursorVisible === false`, IME textarea is positioned at bottom-left of terminal area
- [ ] When `cursorVisible === false`, EditContext selection bounds are at bottom-left
- [ ] When `cursorVisible === false`, composition view appears at bottom-left
- [ ] When `cursorVisible === true`, all positioning behavior is identical to current implementation
- [ ] Existing E2E tests pass without regression

**Estimated Effort**: small

---

## Complete File Structure

```
src/terminal-app/handlers/
  ime.ts                    # Modified: add cursorVisible conditional to 3 positioning methods
```

No new files created.

## Testing Strategy

- **Unit**: Position calculation for both modes (cursor-visible, cursor-hidden). Target 90%+ coverage for modified methods.
- **Integration**: Cursor visibility toggle during active composition
- **E2E (Docker)**: Existing test suite regression check via `./scripts/run-e2e-docker.sh`
- **Manual**: IME input in TUI application (Claude Code) on Linux (WebKitGTK) and Windows (WebView2)

## Dependencies

No new packages or dependencies required.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Composition position flicker on cursor visibility toggle | Low | Low | Position updates on next `updatePosition()` call; composition view moves smoothly |
| Different behavior between WebKitGTK and WebView2 | Low | Medium | Both code paths (textarea and EditContext) receive the same conditional logic |

## Open Questions

None. All requirements are clearly defined in SPEC.md.

## Success Metrics

- [ ] FR1-FR3 implemented and verified
- [ ] Cursor-visible mode behavior unchanged (NFR1)
- [ ] Works on Linux (WebKitGTK) and Windows (WebView2) (NFR2)
- [ ] No settings or configuration needed (NFR3)
- [ ] No regression in existing IME functionality
