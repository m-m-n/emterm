# Feature: Viewer Keyboard Scroll (Space / Shift+Space)

## Overview

Add Space and Shift+Space keyboard shortcuts to the Markdown fullscreen viewer and the Image viewer for page-based scrolling. This provides a familiar browser-like scrolling experience where Space scrolls down by approximately one viewport page and Shift+Space scrolls up.

## Objectives

- Enable page-based keyboard scrolling in both Markdown and Image viewers
- Match the familiar browser Space-scroll convention (~85% of viewport height)
- Maintain consistency with existing keyboard shortcut patterns in both viewers

## User Stories

### US1: Markdown Viewer Keyboard Scroll

As a user reading a long Markdown document in fullscreen, I want to press Space to scroll down by roughly one page and Shift+Space to scroll up, so that I can navigate the document quickly without reaching for PageUp/PageDown keys.

**Acceptance Criteria:**
- [ ] Space scrolls the content down by ~85% of the viewport height
- [ ] Shift+Space scrolls the content up by ~85% of the viewport height
- [ ] Scrolling uses smooth animation
- [ ] Keys are blocked from reaching the shell (existing behavior maintained)

### US2: Image Viewer Keyboard Scroll

As a user viewing a large image in pixel mode, I want to press Space to pan down and Shift+Space to pan up, so that I can navigate the image vertically using the keyboard.

**Acceptance Criteria:**
- [ ] Space pans the image down by ~85% of viewport height (pixel mode, image exceeds viewport)
- [ ] Shift+Space pans the image up by ~85% of viewport height (same conditions)
- [ ] In fit mode or when the image fits within the viewport, Space does nothing
- [ ] Keys are blocked from reaching the shell in all modes (existing behavior maintained)

## Technical Requirements

### Functional Requirements

- **FR1: Markdown viewer Space scroll** - In the Markdown fullscreen viewer, pressing Space scrolls down by `clientHeight * 0.85` with smooth behavior. Pressing Shift+Space scrolls up by the same amount.
- **FR2: Image viewer Space scroll** - In the Image viewer pixel mode, pressing Space pans down by `viewportHeight * 0.85`. Pressing Shift+Space pans up by the same amount. No action in fit mode or when the image fits within the viewport.

### Non-Functional Requirements

- **NFR1 - Consistency:** Scroll amount (85% of viewport) matches browser Space-scroll convention.
- **NFR2 - UX:** Smooth scrolling animation for Markdown viewer. Image viewer uses `PanController.setOffset()` (instant, matching existing wheel behavior).

## Implementation Approach

### Files to Modify

| File | Change |
|------|--------|
| `src/markdown/fullscreen.ts` | Add `case " ":` to `handleKeydown()` switch |
| `src/image-viewer/display-mode.ts` | Add `case " ":` to `handleKeydown()` switch; add scroll callback |
| `src/image-viewer/index.ts` | Wire scroll callback to `PanController` |

### Markdown Viewer (`src/markdown/fullscreen.ts`)

Add a new case in the `handleKeydown()` switch block (after the existing `PageDown` case, before `Home`):

```typescript
case " ":
  if (e.shiftKey) {
    this.scrollBy(-(this.content?.clientHeight || 400) * 0.85);
  } else {
    this.scrollBy((this.content?.clientHeight || 400) * 0.85);
  }
  break;
```

The existing `scrollBy()` method already uses `{ behavior: "smooth" }`.

### Image Viewer (`src/image-viewer/display-mode.ts` + `index.ts`)

The keyboard handler lives in `DisplayModeController` but the `PanController` is owned by `ImageViewer`. A callback is needed to bridge them.

**1. Add `onScroll` callback to `DisplayModeControllerOptions`:**

```typescript
export interface DisplayModeControllerOptions {
  // ... existing fields ...
  /** Callback for keyboard-initiated scroll (delta in pixels, positive = down) */
  onScroll?: (deltaY: number) => void;
}
```

**2. Add `case " ":` in `DisplayModeController.handleKeydown()`:**

```typescript
case " ": {
  e.preventDefault();
  e.stopPropagation();
  const viewportHeight = this.overlay?.clientHeight || 0;
  const delta = viewportHeight * 0.85;
  if (e.shiftKey) {
    this.onScroll?.(-delta);
  } else {
    this.onScroll?.(delta);
  }
  break;
}
```

**3. Wire up in `ImageViewer` (index.ts) when constructing `DisplayModeController`:**

```typescript
this.displayModeController = new DisplayModeController({
  // ... existing options ...
  onScroll: (deltaY) => {
    if (!this.panController?.canPan()) return;
    const offset = this.panController.getOffset();
    this.panController.setOffset(offset.x, offset.y - deltaY);
  },
});
```

Note: `setOffset` subtracts deltaY because positive pan offset moves the image up (same convention as the existing `handleWheel`).

When `canPan()` returns false (fit mode, or image fits within viewport), the callback does nothing.

## Test Scenarios

### Unit Tests

- [ ] Markdown viewer: Space key triggers `scrollBy()` with positive delta (~85% of clientHeight)
- [ ] Markdown viewer: Shift+Space triggers `scrollBy()` with negative delta
- [ ] Image viewer: Space key calls `onScroll` with positive delta
- [ ] Image viewer: Shift+Space calls `onScroll` with negative delta
- [ ] Image viewer: `onScroll` is not called when `canPan()` returns false

### Edge Cases

- [ ] Space key does not leak to shell when viewer is active (already handled by `e.preventDefault()`)
- [ ] Space key in Markdown viewer when link dialog is shown (blocked by existing dialog check)
- [ ] Space key in Image viewer when overlay is hidden or ancestor is hidden (blocked by existing visibility checks)
- [ ] Scroll at content boundary (top/bottom) - default `scrollBy` / `setOffset` clamping behavior applies

### E2E Tests

**Existing E2E tests**: `e2e-tests/` (Docker-based with tauri-driver + WebdriverIO)
**Run command**: `./scripts/run-e2e-docker.sh`

- [ ] Existing E2E tests pass without regression

## Success Criteria

- [ ] All functional requirements (FR1, FR2) are implemented
- [ ] All unit tests pass
- [ ] Existing E2E tests pass without regression
- [ ] Space/Shift+Space keys do not leak to the shell in any viewer state
