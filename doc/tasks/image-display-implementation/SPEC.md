# Feature: Image Display Implementation (Fullscreen Viewer)

## Overview

Complete the image display functionality in emterm using a **fullscreen viewer** approach (similar to the Markdown viewer). The backend (Rust) correctly outputs Kitty Graphics Protocol sequences via `emterm image` command, decodes the image data, and sends it to the frontend. The frontend displays the image in a fullscreen overlay viewer.

**Key Design Decision**: Instead of inline image display at cursor position, images are displayed in a fullscreen viewer. This:
- Eliminates cursor position tracking complexity
- Provides consistent UX with Markdown viewer
- Enables future inline display as an extension
- Maintains compatibility with external tools (viu, img2sixel, etc.)

## Objectives

- Decode Kitty Graphics Protocol images in the backend and display in fullscreen viewer
- Decode SIXEL images in the backend and display in fullscreen viewer
- Implement IPC event flow from backend ImageProcessor to frontend ImageViewer
- Support animated images (GIF, APNG) in the viewer
- Ensure both protocols work with external tools (timg, viu, img2sixel, etc.)

## User Stories

### US1: Display Image via CLI Command
As a user, I want to display an image using `emterm image <file>`, so that I can view images directly in my terminal.

**Acceptance Criteria:**
- [ ] `emterm image photo.png` opens fullscreen viewer with the PNG image
- [ ] `emterm image animation.gif` displays and plays the animated GIF in viewer
- [ ] Press Escape to close viewer and return to terminal
- [ ] Unsupported formats show clear error messages

### US2: External Tool Compatibility
As a CLI tool developer, I want emterm to support Kitty Graphics Protocol, so that my tools can display images.

**Acceptance Criteria:**
- [ ] Query command returns "OK" indicating protocol support
- [ ] Tools like `viu`, `timg` successfully display images in viewer
- [ ] Chunked image transfers work correctly

### US3: SIXEL Support
As a user of legacy tools, I want emterm to support SIXEL graphics, so that SIXEL-based tools work.

**Acceptance Criteria:**
- [ ] `img2sixel` successfully displays images in viewer
- [ ] SIXEL color palettes are correctly interpreted
- [ ] SIXEL aspect ratios are respected

## Technical Requirements

### Functional Requirements

- **FR1:** Process APC sequences containing Kitty Graphics commands and emit ImageEvents
- **FR2:** Process DCS sequences containing SIXEL data and emit ImageEvents
- **FR3:** Receive ImageEvents via Tauri IPC and open ImageViewer
- **FR4:** Support all Kitty actions: Transmit, TransmitAndDisplay, Put, Delete, Query
- **FR5:** Support SIXEL graphics with full color palette support
- **FR6:** Handle animation events (FrameReady, StateChanged, Completed)
- **FR7:** Support chunked image transfers for large images
- **FR8:** Close viewer with Escape key and return to terminal

### Non-Functional Requirements

- **NFR1 - Performance:** Image decode < 100ms for images under 1MB
- **NFR2 - Performance:** Maintain 60fps rendering during animation playback
- **NFR3 - Memory:** Image cache limited to 50MB (configurable)
- **NFR4 - Memory:** Individual image size limit of 100MB (configurable)
- **NFR5 - Reliability:** Graceful error handling for malformed data

## Implementation Approach

### Architecture

**Current Architecture (Incomplete):**
```
┌─────────────────────────────────────────────────────────────────┐
│                         PTY Output                               │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ANSI Parser (Rust)                            │
│   - Parses APC (Kitty) sequences                                 │
│   - Parses DCS (SIXEL) sequences                                 │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                  ImageProcessor (Rust)                           │
│   - KittyHandler: decodes Kitty commands                         │
│   - SixelHandler: decodes SIXEL data                             │
│   - Emits ImageEvent (ImageReady, Place, Delete, etc.)           │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼ [MISSING: IPC Event Emission]
┌─────────────────────────────────────────────────────────────────┐
│                  Frontend (TypeScript)                           │
│   - TerminalState.handleApc() - receives but does nothing        │
│   - TerminalState.handleDcs() - receives but does nothing        │
└─────────────────────────────────────────────────────────────────┘
```

**Target Architecture (Fullscreen Viewer):**
```
┌─────────────────────────────────────────────────────────────────┐
│                         PTY Output                               │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ANSI Parser (Rust)                            │
│   - Parses APC → KittyCommand                                    │
│   - Parses DCS → SixelData                                       │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│               ImageProcessor (Rust) [EXISTS]                     │
│   - process_kitty_command() → Vec<ImageEvent>                    │
│   - process_sixel() → Vec<ImageEvent>                            │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│              Tauri IPC (image_event) [NEW]                       │
│   - Emit "image_event" to frontend                               │
│   - Payload: ImageEvent (serialized)                             │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│             Frontend Event Handler [NEW]                         │
│   - Listen for "image_event"                                     │
│   - Open ImageViewer overlay (like Markdown viewer)              │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                  ImageViewer [NEW/MODIFY]                        │
│   - Fullscreen overlay component                                 │
│   - Display decoded image                                        │
│   - Handle animations                                            │
│   - Close on Escape key                                          │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

**Kitty Graphics Protocol Flow (Fullscreen Viewer):**
```
1. Tool/Command sends: ESC_G a=T,f=100;BASE64_DATA ESC\
2. PTY receives escape sequence
3. ANSI Parser extracts APC payload → KittyCommand
4. ImageProcessor.process_kitty_command()
   - Decodes base64 → raw bytes
   - Decompresses (if zlib)
   - Decodes PNG/converts RGB/validates RGBA
   - Returns Vec<ImageEvent>
5. Emit image_event via Tauri IPC
6. Frontend receives ImageEvent
7. On ImageReady/Place event:
   - Open fullscreen ImageViewer overlay
   - Display decoded image
   - Handle animation if applicable
8. User presses Escape → close viewer, return to terminal
```

**SIXEL Flow (Fullscreen Viewer):**
```
1. Tool sends: ESC P [params] q [sixel_data] ESC\
2. PTY receives DCS sequence
3. ANSI Parser extracts → SixelData
4. ImageProcessor.process_sixel()
   - Parses SIXEL data
   - Builds color palette
   - Converts to RGBA pixels
   - Returns Vec<ImageEvent>
5. Emit image_event via Tauri IPC
6. Frontend receives ImageEvent
7. Open fullscreen ImageViewer overlay
8. User presses Escape → close viewer
```

### API Design

#### Backend: ImageEvent IPC Emission

**Event Name:** `image_event`

**Payload:**
```typescript
type ImageEventPayload = {
  session_id: string;
  event: ImageEvent;
};

type ImageEvent =
  | { type: "ImageReady"; image: DecodedImage }
  | { type: "Place"; placement: ImagePlacement }
  | { type: "Delete"; target: ImageDeleteTarget }
  | { type: "QueryResponse"; supported: boolean }
  | { type: "Response"; data: string }
  | { type: "Animation"; data: AnimationEvent };
```

**DecodedImage:**
```typescript
interface DecodedImage {
  id: number;
  width: number;
  height: number;
  rgba_base64: string;  // Base64-encoded RGBA pixels
}
```

**ImagePlacement (simplified for viewer):**
```typescript
interface ImagePlacement {
  image_id: number;
  placement_id: number;
  // Note: row/col not used for fullscreen viewer
  // Kept for future inline display support
  row: number;
  col: number;
}
```

**ImageDeleteTarget (Kitty Delete command targets):**

Delete command format: `ESC_G a=d,d=<target>[,i=<id>][,p=<placement>] ESC\`
- `a=d` specifies the delete action
- `d=<target>` specifies what to delete (see mapping below)

| `d=` value | ImageDeleteTarget type | Description |
|------------|------------------------|-------------|
| `d=a` | `All` | Delete all images visible on screen |
| `d=A` | `AllIncludingHidden` | Delete all images including hidden |
| `d=i` | `ById` | Delete image by `i=` image_id |
| `d=n` | `ByPlacement` | Delete placement by `p=` placement_id |
| `d=c` | `AtCursor` | Delete images intersecting cursor |
| `d=f` | `AtCursorByColumns` | Delete images at cursor column range |
| `d=p` | `AtPosition` | Delete images at pixel position |
| `d=q` | `AtCell` | Delete images at cell position |
| `d=x` | `ByZIndex` | Delete images with specific z-index |
| `d=y` | `ByZIndex` | Delete images at z-index and column |
| `d=z` | `ByZIndex` | Delete images at z-index and column range |

```typescript
type ImageDeleteTarget =
  | { type: "All" }                                    // d=a
  | { type: "AllIncludingHidden" }                    // d=A
  | { type: "ById"; image_id: number }                // d=i,i=<id>
  | { type: "ByPlacement"; image_id?: number; placement_id: number }  // d=n,p=<pid>
  | { type: "AtCursor" }                              // d=c
  | { type: "AtCursorByColumns" }                     // d=f
  | { type: "AtPosition"; x?: number; y?: number }    // d=p
  | { type: "AtCell"; row?: number; col?: number }    // d=q
  | { type: "ByZIndex"; z_index: number };            // d=x/y/z
```

**AnimationEvent (for animated GIF/APNG):**
```typescript
type AnimationEvent =
  | { type: "FrameReady"; image_id: number; frame_index: number; rgba_base64: string; delay_ms: number }
  | { type: "StateChanged"; image_id: number; state: "Playing" | "Paused" | "Stopped" }
  | { type: "Completed"; image_id: number; loop_count: number };
```

#### Frontend: ImageViewer Component

```typescript
// In src/image-viewer/index.ts (similar to markdown-viewer)

export class ImageViewer {
  private overlay: HTMLElement;
  private canvas: HTMLCanvasElement;
  private currentImage: DecodedImage | null = null;
  private animationController: AnimationController | null = null;

  constructor(container: HTMLElement) {
    this.overlay = this.createOverlay();
    this.canvas = this.createCanvas();
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
    this.stopAnimation();
    this.currentImage = null;
  }

  private setupKeyboardHandler(): void {
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.isVisible()) {
        this.hide();
      }
    });
  }

  isVisible(): boolean {
    return this.overlay.classList.contains("visible");
  }

  async handleAnimationEvent(event: AnimationEvent): Promise<void> {
    // Handle animation frame updates
  }
}
```

#### Frontend: Event Handler

```typescript
// In src/terminal-app/index.ts

export class TerminalApp {
  private imageViewer: ImageViewer | null = null;

  async initialize() {
    // ... existing initialization

    // Setup image viewer (like markdown viewer)
    this.imageViewer = new ImageViewer(this.container);

    // Listen for image events
    await this.setupImageEventListener();
  }

  private async setupImageEventListener() {
    const { listen } = await import("@tauri-apps/api/event");

    await listen<{ session_id: string; event: ImageEvent }>(
      "image_event",
      async (event) => {
        if (event.payload.session_id !== this.sessionId) return;
        if (!this.imageViewer) return;

        await this.handleImageEvent(event.payload.event);
      }
    );
  }

  private async handleImageEvent(event: ImageEvent) {
    switch (event.type) {
      case "ImageReady":
        // Store image for later display
        this.pendingImage = event.image;
        break;
      case "Place":
        // Open viewer with pending or cached image
        if (this.pendingImage) {
          await this.imageViewer!.show(this.pendingImage);
          this.pendingImage = null;
        }
        break;
      case "Delete":
        // Close viewer if showing deleted image
        if (this.imageViewer!.isVisible()) {
          this.imageViewer!.hide();
        }
        break;
      case "Animation":
        await this.imageViewer!.handleAnimationEvent(event.data);
        break;
      case "QueryResponse":
        console.debug("Image query response:", event.supported);
        break;
    }
  }
}
```

### File Structure

```
src-tauri/src/
├── pty/
│   └── session.rs      # Add image_event emission
├── image/
│   ├── mod.rs          # ImageProcessor (exists)
│   ├── kitty.rs        # KittyHandler (exists)
│   ├── sixel.rs        # SixelHandler (exists)
│   └── ...

src/
├── terminal-app/
│   ├── index.ts        # Add image event listener setup
│   └── ...
├── image-viewer/       # NEW: Similar to markdown-viewer
│   ├── index.ts        # ImageViewer component
│   ├── styles.css      # Fullscreen overlay styles
│   └── animation.ts    # Animation handling
├── image/
│   ├── layer.ts        # ImageLayer (keep for future inline)
│   ├── types.ts        # Types (exists)
│   └── ...
├── terminal/
│   └── state.ts        # Remove placeholder APC/DCS handlers
```

### Integration Points

**1. PTY Session (Rust) - Emit ImageEvents:**

```rust
// In src-tauri/src/pty/session.rs

fn handle_terminal_action(&mut self, action: TerminalAction) {
    match action {
        TerminalAction::Apc(ApcAction::KittyGraphics(cmd)) => {
            let events = self.image_processor.process_kitty_command(&cmd);
            for event in events {
                self.emit_image_event(event);
            }
        }
        TerminalAction::Dcs(DcsAction::Sixel(sixel)) => {
            let events = self.image_processor.process_sixel(&sixel);
            for event in events {
                self.emit_image_event(event);
            }
        }
        // ... other actions
    }
}

fn emit_image_event(&self, event: ImageEvent) {
    if let Some(app_handle) = &self.app_handle {
        let _ = app_handle.emit_all("image_event", ImageEventPayload {
            session_id: self.id.clone(),
            event,
        });
    }
}
```

**2. TerminalState - Remove Placeholder Handlers:**

```typescript
// In src/terminal/state.ts

private handleApc(action: ApcAction): void {
    // Processing now happens in backend, events come via IPC
    // Viewer is opened by TerminalApp, not TerminalState
    switch (action.action) {
        case "KittyGraphics":
            console.debug("Kitty Graphics command received (processed by backend)");
            break;
        case "Unknown":
            break;
    }
}

private handleDcs(action: DcsAction): void {
    // Processing now happens in backend, events come via IPC
    switch (action.action) {
        case "Sixel":
            console.debug("SIXEL data received (processed by backend)");
            break;
        case "Unknown":
            break;
    }
}
```

## Test Scenarios

### Unit Tests

- [ ] ImageViewer.show() displays image in overlay
- [ ] ImageViewer.hide() closes overlay
- [ ] ImageViewer responds to Escape key
- [ ] AnimationController handles frame events correctly

### Integration Tests

- [ ] Kitty Query command returns OK response
- [ ] Kitty Transmit stores image without display
- [ ] Kitty TransmitAndDisplay stores and opens viewer
- [ ] Kitty Put opens viewer with previously stored image
- [ ] Kitty Delete closes viewer if showing deleted image
- [ ] SIXEL sequence opens viewer with decoded image
- [ ] Chunked transfer assembles image correctly

### E2E Tests

- [ ] `emterm image photo.png` opens viewer with PNG
- [ ] `emterm image animation.gif` plays GIF animation in viewer
- [ ] Press Escape closes viewer and returns to terminal
- [ ] External tool (viu/timg) successfully opens viewer
- [ ] img2sixel successfully opens viewer with SIXEL image

### Edge Cases

- [ ] 1x1 pixel image displays correctly in viewer
- [ ] Maximum size image (100MB) is rejected with error
- [ ] Malformed base64 data returns EINVAL error
- [ ] Corrupted PNG data returns decode error
- [ ] Multiple rapid image events queue properly

### Performance Tests

- [ ] 1MB PNG decodes in < 100ms
- [ ] Animation maintains 60fps in viewer
- [ ] Viewer opens within 50ms after image ready

## Security Considerations

- **Input Validation:** All image data must be validated before processing
  - Maximum image dimensions (e.g., 16384x16384)
  - Maximum data size (configurable, default 100MB)
  - Valid base64 encoding
- **Memory Protection:**
  - Image cache limited to 50MB
  - Automatic eviction of old images when limit reached
- **Error Isolation:** Malformed image data should not crash the application

## Error Handling

### Error Codes (Kitty Protocol)

| Code | Description | HTTP-like | User Message |
|------|-------------|-----------|--------------|
| EINVAL | Invalid parameters | 400 | "Invalid image data or parameters" |
| ENOENT | Image/placement not found | 404 | "Image not found" |
| ENOSPC | Storage quota exceeded | 507 | "Image cache full" |
| EFAILED | General failure | 500 | "Failed to process image" |

### Error Response Format

```
ESC_G i=<image_id>;ERROR:<code> ESC\
```

Example: `ESC_G i=42;ERROR:EINVAL ESC\`

## Success Criteria

- [ ] All functional requirements (FR1-FR8) implemented
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] E2E test: `emterm image photo.png` opens fullscreen viewer
- [ ] E2E test: Press Escape closes viewer
- [ ] E2E test: `viu photo.png` opens viewer (Kitty protocol)
- [ ] E2E test: `img2sixel photo.png` opens viewer (SIXEL)
- [ ] Performance: decode < 100ms for 1MB images
- [ ] Performance: animation maintains 60fps in viewer
- [ ] Memory usage stays within 50MB cache limit

## Implementation Phases

### Phase 1: Backend IPC Emission
**Goals:** Enable ImageEvent emission from PTY session
**Deliverables:**
- Add `emit_image_event()` to PTY session
- Call ImageProcessor in APC/DCS handlers
- Emit events via Tauri IPC

### Phase 2: Frontend ImageViewer Component
**Goals:** Create fullscreen viewer overlay
**Deliverables:**
- Create ImageViewer component (similar to MarkdownViewer)
- Implement show/hide with Escape key
- Handle animation playback
- Add image event listener in TerminalApp

### Phase 3: Integration and Testing
**Goals:** Verify end-to-end functionality
**Deliverables:**
- Integration tests for Kitty protocol
- Integration tests for SIXEL
- E2E tests with external tools
- Performance benchmarks

### Phase 4: Polish and Documentation
**Goals:** Production-ready implementation
**Deliverables:**
- Error message improvements
- Debug logging
- User documentation update

## Open Questions

- [ ] Should viewer support zoom/pan for large images?
- [ ] Should viewer show image metadata (dimensions, format)?
- [ ] Should we expose image cache settings in the UI?

## Future Extensions

- **Inline Display Mode**: Add `--inline` flag to display images at cursor position
- **Image Gallery**: Support multiple images with navigation
- **Image Manipulation**: Zoom, rotate, export functionality

## References

- [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
- [SIXEL Graphics Extension](https://en.wikipedia.org/wiki/Sixel)
- Existing code: `src-tauri/src/image/`, `src/image/`
- Related: `src/markdown-viewer/` (similar fullscreen overlay pattern)
- Related tasks: `doc/tasks/image-display/`, `doc/tasks/cli-display-commands/`
