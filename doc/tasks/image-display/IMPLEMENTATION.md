# Image Display Feature - Implementation Plan

## 1. Overview

### 1.1 Purpose

This document defines the implementation plan for inline image display functionality in eMterm. The implementation supports both Kitty Graphics Protocol and SIXEL protocol, enabling rich graphical content within the terminal emulator.

### 1.2 Scope

- **In Scope**: Terminal emulator-side image rendering
- **Out of Scope**: CLI `emterm image` command (separate branch)

### 1.3 Key Principles

- **SSOT**: This plan derives from SPEC.md; specification changes require plan updates
- **Incremental Delivery**: Each phase is independently testable and deployable
- **Non-blocking Design**: Image processing must not impact typing latency (<10ms)
- **Protocol Parity**: Kitty and SIXEL receive equal implementation priority

---

## 2. Implementation Phases

### 2.1 Phase 1: Core Infrastructure (Size: Large)

**Goal**: Establish foundational components for image sequence parsing and basic display.

**Dependencies**: None (starting phase)

**Deliverables**:
1. ANSI parser extension for APC and DCS sequences
2. Basic Kitty `a=t`, `a=T`, `a=p` action handlers
3. Basic SIXEL parsing and RGBA conversion
4. Frontend image layer with Canvas-based rendering
5. Tauri IPC for image data transfer

**Completion Criteria**:
- [ ] Parser correctly identifies and extracts APC sequences (`ESC _ G ... ESC \`)
- [ ] Parser correctly identifies and extracts DCS sequences (`ESC P ... ST`)
- [ ] Kitty `a=T` displays a PNG image at cursor position
- [ ] SIXEL data renders correctly with basic color support
- [ ] Image layer renders behind terminal text (z-index handling)
- [ ] All unit tests pass
- [ ] No regression in terminal input latency

### 2.2 Phase 2: Full Protocol Support (Size: Large)

**Goal**: Complete Kitty and SIXEL protocol implementation with resource management.

**Dependencies**: Phase 1

**Deliverables**:
1. All Kitty actions (`a=d`, `a=q`)
2. Chunked transfer support (`m` key)
3. Compression support (`o=z`)
4. Full SIXEL color specification (HLS/RGB, 256 colors)
5. SIXEL DECSDM mode support (`CSI ? 80 h/l`)
6. Memory quota management (320MB limit with LRU eviction)
7. Placement management system
8. Kitty error response generation (EINVAL, ENOENT, ENOSPC, etc.)
9. DoS prevention mechanisms (rate limiting, max concurrent decodes, timeouts)

**Completion Criteria**:
- [ ] Kitty `a=q` responds with protocol version
- [ ] Kitty `a=d` correctly removes images by various criteria
- [ ] Chunked transfers assemble correctly
- [ ] ZLIB-compressed payloads decompress correctly
- [ ] SIXEL renders with full HLS/RGB color support
- [ ] SIXEL DECSDM mode toggles correctly
- [ ] Memory quota enforced with LRU eviction
- [ ] Placement positions track correctly across scroll
- [ ] Error responses sent with correct format and codes
- [ ] Rate limiting prevents excessive image commands

### 2.3 Phase 3: Animation Support (Size: Medium)

**Goal**: Enable animated image display via Kitty protocol and GIF support.

**Dependencies**: Phase 2

**Deliverables**:
1. Kitty animation frames (`a=f`, `a=a`, `a=c`)
2. GIF animation decoding and playback
3. Animation state management (play/pause/stop)
4. Frame composition and timing control
5. Visibility-based animation pause

**Completion Criteria**:
- [ ] Kitty animation frames display with correct timing
- [ ] Animation playback controls work (s=1/2/3)
- [ ] GIF animations play with correct frame delays
- [ ] Off-screen animations pause automatically
- [ ] Frame composition blends correctly

### 2.4 Phase 4: Optimization (Size: Medium)

**Goal**: Improve rendering performance and user experience.

**Dependencies**: Phase 3

**Deliverables**:
1. WebGL-accelerated rendering path
2. Progressive image loading
3. Bitmap caching at common scales
4. Window resize handling with debounce
5. Performance monitoring integration

**Completion Criteria**:
- [ ] WebGL renderer active when supported
- [ ] Large images show progressive loading
- [ ] Resize operations complete within 100ms
- [ ] Frame render time stays under 16ms
- [ ] Performance metrics available via debug interface

---

## 3. Component Design

### 3.1 Rust Backend Components

#### 3.1.1 ANSI Parser Extension

**Location**: `src-tauri/src/ansi/`

**Responsibilities**:
- Recognize APC (`ESC _`) and DCS (`ESC P`) sequence starts
- Accumulate sequence data until String Terminator (ST)
- Parse Kitty control data (key=value pairs)
- Parse SIXEL parameters and data

**New Files**:
| File | Responsibility |
|------|----------------|
| `apc.rs` | APC sequence state machine and Kitty command parsing |
| `dcs.rs` | DCS sequence state machine and SIXEL data extraction |

**Modified Files**:
| File | Changes |
|------|---------|
| `parser.rs` | Add `ApcString` and `DcsString` states |
| `sequence.rs` | Add `ApcAction` and `DcsAction` variants to `TerminalAction` |
| `mod.rs` | Re-export new types |

**Contract**:
```
Input: Byte stream containing escape sequences
Output: TerminalAction::Apc(ApcAction) or TerminalAction::Dcs(DcsAction)
```

#### 3.1.2 Image Processor

**Location**: `src-tauri/src/image/`

**Responsibilities**:
- Decode image formats (PNG, JPEG, GIF, raw RGB/RGBA)
- Decompress ZLIB payloads
- Convert SIXEL to RGBA
- Manage chunk accumulation
- Enforce size/timeout limits

**New Files**:
| File | Responsibility |
|------|----------------|
| `mod.rs` | Module exports and ImageProcessor struct |
| `decoder.rs` | Format-specific decoders (PNG, JPEG, GIF, SIXEL) |
| `kitty.rs` | Kitty protocol command handling |
| `sixel.rs` | SIXEL parsing and RGBA conversion |
| `store.rs` | Image storage with quota management |
| `placement.rs` | Placement tracking and lifecycle |
| `animation.rs` | Animation frame and timing management |

**Contract**:
```
Input: Parsed ApcAction (Kitty) or DcsAction (SIXEL)
Output: ImageEvent sent to frontend via Tauri event
```

#### 3.1.3 Session Integration

**Location**: `src-tauri/src/pty/`

**Modified Files**:
| File | Changes |
|------|---------|
| `session.rs` | Route image-related TerminalActions to ImageProcessor |
| `manager.rs` | Maintain per-session ImageStore reference |

### 3.2 TypeScript Frontend Components

#### 3.2.1 Image Layer

**Location**: `src/image/`

**Responsibilities**:
- Receive image data from backend via Tauri events
- Manage Canvas element for image rendering
- Track image placements and visibility
- Handle z-index layering (above/below text)

**New Files**:
| File | Responsibility |
|------|----------------|
| `index.ts` | Module exports |
| `types.ts` | TypeScript interfaces for image data |
| `layer.ts` | ImageLayer class managing Canvas rendering |
| `placement.ts` | PlacementManager tracking image positions |
| `animation.ts` | AnimationController for frame timing |

**Contract**:
```
Input: ImageMessage events from Tauri IPC
Output: Rendered images on Canvas overlay
```

#### 3.2.2 Terminal Renderer Integration

**Location**: `src/terminal/`

**Modified Files**:
| File | Changes |
|------|---------|
| `renderer.ts` | Add image overlay container, integrate ImageLayer |
| `state.ts` | Track pending image placements |

#### 3.2.3 IPC Types

**Location**: `src/types/`

**New Files**:
| File | Responsibility |
|------|----------------|
| `image.ts` | ImageMessage, ImagePlacement, AnimationState types |

### 3.3 Tauri IPC Design

#### 3.3.1 Events (Backend to Frontend)

| Event Name | Payload | Description |
|------------|---------|-------------|
| `image_add` | `ImageAddPayload` | New image data to display |
| `image_remove` | `ImageRemovePayload` | Remove image(s) by criteria |
| `image_update` | `ImageUpdatePayload` | Update placement position |
| `animation_frame` | `AnimationFramePayload` | Next animation frame |

#### 3.3.2 Commands (Frontend to Backend)

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `image_query` | `session_id` | `ImageQueryResult` | Query current images |

---

## 4. File Structure

### 4.1 New Files

```
src-tauri/src/
  ansi/
    apc.rs              # APC sequence parsing
    dcs.rs              # DCS/SIXEL sequence parsing
  image/
    mod.rs              # Module entry point
    decoder.rs          # Image format decoders
    kitty.rs            # Kitty protocol handler
    sixel.rs            # SIXEL parser and converter
    store.rs            # Image storage with LRU
    placement.rs        # Placement manager
    animation.rs        # Animation controller

src/
  image/
    index.ts            # Module exports
    types.ts            # TypeScript interfaces
    layer.ts            # Canvas-based image layer (includes placement tracking)
    animation.ts        # Animation timing/control
    webgl-layer.ts      # WebGL-accelerated rendering (Phase 4)
    cache.ts            # Bitmap cache with LRU (Phase 4)
    resize-handler.ts   # Resize handler with debounce (Phase 4)
    performance.ts      # Performance monitoring (Phase 4)
```

### 4.2 Modified Files

```
src-tauri/src/
  ansi/
    parser.rs           # Add APC/DCS states
    sequence.rs         # Add ApcAction/DcsAction
    mod.rs              # Re-exports
  pty/
    session.rs          # Image processor integration
    manager.rs          # Per-session image store

src/
  terminal/
    renderer.ts         # Image layer integration
    state.ts            # Pending image tracking
  main.ts               # Image event handlers
```

---

## 5. Test Strategy

### 5.1 Unit Tests

#### Rust (cargo test)

| Component | Test Focus |
|-----------|------------|
| APC Parser | State transitions, sequence extraction, malformed input |
| DCS Parser | SIXEL parameter parsing, data accumulation |
| Kitty Parser | Key=value parsing, action dispatch |
| SIXEL Decoder | Color table, repeat codes, newlines |
| Image Store | Quota enforcement, LRU eviction, concurrent access |
| Placement | Position calculation, visibility tracking |

#### TypeScript (bun test)

| Component | Test Focus |
|-----------|------------|
| ImageLayer | Canvas operations, placement rendering |
| PlacementManager | Add/remove/update operations |
| AnimationController | Frame timing, pause/resume |

### 5.2 Integration Tests

| Test Scenario | Validation |
|---------------|------------|
| Kitty PNG display | Image appears at correct position |
| Kitty chunked transfer | Large image assembles correctly |
| SIXEL basic | 16-color image renders |
| SIXEL full color | 256-color image renders |
| Image deletion | Various `a=d` criteria work |
| Memory quota | LRU eviction triggers correctly |
| Scroll tracking | Images follow scroll position |
| Animation playback | GIF frames cycle correctly |

### 5.3 Compatibility Tests

| Tool/Library | Test Method |
|--------------|-------------|
| Kitty `icat` | Display test images via `icat` |
| libsixel `img2sixel` | Display converted images |
| `timg` | Display images via timg |
| `chafa` | Display via chafa with SIXEL |

### 5.4 Performance Tests

| Metric | Target | Test Method |
|--------|--------|-------------|
| Input latency | <10ms during decode | Keystroke timing benchmark |
| Decode time (100KB) | <50ms | Automated benchmark |
| Decode time (5MB) | <500ms | Automated benchmark |
| Frame render | <16ms | RAF timing measurement |
| Memory accuracy | Within 5% of quota | Memory profiling |

---

## 6. Verification Criteria

### 6.1 Phase 1 Verification

```markdown
## Parser Verification
- [ ] APC sequence `ESC _ G ... ESC \` parsed correctly
- [ ] DCS sequence `ESC P ... ST` parsed correctly
- [ ] Control data key=value pairs extracted
- [ ] Base64 payload decoded
- [ ] Invalid sequences handled gracefully

## Display Verification
- [ ] PNG image displays at cursor position
- [ ] Image respects z-index (below text by default)
- [ ] SIXEL basic palette renders correctly
- [ ] Canvas layer positioned correctly

## Performance Verification
- [ ] Typing latency unaffected during image decode
- [ ] No memory leaks after image display/removal
```

### 6.2 Phase 2 Verification

```markdown
## Protocol Verification
- [ ] All Kitty actions (t/T/p/d/q) functional
- [ ] Chunked transfer assembles correctly
- [ ] ZLIB decompression works
- [ ] SIXEL HLS color space supported
- [ ] SIXEL RGB color space supported

## Resource Management
- [ ] 320MB quota enforced
- [ ] LRU eviction removes oldest images
- [ ] Placement manager tracks all positions
- [ ] Response messages sent correctly
```

### 6.3 Phase 3 Verification

```markdown
## Animation Verification
- [ ] Kitty `a=f` frames display
- [ ] Kitty `a=a` controls work (s=1/2/3)
- [ ] GIF animation plays correctly
- [ ] Frame timing accurate
- [ ] Off-screen animations pause

## Composition Verification
- [ ] Alpha blending works
- [ ] Frame replacement mode works
- [ ] Background color handling correct
```

### 6.4 Phase 4 Verification

```markdown
## Optimization Verification
- [ ] WebGL renderer activates when available
- [ ] Progressive loading visible for large images
- [ ] Resize debounce prevents excessive renders
- [ ] Bitmap cache reduces repeated scaling
- [ ] Performance metrics accessible
```

### 6.5 Overall Completion Criteria

- [ ] All phase verification criteria met
- [ ] No regression in existing terminal functionality
- [ ] Kitty `icat` compatibility confirmed
- [ ] libsixel `img2sixel` compatibility confirmed
- [ ] Documentation updated
- [ ] All tests pass in CI

---

## 7. Risk Mitigation

### 7.1 Technical Risks

| Risk | Mitigation |
|------|------------|
| Parser complexity | Reuse existing state machine pattern from OSC |
| Memory pressure | Strict quota with aggressive eviction |
| Decode blocking | Use thread pool for async decoding |
| WebView limitations | Fallback to Canvas 2D if WebGL unavailable |

### 7.2 Compatibility Risks

| Risk | Mitigation |
|------|------------|
| Protocol variations | Test against multiple Kitty versions |
| SIXEL edge cases | Reference libsixel implementation |
| Platform differences | Test on macOS/Windows/Linux |

---

## 8. Reference Architecture

### 8.1 Data Flow

```
PTY Output
    |
    v
+----------------+
| ANSI Parser    |
| (parser.rs)    |
+----------------+
    |
    | ApcAction / DcsAction
    v
+----------------+
| Image Processor|
| (image/*.rs)   |
+----------------+
    |
    | Tauri Event (image_add)
    v
+----------------+
| Image Layer    |
| (layer.ts)     |
+----------------+
    |
    | Canvas Draw
    v
+----------------+
| WebView        |
+----------------+
```

### 8.2 Existing Pattern Reference

The implementation should follow patterns established in the Markdown feature:

| Markdown | Image |
|----------|-------|
| `OscAction::EmtermExtension` | `ApcAction::KittyGraphics` |
| `MarkdownSessionManager` | `ImageProcessor` |
| `MarkdownRenderer` | `ImageLayer` |
| `markdown-overlay` | `image-overlay` |

---

## 9. Implementation Notes

### 9.1 Parser State Machine Extension

The existing parser uses a clean state machine pattern. For image support:

1. Add `State::ApcString` state (similar to `OscString`)
2. Add `State::DcsString` state
3. APC: Accumulate after `ESC _` until `ESC \`
4. DCS: Accumulate after `ESC P` until `ST`

### 9.2 Thread Safety Considerations

- `ImageStore` requires `Arc<RwLock<>>` for concurrent access
- Decoder pool should use message passing, not shared state
- Frontend receives immutable data copies, no shared references

### 9.3 Canvas Layer Strategy

Following the Markdown overlay pattern:
- Create `image-overlay` div as sibling to terminal content
- Use absolute positioning with z-index management
- Canvas element sized to terminal viewport
- Images drawn at calculated pixel positions

### 9.4 Memory Management

- Track total RGBA bytes in `ImageStore`
- Implement LRU via `VecDeque<ImageId>`
- Eviction triggered before allocation, not after
- Animation frames count against quota

---

## 10. Appendix: Protocol Quick Reference

### 10.1 Kitty Sequence Format

```
ESC _ G <key>=<value>,<key>=<value>;[base64_payload] ESC \
```

### 10.2 SIXEL Sequence Format

```
ESC P [P1];[P2];[P3] q [sixel_data] ESC \
```

### 10.3 Common Kitty Keys

| Key | Description |
|-----|-------------|
| `a` | Action (t/T/p/d/q/f/a/c) |
| `t` | Transmission (d/f/t/s) |
| `f` | Format (24/32/100) |
| `i` | Image ID |
| `p` | Placement ID |
| `m` | More chunks (0/1) |
| `o` | Compression (z) |

---

## 11. Future Extensions

The following features are not included in the current implementation scope but may be considered for future versions:

### 11.1 Unicode Placeholder Mode (`U` parameter)

Kitty Graphics Protocol supports Unicode placeholder mode (`U=1`) which uses special Unicode characters to mark image positions. This enables better integration with terminal text selection and accessibility features.

**Consideration**: Requires coordination with text rendering layer and clipboard handling.

### 11.2 Additional Image Formats

- **WebP**: Animated WebP support for better compression
- **APNG**: Animated PNG support
- **SVG**: Vector graphics rendering

### 11.3 Advanced Features

- HDR image support
- Custom shader effects
- Video embedding (requires streaming protocol design)
