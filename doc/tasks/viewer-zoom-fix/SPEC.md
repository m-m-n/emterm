# Feature: Viewer Zoom Fix

## Overview

This feature addresses issues with the existing zoom functionality in eMterm's image viewer and Markdown viewer. The image viewer's zoom percentage will be changed to reference the original image size (100% = actual size), and pan functionality will be added. The Markdown viewer's zoom will be changed from `transform: scale()` to `font-size` based scaling for sharper text rendering.

## Objectives

- Change image viewer zoom baseline from "fit state" to "original image size"
- Add pan (drag-to-move) functionality for images that exceed the viewport
- Change Markdown viewer zoom from transform-based to font-size-based scaling
- Maintain backward compatibility with existing UI and keyboard shortcuts

## User Stories

### US1: View Image at Actual Size
As a user viewing images, I want 100% zoom to show the image at its actual pixel size, so that I can accurately assess the image's true dimensions.

**Acceptance Criteria:**
- [ ] 100% zoom displays image at 1:1 pixel ratio
- [ ] 200% zoom displays image at 2x the original size
- [ ] Initial display fits the image to viewport (showing actual fit percentage)
- [ ] Zoom reset returns to fit state, not 100%

### US2: Pan Zoomed Images
As a user viewing a zoomed image that exceeds the viewport, I want to drag the image to see different parts, so that I can examine details across the entire image.

**Acceptance Criteria:**
- [ ] Mouse drag moves the image when it exceeds viewport
- [ ] Cursor changes to `grab` when pan is available
- [ ] Cursor changes to `grabbing` during pan operation
- [ ] Image cannot be dragged beyond its edges (constrained panning)
- [ ] Pan is disabled when image fits within viewport

### US3: Sharp Markdown Zoom
As a user reading Markdown documents, I want zoomed text to remain sharp and reflow, so that I can comfortably read at any zoom level.

**Acceptance Criteria:**
- [ ] Zooming changes font-size instead of applying scale transform
- [ ] Text remains sharp at all zoom levels
- [ ] Text reflows (line wrapping adjusts) when zoomed
- [ ] Behavior matches browser's Ctrl+wheel zoom

## Technical Requirements

### Functional Requirements
- **FR1:** Image viewer zoom percentage based on original image size (100% = 1:1)
- **FR2:** Image viewer initial display at fit-to-viewport size
- **FR3:** Image viewer pan functionality via mouse drag
- **FR4:** Image viewer pan constraints (cannot drag beyond image edges)
- **FR5:** Image viewer recalculates fit level on window resize
- **FR6:** Markdown viewer zoom via font-size property change
- **FR7:** Markdown viewer text reflow on zoom
- **FR8:** Existing zoom UI (buttons, percentage display) continues to work
- **FR9:** Existing keyboard shortcuts continue to work

### Non-Functional Requirements
- **NFR1 - Performance:** Zoom and pan operations complete within 16ms (60fps)
- **NFR2 - Consistency:** Zoom UI appearance unchanged
- **NFR3 - Compatibility:** Existing keyboard shortcuts (Escape, arrows) continue to work

## Implementation Approach

### Architecture

**Modified Components:**
```
src/
├── shared/
│   ├── zoom-controller.ts      # Modified: Add mode selection
│   └── zoom-styles.ts          # No change
├── image-viewer/
│   ├── index.ts                # Modified: Pan functionality, new zoom calc
│   └── pan-controller.ts       # NEW: Pan logic separated
└── markdown/
    └── fullscreen.ts           # Modified: Font-size zoom method
```

**Component Responsibilities:**
```
┌─────────────────────────────────────────────────────────────┐
│                    ZoomController                            │
│  - Zoom state management (level only, no origin)            │
│  - Event handling (wheel, keyboard, click)                  │
│  - UI rendering (close button, zoom bar)                    │
│  - Callback for applying zoom (delegated to viewer)         │
│  - Does NOT apply zoom directly (viewer responsibility)     │
└─────────────────────────────────────────────────────────────┘
              │                              │
              ▼                              ▼
┌──────────────────────────┐  ┌───────────────────────────────┐
│      ImageViewer         │  │   FullscreenMarkdownView      │
│  - Calculate fit level   │  │   - Apply font-size zoom      │
│  - Apply size-based zoom │  │   - Handle text reflow        │
│  - Pan functionality     │  │                               │
│  - Constrained movement  │  │                               │
└──────────────────────────┘  └───────────────────────────────┘
```

### Image Viewer Changes

#### Zoom Calculation

**Current (to be changed):**
```typescript
// 100% = fit state
const scale = this.state.level / 100;
container.style.transform = `scale(${scale})`;
```

**New:**
```typescript
// 100% = original image size
// Initial display = fit level
interface ImageZoomState {
  zoomLevel: number;    // Current zoom (25-400, 100 = original size)
  fitLevel: number;     // Calculated fit level
  panOffset: { x: number; y: number };
}

// Calculate fit level on image load
const fitLevel = Math.min(
  (viewportWidth / imageWidth) * 100,
  (viewportHeight / imageHeight) * 100
);

// Apply zoom
const displayWidth = imageWidth * (zoomLevel / 100);
const displayHeight = imageHeight * (zoomLevel / 100);
canvas.style.width = `${displayWidth}px`;
canvas.style.height = `${displayHeight}px`;
```

#### Pan Implementation

##### Coordinate System

The PanController uses **screen coordinates (clientX/clientY)** for all calculations:

```
┌─────────────────────────────────────────────────────┐
│ Viewport (container)                                 │
│  ┌─────────────────────────────────────────┐        │
│  │                                         │        │
│  │  ┌─────────────────────────────────┐    │        │
│  │  │     Image (canvas)              │    │        │
│  │  │                                 │    │        │
│  │  │     offset = (0, 0)             │    │        │
│  │  │     = centered                  │    │        │
│  │  │                                 │    │        │
│  │  └─────────────────────────────────┘    │        │
│  │                                         │        │
│  └─────────────────────────────────────────┘        │
│                                                      │
└─────────────────────────────────────────────────────┘

Coordinate definitions:
- offsetX/offsetY: CSS translate offset from center position
- Positive offsetX: Image moves RIGHT
- Negative offsetX: Image moves LEFT
- Positive offsetY: Image moves DOWN
- Negative offsetY: Image moves UP

Bounds calculation:
- overflowX = max(0, imageWidth - viewportWidth)
- minX = -overflowX / 2  (image's left edge at viewport's left edge)
- maxX = +overflowX / 2  (image's right edge at viewport's right edge)
```

##### State Interface

```typescript
interface PanState {
  isDragging: boolean;
  startX: number;
  startY: number;
  offsetX: number;
  offsetY: number;
}

class PanController {
  private state: PanState;
  private canvas: HTMLCanvasElement;
  private container: HTMLElement;

  constructor(canvas: HTMLCanvasElement, container: HTMLElement);

  // Check if pan is available
  canPan(): boolean {
    const canvasRect = this.canvas.getBoundingClientRect();
    const containerRect = this.container.getBoundingClientRect();
    return canvasRect.width > containerRect.width ||
           canvasRect.height > containerRect.height;
  }

  // Start drag
  onMouseDown(e: MouseEvent): void {
    if (!this.canPan()) return;
    this.state.isDragging = true;
    this.state.startX = e.clientX - this.state.offsetX;
    this.state.startY = e.clientY - this.state.offsetY;
    this.canvas.style.cursor = 'grabbing';
  }

  // During drag
  onMouseMove(e: MouseEvent): void {
    if (!this.state.isDragging) return;

    let newOffsetX = e.clientX - this.state.startX;
    let newOffsetY = e.clientY - this.state.startY;

    // Constrain to bounds
    const bounds = this.calculateBounds();
    newOffsetX = Math.max(bounds.minX, Math.min(bounds.maxX, newOffsetX));
    newOffsetY = Math.max(bounds.minY, Math.min(bounds.maxY, newOffsetY));

    this.state.offsetX = newOffsetX;
    this.state.offsetY = newOffsetY;
    this.applyOffset();
  }

  // End drag
  onMouseUp(): void {
    this.state.isDragging = false;
    this.canvas.style.cursor = this.canPan() ? 'grab' : 'default';
  }

  // Calculate allowed pan bounds
  private calculateBounds(): { minX: number; maxX: number; minY: number; maxY: number } {
    const canvasRect = this.canvas.getBoundingClientRect();
    const containerRect = this.container.getBoundingClientRect();

    const overflowX = Math.max(0, canvasRect.width - containerRect.width);
    const overflowY = Math.max(0, canvasRect.height - containerRect.height);

    return {
      minX: -overflowX / 2,
      maxX: overflowX / 2,
      minY: -overflowY / 2,
      maxY: overflowY / 2,
    };
  }

  // Apply offset to canvas
  private applyOffset(): void {
    this.canvas.style.transform =
      `translate(${this.state.offsetX}px, ${this.state.offsetY}px)`;
  }

  // Reset offset (called on zoom reset)
  reset(): void {
    this.state.offsetX = 0;
    this.state.offsetY = 0;
    this.applyOffset();
  }

  // Recalculate bounds on window resize and constrain current offset
  recalculateBounds(): void {
    const bounds = this.calculateBounds();
    this.state.offsetX = Math.max(bounds.minX, Math.min(bounds.maxX, this.state.offsetX));
    this.state.offsetY = Math.max(bounds.minY, Math.min(bounds.maxY, this.state.offsetY));
    this.applyOffset();
    this.canvas.style.cursor = this.canPan() ? 'grab' : 'default';
  }

  dispose(): void {
    // Remove event listeners
  }
}
```

### Markdown Viewer Changes

#### Font-Size Zoom Implementation

**Current (to be changed):**
```typescript
// Transform-based zoom (blurry)
container.style.transform = `scale(${level / 100})`;
container.style.transformOrigin = `${originX}% ${originY}%`;
```

**New:**
```typescript
// Font-size based zoom (sharp, reflowing)
const BASE_FONT_SIZE = 16; // px

class MarkdownZoomController {
  private baseFontSize = BASE_FONT_SIZE;
  private zoomLevel = 100;
  private container: HTMLElement;

  applyZoom(level: number): void {
    this.zoomLevel = level;

    // Preserve scroll position relative to content
    const scrollContainer = this.container.parentElement;
    const prevScrollTop = scrollContainer?.scrollTop ?? 0;
    const prevScrollHeight = scrollContainer?.scrollHeight ?? 1;
    const scrollRatio = prevScrollTop / prevScrollHeight;

    // Apply new font size
    const newFontSize = this.baseFontSize * (level / 100);
    this.container.style.fontSize = `${newFontSize}px`;
    // Browser automatically reflows text

    // Restore scroll position proportionally after reflow
    requestAnimationFrame(() => {
      if (scrollContainer) {
        const newScrollHeight = scrollContainer.scrollHeight;
        scrollContainer.scrollTop = scrollRatio * newScrollHeight;
      }
    });
  }

  getZoomLevel(): number {
    return this.zoomLevel;
  }
}
```

#### CSS Considerations

Ensure relative units are used throughout Markdown styles:

```css
/* Good - scales with font-size */
.markdown-content {
  line-height: 1.6;      /* relative */
  padding: 1em;          /* relative */
  margin-bottom: 0.5em;  /* relative */
}

.markdown-content pre {
  font-size: 0.875em;    /* relative to parent */
  padding: 1em;
}

/* Bad - won't scale */
.markdown-content {
  padding: 16px;         /* fixed */
}
```

### ZoomController Modifications

Change to callback-based design where the viewer is responsible for applying zoom:

```typescript
interface ZoomControllerOptions {
  container: HTMLElement;
  overlay: HTMLElement;
  minZoom?: number;
  maxZoom?: number;
  zoomStep?: number;
  onClose?: () => void;
  onZoomChange: (level: number) => void;  // REQUIRED: Callback for applying zoom
  initialLevel?: number;              // For image viewer's fit level
  formatDisplay?: (level: number) => string;  // Custom display format
}
```

**Design Rationale:**
- ZoomController manages state and UI only, does not apply zoom directly
- Each viewer implements its own zoom application logic via `onZoomChange`
- This avoids mode switching and keeps responsibilities clear
- Image viewer: applies zoom via canvas width/height
- Markdown viewer: applies zoom via font-size

### State Ownership

**Principle:** Each component owns and manages only its own state. No shared mutable state between components.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          State Ownership Matrix                            │
├───────────────────────────────────────────────────────────────────────────┤
│ Component          │ Owns                    │ Receives (read-only)        │
├───────────────────────────────────────────────────────────────────────────┤
│ ZoomController     │ - zoomLevel (current)   │ - initialLevel              │
│                    │ - UI element refs       │ - min/max/step config       │
│                    │                         │                             │
│ PanController      │ - isDragging            │ - canvas dimensions         │
│                    │ - offsetX/offsetY       │ - container dimensions      │
│                    │ - startX/startY         │                             │
│                    │                         │                             │
│ ImageViewer        │ - canvas element        │ - zoomLevel (via callback)  │
│                    │ - currentImage data     │                             │
│                    │ - fitLevel (calculated) │                             │
│                    │ - overlay element       │                             │
│                    │                         │                             │
│ MarkdownViewer     │ - content element       │ - zoomLevel (via callback)  │
│                    │ - overlay element       │                             │
│                    │ - baseFontSize          │                             │
└───────────────────────────────────────────────────────────────────────────┘
```

**State Flow:**
```
User Action (wheel/click/drag)
        │
        ▼
┌───────────────────┐
│  ZoomController   │ ─── owns zoomLevel ───► onZoomChange(level) callback
└───────────────────┘                                    │
                                                         ▼
                                              ┌─────────────────────┐
                                              │   Viewer (Image/MD) │
                                              │   applies zoom      │
                                              │   owns display state│
                                              └─────────────────────┘

Mouse Drag
    │
    ▼
┌───────────────────┐
│   PanController   │ ─── owns offset ───► applyOffset() to canvas transform
└───────────────────┘
```

**Key Invariants:**
1. ZoomController never directly modifies canvas/content styles
2. PanController never reads or modifies zoomLevel
3. Viewers receive zoomLevel via callback, never query ZoomController
4. Each component cleans up its own event listeners in dispose()

### Integration Flow

**Image Viewer Integration:**
```typescript
class ImageViewer {
  private zoomController: ZoomController | null = null;
  private panController: PanController | null = null;
  private fitLevel: number = 100;

  async show(image: DecodedImage): Promise<void> {
    // ... existing code ...

    // Calculate fit level
    this.fitLevel = this.calculateFitLevel(image.width, image.height);

    // Initialize zoom controller with custom zoom handling
    this.zoomController = new ZoomController({
      container: this.canvas,
      overlay: this.overlay,
      initialLevel: this.fitLevel,
      onZoomChange: (level) => this.applyImageZoom(level),
      onClose: () => this.hide(),
      formatDisplay: (level) => `${Math.round(level)}%`,
    });

    // Initialize pan controller
    this.panController = new PanController(this.canvas, this.overlay);
  }

  private calculateFitLevel(imageWidth: number, imageHeight: number): number {
    const viewportWidth = this.overlay.clientWidth * 0.95;
    const viewportHeight = this.overlay.clientHeight * 0.95;
    return Math.min(
      (viewportWidth / imageWidth) * 100,
      (viewportHeight / imageHeight) * 100,
      400  // Cap at max zoom
    );
  }

  private applyImageZoom(level: number): void {
    const displayWidth = this.currentImage!.width * (level / 100);
    const displayHeight = this.currentImage!.height * (level / 100);
    this.canvas.style.width = `${displayWidth}px`;
    this.canvas.style.height = `${displayHeight}px`;

    // Reset pan offset and update cursor
    this.panController?.reset();
    this.updateCursor();
  }

  private updateCursor(): void {
    if (this.panController?.canPan()) {
      this.canvas.style.cursor = 'grab';
    } else {
      this.canvas.style.cursor = 'default';
    }
  }

  // Window resize handling
  private setupResizeHandler(): void {
    this.resizeHandler = () => {
      if (!this.currentImage) return;

      // Recalculate fit level
      const newFitLevel = this.calculateFitLevel(
        this.currentImage.width,
        this.currentImage.height
      );
      this.fitLevel = newFitLevel;

      // Recalculate pan bounds and constrain current offset
      this.panController?.recalculateBounds();
    };

    window.addEventListener('resize', this.resizeHandler);
  }

  dispose(): void {
    if (this.resizeHandler) {
      window.removeEventListener('resize', this.resizeHandler);
    }
    // ... other cleanup
  }
}
```

**Markdown Viewer Integration:**
```typescript
class FullscreenMarkdownView {
  private zoomController: ZoomController | null = null;
  private readonly BASE_FONT_SIZE = 16;

  show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void {
    // ... existing code ...

    // Initialize zoom controller with callback for font-size zoom
    this.zoomController = new ZoomController({
      container: this.content,
      overlay: this.overlay,
      onZoomChange: (level) => this.applyFontSizeZoom(level),
      onClose: () => this.close(),
    });
  }

  private applyFontSizeZoom(level: number): void {
    if (!this.content) return;
    const newFontSize = this.BASE_FONT_SIZE * (level / 100);
    this.content.style.fontSize = `${newFontSize}px`;
  }
}
```

### File Structure

```
src/
├── shared/
│   ├── zoom-controller.ts      # Modified: Add mode, callbacks
│   ├── zoom-controller.test.ts # Updated tests
│   └── zoom-styles.ts          # No change
├── image-viewer/
│   ├── index.ts                # Modified: New zoom calc, pan integration
│   ├── pan-controller.ts       # NEW: Pan functionality
│   └── pan-controller.test.ts  # NEW: Pan tests
└── markdown/
    ├── fullscreen.ts           # Modified: Font-size zoom
    └── styles.ts               # May need updates for relative units
```

## Test Scenarios

### Unit Tests

**ZoomController:**
- [ ] Requires onZoomChange callback (throws if not provided)
- [ ] Calls onZoomChange callback when zoom changes
- [ ] Uses initialLevel when provided
- [ ] formatDisplay customizes percentage display
- [ ] Does not apply zoom directly (viewer responsibility)

**PanController:**
- [ ] canPan() returns true when image exceeds viewport
- [ ] canPan() returns false when image fits in viewport
- [ ] Pan offset is constrained to bounds
- [ ] reset() clears offset
- [ ] Cursor changes during drag operation
- [ ] recalculateBounds() constrains offset after viewport change

**ImageViewer:**
- [ ] calculateFitLevel() returns correct fit percentage
- [ ] applyImageZoom() sets correct canvas dimensions
- [ ] Initial display uses fit level
- [ ] Zoom reset returns to fit level
- [ ] Window resize recalculates fit level
- [ ] Window resize constrains pan offset within new bounds

**MarkdownViewer:**
- [ ] applyFontSizeZoom() sets correct font-size
- [ ] 100% zoom = 16px font-size
- [ ] 150% zoom = 24px font-size
- [ ] 200% zoom = 32px font-size

### Integration Tests

- [ ] Image viewer shows correct fit percentage on open
- [ ] Image viewer zoom changes image dimensions correctly
- [ ] Image viewer pan works when image exceeds viewport
- [ ] Image viewer pan disabled when image fits
- [ ] Markdown viewer text remains sharp at all zoom levels
- [ ] Markdown viewer text reflows on zoom
- [ ] Existing keyboard shortcuts work in both viewers

### E2E Tests

- [ ] Open large image, verify fit percentage shown (e.g., "35%")
- [ ] Zoom image to 100%, verify 1:1 pixel display
- [ ] Zoom image to 200%, drag to see different parts
- [ ] Verify image cannot be dragged past edges
- [ ] Open Markdown, zoom to 150%, verify sharp text
- [ ] Verify Markdown line wrapping changes on zoom
- [ ] Verify Escape key closes both viewers
- [ ] Verify arrow keys scroll Markdown viewer

### Edge Cases

- [ ] Very large image (fit level < 25%): Display at 25%
- [ ] Very small image (fit level > 400%): Display at 400%
- [ ] Rapid zoom + pan operations don't cause glitches
- [ ] Window resize updates fit level correctly
- [ ] GIF animation continues during pan
- [ ] Long Markdown document maintains scroll position on zoom

## Security Considerations

- **Input Validation:** Zoom levels clamped to valid range
- **Event Handling:** No user data transmitted or stored
- **No External Dependencies:** Uses only standard browser APIs

## Error Handling

### Error Scenarios

| Scenario | Handling |
|----------|----------|
| Canvas dimension exceeds browser limits | Clamp to maximum safe size |
| Pan calculation produces NaN | Reset to zero offset |
| Font-size calculation produces invalid value | Clamp to 4-64px range |

## Performance Optimization

### Performance Goals
- Zoom operation: < 16ms
- Pan operation: < 16ms
- Font-size change: Browser-dependent (acceptable)

### Optimization Strategies

1. **Canvas Sizing:** Direct width/height assignment instead of transform
2. **Pan Throttling:** RequestAnimationFrame for smooth movement
3. **Font-size:** Single property change, let browser handle reflow
4. **Event Delegation:** Reuse existing event listeners where possible

### Implementation Notes

```typescript
// Throttle pan with requestAnimationFrame
private pendingPanUpdate = false;

onMouseMove(e: MouseEvent): void {
  if (!this.state.isDragging) return;
  if (this.pendingPanUpdate) return;

  this.pendingPanUpdate = true;
  requestAnimationFrame(() => {
    this.updatePanPosition(e);
    this.pendingPanUpdate = false;
  });
}
```

## Success Criteria

- [ ] All functional requirements implemented
- [ ] All acceptance criteria for user stories met
- [ ] Unit tests pass for new and modified components
- [ ] Integration tests pass for both viewers
- [ ] E2E tests pass
- [ ] Performance meets 16ms target for zoom and pan
- [ ] No regressions in existing functionality
- [ ] Code review completed

## Open Questions

None - all requirements have been confirmed.

## Implementation Phases

### Phase 1: Image Viewer Zoom Refactor
**Goals:** Change zoom baseline to original image size
**Deliverables:**
- Calculate and display fit level
- Apply size-based zoom instead of scale transform
- Update zoom reset to return to fit level

### Phase 2: Pan Functionality
**Goals:** Implement drag-to-move for images
**Deliverables:**
- PanController class
- Integration with ImageViewer
- Cursor feedback
- Boundary constraints

### Phase 3: Markdown Font-Size Zoom
**Goals:** Change Markdown zoom to font-size based
**Deliverables:**
- Font-size zoom implementation
- CSS updates for relative units
- Text reflow verification

### Phase 4: Testing & Polish
**Goals:** Complete testing and refinement
**Deliverables:**
- Unit tests for all new code
- Integration tests
- E2E tests
- Performance verification

## References

- Existing zoom implementation: `src/shared/zoom-controller.ts`
- Image viewer: `src/image-viewer/index.ts`
- Markdown viewer: `src/markdown/fullscreen.ts`
- Original specification: `doc/tasks/viewer-usability/SPEC.md`
- Requirements document: `doc/tasks/viewer-zoom-fix/要件定義書.md`
