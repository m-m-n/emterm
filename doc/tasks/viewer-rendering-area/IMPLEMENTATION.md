# Implementation Plan: Viewer Rendering Area Change

## Overview

Modify ImageViewer and FullscreenMarkdownView to render within the terminal content area (`#tab-content-area`) instead of covering the entire window. This ensures the tab bar remains accessible during viewer display, enabling tab switching while viewing images or Markdown content.

## Objectives

- Change viewer overlay positioning from `document.body` to tab content containers
- Maintain independent viewer state per tab
- Keep tab bar always accessible during viewer display
- Preserve existing close behaviors (Escape key, click outside)
- Preserve existing zoom/pan functionality

## Prerequisites

### Development Environment
- Node.js/Bun runtime
- Tauri development environment configured

### Dependencies
- No new external dependencies required
- Existing dependencies: Tauri plugins (shell, clipboard)

### Knowledge Requirements
- Understanding of CSS positioning (fixed vs absolute)
- DOM manipulation and event handling
- Existing viewer component architecture

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Framework**: Tauri (WebView frontend)
- **Key Libraries**:
  - @tauri-apps/plugin-shell - External link handling
  - @tauri-apps/plugin-clipboard-manager - Copy functionality

### Design Approach

The key architectural change is moving viewer overlays from `document.body` (with `position: fixed`) to individual tab content containers (with `position: absolute`). This creates a natural scoping where each tab can independently manage its viewer state.

**Current Architecture:**
```
document.body
├── #app
│   ├── #tab-bar (32px height)
│   └── #tab-content-area
│       └── .tab-content (per tab)
└── .viewer-overlay (position: fixed, covers entire viewport)
```

**New Architecture:**
```
document.body
└── #app
    ├── #tab-bar (32px height, z-index above content)
    └── #tab-content-area (position: relative)
        └── .tab-content (per tab)
            ├── .terminal-root (terminal content container)
            │   └── #terminal
            └── .overlay-root (viewer overlay container)
                └── .viewer-overlay (position: absolute)
```

### Container Separation Design

Each tab uses a two-container structure to isolate terminal content from viewer operations:

| Container | Purpose | Content |
|-----------|---------|---------|
| `.terminal-root` | Terminal rendering | Canvas, cursor, selection |
| `.overlay-root` | Viewer overlays | ImageViewer, MarkdownViewer |

**Rationale:** Prevents `forceRender()` and DOM manipulation in viewer code from accidentally destroying terminal content.

### Component Interaction

| Component | Responsibility | Interaction |
|-----------|----------------|-------------|
| ImageViewer | Display images with zoom/pan | Receives container from caller |
| FullscreenMarkdownView | Display Markdown documents | Receives container from caller |
| TabManager | Manage tab lifecycle | Provides container reference |
| Tab Content Container | Scope viewer rendering | Parent element for viewer overlays |

## Implementation Phases

### Phase 1: ImageViewer Container-Based Rendering

**Goal**: Update ImageViewer to render within a provided container element instead of document.body

**Files to Modify**:
- `src/image-viewer/index.ts` - Constructor and rendering logic
- `src/styles.css` - CSS positioning changes
- `src/image-viewer/index.test.ts` - Update tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ImageViewer.constructor | Accept and store container reference | Valid HTMLElement passed | Overlay appended to container |
| CSS .image-viewer-overlay | Position overlay within container | Container has position: relative | Overlay covers container bounds |

**Processing Flow**:
```
1. Constructor receives container parameter
2. Create overlay element with position: absolute
3. Append overlay to container (not document.body)
4. Viewport calculations use overlay dimensions (unchanged behavior)
5. Dispose removes overlay from container
```

**Implementation Steps**:

1. **Update CSS positioning**
   - Change `.image-viewer-overlay` from `position: fixed` to `position: absolute`
   - Adjust z-index from 100000 to 1000 (scoped to container)
   - Remove viewport-level positioning (top: 0, left: 0 now relative to container)

2. **Modify constructor behavior**
   - Store container reference for use
   - Append overlay to container instead of document.body
   - Container parameter now serves as actual parent element

3. **Update viewport calculations**
   - Existing calculations use overlay.clientWidth/Height
   - No changes needed as overlay dimensions reflect container size

4. **Update dispose method**
   - Overlay removal already uses this.overlay.remove()
   - **Event handler cleanup**: Explicitly remove all event listeners before DOM removal
     - Remove keydown handlers from document
     - Remove resize handlers from window
     - Remove wheel/touch handlers from overlay elements
   - Clear any references to prevent memory leaks

**Dependencies**:
- Requires: None (can be implemented independently)
- Blocks: Phase 3 (integration)

**Testing Approach**:

*Unit Tests*:
- Verify overlay appends to container parameter
- Verify CSS position is absolute
- Verify dispose removes overlay from container

*Manual Testing*:
- [ ] Image viewer displays within tab content area
- [ ] Tab bar remains visible during viewer display
- [ ] Zoom/pan functionality works correctly
- [ ] Escape key closes viewer
- [ ] Click outside content closes viewer

**Acceptance Criteria**:
- [ ] Overlay uses `position: absolute`
- [ ] Overlay is child of container element
- [ ] Tab bar visible and clickable during viewer display
- [ ] All existing viewer functionality preserved

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Viewport calculation may behave differently
  - **Mitigation**: Existing code uses overlay dimensions which adapt automatically

---

### Phase 2: FullscreenMarkdownView Container-Based Rendering

**Goal**: Update FullscreenMarkdownView to render within a provided container element

**Files to Modify**:
- `src/markdown/fullscreen.ts` - show() method signature and rendering
- `src/styles.css` - CSS positioning changes
- `src/markdown/fullscreen.test.ts` - Update tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FullscreenMarkdownView.show | Accept container parameter | Valid HTMLElement and MarkdownBlock | Overlay appended to container |
| CSS .markdown-fullscreen-overlay | Position overlay within container | Container has position: relative | Overlay covers container bounds |

**Processing Flow**:
```
1. show() receives container parameter
2. Store container reference
3. Create overlay element with position: absolute
4. Append overlay to container
5. close() removes overlay from container
```

**Implementation Steps**:

1. **Update CSS positioning**
   - Change `.markdown-fullscreen-overlay` from `position: fixed` to `position: absolute`
   - Remove `height: 100vh` (use height: 100% instead)
   - Adjust z-index from 9999 to 1000

2. **Modify show() method signature**
   - Add container parameter
   - Store container reference
   - Append overlay to container instead of document.body

3. **Update close() method**
   - **Event handler cleanup**: Explicitly remove all event listeners before DOM removal
     - Remove keydown handlers from document
     - Remove wheel handlers from overlay elements
   - Overlay removal logic unchanged
   - Container reference cleared on close

4. **Update link dialog positioning**
   - Change LinkConfirmDialog from `position: fixed` to `position: absolute`
   - Append dialog to same container as markdown overlay (not document.body)
   - Use z-index: 2000 (above overlay's z-index: 1000)
   - Center dialog within container using transform: translate(-50%, -50%)
   - This ensures dialog scrolls with container and maintains proper stacking

**Dependencies**:
- Requires: None (can be implemented in parallel with Phase 1)
- Blocks: Phase 3 (integration)

**Testing Approach**:

*Unit Tests*:
- Verify show() accepts container parameter
- Verify overlay appends to container
- Verify CSS position is absolute
- Verify close() removes overlay

*Manual Testing*:
- [ ] Markdown viewer displays within tab content area
- [ ] Tab bar remains visible during viewer display
- [ ] Scroll functionality works correctly
- [ ] Zoom functionality works correctly
- [ ] Escape key closes viewer
- [ ] Link confirmation dialog displays correctly

**Acceptance Criteria**:
- [ ] show() method accepts container parameter
- [ ] Overlay uses `position: absolute`
- [ ] Overlay is child of container element
- [ ] Tab bar visible and clickable during viewer display
- [ ] All existing viewer functionality preserved

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Link dialog may render behind overlay
  - **Mitigation**: Adjust dialog z-index or append to same container

---

### Phase 3: Integration and Tab State Management

**Goal**: Integrate viewer changes with tab management system and verify tab-independent behavior

**Files to Modify**:
- `src/terminal-app/index.ts` - Pass container to viewer constructors
- Potentially `src/tab-bar/tab-manager.ts` - If viewer state needs explicit management

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TerminalApp | Pass container to ImageViewer | Container element available | Viewer uses tab-specific container |
| Tab switching | Preserve viewer state | Viewer in DOM within tab content | Viewer visible when tab active |

**Processing Flow**:
```
1. TerminalApp creates ImageViewer with tab's container element
2. Each tab has independent viewer instance
3. Tab switch hides/shows container via display property
4. Viewer DOM preserved within hidden container
5. Tab close triggers viewer dispose
```

**Container Existence Check**:
- Verify container element exists before passing to viewers
- If container is null/undefined, log error and skip viewer creation
- Early return pattern to prevent runtime errors

**Error Handling Requirements**:
- Guard against null container in TerminalApp viewer initialization
- Provide clear error messages for debugging
- Graceful degradation: viewer simply not created if container unavailable

**Focus Management Rules**:
- When tab is hidden: Disable viewer keyboard event handlers
- When viewer is shown: Move focus to overlay element
- When viewer is closed: Restore focus to terminal element

**Implementation Steps**:

1. **Update TerminalApp viewer initialization**
   - Pass tab content container to ImageViewer constructor
   - Pass tab content container to FullscreenMarkdownView.show()

2. **Verify tab switching behavior**
   - Tab content visibility handled by TabManager (display: none/block)
   - Viewer within tab content automatically hidden/shown

3. **Verify tab close cleanup**
   - TabManager calls TerminalApp.dispose()
   - TerminalApp.dispose() should call viewer.dispose()
   - Verify no memory leaks

**Dependencies**:
- Requires: Phase 1, Phase 2 completed
- Blocks: None

**Testing Approach**:

*Integration Tests*:
- Verify tab switching preserves viewer state
- Verify tab close disposes viewer resources

*Manual Testing*:
- [ ] Open image viewer in Tab A
- [ ] Switch to Tab B (Tab A viewer hidden but preserved)
- [ ] Switch back to Tab A (viewer restored)
- [ ] Close Tab A (viewer resources cleaned up)
- [ ] Repeat for Markdown viewer

**Acceptance Criteria**:
- [ ] Each tab maintains independent viewer state
- [ ] Tab switching preserves viewer within tab
- [ ] Tab close properly disposes viewer
- [ ] No memory leaks on repeated tab operations

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Viewer keyboard handlers may conflict when tab hidden
  - **Mitigation**: Handlers already check isVisible()/isActive() state

---

## Complete File Structure

```
src/
├── image-viewer/
│   ├── index.ts              # Container-based constructor
│   ├── index.test.ts         # Updated tests
│   ├── display-mode.ts       # No changes
│   ├── display-mode.test.ts  # No changes
│   ├── pan-controller.ts     # No changes
│   └── pan-controller.test.ts # No changes
├── markdown/
│   ├── fullscreen.ts         # Container parameter in show()
│   ├── fullscreen.test.ts    # Updated tests
│   ├── link-dialog.ts        # Change to position: absolute, container-based
│   └── link-dialog.test.ts   # Update tests for container parameter
├── terminal-app/
│   └── index.ts              # Pass container to viewers
├── tab-bar/
│   └── tab-manager.ts        # No changes (existing cleanup sufficient)
└── styles.css                # CSS positioning changes
```

**File Descriptions**:
- `image-viewer/index.ts`: Main ImageViewer class - constructor change to use container
- `markdown/fullscreen.ts`: FullscreenMarkdownView - show() signature change
- `styles.css`: CSS changes for both viewer overlays (fixed -> absolute)
- `terminal-app/index.ts`: Integration point - pass container references

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in testing framework
- Mock DOM elements where necessary
- Table-driven tests for positioning verification

**Test Coverage Goals**:
- Core logic: 80%+ coverage
- DOM manipulation: Verify element hierarchy

**Key Test Areas**:

1. **ImageViewer** (`src/image-viewer/`)
   - Constructor accepts container parameter
   - Overlay appended to container
   - Overlay position is absolute
   - dispose() removes from container

2. **FullscreenMarkdownView** (`src/markdown/`)
   - show() accepts container parameter
   - Overlay appended to container
   - Overlay position is absolute
   - close() removes from container

### Integration Testing

**Scenarios**:
1. Tab with viewer open - switch away and back
2. Multiple tabs with viewers - independent state
3. Tab close with viewer open - cleanup verification

### Manual Testing Checklist

Based on spec test scenarios:
- [ ] Tab bar visible when ImageViewer is displayed
- [ ] Tab bar visible when MarkdownView is displayed
- [ ] Tab click switches tabs during viewer display
- [ ] Ctrl+Tab/Ctrl+Shift+Tab works during viewer display
- [ ] Escape key closes viewer
- [ ] Click outside content closes viewer
- [ ] Window resize - viewer resizes correctly
- [ ] Rapid tab switching with viewers - no visual glitches

## Dependencies

### External Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| @tauri-apps/plugin-shell | existing | Link opening |
| @tauri-apps/plugin-clipboard-manager | existing | Code copy |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 and Phase 2 can be done in parallel
2. Phase 3 depends on both Phase 1 and Phase 2

**Component Dependencies**:
- `ImageViewer` uses container element
- `FullscreenMarkdownView` uses container element
- `TerminalApp` provides container to both viewers
- `TabManager` manages tab content visibility

## Risk Assessment

### Technical Risks

1. **Event Handler Conflicts**
   - **Risk**: Keyboard handlers active when tab is hidden
   - **Likelihood**: Low (handlers check visibility state)
   - **Impact**: Low (keys may be captured incorrectly)
   - **Mitigation**: Verify handlers check isVisible()/isActive()

2. **Z-Index Stacking Issues**
   - **Risk**: Link dialog appears behind overlay
   - **Likelihood**: Medium
   - **Impact**: Medium (dialog unusable)
   - **Mitigation**: Test dialog positioning, adjust z-index if needed

3. **Viewport Calculation Differences**
   - **Risk**: Fit calculations may differ
   - **Likelihood**: Low (uses overlay dimensions)
   - **Impact**: Medium (incorrect zoom levels)
   - **Mitigation**: Test various image sizes and window sizes

## Performance Considerations

1. **Tab Switching**
   - Tab content visibility toggle via CSS (display: none/block)
   - Viewer DOM preserved, no re-render on switch
   - Expected: < 50ms latency

2. **Viewer Show/Hide**
   - Existing CSS transitions (opacity 0.15s)
   - Expected: < 150ms animation

3. **Memory**
   - Each tab maintains viewer state in DOM
   - Resources released on tab close via dispose()

## Security Considerations

- No changes to security model
- Existing XSS protections in Markdown rendering remain unchanged
- Link confirmation dialog behavior unchanged

## Open Questions

### From Specification
- None - all requirements are clear

### Implementation-Specific
- None - approach is straightforward

## Future Enhancements

Items not in current scope:
- Viewer state persistence across application restart
- Drag viewer between tabs
- Picture-in-picture mode for images

## Success Metrics

### Functional Completeness
- [ ] All MVP features implemented (US1, US2)
- [ ] All test scenarios pass
- [ ] Error handling works correctly

### Quality Metrics
- [ ] All existing tests pass
- [ ] New tests added for container-based rendering
- [ ] No critical bugs in manual testing

### Performance Metrics
- [ ] Tab switch latency < 50ms
- [ ] Viewer show/hide transition < 150ms

### User Experience
- [ ] Tab bar always accessible during viewer display
- [ ] Viewer close behaviors unchanged
- [ ] Zoom/pan functionality unchanged

## References

- **Specification**: `doc/tasks/viewer-rendering-area/SPEC.md`
- **Requirements**: `doc/tasks/viewer-rendering-area/要件定義書.md`
- **ImageViewer Implementation**: `src/image-viewer/index.ts`
- **FullscreenMarkdownView Implementation**: `src/markdown/fullscreen.ts`
- **Tab Manager**: `src/tab-bar/tab-manager.ts`
- **HTML Structure**: `src/index.html`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Stakeholder review
   - Confirm approach

2. **Begin Implementation**
   - Start with Phase 1 (ImageViewer) or Phase 2 (Markdown)
   - Phases can be implemented in parallel

3. **Testing**
   - Run unit tests after each phase
   - Integration testing after Phase 3

4. **Verification**
   - Follow VERIFICATION.md checklist
   - Manual testing per test scenarios
