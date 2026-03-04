# Implementation Plan: Background Color Erase (BCE)

## Overview

Add BCE support to the WASM terminal core so that all erase, insert/delete, and scroll operations produce blank cells inheriting the cursor's current SGR background color.

## Objectives

- All erase operations (EL, ED, ECH) fill cells with cursor's current background color
- Character insert/delete (ICH, DCH) produce blank cells with cursor's background color
- Scroll and line insert/delete produce blank lines with cursor's background color
- Reset and resize operations remain unchanged (use default background)

## Prerequisites

### Development Environment
- Rust toolchain (for WASM compilation)
- Bun (for TypeScript tests)

### Dependencies
- No new external dependencies

## Architecture Overview

### Technology Stack
- **Language**: Rust (WASM module)
- **Framework**: wasm-bindgen
- **Key Libraries**: No additions

### Design Approach

Introduce a `bce_cell()` helper on `TerminalCore` that returns a `Cell::EMPTY` copy with `bg` set to `cursor.bg`. Replace `Cell::EMPTY` with this helper in all erase/insert/delete/scroll paths. For `ring_push_blank`, add a `PackedColor` parameter to pass the background color from the caller.

### Component Interaction

```
CSI handlers (csi_screen, csi_edit)
   ↓ call
TerminalCore methods (clear_line, clear_line_range, shift_rows_up/down)
   ↓ use
bce_cell() → Cell with cursor.bg

scroll_up_internal
   ↓ call
ring_push_blank(bg: PackedColor)
   ↓ use
Cell::EMPTY + bg override
```

## Implementation Phases

### Phase 1: BCE Cell Helper and Core Erase Methods

**Goal**: Add `bce_cell()` helper and update `clear_line`, `clear_line_range` to use it. This covers FR1 (EL, ED, ECH).

**Files to Modify**:
- `wasm/src/terminal_core.rs` - Add `bce_cell()`, update `clear_line()`, `clear_line_range()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `bce_cell()` | Create blank cell with cursor background | `self.cursor.bg` is valid | Returns Cell with space char and cursor's bg |
| `clear_line()` | Clear entire line with BCE | Row within bounds | All cells on row have cursor's bg |
| `clear_line_range()` | Clear column range with BCE | Row and range within bounds | Cells in range have cursor's bg |

**Implementation Steps**:
1. **Add bce_cell helper** - Method on TerminalCore returning Cell::EMPTY with bg overridden from cursor.bg
2. **Update clear_line** - Replace Cell::EMPTY assignment with bce_cell() result
3. **Update clear_line_range** - Replace Cell::EMPTY assignment with bce_cell() result
4. **Add unit tests** - Test EL 0/1/2, ED 0/2, ECH with non-default background

**Dependencies**: None

**Testing Approach**:
- Unit: Set cursor bg to a known color, call erase operations, verify cells have expected bg
- Regression: Verify default bg case still works (cursor.bg = DEFAULT)

**Acceptance Criteria**:
- [ ] EL/ED/ECH erased cells inherit cursor background color
- [ ] Default background case unchanged

**Estimated Effort**: small

---

### Phase 2: Character Insert/Delete BCE

**Goal**: Update ICH and DCH handlers to use BCE for newly created blank cells. This covers FR2.

**Files to Modify**:
- `wasm/src/csi_edit.rs` - Update `handle_insert_characters()`, `handle_delete_characters()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_insert_characters()` | Insert blank cells with BCE | Cursor within bounds | Inserted cells have cursor's bg |
| `handle_delete_characters()` | Fill trailing cells with BCE | Cursor within bounds | Trailing cells have cursor's bg |

**Implementation Steps**:
1. **Update ICH** - Replace Cell::EMPTY with bce_cell() for inserted cells
2. **Update DCH** - Replace Cell::EMPTY with bce_cell() for trailing cells
3. **Add unit tests** - Test ICH/DCH with non-default background

**Dependencies**: Requires Phase 1 (bce_cell helper)

**Testing Approach**:
- Unit: Set cursor bg, insert/delete chars, verify blank cells have expected bg

**Acceptance Criteria**:
- [ ] ICH inserted cells inherit cursor background color
- [ ] DCH trailing cells inherit cursor background color

**Estimated Effort**: small

---

### Phase 3: Scroll and Line Operations BCE

**Goal**: Update scroll and line insert/delete to produce blank lines with BCE. This covers FR3.

**Files to Modify**:
- `wasm/src/terminal_core.rs` - Update `shift_rows_up()`, `shift_rows_down()`
- `wasm/src/ring_buffer.rs` - Update `ring_push_blank()` signature and callers

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `shift_rows_up()` | Clear vacated bottom rows with BCE | Valid row range | Vacated rows have cursor's bg |
| `shift_rows_down()` | Clear vacated top rows with BCE | Valid row range | Vacated rows have cursor's bg |
| `ring_push_blank()` | Create new scrollback line with given bg | bg parameter provided | New line cells have specified bg |
| `scroll_up_internal()` | Pass cursor bg to ring_push_blank | Cursor state valid | New lines have cursor's bg |

**Processing Flow**:
1. scroll_up_internal receives scroll request
   - Full screen → call ring_push_blank with cursor.bg
   - Scroll region → call shift_rows_up (already uses bce_cell from Phase 1 change)
2. scroll_down_internal → call shift_rows_down (already uses bce_cell)
3. IL/DL → call shift_rows_down/up (already uses bce_cell)

**Implementation Steps**:
1. **Update shift_rows_up** - Replace Cell::EMPTY with bce_cell() in vacated row clearing
2. **Update shift_rows_down** - Replace Cell::EMPTY with bce_cell() in vacated row clearing
3. **Add bg parameter to ring_push_blank** - Accept PackedColor, use for new line cells
4. **Update scroll_up_internal** - Pass cursor.bg to ring_push_blank
5. **Update ring_push_blank tests** - Adjust for new signature
6. **Add BCE-specific scroll tests** - Verify new lines have correct background

**Dependencies**: Requires Phase 1 (bce_cell helper)

**Testing Approach**:
- Unit: Set cursor bg, trigger scroll/IL/DL, verify blank lines have expected bg
- Regression: Verify existing scroll tests still pass

**Acceptance Criteria**:
- [ ] Scroll-created blank lines inherit cursor background color
- [ ] IL/DL-created blank lines inherit cursor background color
- [ ] ring_push_blank produces lines with specified background color

**Estimated Effort**: medium

---

## Complete File Structure

```
wasm/src/
├── terminal_core.rs  # bce_cell(), clear_line(), clear_line_range(), shift_rows_up/down()
├── csi_edit.rs       # handle_insert_characters(), handle_delete_characters()
├── ring_buffer.rs    # ring_push_blank(bg) signature change, scroll_up_internal()
└── cell.rs           # No changes
```

## Testing Strategy

- Unit: All erase/insert/delete/scroll operations tested with non-default cursor background. Coverage target: all modified code paths.
- Integration: Process PTY sequences containing SGR + erase combinations through the WASM parser.
- E2E (Docker): Existing E2E tests pass without regression (`./scripts/run-e2e-docker.sh test`).
- Manual: Visual verification of diff display in eMterm (Claude Code diff output shows solid background blocks).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | - | No new dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Existing tests break due to bg field comparison | Medium | Low | Update test assertions to account for BCE |
| ring_push_blank signature change breaks callers | Low | Low | Only one caller (scroll_up_internal), straightforward update |

## Open Questions

- (none)

## Success Metrics

- [ ] All FR1-FR4 requirements implemented and tested
- [ ] All existing tests pass
- [ ] No performance regression
