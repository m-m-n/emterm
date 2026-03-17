# Feature: Large File Decomposition Refactoring

## Overview

Decompose 6 large files (1000+ lines) into smaller, focused modules by extracting cohesive groups of methods into separate function-based modules. This is a pure refactoring with no functional changes, maintaining all existing public APIs and import paths via re-exports.

## Objectives

- Reduce all target files to under 1000 lines
- Improve code maintainability by separating responsibilities into focused modules
- Maintain backward compatibility for all external import paths

## User Stories

### US1: Code Maintainability
As a developer, I want each file to have a clear, single responsibility, so that I can quickly locate and modify relevant code.

**Acceptance Criteria:**
- [ ] All 6 target files are reduced to under 1000 lines
- [ ] Each split module has a clear, documented responsibility
- [ ] No functional changes are introduced

### US2: Import Compatibility
As a developer, I want existing import paths to continue working, so that I don't need to update import statements across the codebase.

**Acceptance Criteria:**
- [ ] All existing import paths work without modification
- [ ] Re-exports are established in original files for split-out symbols

## Technical Requirements

### Functional Requirements

- **FR1: terminal-app/index.ts decomposition** — Extract PTY handling (~260 lines), OSC callback handling (~200 lines), resize handling, and UI handling into separate function modules (`pty-handler.ts`, `osc-handler.ts`, `resize-handler.ts`, `ui-handler.ts`).
- **FR2: canvas-renderer.ts decomposition** — Extract line rendering, decoration drawing, cursor rendering, selection rendering, fold rendering, and settings application into separate function modules (`renderer-line.ts`, `renderer-decorations.ts`, `renderer-cursor.ts`, `renderer-selection.ts`, `renderer-fold.ts`, `renderer-settings.ts`).
- **FR3: state.ts decomposition** — Extract buffer switching, WASM sync, action processing, and response management into separate function modules (`state-buffer.ts`, `state-wasm-sync.ts`, `state-actions.ts`, `state-response.ts`).
- **FR4: unified-buffer.ts decomposition** — Extract scroll operation methods into `buffer-scroll.ts`.
- **FR5: layer.ts decomposition** — Extract image placement calculation logic into `layer-placement.ts`.
- **FR6: terminal_core.rs decomposition** — Extract cell accessor methods into `terminal_cells.rs` and row operation methods into `terminal_rows.rs`.

### Non-Functional Requirements

- **NFR1 - Backward Compatibility:** All existing public APIs and import paths must remain unchanged. Original files re-export symbols from split modules.
- **NFR2 - Performance:** No runtime performance regression. Function-based splitting has zero overhead as the module system is resolved at build time.
- **NFR3 - Code Quality:** Each split module should be under 400 lines as a guideline. Function-based module style (consistent with existing `renderer-utils.ts` pattern).

## Implementation Approach

### Architecture

**Split Pattern (TypeScript):**
```
Before:
  canvas-renderer.ts (1895 lines, 50+ methods in one class)

After:
  canvas-renderer.ts (orchestration, render loop, re-exports)
  renderer-line.ts (line rendering functions)
  renderer-decorations.ts (decoration drawing functions)
  renderer-cursor.ts (cursor rendering functions)
  renderer-selection.ts (selection rendering functions)
  renderer-fold.ts (fold rendering functions)
  renderer-settings.ts (settings application functions)
```

**Split Pattern (Rust):**
```
Before:
  terminal_core.rs (824 lines of impl methods)

After:
  terminal_core.rs (core struct, construction, main logic)
  terminal_cells.rs (impl TerminalCore - cell accessor methods)
  terminal_rows.rs (impl TerminalCore - row operation methods)
```

### Detailed Split Plans

#### Phase 1: terminal-app/index.ts (1425 lines → ~600 lines + 4 modules)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `src/terminal-app/pty-handler.ts` | `setupPtyHandlers` (PTY data receive → WASM → render pipeline) | ~260 |
| `src/terminal-app/osc-handler.ts` | `handleOscCallback`, `processPendingOscQueue` | ~200 |
| `src/terminal-app/resize-handler.ts` | `setupResizeObserver`, `resize`, `handleCharSizeChange` | ~100 |
| `src/terminal-app/ui-handler.ts` | `handleBell`, `handleWheel`, `handleMiddleClickPaste`, `toggleSearch` | ~100 |

Note: `handlers/` directory already contains `ime.ts`, `keyboard.ts`, `mouse.ts` — this follows the same separation pattern.

#### Phase 2: canvas-renderer.ts (1895 lines → ~500 lines + 6 modules)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `src/terminal/renderer-line.ts` | `renderLine`, `renderLinePacked`, `renderLineBackground`, `renderLineText`, `renderSpanText`, `renderLineBackgroundFromSpans`, `renderLineTextFromSpans` | ~400 |
| `src/terminal/renderer-decorations.ts` | `renderDetectionUnderlines*`, `drawClippedUnderline*`, `drawUnderline`, `drawStrikethrough` | ~250 |
| `src/terminal/renderer-cursor.ts` | `renderCursor`, `renderCursorArea`, `startCursorBlink`, `stopCursorBlink` | ~150 |
| `src/terminal/renderer-selection.ts` | `renderSelection`, `clearSelectionOverlays`, `clearSelectionHighlight` | ~100 |
| `src/terminal/renderer-fold.ts` | `renderFoldSummaryLines`, `renderSummaryLine`, `getVisibleLinesWithFolding` | ~200 |
| `src/terminal/renderer-settings.ts` | `setFontSize`, `setFontFamily`, `setColorScheme`, `setUserColorScheme`, `setCursorStyle`, `setBoldBrightensAnsiColors` | ~150 |

Existing: `renderer-utils.ts` already exists — new modules follow the same pattern.

#### Phase 3: state.ts (1442 lines → ~600 lines + 4 modules)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `src/terminal/state-buffer.ts` | `switchToAlternateBuffer`, `switchToPrimaryBuffer` | ~150 |
| `src/terminal/state-wasm-sync.ts` | `syncModesToWasm`, `syncModesFromWasm`, `syncTabStop*` | ~200 |
| `src/terminal/state-actions.ts` | `processAction`, `flushGraphemeBuffer`, `handleCsiWasm`, `handleEscWasm`, `handleModesWasm`, `executeModAction`, `readAndSendResponse` | ~300 |
| `src/terminal/state-response.ts` | `takePendingResponse`, `addPendingResponse` | ~100 |

Note: `processAction` already delegates to external `handleOsc`, `handleApc`, `handleDcs` — this extends the same delegation pattern.

#### Phase 4: unified-buffer.ts (1154 lines → ~900 lines + 1 module)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `src/terminal/buffer-scroll.ts` | Scroll operation methods | ~250 |

#### Phase 5: layer.ts (1080 lines → ~800 lines + 1 module)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `src/image/layer-placement.ts` | Image placement calculation logic | ~280 |

#### Phase 6: terminal_core.rs (824 lines → ~400 lines + 2 modules)

| New File | Methods to Extract | Est. Lines |
|---|---|---|
| `wasm/src/terminal_cells.rs` | `get_cell_*`, `set_cell*` (cell accessor impl block) | ~200 |
| `wasm/src/terminal_rows.rs` | `shift_rows_*`, `clear_line*`, `copy_row` (row operation impl block) | ~200 |

### Dependencies

**Internal Dependencies:**
- `renderer-utils.ts`: Already exists, new renderer-* modules follow same pattern
- `handlers/` directory: Already contains ime.ts, keyboard.ts, mouse.ts — new handlers follow same pattern
- `state.ts` delegates to `handleOsc`, `handleApc`, `handleDcs` — extends same delegation

**External Dependencies:**
- None (pure refactoring)

### File Structure

```
src/terminal-app/
├── index.ts              # TerminalApp (orchestration, re-exports)
├── pty-handler.ts         # NEW: PTY data handling functions
├── osc-handler.ts         # NEW: OSC callback functions
├── resize-handler.ts      # NEW: Resize handling functions
├── ui-handler.ts          # NEW: UI event handling functions
├── handlers/
│   ├── ime.ts             # (existing)
│   ├── keyboard.ts        # (existing)
│   └── mouse.ts           # (existing)

src/terminal/
├── canvas-renderer.ts     # CanvasRenderer (orchestration, re-exports)
├── renderer-line.ts       # NEW: Line rendering functions
├── renderer-decorations.ts # NEW: Decoration drawing functions
├── renderer-cursor.ts     # NEW: Cursor rendering functions
├── renderer-selection.ts  # NEW: Selection rendering functions
├── renderer-fold.ts       # NEW: Fold rendering functions
├── renderer-settings.ts   # NEW: Settings application functions
├── renderer-utils.ts      # (existing)
├── state.ts               # TerminalState (core, re-exports)
├── state-buffer.ts        # NEW: Buffer switching functions
├── state-wasm-sync.ts     # NEW: WASM sync functions
├── state-actions.ts       # NEW: Action processing functions
├── state-response.ts      # NEW: Response management functions
├── unified-buffer.ts      # UnifiedBuffer (core, re-exports)
├── buffer-scroll.ts       # NEW: Scroll operation functions

src/image/
├── layer.ts               # ImageLayer (core, re-exports)
├── layer-placement.ts     # NEW: Image placement calculation

wasm/src/
├── terminal_core.rs       # TerminalCore (core struct, re-exports)
├── terminal_cells.rs      # NEW: Cell accessor impl block
├── terminal_rows.rs       # NEW: Row operation impl block
```

## Test Scenarios

### Unit Tests
- [ ] All existing Rust tests pass: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] All existing TypeScript tests pass: `bun test`
- [ ] All existing WASM tests pass: `cd wasm && cargo test`

### Integration Tests
- [ ] TypeScript typecheck passes: `bun run typecheck`

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Circular dependency check: No circular imports between split modules
- [ ] Re-export completeness: All previously exported symbols are still accessible from original file paths

## Security Considerations

Not applicable (internal refactoring only).

## Error Handling

Not applicable (no new error paths introduced).

## Performance Optimization

### Performance Goals
- Zero runtime overhead from module splitting (resolved at build time)

### Optimization Strategies
- Function-based splitting: No class instantiation overhead
- TypeScript module bundling: Bun resolves imports at build time

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented
- [ ] All existing tests pass (Rust, TypeScript, WASM, E2E)
- [ ] TypeScript typecheck passes
- [ ] All target files are under 1000 lines
- [ ] No external import path changes
- [ ] No functional changes introduced

## Open Questions

None.

## Implementation Phases

### Phase 1: terminal-app/index.ts
**Goals:** Extract PTY, OSC, resize, and UI handlers
**Deliverables:**
- `pty-handler.ts`, `osc-handler.ts`, `resize-handler.ts`, `ui-handler.ts`
- Updated `index.ts` with re-exports

### Phase 2: canvas-renderer.ts
**Goals:** Extract rendering subsystems
**Deliverables:**
- `renderer-line.ts`, `renderer-decorations.ts`, `renderer-cursor.ts`, `renderer-selection.ts`, `renderer-fold.ts`, `renderer-settings.ts`
- Updated `canvas-renderer.ts` with re-exports

### Phase 3: state.ts
**Goals:** Extract state management subsystems
**Deliverables:**
- `state-buffer.ts`, `state-wasm-sync.ts`, `state-actions.ts`, `state-response.ts`
- Updated `state.ts` with re-exports

### Phase 4: unified-buffer.ts
**Goals:** Extract scroll operations
**Deliverables:**
- `buffer-scroll.ts`
- Updated `unified-buffer.ts` with re-exports

### Phase 5: layer.ts
**Goals:** Extract image placement logic
**Deliverables:**
- `layer-placement.ts`
- Updated `layer.ts` with re-exports

### Phase 6: terminal_core.rs
**Goals:** Extract cell and row operations
**Deliverables:**
- `terminal_cells.rs`, `terminal_rows.rs`
- Updated `terminal_core.rs` with mod declarations
