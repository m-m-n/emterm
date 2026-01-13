# Implementation Plan: Image Display Implementation (Fullscreen Viewer)

## Overview

Complete the image display functionality in emterm using a **fullscreen viewer** approach. The backend ImageProcessor decodes images and emits events via Tauri IPC. The frontend receives events and opens a fullscreen ImageViewer overlay (similar to the Markdown viewer). This eliminates cursor position tracking complexity and provides a consistent UX.

## Objectives

- Connect APC handler to ImageProcessor for Kitty Graphics Protocol images
- Connect DCS handler to ImageProcessor for SIXEL images
- Implement IPC event flow from backend to frontend
- Create fullscreen ImageViewer component
- Enable `emterm image` command and external tools (viu, timg, img2sixel) to display images

## Prerequisites

### Development Environment
- Rust 1.70+ (already installed for Tauri)
- Bun (already installed for frontend build)
- Tauri CLI (already installed)

### Dependencies
- All dependencies already exist in the project
- No new external dependencies required

### Knowledge Requirements
- Tauri IPC event emission (`AppHandle::emit`)
- Existing ImageProcessor API
- Existing MarkdownViewer pattern (for ImageViewer design)
- Kitty Graphics Protocol and SIXEL basics

## Architecture Overview

### Technology Stack
- **Language**: Rust (backend), TypeScript (frontend)
- **Framework**: Tauri (IPC)
- **Key Libraries**:
  - `tauri::Emitter` - IPC event emission
  - `@tauri-apps/api/event` - IPC event listening
  - Existing `ImageProcessor`, `AnimationController`

### Design Approach

The implementation follows the fullscreen viewer pattern used for Markdown:

1. Backend parses APC/DCS sequences (already done by ANSI parser)
2. Backend routes Kitty/SIXEL to ImageProcessor (already implemented)
3. **NEW**: Backend emits `image_event` via Tauri IPC
4. **NEW**: Frontend listens for `image_event` and opens ImageViewer
5. **NEW**: ImageViewer displays image in fullscreen overlay
6. User presses Escape to close viewer and return to terminal

### Component Interaction

```
PTY Output
    |
    v
ANSI Parser (existing)
    |
    +-- APC (KittyGraphics) --> ImageProcessor.process_kitty_command()
    |                              |
    +-- DCS (Sixel) -----------> ImageProcessor.process_sixel()
                                   |
                                   v
                            Vec<ImageEvent>
                                   |
                                   v
                            emit "image_event" [NEW]
                                   |
                                   v
                            Frontend listener [NEW]
                                   |
                                   v
                            ImageViewer.show(image) [NEW]
                                   |
                                   v
                            Fullscreen overlay display
                                   |
                                   v
                            User presses Escape → hide()
```

## Implementation Phases

### Phase 1: Backend IPC Event Emission

**Goal**: Enable ImageEvent emission from the reader thread when APC/DCS sequences contain image commands

**Files to Create**:
- None (all infrastructure exists)

**Files to Modify**:
- `src-tauri/src/lib.rs`:
  - Add ImageEventPayload struct
  - Modify reader thread to process image actions
  - Add ImageProcessor state to reader thread

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ImageEventPayload | Wrap ImageEvent with session_id for IPC | ImageEvent exists | Serializable for IPC |
| ImageProcessor (instance) | Process Kitty/SIXEL commands | Instantiated per session | Returns Vec<ImageEvent> |
| Reader thread (modified) | Detect image actions, call processor, emit events | Parser yields ApcAction/DcsAction | `image_event` emitted to frontend |

**Processing Flow**:
```
1. Reader thread receives parsed actions from ANSI parser
2. For each action:
   +-- ApcAction::KittyGraphics
   |     +-- Call ImageProcessor.process_kitty_command(cmd)
   |     +-- For each returned ImageEvent, emit "image_event"
   |
   +-- DcsAction::Sixel
   |     +-- Call ImageProcessor.process_sixel(sixel)
   |     +-- For each returned ImageEvent, emit "image_event"
   |
   +-- Other actions: process as before
```

**Async Decoding Design**:

Image decoding is CPU-intensive and must not block the reader thread. The implementation uses background thread offloading:

```
Reader Thread (fast path)              Background Thread (CPU-bound)
     |                                        |
     +-- Receive APC/DCS sequence             |
     |                                        |
     +-- Store raw image data                 |
     |                                        |
     +-- Spawn/dispatch decode task --------> +-- Decode image (PNG/GIF/SIXEL)
     |                                        |
     +-- Continue processing PTY              +-- Generate ImageEvent
     |                                        |
     +-- (non-blocking)                       +-- emit "image_event" via AppHandle
```

Key principles:
- Reader thread MUST NOT perform synchronous image decoding
- Image data is passed to a dedicated decode thread/task via channel or spawn
- Decoded results are emitted directly from the background thread using `AppHandle` (thread-safe)
- This ensures terminal input/output remains responsive during large image processing
- Consider using `rayon` or Tokio spawn_blocking for parallel decode operations

**Implementation Steps**:

1. **Define ImageEventPayload**
   - Create IPC payload struct wrapping ImageEvent with session_id
   - Ensure Serialize derive for Tauri IPC compatibility

2. **Add ImageProcessor to reader thread**
   - Instantiate ImageProcessor when starting reader thread
   - Store mutable reference for processing image commands

3. **Process image actions in reader loop**
   - Match on ApcAction::KittyGraphics and DcsAction::Sixel
   - Call appropriate ImageProcessor method
   - Emit resulting ImageEvents via app.emit()

**Dependencies**:
- Requires: Existing ANSI parser, ImageProcessor, ImageEvent types
- Blocks: Phase 2 (frontend cannot receive events until backend emits)

**Testing Approach**:

*Unit Tests*:
- Test ImageEventPayload serialization
- Test ImageProcessor integration (existing tests cover this)

*Integration Tests*:
- Verify `emterm image` triggers image_event emission
- Verify event contains correct session_id

*Manual Testing*:
- [ ] Run `emterm image test.png` and check backend logs for event emission

**Acceptance Criteria**:
- [ ] ImageEventPayload struct defined and serializable
- [ ] Reader thread instantiates ImageProcessor
- [ ] APC KittyGraphics actions trigger ImageProcessor
- [ ] DCS Sixel actions trigger ImageProcessor
- [ ] ImageEvents emitted via "image_event" IPC channel

**Estimated Effort**: Small (1-2 days)

---

### Phase 2: Frontend ImageViewer Component and Event Listener

**Goal**: Create fullscreen ImageViewer and receive/route ImageEvents

**Files to Create**:
- `src/image-viewer/index.ts` - ImageViewer component
- `src/image-viewer/styles.css` - Fullscreen overlay styles
- `src/image-viewer/animation.ts` - Animation controller for GIF/animated image playback

**Files to Modify**:
- `src/terminal-app/index.ts`:
  - Add ImageViewer instantiation
  - Add image_event listener setup
  - Add event routing logic

- `src/terminal/state.ts`:
  - Simplify handleApc/handleDcs (optional cleanup)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ImageViewer | Fullscreen image display overlay | Container element available | Canvas created, Escape key handler |
| image_event listener | Receive and filter events | Session ID known | Route events to ImageViewer |
| Event router | Dispatch ImageEvent to show/hide viewer | ImageEvent received | ImageViewer method called |

**ImageViewer Design (Similar to MarkdownViewer)**:

```typescript
export class ImageViewer {
  private overlay: HTMLElement;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private currentImage: DecodedImage | null = null;

  constructor(container: HTMLElement) {
    this.overlay = this.createOverlay();
    this.canvas = this.createCanvas();
    this.overlay.appendChild(this.canvas);
    container.appendChild(this.overlay);
    this.setupKeyboardHandler();
  }

  async show(image: DecodedImage): Promise<void> {
    this.currentImage = image;
    await this.renderImage(image);
    this.overlay.classList.add("visible");
  }

  hide(): void {
    this.overlay.classList.remove("visible");
    this.currentImage = null;
  }

  isVisible(): boolean {
    return this.overlay.classList.contains("visible");
  }

  private setupKeyboardHandler(): void {
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.isVisible()) {
        this.hide();
      }
    });
  }
}
```

**Processing Flow**:
```
1. TerminalApp.init() creates ImageViewer
2. TerminalApp.init() registers image_event listener
3. Maintain pendingImages: Map<number, DecodedImage> for multi-chunk transfers
4. When image_event received:
   +-- Check session_id matches
   +-- Switch on event.type:
       +-- "ImageReady" --> pendingImages.set(image.id, image)
       +-- "Place" --> imageViewer.show(pendingImages.get(imageId)); pendingImages.delete(imageId)
       +-- "Delete" --> imageViewer.hide() if visible; pendingImages.delete(target.id) if applicable
       +-- "Animation" --> imageViewer.handleAnimation(event)
       +-- "QueryResponse" --> log for debugging
```

**Memory Optimization Note**:
- Base64 strings received from IPC should be decoded to `ImageData` immediately
- After decoding, the raw base64 string reference should be released (not retained)
- `pendingImages` stores decoded `DecodedImage` objects, not raw event payloads
- This ensures memory is not bloated by duplicate base64 + decoded data

**Implementation Steps**:

1. **Create ImageViewer component**
   - Create fullscreen overlay element (similar to MarkdownViewer)
   - Create canvas for image rendering
   - Implement show()/hide() methods
   - Setup Escape key handler

2. **Implement image rendering**
   - Decode Base64 RGBA data to ImageData
   - Draw to canvas with appropriate scaling
   - Center image in viewport

3. **Instantiate ImageViewer in TerminalApp**
   - Create ImageViewer after container is ready
   - Store reference for event handling

4. **Setup image_event listener**
   - Use @tauri-apps/api/event listen function
   - Filter by session_id to handle multi-session scenarios
   - Clean up listener in dispose()

5. **Implement event routing**
   - Switch statement based on ImageEvent.type
   - Open viewer on Place event
   - Close viewer on Delete event or Escape key

**Dependencies**:
- Requires: Phase 1 (backend must emit events)
- Blocks: Phase 3 (cannot test without frontend receiving)

**Testing Approach**:

*Unit Tests*:
- Test ImageViewer show/hide logic
- Test event routing logic (mock ImageViewer)
- Test session_id filtering

*Integration Tests*:
- Verify ImageReady + Place events open viewer
- Verify Escape key closes viewer

*Manual Testing*:
- [ ] Run `emterm image test.png` and see fullscreen viewer
- [ ] Press Escape to close viewer
- [ ] Run `viu test.png` (external Kitty tool) and see viewer
- [ ] Run `img2sixel test.png` (external SIXEL tool) and see viewer

**Acceptance Criteria**:
- [ ] ImageViewer component created with fullscreen overlay
- [ ] ImageViewer renders decoded image to canvas
- [ ] Escape key closes viewer
- [ ] image_event listener registered on init
- [ ] Listener filters by session_id
- [ ] ImageReady events store pending image
- [ ] Place events open viewer with image
- [ ] Delete events close viewer
- [ ] Listener cleaned up in dispose()

**Estimated Effort**: Small (1-2 days)

---

### Phase 3: Integration Testing and Verification

**Goal**: Verify end-to-end functionality with both protocols and external tools

**Files to Create**:
- `src/image-viewer/integration.test.ts` (optional, for automated tests)

**Files to Modify**:
- None (testing only)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| E2E test harness | Simulate full flow | App running | Images displayed in viewer |
| Performance benchmarks | Measure decode/render time | Test images available | Metrics captured |

**Processing Flow**:
```
1. Setup test environment with sample images
2. Test Kitty protocol:
   +-- emterm image PNG → viewer opens
   +-- emterm image GIF (animation) → viewer plays animation
   +-- viu tool compatibility
3. Test SIXEL protocol:
   +-- img2sixel tool compatibility
4. Test viewer interaction:
   +-- Escape key closes viewer
   +-- Multiple images queue properly
5. Test edge cases:
   +-- Large images
   +-- Malformed data
6. Measure performance metrics
```

**Implementation Steps**:

1. **Create test image set**
   - Small PNG (1x1, 100x100)
   - Large PNG (1000x1000)
   - Animated GIF
   - Various formats (JPEG, WebP if supported)

2. **Manual protocol testing**
   - Test emterm image command with each image type
   - Verify viewer opens and displays correctly
   - Test Escape key closes viewer
   - Test viu (Kitty) if available
   - Test img2sixel (SIXEL) if available

3. **Performance measurement**
   - Time from command to viewer open
   - Memory usage during large image transfer
   - Animation frame rate in viewer

4. **Edge case verification**
   - Corrupted image data handling
   - Maximum size limit enforcement
   - Multiple rapid image commands

**Dependencies**:
- Requires: Phase 1 and 2 complete
- Blocks: None (final phase)

**Testing Approach**:

*Manual Testing Checklist*:
- [ ] `emterm image photo.png` opens fullscreen viewer with PNG
- [ ] `emterm image animation.gif` plays animated GIF in viewer
- [ ] Press Escape closes viewer and returns to terminal
- [ ] Large image (>1MB) displays within performance limits
- [ ] Invalid image shows error message (not crash)
- [ ] External tool viu displays image in viewer
- [ ] External tool img2sixel displays image in viewer
- [ ] Rapid image commands don't crash

**Acceptance Criteria**:
- [ ] PNG images display correctly in viewer
- [ ] JPEG images display correctly in viewer
- [ ] Animated GIF plays correctly in viewer
- [ ] SIXEL images display correctly in viewer
- [ ] Escape key reliably closes viewer
- [ ] Large images handled gracefully
- [ ] Malformed data handled without crash
- [ ] Performance meets requirements (<100ms decode for 1MB)

**Estimated Effort**: Small (1 day)

---

## Complete File Structure

```
src-tauri/src/
+-- lib.rs                    # Add ImageEventPayload, modify reader thread
+-- image/
    +-- mod.rs                # ImageProcessor, ImageEvent (existing)
    +-- kitty.rs              # KittyHandler (existing)
    +-- sixel.rs              # SixelHandler (existing)
    +-- ...

src/
+-- terminal-app/
    +-- index.ts              # Add ImageViewer setup and event listener
+-- image-viewer/             # NEW: Fullscreen viewer component
    +-- index.ts              # ImageViewer class
    +-- styles.css            # Fullscreen overlay styles
    +-- animation.ts          # Animation controller for GIF/animated images
+-- image/
    +-- layer.ts              # ImageLayer (keep for future inline display)
    +-- types.ts              # TypeScript types (existing)
    +-- ...
+-- markdown-viewer/          # Reference: similar fullscreen pattern
    +-- ...
```

**File Descriptions**:
- `lib.rs`: Main Tauri backend, reader thread processes image actions
- `index.ts` (terminal-app): TerminalApp orchestrator, manages ImageViewer lifecycle
- `index.ts` (image-viewer): ImageViewer component, fullscreen overlay with canvas
- `styles.css`: CSS for fullscreen overlay (similar to markdown-viewer)
- `animation.ts`: Animation controller managing frame timing, playback state, and loop control for animated images (GIF, APNG, etc.)

## Testing Strategy

### Unit Testing

**Approach**:
- Use Rust's built-in `#[test]` for backend
- Use Bun test runner for frontend

**Test Coverage Goals**:
- Backend ImageEvent emission: 80%+
- Frontend ImageViewer: 80%+
- Event routing: 80%+

**Key Test Areas**:
1. **ImageEventPayload serialization** (`src-tauri/src/lib.rs`)
   - Verify JSON serialization matches frontend expectations
   - Test all ImageEvent variants

2. **ImageViewer** (`src/image-viewer/`)
   - Test show/hide visibility toggling
   - Test Escape key handler
   - Test canvas rendering

3. **Event routing** (`src/terminal-app/`)
   - Test each event type routes correctly
   - Test session_id filtering

### Integration Testing

**Scenarios**:
1. End-to-end image display via emterm image → viewer opens
2. Escape key closes viewer → terminal responsive
3. Kitty protocol Query response
4. SIXEL image display
5. Animation playback in viewer

### Manual Testing Checklist

Based on spec test scenarios:
- [ ] `emterm image photo.png` opens fullscreen viewer
- [ ] `emterm image animation.gif` displays and plays GIF
- [ ] Escape key closes viewer
- [ ] Large image (1MB PNG) displays within 100ms
- [ ] Corrupted image data shows error
- [ ] Kitty Query command returns OK
- [ ] Image deletion closes viewer if showing that image

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| @tauri-apps/api | ^2.0.0 | IPC event listening | Already installed |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 (backend emission - no frontend deps)
2. Phase 2 (frontend viewer - depends on Phase 1)
3. Phase 3 (testing - depends on Phases 1 & 2)

**Component Dependencies**:
- `ImageEventPayload` depends on `ImageEvent` (existing)
- `ImageViewer` depends on `DecodedImage` type
- Event routing depends on TypeScript `ImageEvent` types (existing)

## Risk Assessment

### Technical Risks

1. **Large Image IPC Transfer**
   - **Risk**: Large images cause memory issues during IPC transfer
   - **Likelihood**: Low (existing ImageProcessor handles limits)
   - **Impact**: Medium (potential crash)
   - **Mitigation**:
     - ImageProcessor already enforces size limits
     - Base64 encoding handled by existing infrastructure

2. **Session ID Mismatch**
   - **Risk**: Multi-session scenarios may route events incorrectly
   - **Likelihood**: Low (single session typical)
   - **Impact**: High (wrong terminal gets images)
   - **Mitigation**:
     - Always filter by session_id in frontend
     - Include session_id in all ImageEvents

### Implementation Risks

1. **Breaking Existing Functionality**
   - **Risk**: Changes to reader thread break terminal output
   - **Mitigation**: Minimal changes, add image handling alongside existing logic

## Performance Considerations

1. **IPC Overhead**
   - Large images encoded as Base64 (33% overhead)
   - Existing ImageProcessor handles this

2. **Canvas Rendering**
   - Decode Base64 → Uint8Array → ImageData → drawImage
   - Single render, no continuous updates needed

3. **Viewer Open/Close**
   - CSS visibility toggle (fast)
   - No DOM creation/destruction on each open

## Security Considerations

1. **Input Validation**
   - ImageProcessor already validates image data
   - Size limits enforced (100MB default)
   - Base64 decode errors handled gracefully

2. **Session Isolation**
   - session_id filtering prevents cross-session data leakage

## Open Questions

### Resolved:
- ~~How to track cursor position?~~ → Not needed for fullscreen viewer
- ~~Inline vs fullscreen display?~~ → Fullscreen viewer (Codex recommendation)

### Deferred:
- [ ] Should viewer support zoom/pan for large images? (Low priority)
- [ ] Should viewer show image metadata? (Low priority)
- [ ] Should we expose image cache settings in the UI? (Low priority)

## Future Enhancements

Items deferred to later phases:

### Potential Future Features:
- **Inline Display Mode**: Add `--inline` flag for cursor position display
- **Image Gallery**: Support multiple images with navigation
- **Zoom/Pan**: For large images
- **Image Metadata**: Show dimensions, format, size

## Success Metrics

### Functional Completeness
- [ ] emterm image command opens fullscreen viewer
- [ ] Escape key closes viewer
- [ ] Kitty protocol commands work
- [ ] SIXEL protocol works
- [ ] Animations play correctly in viewer

### Quality Metrics
- [ ] Existing tests continue to pass
- [ ] No regressions in terminal functionality
- [ ] Error handling works for malformed data

### Performance Metrics
- [ ] Image decode < 100ms for 1MB images
- [ ] Viewer opens within 50ms of image ready
- [ ] No visible lag in terminal input
- [ ] Animation maintains smooth playback

### User Experience
- [ ] Viewer opens reliably on image command
- [ ] Escape key reliably closes viewer
- [ ] External tools (viu, img2sixel) work
- [ ] Clear error messages for failures

## References

- **Specification**: `doc/tasks/image-display-implementation/SPEC.md`
- **Requirements**: `doc/tasks/image-display-implementation/要件定義書.md`
- **Kitty Graphics Protocol**: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- **Existing Code**:
  - Backend: `src-tauri/src/image/`
  - Frontend: `src/image/`
  - Markdown Viewer: `src/markdown-viewer/` (reference pattern)
  - Tauri IPC: `src-tauri/src/lib.rs`

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm fullscreen viewer approach
   - Review ImageViewer design

2. **Begin Implementation**
   - Start with Phase 1 (backend emission)
   - Follow TDD approach where applicable
   - Commit incrementally

3. **Verification**
   - Use `/sdd.5-check` to verify implementation matches plan
   - Use `/sdd.6-verify` to verify spec compliance
