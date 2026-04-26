# Feature: WASM Slim Cell - Scrollback Memory Reduction

## Overview

Reduce per-cell memory footprint in the WASM scrollback region by introducing a `SlimCell` (target 8 bytes) that references styles via a `StyleTable` (style ID) and large graphemes via a unified `CharTable`. The current `Cell` struct (measured 34 bytes) remains in use for the active viewport; only cells evicted from the viewport into scrollback are compressed. Expected reduction: per-cell footprint approximately 76% (34B → 8B), translating to roughly 50% or more total scrollback memory reduction once table overhead is accounted for.

## Objectives

- Reduce per-scrollback-cell memory from 34 bytes (current `Cell`) to 8 bytes (`SlimCell`)
- Deduplicate repeated styles via `StyleTable` (id-based reference + refcount GC)
- Unify the existing `OverflowTable` with a `CharTable` keyed by content (id-based, refcount GC)
- Preserve existing WASM public API surface (`set_cell`, `get_cell_char`, packed binary format)
- Keep render latency degradation within 5% (p99) compared to current implementation
- Lay groundwork for future Phase 2 (viewport `SlimCell`) and Phase 3 (ASCII `TinyCell`)

## User Stories

### US1: Long-Running Claude Code Session

As a Claude Code heavy user running 8+ hour sessions, I want WebKitWebProcess RSS growth to stay flat even after thousands of lines of streamed output, so that my workstation does not slow down or get OOM-killed overnight.

**Acceptance Criteria:**
- [ ] Scrollback region stores cells as `SlimCell` (8 bytes)
- [ ] Memory usage of a fully populated 10,000-line × 200-column scrollback is at most 50% of the current implementation
- [ ] No visible behavior change in scrollback display, search, or copy

### US2: mux Multi-Window Memory Containment

As a mux multi-window user with 5+ windows open, I want the total terminal memory footprint to scale gracefully, so that each additional window adds modest memory overhead.

**Acceptance Criteria:**
- [ ] Per-window scrollback memory drops by ≥ 50% (table overhead amortized)
- [ ] mux mux-window count × per-window scrollback cost is significantly reduced

### US3: Scrollback Display, Selection, and Copy Integrity

As a terminal user, I want to scroll back, select text, and copy content from the scrollback region without any visible regression, so that the optimization is invisible to me.

**Acceptance Criteria:**
- [ ] Scrolling renders scrollback rows identically to the unoptimized version (chars, fg, bg, flags, underline, hyperlink)
- [ ] Mouse selection across viewport ↔ scrollback boundary works
- [ ] Clipboard copy preserves all styling information

### US4: Reflow with Rich Scrollback Content

As a user resizing the window after accumulating colored output, emoji, and hyperlinks in scrollback, I want reflow to preserve all visual information correctly, so that I do not lose past output.

**Acceptance Criteria:**
- [ ] Reflow preserves all `SlimCell` data, including style and char references
- [ ] StyleTable / CharTable refcounts are consistent after reflow (verified by rebuild + diff)
- [ ] ZWJ family emoji (e.g., `👨‍👩‍👧‍👦`, 25 bytes) survives reflow inside scrollback

## Technical Requirements

### Functional Requirements

- **FR1: SlimCell struct** — Define `SlimCell` with `#[repr(C)]` layout: `char_ref: u32` (4B), `width: u8` (1B), `flags: u8` (1B), `style_id: u16` (2B). Total: 8 bytes. Static assert via test (`assert_eq!(size_of::<SlimCell>(), 8)`).

- **FR2: SlimCell flags semantics** — `flags` byte encodes char storage mode and width hints. Bit layout: bit 0 = inline ASCII (`char_ref` is up to 4-byte UTF-8), bit 1 = CharTable ref (`char_ref` is `char_id`), bit 2 = wide-char continuation (right half of double-width), bits 3-7 reserved.

- **FR3: StyleTable** — `StyleTable` with operations `intern(StyleEntry) -> u16`, `get(id) -> &StyleEntry`, `inc_ref(id)`, `dec_ref(id)`. `StyleEntry` contains `fg: PackedColor`, `bg: PackedColor`, `flags: u16`, `underline_style: u8`, `underline_color: [u8; 3]`, `hyperlink_id: u16`. Internal storage is `Vec<StyleEntry>` indexed by id, plus `HashMap<StyleEntry, u16>` for dedup, plus `Vec<u32>` refcounts. `id = 0` is reserved for the default style (Cell::EMPTY equivalent) and is never decremented below 1.

- **FR4: CharTable** — `CharTable` with operations `intern(&str) -> u32`, `get(id) -> &str`, `inc_ref(id)`, `dec_ref(id)`. The existing `OverflowTable: HashMap<(u32, u32), String>` is replaced (within scrollback paths) by the id-keyed CharTable; viewport-side overflow continues to use the (col, row) keying for now to limit blast radius. Migration path documented in §"Migration".

- **FR5: Refcount GC** — On scrollback cell write, increment refcount of the new style/char and decrement the old one. When refcount reaches 0, the entry is freed (removed from dedup map; slot in storage Vec is added to a `free_list: Vec<u16/u32>` for reuse).

- **FR6: Scrollback compression on eviction** — When `ring_push_blank` evicts a row from viewport into scrollback (i.e., the row about to be overwritten in the ring is part of scrollback), the row's cells (currently `Cell`) are compressed into `SlimCell` at that moment. When `ring_push_blank` extends the ring (scrollback growing), compression happens for the new scrollback line as it slides off the viewport.

- **FR7: Scrollback decompression on read** — Read paths (rendering, selection, search, copy, packed-row generation for JS) call `slim_to_cell(&SlimCell, &StyleTable, &CharTable) -> Cell` to materialize a temporary `Cell` for downstream code. The conversion does not change refcounts.

- **FR8: Reflow integration** — Reflow temporarily decompresses scrollback `SlimCell` rows into `Cell` rows (per-row, not all at once), runs existing reflow logic on `Cell` rows, then re-compresses the resulting rows into `SlimCell`. After reflow completes, StyleTable / CharTable are rebuilt from the ring (full sweep) to guarantee refcount integrity.

- **FR9: Capacity saturation** — When StyleTable exceeds 65,535 entries, new style intern requests fall back to `id = 0` (default style). A `log::warn!` is emitted once per saturation event (rate-limited to once per N seconds to avoid log spam). When CharTable exceeds 4,294,967,295 entries (u32::MAX), the same fallback applies (treat as inline replacement char `?`). In practice neither is expected to occur.

- **FR10: Snapshot compatibility** — `snapshot.rs` serde format is bumped to a new version. The new format serializes `(SlimCell rows for scrollback, Cell rows for viewport, StyleTable, CharTable)`. The old format is read-only: when loading an old snapshot, all rows are deserialized as `Cell` rows in viewport-mode and scrollback is treated as empty (acceptable degradation since snapshots are short-lived).

- **FR11: Debug snapshot command** — Add `wasm_debug_slim_stats() -> JsValue` (wasm-bindgen export) returning `{ slim_cells, style_entries, style_bytes, char_entries, char_bytes }`. Used during development and verification.

### Non-Functional Requirements

- **NFR1 - Memory:** `size_of::<SlimCell>() == 8` (static assert via test). Total scrollback memory for a fully populated 10,000 × 200 grid is reduced by ≥ 50% versus the current `Cell`-based scrollback (including StyleTable / CharTable overhead).
- **NFR2 - Render latency:** Scroll-render p99 latency (10,000-line scrollback, 80 × 24 viewport) does not regress by more than 5% versus the current implementation. Measured via a Rust-side bench (`cargo bench`).
- **NFR3 - Compression latency:** Per-row Cell→SlimCell compression takes ≤ 50µs for a 200-cell row (typical Claude Code output) on the developer's reference machine.
- **NFR4 - Decompression latency:** Per-cell SlimCell→Cell decompression takes ≤ 200ns.
- **NFR5 - Reflow latency:** Full reflow (resize) on a fully populated 10,000 × 200 scrollback completes in ≤ 2× the time of the current implementation.
- **NFR6 - Compatibility:** WASM public API surface (`set_cell`, `get_cell_char`, `get_packed_row`, etc.) is unchanged. Packed binary format sent to JS is unchanged. TypeScript callers require zero changes.
- **NFR7 - Memory safety:** Implementation uses safe Rust only. No `unsafe` blocks added.
- **NFR8 - Test coverage:** All existing wasm tests pass. New unit tests cover SlimCell round-trip, StyleTable intern/dec, CharTable intern/dec, refcount GC, reflow with rich scrollback, and saturation fallback.

## Implementation Approach

### Architecture

**Component Diagram:**
```
┌────────────────────────────────────────────────────────────────┐
│ TerminalCore                                                    │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ring_cells: Vec<RingSlot>                                │  │
│  │   RingSlot = ViewportRow(Vec<Cell>) | ScrollbackRow(...) │  │
│  │              ScrollbackRow = Vec<SlimCell>               │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌────────────────────┐  ┌────────────────────┐                │
│  │ StyleTable         │  │ CharTable          │                │
│  │  - storage         │  │  - storage         │                │
│  │  - dedup map       │  │  - dedup map       │                │
│  │  - refcounts       │  │  - refcounts       │                │
│  │  - free_list       │  │  - free_list       │                │
│  └────────────────────┘  └────────────────────┘                │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ Compression bridge                                        │  │
│  │   cell_to_slim(&Cell) -> SlimCell                        │  │
│  │   slim_to_cell(&SlimCell) -> Cell                        │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

**Note on `RingSlot`:** The current `ring_cells: Vec<Cell>` is a flat array. We change it to a per-line discriminator: lines in the viewport region store `Cell`s; lines in scrollback store `SlimCell`s. Concretely, `ring_cells: Vec<Cell>` is replaced by two parallel structures or a per-line enum. The exact representation (Vec<enum> vs two Vecs vs a Vec<u8> backing buffer with indexing) is decided in §"Detailed Design".

### Data Flow

#### FR6/FR7: Compression and Decompression Lifecycle

```
PTY data → process_pty_data → set_cell on viewport row (Cell)
                              ↓ (eventually)
                        ring_push_blank
                              ↓
                  viewport row[0] slides to scrollback
                              ↓
                  cell_to_slim per cell (StyleTable.intern, CharTable.intern, refcount++)
                              ↓
                  scrollback row stored as Vec<SlimCell>

scroll-up render → for row in scrollback_visible_range:
                       for col in 0..cols:
                           cell = slim_to_cell(slim_row[col])
                       packed_row.push_cell(cell)
                       send to JS via existing format
```

#### FR5: Refcount GC

```
On overwriting a scrollback line (ring full):
   for slim_cell in old_row:
       StyleTable.dec_ref(slim_cell.style_id)
       if (slim_cell uses CharTable):
           CharTable.dec_ref(char_id)
   for slim_cell in new_row:
       StyleTable.inc_ref(...)  (already done by intern)
       CharTable.inc_ref(...)   (already done by intern)
```

#### FR8: Reflow

```
resize_reflow():
   1. Drain scrollback into Vec<Vec<Cell>> (decompress per-row)
   2. Drain viewport into Vec<Vec<Cell>>
   3. Run existing reflow logic on combined Vec<Vec<Cell>>
   4. Allocate new ring with new dimensions
   5. Push reflowed rows back:
        - viewport rows as Cell (no compression)
        - scrollback rows compressed to SlimCell (StyleTable/CharTable rebuilt from scratch)
```

### Dependencies

**Internal Dependencies:**
- `wasm/src/cell.rs` (Cell struct, OverflowTable) — extended with SlimCell, StyleEntry
- `wasm/src/ring_buffer.rs` — refactored to hold `RingSlot` per line
- `wasm/src/terminal_core.rs` — owns StyleTable, CharTable
- `wasm/src/reflow.rs` — invokes compress/decompress around existing logic
- `wasm/src/snapshot.rs` — bumps format version
- `wasm/src/terminal_cells.rs`, `wasm/src/terminal_rows.rs` — read/write paths use `slim_to_cell` for scrollback

**External Dependencies:**
- No new crate dependencies. `HashMap` from std is sufficient for dedup maps.

### File Structure

```
wasm/src/
├── cell.rs                # SlimCell, StyleEntry, StyleTable, CharTable (new), existing Cell unchanged
├── slim_cell.rs           # NEW: cell_to_slim, slim_to_cell, SlimCell tests
├── style_table.rs         # NEW: StyleTable impl + tests
├── char_table.rs          # NEW: CharTable impl + tests
├── ring_buffer.rs         # MODIFIED: RingSlot per line, compress on eviction
├── reflow.rs              # MODIFIED: decompress → reflow → recompress
├── terminal_core.rs       # MODIFIED: owns StyleTable, CharTable
├── terminal_cells.rs      # MODIFIED: scrollback read path uses slim_to_cell
├── terminal_rows.rs       # MODIFIED: get_packed_row materializes Cell from SlimCell
├── snapshot.rs            # MODIFIED: format version bump, new schema
└── lib.rs                 # MODIFIED: register new modules + wasm_debug_slim_stats export
```

## Detailed Design

### FR1, FR2: SlimCell

```rust
/// Compressed cell for scrollback storage. Exactly 8 bytes.
///
/// `char_ref` semantics depend on `flags`:
///   - flags & 0x01 (INLINE_ASCII): char_ref bytes are direct UTF-8 (1..=4 bytes)
///   - flags & 0x02 (CHAR_TABLE): char_ref is a CharTable id (u32)
///   - flags & 0x04 (WIDE_CONT): right half of a double-width cell (char_ref ignored)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SlimCell {
    pub char_ref: u32,
    pub width: u8,
    pub flags: u8,
    pub style_id: u16,
}

pub const SLIM_FLAG_INLINE_ASCII: u8 = 0x01;
pub const SLIM_FLAG_CHAR_TABLE:   u8 = 0x02;
pub const SLIM_FLAG_WIDE_CONT:    u8 = 0x04;

#[cfg(test)]
mod size_check {
    use super::SlimCell;
    #[test]
    fn slim_cell_is_8_bytes() {
        assert_eq!(std::mem::size_of::<SlimCell>(), 8);
    }
}
```

### FR3: StyleTable

```rust
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct StyleEntry {
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: u16,
    pub underline_style: u8,
    pub underline_color: [u8; 3],
    pub hyperlink_id: u16,
}

pub struct StyleTable {
    storage: Vec<StyleEntry>,         // indexed by id
    dedup:   HashMap<StyleEntry, u16>,
    refcount: Vec<u32>,               // parallel to storage
    free_list: Vec<u16>,
    saturated_warned_at: Option<Instant>, // rate-limit warn log
}

impl StyleTable {
    pub fn new() -> Self {
        let mut t = Self {
            storage: vec![StyleEntry::default()],
            dedup:   HashMap::from([(StyleEntry::default(), 0u16)]),
            refcount: vec![u32::MAX], // default style is permanent
            free_list: Vec::new(),
            saturated_warned_at: None,
        };
        t
    }

    pub fn intern(&mut self, entry: StyleEntry) -> u16 {
        if let Some(&id) = self.dedup.get(&entry) {
            self.refcount[id as usize] = self.refcount[id as usize].saturating_add(1);
            return id;
        }
        if let Some(id) = self.free_list.pop() {
            self.storage[id as usize] = entry;
            self.dedup.insert(entry, id);
            self.refcount[id as usize] = 1;
            return id;
        }
        if self.storage.len() >= u16::MAX as usize {
            self.warn_saturated();
            return 0; // fallback to default
        }
        let id = self.storage.len() as u16;
        self.storage.push(entry);
        self.refcount.push(1);
        self.dedup.insert(entry, id);
        id
    }

    pub fn dec_ref(&mut self, id: u16) {
        if id == 0 { return; } // default never freed
        let rc = &mut self.refcount[id as usize];
        debug_assert!(*rc > 0, "StyleTable refcount underflow at id {id}");
        *rc = rc.saturating_sub(1);
        if *rc == 0 {
            let entry = self.storage[id as usize];
            self.dedup.remove(&entry);
            self.free_list.push(id);
        }
    }

    pub fn get(&self, id: u16) -> &StyleEntry {
        &self.storage[id as usize]
    }
}
```

### FR4: CharTable

```rust
pub struct CharTable {
    storage: Vec<String>,
    dedup:   HashMap<String, u32>,
    refcount: Vec<u32>,
    free_list: Vec<u32>,
}

impl CharTable {
    pub fn new() -> Self {
        Self { storage: Vec::new(), dedup: HashMap::new(), refcount: Vec::new(), free_list: Vec::new() }
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.dedup.get(s) {
            self.refcount[id as usize] = self.refcount[id as usize].saturating_add(1);
            return id;
        }
        if let Some(id) = self.free_list.pop() {
            self.storage[id as usize] = s.to_owned();
            self.dedup.insert(s.to_owned(), id);
            self.refcount[id as usize] = 1;
            return id;
        }
        let id = self.storage.len() as u32;
        self.storage.push(s.to_owned());
        self.refcount.push(1);
        self.dedup.insert(s.to_owned(), id);
        id
    }

    pub fn dec_ref(&mut self, id: u32) {
        let rc = &mut self.refcount[id as usize];
        debug_assert!(*rc > 0, "CharTable refcount underflow at id {id}");
        *rc = rc.saturating_sub(1);
        if *rc == 0 {
            let s = std::mem::take(&mut self.storage[id as usize]);
            self.dedup.remove(&s);
            self.free_list.push(id);
        }
    }

    pub fn get(&self, id: u32) -> &str {
        &self.storage[id as usize]
    }
}
```

### Compression / Decompression Bridge

```rust
pub fn cell_to_slim(
    cell: &Cell,
    styles: &mut StyleTable,
    chars: &mut CharTable,
) -> SlimCell {
    // Style intern
    let style_id = styles.intern(StyleEntry {
        fg: cell.fg,
        bg: cell.bg,
        flags: cell.flags,
        underline_style: cell.underline_style,
        underline_color: cell.underline_color,
        hyperlink_id: cell.hyperlink_id,
    });

    // Char encoding
    let (char_ref, flags) = if cell.is_overflow() {
        // Inline overflow data was stored in the per-(col,row) overflow side table.
        // The caller is responsible for translating that to a CharTable id beforehand
        // (see ring_buffer compression code).
        unreachable!("caller must handle overflow translation")
    } else {
        let bytes = &cell.char_data[..cell.char_len as usize];
        if cell.char_len <= 4 {
            // Inline ASCII / short UTF-8: pack directly into char_ref (little-endian)
            let mut buf = [0u8; 4];
            buf[..bytes.len()].copy_from_slice(bytes);
            (u32::from_le_bytes(buf), SLIM_FLAG_INLINE_ASCII)
        } else {
            // Up to 16 bytes: needs CharTable
            let s = std::str::from_utf8(bytes).unwrap_or("?");
            (chars.intern(s), SLIM_FLAG_CHAR_TABLE)
        }
    };

    SlimCell { char_ref, width: cell.width, flags, style_id }
}

pub fn slim_to_cell(
    slim: &SlimCell,
    styles: &StyleTable,
    chars: &CharTable,
) -> Cell {
    let style = styles.get(slim.style_id);
    let mut cell = Cell::EMPTY;
    cell.width = slim.width;
    cell.fg = style.fg;
    cell.bg = style.bg;
    cell.flags = style.flags;
    cell.underline_style = style.underline_style;
    cell.underline_color = style.underline_color;
    cell.hyperlink_id = style.hyperlink_id;

    if slim.flags & SLIM_FLAG_INLINE_ASCII != 0 {
        let bytes = slim.char_ref.to_le_bytes();
        let len = bytes.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(1);
        cell.char_data[..len].copy_from_slice(&bytes[..len]);
        cell.char_len = len as u8;
    } else if slim.flags & SLIM_FLAG_CHAR_TABLE != 0 {
        let s = chars.get(slim.char_ref);
        let bytes = s.as_bytes();
        if bytes.len() <= 16 {
            cell.char_data[..bytes.len()].copy_from_slice(bytes);
            cell.char_len = bytes.len() as u8;
        } else {
            // Same convention as Cell::set_char overflow case.
            cell.char_len = 0xFF;
            // Caller must read the actual string from CharTable separately for >16-byte cases.
        }
    }
    cell
}
```

### RingBuffer Refactor

The current `ring_cells: Vec<Cell>` flat array is replaced with per-line storage that knows its mode:

```rust
enum RingRow {
    Viewport(Vec<Cell>),       // size == cols
    Scrollback(Vec<SlimCell>), // size == cols
}

pub struct RingBuffer {
    rows: Vec<RingRow>,        // length == ring_capacity
    head: usize,
    size: usize,
    cols: u16,
    visible_rows: u16,
}
```

Whenever the head/size pointer advances such that a previously-viewport row now belongs to scrollback, the row is rewritten in-place from `Viewport(Vec<Cell>)` to `Scrollback(Vec<SlimCell>)` via `cell_to_slim`. Conversely, when the viewport region grows back over a scrollback row (rare; only on certain reflows), the row is decompressed.

**Read-side accessors** (`get_cell_char`, `get_packed_row`, etc.) check the row mode and call `slim_to_cell` for scrollback rows. The decompressed `Cell` is returned by value (8 bytes of slim → 34 bytes of Cell on the stack — cheap).

### snapshot.rs

```rust
const SNAPSHOT_FORMAT_V1: u32 = 1; // legacy: all rows as Cell
const SNAPSHOT_FORMAT_V2: u32 = 2; // viewport: Cell, scrollback: SlimCell + StyleTable + CharTable

#[derive(Serialize, Deserialize)]
pub struct SnapshotV2 {
    pub format: u32, // = SNAPSHOT_FORMAT_V2
    pub viewport_rows: Vec<Vec<Cell>>,
    pub scrollback_rows: Vec<Vec<SlimCell>>,
    pub style_table: SerializedStyleTable,
    pub char_table:  SerializedCharTable,
    // ...other existing fields
}
```

When reading: detect `format` field; if V1, deserialize as legacy and place all rows in viewport mode (scrollback empty). If V2, deserialize natively.

### wasm_debug_slim_stats

```rust
#[wasm_bindgen]
pub fn wasm_debug_slim_stats() -> JsValue {
    let core = TERMINAL_CORE.lock();
    let stats = SlimStats {
        slim_cells: core.count_slim_cells(),
        style_entries: core.styles.live_entries(),
        style_bytes: core.styles.bytes_used(),
        char_entries: core.chars.live_entries(),
        char_bytes: core.chars.bytes_used(),
    };
    serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
}
```

## Test Scenarios

### Unit Tests
- [ ] FR1: `size_of::<SlimCell>() == 8`
- [ ] FR2: SlimCell flags decode correctly (inline ASCII / CharTable / wide cont)
- [ ] FR3: StyleTable intern returns same id for equal entries
- [ ] FR3: StyleTable id 0 is the default style and refcount never reaches 0
- [ ] FR3: StyleTable saturation (forge 65,535 unique styles) falls back to id 0 with a warn log
- [ ] FR4: CharTable intern returns same id for equal strings
- [ ] FR4: CharTable dec_ref to 0 frees the slot and `free_list` reuses it
- [ ] FR5: Refcount survives intern of same style 1000 times then dec_ref 1000 times → entry freed
- [ ] FR6: Cell→SlimCell→Cell round-trip preserves char (ASCII, CJK, emoji, ZWJ family ≤ 16 bytes)
- [ ] FR7: slim_to_cell does not change refcounts
- [ ] FR8: Reflow with rich scrollback (10 colors, 5 hyperlinks, 3 ZWJ family emoji) preserves all data
- [ ] FR8: After reflow, StyleTable + CharTable rebuilt-from-ring matches actual tables
- [ ] FR10: Snapshot V1 → load → save as V2 → reload round-trip
- [ ] FR11: wasm_debug_slim_stats returns non-null object with expected fields

### Integration Tests
- [ ] Eviction lifecycle: 100 lines pushed, then 10,000 more lines pushed → first 100 lines evicted, scrollback shows correct content
- [ ] Render parity: rendered packed row from a SlimCell-backed scrollback row matches a Cell-backed reference

### E2E Tests
**Existing E2E tests**: detected (`e2e-tests/specs/`, ran via `./scripts/run-e2e-docker.sh test`)
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] Scenario: scroll up through 5,000 lines of mixed colored output and verify rendering
- [ ] Scenario: select+copy across viewport↔scrollback boundary; clipboard contains correct text and styles

### Edge Cases
- [ ] Edge case: scrollback_lines = 0 → no SlimCell ever created, all rows always Viewport mode
- [ ] Edge case: 1,000 unique colors in scrollback → StyleTable holds 1,000 entries, no duplication
- [ ] Edge case: Same color used 1,000,000 times → StyleTable holds 1 entry with refcount 1,000,000
- [ ] Edge case: ZWJ family emoji (>16 bytes) in scrollback → CharTable handles it, slim flag = CHAR_TABLE
- [ ] Edge case: All-ASCII workload → CharTable mostly unused, all cells inline ASCII
- [ ] Edge case: refcount underflow → debug_assert in debug build, saturating sub in release

### Performance Tests
- [ ] Bench: 10,000 × 200 scrollback memory before/after (StyleTable / CharTable overhead included)
- [ ] Bench: scroll-render p50/p95/p99 before/after
- [ ] Bench: per-row Cell→SlimCell compression latency
- [ ] Bench: per-cell SlimCell→Cell decompression latency
- [ ] Bench: full reflow on 10,000 × 200 scrollback before/after

## Security Considerations

- **Input Validation:** SlimCell `style_id` and `char_ref` are validated by `StyleTable::get` / `CharTable::get` — out-of-range access panics in debug, falls back to default in release (via `get_or_default` wrapper).
- **Memory Safety:** All-safe Rust. No `unsafe` blocks added.
- **DoS Prevention:** StyleTable saturation (>65,535 unique styles) cannot be triggered by remote PTY data in any realistic scenario; in the worst case, it falls back gracefully and emits a rate-limited warn log. CharTable does not have a practical upper bound (u32::MAX ≈ 4B entries; OOM hits before this).
- **No XSS surface:** All changes are within WASM; no DOM/HTML interaction.

## Error Handling

### Error Cases

| Case | Trigger | Behavior |
|------|---------|----------|
| StyleTable saturated | >65,535 unique style entries | Fallback to id 0; rate-limited warn log |
| CharTable saturated | >u32::MAX entries (impossible in practice) | Fallback to inline `?`; warn log |
| refcount underflow | Internal bug | debug_assert (debug); saturating_sub (release) |
| SlimCell invalid style_id | Internal bug | get_or_default returns default style; warn log |
| SlimCell invalid char_ref (CharTable mode) | Internal bug | get_or_default returns " "; warn log |
| Snapshot V1 load | Loading old session | Treat all rows as viewport, scrollback empty |

### Error Flow

```
Internal invariant violation
   → debug_assert (debug builds: panic immediately for fast detection)
   → graceful fallback (release builds: log::warn, default value)
   → caller continues with default; user sees "?" or default style instead of correct content
```

## Performance Optimization

### Performance Goals
- Per-cell scrollback footprint: **8 bytes** (down from 34 bytes)
- Total scrollback memory: **≤ 50%** of current (10,000 × 200 grid, including table overhead)
- Scroll render p99 latency: **≤ 105%** of current
- Cell→SlimCell compression: **≤ 50µs** per 200-cell row
- SlimCell→Cell decompression: **≤ 200ns** per cell
- Reflow latency: **≤ 200%** of current

### Optimization Strategies
- **Style dedup via HashMap intern:** Real workloads have very few unique styles (typically < 100); StyleTable size stays small.
- **Inline ASCII fast path:** ASCII cells (vast majority) skip CharTable, store char directly in `char_ref`.
- **Lazy decompression:** SlimCell→Cell happens only on read; storage stays compressed.
- **Per-row compression at eviction:** Compression cost amortized across the lifetime of the row in scrollback.
- **free_list reuse:** Avoids growing storage Vec indefinitely as styles/chars are added and removed.

### Caching Strategy
- StyleTable and CharTable themselves are caches (intern-style). No additional caching layer.
- Decompressed `Cell` is returned by value, not cached, to avoid lifetime/aliasing complications.

## Migration

The transition from `Cell`-only scrollback to mixed `Cell`/`SlimCell` storage is implemented as a single atomic change in this PR. There is no flag to disable the optimization at runtime; if a regression is found, revert via git.

Snapshot format migration:
- Old snapshots (V1) load as viewport-only (scrollback discarded).
- New snapshots are saved as V2.
- A 1-release window where users could see "lost scrollback" on upgrade is acceptable since snapshots are short-lived (per-session).

## Success Criteria

- [ ] All functional requirements (FR1–FR11) are implemented
- [ ] All existing wasm tests pass without modification (except where they directly verified Cell layout in scrollback paths)
- [ ] All new unit tests pass
- [ ] All existing E2E tests pass
- [ ] Bench: scrollback memory reduction ≥ 50% on 10,000 × 200 grid
- [ ] Bench: scroll-render p99 regression ≤ 5%
- [ ] Bench: reflow latency ≤ 2× current
- [ ] `size_of::<SlimCell>() == 8` (asserted in test)
- [ ] Manual: 8-hour Claude Code session shows reduced RSS growth versus baseline
- [ ] Code review completed via `/user-code-review`

## Open Questions

> **Note**: Unresolved requirements are tracked with `status: tbd` in sdd.yaml.
> Resolve them before running `/sdd.2-create-plan`.

- [ ] FR3 saturation threshold — Real-world StyleTable size needs measurement during initial implementation; if 65,535 is too tight, switch to u32 ids (cost: SlimCell grows by 2 bytes → 10 bytes).
- [ ] FR10 snapshot — Whether to bump format version or to maintain dual-read with auto-conversion. Decided in Phase 1 implementation.

## Implementation Phases

### Phase 1: SlimCell + StyleTable + CharTable for scrollback (this task)
**Goals:** Reduce scrollback per-cell footprint to 8 bytes, achieve ≥ 50% scrollback memory reduction.
**Deliverables:**
- `wasm/src/slim_cell.rs`, `style_table.rs`, `char_table.rs` (new)
- `wasm/src/ring_buffer.rs` refactored to per-line storage
- `wasm/src/reflow.rs` updated to handle SlimCell rows
- `wasm/src/snapshot.rs` format V2
- `wasm_debug_slim_stats` debug export
- Bench harness and results documented

### Phase 2: SlimCell for active viewport (future task)
**Goals:** Apply SlimCell-style compression to viewport rows as well, further reducing baseline memory.
**Deliverables:** TBD in a future spec.

### Phase 3: ASCII TinyCell (future task)
**Goals:** ASCII-only super-compact 4-byte representation for the hot path.
**Deliverables:** TBD in a future spec.

## References

- Memory consumption investigation: `tmp/memory-alloc.md`
- Prior optimization task: `doc/tasks/wasm-optimization/SPEC.md`
- Current Cell struct: `wasm/src/cell.rs`
- Ring buffer: `wasm/src/ring_buffer.rs`
- Reflow: `wasm/src/reflow.rs`
- Snapshot serialization: `wasm/src/snapshot.rs`
- Project guidelines: `CLAUDE.md`
- Requirements document (Japanese): `doc/tasks/wasm-slim-cell/要件定義書.md`
