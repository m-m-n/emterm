# Feature: WASM Implementation Optimization

## Overview

Optimize the WASM terminal core implementation across 9 areas: eliminate intermediate allocations in PTY data processing, preserve overflow data during reflow, convert heap-allocated CSI parameters to fixed-length arrays, utilize Cell padding bytes for underline styling, expand OverflowTable keys for large scrollback, add reverse index for row shifting, optimize Cargo.toml build settings, implement differential scroll rendering, and pre-allocate APC/DCS buffers.

## Objectives

- Eliminate unnecessary heap allocations in the hot path (process_pty_data, CSI dispatch)
- Preserve user-visible data (overflow grapheme clusters) across terminal resize/reflow
- Reduce WASM binary size through build configuration optimization
- Improve scroll rendering performance with Canvas-level differential drawing
- Lay groundwork for extended underline styling (SGR 4:x, SGR 58)

## User Stories

### US1: Fast Large File Display
As a terminal user, I want large file output (`cat large_file`) to be processed with minimal overhead, so that scrolling through output remains smooth.

**Acceptance Criteria:**
- [ ] `process_pty_data` dispatches actions directly without intermediate Vec
- [ ] CSI parameters use stack-allocated fixed-length arrays
- [ ] APC/DCS buffers are pre-allocated to avoid repeated growth

### US2: Emoji Preservation After Resize
As a user who uses ZWJ family emoji and other complex grapheme clusters, I want them to remain visible after resizing the terminal window, so that I don't lose displayed content.

**Acceptance Criteria:**
- [ ] Overflow entries (char_len=0xFF) survive reflow
- [ ] ZWJ family emoji (e.g., `👨‍👩‍👧‍👦`, 25 bytes) displays correctly after resize

### US3: Smooth Scrolling
As a terminal user, I want full-screen scrolling to be efficient, so that rapid output does not cause visible lag.

**Acceptance Criteria:**
- [ ] Full-screen scroll (count=1) does not mark all rows dirty
- [ ] Canvas uses scrollBy or drawImage self-copy to shift existing content
- [ ] Only the new row is drawn from scratch

## Technical Requirements

### Functional Requirements

- **FR1:** `process_pty_data` uses `std::mem::take` to temporarily extract the parser from `self`, enabling direct dispatch in the parse callback without intermediate Vec allocation
- **FR2:** Reflow preserves overflow entries by incorporating them into PhysicalLine during `reflow_drain` and re-registering them in the new overflow table after `reflow_split_at_width`
- **FR3:** `ParsedAction::CsiDispatch` uses fixed-length arrays: `params: [u16; 8]` with `param_count: u8`, and `intermediates: [u8; 2]` with `intermediate_count: u8`
- **FR4:** Cell's `_padding: [u8; 4]` is replaced with `underline_style: u8` and `underline_color: [u8; 3]` while maintaining 32-byte struct size
- **FR5:** `OverflowTable` key type changes from `(u16, u16)` to `(u32, u32)` to support scrollback capacities exceeding 65535
- **FR6:** A reverse index `HashMap<u32, Vec<u32>>` (row → column list) is maintained alongside the overflow table, updated on overflow add/remove
- **FR7:** `wasm/Cargo.toml` `[profile.release]` adds `codegen-units = 1` and `strip = "symbols"`
- **FR8:** Full-screen scroll (count=1) uses differential Canvas rendering: WASM marks only the new row dirty and provides scroll event info; frontend uses Canvas scrollBy/drawImage to shift existing content and draws only the new row
- **FR9:** Parser's `apc_buffer` and `dcs_buffer` are initialized with `Vec::with_capacity(4096)`

### Non-Functional Requirements

- **NFR1 - Performance:** Heap allocation count in `process_pty_data` reduced to near-zero for typical PTY data
- **NFR2 - Memory:** Cell struct remains exactly 32 bytes (`#[repr(C)]`)
- **NFR3 - Binary Size:** WASM binary size reduced through `codegen-units = 1` and `strip = "symbols"`
- **NFR4 - Compatibility:** Packed binary format for JS boundary remains unchanged
- **NFR5 - Data Integrity:** All existing tests pass without modification (unless testing the changed behavior)

## Implementation Approach

### Architecture

**Component Diagram:**
```
┌─────────────────────────────────────────────┐
│  Frontend (TypeScript)                       │
│  ┌─────────────────────────────────────┐    │
│  │  CanvasRenderer                     │    │
│  │  - Differential scroll (FR8)        │    │
│  │  - scrollBy / drawImage self-copy   │    │
│  └─────────────────────────────────────┘    │
├─────────────────────────────────────────────┤
│  WASM (Rust)                                 │
│  ┌───────────────┐  ┌──────────────────┐    │
│  │  Parser        │  │  TerminalCore    │    │
│  │  - Fixed-len   │  │  - take pattern  │    │
│  │    arrays (FR3)│  │    (FR1)         │    │
│  │  - Pre-alloc   │  │  - Scroll event  │    │
│  │    buffers(FR9)│  │    (FR8)         │    │
│  └───────────────┘  └──────────────────┘    │
│  ┌───────────────┐  ┌──────────────────┐    │
│  │  Cell          │  │  RingBuffer      │    │
│  │  - underline   │  │  - Reflow (FR2)  │    │
│  │    fields(FR4) │  │  - Dirty mgmt    │    │
│  └───────────────┘  │    (FR8)         │    │
│  ┌───────────────┐  │  - Reverse idx   │    │
│  │  OverflowTable │  │    (FR6)         │    │
│  │  - u32 keys   │  └──────────────────┘    │
│  │    (FR5)       │                          │
│  └───────────────┘                           │
└─────────────────────────────────────────────┘
```

### Data Flow

#### FR1: Direct Dispatch Pattern
```
PTY data → process_pty_data()
  → parser = std::mem::take(&mut self.parser)
  → parser.parse(data, |action| {
        self.dispatch(action)  // direct dispatch, no Vec
    })
  → self.parser = parser  // restore
```

#### FR2: Reflow Overflow Preservation
```
resize_reflow()
  → reflow_drain(): read cells, capture overflow data into PhysicalLine
  → reflow_split_at_width(): write cells to new ring, re-register overflow
  → resize_post_cleanup(): skip overflow.clear() (already rebuilt)
```

#### FR8: Differential Scroll Rendering
```
scroll_up_internal(1, full_screen)
  → ring_push_blank()
  → mark only last row dirty (not mark_all_dirty)
  → set scroll_event = { direction: Up, count: 1 }

Frontend render():
  → detect scroll_event
  → Canvas scrollBy(0, -cellHeight) or drawImage self-copy
  → draw only dirty row (the new blank line)
  → clear scroll_event
```

### Dependencies

**Internal Dependencies:**
- FR1 depends on Parser being `Default`-implementable (for `std::mem::take`)
- FR2 depends on FR5 (overflow keys must be consistent after reflow)
- FR6 depends on FR5 (reverse index uses same key type)
- FR8 depends on dirty management changes in RingBuffer

**External Dependencies:**
- No new crate dependencies required
- FR3/FR6 may optionally use `arrayvec` crate, but fixed arrays with length fields are preferred to avoid new dependencies

### File Structure

```
wasm/src/
├── terminal_core.rs    # FR1 (process_pty_data), FR8 (scroll event)
├── parser.rs           # FR3 (emit fixed-len), FR9 (buffer pre-alloc)
├── parser_types.rs     # FR3 (ParsedAction fixed-len arrays)
├── cell.rs             # FR4 (underline fields), FR5 (key type), FR6 (reverse index)
├── ring_buffer.rs      # FR2 (reflow), FR6 (reverse index ops), FR8 (dirty management)
├── print_handler.rs    # FR4 (underline style handling)
├── csi_dispatch.rs     # FR3 (param access), FR4 (SGR 4:x, SGR 58)
wasm/Cargo.toml         # FR7 (build settings)
src/terminal/
├── canvas-renderer.ts  # FR8 (differential scroll drawing)
```

## Detailed Design

### FR1: process_pty_data Direct Dispatch

**Current code** (`wasm/src/terminal_core.rs:789-829`):
```rust
pub fn process_pty_data(&mut self, data: &[u8]) {
    let mut actions = Vec::new();  // heap allocation every call
    self.parser.parse(data, |action| {
        actions.push(action);
    });
    for action in actions {
        match action { ... }
    }
}
```

**New design:**
```rust
pub fn process_pty_data(&mut self, data: &[u8]) {
    let mut parser = std::mem::take(&mut self.parser);
    parser.parse(data, |action| {
        self.dispatch_action(action);
    });
    self.parser = parser;
}

fn dispatch_action(&mut self, action: ParsedAction) {
    match action {
        ParsedAction::Print(ch) => self.handle_print(ch as u32),
        ParsedAction::Execute(byte) => self.handle_execute_internal(byte),
        ParsedAction::CsiDispatch { params, param_count, intermediates, intermediate_count, final_byte } => {
            self.handle_csi_internal(&params[..param_count as usize], &intermediates[..intermediate_count as usize], final_byte);
        }
        // ... other variants
    }
}
```

**Requirement:** Parser must implement `Default` for `std::mem::take`. The default parser should be in a valid initial state (ground state).

### FR3: ParsedAction Fixed-Length Arrays

**Current** (`wasm/src/parser_types.rs`):
```rust
CsiDispatch {
    params: Vec<u16>,
    intermediates: Vec<u8>,
    final_byte: u8,
}
```

**New design:**
```rust
CsiDispatch {
    params: [u16; 8],
    param_count: u8,
    intermediates: [u8; 2],
    intermediate_count: u8,
    final_byte: u8,
}
```

- Max 8 CSI params covers all standard sequences (SGR has at most ~6 params in practice)
- Max 2 intermediates covers all known sequences (typically 0-1)
- Excess params/intermediates are silently truncated

### FR4: Cell Underline Fields

**Current** (`wasm/src/cell.rs:77-85`):
```rust
pub struct Cell {
    pub char_data: [u8; 16],   // 16
    pub char_len: u8,           // 1
    pub width: u8,              // 1
    pub fg: PackedColor,        // 4
    pub bg: PackedColor,        // 4
    pub flags: u16,             // 2
    pub _padding: [u8; 4],      // 4
}  // Total: 32 bytes
```

**New design:**
```rust
pub struct Cell {
    pub char_data: [u8; 16],       // 16
    pub char_len: u8,               // 1
    pub width: u8,                  // 1
    pub fg: PackedColor,            // 4
    pub bg: PackedColor,            // 4
    pub flags: u16,                 // 2
    pub underline_style: u8,        // 1  (0=none, 1=single, 2=double, 3=curly, 4=dotted, 5=dashed)
    pub underline_color: [u8; 3],   // 3  ([r, g, b], all-zero = default/fg color)
}  // Total: 32 bytes
```

**underline_color encoding:**
- `[0, 0, 0]` = use foreground color (default behavior)
- Any other value = explicit RGB color

**SGR mapping:**
- SGR 4 / SGR 4:1 → underline_style = 1
- SGR 4:2 → underline_style = 2
- SGR 4:3 → underline_style = 3
- SGR 4:4 → underline_style = 4
- SGR 4:5 → underline_style = 5
- SGR 4:0 / SGR 24 → underline_style = 0
- SGR 58;2;r;g;b → underline_color = [r, g, b]
- SGR 58;5;n → underline_color = indexed_to_rgb(n)
- SGR 59 → underline_color = [0, 0, 0]

**Packed format update:** The packed binary row format sent to JS must include underline_style and underline_color for rendering. The exact byte layout change should be designed during implementation planning.

### FR5: OverflowTable Key Expansion

**Current** (`wasm/src/cell.rs:133`):
```rust
pub type OverflowTable = HashMap<(u16, u16), String>;
```

**New design:**
```rust
pub type OverflowTable = HashMap<(u32, u32), String>;
```

All functions accepting `(u16, u16)` keys for overflow operations must be updated:
- `overflow_shift_up`, `overflow_shift_down`
- `set_cell`, `get_cell_char` overflow paths
- Reflow overflow handling (FR2)
- Shift rows overflow movement in `terminal_core.rs`

### FR6: Overflow Reverse Index

**New type:**
```rust
pub type OverflowRowIndex = HashMap<u32, Vec<u32>>;  // row → [col, ...]
```

**Operations:**
- On overflow insert: add col to row's Vec
- On overflow remove: remove col from row's Vec (remove entry if Vec empty)
- On row shift: move entire Vec entry from old row to new row
- `shift_rows_up/down`: O(1) lookup per row instead of O(n) HashMap scan

**Location:** Stored alongside `OverflowTable` in `TerminalCore` (or `RingBuffer`).

### FR7: Cargo.toml Build Optimization

**Current** (`wasm/Cargo.toml`):
```toml
[profile.release]
opt-level = "z"
lto = true
```

**New:**
```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = "symbols"
```

- `codegen-units = 1`: Maximizes LTO effectiveness (single codegen unit)
- `strip = "symbols"`: Removes debug symbols from release binary

### FR8: Differential Scroll Rendering

#### WASM Side Changes

**RingBuffer:**
- Add `scroll_event: Option<ScrollEvent>` field
- `ScrollEvent { direction: ScrollDirection, count: u16 }`
- `scroll_up_internal` for full-screen count=1: set scroll_event instead of `mark_all_dirty()`
- Mark only the new blank row as dirty
- Expose `get_scroll_event()` and `clear_scroll_event()` via wasm-bindgen

**Fallback:** For count > 1 or scroll region (non-full-screen), continue using `mark_all_dirty()`.

#### Frontend Side Changes

**CanvasRenderer:**
```typescript
render(state) {
    const scrollEvent = wasmCore.getScrollEvent();
    if (scrollEvent) {
        wasmCore.clearScrollEvent();
        // Shift existing canvas content
        const shiftPx = scrollEvent.count * this.cellHeight;
        const ctx = this.ctx;
        // Copy canvas content shifted up
        ctx.drawImage(this.canvas, 0, shiftPx, this.canvas.width, this.canvas.height - shiftPx,
                      0, 0, this.canvas.width, this.canvas.height - shiftPx);
        // Clear the new row area
        ctx.clearRect(0, this.canvas.height - shiftPx, this.canvas.width, shiftPx);
    }
    // Then draw dirty rows as usual (only the new row in scroll case)
    for (const rowIndex of dirtyRows) {
        this.renderRow(rowIndex, state);
    }
}
```

### FR9: APC/DCS Buffer Pre-allocation

**Current** (`wasm/src/parser.rs:61-62`):
```rust
apc_buffer: Vec::new(),
dcs_buffer: Vec::new(),
```

**New:**
```rust
apc_buffer: Vec::with_capacity(4096),
dcs_buffer: Vec::with_capacity(4096),
```

4096 bytes is a reasonable initial size that covers most small-to-medium image payloads without over-allocating.

## Test Scenarios

### Unit Tests

- [ ] FR1: `process_pty_data` correctly processes mixed PTY data (print, CSI, OSC, ESC)
- [ ] FR1: Parser state is correctly restored after `std::mem::take` pattern
- [ ] FR2: Overflow cell survives reflow when terminal width changes
- [ ] FR2: ZWJ family emoji (25 bytes) displays correctly after resize
- [ ] FR3: CSI with 0 params dispatches correctly
- [ ] FR3: CSI with 8 params (max) dispatches correctly
- [ ] FR3: CSI with >8 params truncates silently
- [ ] FR4: Cell with underline_style and underline_color round-trips correctly
- [ ] FR5: Overflow with row index > 65535 stores and retrieves correctly
- [ ] FR6: Reverse index stays consistent after shift_rows_up/down
- [ ] FR8: scroll_up_internal(1) marks only last row dirty (full-screen case)
- [ ] FR8: scroll_up_internal(1) in scroll region still marks all dirty

### Integration Tests

- [ ] FR1+FR3: Full ANSI sequence processing with fixed-length arrays through direct dispatch
- [ ] FR2+FR5: Reflow with overflow using u32 keys
- [ ] FR8: Scroll event is generated and consumed correctly across WASM-JS boundary

### Edge Cases

- [ ] FR1: Empty PTY data (0 bytes) - no dispatch, parser unchanged
- [ ] FR1: Parser panic during dispatch - parser must be restored (consider `catch_unwind` or drop guard)
- [ ] FR2: Reflow when all cells are overflow - all should survive
- [ ] FR3: CSI with intermediates beyond 2 bytes - truncated
- [ ] FR5: scrollback_lines = 0 - no overflow possible, empty table
- [ ] FR8: Rapid successive scrolls - scroll events must not accumulate incorrectly

### Performance Tests

- [ ] FR1: Benchmark `process_pty_data` with 1MB of mixed ANSI data (before/after comparison)
- [ ] FR7: Measure WASM binary size before and after Cargo.toml changes

## Security Considerations

- **Input Validation:** CSI param truncation (FR3) prevents buffer overflow from malicious sequences
- **Memory Safety:** All changes use safe Rust; no `unsafe` blocks required
- **DoS Prevention:** APC/DCS buffer caps (MAX_APC_LEN, MAX_DCS_LEN) remain unchanged

## Error Handling

- FR1: If parser panics during dispatch, the parser must be restored to `self` to avoid leaving `self.parser` in a default/empty state. Consider wrapping the parse+dispatch in a scope that restores on drop.
- FR3: Param overflow is handled by silent truncation, matching behavior of other terminal emulators (xterm, kitty)
- FR9: Pre-allocation failure in WASM environment is handled by the allocator (OOM = abort in wasm32)

## Success Criteria

- [ ] All existing tests pass
- [ ] New tests for each FR pass
- [ ] WASM binary size is reduced (FR7)
- [ ] `process_pty_data` shows measurable allocation reduction (FR1, FR3)
- [ ] ZWJ family emoji survives terminal resize (FR2)
- [ ] Full-screen scroll renders only 1 new row (FR8)
- [ ] Cell struct remains exactly 32 bytes (FR4, NFR2)
- [ ] Code review completed

## Open Questions

> **Note**: Unresolved requirements are tracked with `status: tbd` in sdd.yaml.
> Resolve them before running `/sdd.2-create-plan`.

- [ ] FR4: underline_color default detection — Using `[0, 0, 0]` as "use foreground" means true black underlines on black text need special handling (edge case, acceptable for now)
- [ ] FR8: Canvas scrollBy/drawImage compatibility across WebView2 (Windows) and WebKitGTK (Linux) — needs verification during implementation

## References

- Research report: `tmp/wasm-update.md`
- WASM terminal core: `wasm/src/terminal_core.rs`
- Cell structures: `wasm/src/cell.rs`
- ANSI parser: `wasm/src/parser.rs`
- Ring buffer / reflow: `wasm/src/ring_buffer.rs`
- Canvas renderer: `src/terminal/canvas-renderer.ts`
