# Implementation Plan: Simple Image Viewer

## Overview

Replace the ImageViewer's multi-level zoom system (25%-400% with incremental controls) with a two-mode display: "Pixel Perfect" (100%) and "Fit to Window". Create a new DisplayModeController for ImageViewer while preserving ZoomController for MarkdownViewer.

## Objectives

- Remove incremental zoom controls (+/- keys, mouse wheel zoom, zoom in/out buttons) from ImageViewer
- Implement two display modes: Pixel Perfect (100%) and Fit to Window
- Maintain existing drag pan functionality for large images in Pixel Perfect mode
- Simplify the UI with a mode toggle bar instead of zoom bar
- Ensure no impact on MarkdownViewer's zoom functionality

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- TypeScript 5.x

### Dependencies
- No new external dependencies required
- Existing internal dependencies: PanController

### Knowledge Requirements
- Understanding of the existing ImageViewer and ZoomController architecture
- CSS transform-based scaling and centering

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Framework**: Vanilla TypeScript with DOM APIs
- **Key Libraries**:
  - No additional libraries required

### Design Approach

**Option B (Recommended)**: Create a new DisplayModeController specifically for ImageViewer, leaving ZoomController unchanged for MarkdownViewer.

Rationale:
- Cleaner separation of concerns
- No risk of affecting MarkdownViewer
- Simpler code with no mode flags or conditional logic
- Easier to maintain and test

### Responsibility Separation Principles

**DisplayModeController** is designed with focused, single-purpose responsibilities:

| Layer | Responsibility | What It Does NOT Do |
|-------|---------------|---------------------|
| **Mode State** | Manage 'pixel'/'fit' mode state | Does NOT control image rendering |
| **Scale Calculation** | Calculate scale values for each mode | Does NOT apply transforms |
| **UI Creation** | Create and manage mode bar DOM elements | Does NOT handle image display |
| **Keyboard Handling** | Process mode-related keyboard events | Does NOT handle pan/drag events |

**Design Intent**: DisplayModeController is intentionally limited to mode management and UI. It delegates actual image manipulation to ImageViewer (transform application) and PanController (drag handling). This ensures:

1. **Single Responsibility**: Each component has one clear purpose
2. **Testability**: Mode logic can be unit tested without DOM manipulation
3. **Extensibility**: Adding new modes (e.g., 'fit-width') only affects DisplayModeController
4. **Maintainability**: Changes to pan behavior don't affect mode logic and vice versa

**Boundary Enforcement**:
- DisplayModeController communicates via `onModeChange` callback only
- It never directly manipulates image transforms or pan state
- ImageViewer remains the coordinator that connects mode changes to visual updates

If future requirements demand additional features (zoom levels, rotation, etc.), consider creating separate controllers rather than expanding DisplayModeController.

### Component Interaction

```
ImageViewer
    |
    +-- DisplayModeController (NEW)
    |       - Manages 'pixel' vs 'fit' mode
    |       - Creates simplified UI (mode toggle bar)
    |       - Handles mode-specific keyboard shortcuts
    |
    +-- PanController (EXISTING - no changes)
            - Enable/disable based on mode and image size
```

## Implementation Phases

### Phase 1: DisplayModeController Core

**Goal**: Create the core DisplayModeController with mode state management and scale calculation

**Files to Create**:
- `src/image-viewer/display-mode.ts` - DisplayModeController implementation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DisplayModeController | Manage display mode state and UI | Overlay and image dimensions provided | Mode state maintained, UI rendered |
| DisplayMode type | Define mode states | - | Type-safe mode values ('pixel' \| 'fit') |
| calculateFitScale | Calculate scale for fit mode | Valid image and viewport dimensions | Scale value (never exceeds 100) |

**Processing Flow**:
```
1. Initialize DisplayModeController
   +-- Create mode state (default: 'pixel')
   +-- Store image and viewport dimensions
   +-- Calculate fit scale

2. Mode Toggle Request
   +-- pixel mode -> Switch to fit mode
   |       +-- Apply fit scale
   |       +-- Disable pan
   +-- fit mode -> Switch to pixel mode
           +-- Apply 100% scale
           +-- Condition: image > viewport -> Enable pan
           +-- Condition: image <= viewport -> Disable pan
   +-- Center image
   +-- Notify change callback
```

**Implementation Steps**:

1. **Define DisplayModeController interface and types**
   - Define DisplayMode type ('pixel' | 'fit')
   - Define DisplayModeState interface:
     ```typescript
     interface DisplayModeState {
       mode: DisplayMode;
       scale: number;  // Current scale (1.0 = 100%)
       fitScale: number;  // Pre-calculated fit scale
     }
     ```
   - Define DisplayModeControllerOptions interface:
     ```typescript
     interface DisplayModeControllerOptions {
       overlay: HTMLElement;  // Container element for UI
       imageWidth: number;  // Natural image width in pixels
       imageHeight: number;  // Natural image height in pixels
       viewportWidth: number;  // Available viewport width
       viewportHeight: number;  // Available viewport height
       initialMode?: DisplayMode;  // Default: 'pixel'
       onModeChange?: (state: DisplayModeState) => void;  // Mode change callback
       onClose?: () => void;  // Close button callback
     }
     ```
   - Define DisplayModeController public methods:
     ```typescript
     class DisplayModeController {
       toggle(): void;  // Toggle between pixel and fit mode
       setMode(mode: DisplayMode): void;  // Set specific mode
       getState(): DisplayModeState;  // Get current state
       updateViewport(width: number, height: number): void;  // Update viewport dimensions (for resize)
       dispose(): void;  // Cleanup resources
     }
     ```

2. **Implement mode state management**
   - Mode initialization (default: 'pixel')
   - Mode toggle logic
   - Scale calculation for each mode

3. **Implement scale calculation helpers**
   - Implement calculateFitScale for fit scale calculation
   - Ensure fit scale never exceeds 100% (no upscaling)
   - Clamp to minimum if image is extremely large

4. **Implement error handling**
   - Handle zero or negative dimensions gracefully (return 100% scale)
   - Validate callback functions before invocation
   - Log warnings for unexpected state transitions
   - Ensure cleanup on disposal to prevent memory leaks

**Dependencies**:
- Requires: None (standalone module)
- Blocks: Phase 2 (UI) and Phase 3 (Integration)

**Testing Approach**:

*Unit Tests*:
- Test mode toggle between 'pixel' and 'fit'
- Test fit scale calculation with various image/viewport ratios
- Test that fit scale never exceeds 100% for small images
- Test edge cases: zero dimensions, very large images

**Acceptance Criteria**:
- [ ] DisplayModeController exports DisplayMode type and class
- [ ] Mode toggles correctly between 'pixel' and 'fit'
- [ ] Fit scale is calculated correctly using viewport padding (95%)
- [ ] Small images are not upscaled beyond 100% in fit mode

**Estimated Effort**: Small (1-2 days)

---

### Phase 2: UI and Keyboard Controls

**Goal**: Add mode toggle UI and keyboard shortcuts to DisplayModeController

**Files to Modify**:
- `src/image-viewer/display-mode.ts` - Add UI creation and keyboard handling

**Files to Create**:
- `src/image-viewer/display-mode-styles.ts` - CSS styles for mode bar

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Mode Bar UI | Display current mode and toggle button | Controller initialized | UI elements in overlay |
| Keyboard Handler | Process mode-switch keyboard events | Viewer visible | Mode changed on valid key |

**Processing Flow**:
```
1. UI Creation
   +-- Create mode bar container
   +-- Create mode display label ("100%" or "Fit")
   +-- Create toggle button
   +-- Create close button
   +-- Append to overlay

2. Keyboard Event Handling
   +-- Key 'f' -> Toggle mode
   +-- Key '1' -> Switch to pixel mode
   +-- Key '0' -> Switch to fit mode
   +-- Key 'Escape' -> Close (delegate to onClose callback)
   +-- Other keys -> Block from shell
```

**Implementation Steps**:

1. **Create mode bar UI elements**
   - Mode bar container with current mode label
   - Toggle button for switching modes
   - Close button (reuse existing pattern)
   - Visual indication of current mode

2. **Implement keyboard shortcut handling**
   - 'f' key for toggle
   - '1' key for pixel mode
   - '0' key for fit mode
   - Block other keys from reaching shell

   **Keyboard Conflict Analysis**:
   - Existing ZoomController (used by MarkdownViewer) uses `0` for reset and `1` for 100%
   - New DisplayModeController (ImageViewer only) redefines these keys:
     - `0`: Switch to fit mode (vs. reset in ZoomController)
     - `1`: Switch to pixel mode / 100% (semantically similar - both mean "original size")
   - **Decision**: Accept the semantic difference for `0` key between viewers
     - ImageViewer: `0` = fit to window, `1` = 100%
     - MarkdownViewer: `0` = reset to initial zoom, `1` = 100%
   - **Rationale**: Different viewers have different use cases; consistency within each viewer is more important
   - `f` key is new and has no conflict

3. **Define CSS styles for mode bar**
   - Position: fixed, bottom-right
   - Similar styling to existing zoom bar but simplified
   - Active mode visual indicator

**Dependencies**:
- Requires: Phase 1 (core mode logic)
- Blocks: Phase 3 (integration)

**Testing Approach**:

*Unit Tests*:
- Test UI element creation
- Test keyboard event handling

*Integration Tests*:
- Test clicking mode button toggles display mode
- Test 'f' key toggles mode
- Test '1' key switches to pixel mode
- Test '0' key switches to fit mode
- Test other keys are blocked

**Acceptance Criteria**:
- [ ] Mode bar displays current mode ("100%" or "Fit")
- [ ] Toggle button switches between modes
- [ ] Close button works
- [ ] Keyboard shortcuts 'f', '1', '0' work correctly
- [ ] Other keys do not reach the shell

**Estimated Effort**: Small (1-2 days)

---

### Phase 3: ImageViewer Integration and Cleanup

**Goal**: Replace ZoomController with DisplayModeController in ImageViewer and remove incremental zoom code

**Files to Modify**:
- `src/image-viewer/index.ts`:
  - Replace ZoomController with DisplayModeController
  - Update PanController enable/disable logic
  - Remove incremental zoom handling
  - Update keyboard handling

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ImageViewer | Coordinate display mode and pan | Image loaded | Correct display and pan state |
| Pan Integration | Enable/disable pan based on mode | Mode and dimensions known | Pan enabled only when appropriate |

**Processing Flow**:
```
1. ImageViewer.show()
   +-- Decode and render image
   +-- Initialize DisplayModeController (mode: 'pixel')
   +-- Initialize PanController
   +-- Register resize handler
   +-- Calculate if pan should be enabled
       +-- Condition: pixel mode AND image > viewport -> Enable
       +-- Otherwise -> Disable

2. Mode Change (callback from DisplayModeController)
   +-- Update pan offset to center
   +-- Update pan controller bounds
   +-- Enable/disable pan based on new mode and size

3. Window Resize Handling
   +-- Event: 'resize' on window
   +-- Debounce: 100ms (avoid excessive recalculation)
   +-- Get new viewport dimensions
   +-- Recalculate fit scale: DisplayModeController.updateViewport(w, h)
   +-- If current mode is 'fit':
       +-- Apply new fit scale immediately
       +-- Re-center image
   +-- If current mode is 'pixel':
       +-- Update pan bounds based on new viewport
       +-- Clamp current pan offset if needed
   +-- Update UI mode bar position if needed

4. Cleanup (hide/dispose)
   +-- Remove resize event listener
   +-- Dispose DisplayModeController
   +-- Dispose PanController
   +-- Reset state
```

**Implementation Steps**:

1. **Replace ZoomController with DisplayModeController**
   - Remove ZoomController import and usage
   - Add DisplayModeController import and initialization
   - Connect mode change callback to pan and transform updates

2. **Update pan controller integration**
   - Enable pan only in pixel mode when image exceeds viewport
   - Disable pan in fit mode (image always fits)
   - Reset pan offset on mode change

3. **Remove incremental zoom code**
   - Remove +/- key handling
   - Remove wheel zoom handling (Ctrl+wheel)
   - Remove zoomIn/zoomOut method calls
   - Simplify applyImageZoom to use mode-based scale only

4. **Update keyboard handling**
   - Delegate mode keys to DisplayModeController
   - Keep Escape key for close
   - Block all other keys from shell

**Dependencies**:
- Requires: Phase 1 and Phase 2
- Blocks: None

**Testing Approach**:

*Integration Tests*:
- Test initial display is at 100% (pixel perfect)
- Test mode toggle via button and keyboard
- Test drag pan works in pixel mode for large images
- Test drag pan is disabled in fit mode
- Test +/- keys do not affect zoom
- Test Ctrl+wheel does not affect zoom
- Test Escape closes viewer
- Test window resize recalculates fit scale

*Edge Cases*:
- Image exactly same size as viewport
- Very small image (< 100px)
- Very large image (> 10000px)
- Animated images (GIF/APNG)

**Acceptance Criteria**:
- [ ] ImageViewer opens in pixel perfect mode (100%)
- [ ] Mode can be toggled via UI button
- [ ] Mode can be toggled via keyboard shortcuts
- [ ] Drag pan works for large images in pixel mode
- [ ] Drag pan is disabled in fit mode
- [ ] +/- keys have no effect
- [ ] Mouse wheel (Ctrl+wheel) has no effect
- [ ] Zoom in/out buttons are removed
- [ ] MarkdownViewer zoom functionality is unaffected

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Animation playback affected by mode changes
  - **Mitigation**: Test animation during mode toggle, ensure frame rendering continues

---

## Complete File Structure

```
src/
+-- image-viewer/
|   +-- index.ts              # ImageViewer (modify: replace ZoomController)
|   +-- index.test.ts         # ImageViewer tests (modify: add mode tests)
|   +-- pan-controller.ts     # PanController (no changes)
|   +-- pan-controller.test.ts # PanController tests (no changes)
|   +-- display-mode.ts       # NEW: DisplayModeController
|   +-- display-mode.test.ts  # NEW: DisplayModeController tests
|   +-- display-mode-styles.ts # NEW: Mode bar CSS styles
+-- shared/
    +-- zoom-controller.ts    # ZoomController (no changes - kept for MarkdownViewer)
    +-- zoom-styles.ts        # Zoom bar CSS (no changes)
```

**File Descriptions**:
- `display-mode.ts`: Core DisplayModeController class with mode state, scale calculation, UI creation, and keyboard handling
- `display-mode-styles.ts`: CSS styles for the mode bar (similar pattern to zoom-styles.ts)
- `display-mode.test.ts`: Unit tests for DisplayModeController
- Modified `index.ts`: ImageViewer using DisplayModeController instead of ZoomController
- Modified `index.test.ts`: Additional tests for two-mode display behavior

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test framework
- Test logic that does not require full DOM

**Test Coverage Goals**:
- DisplayModeController logic: 80%+ coverage
- Scale calculation: 90%+ coverage

**Key Test Areas**:
1. **DisplayModeController** (`display-mode.test.ts`)
   - Mode state transitions (pixel <-> fit)
   - Fit scale calculation with various ratios
   - Edge cases: zero dimensions, extreme sizes
   - Callback invocation on mode change

2. **calculateFitScale** (new function)
   - Fit scale calculation with various image/viewport ratios
   - No upscaling beyond 100%
   - Minimum zoom clamping

### Integration Testing

**Scenarios**:
1. Open image, verify 100% display
2. Toggle mode via button
3. Toggle mode via keyboard
4. Pan in pixel mode with large image
5. Verify pan disabled in fit mode
6. Verify incremental zoom is disabled

**Approach**:
- Manual testing in browser
- Verify with various image sizes

### Manual Testing Checklist

From spec test scenarios:
- [ ] Initial display is at 100% (pixel perfect)
- [ ] Clicking mode button toggles display mode
- [ ] Pressing 'f' key toggles display mode
- [ ] Pressing '1' key switches to pixel mode
- [ ] Pressing '0' key switches to fit mode
- [ ] Drag pan works in pixel mode for large images
- [ ] Drag pan is disabled in fit mode
- [ ] +/- keys do not affect zoom
- [ ] Mouse wheel does not affect zoom
- [ ] Escape closes viewer
- [ ] Close button closes viewer
- [ ] Image exactly same size as viewport: Both modes look identical
- [ ] Very small image: Displays at 100%, not upscaled in fit mode
- [ ] Very large image: Handles without performance issues
- [ ] Window resize: Fit scale recalculates

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| None | - | No new dependencies | - |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: DisplayModeController core (no dependencies)
2. Phase 2: UI and keyboard (depends on Phase 1)
3. Phase 3: ImageViewer integration (depends on Phases 1 & 2)

**Component Dependencies**:
- `display-mode.ts` is standalone (no internal dependencies)
- `index.ts` imports `display-mode.ts` and `pan-controller.ts`
- `shared/zoom-controller.ts` remains unchanged (used by MarkdownViewer only)

## Risk Assessment

### Technical Risks

1. **Animation Frame Rendering During Mode Switch**
   - **Risk**: Mode switch may interfere with animation playback
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Test with animated GIF/APNG, ensure frame timer continues

2. **Aspect Ratio Correction Compatibility**
   - **Risk**: Existing aspect ratio correction logic may need adjustment
   - **Likelihood**: Medium
   - **Impact**: Low
   - **Mitigation**: Verify with the existing constrainedBaseWidth/Height logic

3. **Window Resize Behavior**
   - **Risk**: Fit scale may not recalculate correctly on resize
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Reuse existing resize handler logic

### Implementation Risks

1. **MarkdownViewer Regression**
   - **Risk**: Accidentally affecting MarkdownViewer zoom
   - **Mitigation**: ZoomController is completely untouched; verify MarkdownViewer after changes

## Performance Considerations

1. **Mode Switch Performance**
   - Goal: < 100ms mode switch
   - Transform-based scaling is instant (no re-rendering)
   - No animation needed between modes

2. **Pan Smoothness**
   - Goal: 60fps during drag
   - Existing PanController already achieves this

## Security Considerations

1. **Input Validation**
   - Existing image data validation maintained
   - No new attack vectors introduced

2. **XSS Prevention**
   - No user-generated content in UI
   - Mode bar uses text content, not innerHTML

## Open Questions

### From Specification:
- [x] Keyboard shortcut keys: `f` (toggle), `1` (100%), `0` (fit) - confirmed in spec
- [ ] UI design preference: Single toggle button recommended (simpler)

### Implementation-Specific:
- [x] Initial mode: Pixel Perfect (100%) - confirmed in spec

## Success Metrics

### Functional Completeness
- [ ] All MVP features implemented (2 modes, toggle, pan, close)
- [ ] All test scenarios pass
- [ ] +/- keys and wheel zoom confirmed non-functional

### Quality Metrics
- [ ] Test coverage meets goals (80%+ core logic)
- [ ] No critical bugs in manual testing
- [ ] Code follows TypeScript best practices

### Performance Metrics
- [ ] Mode switch renders within 100ms
- [ ] Pan maintains 60fps smoothness

### Code Simplification Metrics
- [ ] Code reduction target: 10% or more in zoom-related code
  - Measurement: Compare line count of `src/image-viewer/index.ts` + `src/shared/zoom-controller.ts` (before) vs `src/image-viewer/index.ts` + `src/image-viewer/display-mode.ts` (after)
  - Baseline: Record current line count before implementation
  - Note: Net reduction expected due to removal of increment zoom logic, zoom buttons, wheel zoom handlers

### User Experience
- [ ] Single click/key toggles mode
- [ ] Current mode clearly indicated
- [ ] Help text in info display

## References

- **Specification**: `doc/tasks/simple-image-viewer/SPEC.md`
- **Requirements**: `doc/tasks/simple-image-viewer/要件定義書.md`
- **Existing implementation**: `src/image-viewer/index.ts`
- **Existing zoom controller**: `src/shared/zoom-controller.ts`
- **Pan controller**: `src/image-viewer/pan-controller.ts`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm UI design (single toggle button)
   - Confirm keyboard shortcuts

2. **Begin Implementation**
   - Start with Phase 1: DisplayModeController core
   - Write tests alongside implementation
   - Commit incrementally

3. **Verification**
   - Run `/sdd.3-verify-plan` for design review
   - Execute VERIFICATION.md checklist
