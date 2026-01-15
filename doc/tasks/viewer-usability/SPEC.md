# Feature: Viewer Usability Improvements

## Overview

Add zoom functionality and a close button to the fullscreen image viewer and Markdown viewer in eMterm. This improvement enhances user experience by providing intuitive ways to examine content details and close the viewer.

## Objectives

- Implement zoom in/out functionality for both viewers (25%-400% range)
- Add a visual close button in the top-right corner
- Provide a zoom control bar in the bottom-right corner
- Ensure consistent UX across both viewers

## User Stories

### US1: Zoom Content with Mouse Wheel
As a user viewing images or Markdown content, I want to zoom in/out using Ctrl+mouse wheel, so that I can examine details more closely.

**Acceptance Criteria:**
- [ ] Ctrl + wheel up zooms in by 10%
- [ ] Ctrl + wheel down zooms out by 10%
- [ ] Zoom is centered on mouse cursor position
- [ ] Zoom range is limited to 25%-400%

### US2: Zoom Content with Keyboard
As a user viewing images or Markdown content, I want to zoom in/out using +/- keys, so that I can adjust zoom without a mouse.

**Acceptance Criteria:**
- [ ] `+` or `=` key zooms in by 10%
- [ ] `-` key zooms out by 10%
- [ ] `0` key resets zoom to 100%
- [ ] Zoom is centered on viewport center

### US3: Zoom Content with UI Buttons
As a user viewing images or Markdown content, I want to use zoom buttons, so that I have a visual way to control zoom level.

**Acceptance Criteria:**
- [ ] `+` button zooms in by 10%
- [ ] `-` button zooms out by 10%
- [ ] Clicking percentage display resets to 100%
- [ ] Current zoom level is displayed

### US4: Close Viewer with Mouse
As a user, I want to click a close button to exit the viewer, so that I don't need to remember keyboard shortcuts.

**Acceptance Criteria:**
- [ ] Close button (x) is visible in top-right corner
- [ ] Clicking the button closes the viewer
- [ ] Button has hover feedback
- [ ] Button position is fixed regardless of scroll/zoom

## Technical Requirements

### Functional Requirements
- **FR1:** Zoom functionality using transform: scale CSS property
- **FR2:** Three zoom input methods: mouse wheel (Ctrl+), keyboard (+/-), UI buttons
- **FR3:** Zoom range: 25% to 400%, 10% increments
- **FR4:** Mouse wheel zoom centers on cursor position
- **FR5:** Keyboard/button zoom centers on viewport center
- **FR6:** Close button fixed to top-right corner
- **FR7:** Zoom control bar fixed to bottom-right corner
- **FR8:** Both viewers share the same zoom component/logic

### Non-Functional Requirements
- **NFR1 - Performance:** Zoom operations must complete within 16ms (60fps)
- **NFR2 - Consistency:** Both viewers must have identical zoom behavior
- **NFR3 - Compatibility:** Existing keyboard shortcuts (Escape, arrows) must continue to work

## Implementation Approach

### Architecture

**Component Structure:**
```
src/
├── shared/
│   └── zoom-controller.ts    # Shared zoom logic and UI
├── image-viewer/
│   └── index.ts              # Integrate ZoomController
└── markdown/
    └── fullscreen.ts         # Integrate ZoomController
```

**Component Responsibilities:**
```
┌─────────────────────────────────────────────────┐
│              ZoomController                      │
│  - Zoom state management (level, origin)        │
│  - Event handling (wheel, keyboard, click)      │
│  - UI rendering (close button, zoom bar)        │
│  - Scale transform application                  │
└─────────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
┌─────────────────┐  ┌─────────────────────────┐
│  ImageViewer    │  │  FullscreenMarkdownView │
│  - Uses zoom    │  │  - Uses zoom controller │
│    controller   │  │  - Existing scroll      │
│  - Canvas render│  │    functionality        │
└─────────────────┘  └─────────────────────────┘
```

### Data Flow

**Zoom Operation Flow:**
```
User Input → Event Handler → Calculate New Zoom Level → Apply Transform → Update UI
    │              │                   │                      │              │
    │              │                   │                      │              │
  wheel/       detect type         clamp to              transform:      update
  key/         and origin          25-400%               scale()        percentage
  click                                                                  display
```

**Zoom with Mouse Position:**
```
1. Capture mouse position (clientX, clientY)
2. Convert to content coordinates (accounting for current zoom)
3. Calculate new zoom level
4. Adjust scroll/transform-origin to keep mouse point stable
5. Apply new transform: scale(newLevel)
```

### Shared Zoom Controller

```typescript
interface ZoomState {
  level: number;        // 25-400 (percentage)
  originX: number;      // transform-origin X
  originY: number;      // transform-origin Y
}

interface ZoomControllerOptions {
  container: HTMLElement;       // Element to zoom
  overlay: HTMLElement;         // Parent overlay for fixed UI
  minZoom?: number;             // Default: 25
  maxZoom?: number;             // Default: 400
  zoomStep?: number;            // Default: 10
  onClose?: () => void;         // Close callback
}

class ZoomController {
  private state: ZoomState;
  private options: ZoomControllerOptions;
  private closeButton: HTMLElement;
  private zoomBar: HTMLElement;

  constructor(options: ZoomControllerOptions);

  // Public methods
  zoomIn(): void;
  zoomOut(): void;
  zoomTo(level: number): void;
  resetZoom(): void;
  getZoomLevel(): number;
  dispose(): void;

  // Event handlers
  private handleWheel(e: WheelEvent): void;
  private handleKeydown(e: KeyboardEvent): void;
  private handleButtonClick(action: 'in' | 'out' | 'reset'): void;

  // UI methods
  private createUI(): void;
  private updateZoomDisplay(): void;
  private applyZoom(): void;
}
```

### CSS Styles

**Close Button:**
```css
.viewer-close-button {
  position: fixed;
  top: 16px;
  right: 16px;
  width: 32px;
  height: 32px;
  background: rgba(0, 0, 0, 0.5);
  border: none;
  border-radius: 6px;
  color: white;
  font-size: 18px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10001;
  transition: background 0.15s ease;
}

.viewer-close-button:hover {
  background: rgba(0, 0, 0, 0.7);
}
```

**Zoom Control Bar:**
```css
.viewer-zoom-bar {
  position: fixed;
  bottom: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  gap: 4px;
  background: rgba(0, 0, 0, 0.5);
  border-radius: 6px;
  padding: 4px;
  z-index: 10001;
}

.viewer-zoom-button {
  width: 28px;
  height: 28px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: white;
  font-size: 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.viewer-zoom-button:hover {
  background: rgba(255, 255, 255, 0.1);
}

.viewer-zoom-level {
  min-width: 50px;
  text-align: center;
  color: white;
  font-family: monospace;
  font-size: 12px;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
}

.viewer-zoom-level:hover {
  background: rgba(255, 255, 255, 0.1);
}
```

### Keyboard Mappings

| Key | Action | Notes |
|-----|--------|-------|
| `+` or `=` | Zoom in 10% | Center-based |
| `-` | Zoom out 10% | Center-based |
| `0` | Reset to 100% | |
| `Escape` | Close viewer | Existing behavior |
| Arrow keys | Scroll (Markdown) | Existing behavior |
| Page Up/Down | Scroll page (Markdown) | Existing behavior |
| Home/End | Scroll to start/end (Markdown) | Existing behavior |

### Integration with ImageViewer

```typescript
// In src/image-viewer/index.ts
export class ImageViewer {
  private zoomController: ZoomController | null = null;

  async show(image: DecodedImage): Promise<void> {
    // ... existing code ...

    // Initialize zoom controller
    this.zoomController = new ZoomController({
      container: this.canvas,
      overlay: this.overlay,
      onClose: () => this.hide(),
    });
  }

  hide(): void {
    // ... existing code ...

    // Dispose zoom controller
    if (this.zoomController) {
      this.zoomController.dispose();
      this.zoomController = null;
    }
  }
}
```

### Integration with FullscreenMarkdownView

```typescript
// In src/markdown/fullscreen.ts
export class FullscreenMarkdownView {
  private zoomController: ZoomController | null = null;

  show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void {
    // ... existing code ...

    // Initialize zoom controller
    this.zoomController = new ZoomController({
      container: this.content,
      overlay: this.overlay,
      onClose: () => this.close(),
    });
  }

  close(): void {
    // ... existing code ...

    // Dispose zoom controller
    if (this.zoomController) {
      this.zoomController.dispose();
      this.zoomController = null;
    }
  }
}
```

### File Structure

```
src/
├── shared/
│   ├── zoom-controller.ts      # ZoomController class
│   ├── zoom-controller.test.ts # Unit tests
│   └── zoom-styles.ts          # CSS styles as string
├── image-viewer/
│   └── index.ts                # Modified to use ZoomController
└── markdown/
    └── fullscreen.ts           # Modified to use ZoomController
```

## Test Scenarios

### Unit Tests

- [ ] ZoomController initializes with default zoom level (100%)
- [ ] zoomIn() increases level by step (10%)
- [ ] zoomOut() decreases level by step (10%)
- [ ] Zoom level is clamped to min (25%)
- [ ] Zoom level is clamped to max (400%)
- [ ] resetZoom() sets level to 100%
- [ ] dispose() removes event listeners and UI elements
- [ ] getZoomLevel() returns current level

### Integration Tests

- [ ] Mouse wheel with Ctrl zooms content in ImageViewer
- [ ] Mouse wheel with Ctrl zooms content in MarkdownView
- [ ] +/- keys zoom content
- [ ] 0 key resets zoom
- [ ] Close button closes viewer
- [ ] Zoom bar buttons work correctly
- [ ] Clicking percentage resets zoom

### E2E Tests

- [ ] Open image viewer, zoom in with wheel, verify visual change
- [ ] Open Markdown viewer, zoom out with keyboard, verify visual change
- [ ] Open viewer, click close button, verify viewer closes
- [ ] Zoom to 400%, try to zoom more, verify no change
- [ ] Zoom to 25%, try to zoom less, verify no change
- [ ] Verify existing Escape key still closes viewer
- [ ] Verify existing arrow keys still scroll Markdown

### Edge Cases

- [ ] Rapid zoom operations don't cause visual glitches
- [ ] Zoom state resets when viewer is reopened
- [ ] Zoom works correctly with animated images (GIF)
- [ ] Zoom works correctly with large Markdown documents
- [ ] UI elements remain visible at all zoom levels

## Security Considerations

- **Input Validation:** Zoom level values are clamped to safe range
- **Event Handling:** No user data is transmitted or stored
- **No External Dependencies:** Implementation uses only standard browser APIs

## Error Handling

### Error Scenarios

| Scenario | Handling |
|----------|----------|
| Transform not supported | Fallback to no zoom (log warning) |
| Event listener attachment fails | Log error, continue without feature |
| Invalid zoom level input | Clamp to valid range |

## Performance Optimization

### Performance Goals
- Zoom operation: < 16ms (maintain 60fps)
- UI update: < 5ms
- Memory: No additional allocations during zoom

### Optimization Strategies

1. **CSS Transform:** Use GPU-accelerated transform: scale() instead of resizing content
2. **Throttling:** Throttle wheel events to prevent excessive calculations
3. **Single Reflow:** Batch UI updates to minimize reflows
4. **Event Delegation:** Use single event listeners on container

### Implementation Notes

```typescript
// Throttle wheel events
private lastWheelTime = 0;
private readonly WHEEL_THROTTLE = 16; // ms

private handleWheel(e: WheelEvent): void {
  const now = performance.now();
  if (now - this.lastWheelTime < this.WHEEL_THROTTLE) return;
  this.lastWheelTime = now;

  // Process wheel event...
}
```

## Success Criteria

- [ ] All functional requirements implemented
- [ ] All acceptance criteria for user stories met
- [ ] Unit tests pass with > 80% coverage for ZoomController
- [ ] Integration tests pass for both viewers
- [ ] E2E tests pass
- [ ] Performance meets 16ms target for zoom operations
- [ ] No regressions in existing functionality
- [ ] Code review completed

## Open Questions

None - all requirements have been confirmed.

## Implementation Phases

### Phase 1: Core Zoom Logic
**Goals:** Implement ZoomController with zoom state management
**Deliverables:**
- ZoomController class with zoom in/out/reset methods
- Unit tests for zoom logic

### Phase 2: UI Components
**Goals:** Implement close button and zoom control bar
**Deliverables:**
- Close button component
- Zoom bar component
- CSS styles

### Phase 3: Event Handling
**Goals:** Implement all input methods
**Deliverables:**
- Mouse wheel handler (with Ctrl detection)
- Keyboard handler (+/-/0 keys)
- Button click handlers

### Phase 4: Integration
**Goals:** Integrate ZoomController with both viewers
**Deliverables:**
- ImageViewer integration
- FullscreenMarkdownView integration
- Integration tests

### Phase 5: Testing & Polish
**Goals:** Complete testing and refinement
**Deliverables:**
- E2E tests
- Performance optimization
- Documentation

## References

- Existing implementation: `src/image-viewer/index.ts`
- Existing implementation: `src/markdown/fullscreen.ts`
- Requirements document: `doc/tasks/viewer-usability/要件定義書.md`
