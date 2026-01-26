# Feature: Viewer Rendering Area Change

## Overview

Modify the viewer components (ImageViewer and FullscreenMarkdownView) to render within the terminal content area (`#tab-content-area`) instead of covering the entire screen. This ensures the tab bar remains accessible during viewer display, enabling tab switching while viewing images or Markdown content.

## Objectives

- Change viewer overlay positioning from `document.body` to `#tab-content-area`
- Maintain independent viewer state per tab
- Keep tab bar always accessible during viewer display
- Preserve existing close behaviors (Escape key, click outside)

## User Stories

### US1: Tab Switching During Viewer Display
As a terminal user, I want to switch tabs while viewing an image or Markdown document, so that I can work with multiple terminal sessions without closing the viewer.

**Acceptance Criteria:**
- [ ] Tab bar is visible and clickable when viewer is displayed
- [ ] Keyboard shortcuts for tab switching work during viewer display
- [ ] Clicking a tab switches to that tab without closing the current viewer

### US2: Independent Viewer State Per Tab
As a terminal user, I want each tab to maintain its own viewer state, so that I can have different images or documents open in different tabs.

**Acceptance Criteria:**
- [ ] Opening a viewer in Tab A does not affect Tab B
- [ ] Switching from Tab A (with viewer) to Tab B and back preserves Tab A's viewer
- [ ] Closing a tab properly disposes its viewer resources

## Technical Requirements

### Functional Requirements
- **FR1:** Viewer overlay must render within `#tab-content-area` bounds
- **FR2:** Tab bar (32px height) must remain visible and interactive
- **FR3:** Each tab must maintain independent viewer state
- **FR4:** Existing close behaviors (Escape, click outside) must be preserved
- **FR5:** Existing zoom/pan functionality must work correctly

### Non-Functional Requirements
- **NFR1 - Performance:** Tab switch latency < 100ms
- **NFR2 - Animation:** Viewer show/hide transition < 150ms

## Implementation Approach

### Architecture

**Current Architecture (Before):**
```
document.body
├── #app
│   ├── #tab-bar
│   └── #tab-content-area
│       └── .tab-content (per tab)
└── .image-viewer-overlay (position: fixed, covers entire viewport)
└── .markdown-fullscreen-overlay (position: fixed, covers entire viewport)
```

**New Architecture (After):**
```
document.body
└── #app
    ├── #tab-bar (always visible, z-index above content)
    └── #tab-content-area (position: relative)
        └── .tab-content (per tab)
            ├── .terminal-root (terminal content container)
            │   └── #terminal
            └── .overlay-root (viewer overlay container)
                └── .viewer-overlay (position: absolute)
```

### Container Separation Design

To prevent viewer operations from destroying terminal content, each tab uses a two-container structure:

**Structure:**
- `.terminal-root`: Contains terminal canvas and related elements
- `.overlay-root`: Contains viewer overlays (ImageViewer, MarkdownViewer)

**Benefits:**
- `forceRender()` and similar viewer operations only affect `.overlay-root`
- Terminal content in `.terminal-root` is never affected by viewer lifecycle
- Clear separation of concerns between terminal rendering and overlay management

**CSS Requirements:**
```css
.tab-content {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
}

.terminal-root {
  flex: 1;
  min-height: 0;
  position: relative;
}

.overlay-root {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  pointer-events: none; /* Allow clicks through when no overlay */
}

.overlay-root > * {
  pointer-events: auto; /* Re-enable for actual overlays */
}
```

### Component Changes

#### ImageViewer (`src/image-viewer/index.ts`)

**Current:**
```typescript
constructor(container: HTMLElement) {
  // ...
  document.body.appendChild(this.overlay);
}
```

**New:**
```typescript
constructor(container: HTMLElement) {
  // container is the tab's content element
  this.container = container;
  // ...
  this.container.appendChild(this.overlay);
}
```

**CSS Changes:**
```css
/* Current */
.image-viewer-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 10000;
}

/* New */
.image-viewer-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 1000; /* Lower z-index, scoped to tab-content-area */
}
```

#### FullscreenMarkdownView (`src/markdown/fullscreen.ts`)

**Current:**
```typescript
show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void {
  // ...
  document.body.appendChild(this.overlay);
}
```

**New:**
```typescript
show(block: MarkdownBlock, container: HTMLElement, config?: Partial<FullscreenConfig>): void {
  // container is the tab's content element
  this.container = container;
  // ...
  this.container.appendChild(this.overlay);
}
```

**CSS Changes:**
```css
/* Current */
.markdown-fullscreen-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 10000;
}

/* New */
.markdown-fullscreen-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 1000; /* Lower z-index, scoped to tab-content-area */
}
```

### Tab State Management

Each terminal tab maintains its own viewer instances:

```typescript
interface TabState {
  id: string;
  terminal: Terminal;
  imageViewer: ImageViewer | null;
  markdownView: FullscreenMarkdownView | null;
}
```

**Tab Switch Behavior:**
1. When switching away from a tab with active viewer:
   - Viewer remains in DOM but tab-content becomes hidden
   - No explicit state save needed (DOM state preserved)

2. When switching to a tab with active viewer:
   - Tab-content becomes visible
   - Viewer appears with its previous state

3. When closing a tab:
   - Call `viewer.dispose()` to clean up resources

### Viewport Calculation Updates

Both viewers calculate fit/scale based on viewport dimensions. These calculations must use `#tab-content-area` dimensions instead of window dimensions.

**ImageViewer:**
```typescript
// Current
viewportWidth: this.overlay.clientWidth,
viewportHeight: this.overlay.clientHeight,

// New (no change needed - overlay is inside tab-content-area)
// overlay.clientWidth/Height will automatically reflect tab-content-area size
```

**FullscreenMarkdownView:**
- Similar - overlay dimensions automatically reflect container size

### Event Handler Considerations

**Keyboard Events:**
- Currently use `document.addEventListener` with capture phase
- No change needed - events still bubble up from overlay
- Tab bar shortcuts should work as overlay doesn't cover tab bar

**Click Events:**
- Close-on-click-outside behavior needs adjustment
- Click target must be the overlay itself (not content within)
- No change to logic, just positioning

### File Structure

No new files required. Changes to existing files:

```
src/
├── image-viewer/
│   ├── index.ts              # Constructor change, CSS update
│   └── index.test.ts         # Update tests for container-based rendering
├── markdown/
│   ├── fullscreen.ts         # show() signature change, CSS update
│   └── fullscreen.test.ts    # Update tests
└── styles.css                # CSS position changes (if not in JS)
```

## Test Scenarios

### Unit Tests

#### ImageViewer
- [ ] Overlay appends to container (not document.body)
- [ ] Overlay uses position: absolute
- [ ] Overlay dimensions match container dimensions
- [ ] dispose() removes overlay from container

#### FullscreenMarkdownView
- [ ] Overlay appends to container (not document.body)
- [ ] Overlay uses position: absolute
- [ ] Container parameter is required in show()
- [ ] dispose() removes overlay from container

### Integration Tests
- [ ] Tab bar visible when ImageViewer is displayed
- [ ] Tab bar visible when MarkdownView is displayed
- [ ] Tab click switches tabs during viewer display
- [ ] Ctrl+Tab works during viewer display

### E2E Tests
- [ ] Open image viewer, verify tab bar is clickable
- [ ] Open markdown viewer, verify tab bar is clickable
- [ ] Switch tabs with viewer open, switch back, verify state preserved
- [ ] Close tab with viewer open, verify no memory leak

### Edge Cases
- [ ] Window resize with viewer open - viewer resizes correctly
- [ ] Very small window - viewer still functional
- [ ] Rapid tab switching with viewers - no visual glitches

## Security Considerations

- No changes to security model
- Existing XSS protections in Markdown rendering remain unchanged
- Link confirmation dialog behavior unchanged

## Error Handling

No new error conditions introduced. Existing error handling remains:

| Scenario | Handling |
|----------|----------|
| Container not found | Throw error in constructor |
| Image decode failure | Show error state in canvas |
| Invalid zoom level | Clamp to valid range |

## Performance Optimization

### Unchanged Optimizations
- ImageBitmap for efficient image rendering
- CSS transform for zoom (GPU accelerated)
- Resize throttling (100ms)

### Tab Switch Performance
- No additional optimization needed
- Tab content visibility toggle via CSS (display: none)
- Viewer DOM preserved, no re-render on switch

## Success Criteria

- [ ] Tab bar always visible and interactive during viewer display
- [ ] Each tab maintains independent viewer state
- [ ] Existing close behaviors work (Escape, click outside)
- [ ] Existing zoom/pan functionality works
- [ ] No visual regression in viewer appearance
- [ ] All existing tests pass
- [ ] New tests for container-based rendering pass
- [ ] Tab switch latency < 100ms

## Implementation Phases

### Phase 1: ImageViewer Changes
**Goals:** Update ImageViewer to render within container
**Deliverables:**
- Modified constructor to accept and use container
- Updated CSS positioning
- Updated tests

### Phase 2: FullscreenMarkdownView Changes
**Goals:** Update FullscreenMarkdownView to render within container
**Deliverables:**
- Modified show() signature to accept container
- Updated CSS positioning
- Updated tests

### Phase 3: Integration Testing
**Goals:** Verify tab-level integration
**Deliverables:**
- E2E tests for tab switching with viewers
- Visual verification of tab bar accessibility

## References

- Existing ImageViewer: `src/image-viewer/index.ts`
- Existing FullscreenMarkdownView: `src/markdown/fullscreen.ts`
- HTML Structure: `src/index.html`
- Tab Bar Styles: `src/styles/tab-bar.css`
- Test Guidelines: `test/README.md`
