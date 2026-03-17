# Implementation Plan: Large File Decomposition Refactoring

## Overview

Decompose 6 large files (1000+ lines) into focused function-based modules, maintaining all public APIs and import paths via re-exports. Pure refactoring with zero functional changes.

## Objectives

- Reduce all 6 target files to under 1000 lines
- Improve maintainability by separating responsibilities into focused modules
- Maintain backward compatibility for all external import paths via re-exports

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- Rust toolchain with wasm-pack
- Docker (for test execution)

### Dependencies
- No new dependencies required (pure refactoring)

## Architecture Overview

### Technology Stack
- **TypeScript**: 5 files (terminal-app, canvas-renderer, state, unified-buffer, layer)
- **Rust/WASM**: 1 file (terminal_core)
- **Bundler**: Bun
- **Test runners**: bun test, cargo test, tauri-driver E2E

### Design Approach

**TypeScript Split Pattern**: Extract method groups into standalone function modules. Each extracted function receives necessary state as parameters instead of accessing `this`. The original class delegates to these functions, and re-exports any publicly needed symbols.

**Rust Split Pattern**: Move `impl` blocks into separate files using `mod` declarations. Rust's module system natively supports splitting `impl` blocks across files.

**Key Principle**: Every split follows a consistent pattern:
1. Identify a cohesive group of methods sharing a responsibility
2. Extract them into a new file as standalone functions (TS) or impl block (Rust)
3. Update the original file to import and delegate
4. Re-export any public symbols from the original file

### Component Interaction

The data flow remains unchanged:
```
PTY → PtyClient → TerminalApp → TerminalState → WASM/Handlers → CanvasRenderer → Canvas
                                                                → ImageLayer → WebGL/Canvas
```

Split modules are internal implementation details — no cross-component interfaces change.

## Implementation Phases

### Phase 1: terminal-app/index.ts Decomposition

**Goal**: Reduce TerminalApp from 1425 lines to ~600 lines by extracting 4 handler modules.

**Files to Create**:
- `src/terminal-app/pty-handler.ts` — PTY data flow orchestration, WASM watchdog, error recovery
- `src/terminal-app/osc-handler.ts` — OSC callback dispatch, pending queue processing, iTerm2 image handling
- `src/terminal-app/resize-handler.ts` — ResizeObserver setup, dimension computation, char size propagation
- `src/terminal-app/ui-handler.ts` — Bell flash, wheel scroll, middle-click paste, search toggle

**Files to Modify**:
- `src/terminal-app/index.ts` — Remove extracted methods, import and delegate to new modules, add re-exports

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| pty-handler | Manage PTY data receive → WASM process → render pipeline | TerminalApp initialized with PtyClient and TerminalState | PTY data flows through WASM and triggers scheduled renders |
| osc-handler | Dispatch OSC actions to appropriate handlers (title, color, image, clipboard, fold, semantic) | TerminalState and ImageLayer available | OSC actions processed, UI updated accordingly |
| resize-handler | Observe container resize, compute cols/rows from char size, propagate to PTY/renderer | Container element exists, char size measured | Terminal dimensions updated across all components |
| ui-handler | Handle user-facing events (bell, scroll, paste, search) | TerminalApp components initialized | Events dispatched to appropriate subsystems |

**Processing Flow** (pty-handler):
1. Receive binary PTY data chunk
   - Normal → feed to WASM parser
   - WASM error → enter recovery mode
2. Process parsed actions (already done by WASM callbacks)
3. Flush grapheme buffer
4. Schedule render if dirty rows exist
5. Process pending OSC queue
   - Queue non-empty → delegate to osc-handler

**Implementation Steps**:
1. **Extract pty-handler** — Move `setupPtyHandlers` and WASM recovery logic into standalone functions that accept TerminalApp context as parameter
2. **Extract osc-handler** — Move `handleOscCallback`, `processPendingOscQueue`, `handleIterm2InlineImage`, `updateWindowTitle` into standalone functions
3. **Extract resize-handler** — Move `setupResizeObserver`, resize computation, `handleCharSizeChange` into standalone functions
4. **Extract ui-handler** — Move `handleBell`, `handleWheel`, `handleMiddleClickPaste`, `toggleSearch` into standalone functions
5. **Update index.ts** — Replace method bodies with delegation calls, add re-exports
6. **Verify** — Run typecheck and existing tests

**Dependencies**: None (first phase). Blocks nothing.

**Testing Approach**:
- Unit: Existing bun test suite must pass
- Integration: TypeScript typecheck must pass
- E2E: Existing E2E tests must pass

**Acceptance Criteria**:
- [ ] index.ts is under 1000 lines
- [ ] All 4 new modules created
- [ ] All existing tests pass
- [ ] Typecheck passes

**Estimated Effort**: medium

---

### Phase 2: canvas-renderer.ts Decomposition

**Goal**: Reduce CanvasRenderer from 1895 lines to ~500 lines by extracting 6 rendering modules.

**Files to Create**:
- `src/terminal/renderer-line.ts` — Line-level rendering: background spans, text spans, packed and unpacked modes
- `src/terminal/renderer-decorations.ts` — URL/file detection underlines, strikethrough, clipped underline drawing
- `src/terminal/renderer-cursor.ts` — Cursor drawing (block/underline/bar), blink timer management, cursor area rendering
- `src/terminal/renderer-selection.ts` — DOM-based selection overlay creation, clearing, highlight management
- `src/terminal/renderer-fold.ts` — Fold region visibility, summary line rendering, visible row calculation with folding
- `src/terminal/renderer-settings.ts` — Font family/size, color scheme, cursor style, bold-brightens configuration

**Files to Modify**:
- `src/terminal/canvas-renderer.ts` — Remove extracted methods, import and delegate, add re-exports

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderer-line | Render single lines (background + text) from JS Line or WASM packed data | Canvas context ready, char dimensions measured | Line pixels drawn on canvas |
| renderer-decorations | Draw detection underlines for URLs and file paths | Line rendered, detection results available | Underlines drawn below detected links |
| renderer-cursor | Draw cursor shape and manage blink animation | Cursor position and style known | Cursor visible at correct position with blink state |
| renderer-selection | Create/remove DOM overlay elements for selection highlighting | Selection range computed | DOM overlays positioned over selected cells |
| renderer-fold | Calculate visible rows respecting fold regions, render fold summaries | FoldManager available with fold state | Folded regions collapsed, summaries displayed |
| renderer-settings | Apply font/color/cursor configuration changes | CanvasRenderer initialized | Canvas re-measured, colors updated, render triggered |

**Processing Flow** (renderer-line):
1. Receive row index and line data (JS Line or packed bytes)
2. Group cells into spans by shared attributes
3. Render background spans (fill rectangles)
4. Render text spans (draw text with font attributes)
   - Wide character → use fitted drawing with scaling
   - Custom glyph → delegate to custom glyph renderer
   - Normal character → standard text draw

**Implementation Steps**:
1. **Extract renderer-line** — Move 7 line rendering methods (renderLine, renderLinePacked, renderLineBackground/Text, renderLineBackground/TextFromSpans, renderSpanText) plus wide/fitted character drawing
2. **Extract renderer-decorations** — Move 6 detection underline methods plus drawUnderline, drawStrikethrough
3. **Extract renderer-cursor** — Move renderCursor, renderCursorArea, startCursorBlink, stopCursorBlink
4. **Extract renderer-selection** — Move renderSelection, clearSelectionOverlays, clearSelectionHighlight
5. **Extract renderer-fold** — Move getVisibleLinesWithFolding, renderFoldSummaryLines, renderSummaryLine, getVisibleRowsPacked
6. **Extract renderer-settings** — Move all set*/get* configuration methods
7. **Update canvas-renderer.ts** — Replace method bodies with delegation, add re-exports. Note: `renderSearchHighlights` remains in the main file as it is tightly coupled to the render loop orchestration

**Dependencies**: Requires Phase 1 complete (for stable test baseline). Blocks nothing.

**Testing Approach**:
- Unit: Existing bun test suite must pass
- Integration: TypeScript typecheck must pass
- E2E: Rendering must be visually correct (existing E2E tests)

**Acceptance Criteria**:
- [ ] canvas-renderer.ts is under 1000 lines
- [ ] All 6 new modules created
- [ ] All existing tests pass
- [ ] Typecheck passes
- [ ] Existing renderer-utils.ts pattern followed

**Estimated Effort**: large

---

### Phase 3: state.ts Decomposition

**Goal**: Reduce TerminalState from 1442 lines to ~600 lines by extracting 4 state management modules.

**Files to Create**:
- `src/terminal/state-buffer.ts` — Alternate/primary buffer switching with cursor save/restore
- `src/terminal/state-wasm-sync.ts` — Mode synchronization, cell size sync, tab stop sync between JS and WASM
- `src/terminal/state-actions.ts` — Action dispatch (processAction), grapheme buffer flushing, CSI/ESC WASM handlers
- `src/terminal/state-response.ts` — Device response queue management (DA, DSR, cursor position reports)

**Files to Modify**:
- `src/terminal/state.ts` — Remove extracted methods, import and delegate, add re-exports

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| state-buffer | Switch between primary and alternate buffers, save/restore cursor state | Both buffers initialized | Active buffer switched, cursor state preserved |
| state-wasm-sync | Synchronize terminal modes, cell dimensions, tab stops between JS state and WASM core | WASM core initialized | JS and WASM state consistent |
| state-actions | Route parsed terminal actions to appropriate handlers (WASM for CSI/ESC, JS for OSC/APC/DCS) | WASM core and handlers available | Actions processed, grid/cursor/modes updated |
| state-response | Manage FIFO queue of device responses to send back via PTY | Response generated by handler | Response available for PTY write-back |

**Processing Flow** (state-actions):
1. Receive parsed action with type discriminator
   - Print → accumulate in grapheme buffer
   - Control (CR, LF, etc.) → execute directly
   - CSI → flush grapheme buffer, delegate to WASM CSI handler
   - ESC → flush grapheme buffer, delegate to WASM ESC handler
   - OSC/APC/DCS → flush grapheme buffer, delegate to JS handler functions
2. After WASM handling, check for pending response
   - Response available → add to response queue

**Implementation Steps**:
1. **Extract state-buffer** — Move switchToAlternateBuffer, switchToPrimaryBuffer, related cursor save/restore logic
2. **Extract state-wasm-sync** — Move syncModesToWasm, syncModesFromWasm, setCellSizePx, syncTabStop*, setCursorShowInterrupt
3. **Extract state-actions** — Move processAction, flushGraphemeBuffer, handleCsiWasm, handleEscWasm, handleModesWasm, executeModAction, readAndSendResponse
4. **Extract state-response** — Move takePendingResponse, addPendingResponse
5. **Update state.ts** — Replace method bodies with delegation, add re-exports

**Dependencies**: Requires Phase 1-2 complete. Blocks nothing.

**Testing Approach**:
- Unit: Existing bun test suite must pass
- Integration: TypeScript typecheck must pass
- E2E: Terminal behavior must be unchanged

**Acceptance Criteria**:
- [ ] state.ts is under 1000 lines
- [ ] All 4 new modules created
- [ ] All existing tests pass
- [ ] Typecheck passes

**Estimated Effort**: medium

---

### Phase 4: unified-buffer.ts Decomposition

**Goal**: Reduce UnifiedBuffer from 1154 lines to ~900 lines by extracting scroll operations.

**Files to Create**:
- `src/terminal/buffer-scroll.ts` — Scroll region management, scrollUp/scrollDown, insertLines, deleteLines

**Files to Modify**:
- `src/terminal/unified-buffer.ts` — Remove extracted methods, import and delegate, add re-exports

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| buffer-scroll | Execute scroll operations within scroll regions, insert/delete lines | Buffer initialized, scroll region defined | Lines shifted, blank lines inserted, dirty flags set |

**Implementation Steps**:
1. **Extract buffer-scroll** — Move scrollUp, scrollDown, insertLines, deleteLines, and scroll region methods (setScrollRegion, clearScrollRegion, getScrollRegion, getEffectiveScrollRegion)
2. **Update unified-buffer.ts** — Replace method bodies with delegation, add re-exports

**Dependencies**: Requires Phase 1-3 complete. Blocks nothing.

**Testing Approach**:
- Unit: Existing unified-buffer.test.ts must pass
- Integration: TypeScript typecheck must pass

**Acceptance Criteria**:
- [ ] unified-buffer.ts is under 1000 lines
- [ ] buffer-scroll.ts created
- [ ] All existing tests pass

**Estimated Effort**: small

---

### Phase 5: layer.ts Decomposition

**Goal**: Reduce ImageLayer from 1080 lines to ~800 lines by extracting placement calculation.

**Files to Create**:
- `src/image/layer-placement.ts` — Image placement pixel calculation, aspect ratio sizing, placement storage/deletion

**Files to Modify**:
- `src/image/layer.ts` — Remove extracted methods, import and delegate, add re-exports

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| layer-placement | Calculate display dimensions from placement spec (cols/rows/both/neither), compute pixel positions, manage placement lifecycle | Image stored, char dimensions known | Placement positioned with correct aspect ratio |

**Implementation Steps**:
1. **Extract layer-placement** — Move placeImage sizing logic, deleteImages dispatch, scrollPlacements, getPlacementsAtPosition, getPlacementsForImage
2. **Update layer.ts** — Replace method bodies with delegation, add re-exports

**Dependencies**: Requires Phase 1-4 complete. Blocks nothing.

**Testing Approach**:
- Unit: Existing bun test suite must pass
- Integration: TypeScript typecheck must pass

**Acceptance Criteria**:
- [ ] layer.ts is under 1000 lines
- [ ] layer-placement.ts created
- [ ] All existing tests pass

**Estimated Effort**: small

---

### Phase 6: terminal_core.rs Decomposition

**Goal**: Reduce TerminalCore impl from 824 lines to ~400 lines by extracting cell and row operations.

**Files to Create**:
- `wasm/src/terminal_cells.rs` — Cell read/write accessors: set_cell, set_cell_ascii, get_cell_char/width/fg/bg/flags, get_cell_hyperlink_id, hyperlink queries
- `wasm/src/terminal_rows.rs` — Row operations: clear_line, clear_line_range, shift_rows_up/down, copy_row, fill_row_default, get_line_text, is_line_empty, wrapped flag accessors

**Files to Modify**:
- `wasm/src/terminal_core.rs` — Remove extracted impl methods, add mod declarations
- `wasm/src/lib.rs` — Add mod declarations for new modules (if needed)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| terminal_cells | Individual cell read/write with bounds checking and overflow tracking | Grid initialized with valid dimensions | Cell data stored/retrieved correctly |
| terminal_rows | Bulk row operations: clearing, shifting, copying within scroll regions | Grid initialized, row indices valid | Rows manipulated, dirty flags updated |

**Implementation Steps**:
1. **Extract terminal_cells** — Move cell accessor impl block (set_cell, set_cell_ascii, get_cell_*, get_hyperlink_*, bce_cell, cell_index)
2. **Extract terminal_rows** — Move row operation impl block (clear_line*, shift_rows_*, copy_row, fill_row_default, get_line_text, is_line_empty, wrapped accessors)
3. **Update terminal_core.rs** — Add mod declarations, remove moved impl blocks
4. **Verify** — Run cargo test and wasm-pack build

**Dependencies**: Independent of TypeScript phases but scheduled last to maintain test stability.

**Testing Approach**:
- Unit: Existing wasm cargo test must pass
- Integration: wasm-pack build must succeed
- E2E: Terminal behavior unchanged

**Acceptance Criteria**:
- [ ] terminal_core.rs impl body is under 500 lines
- [ ] Both new modules created with impl blocks
- [ ] cargo test passes
- [ ] wasm-pack build succeeds

**Estimated Effort**: medium

---

## Complete File Structure

```
src/terminal-app/
├── index.ts                  # TerminalApp (reduced, delegates to handlers)
├── pty-handler.ts            # NEW: PTY data flow functions
├── osc-handler.ts            # NEW: OSC callback functions
├── resize-handler.ts         # NEW: Resize handling functions
├── ui-handler.ts             # NEW: UI event functions
├── handlers/
│   ├── ime.ts                # (existing, unchanged)
│   ├── keyboard.ts           # (existing, unchanged)
│   └── mouse.ts              # (existing, unchanged)

src/terminal/
├── canvas-renderer.ts        # CanvasRenderer (reduced, delegates to modules)
├── renderer-line.ts          # NEW: Line rendering functions
├── renderer-decorations.ts   # NEW: Decoration drawing functions
├── renderer-cursor.ts        # NEW: Cursor rendering functions
├── renderer-selection.ts     # NEW: Selection rendering functions
├── renderer-fold.ts          # NEW: Fold rendering functions
├── renderer-settings.ts      # NEW: Settings application functions
├── renderer-utils.ts         # (existing, unchanged)
├── state.ts                  # TerminalState (reduced, delegates to modules)
├── state-buffer.ts           # NEW: Buffer switching functions
├── state-wasm-sync.ts        # NEW: WASM sync functions
├── state-actions.ts          # NEW: Action processing functions
├── state-response.ts         # NEW: Response management functions
├── unified-buffer.ts         # UnifiedBuffer (reduced, delegates to modules)
├── buffer-scroll.ts          # NEW: Scroll operation functions

src/image/
├── layer.ts                  # ImageLayer (reduced, delegates to modules)
├── layer-placement.ts        # NEW: Placement calculation functions

wasm/src/
├── terminal_core.rs          # TerminalCore (reduced, mod declarations)
├── terminal_cells.rs         # NEW: Cell accessor impl block
├── terminal_rows.rs          # NEW: Row operation impl block
├── lib.rs                    # (may need mod additions)
```

## Testing Strategy

- **Unit**: All existing test suites must pass unchanged (bun test, cargo test, wasm cargo test)
- **Integration**: TypeScript typecheck (bun run typecheck) must pass after each phase
- **E2E (Docker)**: Full E2E suite must pass after all phases complete
- **Regression**: No new tests needed — existing tests serve as regression suite

**Test execution per phase**:
1. After each TypeScript phase: `bun test` + `bun run typecheck`
2. After Rust phase: `cd wasm && cargo test` + `cargo test --manifest-path src-tauri/Cargo.toml`
3. After all phases: `./scripts/run-e2e-docker.sh test`

## Dependencies

No new external dependencies.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Private member access from extracted functions | Medium | Medium | Pass required state as function parameters; add accessor methods if needed |
| Circular imports between split modules | Low | High | Maintain unidirectional dependency: split modules depend on types only, never on each other |
| TypeScript `this` binding loss in extracted functions | Medium | Low | Pass all needed context explicitly as parameters |
| Rust visibility issues in split impl blocks | Low | Low | Use `pub(crate)` for internal methods, keep `pub` for WASM-exported methods |
| Large merge conflicts with concurrent development | Low | Medium | Complete one phase at a time, commit after each phase |

## Open Questions

None.

## Success Metrics

- [ ] All 6 target files reduced to under 1000 lines
- [ ] 18 new focused modules created
- [ ] All existing tests pass (Rust, TypeScript, WASM, E2E)
- [ ] TypeScript typecheck passes
- [ ] No external import path changes (re-exports maintain compatibility)
- [ ] No functional changes introduced
