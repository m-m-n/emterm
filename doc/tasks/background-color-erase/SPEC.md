# Feature: Background Color Erase (BCE)

## Overview

Implement BCE (Background Color Erase) support in the WASM terminal core. When erase operations (EL, ED, ECH, ICH, DCH, scroll, line insert/delete) create blank cells, those cells must inherit the cursor's current SGR background color instead of always using the default background. BCE is always enabled (no DECBKM mode toggle).

## Objectives

- Erase operations produce cells with the cursor's current background color
- Scroll and line insert/delete operations produce blank lines with the cursor's current background color
- Character insert/delete operations produce blank cells with the cursor's current background color
- No performance regression from BCE implementation

## User Stories

### US1: Diff display with colored background blocks
As a terminal user, I want erase operations to fill with the current background color, so that diff output (e.g., Claude Code) displays solid colored background blocks across the entire line.

**Acceptance Criteria:**
- [ ] `ESC[42m` text `ESC[K` fills the rest of the line with green background
- [ ] `ESC[0m` `ESC[K` fills the rest of the line with default background

### US2: Colored background during scroll
As a terminal user, I want scroll operations to create blank lines with the current background color, so that applications using colored regions render correctly during scrolling.

**Acceptance Criteria:**
- [ ] Scroll-up creates new bottom lines with cursor's background color
- [ ] Scroll-down creates new top lines with cursor's background color

## Technical Requirements

### Functional Requirements

- **FR1: Erase operations inherit cursor background color**
  EL (CSI K), ED (CSI J), and ECH (CSI X) must set the `bg` field of erased cells to `cursor.bg`.

- **FR2: Character insert/delete inherit cursor background color**
  ICH (CSI @) and DCH (CSI P) must set the `bg` field of newly created blank cells to `cursor.bg`.

- **FR3: Scroll and line operations inherit cursor background color**
  Scroll up/down (CSI S/T), line insert/delete (CSI L/M), and newline-triggered scrolling (ring_push_blank) must set the `bg` field of newly created blank lines to `cursor.bg`.

- **FR4: Reset and resize use default background**
  Terminal reset and resize operations continue to use `Cell::EMPTY` (default background). These are not erase operations and should not inherit cursor state.

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable latency increase. The change adds a field copy to existing cell initialization loops.
- **NFR2 - Compatibility:** Behavior matches xterm, kitty, and alacritty BCE semantics.

## Implementation Approach

### Architecture

The change is localized to the WASM terminal core. No changes needed in TypeScript rendering or Rust backend.

**Data flow (unchanged):**
```
PTY (Rust) → Binary Channel → PtyClient (TS) → process_pty_data (WASM) → callbacks (TS) → Canvas render (TS)
```

The Canvas renderer already draws whatever `bg` value is stored in cells. The fix is entirely in the WASM data layer.

### Core Change: BCE Cell Constructor

Add a helper method to `TerminalCore` that creates a blank cell inheriting the cursor's background:

```rust
/// Create a blank cell with the cursor's current background color (BCE).
pub(crate) fn bce_cell(&self) -> Cell {
    let mut cell = Cell::EMPTY;
    cell.bg = self.cursor.bg;
    cell
}
```

### Affected Methods

#### `wasm/src/terminal_core.rs`

| Method | Usage | Change |
|--------|-------|--------|
| `clear_line()` | ED, EL | Replace `Cell::EMPTY` with `self.bce_cell()` |
| `clear_line_range()` | ED, EL, ECH | Replace `Cell::EMPTY` with `self.bce_cell()` |
| `shift_rows_up()` | DL, scroll up | Replace `Cell::EMPTY` with `self.bce_cell()` in vacated rows |
| `shift_rows_down()` | IL, scroll down | Replace `Cell::EMPTY` with `self.bce_cell()` in vacated rows |

#### `wasm/src/csi_edit.rs`

| Method | Usage | Change |
|--------|-------|--------|
| `handle_insert_characters()` | ICH | Replace `Cell::EMPTY` with `self.bce_cell()` |
| `handle_delete_characters()` | DCH | Replace `Cell::EMPTY` with `self.bce_cell()` |

#### `wasm/src/ring_buffer.rs`

| Method | Usage | Change |
|--------|-------|--------|
| `ring_push_blank()` | Newline scroll | Accept `bg: PackedColor` parameter, use it instead of `Cell::EMPTY` |

#### Not Changed (use `Cell::EMPTY`)

| Method | Reason |
|--------|--------|
| `reset()` | Full terminal reset clears to default |
| `resize_rows()` / `resize_cols()` / `resize_both()` | Resize is not an erase operation |
| Initial grid allocation | No cursor state exists yet |
| Reflow/wrap operations | Structural operations, not erase |

### Dependencies

**Internal Dependencies:**
- `Cell` struct (`wasm/src/cell.rs`): No changes needed
- `CursorState` (`wasm/src/terminal_core.rs`): Read `cursor.bg`, no changes needed
- `PackedColor` (`wasm/src/cell.rs`): Used as parameter type, no changes needed

**External Dependencies:**
- None

### File Structure

```
wasm/src/
├── terminal_core.rs  # bce_cell(), clear_line(), clear_line_range(), shift_rows_up/down()
├── csi_edit.rs       # handle_insert_characters(), handle_delete_characters()
├── ring_buffer.rs    # ring_push_blank() signature change
└── cell.rs           # No changes (Cell::EMPTY remains unchanged)
```

## Test Scenarios

### Unit Tests

- [ ] EL 0 (erase to end): erased cells have cursor's background color
- [ ] EL 1 (erase to start): erased cells have cursor's background color
- [ ] EL 2 (erase line): all cells on line have cursor's background color
- [ ] ED 0 (erase below): erased cells have cursor's background color
- [ ] ED 2 (erase all): all cells have cursor's background color
- [ ] ECH: erased characters have cursor's background color
- [ ] ICH: inserted blank cells have cursor's background color
- [ ] DCH: trailing blank cells have cursor's background color
- [ ] Scroll up: new bottom line cells have cursor's background color
- [ ] Scroll down: new top line cells have cursor's background color
- [ ] IL (insert lines): new blank lines have cursor's background color
- [ ] DL (delete lines): new blank lines at bottom have cursor's background color
- [ ] Default background: when cursor.bg is DEFAULT, erased cells have DEFAULT background (same as current behavior)
- [ ] SGR reset then erase: after ESC[0m, erased cells have DEFAULT background

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with `./scripts/run-e2e-docker.sh`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Erase with 256-color background (CSI 48;5;Nm): BCE applies correctly
- [ ] Erase with RGB background (CSI 48;2;R;G;Bm): BCE applies correctly
- [ ] Erase after cursor save/restore: uses restored cursor's background color

## Success Criteria

- [ ] All functional requirements (FR1-FR4) are implemented
- [ ] All unit tests pass
- [ ] All existing tests pass without regression
- [ ] E2E tests pass without regression
- [ ] No performance regression in PTY data processing

## References

- xterm BCE specification: xterm ctlseqs documentation
- VT520 specification: EL, ED, ECH, ICH, DCH definitions
