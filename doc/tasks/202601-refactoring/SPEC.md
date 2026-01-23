# Feature: terminal/state.ts Refactoring

## Overview

Refactor the monolithic `TerminalState` class (~1200 lines) into a data-oriented architecture with externalized handler functions organized by functional category. This improves maintainability, testability, and enables systematic addition of new terminal sequences.

## Objectives

- Transform TerminalState into a pure data structure with dispatch logic only
- Extract sequence handlers into external functions grouped by category
- Create corresponding test files for each handler module
- Maintain all existing public APIs for backward compatibility

## Technical Requirements

### Functional Requirements

- **FR1:** TerminalState class retains data properties (buffers, cursor, modes, etc.) and processAction dispatch
- **FR2:** All sequence handlers extracted to `src/terminal/handlers/` directory
- **FR3:** Handler functions are pure functions receiving state and returning void (mutating state in place)
- **FR4:** Existing modules (cursor.ts, modes.ts, sgr.ts) remain unchanged
- **FR5:** Each handler file has a corresponding test file

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable performance regression
- **NFR2 - Maintainability:** Each file under 300 lines
- **NFR3 - Compatibility:** All existing tests pass without modification

## Implementation Approach

### Architecture

**Current Architecture:**
```
TerminalState (monolithic)
├── Data: buffers, cursor, modes, etc.
├── processAction(): dispatcher
├── handlePrint(): inline
├── handleExecute(): inline
├── handleCsi(): inline (large switch)
├── handleEsc(): inline
├── handleOsc(): inline
├── handleApc(): inline
└── handleDcs(): inline
```

**Target Architecture:**
```
TerminalState (data + dispatch)
├── Data: buffers, cursor, modes, etc.
├── processAction(): dispatcher → handlers
└── Utility methods: getDirtyRows, clearDirty, resize, etc.

handlers/
├── csi_cursor.ts      → cursor movement handlers
├── csi_screen.ts      → screen operation handlers
├── csi_edit.ts        → edit operation handlers
├── csi_scrolling.ts   → scroll operation handlers
├── csi_char_attrs.ts  → SGR handlers
├── csi_modes.ts       → mode setting handlers
├── csi_device.ts      → device response handlers
├── esc_handlers.ts    → ESC sequence handlers
├── osc_handlers.ts    → OSC sequence handlers
├── c0_handlers.ts     → C0 control handlers
└── print_handler.ts   → character print handler
```

### File Structure

```
src/terminal/
├── handlers/
│   ├── index.ts              # Re-exports all handlers
│   ├── types.ts              # Handler-specific types
│   ├── semantics.ts          # CSI defaults, clamping, 0/1-index conversion
│   ├── semantics.test.ts     # Tests for semantics utilities
│   ├── csi_cursor.ts         # CursorUp, CursorDown, CursorForward, CursorBack,
│   │                         # CursorNextLine, CursorPreviousLine,
│   │                         # CursorHorizontalAbsolute, CursorVerticalAbsolute,
│   │                         # CursorPosition
│   ├── csi_cursor.test.ts
│   ├── csi_screen.ts         # EraseInDisplay, EraseInLine, EraseCharacters
│   ├── csi_screen.test.ts
│   ├── csi_edit.ts           # InsertLines, DeleteLines, InsertCharacters, DeleteCharacters
│   ├── csi_edit.test.ts
│   ├── csi_scrolling.ts      # ScrollUp, ScrollDown, SetScrollRegion
│   ├── csi_scrolling.test.ts
│   ├── csi_char_attrs.ts     # Sgr (delegates to existing sgr.ts + attributes.ts)
│   ├── csi_char_attrs.test.ts
│   ├── csi_modes.ts          # SetMode, ResetMode (delegates to existing modes.ts)
│   ├── csi_modes.test.ts
│   ├── csi_device.ts         # DeviceStatusReport, PrimaryDeviceAttributes,
│   │                         # SecondaryDeviceAttributes, TertiaryDeviceAttributes
│   ├── csi_device.test.ts
│   ├── esc_handlers.ts       # SaveCursor, RestoreCursor, Index, NextLine,
│   │                         # ReverseIndex, HorizontalTabSet, ResetToInitialState,
│   │                         # SetG0CharSet, SetG1CharSet
│   ├── esc_handlers.test.ts
│   ├── osc_handlers.ts       # SetTitle, SetIconName, SetTitleAndIcon, SetColorPalette,
│   │                         # SetWorkingDirectory, Hyperlink, SetForegroundColor,
│   │                         # SetBackgroundColor, EmtermExtension
│   ├── osc_handlers.test.ts
│   ├── c0_handlers.ts        # BEL, BS, HT, LF, VT, FF, CR, SO, SI
│   ├── c0_handlers.test.ts
│   ├── print_handler.ts      # handlePrint, handlePrintSlow, translateCharacter
│   └── print_handler.test.ts
├── state.ts                  # Refactored TerminalState
├── cursor.ts                 # Existing (unchanged)
├── modes.ts                  # Existing (unchanged)
├── sgr.ts                    # Existing (unchanged)
├── attributes.ts             # Existing (unchanged)
├── buffer.ts                 # Existing (unchanged)
├── grid.ts                   # Existing (unchanged)
├── unicode.ts                # Existing (unchanged)
└── ...
```

### Handler Function Signatures

**Standard Pattern:**
```typescript
// Each handler receives TerminalState and action-specific data
export function handleCursorUp(state: TerminalState, count: number): void {
  state.cursor.moveUp(count);
  state.wrapPending = false;
}

export function handleCursorPosition(
  state: TerminalState,
  row: number,
  col: number
): void {
  state.cursor.moveTo(col, row);
  state.wrapPending = false;
}
```

**For handlers needing buffer access:**
```typescript
export function handleEraseInDisplay(
  state: TerminalState,
  mode: "Below" | "Above" | "All" | "Scrollback"
): void {
  const buffer = state.getActiveBuffer();
  switch (mode) {
    case "Below":
      buffer.clearBelow(state.cursor.col, state.cursor.row);
      break;
    // ...
  }
}
```

### Semantics Module (semantics.ts)

Centralize ANSI sequence semantics:

```typescript
/**
 * Default parameter values for CSI sequences.
 * Most count parameters default to 1.
 */
export const CSI_DEFAULTS = {
  CursorUp: 1,
  CursorDown: 1,
  CursorForward: 1,
  CursorBack: 1,
  CursorNextLine: 1,
  CursorPreviousLine: 1,
  CursorHorizontalAbsolute: 1,  // Column 1 (1-indexed)
  CursorVerticalAbsolute: 1,    // Row 1 (1-indexed)
  CursorPosition: { row: 1, col: 1 },
  EraseCharacters: 1,
  InsertLines: 1,
  DeleteLines: 1,
  InsertCharacters: 1,
  DeleteCharacters: 1,
  ScrollUp: 1,
  ScrollDown: 1,
} as const;

/**
 * Convert 1-indexed ANSI parameter to 0-indexed.
 */
export function toZeroIndexed(value: number): number {
  return Math.max(0, value - 1);
}

/**
 * Clamp position to valid screen coordinates.
 */
export function clampPosition(
  col: number,
  row: number,
  cols: number,
  rows: number
): { col: number; row: number } {
  return {
    col: Math.max(0, Math.min(cols - 1, col)),
    row: Math.max(0, Math.min(rows - 1, row)),
  };
}

/**
 * Get default value for a CSI action parameter.
 * Per ANSI standard, 0 is treated as "use default" (same as omitted).
 */
export function getDefault<T extends keyof typeof CSI_DEFAULTS>(
  action: T,
  value: typeof CSI_DEFAULTS[T] | undefined
): typeof CSI_DEFAULTS[T] {
  return value || CSI_DEFAULTS[action];
}
```

### Refactored state.ts Structure

```typescript
import {
  handlePrint,
  handleExecute,
  handleCsi,
  handleEsc,
  handleOsc,
  handleApc,
  handleDcs,
} from "./handlers/index.ts";

export class TerminalState {
  // ===== Data Properties =====
  private primaryBuffer: ScreenBuffer;
  private alternateBuffer: ScreenBuffer | null = null;
  private useAlternate: boolean = false;
  private primaryCursor: CursorState;
  private alternateCursor: CursorState | null = null;
  cursor: CursorState;  // Made accessible to handlers
  modes: TerminalModes;
  wrapPending: boolean = false;
  tabStops: Set<number>;
  // ... other data properties

  // ===== Constructor =====
  constructor(cols: number, rows: number) { /* ... */ }

  // ===== Public API (unchanged) =====
  get cols(): number { /* ... */ }
  get rows(): number { /* ... */ }
  get cursorCol(): number { /* ... */ }
  get cursorRow(): number { /* ... */ }
  // ... other getters

  processAction(action: TerminalAction): void {
    switch (action.type) {
      case "Print":
        handlePrint(this, action.value);
        break;
      case "Execute":
        handleExecute(this, action.value);
        break;
      case "Csi":
        handleCsi(this, action.value);
        break;
      case "Esc":
        handleEsc(this, action.value);
        break;
      case "Osc":
        handleOsc(this, action.value);
        break;
      case "Apc":
        handleApc(this, action.value);
        break;
      case "Dcs":
        handleDcs(this, action.value);
        break;
    }
  }

  // ===== Buffer Access Methods =====
  getActiveBuffer(): ScreenBuffer { /* ... */ }
  switchToAlternateBuffer(saveCursor?: boolean): void { /* ... */ }
  switchToPrimaryBuffer(restoreCursor?: boolean): void { /* ... */ }

  // ===== Utility Methods =====
  getDirtyRows(): number[] { /* ... */ }
  clearDirty(): void { /* ... */ }
  resize(cols: number, rows: number): void { /* ... */ }
  reset(): void { /* ... */ }
  extractText(startCol: number, startRow: number, endCol: number, endRow: number): string { /* ... */ }

  // ===== Tab Stop Methods (used by handlers) =====
  createDefaultTabStops(cols: number): Set<number> { /* ... */ }
  handleTab(): void { /* ... */ }
  setTabStop(): void { /* ... */ }
  clearTabStop(): void { /* ... */ }
  clearAllTabStops(): void { /* ... */ }

  // ===== Response Methods =====
  takePendingResponse(): Uint8Array | null { /* ... */ }
  addPendingResponse(response: Uint8Array): void { /* ... */ }
}
```

### Handler Migration Checklist

| Current Method | Target File | Handler Function |
|----------------|-------------|------------------|
| handlePrint | print_handler.ts | handlePrint, handlePrintSlow |
| translateCharacter | print_handler.ts | translateCharacter |
| translateLineDrawing | print_handler.ts | translateLineDrawing |
| handleExecute | c0_handlers.ts | handleExecute (dispatcher) |
| handleTab | c0_handlers.ts | handleTab |
| handleCsi | handlers/index.ts | handleCsi (dispatcher) |
| CursorUp/Down/etc | csi_cursor.ts | handleCursorUp, handleCursorDown, etc. |
| EraseInDisplay/Line | csi_screen.ts | handleEraseInDisplay, handleEraseInLine, etc. |
| InsertLines/DeleteLines | csi_edit.ts | handleInsertLines, handleDeleteLines, etc. |
| ScrollUp/Down | csi_scrolling.ts | handleScrollUp, handleScrollDown, etc. |
| Sgr | csi_char_attrs.ts | handleSgr |
| SetMode/ResetMode | csi_modes.ts | handleSetMode |
| DeviceStatusReport | csi_device.ts | handleDeviceStatusReport, etc. |
| handleEsc | esc_handlers.ts | handleEsc (dispatcher) |
| handleOsc | osc_handlers.ts | handleOsc (dispatcher) |
| handleApc | handlers/index.ts | handleApc |
| handleDcs | handlers/index.ts | handleDcs |

## Test Scenarios

### Unit Tests (per handler file)

**csi_cursor.test.ts:**
- [ ] CursorUp moves cursor up by specified count
- [ ] CursorUp at row 0 stays at row 0
- [ ] CursorDown moves cursor down by specified count
- [ ] CursorDown at bottom row stays at bottom
- [ ] CursorForward moves cursor right
- [ ] CursorBack moves cursor left
- [ ] CursorNextLine moves down and to column 0
- [ ] CursorPreviousLine moves up and to column 0
- [ ] CursorHorizontalAbsolute sets column (1-indexed input)
- [ ] CursorVerticalAbsolute sets row (1-indexed input)
- [ ] CursorPosition sets absolute position (1-indexed input)
- [ ] All cursor operations clear wrapPending flag

**csi_screen.test.ts:**
- [ ] EraseInDisplay Below clears from cursor to end
- [ ] EraseInDisplay Above clears from start to cursor
- [ ] EraseInDisplay All clears entire screen
- [ ] EraseInLine Below clears from cursor to line end
- [ ] EraseInLine Above clears from line start to cursor
- [ ] EraseInLine All clears entire line
- [ ] EraseCharacters erases N characters without shifting

**csi_edit.test.ts:**
- [ ] InsertLines inserts blank lines at cursor row
- [ ] DeleteLines deletes lines and shifts content up
- [ ] InsertCharacters inserts blanks and shifts content right
- [ ] DeleteCharacters deletes and shifts content left

**csi_scrolling.test.ts:**
- [ ] ScrollUp scrolls content up by N lines
- [ ] ScrollDown scrolls content down by N lines
- [ ] SetScrollRegion sets scroll region and moves cursor home

**csi_modes.test.ts:**
- [ ] SetMode enables specified DEC private modes
- [ ] ResetMode disables specified DEC private modes
- [ ] Mode 1049 saves cursor and switches to alternate buffer
- [ ] Mode 1049 reset switches back and restores cursor

**print_handler.test.ts:**
- [ ] ASCII character prints at cursor position
- [ ] Wide character advances cursor by 2
- [ ] Line drawing characters are translated
- [ ] Wrap pending triggers line wrap on next print

### Integration Tests

**state.test.ts (existing):**
- [ ] All existing tests continue to pass
- [ ] Complex sequences involving multiple handlers work correctly

## Security Considerations

- No new security concerns (refactoring only, no new functionality)
- Input validation remains in place for CSI parameters

## Success Criteria

- [ ] All existing tests pass without modification
- [ ] TerminalState public API remains unchanged
- [ ] `src/terminal/handlers/` directory contains all handler modules
- [ ] Each handler module has corresponding test file
- [ ] `state.ts` reduced to ~300-500 lines (dispatch + data only)
- [ ] TypeScript strict mode passes
- [ ] No performance regression measurable in existing performance tests

## Implementation Phases

### Phase 1: Infrastructure Setup
**Goals:** Create handlers directory structure and types
**Deliverables:**
- `handlers/` directory created
- `handlers/index.ts` with export structure
- `handlers/types.ts` with handler-specific types
- `handlers/semantics.ts` with CSI defaults and utilities

### Phase 2: Extract CSI Handlers
**Goals:** Extract all CSI sequence handlers
**Deliverables:**
- csi_cursor.ts with cursor movement handlers
- csi_screen.ts with erase handlers
- csi_edit.ts with insert/delete handlers
- csi_scrolling.ts with scroll handlers
- csi_char_attrs.ts with SGR handler
- csi_modes.ts with mode handlers
- csi_device.ts with device response handlers
- Corresponding test files for each

### Phase 3: Extract Other Handlers
**Goals:** Extract ESC, OSC, C0, Print handlers
**Deliverables:**
- esc_handlers.ts with ESC sequence handlers
- osc_handlers.ts with OSC sequence handlers
- c0_handlers.ts with C0 control handlers
- print_handler.ts with character print logic
- Corresponding test files for each

### Phase 4: Refactor state.ts
**Goals:** Simplify state.ts to data + dispatch only
**Deliverables:**
- Refactored state.ts using external handlers
- Verify all existing tests pass
- Update internal method visibility as needed

### Phase 5: Verification and Cleanup
**Goals:** Ensure quality and documentation
**Deliverables:**
- All tests passing
- Performance verification
- Clean up any unused code

## References

- Current state.ts: `src/terminal/state.ts`
- Existing modules: cursor.ts, modes.ts, sgr.ts, attributes.ts
- Test file: `src/terminal/state.test.ts`
- Type definitions: `src/types/terminal.ts`
