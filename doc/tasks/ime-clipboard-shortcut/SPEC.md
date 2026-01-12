# Feature: IME Clipboard Shortcut Support

## Overview

Add capture phase event listeners for Ctrl+Shift+C (copy) and Ctrl+Shift+V (paste) shortcuts to ensure they work correctly when IME (Input Method Editor) is active. Currently, these shortcuts fail because the bubbling phase event handler receives events after IME/OS default processing has already consumed them.

## Objectives

- Enable Ctrl+Shift+C/V shortcuts to work while IME is active
- Maintain backward compatibility with existing keyboard handling
- Minimize changes to the existing codebase
- Ensure proper cleanup of event listeners

## User Stories

### US1: Copy with IME Active
As a Japanese/Chinese/Korean user, I want to copy selected terminal text using Ctrl+Shift+C while my IME is active, so that I can work efficiently without switching IME modes.

**Acceptance Criteria:**
- [ ] Ctrl+Shift+C copies selected text when IME is active
- [ ] Selection is cleared after successful copy
- [ ] Event is prevented from propagating to IME

### US2: Paste with IME Active
As a Japanese/Chinese/Korean user, I want to paste clipboard content using Ctrl+Shift+V while my IME is active, so that I can work efficiently without switching IME modes.

**Acceptance Criteria:**
- [ ] Ctrl+Shift+V pastes clipboard content when IME is active
- [ ] Multi-line paste confirmation dialog appears correctly
- [ ] Event is prevented from propagating to IME

### US3: Existing Behavior Preserved
As a user, I want all existing keyboard shortcuts to continue working as before, so that my workflow is not disrupted.

**Acceptance Criteria:**
- [ ] All existing shortcuts work when IME is inactive
- [ ] Other Ctrl+Shift combinations are unaffected
- [ ] Regular IME input is unaffected

## Technical Requirements

### Functional Requirements
- **FR1:** Add a new capture phase event listener specifically for Ctrl+Shift+C and Ctrl+Shift+V
- **FR2:** The capture listener must call `preventDefault()` and `stopPropagation()` for handled events
- **FR3:** The capture listener must be registered with `{ capture: true }` option
- **FR4:** The capture listener must be properly cleaned up in `detach()`
- **FR5:** The existing `handleKeyDown` method must remain unchanged

### Non-Functional Requirements
- **NFR1 - Performance:** Event handling latency must remain under 1ms
- **NFR2 - Compatibility:** Must work with major IMEs (Google Japanese Input, macOS native, Windows IME)
- **NFR3 - Maintainability:** New code must follow existing patterns in the codebase

## Implementation Approach

### Architecture

The solution adds a parallel event handling path that intercepts clipboard shortcuts before they can be consumed by IME:

```
Event Flow (Current):
┌──────────────────────────────────────────────────────────────┐
│ keydown event                                                 │
│   └─> Capture Phase (unused) ─> Target ─> Bubble Phase       │
│                                              └─> handleKeyDown│
│                                              (IME may block)  │
└──────────────────────────────────────────────────────────────┘

Event Flow (Proposed):
┌──────────────────────────────────────────────────────────────┐
│ keydown event                                                 │
│   └─> Capture Phase ─────────────> Target ─> Bubble Phase    │
│         └─> handleClipboardShortcut          └─> handleKeyDown│
│             (Ctrl+Shift+C/V only)                             │
│             preventDefault if matched                         │
└──────────────────────────────────────────────────────────────┘
```

### Component Changes

**KeyboardHandler Class Modifications:**

```typescript
// New private property
private boundHandleClipboardShortcut: ((e: KeyboardEvent) => void) | null = null;

// Modified attach() method
attach(target: EventTarget): void {
  this.target = target;

  // New: Capture phase listener for clipboard shortcuts
  this.boundHandleClipboardShortcut = (e: KeyboardEvent) => {
    this.handleClipboardShortcut(e);
  };
  target.addEventListener("keydown", this.boundHandleClipboardShortcut, { capture: true });

  // Existing: Bubble phase listener (unchanged)
  this.boundHandleKeyDown = (e: KeyboardEvent) => {
    this.handleKeyDown(e);
  };
  target.addEventListener("keydown", this.boundHandleKeyDown as EventListener);
}

// Modified detach() method
detach(): void {
  if (this.target) {
    // New: Remove capture listener
    if (this.boundHandleClipboardShortcut) {
      this.target.removeEventListener(
        "keydown",
        this.boundHandleClipboardShortcut,
        { capture: true }
      );
    }

    // Existing: Remove bubble listener (unchanged logic)
    if (this.boundHandleKeyDown) {
      this.target.removeEventListener(
        "keydown",
        this.boundHandleKeyDown as EventListener
      );
    }
  }
  this.boundHandleClipboardShortcut = null;
  this.boundHandleKeyDown = null;
  this.target = null;
}

// New private method
private handleClipboardShortcut(event: KeyboardEvent): void {
  // Only handle Ctrl+Shift+C/V
  if (!event.ctrlKey || !event.shiftKey) {
    return;
  }

  const key = event.key.toLowerCase();

  if (key === "c") {
    // CRITICAL: preventDefault/stopPropagation must be called synchronously
    // before any async operation to prevent IME from consuming the event
    event.preventDefault();
    event.stopPropagation();
    this.handleCopy(event);
    return;
  }

  if (key === "v") {
    // CRITICAL: preventDefault/stopPropagation must be called synchronously
    // before any async operation to prevent IME from consuming the event
    event.preventDefault();
    event.stopPropagation();
    this.handlePaste(event);
    return;
  }
}
```

### Data Flow

```
User presses Ctrl+Shift+C with IME active
    │
    ▼
Browser dispatches keydown event
    │
    ▼
Capture Phase starts (root → target)
    │
    ▼
handleClipboardShortcut() called
    │
    ├── Check: ctrlKey && shiftKey?
    │   └── No: return (let event continue)
    │
    ├── Check: key === "c"?
    │   └── Yes: preventDefault() → stopPropagation() → handleCopy()
    │           (CRITICAL: sync calls before async handler)
    │
    ├── Check: key === "v"?
    │   └── Yes: preventDefault() → stopPropagation() → handlePaste()
    │           (CRITICAL: sync calls before async handler)
    │
    └── Neither: return (let event continue)
```

### File Structure

No new files required. Changes are limited to:

```
src/terminal-app/handlers/keyboard.ts  # Modified
```

## Test Scenarios

### Unit Tests

Unit tests should be added to `src/terminal-app/handlers/keyboard.test.ts`:

- [ ] Test: `handleClipboardShortcut` ignores events without Ctrl key
- [ ] Test: `handleClipboardShortcut` ignores events without Shift key
- [ ] Test: `handleClipboardShortcut` calls handleCopy for Ctrl+Shift+C
- [ ] Test: `handleClipboardShortcut` calls handlePaste for Ctrl+Shift+V
- [ ] Test: `handleClipboardShortcut` ignores other Ctrl+Shift combinations
- [ ] Test: `attach()` registers capture phase listener
- [ ] Test: `detach()` removes capture phase listener
- [ ] Test: Multiple attach/detach cycles work correctly

### Integration Tests

- [ ] Test: Ctrl+Shift+C copies text with IME active (E2E)
- [ ] Test: Ctrl+Shift+V pastes text with IME active (E2E)
- [ ] Test: Regular IME input still works after changes

### Edge Cases

- [ ] Edge case: Rapid repeated Ctrl+Shift+C presses
- [ ] Edge case: Ctrl+Shift+C during IME composition
- [ ] Edge case: Empty selection with Ctrl+Shift+C
- [ ] Edge case: Empty clipboard with Ctrl+Shift+V
- [ ] Edge case: attach() called multiple times without detach()

## Security Considerations

- **Clipboard Access:** Uses existing secure clipboard handling via SelectionController
- **Event Propagation:** stopPropagation() is used safely and only for intended shortcuts
- **No New Attack Surface:** No new external inputs or data sources introduced

## Error Handling

### Error Scenarios

| Scenario | Handling | User Impact |
|----------|----------|-------------|
| Clipboard access denied | Error logged, event consumed | No visual feedback, shortcut appears to do nothing |
| No selection for copy | Event consumed, no error | Normal behavior |
| Empty clipboard for paste | Event consumed, no error | Normal behavior |

### Error Flow

```
Error occurs in handleCopy/handlePaste
    │
    ▼
Error caught by existing try/catch
    │
    ▼
Error logged via console.error
    │
    ▼
Event still prevented (no IME interference)
```

## Performance Optimization

### Performance Goals
- Event processing time: < 1ms (imperceptible)
- Memory overhead: ~8 bytes (one function reference)

### Optimization Notes
- Capture listener checks Ctrl+Shift early and returns immediately if not matched
- No additional DOM queries or expensive operations
- Reuses existing handleCopy/handlePaste methods

## Success Criteria

- [ ] All functional requirements implemented
- [ ] All unit tests pass
- [ ] TypeScript type check passes (`bun run typecheck`)
- [ ] Manual testing confirms IME + Ctrl+Shift+C/V works
- [ ] Manual testing confirms existing behavior unchanged
- [ ] Code review completed

## Open Questions

None - implementation approach has been decided.

## Implementation Notes

### Key Points
1. The capture listener must be registered BEFORE the bubble listener to ensure proper event flow
2. `stopPropagation()` is called after handling to prevent the bubble listener from processing the same event twice
3. The existing `handleKeyDown` Ctrl+Shift+C/V handling can remain as-is (serves as fallback and handles non-IME scenarios)

### Testing Methodology
Since this is an IME-related fix, manual testing should include:
1. Enable Japanese IME (or similar)
2. Start typing in hiragana mode
3. Select text in terminal
4. Press Ctrl+Shift+C
5. Verify copy succeeds (paste elsewhere to confirm)
6. Press Ctrl+Shift+V
7. Verify paste succeeds

### Browser Compatibility
The `{ capture: true }` option is supported in all modern browsers and WebView implementations used by Tauri.

## References

- MDN: EventTarget.addEventListener() - https://developer.mozilla.org/en-US/docs/Web/API/EventTarget/addEventListener
- DOM Events Capture/Bubble - https://javascript.info/bubbling-and-capturing
- Existing implementation: `/home/sakura/src/my_projects/tauri/emterm/src/terminal-app/handlers/keyboard.ts`
