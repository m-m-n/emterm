# Implementation Plan: terminal/state.ts Refactoring

## Overview

Refactor the monolithic `TerminalState` class (~1225 lines) into a data-oriented architecture with externalized handler functions organized by functional category.

## Objectives

- Transform TerminalState into a pure data structure with dispatch logic only
- Extract sequence handlers into external functions grouped by category
- Create corresponding test files for each handler module
- Maintain all existing public APIs for backward compatibility

## Prerequisites

### Development Environment
- Bun v1.0+ (package manager and test runner)
- TypeScript 5.0+

### Dependencies
- Existing modules remain unchanged: `cursor.ts`, `modes.ts`, `sgr.ts`, `attributes.ts`, `buffer.ts`, `grid.ts`, `unicode.ts`
- Type definitions: `src/types/terminal.ts`

### Knowledge Requirements
- ANSI escape sequence semantics (CSI, ESC, OSC, C0)
- TerminalState internal data structure and flow

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Build Tool**: Bun
- **Test Framework**: Bun test

### Design Approach

**Data-Oriented Design**: Separate data storage (state) from behavior (handlers).

- **TerminalState**: Data container with dispatch logic only
- **Handlers**: Pure functions receiving state reference and action-specific parameters
- **Mutation**: Handlers mutate state in place (void return)

### Component Interaction

```
processAction(action)
    |
    v
TerminalState.processAction()
    |
    +-- "Print"    --> handlePrint(state, char)
    +-- "Execute"  --> handleExecute(state, code)
    +-- "Csi"      --> handleCsi(state, action)
    |                   |
    |                   +-- "CursorUp"   --> handleCursorUp(state, count)
    |                   +-- "EraseInDisplay" --> handleEraseInDisplay(state, mode)
    |                   +-- ... (dispatch by action type)
    +-- "Esc"      --> handleEsc(state, action)
    +-- "Osc"      --> handleOsc(state, action)
    +-- "Apc"      --> handleApc(state, action)
    +-- "Dcs"      --> handleDcs(state, action)
```

## Implementation Phases

### Phase 1: Infrastructure Setup

**Goal**: Create handlers directory structure, type definitions, and semantics utilities

**Files to Create**:
- `src/terminal/handlers/index.ts` - Re-exports all handler entry points
- `src/terminal/handlers/types.ts` - Handler-specific type definitions
- `src/terminal/handlers/semantics.ts` - CSI default values and coordinate utilities

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CSI_DEFAULTS | Provide default parameter values for CSI sequences | None | Typed constant object available |
| toZeroIndexed | Convert 1-indexed ANSI value to 0-indexed | Value is number | Returns 0-indexed value |
| clampPosition | Clamp coordinates to valid screen bounds | Col, row, screen size provided | Returns valid coordinates |
| TerminalStateAccessor | Type exposing state properties handlers need | State created | Typed interface for handlers |

**Processing Flow**:
```
1. Define CSI_DEFAULTS constant
   - Map each CSI action to default parameter value
2. Implement coordinate utilities
   - toZeroIndexed for ANSI-to-internal conversion
   - clampPosition for boundary validation
3. Define TerminalStateAccessor interface
   - Expose data properties (cursor, modes, buffers)
   - Expose accessor methods (getActiveBuffer, addPendingResponse)
4. Create index.ts with placeholder exports
```

**Implementation Steps**:

1. **Create handlers directory**
   - Ensure `src/terminal/handlers/` directory exists

2. **Create types.ts with accessor interface**
   - Define interface for handler functions to access state
   - Include necessary methods and properties

3. **Create semantics.ts with utilities**
   - CSI_DEFAULTS constant with all default values
   - Coordinate conversion and clamping functions

4. **Create index.ts with handler exports**
   - Initially export placeholder functions
   - Will be updated as handlers are implemented

**Dependencies**:
- Requires: None (no dependencies on other phases)
- Blocks: All handler phases (Phase 2, Phase 3)

**Testing Approach**:

*Unit Tests*:
- Test toZeroIndexed with various inputs including edge cases (0, negative)
- Test clampPosition with boundary values
- Verify CSI_DEFAULTS contains all required actions

**Acceptance Criteria**:
- [ ] `src/terminal/handlers/` directory exists
- [ ] types.ts defines TerminalStateAccessor interface
- [ ] semantics.ts exports CSI_DEFAULTS, toZeroIndexed, clampPosition
- [ ] index.ts compiles without errors
- [ ] TypeScript strict mode passes

**Estimated Effort**: Small (1-2 days)

---

### Phase 2: Extract CSI Handlers

**Goal**: Extract all CSI sequence handlers to separate files by category

**Files to Create**:
- `src/terminal/handlers/csi_cursor.ts` - Cursor movement handlers
- `src/terminal/handlers/csi_cursor.test.ts`
- `src/terminal/handlers/csi_screen.ts` - Erase operation handlers
- `src/terminal/handlers/csi_screen.test.ts`
- `src/terminal/handlers/csi_edit.ts` - Insert/delete operation handlers
- `src/terminal/handlers/csi_edit.test.ts`
- `src/terminal/handlers/csi_scrolling.ts` - Scroll operation handlers
- `src/terminal/handlers/csi_scrolling.test.ts`
- `src/terminal/handlers/csi_char_attrs.ts` - SGR handler
- `src/terminal/handlers/csi_char_attrs.test.ts`
- `src/terminal/handlers/csi_modes.ts` - Mode setting handlers
- `src/terminal/handlers/csi_modes.test.ts`
- `src/terminal/handlers/csi_device.ts` - Device response handlers
- `src/terminal/handlers/csi_device.test.ts`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleCursorUp | Move cursor up by count | Count >= 1 | Cursor row decreased, wrapPending cleared |
| handleCursorDown | Move cursor down by count | Count >= 1 | Cursor row increased, wrapPending cleared |
| handleCursorForward | Move cursor right by count | Count >= 1 | Cursor col increased, wrapPending cleared |
| handleCursorBack | Move cursor left by count | Count >= 1 | Cursor col decreased, wrapPending cleared |
| handleCursorNextLine | Move down N lines, to column 0 | Count >= 1 | Cursor at col 0, row increased |
| handleCursorPreviousLine | Move up N lines, to column 0 | Count >= 1 | Cursor at col 0, row decreased |
| handleCursorHorizontalAbsolute | Set cursor column (1-indexed input) | Column value | Cursor col set, wrapPending cleared |
| handleCursorVerticalAbsolute | Set cursor row (1-indexed input) | Row value | Cursor row set, wrapPending cleared |
| handleCursorPosition | Set cursor position (1-indexed input) | Row, col values | Cursor at position, wrapPending cleared |
| handleEraseInDisplay | Erase screen region by mode | Mode: Below/Above/All/Scrollback | Buffer cleared in specified region |
| handleEraseInLine | Erase line region by mode | Mode: Below/Above/All | Line cleared in specified region |
| handleEraseCharacters | Erase N characters at cursor | Count >= 1 | Characters erased without shift |
| handleInsertLines | Insert blank lines at cursor row | Count >= 1 | Lines inserted, content shifted down |
| handleDeleteLines | Delete lines at cursor row | Count >= 1 | Lines deleted, content shifted up |
| handleInsertCharacters | Insert blank characters at cursor | Count >= 1 | Characters inserted, content shifted right |
| handleDeleteCharacters | Delete characters at cursor | Count >= 1 | Characters deleted, content shifted left |
| handleScrollUp | Scroll content up by N lines | Count >= 1 | Content scrolled, blank lines at bottom |
| handleScrollDown | Scroll content down by N lines | Count >= 1 | Content scrolled, blank lines at top |
| handleSetScrollRegion | Set scrolling region | Top, bottom values | Scroll region set, cursor at home |
| handleSgr | Apply SGR attributes | SGR parameters array | Cursor attributes updated |
| handleSetMode | Enable/disable DEC modes | Mode numbers, enable flag | Modes updated, buffer switch if needed |
| handleDeviceStatusReport | Respond to DSR request | DSR code | Response queued |
| handlePrimaryDeviceAttributes | Respond to DA1 | None | DA1 response queued |
| handleSecondaryDeviceAttributes | Respond to DA2 | None | DA2 response queued |
| handleTertiaryDeviceAttributes | Respond to DA3 | None | DA3 response queued (unit ID) |

**Processing Flow (csi_cursor.ts)**:
```
1. Receive state and count parameter
2. Apply default if count undefined
   - Use CSI_DEFAULTS for action type
3. Execute cursor movement
   - Delegate to state.cursor methods
4. Clear wrapPending flag
5. Mark affected rows as dirty (handled by cursor/buffer)
```

**Processing Flow (csi_screen.ts)**:
```
1. Receive state and erase mode
2. Get active buffer from state
3. Branch by mode (Below/Above/All/Scrollback)
   - Below: Clear from cursor to end
   - Above: Clear from start to cursor
   - All: Clear entire buffer
   - Scrollback: Clear buffer (scrollback not implemented)
4. Buffer marks affected rows as dirty
```

**Processing Flow (csi_modes.ts)**:
```
1. Receive state, mode array, and enable flag
2. For each mode number:
   - Call setDecPrivateMode to update mode state
   - Collect returned actions (buffer switch, cursor save/restore)
3. Execute collected actions in order
   - Buffer switches
   - Cursor operations
```

**Implementation Steps**:

1. **Create csi_cursor.ts**
   - Export individual cursor movement handlers
   - Each handler: apply default, call cursor method, clear wrapPending

2. **Create csi_screen.ts**
   - Export erase handlers
   - Each handler: get buffer, execute clear operation

3. **Create csi_edit.ts**
   - Export insert/delete handlers
   - Delegate to buffer methods

4. **Create csi_scrolling.ts**
   - Export scroll handlers
   - Include SetScrollRegion with cursor home

5. **Create csi_char_attrs.ts**
   - Export handleSgr
   - Delegate to existing parseSgrParams and applySgrAttr

6. **Create csi_modes.ts**
   - Export handleSetMode
   - Handle buffer switching and cursor save/restore actions

7. **Create csi_device.ts**
   - Export DSR and DA handlers
   - Generate response bytes and queue to state

8. **Create test files for each module**
   - Test each handler in isolation
   - Use minimal state setup

**Dependencies**:
- Requires: Phase 1 (types.ts, semantics.ts)
- Blocks: Phase 4 (state.ts refactoring)

**Testing Approach**:

*Unit Tests (csi_cursor.test.ts)*:
- CursorUp at row 5 with count 2 results in row 3
- CursorUp at row 0 stays at row 0 (boundary)
- CursorDown at bottom row stays at bottom
- CursorPosition with 1,1 results in 0,0 (0-indexed)
- All operations clear wrapPending flag

*Unit Tests (csi_screen.test.ts)*:
- EraseInDisplay Below clears cells from cursor to end
- EraseInLine All clears entire line
- EraseCharacters erases N characters without shifting content

*Unit Tests (csi_modes.test.ts)*:
- SetMode 1049 saves cursor and switches to alternate buffer
- ResetMode 1049 restores cursor and switches to primary buffer
- Mode changes update modes object correctly

**Acceptance Criteria**:
- [ ] All 7 csi_*.ts files created
- [ ] All 7 csi_*.test.ts files created
- [ ] Each handler function exported and callable
- [ ] Handler unit tests pass
- [ ] Each file under 300 lines
- [ ] TypeScript strict mode passes

**Estimated Effort**: Medium (3-5 days)

---

### Phase 3: Extract Other Handlers

**Goal**: Extract ESC, OSC, C0, and Print handlers to separate files

**Files to Create**:
- `src/terminal/handlers/esc_handlers.ts` - ESC sequence handlers
- `src/terminal/handlers/esc_handlers.test.ts`
- `src/terminal/handlers/osc_handlers.ts` - OSC sequence handlers
- `src/terminal/handlers/osc_handlers.test.ts`
- `src/terminal/handlers/c0_handlers.ts` - C0 control handlers
- `src/terminal/handlers/c0_handlers.test.ts`
- `src/terminal/handlers/print_handler.ts` - Character print handlers
- `src/terminal/handlers/print_handler.test.ts`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleEsc | Dispatch ESC action to specific handler | EscAction provided | Action executed |
| handleSaveCursor | Save cursor state | None | Cursor state saved |
| handleRestoreCursor | Restore cursor state | Cursor saved previously | Cursor restored, wrapPending cleared |
| handleIndex | Move cursor down, scroll if at bottom | None | Cursor down or buffer scrolled |
| handleNextLine | CR + LF | None | Cursor at col 0, row advanced |
| handleReverseIndex | Move cursor up, scroll down if at top | None | Cursor up or buffer scrolled down |
| handleHorizontalTabSet | Set tab stop at cursor | None | Tab stop added |
| handleResetToInitialState | Reset terminal | None | State fully reset |
| handleSetG0CharSet | Set G0 character set | CharSet value | g0CharSet updated |
| handleSetG1CharSet | Set G1 character set | CharSet value | g1CharSet updated |
| handleOsc | Dispatch OSC action to specific handler | OscAction provided | Action executed |
| handleSetTitle | Set window title | Title string | _title updated |
| handleSetIconName | Set icon name | Name string | _iconName updated |
| handleSetTitleAndIcon | Set both title and icon | String value | Both updated |
| handleSetWorkingDirectory | Set CWD | Path string | _workingDirectory updated |
| handleHyperlink | Set/clear active hyperlink | URI and params | _activeHyperlink updated |
| handleEmtermExtension | Handle emterm commands | Verb and params | Delegate to MarkdownSessionManager |
| MarkdownSessionManager | Manage markdown session lifecycle | State holds reference | Sessions created/closed via OSC commands |
| handleExecute | Dispatch C0 control to handler | C0 code | Control executed |
| handleBel | Handle bell | None | No-op (could emit event) |
| handleBackspace | Move cursor back | None | Cursor moved left, wrapPending cleared |
| handleTab | Move to next tab stop | None | Cursor at next tab, wrapPending cleared |
| handleLineFeed | Move cursor down, scroll if needed | None | Cursor down or scrolled, wrapPending cleared |
| handleCarriageReturn | Move cursor to column 0 | None | Cursor at col 0, wrapPending cleared |
| handleShiftOut | Switch to G1 charset | None | activeCharSet = G1 |
| handleShiftIn | Switch to G0 charset | None | activeCharSet = G0 |
| handlePrint | Print character (fast path) | Character string | Cell written, cursor advanced |
| handlePrintSlow | Print character (slow path) | Character string | Cell written with wrap handling |
| translateCharacter | Translate via active charset | Character | Translated character returned |
| translateLineDrawing | Translate DEC line drawing | Character | Box drawing character returned |
| handleApc | Handle APC sequences | APC payload | APC action executed (currently no-op) |
| handleDcs | Handle DCS sequences | DCS payload | DCS action executed (SIXEL, DECRQSS) |

**Processing Flow (print_handler.ts)**:
```
1. Check fast path conditions
   - ASCII character (0x20-0x7E)
   - No wrap pending
   - G0 charset is Ascii
2. If fast path:
   - Create ASCII cell
   - Write to buffer
   - Advance cursor
   - Handle wrap pending if at end
3. Else slow path:
   - Calculate character width
   - Translate character if needed
   - Handle wrap pending from previous
   - Handle wide character wrap
   - Write cell to buffer
   - Set placeholder for wide char
   - Advance cursor with wrap logic
```

**Processing Flow (c0_handlers.ts)**:
```
1. Receive state and C0 code
2. Branch by code
   - BEL: No-op
   - BS: Backspace, clear wrapPending
   - HT: Tab, clear wrapPending
   - LF/VT/FF: Line feed, scroll if needed, clear wrapPending
   - CR: Carriage return, clear wrapPending
   - SO: Shift Out (G1)
   - SI: Shift In (G0)
3. Default: Ignore unknown codes
```

**Processing Flow (handleApc)**:
```
1. Receive state and APC payload
2. Parse APC sequence (format: <command>;<data>)
3. Branch by command type:
   - Currently no APC commands implemented
   - Log unknown APC sequences for debugging
4. Future extension point for APC-based protocols
```

**Processing Flow (handleDcs)**:
```
1. Receive state and DCS payload
2. Parse DCS sequence (intermediate chars + final char + data)
3. Branch by sequence type:
   - SIXEL graphics: Delegate to SIXEL renderer (if implemented)
   - DECRQSS: Send response for settings query
   - Other: Log unknown DCS sequences
4. Future extension point for DCS-based protocols
```

**Implementation Steps**:

1. **Create esc_handlers.ts**
   - Export handleEsc dispatcher
   - Export individual ESC handlers

2. **Create osc_handlers.ts**
   - Export handleOsc dispatcher
   - Export individual OSC handlers
   - Include emterm extension routing

3. **Create c0_handlers.ts**
   - Export handleExecute dispatcher
   - Export individual C0 handlers

4. **Create print_handler.ts**
   - Export handlePrint (fast path)
   - Export handlePrintSlow (complex cases)
   - Export character translation functions

5. **Create test files for each module**
   - Test each handler with representative inputs

**Dependencies**:
- Requires: Phase 1 (types.ts, semantics.ts)
- Blocks: Phase 4 (state.ts refactoring)

**Testing Approach**:

*Unit Tests (esc_handlers.test.ts)*:
- SaveCursor followed by RestoreCursor restores position
- Index at bottom row scrolls buffer
- ReverseIndex at top row scrolls down
- SetG0CharSet changes g0CharSet property

*Unit Tests (osc_handlers.test.ts)*:
- SetTitle updates _title property
- Hyperlink with URI sets _activeHyperlink
- Hyperlink with empty URI clears _activeHyperlink
- EmtermExtension delegates to MarkdownSessionManager

**MarkdownSessionManager Lifecycle**:
- TerminalState holds optional reference to MarkdownSessionManager
- Manager is injected via setMarkdownSessionManager() after state creation
- handleEmtermExtension checks for manager presence before delegating
- If manager not set, OSC emterm commands are no-op with warning log

*Unit Tests (c0_handlers.test.ts)*:
- Backspace decreases cursor column
- Tab advances to next tab stop
- Line feed at bottom scrolls

*Unit Tests (print_handler.test.ts)*:
- ASCII character prints at cursor and advances
- Wide character advances by 2
- Wide character at end-1 wraps to next line
- Line drawing character translates to Unicode

**Acceptance Criteria**:
- [ ] All 4 handler files created
- [ ] All 4 test files created
- [ ] Each handler function exported and callable
- [ ] Handler unit tests pass
- [ ] Print fast path handles ASCII efficiently
- [ ] Character translation works correctly
- [ ] TypeScript strict mode passes

**Estimated Effort**: Medium (3-5 days)

---

### Phase 4: Refactor state.ts

**Goal**: Simplify state.ts to data container with dispatch logic, importing external handlers

**Files to Modify**:
- `src/terminal/state.ts` - Refactor to use external handlers

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TerminalState (data) | Hold all terminal state data | None | Properties accessible to handlers |
| TerminalState.processAction | Dispatch actions to handlers | Action provided | Handler called with state |
| TerminalState.getActiveBuffer | Return current buffer | None | Buffer returned |
| TerminalState.addPendingResponse | Queue response bytes for PTY write-back | Bytes provided | Response appended to pending buffer |
| TerminalState.switchToAlternateBuffer | Switch to alt buffer | None | Alternate active |
| TerminalState.switchToPrimaryBuffer | Switch to primary buffer | None | Primary active |

**Processing Flow**:
```
1. Import all handler functions from handlers/index.ts
2. processAction dispatches to handlers
   - Pass `this` as state reference
   - Pass action data to handler
3. Handlers access state properties via `this`
   - cursor, modes, buffers, etc.
4. State retains utility methods
   - getDirtyRows, clearDirty, resize, reset
   - extractText, tab stop management
```

**addPendingResponse Implementation**:
```
1. Append provided response bytes to internal pendingResponse buffer
2. If pendingResponse is null, create new Uint8Array from input
3. Otherwise, concatenate existing buffer with new bytes
4. Called by device handlers (DSR, DA1, DA2, DA3) to queue responses
5. takePendingResponse returns and clears the buffer
```

**Implementation Steps**:

1. **Update property visibility**
   - Change private properties to internal/accessible
   - Handlers need access to cursor, modes, buffers, etc.
   - Consider using interface to define accessible properties

2. **Import handlers in state.ts**
   - Import from handlers/index.ts

3. **Replace inline handlers with imports**
   - processAction calls external handlers
   - Remove inline method implementations

4. **Update switch statement in processAction**
   - Dispatch "Print" to handlePrint
   - Dispatch "Execute" to handleExecute
   - Dispatch "Csi" to handleCsi
   - Dispatch "Esc" to handleEsc
   - Dispatch "Osc" to handleOsc
   - Dispatch "Apc" to handleApc
   - Dispatch "Dcs" to handleDcs

5. **Keep utility methods in state.ts**
   - getDirtyRows, clearDirty, resize, reset
   - extractText
   - Tab stop methods if used by handlers

6. **Verify all existing tests pass**
   - Run full test suite
   - No modifications to test files

**Dependencies**:
- Requires: Phase 2 (CSI handlers), Phase 3 (other handlers)
- Blocks: Phase 5 (verification)

**Testing Approach**:

*Integration Tests*:
- All existing state.test.ts tests pass unchanged
- All existing state.phase5.test.ts tests pass
- All existing state.phase6.test.ts tests pass
- Complex multi-action sequences work correctly

*Manual Testing*:
- Terminal application runs normally
- All terminal operations function correctly

**Acceptance Criteria**:
- [ ] state.ts reduced to ~300-500 lines
- [ ] All existing tests pass without modification
- [ ] processAction dispatches to external handlers
- [ ] Public API unchanged
- [ ] Internal methods accessible to handlers
- [ ] TypeScript strict mode passes

**Estimated Effort**: Medium (3-5 days)

---

### Phase 5: Verification and Cleanup

**Goal**: Ensure quality, verify no regressions, clean up

**Files to Modify**:
- `src/terminal/handlers/index.ts` - Final exports
- Any files requiring cleanup

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| index.ts | Export all public handlers | All handlers implemented | Clean re-exports |
| Test coverage | Verify all handlers tested | Tests written | Coverage meets goals |

**Processing Flow**:
```
1. Run full test suite
   - All handler tests pass
   - All integration tests pass
2. Verify performance
   - Run performance tests
   - Compare with baseline
3. Clean up code
   - Remove unused imports
   - Fix any lint issues
4. Final verification
   - TypeScript strict mode
   - Build passes
   - Application runs correctly
```

**Implementation Steps**:

1. **Run full test suite**
   - Execute `bun test`
   - Verify 100% pass rate

2. **Run performance tests**
   - Execute performance.test.ts
   - Ensure no measurable regression

3. **Verify line counts**
   - state.ts: 300-500 lines
   - Each handler file: under 300 lines

4. **Clean up handlers/index.ts**
   - Ensure clean exports
   - Remove any placeholder code

5. **Verify TypeScript compilation**
   - Run `bun run typecheck`
   - Fix any type errors

6. **Manual application testing**
   - Run terminal application
   - Test various terminal operations

**Dependencies**:
- Requires: Phase 4 (refactored state.ts)
- Blocks: None (final phase)

**Testing Approach**:

*Verification*:
- All 661 existing test lines in state.test.ts pass
- Additional handler tests pass
- Performance tests show no regression
- TypeScript strict mode passes

**Acceptance Criteria**:
- [ ] All tests pass
- [ ] No performance regression
- [ ] state.ts under 500 lines
- [ ] All handler files under 300 lines
- [ ] TypeScript strict mode passes
- [ ] Build succeeds
- [ ] Application runs correctly

**Estimated Effort**: Small (1-2 days)

---

## Complete File Structure

```
src/terminal/
├── handlers/
│   ├── index.ts              # Re-exports: handlePrint, handleExecute, handleCsi, handleEsc, handleOsc, handleApc, handleDcs
│   ├── types.ts              # TerminalStateAccessor interface
│   ├── semantics.ts          # CSI_DEFAULTS, toZeroIndexed, clampPosition
│   ├── semantics.test.ts     # Tests for semantics utilities
│   ├── csi_cursor.ts         # handleCursorUp, Down, Forward, Back, NextLine, PreviousLine, HorizontalAbsolute, VerticalAbsolute, Position
│   ├── csi_cursor.test.ts
│   ├── csi_screen.ts         # handleEraseInDisplay, handleEraseInLine, handleEraseCharacters
│   ├── csi_screen.test.ts
│   ├── csi_edit.ts           # handleInsertLines, DeleteLines, InsertCharacters, DeleteCharacters
│   ├── csi_edit.test.ts
│   ├── csi_scrolling.ts      # handleScrollUp, ScrollDown, SetScrollRegion
│   ├── csi_scrolling.test.ts
│   ├── csi_char_attrs.ts     # handleSgr (delegates to sgr.ts, attributes.ts)
│   ├── csi_char_attrs.test.ts
│   ├── csi_modes.ts          # handleSetMode (delegates to modes.ts)
│   ├── csi_modes.test.ts
│   ├── csi_device.ts         # handleDeviceStatusReport, PrimaryDeviceAttributes, SecondaryDeviceAttributes
│   ├── csi_device.test.ts
│   ├── esc_handlers.ts       # handleEsc, SaveCursor, RestoreCursor, Index, NextLine, ReverseIndex, HorizontalTabSet, ResetToInitialState, SetG0CharSet, SetG1CharSet
│   ├── esc_handlers.test.ts
│   ├── osc_handlers.ts       # handleOsc, SetTitle, SetIconName, SetTitleAndIcon, SetWorkingDirectory, Hyperlink, EmtermExtension
│   ├── osc_handlers.test.ts
│   ├── c0_handlers.ts        # handleExecute, BEL, BS, HT, LF, VT, FF, CR, SO, SI
│   ├── c0_handlers.test.ts
│   ├── print_handler.ts      # handlePrint, handlePrintSlow, translateCharacter, translateLineDrawing
│   └── print_handler.test.ts
├── state.ts                  # Refactored: data + dispatch only (~300-500 lines)
├── state.test.ts             # Existing tests (unchanged)
├── state.phase5.test.ts      # Existing tests (unchanged)
├── state.phase6.test.ts      # Existing tests (unchanged)
├── cursor.ts                 # Existing (unchanged)
├── cursor.test.ts            # Existing (unchanged)
├── modes.ts                  # Existing (unchanged)
├── modes.test.ts             # Existing (unchanged)
├── sgr.ts                    # Existing (unchanged)
├── sgr.test.ts               # Existing (unchanged)
├── attributes.ts             # Existing (unchanged)
├── attributes.test.ts        # Existing (unchanged)
├── buffer.ts                 # Existing (unchanged)
├── buffer.test.ts            # Existing (unchanged)
├── grid.ts                   # Existing (unchanged)
├── grid.test.ts              # Existing (unchanged)
├── unicode.ts                # Existing (unchanged)
├── unicode.test.ts           # Existing (unchanged)
└── ... (other existing files)
```

**File Descriptions**:

| File | Purpose | Lines (Target) |
|------|---------|----------------|
| handlers/index.ts | Central export point for handler entry functions | ~50 |
| handlers/types.ts | Type definitions for handler access to state | ~50 |
| handlers/semantics.ts | ANSI sequence semantics and utilities | ~100 |
| handlers/csi_cursor.ts | Cursor movement CSI handlers | ~150 |
| handlers/csi_screen.ts | Screen erase CSI handlers | ~100 |
| handlers/csi_edit.ts | Insert/delete CSI handlers | ~80 |
| handlers/csi_scrolling.ts | Scroll CSI handlers | ~80 |
| handlers/csi_char_attrs.ts | SGR attribute handler | ~30 |
| handlers/csi_modes.ts | Mode set/reset handler | ~80 |
| handlers/csi_device.ts | Device response handlers | ~80 |
| handlers/esc_handlers.ts | ESC sequence handlers | ~150 |
| handlers/osc_handlers.ts | OSC sequence handlers | ~150 |
| handlers/c0_handlers.ts | C0 control handlers | ~100 |
| handlers/print_handler.ts | Character print handlers | ~200 |
| state.ts | Data container + dispatch | ~300-500 |

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- Each handler file has corresponding .test.ts
- Test handlers in isolation with minimal state setup

**Test Coverage Goals**:
- Handler modules: 90%+ coverage (core logic)
- state.ts dispatch: Covered by integration tests

**Key Test Areas**:

1. **Cursor Handlers** (`csi_cursor.test.ts`)
   - Movement with default parameters
   - Movement at boundaries (row 0, max row, col 0, max col)
   - wrapPending cleared after each operation

2. **Screen Handlers** (`csi_screen.test.ts`)
   - Each erase mode (Below, Above, All, Scrollback)
   - Edge cases: cursor at boundaries

3. **Edit Handlers** (`csi_edit.test.ts`)
   - Insert/delete at various positions
   - Interaction with scroll regions

4. **Print Handler** (`print_handler.test.ts`)
   - ASCII fast path
   - Wide character handling
   - Character set translation

### Integration Testing

**Existing Tests** (must pass unchanged):
- `state.test.ts` (661 lines)
- `state.phase5.test.ts`
- `state.phase6.test.ts`

**Scenarios**:
- Complex sequences involving multiple handlers
- Buffer switching with cursor save/restore
- Full terminal operation workflows

### Performance Testing

**Existing Tests**:
- `performance.test.ts`

**Criteria**:
- No measurable regression from baseline
- Handler function call overhead acceptable

## Dependencies

### External Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| bun | 1.0+ | Test runner, bundler |
| typescript | 5.0+ | Type checking |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Infrastructure (no dependencies)
2. Phase 2: CSI handlers (depends on Phase 1)
3. Phase 3: Other handlers (depends on Phase 1)
4. Phase 4: state.ts refactor (depends on Phase 2 & 3)
5. Phase 5: Verification (depends on Phase 4)

**Component Dependencies**:
- All handlers depend on `handlers/types.ts` and `handlers/semantics.ts`
- CSI handlers depend on existing `cursor.ts`, `modes.ts`, `sgr.ts`, `attributes.ts`, `buffer.ts`
- Print handler depends on `unicode.ts`, `grid.ts`
- state.ts depends on all handlers via `handlers/index.ts`

## Risk Assessment

### Technical Risks

1. **Property Visibility Changes**
   - **Risk**: Changing private to accessible may break encapsulation
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Use interface to define handler access, keep public API unchanged

2. **Handler Function Call Overhead**
   - **Risk**: Additional function calls may impact performance
   - **Likelihood**: Low (JS engines optimize well)
   - **Impact**: Low
   - **Mitigation**: Run performance tests, optimize hot paths if needed

3. **Circular Dependencies**
   - **Risk**: Handlers importing state, state importing handlers
   - **Likelihood**: Low (one-way dependency)
   - **Impact**: Medium
   - **Mitigation**: Handlers receive state as parameter, no imports of state

### Implementation Risks

1. **Test Regression**
   - **Risk**: Breaking existing functionality
   - **Likelihood**: Medium
   - **Impact**: High
   - **Mitigation**: All existing tests must pass unchanged, incremental refactoring

2. **Incomplete Handler Extraction**
   - **Risk**: Missing edge cases in handler logic
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Extract logic verbatim, add comprehensive unit tests

## Performance Considerations

1. **Fast Path Preservation**
   - Maintain ASCII fast path in print handler
   - Avoid unnecessary function calls for common operations

2. **State Access**
   - Handlers receive state reference, no copying
   - Direct property mutation (no immutable patterns)

3. **Handler Dispatch**
   - Single dispatch in processAction
   - Second dispatch in handler modules (e.g., handleCsi to handleCursorUp)

4. **Tab Stop Handling (handleTab)**
   - Tab stops stored in Set<number> for O(1) lookup
   - handleTab iterates from cursor position to find next stop
   - For typical use (8-column tabs), iteration is bounded and fast
   - Edge case: Many custom tab stops may require linear scan

## Security Considerations

- No new security concerns (refactoring only)
- Input validation unchanged
- No new external inputs

## Open Questions

### From Specification
- None listed

### Implementation-Specific
- None identified

## Future Enhancements

Items not in current scope:
- Handler registration system (dynamic dispatch)
- Plugin architecture for custom handlers
- State snapshot/restore functionality

## Success Metrics

### Functional Completeness
- [ ] All existing tests pass without modification
- [ ] TerminalState public API unchanged
- [ ] All handlers extracted to `src/terminal/handlers/`

### Quality Metrics
- [ ] state.ts reduced to 300-500 lines
- [ ] Each handler file under 300 lines
- [ ] New handler tests written
- [ ] TypeScript strict mode passes

### Performance Metrics
- [ ] No measurable performance regression
- [ ] Performance tests pass

## References

- **Specification**: `doc/tasks/202601-refactoring/SPEC.md`
- **Current state.ts**: `src/terminal/state.ts` (1225 lines)
- **Type definitions**: `src/types/terminal.ts`
- **Existing modules**: cursor.ts, modes.ts, sgr.ts, attributes.ts, buffer.ts, grid.ts, unicode.ts

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm approach and phase division
   - Address any open questions

2. **Begin Implementation**
   - Start with Phase 1 (infrastructure)
   - Follow TDD approach where possible
   - Commit incrementally per phase

3. **Continuous Verification**
   - Run tests after each phase
   - Verify no regressions
