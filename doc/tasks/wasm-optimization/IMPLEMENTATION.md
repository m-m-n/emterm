# Implementation Plan: WASM Implementation Optimization

## Overview

Optimize the WASM terminal core across 9 functional requirements: eliminate intermediate allocations in PTY processing (FR1), preserve overflow during reflow (FR2), convert CSI params to fixed-length arrays (FR3), utilize Cell padding for underline styling (FR4), expand OverflowTable keys (FR5), add overflow reverse index (FR6), optimize Cargo.toml (FR7), implement differential scroll rendering (FR8), and pre-allocate APC/DCS buffers (FR9).

## Objectives

- Eliminate heap allocations in the hot path (process_pty_data, CSI dispatch)
- Preserve overflow grapheme clusters across terminal resize
- Reduce WASM binary size and improve scroll rendering performance
- Lay groundwork for SGR 4:x / SGR 58 underline styling

## Prerequisites

### Development Environment
- Rust toolchain with wasm32-unknown-unknown target
- wasm-pack
- Bun (package manager and bundler)

### Dependencies
- No new external crate dependencies
- Parser already implements `Default` (delegates to `Parser::new()`)

## Architecture Overview

### Technology Stack
- **Language**: Rust (WASM) + TypeScript (frontend)
- **Framework**: Tauri + Canvas 2D renderer
- **Key Libraries**: wasm-bindgen, wasm-pack

### Design Approach

Bottom-up optimization: start with isolated low-risk changes (build config, buffer pre-allocation, type definitions), then propagate type changes through the codebase, optimize the hot path, and finally implement the cross-boundary rendering change.

### Component Interaction

```
Parser ──emits──> ParsedAction ──dispatched──> TerminalCore
                  (FR3: fixed-len)              (FR1: direct dispatch)
                  (FR9: pre-alloc)                    │
                                                      ▼
                                           Cell (FR4: underline fields)
                                           RingBuffer (FR2: reflow, FR8: dirty)
                                           OverflowTable (FR5: u32 keys)
                                           OverflowRowIndex (FR6: reverse index)
                                                      │
                                                      ▼
                                           CanvasRenderer (FR8: differential scroll)
```

### Dependency Graph

```
FR7 ─────────────────────┐
FR9 ─────────────────────┤
FR3 ──────> FR1           │  (FR3 before FR1: fixed-len ParsedAction enables zero-alloc dispatch)
FR5 ──────> FR6           │  (FR5 before FR6: reverse index uses same u32 key type)
FR5 ──────> FR2           │  (FR5 before FR2: reflow overflow needs u32 keys)
FR4 ─────────────────────┤
FR8 (WASM) ──> FR8 (TS)  │  (WASM scroll event API before frontend consumer)
```

## Implementation Phases

### Phase 1: Build and Buffer Foundation

**Goal**: Apply isolated, low-risk optimizations that require no API changes — build settings and buffer pre-allocation.

**Files to Modify**:
- `wasm/Cargo.toml` — Add release profile settings (FR7)
- `wasm/src/parser.rs` — Pre-allocate APC/DCS buffers (FR9)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cargo.toml release profile | Maximize LTO, strip symbols | Existing opt-level="z" + lto=true | codegen-units=1, strip="symbols" added |
| Parser constructor | Pre-allocate image buffers | apc_buffer/dcs_buffer = Vec::new() | Both initialized with capacity 4096 |

**Implementation Steps**:
1. **Add Cargo.toml settings** — Append codegen-units and strip to profile.release
2. **Pre-allocate parser buffers** — Change Vec::new() to Vec::with_capacity(4096) for apc_buffer and dcs_buffer in Parser::new()
3. **Verify build** — Confirm WASM binary compiles and existing tests pass
4. **Measure binary size** — Record before/after WASM binary size for FR7 verification

**Dependencies**: None — fully independent

**Testing Approach**:
- Unit: Existing parser tests continue passing
- Manual: Compare WASM binary size before/after

**Acceptance Criteria**:
- [ ] All existing tests pass
- [ ] WASM binary size recorded (before/after)
- [ ] Parser pre-allocates 4096 bytes for APC and DCS buffers

**Estimated Effort**: small

---

### Phase 2: Type System Changes

**Goal**: Change foundational types (ParsedAction, Cell, OverflowTable) that propagate through the codebase. No behavioral changes yet — just type definitions and all consuming code updated to match.

**Files to Modify**:
- `wasm/src/parser_types.rs` — Fixed-length arrays for CsiDispatch (FR3)
- `wasm/src/parser.rs` — Emit fixed-length ParsedAction in CSI completion (FR3)
- `wasm/src/cell.rs` — OverflowTable key type (u16,u16)→(u32,u32) (FR5), Cell underline fields (FR4), OverflowRowIndex type definition (FR6)
- `wasm/src/terminal_core.rs` — Update all overflow key casts and Cell field references
- `wasm/src/ring_buffer.rs` — Update overflow key types in reflow/shift functions
- `wasm/src/csi_dispatch.rs` — Update CSI param access to use sliced fixed-length arrays
- `wasm/src/print_handler.rs` — Update Cell initialization for underline fields

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ParsedAction::CsiDispatch | Hold CSI params without heap allocation | params: Vec, intermediates: Vec | params: [u16;8]+count, intermediates: [u8;2]+count |
| Cell | Store underline style and color | _padding: [u8;4] | underline_style: u8 + underline_color: [u8;3] |
| OverflowTable | Support scrollback > 65535 | Key type (u16,u16) | Key type (u32,u32) |
| OverflowRowIndex | Enable O(1) row lookup | Does not exist | Type alias defined (populated in Phase 4) |

**Processing Flow (FR3 - Parser CSI emission)**:
1. Parser accumulates CSI params into fixed-length buffer during parse
2. On CSI final byte, emit CsiDispatch with array + count
   - If accumulated params exceed 8 → silently cap at 8
   - If intermediates exceed 2 → silently cap at 2
3. Consumer slices array to count length for dispatch

**Processing Flow (FR5 - OverflowTable key migration)**:
1. Change type alias in cell.rs
2. Update all `as u16` casts in overflow operations to `as u32`
3. Update function signatures: overflow_shift_up, overflow_shift_down, overflow_clear_row, overflow_clear_range
4. Update shift_rows_up/down overflow movement code

**Implementation Steps**:
1. **Define ParsedAction fixed-length variant** — Replace Vec fields with fixed arrays + counts in parser_types.rs
2. **Update parser CSI emission** — Modify CSI completion in parser.rs to populate fixed-length arrays with truncation
3. **Update Cell struct** — Replace _padding with underline_style + underline_color, update EMPTY constant and all Cell initialization sites
4. **Expand OverflowTable key type** — Change type alias and propagate u32 keys through all overflow functions
5. **Define OverflowRowIndex type** — Add type alias (actual population deferred to Phase 4)
6. **Update all consumers** — csi_dispatch.rs param access, print_handler.rs Cell initialization, terminal_core.rs overflow operations

**Dependencies**: None — foundation for Phases 3-5

**Testing Approach**:
- Unit: All existing tests pass with new types (mechanical migration, no behavioral change)
- Unit: CSI with 0, 1, 8, >8 params — correct truncation
- Unit: Cell underline_style/color round-trip (set and read back)
- Unit: OverflowTable with row index > 65535

**Acceptance Criteria**:
- [ ] ParsedAction::CsiDispatch uses fixed-length arrays
- [ ] Cell struct is 32 bytes with underline_style and underline_color
- [ ] OverflowTable uses (u32,u32) keys
- [ ] All existing tests pass
- [ ] No heap allocation in ParsedAction::CsiDispatch construction

**Estimated Effort**: medium

---

### Phase 3: Direct Dispatch Optimization

**Goal**: Eliminate the intermediate Vec in process_pty_data by using the take-dispatch-restore pattern. This is the highest-impact performance optimization.

**Files to Modify**:
- `wasm/src/terminal_core.rs` — Refactor process_pty_data to take pattern + add dispatch_action method

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| process_pty_data | Process PTY data without intermediate buffer | Collects to Vec then iterates | Takes parser, dispatches in callback, restores |
| dispatch_action | Route ParsedAction to handler | Does not exist | Extracted match block from process_pty_data |

**Processing Flow**:
1. Temporarily extract parser from self (parser implements Default, take yields valid empty parser)
2. Call parse on extracted parser with closure that calls self.dispatch_action
3. Restore parser to self after parse completes
4. If dispatch panics, parser must still be restored — use scope guard or manual restore in all paths

**Implementation Steps**:
1. **Extract dispatch_action method** — Move the match block from process_pty_data into a separate method
2. **Implement take-dispatch-restore** — Replace Vec pattern with temporary parser extraction
3. **Add panic safety** — Ensure parser restoration even if dispatch panics (drop guard pattern)
4. **Verify behavioral equivalence** — Same test results as before

**Dependencies**: Phase 2 (FR3 — dispatch_action receives fixed-length ParsedAction)

**Testing Approach**:
- Unit: Mixed PTY data (print + CSI + OSC + ESC + APC) processed correctly
- Unit: Parser state preserved across multiple process_pty_data calls (split sequences)
- Unit: Empty data (0 bytes) — no dispatch, parser state unchanged
- Edge: Parser state after dispatch that modifies terminal state (cursor move, etc.)

**Acceptance Criteria**:
- [ ] process_pty_data dispatches directly without intermediate Vec
- [ ] Parser state is correctly preserved across calls
- [ ] Split ANSI sequences across multiple calls still work
- [ ] All existing tests pass

**Estimated Effort**: medium

---

### Phase 4: Overflow Management Improvements

**Goal**: Add reverse index for efficient row shifting, and preserve overflow data through reflow. These are the most complex changes affecting data integrity.

**Files to Modify**:
- `wasm/src/cell.rs` — OverflowRowIndex population helpers (FR6)
- `wasm/src/terminal_core.rs` — Maintain reverse index on overflow add/remove (FR6), update shift_rows to use reverse index
- `wasm/src/ring_buffer.rs` — Modify reflow_drain to capture overflow data, modify reflow_split_at_width to re-register overflow, remove overflow.clear() from resize_post_cleanup (FR2)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OverflowRowIndex | O(1) row→columns lookup | Does not exist (type defined in Phase 2) | Populated and maintained alongside OverflowTable |
| shift_rows_up/down | Efficient overflow movement | O(n) HashMap scan per row | O(1) row lookup via reverse index |
| reflow_drain | Capture overflow during drain | Copies cells, ignores overflow | Captures overflow strings into PhysicalLine |
| reflow_split_at_width | Re-register overflow after split | No overflow handling | Re-registers overflow cells in new positions |
| resize_post_cleanup | Preserve rebuilt overflow | Clears all overflow | Skips clear (overflow already rebuilt by reflow) |

**Processing Flow (FR6 — Reverse Index)**:
1. On overflow insert (set_cell with char_len=0xFF): add (row, col) to reverse index
2. On overflow remove (clear operations): remove (row, col) from reverse index
3. On shift_rows_up/down: lookup columns for source row via reverse index, move to destination row in both tables
4. On overflow.clear() (reflow): also clear reverse index

**Processing Flow (FR2 — Reflow Overflow Preservation)**:
1. reflow_drain phase: when reading a cell with char_len=0xFF, look up the overflow string and attach it to the PhysicalLine (alongside the cell)
2. Logical line joining: preserve overflow associations through wrapped line merging
3. reflow_split_at_width phase: when writing a cell with overflow data, insert into the new overflow table at the new (col, abs_row) position
4. resize_post_cleanup: remove the overflow.clear() call (overflow has been rebuilt)
5. Also rebuild the reverse index after reflow

**Implementation Steps**:
1. **Add reverse index field** — Store OverflowRowIndex in TerminalCore alongside overflow
2. **Maintain reverse index** — Update all overflow insert/remove call sites to also update reverse index
3. **Refactor shift_rows_up/down** — Use reverse index for O(1) row lookup instead of HashMap scan
4. **Extend PhysicalLine** — Add optional overflow data storage to carry overflow strings through reflow
5. **Update reflow_drain** — Capture overflow strings from OverflowTable when reading overflow cells
6. **Update reflow_split_at_width** — Re-register overflow entries at new positions in new overflow table
7. **Update resize_post_cleanup** — Remove overflow.clear(), rebuild reverse index from new overflow table

**Dependencies**: Phase 2 (FR5 — u32 keys)

**Testing Approach**:
- Unit: Reverse index consistency after insert, remove, shift_up, shift_down, clear
- Unit: Overflow cell (ZWJ family emoji) survives width-change reflow
- Unit: Overflow cell survives same-width row-count-change resize
- Unit: Multiple overflow cells in same row survive reflow
- Edge: All cells are overflow — all survive
- Edge: Reflow that wraps an overflow cell to the next line
- Integration: shift_rows_up/down with overflow using reverse index

**Acceptance Criteria**:
- [ ] Reverse index maintained correctly alongside overflow table
- [ ] shift_rows uses O(1) reverse index lookup
- [ ] ZWJ family emoji (25 bytes, char_len=0xFF) survives resize_reflow
- [ ] resize_post_cleanup does not clear overflow (rebuilt by reflow)
- [ ] All existing tests pass

**Estimated Effort**: large

---

### Phase 5: Differential Scroll Rendering

**Goal**: Implement differential Canvas rendering for full-screen scroll, avoiding full redraw when only one new row appears. Requires coordinated WASM + frontend changes.

**Files to Modify**:
- `wasm/src/ring_buffer.rs` — Add scroll event field, modify scroll_up_internal dirty marking (FR8 WASM)
- `wasm/src/terminal_core.rs` — Expose scroll event getter/clear via WASM binding (FR8 WASM)
- `src/terminal/canvas-renderer.ts` — Consume scroll event for differential drawing (FR8 frontend)
- `src/terminal/wasm/terminal-core.ts` — Bridge scroll event methods from WASM to renderer (FR8 bridge)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ScrollEvent | Communicate scroll info from WASM to JS | Does not exist | Struct with direction and count, stored in RingBuffer |
| scroll_up_internal | Set scroll event + mark new row dirty | Calls mark_all_dirty() | Conditionally uses scroll event (full-screen count=1 only) |
| CanvasRenderer.render | Differential Canvas shift + dirty draw | Full redraw for all dirty rows | Detects scroll event, shifts canvas content, draws only new row |
| WASM bridge | Expose scroll event API | No scroll event methods | get_scroll_event_direction/count/clear methods |

**Processing Flow (WASM side)**:
1. scroll_up_internal detects full-screen scroll with count=1
2. After ring_push_blank, stores ScrollEvent (direction=Up, count=1)
3. Marks only the last viewport row as dirty (the new blank row)
4. For count>1 or scroll region: falls back to mark_all_dirty (no scroll event)

**Processing Flow (Frontend side)**:
1. render() checks for scroll event before processing dirty rows
2. If scroll event present:
   - Clear the scroll event via WASM API
   - Use Canvas drawImage self-copy to shift existing content up by (count * cellHeight) pixels
   - Clear the vacated area at the bottom
3. Process dirty rows as usual (only the new row in scroll case)
4. Fallback: if no scroll event, render dirty rows normally (no behavior change)

**Implementation Steps**:
1. **Define ScrollEvent struct** — Direction enum + count field in ring_buffer.rs
2. **Add scroll event field** — Option<ScrollEvent> in RingBuffer, with getter/setter/clear methods
3. **Modify scroll_up_internal** — Conditionally set scroll event instead of mark_all_dirty for full-screen count=1
4. **Expose WASM API** — Add wasm_bindgen methods for scroll event query and clear
5. **Update TS bridge** — Add scroll event methods to WasmGrid/terminal-core.ts
6. **Implement differential rendering** — In CanvasRenderer.render(), detect scroll event and use drawImage self-copy before dirty row rendering

**Dependencies**: None (logically independent, but should be last since it's cross-boundary)

**Testing Approach**:
- Unit: scroll_up_internal(1, full-screen) marks only last row dirty
- Unit: scroll_up_internal(1, scroll-region) still marks all dirty (fallback)
- Unit: scroll_up_internal(count>1, full-screen) still marks all dirty (fallback)
- Unit: ScrollEvent generated and cleared correctly
- Manual: Visual verification of smooth scrolling without artifacts
- Manual: Canvas drawImage compatibility on target WebView (WebKitGTK, WebView2)

**Acceptance Criteria**:
- [ ] Full-screen scroll(1) does not call mark_all_dirty
- [ ] ScrollEvent is generated and consumed correctly across WASM-JS boundary
- [ ] Canvas content shifts visually without artifacts
- [ ] Only one row is drawn after scroll event
- [ ] Fallback to full redraw for non-eligible scrolls

**Estimated Effort**: large

---

## Complete File Structure

```
wasm/
├── Cargo.toml              # FR7: codegen-units=1, strip="symbols"
├── src/
│   ├── parser_types.rs     # FR3: CsiDispatch fixed-length arrays
│   ├── parser.rs           # FR3: emit fixed-len, FR9: buffer pre-alloc
│   ├── cell.rs             # FR4: underline fields, FR5: key type, FR6: index type
│   ├── terminal_core.rs    # FR1: take pattern, FR5/FR6: overflow ops, FR8: scroll event API
│   ├── ring_buffer.rs      # FR2: reflow overflow, FR6: reverse index, FR8: scroll event + dirty
│   ├── csi_dispatch.rs     # FR3: param slicing, FR4: SGR 4:x/58 dispatch
│   └── print_handler.rs    # FR4: Cell underline initialization
src/terminal/
├── canvas-renderer.ts      # FR8: differential scroll rendering
└── wasm/
    └── terminal-core.ts    # FR8: scroll event bridge methods
```

## Testing Strategy

- **Unit**: Core logic per FR — Rust tests in each module, target 90%+ on changed code
- **Integration**: Cross-FR interactions (FR1+FR3, FR2+FR5, FR6+FR5)
- **Manual**: Visual scroll verification (FR8), binary size measurement (FR7)

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | -       | No new dependencies required |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| FR1 parser panic leaves self.parser empty | Low | High | Drop guard pattern for parser restoration |
| FR2 reflow overflow logic introduces data corruption | Medium | High | Extensive test coverage, compare with overflow-free reflow |
| FR4 packed format change breaks JS parser | Medium | Medium | Update groupPackedCellsIntoSpans in lockstep |
| FR8 Canvas drawImage artifacts on specific WebViews | Medium | Medium | Runtime feature detection, fallback to mark_all_dirty |
| FR3 max 8 params insufficient for future sequences | Low | Low | 8 covers all known CSI sequences; can increase later |

## Open Questions

- [ ] FR4: underline_color `[0,0,0]` as "use foreground" — true black underlines on black text edge case (acceptable for now)
- [ ] FR8: Canvas drawImage self-copy compatibility on WebKitGTK and WebView2 — verify during implementation
- [ ] FR4: Packed format byte layout for underline data — exact position TBD (after flags, before next cell)

## Success Metrics

- [ ] All 9 FRs implemented and tested
- [ ] All existing tests pass (NFR5)
- [ ] Cell struct exactly 32 bytes (NFR2)
- [ ] WASM binary size reduced (NFR3)
- [ ] No new crate dependencies
- [ ] ZWJ family emoji survives resize (FR2)
- [ ] Full-screen scroll renders only 1 new row (FR8)
