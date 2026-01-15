# Implementation Plan: Viewer Zoom Fix

## Overview

This implementation plan addresses issues with the existing zoom functionality in eMterm's image viewer and Markdown viewer. The image viewer's zoom percentage will be changed to reference the original image size (100% = actual size) with pan functionality added. The Markdown viewer's zoom will be changed from `transform: scale()` to `font-size` based scaling for sharper text rendering.

## Objectives

- Change image viewer zoom baseline from "fit state" to "original image size" (100% = 1:1 pixels)
- Add pan (drag-to-move) functionality for images that exceed the viewport
- Change Markdown viewer zoom from transform-based to font-size-based scaling
- Maintain backward compatibility with existing UI and keyboard shortcuts

## Prerequisites

### Development Environment

- Bun (package manager and bundler)
- TypeScript 5.x
- Tauri development environment configured

### Dependencies

- Existing `ZoomController` class (`src/shared/zoom-controller.ts`)
- Existing `ImageViewer` class (`src/image-viewer/index.ts`)
- Existing `FullscreenMarkdownView` class (`src/markdown/fullscreen.ts`)

### Knowledge Requirements

- Understanding of CSS transform vs width/height sizing
- Understanding of mouse drag event handling
- Understanding of CSS relative units (em, rem)

## Architecture Overview

### Technology Stack

- **Language**: TypeScript
- **Runtime**: Bun
- **UI Framework**: Vanilla TypeScript with DOM APIs
- **Styling**: CSS-in-JS (inline styles)

### Design Approach

The implementation follows a callback-based approach where ZoomController manages zoom state and UI, but delegates the actual zoom application to each viewer. This allows different zoom mechanisms (size-based for images, font-size-based for Markdown) while sharing zoom UI components.

### Component Interaction

```
ZoomController (shared)
  - Manages zoom state (level, origin)
  - Provides UI (close button, zoom bar)
  - Calls onZoomChange callback when zoom changes
  - Calls onReset callback when reset is triggered
          |
          +---> ImageViewer
          |       - Calculates fit level on image load
          |       - Applies zoom by setting canvas width/height
          |       - Manages PanController for drag functionality
          |
          +---> FullscreenMarkdownView
                  - Applies zoom by setting font-size
                  - Text automatically reflows
```

## Implementation Phases

### Phase 1: ZoomController API Extension

**Goal**: Extend ZoomController to support custom zoom handling via callbacks

**Files to Modify**:
- `src/shared/zoom-controller.ts` - Add callback options and initial level support

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ZoomControllerOptions | Extended options interface | None | Includes onZoomChange, onReset, initialLevel |
| ZoomController.applyZoom | Apply zoom via callback or default transform | Callback or container set | Zoom applied to content |
| ZoomController.resetZoom | Reset to initial level | Initial level set | Returns to initial or 100% |

**Processing Flow**:
```
1. Constructor receives options with optional callbacks
   +-- If onZoomChange provided -> Store for later use
   +-- If initialLevel provided -> Use as starting level
2. On zoom operation (zoomIn/zoomOut/zoomTo)
   +-- Update internal level state
   +-- If onZoomChange callback exists -> Call callback with new level
   +-- Else -> Apply default transform-based zoom
3. On resetZoom
   +-- If onReset callback exists -> Call onReset
   +-- Reset to initialLevel (if set) or 100%
```

**Implementation Steps**:

1. **Extend ZoomControllerOptions interface**
   - Add `onZoomChange?: (level: number) => void` callback
   - Add `onReset?: () => void` callback
   - Add `initialLevel?: number` for non-100% starting zoom

2. **Modify applyZoom method**
   - Check if onZoomChange callback exists
   - If exists, call callback instead of applying transform
   - If not, apply default transform behavior

3. **Modify resetZoom method**
   - Reset to initialLevel if provided, otherwise 100%
   - Call onReset callback if provided

**Dependencies**:
- Requires: None (self-contained changes)
- Blocks: Phase 2 (ImageViewer), Phase 3 (Markdown)

**Testing Approach**:

*Unit Tests*:
- Test callback is called on zoom change
- Test initialLevel sets correct starting zoom
- Test resetZoom returns to initialLevel when set
- Test default transform behavior when no callback

**Acceptance Criteria**:
- [ ] onZoomChange callback is invoked on each zoom operation
- [ ] initialLevel option sets starting zoom level
- [ ] resetZoom returns to initialLevel, not always 100%
- [ ] Backward compatibility: existing usage without callbacks works unchanged

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Breaking existing ZoomController usage
  - **Mitigation**: All new options are optional; default behavior unchanged

---

### Phase 2: Image Viewer Zoom Refactor and Pan

**Goal**: Implement size-based zoom (100% = original image size) and add pan functionality

**Files to Create**:
- `src/image-viewer/pan-controller.ts` - Pan drag logic
- `src/image-viewer/pan-controller.test.ts` - Pan tests

**Files to Modify**:
- `src/image-viewer/index.ts` - Integrate new zoom calculation and pan

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| calculateFitLevel | Calculate zoom level to fit image in viewport | Image dimensions known | Returns fit percentage |
| applyImageZoom | Set canvas dimensions based on zoom level | Canvas exists, level valid | Canvas sized to level% of original |
| PanController | Handle mouse drag for image panning | Canvas larger than viewport | Image position updated within bounds |
| PanController.calculateBounds | Compute allowed pan range | Image and viewport sizes known | Returns min/max X/Y offsets |

**Processing Flow**:

```
Image Display Flow:
1. Image loaded with dimensions (width, height)
2. Calculate fit level: min(viewportWidth/imageWidth, viewportHeight/imageHeight) * 100
3. Initialize ZoomController with fitLevel as initialLevel
4. Initialize PanController for drag handling
5. Display image at fit level

Zoom Operation Flow:
1. User triggers zoom (wheel, keyboard, button)
2. ZoomController calls onZoomChange with new level
3. Calculate display dimensions: originalSize * (level / 100)
4. Set canvas width/height to display dimensions
5. Reset pan offset to center
6. Update cursor based on pan availability

Pan Operation Flow:
1. User initiates drag (mousedown on canvas)
   +-- If canvas smaller than viewport -> Ignore
   +-- Else -> Set dragging state, record start position
2. User drags (mousemove)
   +-- Calculate new offset from delta
   +-- Constrain offset to bounds
   +-- Apply offset via transform: translate()
3. User releases (mouseup)
   +-- Clear dragging state
   +-- Update cursor to 'grab'
```

**Implementation Steps**:

1. **Create PanController class**
   - State: isDragging, startPosition, currentOffset
   - Methods: onMouseDown, onMouseMove, onMouseUp, reset, dispose
   - Bound constraint calculation prevents dragging beyond edges

2. **Implement fit level calculation in ImageViewer**
   - Calculate on image load
   - Account for viewport padding (95% of viewport)
   - Clamp to min/max zoom range

3. **Modify ImageViewer.show to use new zoom approach**
   - Store original image dimensions
   - Calculate and display fit percentage
   - Initialize ZoomController with custom callbacks

4. **Implement size-based zoom application**
   - Set canvas width/height (not transform scale)
   - Center image in viewport
   - Reset pan on zoom change

5. **Integrate PanController with ImageViewer**
   - Create on show, dispose on hide
   - Wire up mouse events
   - Manage cursor states

**Dependencies**:
- Requires: Phase 1 (ZoomController API)
- Blocks: Phase 4 (Testing)

**Testing Approach**:

*Unit Tests*:
- PanController.canPan returns true when image exceeds viewport
- PanController.canPan returns false when image fits
- Pan offset constrained to calculated bounds
- reset() clears offset to zero
- Cursor changes to 'grab' when pan available

*Integration Tests*:
- ImageViewer displays at fit level on open
- Zoom changes canvas dimensions correctly
- Pan works when zoomed beyond viewport
- Pan disabled when image fits

**Acceptance Criteria**:
- [ ] 100% zoom displays image at 1:1 pixel ratio
- [ ] Initial display fits image to viewport with correct percentage shown
- [ ] Zoom reset returns to fit state
- [ ] Mouse drag pans image when it exceeds viewport
- [ ] Cursor changes to grab/grabbing appropriately
- [ ] Image cannot be dragged beyond its edges
- [ ] Pan is disabled when image fits within viewport
- [ ] GIF animation continues during pan

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Canvas dimension limits in browsers
  - **Mitigation**: Clamp display size to browser safe limits
- **Risk**: Performance issues with large images
  - **Mitigation**: Use requestAnimationFrame for pan updates

---

### Phase 3: Markdown Font-Size Zoom

**Goal**: Change Markdown zoom from transform-based to font-size-based for sharp text

**Files to Modify**:
- `src/markdown/fullscreen.ts` - Apply font-size instead of transform

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applyFontSizeZoom | Set container font-size based on zoom level | Content container exists | Font-size updated, text reflows |
| BASE_FONT_SIZE | Constant for 100% zoom font size (16px) | None | Used for zoom calculations |

**Processing Flow**:
```
Zoom Application Flow:
1. ZoomController triggers zoom change with new level
2. Calculate new font size: BASE_FONT_SIZE * (level / 100)
3. Set container.style.fontSize to calculated value
4. Browser automatically reflows text with new font size
```

**CSS Considerations**:
```
Markdown content should use relative units:
- line-height: 1.6 (unitless, relative to font-size)
- padding: 1em (relative to font-size)
- margin: 0.5em (relative to font-size)
- code font-size: 0.875em (relative to parent)
```

**Implementation Steps**:

1. **Add font-size zoom method to FullscreenMarkdownView**
   - Define BASE_FONT_SIZE constant (16px)
   - Implement applyFontSizeZoom(level: number)
   - Clamp calculated font-size to reasonable range (4-64px)

2. **Modify FullscreenMarkdownView to use ZoomController callbacks**
   - Pass onZoomChange callback to ZoomController
   - Apply font-size zoom instead of transform

3. **Verify CSS uses relative units** (if changes needed)
   - Review existing Markdown CSS in theme.ts or injected styles
   - Convert fixed pixel values to em/rem where applicable

**Dependencies**:
- Requires: Phase 1 (ZoomController API)
- Blocks: Phase 4 (Testing)

**Testing Approach**:

*Unit Tests*:
- applyFontSizeZoom sets correct font-size
- 100% zoom = 16px font-size
- 150% zoom = 24px font-size
- 200% zoom = 32px font-size
- Font-size clamped to safe range

*Integration Tests*:
- Text remains sharp at all zoom levels
- Text reflows (line wrapping adjusts) when zoomed
- Scroll position maintained on zoom

**Acceptance Criteria**:
- [ ] Zooming changes font-size instead of transform
- [ ] Text remains sharp at all zoom levels
- [ ] Text reflows when zoomed
- [ ] Existing keyboard shortcuts continue to work
- [ ] Behavior matches browser's Ctrl+wheel zoom expectation

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Some CSS may use fixed pixel values
  - **Mitigation**: Audit CSS and convert to relative units
- **Risk**: Scroll position may jump on zoom
  - **Mitigation**: Scroll position is maintained relative to viewport

---

### Phase 4: Testing and Polish

**Goal**: Complete testing and ensure quality

**Files to Create**:
- `src/image-viewer/pan-controller.test.ts` - Unit tests for PanController

**Files to Modify**:
- `src/shared/zoom-controller.test.ts` - Add tests for new options
- `src/image-viewer/index.test.ts` - Add tests for new zoom behavior
- `src/markdown/fullscreen.test.ts` - Add tests for font-size zoom

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ZoomController tests | Verify callback behavior | Test setup complete | All callback tests pass |
| PanController tests | Verify pan constraints and state | Mock canvas/container | All pan tests pass |
| ImageViewer tests | Verify zoom and pan integration | Mock DOM elements | Integration tests pass |
| FullscreenMarkdownView tests | Verify font-size zoom | Mock DOM elements | Font-size tests pass |

**Testing Categories**:

*Unit Tests*:
- ZoomController callback invocation
- PanController bounds calculation
- PanController state management
- Zoom level calculations

*Integration Tests*:
- ImageViewer zoom + pan workflow
- MarkdownViewer zoom + reflow
- Keyboard shortcut handling

*Manual E2E Tests*:
- Open large image, verify fit percentage
- Zoom to 100%, verify 1:1 display
- Zoom and pan with mouse
- Verify pan constraints
- Open Markdown, zoom and verify sharpness
- Verify text reflow on zoom

**Implementation Steps**:

1. **Write PanController unit tests**
   - Test canPan logic
   - Test bounds calculation
   - Test offset constraints
   - Test reset behavior

2. **Extend ZoomController tests**
   - Test onZoomChange callback
   - Test onReset callback
   - Test initialLevel behavior

3. **Extend ImageViewer tests**
   - Test fit level calculation
   - Test size-based zoom application

4. **Extend FullscreenMarkdownView tests**
   - Test font-size calculation
   - Test zoom application

5. **Performance verification**
   - Verify zoom operations complete within 16ms
   - Verify pan is smooth (60fps)

**Dependencies**:
- Requires: Phases 1, 2, 3

**Testing Approach**:

*Coverage Goals*:
- New code: 90%+ coverage
- Modified code: Maintain existing coverage

**Acceptance Criteria**:
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] E2E tests pass for specified scenarios
- [ ] Performance meets 16ms target
- [ ] No regressions in existing functionality

**Estimated Effort**: Medium (2-3 days)

---

## Complete File Structure

```
src/
+-- shared/
|   +-- zoom-controller.ts      # Modified: Add callbacks, initialLevel
|   +-- zoom-controller.test.ts # Modified: Add callback tests
|   +-- zoom-styles.ts          # No change
+-- image-viewer/
|   +-- index.ts                # Modified: Size-based zoom, pan integration
|   +-- index.test.ts           # Modified: Add zoom/pan tests
|   +-- pan-controller.ts       # NEW: Pan drag logic
|   +-- pan-controller.test.ts  # NEW: Pan unit tests
+-- markdown/
    +-- fullscreen.ts           # Modified: Font-size zoom
    +-- fullscreen.test.ts      # Modified: Add font-size tests
```

**File Descriptions**:

| File | Purpose |
|------|---------|
| `zoom-controller.ts` | Shared zoom state/UI with callback support for custom zoom handling |
| `pan-controller.ts` | Encapsulated pan logic with bounds constraints and state management |
| `image-viewer/index.ts` | Image display with size-based zoom and pan integration |
| `markdown/fullscreen.ts` | Fullscreen Markdown with font-size based zoom |

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- Mock DOM elements for UI tests
- Table-driven tests for multiple scenarios

**Test Coverage Goals**:
- New code: 90%+ coverage
- Core logic (pan bounds, zoom calculation): 100% coverage

**Key Test Areas**:

1. **ZoomController** (`src/shared/`)
   - Callback invocation on zoom operations
   - InitialLevel respected on construction
   - Reset returns to initialLevel

2. **PanController** (`src/image-viewer/`)
   - canPan() correctness
   - Bounds calculation accuracy
   - Offset constraint enforcement
   - State transitions (dragging)

3. **ImageViewer** (`src/image-viewer/`)
   - Fit level calculation
   - Canvas dimension updates on zoom
   - Pan integration

4. **FullscreenMarkdownView** (`src/markdown/`)
   - Font-size calculation
   - Zoom application

### Integration Testing

**Scenarios**:
1. Image viewer: Open -> Zoom -> Pan -> Close
2. Markdown viewer: Open -> Zoom -> Scroll -> Close
3. Keyboard shortcuts in both viewers

### Manual Testing Checklist

From SPEC.md test scenarios:
- [ ] Open large image (e.g., 4000x3000px)
- [ ] Verify fit percentage shown (e.g., "35%")
- [ ] Zoom to 100%, verify 1:1 pixel display
- [ ] Zoom to 200%, drag to see different parts
- [ ] Verify image cannot be dragged past edges
- [ ] Open Markdown, zoom to 150%
- [ ] Verify text is sharp and reflows
- [ ] Verify Escape key closes both viewers
- [ ] Verify arrow keys scroll Markdown viewer
- [ ] Verify GIF animation during pan

## Dependencies

### External Dependencies

None - uses only browser standard APIs.

### Internal Dependencies

**Implementation Order**:
1. Phase 1: ZoomController API (no dependencies)
2. Phase 2: ImageViewer changes (depends on Phase 1)
3. Phase 3: Markdown changes (depends on Phase 1)
4. Phase 4: Testing (depends on Phases 1, 2, 3)

**Component Dependencies**:
- `PanController` depends on `ImageViewer` canvas and overlay references
- `ImageViewer` depends on extended `ZoomController`
- `FullscreenMarkdownView` depends on extended `ZoomController`

## Risk Assessment

### Technical Risks

1. **Canvas Dimension Browser Limits**
   - **Risk**: Large zoom levels may exceed browser canvas limits
   - **Likelihood**: Low
   - **Impact**: Medium (image display fails)
   - **Mitigation**: Clamp canvas dimensions to safe maximum (e.g., 16384px)

2. **Pan Performance with Large Images**
   - **Risk**: Pan may lag with very large images
   - **Likelihood**: Low
   - **Impact**: Medium (poor UX)
   - **Mitigation**: Use requestAnimationFrame, canvas is already pre-rendered

3. **CSS Relative Units Not Used**
   - **Risk**: Some Markdown CSS may use fixed pixels, breaking reflow
   - **Likelihood**: Medium
   - **Impact**: Low (minor visual issues)
   - **Mitigation**: Audit and fix CSS in implementation

### Implementation Risks

1. **Breaking Existing Zoom Behavior**
   - **Risk**: Changes may break existing zoom functionality
   - **Likelihood**: Low (using optional callbacks)
   - **Impact**: High
   - **Mitigation**: All new options optional, extensive testing

## Performance Considerations

1. **Zoom Operations**
   - Direct property assignment (width/height, font-size) is fast
   - No need for requestAnimationFrame for single zoom operations
   - Target: < 16ms

2. **Pan Operations**
   - Use requestAnimationFrame for smooth movement
   - Single transform property update per frame
   - Throttle mousemove handling
   - Target: < 16ms per frame (60fps)

3. **Font-Size Zoom**
   - Browser handles text reflow automatically
   - May be slightly slower on very long documents
   - Acceptable performance tradeoff for sharp text

## Security Considerations

1. **Input Validation**
   - Zoom levels clamped to valid range (25-400%)
   - Pan offsets clamped to calculated bounds
   - Font-size clamped to reasonable range (4-64px)

2. **No External Data**
   - All operations are client-side
   - No user data transmitted or stored
   - Uses only standard browser APIs

## Open Questions

### From Specification

None - all requirements have been confirmed.

### Implementation-Specific

None - implementation approach is clear.

## Future Enhancements

Items not in current scope:

- Zoom to cursor position (mouse position as origin)
- Pinch-to-zoom gesture support
- Keyboard-based pan (arrow keys in image viewer)
- Remember zoom level per image

## Success Metrics

### Functional Completeness
- [ ] All functional requirements FR1-FR8 implemented
- [ ] All acceptance criteria for US1-US3 met
- [ ] All test scenarios pass

### Quality Metrics
- [ ] Test coverage 90%+ for new code
- [ ] No critical bugs in manual testing
- [ ] TypeScript strict mode compliance

### Performance Metrics
- [ ] Zoom operation < 16ms
- [ ] Pan operation < 16ms (60fps)
- [ ] No visible lag in UI interactions

### User Experience
- [ ] Intuitive pan with grab cursor
- [ ] Sharp text in Markdown at all zoom levels
- [ ] Correct fit percentage displayed

## References

- **Specification**: `doc/tasks/viewer-zoom-fix/SPEC.md`
- **Requirements Document**: `doc/tasks/viewer-zoom-fix/要件定義書.md`
- **Existing Components**:
  - `src/shared/zoom-controller.ts`
  - `src/image-viewer/index.ts`
  - `src/markdown/fullscreen.ts`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Stakeholder review
   - Confirm approach and timeline

2. **Begin Implementation**
   - Start with Phase 1 (ZoomController API)
   - Follow TDD approach where practical
   - Commit incrementally

3. **Verification**
   - Run `/sdd.3-verify-plan` for plan validation
   - Run `/sdd.6-verify` after implementation
