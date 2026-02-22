# Implementation Plan: Kitty Protocol Compatibility

## Overview

Improve eMterm's Kitty Graphics Protocol compatibility so external tools (treemd, kitten icat, ratatui-image) that rely on Kitty capability detection work correctly. Fix timing issues with query responses, add XTWINOPS device responses, ensure cell size propagation on buffer switch, and verify the Kitty image and animation pipeline.

## Objectives

- Fix Kitty query response timing (synchronous in WASM) so capability detection works
- Implement XTWINOPS (CSI 14t/16t/18t) for accurate cell size reporting
- Propagate cell size to alternate buffer cores on buffer switch
- Verify and fix Kitty image pipeline (kitten icat → image viewer overlay)
- Support Kitty animation frames (a=f, a=a)
- Eliminate treemd red background and warning messages

## Prerequisites

### Development Environment
- Rust toolchain with wasm-pack
- Bun runtime
- Tauri CLI
- Docker (for test execution)

### Dependencies
- WASM parser with APC/CSI dispatch (exists)
- Callback mechanism with device_response_callback (exists)
- Image pipeline: APC callback → Tauri IPC → Rust image handler (exists)
- ImageViewer overlay (exists)

## Architecture Overview

### Technology Stack
- **WASM (Rust)**: ANSI parser, grid state, device response generation
- **TypeScript**: Terminal UI, event handling, WASM integration, image viewer
- **Rust (Tauri backend)**: Image processing, Kitty protocol handler, animation

### Design Approach
- Synchronous device responses in WASM for timing-critical queries (Kitty query, DA1, CSI 16t, DSR)
- Asynchronous image pipeline for actual image data (APC → Tauri IPC → Rust decode → image_event)
- Cell size propagation at all points where WASM cores are created or become active

### Component Interaction

**Synchronous Response Path** (queries/device status):
1. PTY data arrives at WASM parser
2. Parser detects sequence type (APC query, CSI 16t, CSI c, CSI 5n)
3. Handler generates response into response_buffer
4. fire_device_response_callback sends response to PTY immediately

**Asynchronous Image Path** (actual image data):
1. PTY data arrives at WASM parser
2. Parser detects non-query APC → fire_apc_callback
3. TS queues APC data, then invokes Rust process_image_data
4. Rust decodes image, emits image_event
5. TS receives event, displays via ImageViewer overlay

## Implementation Phases

### Phase 1: Synchronous Responses (Implemented)

**Goal**: Fix Kitty query timing and add XTWINOPS support so ratatui-image capability detection succeeds

**Status**: Already implemented in current codebase

**Files Modified**:
- `wasm/src/apc_handler.rs` - Kitty query synchronous response via response_buffer
- `wasm/src/csi_device.rs` - XTWINOPS handlers (CSI 14t/16t/18t)
- `wasm/src/csi_dispatch.rs` - CSI 't' dispatch routing
- `wasm/src/terminal_core.rs` - cell_width_px/cell_height_px fields, set_cell_size_px()
- `src/terminal-app/index.ts` - set_cell_size_px calls at init and resize

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| apc_handler | Detect a=q in APC payload, generate synchronous response | Valid APC payload starting with 'G' | Response in response_buffer, callback fired |
| csi_device (XTWINOPS) | Generate CSI 14t/16t/18t responses using cell dimensions | cell_size_px set to actual values | Correct pixel/char size responses |
| csi_dispatch | Route CSI 't' final byte to XTWINOPS handlers | Parsed CSI params available | Appropriate handler called based on Ps |
| terminal_core | Store cell dimensions, provide set_cell_size_px API | None | cell_width_px/cell_height_px updated |

**Acceptance Criteria**:
- [x] Kitty query (a=q) returns synchronous response before DSR sentinel
- [x] CSI 16t returns correct cell size in pixels
- [x] CSI 14t returns correct text area size in pixels
- [x] CSI 18t returns correct text area size in characters
- [x] set_cell_size_px called at init and resize
- [x] All existing tests pass

**Estimated Effort**: Already complete

---

### Phase 2: Cell Size Sync and Pipeline Investigation

**Goal**: Ensure alternate buffer cores receive correct cell size, investigate and fix treemd red background and viewer not launching

**Files to Modify**:
- `src/terminal/state.ts` - Add cell size propagation to switchToAlternateBuffer
- `src/terminal-app/index.ts` - Ensure cell size set after any buffer switch in data handler

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| state.switchToAlternateBuffer | Propagate cell size to newly created/reset alternate core | charSize known from terminal app | Alternate core has correct cell_width_px/cell_height_px |
| index.ts data handler | Set cell size after buffer switch creates new core | Core change detected in while loop | New core has cell size set before processing continues |

**Processing Flow** (diagram-convertible):
1. Buffer switch mode action detected in data handler
   - Action is SWITCH_TO_ALT or SAVE_AND_SWITCH_TO_ALT → state.handleModeAction()
   - state creates alternate WASM core
2. Next iteration of while loop detects core change
   - Register callbacks on new core
   - Set cell size on new core from stored charSize
3. Remaining PTY data processed by alternate core with correct cell size
   - CSI 16t queries now return actual values, not defaults (8x16)

**Implementation Steps**:
1. **Add cell size propagation to switchToAlternateBuffer** - After creating/resetting alternate WASM grid, call set_cell_size_px with current terminal dimensions
2. **Ensure data handler sets cell size on core change** - When registeredCore changes in the while loop, propagate cell size to the new core
3. **Investigate treemd red background** - Run treemd with logging enabled, check if CSI 16t response is correct during alternate buffer, verify rendering sequences
4. **Investigate viewer not launching** - Run kitten icat with logging, verify APC data reaches Rust backend, check image decode and event emission
5. **Fix identified issues** - Apply targeted fixes based on investigation findings

**Dependencies**: Phase 1 (complete)

**Testing Approach**:
- Unit: set_cell_size_px called after buffer switch with correct dimensions
- Unit: alternate core cell size matches primary after switch
- Integration: Capability detection sequence produces correct responses from alternate buffer
- Manual: treemd renders without red background; kitten icat displays image

**Acceptance Criteria**:
- [ ] Alternate buffer core has correct cell size after switch
- [ ] CSI 16t from alternate buffer returns actual cell dimensions
- [ ] treemd root cause identified and fixed
- [ ] kitten icat image display works via viewer overlay

**Estimated Effort**: Medium

---

### Phase 3: Animation Support Verification

**Goal**: Verify Kitty animation frame handling works end-to-end

**Files to Modify**:
- `src-tauri/src/image/kitty.rs` - Verify handle_frame (a=f) and handle_animate (a=a) paths
- `src-tauri/src/image/animation.rs` - Verify animation state management

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| KittyHandler.handle_frame | Process animation frame commands (a=f) | Valid frame data with image_id | Frame added to animation, events emitted |
| KittyHandler.handle_animate | Process animation control commands (a=a) | Valid animation params | Animation state updated, events emitted |
| AnimationManager | Track animation state per image | Image registered | Frame management, playback control |

**Processing Flow** (diagram-convertible):
1. APC with a=f arrives
   - Parse frame parameters (frame number, composition mode, duration)
   - Decode frame image data
   - Add frame to animation for target image_id
   - Emit animation events to frontend
2. APC with a=a arrives
   - Parse animation control parameters (loops, frame number)
   - Update animation state (play/stop/set frame)
   - Emit animation events to frontend
3. Frontend handles animation events
   - ImageViewer receives animation event payload
   - Updates displayed frame accordingly

**Implementation Steps**:
1. **Review animation frame handling** - Verify handle_frame correctly decodes frame data and adds to AnimationManager
2. **Review animation control handling** - Verify handle_animate correctly controls playback state
3. **Verify frontend animation event routing** - Confirm image_event with type "Animation" reaches ImageViewer
4. **Add missing test coverage** - Write unit tests for frame and animate command handling if gaps found

**Dependencies**: Phase 2

**Testing Approach**:
- Unit: handle_frame adds frame to animation manager correctly
- Unit: handle_animate updates animation state correctly
- Integration: Animation APC sequence produces correct events
- Manual: treemd Kitty-mode animations render correctly

**Acceptance Criteria**:
- [ ] Kitty animation frame commands (a=f) processed correctly
- [ ] Kitty animation control commands (a=a) processed correctly
- [ ] Animation events reach frontend ImageViewer
- [ ] treemd animations render (if applicable)

**Estimated Effort**: Small

---

## Complete File Structure

```
wasm/src/
├── apc_handler.rs          # Kitty query synchronous response (Phase 1 - done)
├── csi_device.rs           # XTWINOPS handlers (Phase 1 - done)
├── csi_dispatch.rs         # CSI dispatch including 't' (Phase 1 - done)
├── terminal_core.rs        # cell_size_px fields (Phase 1 - done)

src/
├── terminal-app/index.ts   # Cell size propagation in data handler (Phase 2)
├── terminal/state.ts       # Cell size sync on buffer switch (Phase 2)

src-tauri/src/image/
├── kitty.rs                # Animation frame/control handlers (Phase 3 - verify)
├── animation.rs            # Animation state management (Phase 3 - verify)
```

## Testing Strategy

- **Unit (WASM)**: Kitty query responses, XTWINOPS responses, cell size defaults — existing tests cover 80%+
- **Unit (TypeScript)**: syncModesFromWasm, buffer switch cell size propagation — new tests needed for Phase 2
- **Integration**: Full capability detection sequence (query + DA1 + CSI 16t + DSR) response ordering
- **E2E (Manual)**: treemd rendering, kitten icat display, emterm image regression

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| wasm-pack | latest | WASM build |
| bun | latest | TS build and test |
| ratatui-image | v10.0.6+ | External tool for verification |
| kitten | latest | External tool for verification |
| treemd | latest | External tool for verification |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| treemd red background has multiple causes | Medium | Medium | Systematic investigation with logging; fix incrementally |
| Viewer not launching is not related to cell size | Low | High | Pipeline is correctly wired; Phase 2 includes investigation step |
| Animation frame format mismatch | Low | Low | Existing handler code covers standard Kitty animation protocol |
| Breaking existing emterm image functionality | Low | High | Regression tests; NFR2 verification |

## Open Questions

- [ ] FR4: Root cause of viewer not launching (hypothesis: cell size mismatch in alternate buffer)
- [ ] FR3 red background: Exact CSS/rendering cause in treemd (needs runtime investigation)

## Success Metrics

- [ ] All functional requirements (FR1-FR5) implemented and tested
- [ ] treemd displays markdown without red background or warnings
- [ ] kitten icat displays images via viewer overlay
- [ ] All existing tests pass (WASM, TS, Rust)
- [ ] No regression in emterm image functionality
- [ ] Kitty animation frames handled
