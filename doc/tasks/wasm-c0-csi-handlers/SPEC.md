# Feature: WASM C0 + CSI Cursor + CSI Screen Handlers (Sprint 3)

## Overview

Port C0 control character handlers, CSI cursor movement handlers, and CSI screen erase handlers from TypeScript to Rust/WebAssembly. This is Sprint 3 of the WASM migration roadmap (`tmp/wasm.md`). Combined with Sprint 2's Print handler, this brings 95%+ of all terminal actions under WASM processing.

## Objectives

- Move C0 control handlers (BEL, BS, HT, LF/VT/FF, CR, SO, SI) to WASM
- Move CSI cursor handlers (CUU/CUD/CUF/CUB/CNL/CPL/CHA/CUP/VPA) to WASM
- Move CSI screen handlers (ED, EL, ECH) to WASM
- Achieve 95%+ terminal actions processing in WASM
- Complete HT (tab) processing entirely within WASM using existing `tab_stops: Vec<bool>`

## User Stories

### US1: C0 Control Codes via WASM

As a terminal handler, I want C0 control codes to be processed entirely within WASM, so that the most frequent non-Print actions have zero TS-WASM boundary overhead.

**Acceptance Criteria:**
- [ ] `core.handle_execute(byte)` processes BEL, BS, HT, LF/VT/FF, CR, SO, SI
- [ ] Returns scroll count (LF/VT/FF can return 1+ when at scroll region bottom)
- [ ] BEL returns sentinel value 0xFE, TS dispatches `onBell` callback
- [ ] BS decrements cursor col, clears wrapPending
- [ ] HT finds next tab stop using WASM-internal tab_stops
- [ ] LF/VT/FF delegates to existing `line_feed()`, clears wrapPending
- [ ] CR sets cursor col to 0, clears wrapPending
- [ ] SO/SI switch active charset (0=G0, 1=G1)

### US2: CSI Cursor Movement via WASM

As a terminal handler, I want cursor movement CSI sequences to be processed entirely within WASM, so that cursor positioning is handled in a single WASM call instead of 2-4 boundary crossings.

**Acceptance Criteria:**
- [ ] CUU (A): cursor up by count, clamped to row 0
- [ ] CUD (B): cursor down by count, clamped to rows-1
- [ ] CUF (C): cursor right by count, clamped to cols-1
- [ ] CUB (D): cursor left by count, clamped to col 0
- [ ] CNL (E): cursor down by count + col to 0
- [ ] CPL (F): cursor up by count + col to 0
- [ ] CHA (G): set cursor col (1-indexed input → 0-indexed, clamped)
- [ ] CUP (H): set cursor row and col (1-indexed input → 0-indexed, clamped)
- [ ] VPA (d): set cursor row (1-indexed input → 0-indexed, clamped)
- [ ] All operations clear wrapPending

### US3: CSI Screen Erase via WASM

As a terminal handler, I want screen erase operations to be processed entirely within WASM, so that ED/EL/ECH execute in a single WASM call instead of N per-row calls.

**Acceptance Criteria:**
- [ ] ED (J): Erase in Display - Below(0), Above(1), All(2) modes
- [ ] ED Scrollback(3): Returns sentinel value, TS handles scrollback clearing
- [ ] EL (K): Erase in Line - ToEnd(0), ToStart(1), All(2) modes
- [ ] ECH (X): Erase N characters at cursor position
- [ ] All erase operations write Cell::EMPTY and mark rows dirty

### US4: JS Fallback

As the terminal state manager, I want the JS fallback path to remain functional when WASM is unavailable.

**Acceptance Criteria:**
- [ ] When `getActiveWasmGrid()` returns null, existing TS handlers are used
- [ ] No changes to existing TS handler implementations
- [ ] All existing tests pass with and without WASM

## Technical Requirements

### Functional Requirements

- **FR1:** `TerminalCore.handle_execute(byte: u8) -> u8` dispatches C0 controls and returns scroll count
- **FR2:** BEL (0x07) returns sentinel value `0xFE` from `handle_execute`; TS dispatches `state.onBell()` callback
- **FR3:** BS (0x08) decrements `cursor.col` (clamped to 0) and clears `wrap_pending`
- **FR4:** HT (0x09) finds next tab stop in `tab_stops: Vec<bool>` and moves cursor
- **FR5:** LF/VT/FF (0x0A/0x0B/0x0C) calls internal `line_feed()`, clears `wrap_pending`
- **FR6:** CR (0x0D) sets `cursor.col = 0`, clears `wrap_pending`
- **FR7:** SO (0x0E) sets `active_charset = 1`; SI (0x0F) sets `active_charset = 0`
- **FR8:** `TerminalCore.handle_cursor_up(count: u16)` - cursor up with clamp and wrapPending clear
- **FR9:** `TerminalCore.handle_cursor_down(count: u16)` - cursor down with clamp and wrapPending clear
- **FR10:** `TerminalCore.handle_cursor_forward(count: u16)` - cursor right with clamp and wrapPending clear
- **FR11:** `TerminalCore.handle_cursor_back(count: u16)` - cursor left with clamp and wrapPending clear
- **FR12:** `TerminalCore.handle_cursor_next_line(count: u16)` - cursor down + col=0 and wrapPending clear
- **FR13:** `TerminalCore.handle_cursor_previous_line(count: u16)` - cursor up + col=0 and wrapPending clear
- **FR14:** `TerminalCore.handle_cursor_horizontal_absolute(col: u16)` - set col (1-indexed input), wrapPending clear
- **FR15:** `TerminalCore.handle_cursor_position(row: u16, col: u16)` - set row and col (1-indexed inputs), wrapPending clear
- **FR16:** `TerminalCore.handle_cursor_vertical_absolute(row: u16)` - set row (1-indexed input), wrapPending clear
- **FR17:** `TerminalCore.handle_erase_in_display(mode: u8)` - erase display (0=Below, 1=Above, 2=All, 3=Scrollback returns 0xFF sentinel)
- **FR18:** `TerminalCore.handle_erase_in_line(mode: u8)` - erase line (0=ToEnd, 1=ToStart, 2=All)
- **FR19:** `TerminalCore.handle_erase_characters(count: u16)` - erase N chars at cursor position
- **FR20:** TS `processAction()` updated: Execute and qualifying CSI actions route to WASM when available
- **FR21:** JS fallback: when `getActiveWasmGrid()` is null, existing TS handlers are used unchanged

### Non-Functional Requirements

- **NFR1 - Performance:** Each C0/CSI operation completes in 1 WASM call (vs. 2-4 boundary crossings currently)
- **NFR2 - Performance:** ED clearAll in 1 WASM call (vs. rows×1 calls currently)
- **NFR3 - Compatibility:** All existing TypeScript tests pass (1824+)
- **NFR4 - Binary size:** WASM binary increase < 5KB over Sprint 2 baseline (43.6KB)
- **NFR5 - Compatibility:** JS fallback path unchanged and functional
- **NFR6 - Compatibility:** vttest basic tests unchanged

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ wasm/ (Rust Crate - extended from Sprint 2)     │
│                                                  │
│  src/terminal_core.rs (MODIFIED)                 │
│    + handle_execute(byte: u8) -> u8              │
│    + handle_cursor_up/down/forward/back(u16)     │
│    + handle_cursor_next_line/previous_line(u16)  │
│    + handle_cursor_horizontal_absolute(u16)      │
│    + handle_cursor_position(u16, u16)            │
│    + handle_cursor_vertical_absolute(u16)        │
│    + handle_erase_in_display(u8) -> u8           │
│    + handle_erase_in_line(u8)                    │
│    + handle_erase_characters(u16)                │
│    + find_next_tab_stop() (internal)             │
│                                                  │
│  src/lib.rs (UNCHANGED)                           │
│  src/cell.rs (UNCHANGED)                         │
│  src/unicode.rs (UNCHANGED)                      │
└──────────────────────┬──────────────────────────┘
                       │ wasm_bindgen
                       ↓
┌─────────────────────────────────────────────────┐
│ src/terminal/ (TypeScript - MODIFIED)           │
│                                                  │
│  state.ts (MODIFIED)                             │
│    - processAction: WASM paths for Execute/CSI   │
│                                                  │
│  wasm/terminal-core.ts (UNCHANGED)               │
│                                                  │
│  handlers/c0_handlers.ts (UNCHANGED - fallback)  │
│  handlers/csi_cursor.ts (UNCHANGED - fallback)   │
│  handlers/csi_screen.ts (UNCHANGED - fallback)   │
│  handlers/index.ts (UNCHANGED)                   │
└─────────────────────────────────────────────────┘
```

### Data Flow

**C0 Execute Path (WASM, LF example):**
```
TS processAction("Execute", 0x0A)
  → grid = getActiveWasmGrid()
  → scrollCount = grid.core.handle_execute(0x0A)  [single WASM call]
  → scrollCount == 1
  → buffer.scrollUp()  [TS: WASM row → JS Line, shift, clear]

  Inside WASM handle_execute:
    → match byte { 0x0A => self.line_feed_with_wrap_clear() }
    → self.line_feed() returns true (at scroll_region_bottom)
    → self.wrap_pending = false
    → return 1
```

**C0 Execute Path (WASM, BEL example):**
```
TS processAction("Execute", 0x07)
  → result = grid.core.handle_execute(0x07)  [single WASM call]
  → result == 0xFE → BEL sentinel
  → state.onBell?.()

  Inside WASM handle_execute:
    → match byte { 0x07 => return 0xFE }  [BEL sentinel]
```

**CSI Cursor Path (WASM):**
```
TS processAction("Csi", { action: "CursorUp", data: 5 })
  → grid = getActiveWasmGrid()
  → grid.core.handle_cursor_up(5)  [single WASM call]
  → done (no scroll possible)

  Inside WASM handle_cursor_up:
    → self.cursor.row = self.cursor.row.saturating_sub(5).max(0)
    → self.wrap_pending = false
```

**CSI Screen Path (WASM, ED Below):**
```
TS processAction("Csi", { action: "EraseInDisplay", data: "Below" })
  → grid = getActiveWasmGrid()
  → result = grid.core.handle_erase_in_display(0)  [single WASM call]
  → result != 0xFF → done

  Inside WASM handle_erase_in_display:
    → mode 0 (Below):
      → clear_line_range(cursor.row, cursor.col, cols)
      → for r in cursor.row+1..rows: clear_line(r)
      → return 0
```

**CSI Screen Path (ED Scrollback - direct clearScrollback):**
```
TS processAction("Csi", { action: "EraseInDisplay", data: "Scrollback" })
  → grid = getActiveWasmGrid()
  → result = grid.core.handle_erase_in_display(3)  [single WASM call]
  → result == 0xFF → call buffer.clearScrollback() directly
  Note: Do NOT fall back to existing handleEraseInDisplay() which has a
  known bug calling clearAll() instead of clearScrollback().
```

### WASM API Additions

```rust
#[wasm_bindgen]
impl TerminalCore {
    // ── C0 Control Handler (NEW) ──────────────────────────

    /// Handle a C0 Execute action.
    /// Returns: scroll count (0-N for LF/VT/FF), 0xFE for BEL sentinel.
    pub fn handle_execute(&mut self, byte: u8) -> u8;

    // ── CSI Cursor Handlers (NEW) ─────────────────────────

    /// CSI A - Cursor Up. count is the number of rows (already 0-default-adjusted by TS).
    pub fn handle_cursor_up(&mut self, count: u16);

    /// CSI B - Cursor Down
    pub fn handle_cursor_down(&mut self, count: u16);

    /// CSI C - Cursor Forward
    pub fn handle_cursor_forward(&mut self, count: u16);

    /// CSI D - Cursor Back
    pub fn handle_cursor_back(&mut self, count: u16);

    /// CSI E - Cursor Next Line
    pub fn handle_cursor_next_line(&mut self, count: u16);

    /// CSI F - Cursor Previous Line
    pub fn handle_cursor_previous_line(&mut self, count: u16);

    /// CSI G - Cursor Horizontal Absolute (1-indexed input).
    pub fn handle_cursor_horizontal_absolute(&mut self, col: u16);

    /// CSI H - Cursor Position (1-indexed inputs).
    pub fn handle_cursor_position(&mut self, row: u16, col: u16);

    /// CSI d - Cursor Vertical Absolute (1-indexed input).
    pub fn handle_cursor_vertical_absolute(&mut self, row: u16);

    // ── CSI Screen Handlers (NEW) ─────────────────────────

    /// CSI J - Erase in Display.
    /// mode: 0=Below, 1=Above, 2=All, 3=Scrollback.
    /// Returns 0 on success, 0xFF if TS fallback is needed (Scrollback).
    pub fn handle_erase_in_display(&mut self, mode: u8) -> u8;

    /// CSI K - Erase in Line.
    /// mode: 0=ToEnd, 1=ToStart, 2=All.
    pub fn handle_erase_in_line(&mut self, mode: u8);

    /// CSI X - Erase Characters.
    /// count: number of characters to erase (default 1).
    pub fn handle_erase_characters(&mut self, count: u16);
}
```

### Internal Rust Implementation (non-exported)

```rust
impl TerminalCore {
    /// Find the next tab stop after the current cursor column.
    /// Returns the tab stop column, or cols-1 if no more stops.
    fn find_next_tab_stop(&self) -> u16;

    /// Execute LF: line_feed + clear wrap_pending.
    /// Returns 1 if scroll needed, 0 otherwise.
    fn execute_line_feed(&mut self) -> u8;

    /// Convert 1-indexed ANSI parameter to 0-indexed, clamped.
    fn to_zero_indexed_col(&self, col: u16) -> u16;
    fn to_zero_indexed_row(&self, row: u16) -> u16;
}
```

### TS processAction Changes

```typescript
processAction(action: TerminalAction): void {
  // Flush grapheme buffer before non-Print actions (unchanged)
  if (action.type !== "Print") { /* ... */ }

  switch (action.type) {
    case "Print": { /* unchanged Sprint 2 WASM path */ }

    case "Execute": {
      const grid = this.getActiveWasmGrid();
      if (grid) {
        const result = grid.core.handle_execute(action.value);
        if (result === 0xFE) {
          // BEL sentinel
          this.onBell?.();
        } else if (result > 0) {
          const buffer = this.getActiveBuffer();
          for (let i = 0; i < result; i++) {
            buffer.scrollUp();
          }
        }
      } else {
        handleExecute(this, action.value);
      }
      break;
    }

    case "Csi": {
      const grid = this.getActiveWasmGrid();
      if (grid && this.handleCsiWasm(grid, action.value)) {
        break; // Handled by WASM
      }
      handleCsi(this, action.value); // Fallback to TS
      break;
    }

    // Esc, Osc, Apc, Dcs unchanged
  }
}

/**
 * Try to handle a CSI action via WASM.
 * Returns true if handled, false if TS fallback needed.
 */
private handleCsiWasm(grid: WasmGrid, action: CsiAction): boolean {
  switch (action.action) {
    case "CursorUp":
      grid.core.handle_cursor_up(action.data || 1);
      return true;
    case "CursorDown":
      grid.core.handle_cursor_down(action.data || 1);
      return true;
    case "CursorForward":
      grid.core.handle_cursor_forward(action.data || 1);
      return true;
    case "CursorBack":
      grid.core.handle_cursor_back(action.data || 1);
      return true;
    case "CursorNextLine":
      grid.core.handle_cursor_next_line(action.data || 1);
      return true;
    case "CursorPreviousLine":
      grid.core.handle_cursor_previous_line(action.data || 1);
      return true;
    case "CursorHorizontalAbsolute":
      grid.core.handle_cursor_horizontal_absolute(action.data || 1);
      return true;
    case "CursorPosition":
      grid.core.handle_cursor_position(
        action.data.row || 1,
        action.data.col || 1
      );
      return true;
    case "CursorVerticalAbsolute":
      grid.core.handle_cursor_vertical_absolute(action.data || 1);
      return true;
    case "EraseInDisplay": {
      const mode = eraseModeToByte(action.data);
      const result = grid.core.handle_erase_in_display(mode);
      if (result === 0xFF) {
        // Scrollback: call clearScrollback() directly
        // (existing TS handler has a bug calling clearAll() instead)
        const buffer = this.getActiveBuffer();
        buffer.clearScrollback();
        return true;
      }
      return true;
    }
    case "EraseInLine": {
      const mode = eraseModeToByte(action.data);
      grid.core.handle_erase_in_line(mode);
      return true;
    }
    case "EraseCharacters":
      grid.core.handle_erase_characters(action.data || 1);
      return true;
    default:
      return false; // Not handled by Sprint 3 WASM
  }
}

function eraseModeToByte(mode: EraseMode): number {
  switch (mode) {
    case "Below": return 0;
    case "Above": return 1;
    case "All": return 2;
    case "Scrollback": return 3;
  }
}
```

### BEL Notification via Return Value

BEL is handled via the `handle_execute` return value rather than a WASM-to-JS callback. When `handle_execute(0x07)` is called, it returns `0xFE` as a sentinel. The TS dispatch layer checks for this value and invokes `state.onBell?.()`. This avoids global callback registration issues in multi-tab architectures.

### Sentinel Constants

Sentinel values used in WASM-to-TS communication must be defined as named constants in both Rust and TypeScript to avoid magic numbers:

**Rust (wasm/src/terminal_core.rs):**
```rust
const BEL_SENTINEL: u8 = 0xFE;
const SCROLLBACK_SENTINEL: u8 = 0xFF;
```

**TypeScript (src/terminal/state.ts):**
```typescript
const WASM_BEL_SENTINEL = 0xFE;
const WASM_SCROLLBACK_SENTINEL = 0xFF;
```

### File Structure Changes

```
wasm/src/
├── lib.rs              # UNCHANGED
├── unicode.rs          # UNCHANGED
├── terminal_core.rs    # MODIFIED: +handle_execute, +CSI cursor, +CSI screen
└── cell.rs             # UNCHANGED

src/terminal/wasm/
├── loader.ts           # UNCHANGED
├── unicode.ts          # UNCHANGED
└── terminal-core.ts    # UNCHANGED

src/terminal/
├── state.ts            # MODIFIED: WASM paths for Execute/CSI
├── handlers/
│   ├── c0_handlers.ts  # UNCHANGED (JS fallback)
│   ├── csi_cursor.ts   # UNCHANGED (JS fallback)
│   ├── csi_screen.ts   # UNCHANGED (JS fallback)
│   ├── index.ts        # UNCHANGED
│   └── (others)        # UNCHANGED
└── (others)            # UNCHANGED
```

### Dependencies

**No new crate dependencies.**

**No new npm dependencies.**

## Test Scenarios

### Unit Tests (Rust, `cargo test`)

#### C0 Controls
- [ ] handle_execute: BEL (0x07) - returns 0xFE sentinel
- [ ] handle_execute: BS (0x08) at col=5 → col=4, wrapPending=false
- [ ] handle_execute: BS (0x08) at col=0 → col=0 (clamped)
- [ ] handle_execute: HT (0x09) at col=0 → col=8 (default tab stops)
- [ ] handle_execute: HT (0x09) at col=7 → col=8
- [ ] handle_execute: HT (0x09) at col=8 → col=16
- [ ] handle_execute: HT (0x09) past last stop → col=cols-1
- [ ] handle_execute: HT (0x09) with custom tab stops
- [ ] handle_execute: LF (0x0A) at row=0 → row=1, returns 0
- [ ] handle_execute: LF (0x0A) at scroll_region_bottom → returns 1
- [ ] handle_execute: LF (0x0A) at bottom of screen (no scroll region) → returns 1
- [ ] handle_execute: VT (0x0B) → same as LF
- [ ] handle_execute: FF (0x0C) → same as LF
- [ ] handle_execute: CR (0x0D) → col=0, wrapPending=false
- [ ] handle_execute: SO (0x0E) → active_charset=1
- [ ] handle_execute: SI (0x0F) → active_charset=0
- [ ] handle_execute: LF clears wrapPending
- [ ] handle_execute: Unknown byte (e.g., 0x01) → no-op, returns 0

#### CSI Cursor
- [ ] handle_cursor_up: count=3 at row=5 → row=2
- [ ] handle_cursor_up: count=10 at row=5 → row=0 (clamped)
- [ ] handle_cursor_up: clears wrapPending
- [ ] handle_cursor_down: count=3 at row=5 in 24-row term → row=8
- [ ] handle_cursor_down: count=100 at row=5 → row=23 (clamped)
- [ ] handle_cursor_down: clears wrapPending
- [ ] handle_cursor_forward: count=5 at col=10 → col=15
- [ ] handle_cursor_forward: count=100 at col=10 in 80-col → col=79 (clamped)
- [ ] handle_cursor_forward: clears wrapPending
- [ ] handle_cursor_back: count=5 at col=10 → col=5
- [ ] handle_cursor_back: count=100 at col=10 → col=0 (clamped)
- [ ] handle_cursor_back: clears wrapPending
- [ ] handle_cursor_next_line: count=2 at row=3, col=15 → row=5, col=0
- [ ] handle_cursor_next_line: clamped at bottom row
- [ ] handle_cursor_previous_line: count=2 at row=5, col=15 → row=3, col=0
- [ ] handle_cursor_previous_line: clamped at row=0
- [ ] handle_cursor_horizontal_absolute: col=5 (1-indexed) → cursor.col=4
- [ ] handle_cursor_horizontal_absolute: col=0 → cursor.col=0 (clamped)
- [ ] handle_cursor_horizontal_absolute: col=1000 → cursor.col=cols-1 (clamped)
- [ ] handle_cursor_position: row=3, col=5 (1-indexed) → cursor.row=2, cursor.col=4
- [ ] handle_cursor_position: row=0, col=0 → (0, 0) (clamped)
- [ ] handle_cursor_position: row=1000, col=1000 → (rows-1, cols-1) (clamped)
- [ ] handle_cursor_vertical_absolute: row=5 (1-indexed) → cursor.row=4
- [ ] handle_cursor_vertical_absolute: row=0 → cursor.row=0 (clamped)

#### CSI Screen
- [ ] handle_erase_in_display: mode=0 (Below) at (5,10) → cells (5,10)..end cleared, rows 11..23 cleared
- [ ] handle_erase_in_display: mode=1 (Above) at (5,10) → rows 0-4 fully cleared, row 5 cells 0..10 cleared (inclusive of cursor col)
- [ ] handle_erase_in_display: mode=2 (All) → all cells cleared
- [ ] handle_erase_in_display: mode=3 (Scrollback) → returns 0xFF sentinel
- [ ] handle_erase_in_display: dirty rows marked correctly
- [ ] handle_erase_in_line: mode=0 (ToEnd) at col=5 → cells 5..cols-1 cleared
- [ ] handle_erase_in_line: mode=1 (ToStart) at col=5 → cells 0..5 cleared (inclusive)
- [ ] handle_erase_in_line: mode=2 (All) → entire line cleared
- [ ] handle_erase_in_line: dirty row marked
- [ ] handle_erase_characters: count=3 at col=5 → cells 5, 6, 7 cleared
- [ ] handle_erase_characters: count overflows past cols → clamped to end of line
- [ ] handle_erase_characters: dirty row marked

### Integration Tests (TypeScript, `bun test`)

#### C0 WASM Path
- [ ] Execute LF via WASM: cursor moves down, scrollUp triggered at bottom
- [ ] Execute CR via WASM: cursor moves to col 0
- [ ] Execute BS via WASM: cursor moves left
- [ ] Execute HT via WASM: cursor moves to next tab stop
- [ ] Execute BEL via WASM: returns 0xFE, onBell callback is invoked by TS
- [ ] Execute SO/SI via WASM: charset switching works

#### CSI Cursor WASM Path
- [ ] CursorUp via WASM: cursor moves up correctly
- [ ] CursorDown via WASM: cursor moves down correctly
- [ ] CursorPosition via WASM: cursor moves to exact position
- [ ] CHA via WASM: cursor column set correctly
- [ ] VPA via WASM: cursor row set correctly

#### CSI Screen WASM Path
- [ ] EraseInDisplay Below via WASM: correct cells cleared
- [ ] EraseInDisplay All via WASM: all cells cleared
- [ ] EraseInDisplay Scrollback: returns 0xFF, WASM dispatch calls buffer.clearScrollback() directly
- [ ] EraseInLine ToEnd via WASM: correct cells cleared
- [ ] EraseCharacters via WASM: correct cells cleared

#### Regression
- [ ] All existing c0_handlers tests pass
- [ ] All existing csi_cursor tests pass
- [ ] All existing csi_screen tests pass
- [ ] All existing print_handler tests pass (no Sprint 2 regression)

### Edge Cases

- [ ] 1-column terminal: cursor movement clamping
- [ ] 1-row terminal: LF at row 0 with scroll region (0,0) → scroll
- [ ] BS at col=0: no underflow
- [ ] HT with no tab stops set: move to cols-1
- [ ] HT at cols-1: stay at cols-1
- [ ] CursorUp with count=0: no movement (treat as count=1 per spec default)
- [ ] CursorPosition with both row/col as 0: clamp to (0,0)
- [ ] EraseInDisplay at (0,0) Below: clear entire screen
- [ ] EraseInDisplay at (cols-1, rows-1) Above: clear entire screen
- [ ] EraseCharacters with count=0: no-op (treat as count=1)
- [ ] Rapid sequence: LF+CR+Print (common \n\r pattern)
- [ ] wrapPending true → any C0/CSI → wrapPending cleared

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| BEL received | byte == 0x07 | Return 0xFE sentinel, TS invokes onBell |
| Unknown C0 byte | byte < 0x20 but not BEL/BS/HT/LF/VT/FF/CR/SO/SI | Ignore, return 0 |
| Invalid erase mode | mode > 3 | Ignore, return 0 |
| ED Scrollback | mode == 3 | Return 0xFF sentinel for TS fallback |
| WASM not initialized | getActiveWasmGrid() is null | Fall back to TS handlers |
| Count = 0 | CSI param is 0 or undefined | TS normalizes to 1 using `\|\| 1` (handles both undefined and explicit 0) |

## Performance Optimization

### Performance Goals

- C0 handle_execute: 1 WASM call per Execute action (vs. current TS: 2-4 WASM boundary crossings for cursor access)
- CSI cursor: 1 WASM call per CSI action (vs. current TS: cursor read + compute + write + wrapPending = 4 crossings)
- ED/EL: 1 WASM call per erase action (vs. current TS: N getLine+clear calls through WASM proxy)

### Memory Budget (Sprint 3 additions)

| Component | Size |
|-----------|------|
| handle_execute code | ~300 B |
| CSI cursor functions (9) | ~600 B |
| CSI screen functions (3) | ~500 B |
| find_next_tab_stop | ~100 B |
| **Total additional code** | **~1.5 KB** |

No new data structures needed - all state (cursor, tab_stops, wrap_pending, scroll_region, active_charset) already exists in TerminalCore from Sprint 1-2.

## Success Criteria

- [ ] `wasm-pack build` succeeds with all Sprint 3 additions
- [ ] WASM binary size < 50KB total
- [ ] All Rust unit tests pass (Sprint 1-2 tests + Sprint 3 new tests)
- [ ] All existing TypeScript tests pass (1824+)
- [ ] BEL returns 0xFE sentinel, TS invokes onBell correctly
- [ ] C0/CSI cursor/CSI screen operations produce identical results to TS handlers
- [ ] ED Scrollback returns 0xFF, WASM dispatch calls buffer.clearScrollback() directly
- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests unchanged

## Implementation Phases

### Phase 1: Rust C0 Handler + BEL Sentinel
**Goals:** Implement handle_execute in Rust with BEL sentinel return value
**Deliverables:**
- handle_execute(byte) dispatching all C0 codes (BEL returns 0xFE sentinel)
- find_next_tab_stop() for HT processing
- Rust unit tests for all C0 cases
- `wasm-pack build` succeeds

### Phase 2: Rust CSI Cursor Handlers
**Goals:** Implement 9 CSI cursor movement functions
**Deliverables:**
- handle_cursor_up/down/forward/back
- handle_cursor_next_line/previous_line
- handle_cursor_horizontal_absolute/position/vertical_absolute
- 1-indexed → 0-indexed conversion with clamping
- Rust unit tests for all cursor operations

### Phase 3: Rust CSI Screen Handlers
**Goals:** Implement ED, EL, ECH in Rust
**Deliverables:**
- handle_erase_in_display with Below/Above/All/Scrollback modes
- handle_erase_in_line with ToEnd/ToStart/All modes
- handle_erase_characters
- 0xFF sentinel return for Scrollback fallback
- Rust unit tests for all erase operations

### Phase 4: TypeScript Integration
**Goals:** Wire WASM handlers into TS processAction dispatch
**Deliverables:**
- processAction Execute WASM path with scroll bridge and BEL sentinel dispatch
- handleCsiWasm() dispatcher for cursor and screen CSI
- eraseModeToByte() helper
- JS fallback path unchanged

### Phase 5: Verification and Regression Testing
**Goals:** Full regression test and cross-validation
**Deliverables:**
- All existing tests passing
- Cross-validation: WASM vs TS produce identical results for all C0/CSI cases
- Binary size verification
- `bun tauri dev` smoke test

## References

- WASM roadmap: `tmp/wasm.md`
- Sprint 2 SPEC: `doc/tasks/wasm-print-handler/SPEC.md`
- Current implementations:
  - `src/terminal/handlers/c0_handlers.ts` - C0 handlers (TS)
  - `src/terminal/handlers/csi_cursor.ts` - CSI cursor handlers (TS)
  - `src/terminal/handlers/csi_screen.ts` - CSI screen handlers (TS)
  - `src/terminal/handlers/index.ts` - Handler dispatch
  - `src/terminal/state.ts` - processAction dispatch
  - `wasm/src/terminal_core.rs` - TerminalCore (Rust)
