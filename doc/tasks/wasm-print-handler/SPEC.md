# Feature: WASM Print Handler (Sprint 2)

## Overview

Port the terminal's print handler from TypeScript to Rust/WebAssembly. The `handle_print` and `flush_grapheme_buffer` functions execute entirely within WASM, eliminating JS-WASM boundary crossings for the hottest code path (80%+ of all terminal actions). Grapheme buffering, wrap-pending state, and character set translation also move to WASM. This is Sprint 2 of the WASM migration roadmap (`tmp/wasm.md`).

Additionally includes Sprint 1 carry-over items: `dispose()` method and `wasmRowToLine()` optimization.

## Objectives

- Move `handlePrintDispatch()` and `flushGraphemeBuffer()` logic to Rust/WASM
- Eliminate per-character JS-WASM boundary cost for charWidth/classifyCodepoint (now internal calls)
- Move graphemeBuffer, wrapPending, and charSet state to WASM TerminalCore
- Implement DEC Line Drawing character set translation in Rust
- Add `dispose()` method to WasmGrid for resource cleanup
- Optimize `wasmRowToLine()` to use `get_row_packed()` (1 WASM call instead of cols*5)

## User Stories

### US1: ASCII Character Printing via WASM

As a terminal handler, I want ASCII characters to be printed entirely within WASM, so that the most common print path has zero JS-WASM boundary overhead.

**Acceptance Criteria:**
- [ ] `core.handle_print(cp)` handles 0x20-0x7E codepoints with direct grid write
- [ ] Cursor advances correctly after each character
- [ ] Returns 0 when no scroll is needed
- [ ] Returns 1+ when scrollUp is required (wrapPending → LF → scroll)

### US2: Wide Character Printing

As a terminal handler, I want CJK and wide characters to be correctly handled in WASM, including width-2 cells and placeholder cells.

**Acceptance Criteria:**
- [ ] Wide characters (width=2) write to current cell and width=0 placeholder to next cell
- [ ] Wide characters at column cols-1 trigger line wrap (if autoWrap)
- [ ] `charWidth()` is called internally within WASM (no boundary crossing)

### US3: Emoji Grapheme Cluster Handling

As a terminal handler, I want emoji sequences (ZWJ, flags, skin tones) to be buffered and flushed correctly in WASM.

**Acceptance Criteria:**
- [ ] Extended_Pictographic codepoints start grapheme buffering
- [ ] ZWJ (0x200D), Variation Selectors, Skin Tone modifiers extend the buffer
- [ ] Regional Indicator pairs flush on the second RI
- [ ] Non-extending codepoints trigger automatic flush before processing
- [ ] `flush_grapheme_buffer()` returns scroll count
- [ ] Buffer safety limit at 64 codepoints

### US4: DEC Line Drawing Character Set

As a terminal handler, I want DEC Line Drawing characters to be translated within WASM.

**Acceptance Criteria:**
- [ ] G0/G1 charset state managed in WASM
- [ ] Characters 0x5F-0x7E translated to box-drawing Unicode characters when DecLineDrawing is active
- [ ] Charset selection API: `set_g0_charset()`, `set_g1_charset()`, `set_active_charset()`

### US5: WASM Resource Cleanup

As the terminal state manager, I want to properly free WASM TerminalCore resources.

**Acceptance Criteria:**
- [ ] `WasmGrid.dispose()` calls `TerminalCore.free()`
- [ ] Called on terminal close and buffer switch

### US6: Optimized Row-to-Line Conversion

As the scroll manager, I want `wasmRowToLine()` to use batch reading for better performance.

**Acceptance Criteria:**
- [ ] `wasmRowToLine()` uses `get_row_packed()` instead of per-cell WASM calls
- [ ] Output is identical to the current per-cell implementation
- [ ] Single WASM call per row instead of cols*5 calls

## Technical Requirements

### Functional Requirements

- **FR1:** `TerminalCore.handle_print(cp: u32) -> u8` processes a single codepoint in WASM and returns scroll count
- **FR2:** `TerminalCore.flush_grapheme_buffer() -> u8` flushes the grapheme buffer and returns scroll count
- **FR3:** Grapheme buffer (`Vec<u32>`) stored in TerminalCore, with 64-codepoint safety limit
- **FR4:** `wrap_pending: bool` stored in TerminalCore, managed by handle_print
- **FR5:** Character set state (`g0_charset`, `g1_charset`, `active_charset`) stored in TerminalCore
- **FR6:** DEC Line Drawing translation table implemented in Rust (32 entries, 0x5F-0x7E range)
- **FR7:** ASCII fast path: codepoints 0x20-0x7E with G0/Ascii charset, no wrapPending → direct grid write
- **FR8:** Slow path: charWidth (internal call), charSet translation, wrapPending handling, wide char wrap
- **FR9:** Scroll region (top, bottom) stored in TerminalCore for lineFeed boundary check
- **FR10:** `TerminalCore.get_wrap_pending() -> bool` and `TerminalCore.set_wrap_pending(v: bool)` for TS sync
- **FR11:** `TerminalCore.set_scroll_region(top: u16, bottom: u16)` for scroll region sync
- **FR12:** `WasmGrid.dispose()` calls `TerminalCore.free()`
- **FR13:** `wasmRowToLine()` refactored to use `get_row_packed()` binary parsing

### Non-Functional Requirements

- **NFR1 - Performance:** ASCII print < 200ns/char (WASM-internal)
- **NFR2 - Performance:** Character throughput >= 2x improvement over Sprint 1
- **NFR3 - Performance:** wasmRowToLine() <= 1 WASM call per row (vs cols*5 currently)
- **NFR4 - Compatibility:** All existing TypeScript tests pass (1822+)
- **NFR5 - Binary size:** WASM binary increase < 10KB over Sprint 1 baseline (39.5KB)
- **NFR6 - Compatibility:** JS fallback still works when WASM is unavailable

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ wasm/ (Rust Crate - extended from Sprint 1)     │
│                                                  │
│  src/terminal_core.rs (MODIFIED)                 │
│    + handle_print(cp: u32) -> u8                 │
│    + flush_grapheme_buffer() -> u8               │
│    + grapheme_buffer: Vec<u32>                   │
│    + wrap_pending: bool                          │
│    + g0_charset/g1_charset/active_charset        │
│    + scroll_region: (u16, u16)                   │
│    + handle_print_ascii() (internal)             │
│    + handle_print_slow() (internal)              │
│    + translate_line_drawing() (internal)          │
│                                                  │
│  src/unicode.rs (existing, unchanged)            │
│    - char_width()                                │
│    - classify_codepoint()                        │
│    - is_emoji_presentation()                     │
│                                                  │
│  src/cell.rs (existing, unchanged)               │
│  src/lib.rs (minor: new exports)                 │
└──────────────────────┬──────────────────────────┘
                       │ wasm_bindgen
                       ↓
┌─────────────────────────────────────────────────┐
│ src/terminal/ (TypeScript - MODIFIED)           │
│                                                  │
│  wasm/terminal-core.ts (MODIFIED)                │
│    + WasmGrid.dispose()                          │
│    + wasmRowToLine() rewritten (get_row_packed)  │
│                                                  │
│  handlers/print_handler.ts (MODIFIED)            │
│    - WASM path: core.handle_print(cp) → scroll   │
│    - JS fallback: existing handlePrintDispatch()  │
│                                                  │
│  state.ts (MODIFIED)                             │
│    - flushGraphemeBuffer() delegates to WASM      │
│    - processAction(): WASM flush before non-Print │
│    - charSet sync functions                       │
│                                                  │
│  unified-buffer.ts (MODIFIED)                    │
│    - setScrollRegion() syncs to WASM             │
└─────────────────────────────────────────────────┘
```

### Data Flow

**Print Path (WASM, normal case - no scroll):**
```
TS processAction("Print", char)
  → cp = char.codePointAt(0)
  → scroll_count = core.handle_print(cp)     [single WASM call]
  → scroll_count == 0 → done

  Inside WASM handle_print:
    → classify_codepoint(cp)                  [internal]
    → grapheme buffer check/extend
    → ASCII fast path OR slow path
    → char_width()                            [internal]
    → translate_line_drawing()                [internal]
    → grid[idx] = Cell { ... }               [direct memory]
    → cursor.col += width                    [direct field]
    → mark_row_dirty()                       [direct bitset]
    → return 0
```

**Print Path (WASM, with scroll):**
```
TS processAction("Print", char)
  → scroll_count = core.handle_print(cp)     [single WASM call]
  → scroll_count == 1
  → buffer.scrollUp()                         [TS: WASM row→JS Line, shift rows, clear]
```

**Non-Print Action (grapheme flush):**
```
TS processAction("Execute" | "Csi" | ...)
  → scroll_count = core.flush_grapheme_buffer()  [WASM]
  → for i in 0..scroll_count: buffer.scrollUp()
  → handle action
```

### WASM API Additions

```rust
#[wasm_bindgen]
impl TerminalCore {
    // ── Print handler (NEW) ──────────────────────────

    /// Handle a Print action for a single codepoint.
    /// Returns the number of scrollUp operations the TS side should perform.
    pub fn handle_print(&mut self, cp: u32) -> u8;

    /// Flush the grapheme cluster buffer.
    /// Returns the number of scrollUp operations.
    pub fn flush_grapheme_buffer(&mut self) -> u8;

    // ── Wrap-pending state (NEW) ─────────────────────

    pub fn get_wrap_pending(&self) -> bool;
    pub fn set_wrap_pending(&mut self, v: bool);

    // ── Character set state (NEW) ────────────────────

    /// charset: 0 = Ascii, 1 = DecLineDrawing
    pub fn set_g0_charset(&mut self, charset: u8);
    pub fn set_g1_charset(&mut self, charset: u8);
    /// active: 0 = G0, 1 = G1
    pub fn set_active_charset(&mut self, active: u8);
    pub fn get_g0_charset(&self) -> u8;
    pub fn get_g1_charset(&self) -> u8;
    pub fn get_active_charset(&self) -> u8;

    // ── Scroll region (NEW) ──────────────────────────

    pub fn set_scroll_region(&mut self, top: u16, bottom: u16);
    pub fn get_scroll_region_top(&self) -> u16;
    pub fn get_scroll_region_bottom(&self) -> u16;

    // ── Grapheme buffer query (NEW, for sync/debug) ──

    pub fn get_grapheme_buffer_len(&self) -> u32;
    pub fn clear_grapheme_buffer(&mut self);
}
```

### Internal Rust Implementation (non-exported)

```rust
impl TerminalCore {
    // ── Internal print functions ─────────────────────

    /// ASCII fast path: 0x20-0x7E, G0/Ascii, no wrap_pending
    fn handle_print_ascii(&mut self, byte: u8) -> u8;

    /// Slow path: charWidth, charSet translation, wrap handling
    fn handle_print_slow(&mut self, cp: u32) -> u8;

    /// Write a grapheme cluster to the grid at cursor position
    fn write_grapheme_to_grid(&mut self, chars: &str, width: u8) -> u8;

    /// Perform lineFeed: advance cursor row, return true if scroll needed
    fn line_feed(&mut self) -> bool;

    /// Perform carriageReturn: set cursor col to 0
    fn carriage_return(&mut self);

    // ── Character set translation ────────────────────

    /// Translate a codepoint using the active charset
    fn translate_charset(&self, cp: u32) -> u32;

    /// DEC Line Drawing translation table
    fn translate_line_drawing(cp: u32) -> u32;
}
```

### TerminalCore Struct Changes

```rust
#[wasm_bindgen]
pub struct TerminalCore {
    // Existing (Sprint 1)
    cols: u16,
    rows: u16,
    grid: Vec<Cell>,
    wrapped: Vec<bool>,
    dirty: Vec<u64>,
    cursor: CursorState,
    saved_cursor: Option<CursorState>,
    modes: u32,
    tab_stops: Vec<bool>,
    overflow: OverflowTable,

    // New (Sprint 2)
    grapheme_buffer: Vec<u32>,    // Codepoint buffer for emoji sequences
    wrap_pending: bool,            // Line-end wrap pending flag
    g0_charset: u8,                // 0=Ascii, 1=DecLineDrawing
    g1_charset: u8,                // 0=Ascii, 1=DecLineDrawing
    active_charset: u8,            // 0=G0, 1=G1
    scroll_region_top: u16,        // Scroll region top row (inclusive)
    scroll_region_bottom: u16,     // Scroll region bottom row (inclusive)
}
```

### TypeScript Changes

#### print_handler.ts

```typescript
// New WASM-aware dispatcher
export function handlePrint(state: TerminalStateAccessor, char: string): void {
  const cp = char.codePointAt(0);
  if (cp === undefined) return;

  if (state.wasmGrid) {
    const scrollCount = state.wasmGrid.core.handle_print(cp);
    const buffer = state.getActiveBuffer();
    for (let i = 0; i < scrollCount; i++) {
      buffer.scrollUp();
    }
  } else {
    handlePrintDispatch(state, char); // JS fallback
  }
}
```

#### state.ts - flushGraphemeBuffer

```typescript
flushGraphemeBuffer(): void {
  if (this.wasmGrid) {
    const scrollCount = this.wasmGrid.core.flush_grapheme_buffer();
    const buffer = this.getActiveBuffer();
    for (let i = 0; i < scrollCount; i++) {
      buffer.scrollUp();
    }
  } else {
    // existing JS implementation
  }
}
```

#### state.ts - charSet sync

```typescript
// When ESC handlers change charset (Sprint 3+ will move these to WASM)
set g0CharSet(cs: CharSet) {
  this._g0CharSet = cs;
  this.wasmGrid?.core.set_g0_charset(cs === "DecLineDrawing" ? 1 : 0);
}
// Similar for g1CharSet, activeCharSet
```

#### wasm/terminal-core.ts - wasmRowToLine optimization

```typescript
export function wasmRowToLine(core: TerminalCore, row: number): Line {
  const cols = core.cols();
  const line = new Line(cols);
  const packed = core.get_row_packed(row);

  let offset = 0;
  for (let col = 0; col < cols; col++) {
    // Parse packed binary format
    const charLen = packed[offset++];
    let ch: string;
    if (charLen === 0xFF) {
      // Overflow: u16 byte count + data
      const byteLen = (packed[offset] << 8) | packed[offset + 1];
      offset += 2;
      ch = new TextDecoder().decode(packed.subarray(offset, offset + byteLen));
      offset += byteLen;
    } else if (charLen === 0) {
      ch = " ";
    } else {
      ch = new TextDecoder().decode(packed.subarray(offset, offset + charLen));
      offset += charLen;
    }

    const width = packed[offset++];
    const fgTag = packed[offset++];
    const fgR = packed[offset++];
    const fgG = packed[offset++];
    const fgB = packed[offset++];
    const bgTag = packed[offset++];
    const bgR = packed[offset++];
    const bgG = packed[offset++];
    const bgB = packed[offset++];
    const flags = packed[offset] | (packed[offset + 1] << 8);
    offset += 2;

    const fg = unpackColor((fgTag << 24) | (fgR << 16) | (fgG << 8) | fgB);
    const bg = unpackColor((bgTag << 24) | (bgR << 16) | (bgG << 8) | bgB);
    const attrs: CellAttributes = { ...unpackStyleFlags(flags), fg, bg };
    line.setCell(col, { char: ch, width, attrs, dirty: false });
  }

  line.wrapped = core.get_line_wrapped(row);
  return line;
}
```

### DEC Line Drawing Translation Table (Rust)

```rust
/// Translate a codepoint using DEC Line Drawing character set.
/// Maps 0x5F-0x7E to box-drawing Unicode characters.
fn translate_line_drawing(cp: u32) -> u32 {
    match cp {
        0x5F => 0x0020,  // Blank
        0x60 => 0x25C6,  // Diamond
        0x61 => 0x2592,  // Checkerboard
        0x62 => 0x2409,  // HT symbol
        0x63 => 0x240C,  // FF symbol
        0x64 => 0x240D,  // CR symbol
        0x65 => 0x240A,  // LF symbol
        0x66 => 0x00B0,  // Degree
        0x67 => 0x00B1,  // Plus/minus
        0x68 => 0x2424,  // NL symbol
        0x69 => 0x240B,  // VT symbol
        0x6A => 0x2518,  // Lower right corner
        0x6B => 0x2510,  // Upper right corner
        0x6C => 0x250C,  // Upper left corner
        0x6D => 0x2514,  // Lower left corner
        0x6E => 0x253C,  // Crossing lines
        0x6F => 0x23BA,  // Horizontal line - scan 1
        0x70 => 0x23BB,  // Horizontal line - scan 3
        0x71 => 0x2500,  // Horizontal line - scan 5
        0x72 => 0x23BC,  // Horizontal line - scan 7
        0x73 => 0x23BD,  // Horizontal line - scan 9
        0x74 => 0x251C,  // Left tee
        0x75 => 0x2524,  // Right tee
        0x76 => 0x2534,  // Bottom tee
        0x77 => 0x252C,  // Top tee
        0x78 => 0x2502,  // Vertical line
        0x79 => 0x2264,  // Less than or equal
        0x7A => 0x2265,  // Greater than or equal
        0x7B => 0x03C0,  // Pi
        0x7C => 0x2260,  // Not equal
        0x7D => 0x00A3,  // UK pound
        0x7E => 0x00B7,  // Bullet
        _ => cp,
    }
}
```

### Grapheme Buffering Logic (Rust)

The grapheme buffering in `handle_print` follows the same logic as the current TypeScript implementation:

```rust
fn handle_print(&mut self, cp: u32) -> u8 {
    let mut scroll_count: u8 = 0;

    // Safety limit
    if self.grapheme_buffer.len() >= 64 {
        scroll_count += self.flush_grapheme_buffer();
    }

    let props = classify_codepoint(cp);

    if !self.grapheme_buffer.is_empty() {
        // Buffer non-empty: check if cp extends the cluster
        if cp == 0x200D {  // ZWJ
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }
        if props & VARIATION_SEL != 0 {
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }
        if props & SKIN_TONE != 0 {
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }
        if props & REGIONAL_IND != 0 {
            if self.grapheme_buffer.len() == 1 {
                let buf0 = self.grapheme_buffer[0];
                if (0x1F1E6..=0x1F1FF).contains(&buf0) {
                    // Second RI completes the flag pair
                    self.grapheme_buffer.push(cp);
                    scroll_count += self.flush_grapheme_buffer();
                    return scroll_count;
                }
            }
        }
        let last = *self.grapheme_buffer.last().unwrap();
        if last == 0x200D && (props & EXT_PICTOGRAPHIC != 0) {
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }
        if props & COMBINING != 0 {
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }

        // Does not extend: flush, then handle new cp
        scroll_count += self.flush_grapheme_buffer();
        // Fall through
    } else {
        // Buffer empty: check if cp starts buffering
        if props & (EXT_PICTOGRAPHIC | REGIONAL_IND) != 0 {
            self.grapheme_buffer.push(cp);
            return scroll_count;
        }
    }

    // ASCII fast path
    if cp >= 0x20 && cp < 0x7F
        && !self.wrap_pending
        && self.active_charset == 0  // G0
        && self.g0_charset == 0      // Ascii
    {
        scroll_count += self.handle_print_ascii(cp as u8);
    } else {
        scroll_count += self.handle_print_slow(cp);
    }

    scroll_count
}
```

### File Structure Changes

```
wasm/src/
├── lib.rs              # MODIFIED: new exports (handle_print, charset, scroll_region)
├── unicode.rs          # UNCHANGED (Sprint 0)
├── terminal_core.rs    # MODIFIED: +handle_print, +grapheme buffer, +charset, +scroll_region
└── cell.rs             # UNCHANGED (Sprint 1)

src/terminal/wasm/
├── loader.ts           # UNCHANGED
├── unicode.ts          # UNCHANGED
└── terminal-core.ts    # MODIFIED: +dispose(), +wasmRowToLine optimization

src/terminal/
├── state.ts            # MODIFIED: flushGraphemeBuffer delegates to WASM, charset sync
├── unified-buffer.ts   # MODIFIED: scroll region syncs to WASM
├── handlers/
│   └── print_handler.ts  # MODIFIED: WASM dispatch + JS fallback
└── (others unchanged)
```

### Dependencies

**No new crate dependencies.**

**No new npm dependencies.**

### Scroll Region Sync

The scroll region is set by CSI DECSTBM (handled in `csi_scrolling.ts`, Sprint 4 target). Until Sprint 4, the TS side syncs the scroll region to WASM whenever it changes:

```typescript
// In unified-buffer.ts setScrollRegion()
if (this.wasmGrid) {
  this.wasmGrid.core.set_scroll_region(top, bottom);
}
```

## Test Scenarios

### Unit Tests (Rust, `cargo test`)

- [ ] handle_print: ASCII 'A' at (0,0) → cell='A', cursor at (1,0), returns 0
- [ ] handle_print: ASCII at (cols-1,0) with autoWrap → wrap_pending=true, returns 0
- [ ] handle_print: ASCII with wrap_pending → CR+LF, print at (0,1), returns 0
- [ ] handle_print: ASCII at bottom row with wrap_pending → returns 1 (scroll needed)
- [ ] handle_print: CJK at (0,0) → width=2 cell + width=0 placeholder, cursor at (2,0)
- [ ] handle_print: CJK at (cols-1,0) with autoWrap → wrap to next line
- [ ] handle_print: Emoji (Extended_Pictographic) → buffered, not written
- [ ] handle_print: ZWJ after emoji → added to buffer
- [ ] handle_print: Non-extending cp after buffered emoji → flush + new cp
- [ ] handle_print: Regional Indicator pair → both buffered, auto-flush
- [ ] handle_print: Variation Selector FE0E → width 1
- [ ] handle_print: Variation Selector FE0F → width 2
- [ ] handle_print: Skin tone modifier → added to buffer
- [ ] handle_print: Buffer overflow (65 codepoints) → auto-flush at 64
- [ ] handle_print: DEC Line Drawing 'q' (0x71) → '─' (0x2500) when DecLineDrawing active
- [ ] handle_print: DEC Line Drawing inactive → no translation
- [ ] handle_print: G1 charset active with DecLineDrawing
- [ ] handle_print: autoWrap OFF at line end → cursor stays
- [ ] flush_grapheme_buffer: Empty buffer → returns 0
- [ ] flush_grapheme_buffer: Single emoji → width from EmojiPresentation check
- [ ] flush_grapheme_buffer: ZWJ sequence → width 2
- [ ] flush_grapheme_buffer: Flag (RI pair) → width 2
- [ ] flush_grapheme_buffer: With wrap_pending → CR+LF+write, returns scroll count
- [ ] scroll_region: LF within scroll region → no scroll past region bottom
- [ ] scroll_region: LF at region bottom → returns 1 (scroll within region)
- [ ] set_g0_charset/set_g1_charset: Getter returns set value
- [ ] set_active_charset: Switch between G0/G1
- [ ] get_wrap_pending/set_wrap_pending: Round-trip
- [ ] set_scroll_region: Getter returns set values

### Integration Tests (TypeScript, `bun test`)

- [ ] handlePrint WASM path: ASCII characters produce correct output
- [ ] handlePrint WASM path: CJK characters with correct width
- [ ] handlePrint WASM path: Emoji sequences render correctly
- [ ] handlePrint WASM path: DEC Line Drawing characters
- [ ] handlePrint WASM path: autoWrap scrolling works
- [ ] handlePrint JS fallback: Works when wasmGrid is null
- [ ] flushGraphemeBuffer WASM path: Flush before non-Print action
- [ ] wasmRowToLine: Optimized version produces identical output to per-cell version
- [ ] wasmRowToLine: Handles overflow characters correctly
- [ ] dispose(): WasmGrid resources freed without errors
- [ ] All existing print_handler tests pass (regression)

### Cross-Validation Tests

- [ ] For each existing print_handler.test.ts case: WASM and JS produce identical results
- [ ] DEC Line Drawing: All 32 entries match TS translateLineDrawing()
- [ ] Emoji sequences: TS grapheme buffer vs WASM buffer produce same output

### Edge Cases

- [ ] 1-column terminal: every character triggers wrap
- [ ] 1-row terminal with scroll region: immediate scroll on LF
- [ ] Empty string codepoint (should not happen, but guard)
- [ ] Codepoint 0 (NUL) passed to handle_print
- [ ] Very long grapheme cluster (64 codepoints, then flush)
- [ ] Rapid charset switches during print sequence
- [ ] wrap_pending followed by resize (should clear wrap_pending)

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| Invalid codepoint | cp > 0x10FFFF | Treat as replacement character U+FFFD |
| Invalid charset value | value not in {0, 1} | Treat as 0 (Ascii) |
| Invalid active_charset | value not in {0, 1} | Treat as 0 (G0) |
| Scroll region invalid | top >= bottom or >= rows | Reset to (0, rows-1) |
| WASM not initialized | wasmGrid is null | Fall back to JS implementation |

## Performance Optimization

### Performance Goals

- ASCII print throughput: >= 2x Sprint 1 baseline
- Emoji sequence throughput: >= 1.5x Sprint 1 baseline
- wasmRowToLine: >= 3x improvement (1 call vs cols*5 calls)

### Optimization Strategies

- **ASCII fast path in WASM:** Direct `grid[idx].char_data[0] = byte` without string allocation
- **Internal function calls:** `char_width()`, `classify_codepoint()`, `is_emoji_presentation()` called as Rust functions (no JS-WASM boundary)
- **Direct memory access:** Cursor, modes, grid accessed as struct fields (no getter/setter overhead)
- **Grapheme buffer in WASM:** No JS array allocation for emoji buffering
- **Batch row read:** `get_row_packed()` for wasmRowToLine reduces boundary crossings from cols*5 to 1

### Memory Budget (Sprint 2 additions)

| Component | Size |
|-----------|------|
| grapheme_buffer (Vec<u32>, cap 64) | 256 B |
| wrap_pending + charset state | 4 B |
| scroll_region | 4 B |
| DEC Line Drawing table (in code) | ~200 B |
| **Total additional** | **~464 B** |

## Success Criteria

- [ ] `wasm-pack build` succeeds with all Sprint 2 additions
- [ ] WASM binary size < 50KB total (Sprint 1: 39.5KB + Sprint 2 additions)
- [ ] All Rust unit tests pass (Sprint 1 tests + Sprint 2 new tests)
- [ ] All existing TypeScript tests pass (1822+)
- [ ] ASCII character throughput >= 2x Sprint 1 baseline
- [ ] Emoji sequences (ZWJ, flags, skin tones) render correctly
- [ ] DEC Line Drawing characters render correctly (vim borders, etc.)
- [ ] `dispose()` method works correctly
- [ ] `wasmRowToLine()` uses single WASM call per row
- [ ] `bun tauri dev` shows working terminal with WASM print handler
- [ ] vttest basic tests unchanged

## Implementation Phases

### Phase 1: Rust Print Core Logic
**Goals:** Implement handle_print, flush_grapheme_buffer, charSet, and scroll region in Rust; export all new functions via wasm_bindgen
**Deliverables:**
- TerminalCore struct extended with new fields
- handle_print() with ASCII fast path and slow path
- flush_grapheme_buffer() with width determination
- DEC Line Drawing translation table
- Scroll region management
- All new functions exported via wasm_bindgen
- `wasm-pack build` succeeds
- Rust unit tests for all new functions

### Phase 2: TypeScript Integration
**Goals:** Wire WASM print handler into existing TS pipeline
**Deliverables:**
- print_handler.ts WASM dispatch with JS fallback
- state.ts flushGraphemeBuffer WASM delegation
- state.ts charset sync (setter hooks)
- unified-buffer.ts scroll region sync

### Phase 3: Sprint 1 Carry-over Items
**Goals:** Implement dispose() and wasmRowToLine optimization
**Deliverables:**
- WasmGrid.dispose() method
- wasmRowToLine() rewritten with get_row_packed() parsing
- Performance validation for wasmRowToLine

### Phase 4: Verification and Benchmarking
**Goals:** Full regression test and performance measurement
**Deliverables:**
- All existing tests passing
- Cross-validation tests (WASM vs JS identical output)
- Character throughput benchmark
- wasmRowToLine performance comparison
- DEC Line Drawing validation (all 32 entries)

## References

- WASM roadmap: `tmp/wasm.md`
- Sprint 1 SPEC: `doc/tasks/wasm-terminal-core/SPEC.md`
- Current implementations:
  - `src/terminal/handlers/print_handler.ts` - Print handler (TS)
  - `src/terminal/state.ts` - TerminalState (flushGraphemeBuffer)
  - `src/terminal/wasm/terminal-core.ts` - WasmGrid adapter
  - `wasm/src/terminal_core.rs` - TerminalCore (Rust)
  - `wasm/src/unicode.rs` - char_width, classify_codepoint
